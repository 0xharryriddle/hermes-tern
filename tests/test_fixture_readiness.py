"""Test fixture startup allowance, not product operation/expiry timing."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import test_native_chat as native


class FixtureReadinessTests(unittest.TestCase):
    def scratch(self):
        root = Path(tempfile.mkdtemp(prefix="ht-ready-test-", dir=os.environ["TMPDIR"]))
        def inventory():
            target = root / "ownership.json"
            target.touch(mode=0o600)
            target.write_text(json.dumps({"retention": "retain;no deletion", "paths": [
                {"path": str(p), "ino": p.lstat().st_ino, "dev": p.lstat().st_dev}
                for p in [root, *root.rglob("*")]]}, indent=2) + "\n")
        self.addCleanup(inventory)
        return root

    def child(self, code):
        child = subprocess.Popen([sys.executable, "-I", "-B", "-c", code],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL, cwd=self.scratch())
        def stop():
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)
        self.addCleanup(stop)
        return child

    def test_readiness_waits_for_delayed_positive_observation(self):
        from fixture_process import wait_ready
        child = self.child("import time;time.sleep(30)")
        start = time.monotonic()
        calls = []
        def probe(remaining):
            calls.append(remaining)
            return time.monotonic() - start >= 0.12
        elapsed = wait_ready(child, probe, timeout=1)
        self.assertGreaterEqual(elapsed, 0.12)
        self.assertGreater(len(calls), 1)
        self.assertTrue(all(0 < value <= 1 for value in calls))

    def test_exited_child_cannot_pass_an_old_positive_observation(self):
        from fixture_process import wait_ready
        child = self.child("raise SystemExit(7)")
        self.assertEqual(child.wait(timeout=3), 7)
        with self.assertRaisesRegex(AssertionError, "exited before readiness"):
            wait_ready(child, lambda remaining: True, timeout=1)

    def test_child_exiting_during_probe_cannot_pass(self):
        from fixture_process import wait_ready
        child = self.child("import time;time.sleep(30)")
        def probe(remaining):
            child.kill()
            child.wait(timeout=3)
            return True
        with self.assertRaisesRegex(AssertionError, "exited before readiness"):
            wait_ready(child, probe, timeout=1)

    def test_never_ready_and_late_ready_are_bounded_failures(self):
        from fixture_process import wait_ready
        child = self.child("import time;time.sleep(30)")
        start = time.monotonic()
        with self.assertRaisesRegex(AssertionError, "startup deadline"):
            wait_ready(child, lambda remaining: False, timeout=0.08)
        self.assertLess(time.monotonic() - start, 2)
        def late(remaining):
            time.sleep(remaining + 0.03)
            return True
        with self.assertRaisesRegex(AssertionError, "startup deadline"):
            wait_ready(child, late, timeout=0.08)
        for invalid in (0, -1, 16, float("inf"), float("nan"), True):
            with self.assertRaises(ValueError):
                wait_ready(child, lambda remaining: True, timeout=invalid)

    def test_native_fixture_accept_clock_survives_slow_owned_launcher(self):
        popen = subprocess.Popen
        def delayed(argv, *args, **kwargs):
            if argv[0] == str(native.BINARY):
                argv = [sys.executable, "-I", "-B", "-c",
                        "import os,sys,time;time.sleep(3.2);os.execv(sys.argv[1],sys.argv[1:])", *argv]
            return popen(argv, *args, **kwargs)
        with patch.object(native.subprocess, "Popen", side_effect=delayed):
            fixture = native.NativeChat()
        self.addCleanup(fixture.close)
        fixture.ready()
        self.assertFalse(fixture.gateway.methods("prompt.submit"))
        fixture.send(b"\x04")
        self.assertEqual(fixture.wait_exit(), 0)
        self.assertEqual(len(fixture.gateway.methods("session.detach")), 1)

    def status_binary(self, root, body):
        binary = root / "status-fixture"
        binary.write_text("#!" + sys.executable + "\nimport json,os,sys,time\nfrom pathlib import Path\n"
                          "state=Path(sys.argv[3])\n"
                          "with (state/'requests.jsonl').open('a') as out:\n"
                          " out.write(json.dumps({'pid':os.getpid(),'args':sys.argv[1:]})+'\\n')\n"
                          + body)
        binary.chmod(0o700)
        return binary

    def assert_status_probes_reaped(self, root):
        rows = [json.loads(line) for line in (root / "requests.jsonl").read_text().splitlines()]
        for row in rows:
            self.assertEqual(row["args"], ["coord", "--state", str(root), "status"])
            # They were direct children of this test runner; reaped means ECHILD.
            with self.assertRaises(ChildProcessError):
                os.waitpid(row["pid"], os.WNOHANG)
        return rows

    def test_supervisor_readiness_retries_only_status_after_probe_timeout(self):
        from fixture_process import wait_supervisor
        root = self.scratch()
        binary = self.status_binary(root, "if not (state/'first').exists():\n"
                                    " (state/'first').touch()\n time.sleep(30)\n")
        child = self.child("import time;time.sleep(30)")
        wait_supervisor(child, binary, root, timeout=2, probe_timeout=0.2)
        rows = self.assert_status_probes_reaped(root)
        self.assertEqual(len(rows), 2)
        self.assertIsNone(child.poll())

    def test_supervisor_exit_interrupts_a_pending_status_probe(self):
        from fixture_process import wait_supervisor
        root = self.scratch()
        binary = self.status_binary(root, "(state/'probe-started').touch()\ntime.sleep(30)\n")
        child = self.child("from pathlib import Path;import time\n"
                           "while not Path(" + repr(str(root / "probe-started")) + ").exists():time.sleep(.01)\n")
        started = time.monotonic()
        with self.assertRaisesRegex(AssertionError, "exited before readiness"):
            wait_supervisor(child, binary, root, timeout=3, probe_timeout=2)
        self.assertLess(time.monotonic() - started, 1.5, "waited for stalled probe after supervisor exit")
        self.assertEqual(len(self.assert_status_probes_reaped(root)), 1)

    def test_supervisor_status_retries_share_one_deadline_and_reap_last_probe(self):
        from fixture_process import wait_supervisor
        root = self.scratch()
        binary = self.status_binary(root, "time.sleep(30)\n")
        child = self.child("import time;time.sleep(30)")
        started = time.monotonic()
        with self.assertRaisesRegex(AssertionError, "startup deadline"):
            wait_supervisor(child, binary, root, timeout=0.55, probe_timeout=0.2)
        self.assertLess(time.monotonic() - started, 2)
        self.assertLessEqual(len(self.assert_status_probes_reaped(root)), 3)
        requests = (root / "requests.jsonl").read_bytes()
        for timeout in (0, -1, 16, float("inf"), float("nan"), True):
            with self.assertRaises(ValueError):
                wait_supervisor(child, binary, root, timeout=timeout)
        for probe_timeout in (0, -1, 7, float("inf"), float("nan"), True):
            with self.assertRaises(ValueError):
                wait_supervisor(child, binary, root, probe_timeout=probe_timeout)
        self.assertEqual((root / "requests.jsonl").read_bytes(), requests)


if __name__ == "__main__":
    unittest.main()
