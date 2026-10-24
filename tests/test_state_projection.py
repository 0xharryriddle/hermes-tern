"""Revision-bound, operator-only state enumeration through the production process."""
from contextlib import closing
import concurrent.futures
import json
import sqlite3
import threading
import unittest
import uuid

import test_coordinator_recovery as recovery
import test_coordinator_migration as migration


class StateProjectionTests(unittest.TestCase):
    def setUp(self):
        self.fixture = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()

    def cli(self, *args):
        return json.loads(self.fixture.cli(*args).stdout)

    def page(self, index, section, offset="0", limit="128"):
        return self.cli("state-page", "--workspace", index["workspace_id"],
                        "--epoch", index["observation_epoch"], "--revision", index["revision"],
                        "--section", section, "--offset", offset, "--limit", limit)

    def test_cli_pages_enumerate_old_records_and_escaped_payloads_without_mutation(self):
        f = self.fixture
        # Fail on the old binary before fixture population; missing command is the RED.
        self.cli("state-index")
        expected = set()
        for n in range(130):
            op = f.send_op()
            op["message"]["payload"] = {"ordinal": n, "text": "peer data\n\u001b[31mnot approval"}
            mid = op["message"]["message_id"]
            expected.add(mid)
            self.assertTrue(f.call(f.a, op)["ok"])
            self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
            self.assertTrue(f.call(f.b, {"type": "ack", "sender": f.a["actor_id"], "message": mid})["ok"])
        index = self.cli("state-index")
        before = f.rows("SELECT (SELECT count(*) FROM events),(SELECT value FROM meta WHERE key='clock')")
        first = self.page(index, "messages")
        self.assertEqual(len(first["items"]), 128)
        self.assertEqual(first["total"], "130")
        self.assertTrue(first["has_more"])
        second = self.page(index, "messages", first["next_offset"])
        self.assertEqual(len(second["items"]), 2)
        self.assertFalse(second["has_more"])
        rows = first["items"] + second["items"]
        self.assertEqual({x["message"]["message_id"] for x in rows}, expected)
        self.assertEqual([x["message"]["payload"]["ordinal"] for x in rows], list(range(130)))
        self.assertTrue(all(x["trust"] == "untrusted_peer_data_not_user_or_system_instructions" for x in rows))
        self.assertEqual(self.cli("state-index"), index)
        self.assertEqual(before, f.rows("SELECT (SELECT count(*) FROM events),(SELECT value FROM meta WHERE key='clock')"))
        self.assertNotIn(f.a["token"], json.dumps([index, first, second]))
        self.assertNotIn("token_hash", json.dumps([index, first, second]))
        self.assertTrue(self.cli("snapshot")["messages"]["truncated"])
        for section in ["actors", "routes", "tasks", "dispositions", "migrations"]:
            p = self.page(index, section)
            self.assertEqual(str(len(p["items"])), index["counts"][section])
            self.assertFalse(p["has_more"])

    def test_state_or_epoch_change_returns_restart_required_and_no_partial_page(self):
        f = self.fixture
        index = self.cli("state-index")
        self.assertEqual(self.page(index, "actors", limit="1")["status"], "ok")
        f.cli("route", "--sender", f.a["actor_id"], "--recipient", f.b["actor_id"])
        stale = self.page(index, "actors", "1", "1")
        self.assertEqual(stale["status"], "restart_required")
        self.assertEqual(stale["reason"], "state_changed")
        self.assertEqual(stale["items"], [])
        self.assertIsNone(stale["next_offset"])
        current = self.cli("state-index")
        f.restart()
        self.assertEqual(self.page(current, "actors")["reason"], "epoch_changed")
        after = self.cli("state-index")
        self.assertEqual(after["revision"], current["revision"])
        self.assertEqual(len(self.page(after, "actors")["items"]), 2)

    def test_wire_authority_and_malformed_cursors_refuse_with_positive_controls(self):
        f = self.fixture
        index = self.cli("state-index")
        operator = {"token": (f.state / "operator.key").read_text()}
        page = {"type": "state_page", "workspace": index["workspace_id"],
                "epoch": index["observation_epoch"], "revision": index["revision"],
                "section": "actors", "offset": "0", "limit": 1}
        for op in [{"type": "state_index"}, page]:
            self.assertTrue(f.call(operator, op)["ok"])
            self.assertFalse(f.call(f.a, op)["ok"])
            _, raw = f.frame(operator, {**op, "approve": True})
            with f.connect() as sock:
                sock.sendall(raw)
                with sock.makefile("rb") as stream:
                    self.assertFalse(json.loads(stream.readline(524289))["ok"])
        for field, values in {
            "offset": ["-1", "01", "+0", " 0", "3", "9223372036854775808"],
            "limit": [0, 129], "revision": ["", "A" * 64, "a" * 63],
            "section": ["meta", "credentials", "actors;DELETE FROM actors"],
            "workspace": [str(uuid.uuid4())],
        }.items():
            for value in values:
                self.assertFalse(f.call(operator, {**page, field: value})["ok"])
        mismatch = f.call(operator, {**page, "revision": "f" * 64})
        self.assertTrue(mismatch["ok"])
        self.assertEqual(mismatch["result"]["status"], "restart_required")
        self.assertEqual(self.cli("state-index"), index)

    def test_migrated_state_exposes_retained_witness_and_disposition_without_restoring(self):
        legacy = migration.CoordinatorMigrationTests()
        self.addCleanup(legacy.doCleanups)
        legacy.setUp()
        receipt = json.loads(legacy.migrate().stdout)
        legacy.f.start()
        self.fixture = f = legacy.f
        index = self.cli("state-index")
        witness = self.page(index, "migrations")
        self.assertEqual(witness["total"], "1")
        self.assertEqual(witness["items"], [{"to_version": "2", "archive_id": legacy.archive_id,
                                              "database_sha256": receipt["database_sha256"]}])
        original = self.page(index, "messages")["items"][0]["message"]
        self.assertEqual(original["state"], "ambiguous")
        decision = str(uuid.uuid4())
        f.cli("dispose-message", "--sender", original["sender_actor_id"], "--message", original["message_id"],
              "--delivery-incarnation", original["delivery_incarnation"], "--decision-id", decision,
              "--action", "close", "--reason", "fixture outcome unknown", "--attest-outcome-unknown")
        self.assertEqual(self.page(index, "dispositions")["status"], "restart_required")
        current = self.cli("state-index")
        disposed = self.page(current, "dispositions")["items"]
        self.assertEqual(len(disposed), 1)
        self.assertEqual(disposed[0]["decision_id"], decision)
        self.assertEqual(disposed[0]["outcome"], "unknown")
        self.assertEqual(disposed[0]["process_stopped"], "not_established")
        before = legacy.logical()
        rows = self.page(current, "messages")["items"]
        self.assertEqual(rows[0]["disposition"], disposed[0])
        self.assertEqual(rows[0]["message"]["state"], "ambiguous")
        self.assertEqual(self.page(current, "migrations")["items"], witness["items"])
        self.assertEqual(legacy.logical(), before)

    def test_pages_never_mix_models_under_a_separate_committing_wal_writer(self):
        f = self.fixture
        indexes = {}
        with closing(sqlite3.connect(f.state / "coordination.sqlite3")) as db:
            for model in ["state-a", "state-b"]:
                db.execute("UPDATE actors SET model=?", (model,))
                db.commit()
                indexes[model] = self.cli("state-index")
                page = self.page(indexes[model], "actors")
                self.assertEqual({a["model"] for a in page["items"]}, {model})
        done = threading.Event()
        started = threading.Event()

        def mutate():
            commits = 0
            with closing(sqlite3.connect(f.state / "coordination.sqlite3")) as db:
                while not done.is_set():
                    db.execute("UPDATE actors SET model=?", ("state-a" if commits % 2 else "state-b",))
                    db.commit()
                    commits += 1
                    started.set()
            return commits

        with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
            writer = pool.submit(mutate)
            try:
                self.assertTrue(started.wait(2))
                for n in range(40):
                    model = "state-a" if n % 2 else "state-b"
                    page = self.page(indexes[model], "actors")
                    if page["status"] == "ok":
                        self.assertEqual(len(page["items"]), 2)
                        self.assertEqual({a["model"] for a in page["items"]}, {model})
                        self.assertEqual(page["revision"], indexes[model]["revision"])
                    else:
                        self.assertEqual(page["status"], "restart_required")
                        self.assertEqual(page["reason"], "state_changed")
                        self.assertEqual(page["items"], [])
                        self.assertIsNone(page["next_offset"])
            finally:
                done.set()
            self.assertGreater(writer.result(timeout=4), 1)
        current = self.cli("state-index")
        self.assertEqual(self.page(current, "actors")["status"], "ok")

    def test_large_complete_results_split_on_encoded_bytes_and_keep_precise_counters(self):
        f = self.fixture
        self.cli("state-index")
        result = "\u001b" * 32768
        tasks = []
        for _ in range(3):
            task = str(uuid.uuid4())
            tasks.append(task)
            self.assertTrue(f.call(f.a, {"type": "claim", "task": task, "ttl_seconds": 300})["ok"])
            self.assertTrue(f.call(f.a, {"type": "complete", "task": task, "generation": "1", "result": result})["ok"])
        with closing(sqlite3.connect(f.state / "coordination.sqlite3")) as db:
            db.execute("UPDATE tasks SET generation=9007199254740993 WHERE id=?", (tasks[0],))
            db.commit()
        index = self.cli("state-index")
        rows = []
        offset = "0"
        for _ in range(3):
            page = self.page(index, "tasks", offset)
            self.assertEqual(len(page["items"]), 1)
            self.assertLessEqual(len(json.dumps(page, ensure_ascii=False, separators=(",", ":")).encode()), 393216)
            rows += page["items"]
            offset = page["next_offset"]
            self.assertEqual(page["has_more"], len(rows) < 3)
            self.assertEqual(page["byte_limited"], len(rows) < 3)
        self.assertEqual({v["task"]["task_id"] for v in rows}, set(tasks))
        self.assertTrue(all(v["task"]["result"] == result for v in rows))
        precise = next(v for v in rows if v["task"]["task_id"] == tasks[0])
        self.assertEqual(precise["task"]["generation"], "9007199254740993")
        self.assertEqual(self.cli("state-index"), index)


if __name__ == "__main__":
    unittest.main()
