"""Actual Luau execution with synthetic host callbacks, not a real Tern window."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from luau_tool import interpreter
from test_omp_handoff import BINARY, ROOT, request


def luau(value):
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False)
    if isinstance(value, dict):
        return "{" + ",".join("[" + luau(k) + "]=" + luau(v) for k, v in value.items()) + "}"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    raise TypeError("unsupported Luau fixture value")


class OmpWindowTests(unittest.TestCase):
    def execute(self, proposal=None):
        private = Path(tempfile.mkdtemp(prefix="ht-luau-", dir=os.environ["TMPDIR"]))
        source = (ROOT / "plugins/hermes-tern/window.luau").read_text()
        harness = (ROOT / "tests/fixtures/omp_window_harness.luau").read_text()
        script = private / "window-test.luau"
        script.touch(mode=0o600)
        script.write_text("local real_proposal = " + (luau(proposal) if proposal else "nil") +
                          "\nlocal function load_plugin(tern)\n" + source + "\nend\n" + harness)
        result = subprocess.run([str(interpreter()), str(script)], capture_output=True, timeout=15)
        self.assertEqual(result.returncode, 0, result.stderr.decode() + result.stdout.decode())
        self.assertEqual(len(result.stdout.decode().splitlines()), 31)
        self.assertTrue(all(line.startswith("PASS ") for line in result.stdout.decode().splitlines()))

    def test_real_luau_callbacks_and_negative_controls(self):
        self.execute()

    def test_real_rust_proposal_is_consumable_by_luau_bridge(self):
        state = Path(tempfile.mkdtemp(prefix="ht-luau-state-", dir=os.environ["TMPDIR"])) / "state"
        data = request({"type": "offer", "source": "untrusted source", "text": "Review only.\nNo automatic approval."})
        r = subprocess.run([str(BINARY), "omp-handoff", "exchange", "--state", str(state)],
                           input=json.dumps(data).encode(), capture_output=True, timeout=8)
        self.assertEqual(r.returncode, 0, r.stderr.decode())
        self.execute(json.loads(r.stdout)["proposal"])


if __name__ == "__main__":
    unittest.main()
