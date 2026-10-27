"""Closed fixture diagnostics: no raw errors, credentials or mutation retries."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import fixture_process
import test_coordinator_recovery as recovery


class IpcDiagnosticsTests(unittest.TestCase):
    def root(self):
        root = Path(tempfile.mkdtemp(prefix="ht-ipc-diagnostics-", dir=os.environ["TMPDIR"]))
        self.addCleanup(lambda: (root / "ownership.json").write_text(json.dumps({
            "retention": "retain; no deletion", "paths": [str(root), *map(str, root.rglob("*"))],
        }, indent=2) + "\n"))
        return root

    def test_only_complete_exact_diagnostics_are_classified_and_tainted_near_matches_are_not_echoed(self):
        cases = {
            b"hermes-tern: local I/O error (operation interrupted)\n": "io_interrupted",
            b"hermes-tern: local I/O error (operation would block)\n": "io_would_block",
            b"hermes-tern: local I/O error (timed out)\n": "io_timed_out",
            b"hermes-tern: local I/O error (connection reset)\n": "io_connection_reset",
            b"hermes-tern: protocol error: partial IPC frame at EOF\n": "ipc_partial_eof",
            b"hermes-tern: protocol error: IPC response identity mismatch\n": "ipc_identity_mismatch",
            b"hermes-tern: IPC frame timed out; submitted work may have an unknown outcome\n": "ipc_deadline",
        }
        for raw, expected in cases.items():
            result = subprocess.CompletedProcess(["not-logged"], 1, b"PRIVATE stdout", raw)
            report = fixture_process.cli_failure_diagnostic(result, "register")
            self.assertEqual(report["category"], expected)
            self.assertTrue(report["outcome_unknown"])
            self.assertFalse(report["automatic_retry"])
            self.assertNotIn("PRIVATE", json.dumps(report))
            for bad in (raw+b"PRIVATE secret", b"PRIVATE prefix"+raw, raw.rstrip(b"\n")+b"\x1b[31m\n", raw+b"\n", b"x"*100000):
                report = fixture_process.cli_failure_diagnostic(subprocess.CompletedProcess([], 1, b"PRIVATE", bad), "PRIVATE command")
                self.assertEqual(report["category"], "diagnostic_unavailable")
                self.assertEqual(report["operation"], "unclassified")
                self.assertLessEqual(report["stderr_bytes"], 65536)
                self.assertNotIn("PRIVATE", json.dumps(report))

    def test_real_failed_fixture_process_is_called_once_with_safe_retained_receipt(self):
        root = self.root()
        fake = root / "cli"
        count = root / "calls"
        fake.write_text("#!"+sys.executable+"\nfrom pathlib import Path\nimport sys\np=Path(__file__).with_name('calls')\nwith p.open('a') as out:out.write('called\\n')\nsys.stdout.write('PRIVATE stdout')\nsys.stderr.write('hermes-tern: local I/O error (operation interrupted)\\n')\nraise SystemExit(1)\n")
        fake.chmod(0o700)
        fixture = recovery.CoordinatorRecoveryTests()
        fixture.root = root
        fixture.state = root / "state"
        with patch.object(recovery, "BINARY", fake):
            with self.assertRaises(AssertionError) as caught:
                fixture.cli("register", "--session", "PRIVATE arg")
        self.assertEqual(count.read_text().splitlines(), ["called"])
        self.assertIn("io_interrupted", str(caught.exception))
        self.assertNotIn("PRIVATE", str(caught.exception))
        reports = list(root.glob("ipc-failure-*.json"))
        self.assertEqual(len(reports), 1)
        report = json.loads(reports[0].read_text())
        self.assertEqual(report["category"], "io_interrupted")
        self.assertEqual(report["operation"], "register")
        self.assertTrue(report["outcome_unknown"])
        self.assertFalse(report["automatic_retry"])
        self.assertNotIn("PRIVATE", reports[0].read_text())
        self.assertEqual(reports[0].stat().st_mode & 0o777, 0o600)

    def test_unknown_diagnostics_and_receipt_capacity_do_not_mask_or_retry_original_failure(self):
        root = self.root()
        fixture = recovery.CoordinatorRecoveryTests()
        fixture.root = root
        fixture.state = root / "state"
        failed = subprocess.CompletedProcess(["PRIVATE invocation"], 7, b"PRIVATE response", b"PRIVATE error\x1b[31m")
        for index in range(32):
            (root / f"ipc-failure-{index+1:02}.json").write_text("retained\n")
        with patch.object(recovery.subprocess, "run", return_value=failed) as run:
            with self.assertRaises(AssertionError) as caught:
                fixture.cli("register", "--session", "PRIVATE")
        self.assertEqual(run.call_count, 1)
        self.assertIn("diagnostic_unavailable", str(caught.exception))
        self.assertNotIn("PRIVATE", str(caught.exception))
        self.assertEqual([p.read_text() for p in root.glob("ipc-failure-*.json")], ["retained\n"]*32)

    def test_recording_failure_does_not_override_the_original_cli_failure(self):
        root = self.root()
        fixture = recovery.CoordinatorRecoveryTests()
        fixture.root = root
        fixture.state = root / "state"
        result = subprocess.CompletedProcess([], 1, b"", b"hermes-tern: local I/O error (connection reset)\n")
        with patch.object(recovery.subprocess, "run", return_value=result) as run:
            with patch.object(fixture_process, "record_cli_failure", side_effect=OSError("PRIVATE disk error")):
                with self.assertRaises(AssertionError) as caught:
                    fixture.cli("register")
        self.assertEqual(run.call_count, 1)
        self.assertIn("io_connection_reset", str(caught.exception))
        self.assertNotIn("PRIVATE", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
