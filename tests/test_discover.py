import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]
HELPER = ROOT / "crates/gateway/bridge/hermes_discover.py"

class DiscoverTests(unittest.TestCase):
    def fixture(self, code):
        d = tempfile.TemporaryDirectory(dir=os.environ["TMPDIR"], prefix="ht-discovery-test-")
        self.addCleanup(d.cleanup)
        root = pathlib.Path(d.name)
        f = root / "ui-tui/scripts/gateway_bootstrap.py"
        f.parent.mkdir(parents=True)
        f.write_text(code)
        return root

    def run_helper(self, root):
        return subprocess.run([sys.executable, "-I", "-B", str(HELPER), str(root)], capture_output=True, timeout=3)

    def test_discovers_without_start_recover_and_contains_output(self):
        root = self.fixture("""import sys
def bootstrap(start, recover):
    assert (start, recover) == (False, False)
    print('secret diagnostic')
    print('secret error', file=sys.stderr)
    return {'url':'ws://127.0.0.1:1234/api/ws','protocols':['hermes-gateway-v1','hermes-gateway-ticket.test-only'],'profile_id':'p','instance_id':'i'}
""")
        result = self.run_helper(root)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["instance_id"], "i")
        self.assertEqual(result.stderr, b"")
        self.assertNotIn(b"diagnostic", result.stdout)

    def test_failure_never_dumps_remote_or_ticket_diagnostics(self):
        root = self.fixture("def bootstrap(start, recover):\n    raise RuntimeError('SECRET ticket')\n")
        result = self.run_helper(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertNotIn(b"SECRET", result.stderr)

    def test_missing_owner_does_not_return_a_success_grant(self):
        root = self.fixture("def bootstrap(start, recover):\n    return {}\n")
        result = self.run_helper(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
