"""Private archival capture through the production coordinator; no restoration."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import stat
import subprocess
import time
import unittest
import uuid

import test_coordinator_recovery as recovery


class CoordinatorArchiveTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        self.operator = {"token": (self.f.state / "operator.key").read_text()}

    def cli(self, *args):
        return json.loads(self.f.cli(*args).stdout)

    def logical(self):
        return {t: self.f.rows("SELECT * FROM " + t + " ORDER BY 1") for t in
                ["meta", "actors", "routes", "messages", "tasks", "events", "sqlite_sequence", "message_dispositions", "schema_migrations"]}

    def raw(self, actor, op):
        _, frame = self.f.frame(actor, op)
        with self.f.connect() as sock:
            sock.sendall(frame)
            with sock.makefile("rb") as stream:
                return json.loads(stream.readline(524289))

    def test_archive_contains_wal_state_without_mutation_and_retry_keeps_original_snapshot(self):
        f = self.f
        op = f.send_op()
        self.assertTrue(f.call(f.a, op)["ok"])
        self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
        task = str(uuid.uuid4())
        self.assertTrue(f.call(f.a, {"type": "claim", "task": task, "ttl_seconds": 300})["ok"])
        self.assertTrue(f.call(f.a, {"type": "complete", "task": task, "generation": "1", "result": "private fixture"})["ok"])
        before = self.logical()
        archive_id = str(uuid.uuid4())
        receipt = self.cli("archive", "--archive-id", archive_id)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(self.logical(), before, "archive changed source logical state")
        bundle = Path(receipt["archive_dir"])
        self.assertEqual(bundle, f.state / "archives" / archive_id)
        self.assertEqual({p.name for p in bundle.iterdir()}, {"coordination.sqlite3", "manifest.json"})
        self.assertEqual(stat.S_IMODE(bundle.stat().st_mode), 0o700)
        for p in bundle.iterdir():
            self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o600)
        copied = bundle / "coordination.sqlite3"
        manifest = json.loads((bundle / "manifest.json").read_text())
        self.assertEqual(manifest["database_sha256"], hashlib.sha256(copied.read_bytes()).hexdigest())
        self.assertEqual(manifest["database_bytes"], str(copied.stat().st_size))
        with closing(sqlite3.connect(f"file:{copied}?mode=ro", uri=True)) as db:
            actual = {t: db.execute("SELECT * FROM " + t + " ORDER BY 1").fetchall() for t in before}
            self.assertEqual(actual, before, "archive omitted committed WAL rows or altered delivery")
            self.assertEqual(db.execute("PRAGMA quick_check").fetchone(), ("ok",))
        with self.assertRaises(sqlite3.ProgrammingError, msg="archive fixture leaked its read handle"):
            db.execute("SELECT 1")
        # Caller credentials are never copied as files or values into the snapshot/receipt.
        for token in [f.a["token"], f.b["token"], self.operator["token"]]:
            self.assertNotIn(token.encode(), copied.read_bytes())
            self.assertNotIn(token, json.dumps(receipt))
        old_hash = hashlib.sha256(copied.read_bytes()).hexdigest()
        self.assertTrue(f.call(f.b, {"type": "ack", "sender": f.a["actor_id"], "message": op["message"]["message_id"]})["ok"])
        f.restart()
        self.assertEqual(self.cli("archive", "--archive-id", archive_id), receipt)
        self.assertEqual(self.cli("inspect-archive", "--archive-id", archive_id), receipt)
        self.assertEqual(hashlib.sha256(copied.read_bytes()).hexdigest(), old_hash)
        self.assertEqual(f.rows("SELECT state FROM messages"), [("consumed",)])

    def test_lost_archive_reply_is_reconciled_by_exact_id_after_kill(self):
        archive_id = str(uuid.uuid4())
        self.f.lose_response(self.operator, {"type": "archive", "archive_id": archive_id})
        bundle = self.f.state / "archives" / archive_id
        deadline = time.monotonic() + 5
        while not bundle.is_dir():
            self.assertLess(time.monotonic(), deadline, "archive was not published")
            time.sleep(0.01)
        self.f.restart()
        receipt = self.cli("inspect-archive", "--archive-id", archive_id)
        self.assertEqual(self.cli("archive", "--archive-id", archive_id), receipt)
        self.assertEqual(len(list(bundle.parent.iterdir())), 1)
        self.assertEqual(receipt["archive_id"], archive_id)

    def test_archive_authority_corruption_and_capacity_fail_closed(self):
        archive_id = str(uuid.uuid4())
        op = {"type": "archive", "archive_id": archive_id}
        before = self.logical()
        for actor in [self.f.a, self.f.b, {"token": "0" * 64}]:
            for kind in ["archive", "inspect_archive"]:
                self.assertFalse(self.raw(actor, {**op, "type": kind})["ok"])
        for forged in [{**op, "path": str(self.f.root / "escape")},
                       {**op, "archive_id": "../escape"}, {"type": "archive"}]:
            self.assertFalse(self.raw(self.operator, forged)["ok"])
        self.assertFalse((self.f.state / "archives").exists(), "refused request created output")
        receipt = self.cli("archive", "--archive-id", archive_id)
        self.assertEqual(self.logical(), before)
        for actor in [self.f.a, self.f.b]:
            self.assertFalse(self.raw(actor, {"type": "inspect_archive", "archive_id": archive_id})["ok"])
        bundle = Path(receipt["archive_dir"])
        manifest = bundle / "manifest.json"
        manifest.write_text("{}")
        damaged = manifest.read_bytes()
        for command in ["archive", "inspect-archive"]:
            refused = self.f.cli(command, "--archive-id", archive_id, check=False)
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(refused.stdout, b"")
            self.assertEqual(manifest.read_bytes(), damaged, "corrupt bundle was overwritten")
        for i in range(7):
            (bundle.parent / ("retained-" + str(i))).mkdir(mode=0o700)
        refused = self.f.cli("archive", "--archive-id", str(uuid.uuid4()), check=False)
        self.assertNotEqual(refused.returncode, 0)
        self.assertEqual(len(list(bundle.parent.iterdir())), 8)
        self.assertEqual(self.logical(), before)

    def test_archive_retry_repeats_failed_parent_sync_but_inspection_stays_read_only(self):
        f = self.f
        fault = f.root / "sync-fault"
        fault.mkdir(mode=0o700)
        library = fault / "archive_fsync_fault.so"
        mode = fault / "mode"
        log = fault / "calls.log"
        for path in [mode, log]:
            path.touch(mode=0o600)
        mode.write_text("1")
        built = subprocess.run(
            ["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
             str(Path(__file__).parent / "fixtures" / "archive_fsync_fault.c"), "-ldl"],
            capture_output=True, timeout=30,
        )
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_FSYNC_TARGET=str(f.state / "archives"),
                   HT_TEST_FSYNC_MODE=str(mode), HT_TEST_FSYNC_LOG=str(log))

        def start_faulted():
            # Inject only into this owned coordinator, never the test runner or CLI.
            f.server = subprocess.Popen(
                [str(recovery.BINARY), "serve", "--state", str(f.state), "--workspace", str(f.root)],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env,
            )
            f.ready()

        f.start = start_faulted
        f.restart()
        before = self.logical()
        archive_id = str(uuid.uuid4())
        initial = f.cli("archive", "--archive-id", archive_id, check=False)
        self.assertNotEqual(initial.returncode, 0)
        self.assertEqual(initial.stdout, b"")
        bundle = f.state / "archives" / archive_id
        self.assertTrue(bundle.is_dir(), "fault must follow publication rename")
        self.assertEqual(log.read_text().splitlines(), ["fail"], "fsync fault did not fire")
        original = {p.name: (p.stat().st_ino, p.read_bytes()) for p in bundle.iterdir()}
        observed = self.cli("inspect-archive", "--archive-id", archive_id)
        self.assertEqual(observed["status"], "verified")
        self.assertEqual(log.read_text().splitlines(), ["fail"], "inspection attempted fsync")

        retry = f.cli("archive", "--archive-id", archive_id, check=False)
        self.assertNotEqual(retry.returncode, 0, "retry acknowledged unsynced archive root")
        self.assertEqual(retry.stdout, b"")
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail"])
        f.restart()
        retry = f.cli("archive", "--archive-id", archive_id, check=False)
        self.assertNotEqual(retry.returncode, 0, "restart lost the required publication sync")
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail", "fail"])
        self.assertEqual(self.logical(), before)

        mode.write_text("0")
        self.assertEqual(self.cli("archive", "--archive-id", archive_id), observed)
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail", "fail", "ok"])
        self.assertEqual({p.name: (p.stat().st_ino, p.read_bytes()) for p in bundle.iterdir()}, original)
        self.assertEqual(self.logical(), before)
        self.assertEqual(len(list(bundle.parent.iterdir())), 1, "retry created another capture")
        self.assertEqual(self.cli("inspect-archive", "--archive-id", archive_id), observed)
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail", "fail", "ok"])
        # Verify before re-syncing: corrupt bytes must never receive a success receipt.
        (bundle / "manifest.json").write_text("{}")
        self.assertNotEqual(f.cli("archive", "--archive-id", archive_id, check=False).returncode, 0)
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail", "fail", "ok"])


if __name__ == "__main__":
    unittest.main()
