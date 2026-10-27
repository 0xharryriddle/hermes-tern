"""Exercise verify.sh's real scratch setup before its first Cargo gate."""
import json
import os
from pathlib import Path
import stat
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class VerifyLauncherTests(unittest.TestCase):
    def scratch(self):
        root = Path(tempfile.mkdtemp(prefix="ht-verify-", dir=os.environ["TMPDIR"]))
        def retain():
            (root / "ownership.json").write_text(json.dumps({
                "retention": "retain;no deletion",
                "paths": [str(path) for path in [root, *root.rglob("*")]],
            }, indent=2) + "\n")
        self.addCleanup(retain)
        return root

    def launch(self, root, tmp=None):
        home = root / "home"
        home.mkdir(mode=0o700)
        commands = root / "commands"
        commands.mkdir(mode=0o700)
        cargo = commands / "cargo"
        cargo.write_text('#!/bin/sh\nprintf "FIRST_CARGO_GATE\\n"\nexit 23\n')
        cargo.chmod(0o700)
        env = {"HOME": str(home), "PATH": str(commands) + os.pathsep + os.defpath,
               "LANG": "C.UTF-8", "PYTHONDONTWRITEBYTECODE": "1"}
        if tmp is not None:
            env["TMPDIR"] = str(tmp)
        run = subprocess.run(["/bin/bash", "-c", 'umask 022; exec "$1"', "verify-test", str(ROOT / "verify.sh")],
                             cwd=ROOT, env=env, capture_output=True, text=True, timeout=10)
        self.assertEqual(run.returncode, 23, "did not stop at the first downstream gate")
        self.assertEqual(run.stdout, "FIRST_CARGO_GATE\n")
        return tmp if tmp is not None else home / ".cache/hermes-tern/tmp"

    def test_fresh_default_scratch_is_private_with_runner_umask_022(self):
        path = self.launch(self.scratch())
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700)

    def test_new_explicit_scratch_is_private_with_runner_umask_022(self):
        root = self.scratch()
        path = self.launch(root, root / "explicit")
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o700)

    def test_existing_explicit_scratch_is_not_silently_chmodded(self):
        root = self.scratch()
        target = root / "existing"
        target.mkdir(mode=0o755)
        target.chmod(0o755)
        before = target.stat()
        self.launch(root, target)
        after = target.stat()
        self.assertEqual((before.st_dev, before.st_ino, stat.S_IMODE(before.st_mode)),
                         (after.st_dev, after.st_ino, stat.S_IMODE(after.st_mode)))


if __name__ == "__main__":
    unittest.main()
