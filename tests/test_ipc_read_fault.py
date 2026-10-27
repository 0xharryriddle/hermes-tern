"""Actual CLI/Unix endpoint read interruption: never resend a registration."""
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import time
import unittest

from fixture_process import cli_failure_diagnostic, wait_supervisor
import test_coordinator_recovery as recovery


class IpcReadFaultTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="ht-ipc-read-", dir=os.environ["TMPDIR"]))
        self.state = self.root / "state"
        self.library = self.root / "read-fault.so"
        self.log = self.root / "fault.log"
        self.log.touch(mode=0o600)
        self.server = None
        self.addCleanup(self.stop_and_inventory)
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror",
            "-o", str(self.library), str(recovery.ROOT / "tests/fixtures/ipc_read_fault.c"), "-ldl"],
            capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, "IPC read-fault fixture compilation failed")
        self.library.chmod(0o600)
        self.server = subprocess.Popen([str(recovery.BINARY), "serve", "--state", str(self.state),
            "--workspace", str(self.root)], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL)
        wait_supervisor(self.server, recovery.BINARY, self.state)

    def stop_and_inventory(self):
        if self.server is not None:
            if self.server.poll() is None:
                self.server.kill()
            self.server.wait(timeout=3)
        paths = [self.root, *self.root.rglob("*")]
        (self.root / "ownership.json").write_text(json.dumps({"retention":"retain;no deletion",
            "paths":[{"path":str(p),"inode":p.lstat().st_ino} for p in paths]}, indent=2)+"\n")

    def register(self, mode, target=True):
        endpoint = "hermes-tern-" + str(os.geteuid()) + "-" + hashlib.sha256(os.fsencode(self.state.resolve())).hexdigest()
        env = dict(os.environ, LD_PRELOAD=str(self.library), HT_TEST_IPC_FAULT=mode,
                   HT_TEST_IPC_TARGET=endpoint if target else endpoint+"-other", HT_TEST_IPC_LOG=str(self.log))
        # Each call owns one fresh intent. This helper has no retry loop.
        return subprocess.run([str(recovery.BINARY), "coord", "--state", str(self.state),
            "register", "--kind", "hermes", "--profile", "fixture", "--session", "fault-probe",
            "--model", "fixture", "--provider", "no-inference"], env=env, capture_output=True, timeout=6)

    def committed_once(self):
        deadline = time.monotonic()+3
        while True:
            with closing(sqlite3.connect((self.state / "coordination.sqlite3").as_uri()+"?mode=ro", uri=True)) as db:
                actors = db.execute("select count(*) from actors").fetchone()[0]
                events = db.execute("select count(*) from events where kind='actor.registered'").fetchone()[0]
            if (actors, events) == (1, 1):
                return
            self.assertLess(time.monotonic(), deadline, "registration not durably observed")
            time.sleep(0.01)

    def test_single_eintr_during_reply_resumes_same_read_and_commits_exactly_once(self):
        result = self.register("eintr")
        self.committed_once()
        self.assertEqual(self.log.read_text(), "injected\n", "fault did not reach the exact IPC socket")
        self.assertEqual(result.returncode, 0, "one interrupted read incorrectly loses a committed registration reply")
        self.assertEqual(set(json.loads(result.stdout)), {"actor_id", "incarnation", "credential_file"})

    def test_eintr_after_reply_prefix_preserves_buffer_and_one_registration(self):
        result = self.register("eintr-after-prefix")
        self.committed_once()
        self.assertEqual(self.log.read_text(), "injected\n")
        self.assertEqual(result.returncode, 0, "read interruption discarded an already received frame prefix")
        self.assertEqual(set(json.loads(result.stdout)), {"actor_id", "incarnation", "credential_file"})

    def test_noninterrupt_read_failure_stays_unknown_without_resubmission(self):
        result = self.register("reset")
        self.committed_once()
        self.assertEqual(self.log.read_text(), "injected\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        diagnostic = cli_failure_diagnostic(result, "register")
        self.assertEqual(diagnostic["category"], "io_connection_reset")
        self.assertTrue(diagnostic["outcome_unknown"])
        self.assertFalse(diagnostic["automatic_retry"])

    def test_wrong_endpoint_does_not_inject_into_unrelated_reads(self):
        result = self.register("reset", target=False)
        self.assertEqual(result.returncode, 0)
        self.committed_once()
        self.assertEqual(self.log.read_bytes(), b"")

    def test_repeated_eintr_cannot_extend_the_original_reply_deadline(self):
        start = time.monotonic()
        result = self.register("always-eintr")
        elapsed = time.monotonic()-start
        self.committed_once()
        self.assertEqual(self.log.read_text(), "injected\n")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(cli_failure_diagnostic(result, "register")["category"], "ipc_deadline")
        self.assertGreaterEqual(elapsed, 2.8)
        self.assertLess(elapsed, 5)


if __name__ == "__main__":
    unittest.main()
