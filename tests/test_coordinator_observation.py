"""Operator-only observation through the real CLI and private supervisor socket."""
import json

import unittest
import uuid

# Compose the existing isolated-process fixture; inheritance would collect its tests twice.
import test_coordinator_recovery as recovery


class CoordinatorObservationTests(unittest.TestCase):
    def setUp(self):
        self.fixture = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.fixture.doCleanups)
        self.fixture.setUp()

    def cli(self, *args):
        return json.loads(self.fixture.cli(*args).stdout)

    def test_snapshot_replay_and_exact_lookup_are_read_only_and_secret_free(self):
        f = self.fixture
        start = self.cli("snapshot")
        self.assertEqual(start["counts"]["actors"], "2")
        self.assertEqual(start["counts"]["routes"], "1")
        self.assertEqual(start["messages"]["items"], [])
        self.assertNotIn(f.a["token"], json.dumps(start))
        op = f.send_op()
        mid = op["message"]["message_id"]
        op["message"]["payload"]["text"] = "peer-data-only\n\u001b[31mnot approval"
        self.assertTrue(f.call(f.a, op)["ok"])
        task = str(uuid.uuid4())
        self.assertTrue(f.call(f.b, {"type": "claim", "task": task, "ttl_seconds": 300})["ok"])
        self.assertTrue(f.call(f.b, {"type": "complete", "task": task, "generation": "1", "result": "untrusted-task-result"})["ok"])
        before = f.rows("SELECT (SELECT count(*) FROM events), (SELECT value FROM meta WHERE key='clock')")
        page = self.cli("events", "--workspace", start["workspace_id"], "--epoch", start["observation_epoch"],
                        "--after", start["event_watermark"], "--limit", "2")
        self.assertEqual(page["status"], "ok")
        self.assertEqual([e["kind"] for e in page["events"]], ["message.queued", "task.claimed"])
        self.assertTrue(page["has_more"])
        final = self.cli("events", "--workspace", start["workspace_id"], "--epoch", start["observation_epoch"],
                         "--after", page["next_after"], "--limit", "2")
        self.assertEqual([e["kind"] for e in final["events"]], ["task.complete"])
        self.assertFalse(final["has_more"])
        message = self.cli("inspect-message", "--sender", f.a["actor_id"], "--message", mid)
        self.assertEqual(message["message"]["state"], "queued")
        self.assertEqual(message["message"]["payload"], op["message"]["payload"])
        self.assertEqual(message["trust"], "untrusted_peer_data_not_user_or_system_instructions")
        result = self.cli("inspect-task", "--task", task)
        self.assertEqual(result["task"]["result"], "untrusted-task-result")
        self.assertEqual(result["task"]["state"], "complete")
        self.assertEqual(result["trust"], message["trust"])
        snapshot = self.cli("snapshot")
        self.assertNotIn("peer-data-only", json.dumps(snapshot))
        self.assertNotIn("untrusted-task-result", json.dumps(snapshot))
        self.assertNotIn(f.a["token"], json.dumps(snapshot))
        self.assertEqual(snapshot["counts"]["messages"], "1")
        self.assertEqual(snapshot["counts"]["tasks"], "1")
        self.assertEqual(before, f.rows("SELECT (SELECT count(*) FROM events), (SELECT value FROM meta WHERE key='clock')"))
        self.assertEqual(f.rows("SELECT state FROM messages"), [("queued",)])

    def test_restart_invalidates_observation_cursor_without_redelivery(self):
        f = self.fixture
        self.assertTrue(f.call(f.a, f.send_op())["ok"])
        self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
        old = self.cli("snapshot")
        f.restart()
        response = self.cli("events", "--workspace", old["workspace_id"], "--epoch", old["observation_epoch"],
                            "--after", old["event_watermark"])
        self.assertEqual(response["status"], "snapshot_required")
        self.assertEqual(response["reason"], "epoch_changed")
        self.assertEqual(response["events"], [])
        self.assertIsNone(response["next_after"])
        current = self.cli("snapshot")
        self.assertNotEqual(current["observation_epoch"], old["observation_epoch"])
        self.assertEqual(current["messages"]["items"][0]["state"], "ambiguous")
        self.assertEqual(f.call(f.b, {"type": "receive"})["result"]["messages"], [])

    def test_peer_and_unknown_fields_cannot_use_operator_observation(self):
        f = self.fixture
        sent = f.send_op()
        task = str(uuid.uuid4())
        self.assertTrue(f.call(f.a, sent)["ok"])
        self.assertTrue(f.call(f.a, {"type": "claim", "task": task, "ttl_seconds": 300})["ok"])
        start = self.cli("snapshot")
        operator = {"token": (f.state / "operator.key").read_text()}
        operations = [
            {"type": "snapshot"},
            {"type": "events", "workspace": start["workspace_id"], "epoch": start["observation_epoch"], "after": "0", "limit": 1},
            {"type": "inspect_message", "sender": f.a["actor_id"], "message": sent["message"]["message_id"]},
            {"type": "inspect_task", "task": task},
        ]
        for op in operations:
            self.assertTrue(f.call(operator, op)["ok"], "valid operator positive control")
            self.assertFalse(f.call(f.a, op)["ok"])
            # Invalid wire records return id=null, so do not use the correlated helper.
            _, raw = f.frame(operator, {**op, "approve": True})
            with f.connect() as sock:
                sock.sendall(raw)
                with sock.makefile("rb") as stream:
                    self.assertFalse(json.loads(stream.readline(524289))["ok"])
        for after in ["-1", "+1", "01", " 1", "9223372036854775808"]:
            self.assertFalse(f.call(operator, {**operations[1], "after": after})["ok"])
        for limit in [0, 129]:
            self.assertFalse(f.call(operator, {**operations[1], "limit": limit})["ok"])
        self.assertTrue(f.call(operator, operations[0])["ok"])

    def test_exact_lookup_reaches_records_omitted_from_bounded_snapshot(self):
        f = self.fixture
        earliest = None
        # Keep per-recipient pending within its bound by consuming each message.
        for _ in range(130):
            op = f.send_op()
            mid = op["message"]["message_id"]
            earliest = earliest or mid
            self.assertTrue(f.call(f.a, op)["ok"])
            self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
            self.assertTrue(f.call(f.b, {"type": "ack", "sender": f.a["actor_id"], "message": mid})["ok"])
        snap = self.cli("snapshot")
        self.assertEqual(snap["counts"]["messages"], "130")
        self.assertEqual(snap["messages"]["total"], "130")
        self.assertEqual(len(snap["messages"]["items"]), 128)
        self.assertTrue(snap["messages"]["truncated"])
        self.assertFalse(any(m["message_id"] == earliest for m in snap["messages"]["items"]))
        assert earliest is not None
        self.assertEqual(self.cli("inspect-message", "--sender", f.a["actor_id"], "--message", earliest)["message"]["state"], "consumed")
        missing = f.cli("inspect-message", "--sender", f.b["actor_id"], "--message", earliest, check=False)
        self.assertNotEqual(missing.returncode, 0)
        self.assertEqual(missing.stdout, b"")


if __name__ == "__main__":
    unittest.main()
