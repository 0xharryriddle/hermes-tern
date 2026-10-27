"""Offline recovery of one intact retained synthetic key, never key reconstruction."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time
import unittest
import uuid
from typing import Any

import test_coordinator_recovery as recovery


class OperatorKeyRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        self.staging_id = str(uuid.uuid4())
        self.final = self.f.state / "operator.key"
        self.staged = self.f.state / (".pending-" + self.staging_id)
        self.expected = hashlib.sha256(self.final.read_bytes()).hexdigest()
        self.home = self.f.root / "offline-home"
        self.home.mkdir(mode=0o700)

    def stop(self):
        self.f.cli("shutdown")
        assert self.f.server is not None
        self.assertEqual(self.f.server.wait(timeout=4), 0)

    def stage(self):
        self.final.rename(self.staged)
        self.original = self.staged.stat().st_ino, self.staged.read_bytes()

    def observe(self):
        values = {}
        for path in [self.f.state, *self.f.state.rglob("*")]:
            info = path.lstat()
            values[str(path.relative_to(self.f.state))] = (
                info.st_ino, stat.S_IMODE(info.st_mode),
                hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(info.st_mode)
                else os.readlink(path) if stat.S_ISLNK(info.st_mode) else None,
            )
        return values

    def database_image(self):
        return {p.name: (p.stat().st_ino, p.read_bytes()) for p in self.f.state.iterdir()
                if p.name.startswith("coordination.sqlite3")}

    def command(self, *, state=None, staging=None, expected=None, attest=True, env=None):
        argv = [str(recovery.BINARY), "recover-operator-key", "--state", str(state or self.f.state),
                "--staging-id", staging or self.staging_id,
                "--expected-operator-key-sha256", expected or self.expected]
        if attest:
            argv.append("--attest-original-key")
        clean = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "LC_ALL")}
        clean.update(HOME=str(self.home), HERMES_HOME=str(self.home), TMPDIR=str(self.f.root),
                     HERMES_TERN_ROOT=str(self.f.root / "must-not-discover"))
        clean.update(env or {})
        return subprocess.run(argv, capture_output=True, timeout=6, env=clean)

    def fault_environment(self, phase):
        library = self.f.root / "operator-key-fault.so"
        if not library.exists():
            built = subprocess.run([
                "cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                str(recovery.ROOT / "tests/fixtures/operator_key_recovery_fault.c"), "-ldl",
            ], capture_output=True, timeout=30)
            self.assertEqual(built.returncode, 0, "key fault interposer compilation failed")
            library.chmod(0o600)
        mode, log = [self.f.root / name for name in ("key-fault-mode", "key-fault-log")]
        for path in (mode, log):
            if not path.exists():
                path.touch(mode=0o600)
        mode.write_text(phase)
        env = {
            "LD_PRELOAD": str(library), "HT_TEST_KEY_MODE": str(mode), "HT_TEST_KEY_LOG": str(log),
            "HT_TEST_KEY_STATE": str(self.f.state), "HT_TEST_KEY_PARENT": str(self.f.root),
            "HT_TEST_KEY_STAGED": str(self.staged), "HT_TEST_KEY_FINAL": str(self.final),
            "HT_TEST_KEY_RETAINED": str(self.f.root / "retained-inode-before-fault"),
            "HT_TEST_KEY_READY": str(self.f.root / "key-fault-ready"),
            "HT_TEST_KEY_RELEASE": str(self.f.root / "key-fault-release"),
        }
        return env, mode, log

    def assert_redacted(self, result):
        output = result.stdout + result.stderr
        self.assertTrue(self.original[1] not in output, "private key bytes leaked")
        self.assertTrue(self.expected.encode() not in output, "private key digest leaked")

    def test_exact_original_key_is_recovered_without_database_recovery_or_new_authority(self):
        op = self.f.send_op()
        self.assertTrue(self.f.call(self.f.a, op)["ok"])
        self.assertTrue(self.f.call(self.f.b, {"type": "receive"})["ok"])
        self.stop()
        self.stage()
        database = self.database_image()
        result = self.command()
        self.assertEqual(result.returncode, 0, "offline same-key recovery command unavailable or refused")
        self.assertEqual(result.stderr, b"")
        body = json.loads(result.stdout)
        self.assertEqual(body["status"], "operator_key_recovered")
        self.assertTrue(body["published"])
        self.assertFalse(body["new_key_generated"])
        self.assertFalse(body["database_opened"])
        self.assertFalse(body["runtime_launched"])
        self.assertEqual(body["key_originality"], "operator_attested_not_independently_proven")
        self.assertEqual(body["staging_provenance"], "not_established")
        self.assertFalse(self.staged.exists())
        self.assertTrue((self.final.stat().st_ino, self.final.read_bytes()) == self.original,
                        "original key bytes or inode replaced")
        self.assertTrue(self.database_image() == database, "offline recovery touched database")
        self.assert_redacted(result)
        retry = self.command()
        self.assertEqual((retry.returncode, retry.stdout, retry.stderr), (0, result.stdout, b""))
        self.assertTrue(self.database_image() == database)
        self.assertEqual(self.f.rows("SELECT state FROM messages"), [("submitted",)])
        # Ordinary explicit startup alone reclassifies unacknowledged delivery.
        self.f.start()
        self.assertEqual(self.f.rows("SELECT state FROM messages"), [("ambiguous",)])
        self.assertEqual(self.f.call(self.f.b, {"type": "receive"})["result"]["messages"], [])
        self.assertEqual(len(json.loads(self.f.cli("actors").stdout)["actors"]), 2)

    def test_active_owner_missing_attestation_and_wrong_expectations_refuse_without_mutation(self):
        self.stage()
        before = self.observe()
        result = self.command()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        assert self.f.server is not None
        self.assertIsNone(self.f.server.poll())
        self.f.server.kill()
        self.f.server.wait(timeout=3)
        invalid_options: list[dict[str, Any]] = [
            {"expected": "0" * 64}, {"expected": "A" * 64},
            {"staging": "../escape"}, {"attest": False},
            {"state": self.f.root / "absent-state"},
        ]
        for options in invalid_options:
            before = self.observe()
            result = self.command(**options)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
            self.assertTrue(self.observe() == before, "refusal mutated retained private state")
            self.assert_redacted(result)
        self.assertFalse((self.f.root / "absent-state").exists())
        self.assertEqual(self.command().returncode, 0)

    def test_both_names_even_identical_refuse_and_final_only_retry_does_not_invent_provenance(self):
        self.stop()
        self.stage()
        self.final.touch(mode=0o600)
        self.final.write_bytes(self.staged.read_bytes())
        before = self.observe()
        result = self.command()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        self.final.rename(self.f.root / "retained-conflicting-key")
        first = self.command()
        self.assertEqual(first.returncode, 0)
        retry = self.command(staging=str(uuid.uuid4()))
        self.assertEqual((retry.returncode, retry.stdout), (0, first.stdout))
        self.assertEqual(json.loads(retry.stdout)["staging_provenance"], "not_established")
        self.assert_redacted(retry)

    def test_private_file_rejections_are_nonmutating_and_have_a_valid_control(self):
        self.stop()
        self.stage()
        original = self.staged.read_bytes()
        for raw in [b"", b"x" * 4097, b"x" * 64, b"a" * 63, b"a" * 65, b"\xff", original + b"\n"]:
            self.staged.write_bytes(raw)
            before = self.observe()
            result = self.command()
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertTrue(self.observe() == before, "refusal mutated retained private state")
            self.assert_redacted(result)
        self.staged.write_bytes(original)
        self.staged.chmod(0o644)
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        self.staged.chmod(0o600)
        linked = self.f.root / "retained-link"
        os.link(self.staged, linked)
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        self.staged.rename(self.f.root / "retained-linked-file")
        self.staged.symlink_to(linked)
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        self.staged.rename(self.f.root / "retained-symlink")
        self.staged.touch(mode=0o600)
        self.staged.write_bytes(original)
        # This creates a different synthetic inode as a new operator-selected candidate;
        # it does not claim a copied real key has authentic provenance.
        self.assertEqual(self.command().returncode, 0)

    def test_invalid_live_database_sentinel_is_never_opened_and_missing_existing_lock_refuses(self):
        self.stop()
        self.stage()
        lock = self.f.state / "owner.lock"
        lock.rename(self.f.root / "retained-owner-lock")
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        (self.f.root / "retained-owner-lock").rename(lock)
        (self.f.state / "coordination.sqlite3").write_bytes(b"do not open SQLite during key recovery")
        before_db = self.database_image()
        result = self.command()
        self.assertEqual(result.returncode, 0)
        self.assertTrue(self.database_image() == before_db, "sentinel was opened or repaired")
        self.assert_redacted(result)

    def test_sync_failures_never_claim_success_and_same_identity_retry_finishes_publication(self):
        self.stop()
        self.stage()
        env, mode, log = self.fault_environment("entry")
        database = self.database_image()
        for phase in ["entry", "file", "pre"]:
            mode.write_text(phase)
            for _ in range(2):
                result = self.command(env=env)
                self.assertEqual((result.returncode, result.stdout), (1, b""))
                self.assertEqual(log.read_text().splitlines()[-1], phase + ":fail",
                                 "fault did not reach the intended fsync")
                self.assertTrue((self.staged.stat().st_ino, self.staged.read_bytes()) == self.original)
                self.assertFalse(self.final.exists())
                self.assertTrue(self.database_image() == database)
                self.assert_redacted(result)
        mode.write_text("post")
        result = self.command(env=env)
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(log.read_text().splitlines()[-1], "post:fail")
        self.assertFalse(self.staged.exists())
        self.assertTrue((self.final.stat().st_ino, self.final.read_bytes()) == self.original)
        for phase in ["file", "entry", "post"]:
            mode.write_text(phase)
            result = self.command(env=env)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertEqual(log.read_text().splitlines()[-1], phase + ":fail")
        mode.write_text("off")
        result = self.command(env=env)
        self.assertEqual((result.returncode, result.stderr), (0, b""))
        self.assertEqual(json.loads(result.stdout)["status"], "operator_key_recovered")
        self.assertTrue(self.database_image() == database)
        self.assert_redacted(result)

    def test_content_and_same_bytes_inode_substitution_before_publish_are_detected(self):
        self.stop()
        self.stage()
        env, mode, log = self.fault_environment("mutate-bytes")
        database = self.database_image()
        for phase in ["mutate-bytes", "mutate-inode"]:
            mode.write_text(phase)
            prior_lines = len(log.read_text().splitlines())
            result = self.command(env=env)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertIn("file:mutated", log.read_text().splitlines()[prior_lines:],
                          "content/inode injection did not reach selected file sync")
            self.assertFalse(self.final.exists())
            self.assertTrue(self.database_image() == database)
            self.assert_redacted(result)
            if phase == "mutate-bytes":
                self.staged.write_bytes(self.original[1])  # Undo only this fixture injection.
        self.assertNotEqual(self.staged.stat().st_ino, self.original[0])
        mode.write_text("off")
        self.assertEqual(self.command(env=env).returncode, 0)

    def test_racing_final_file_is_preserved_instead_of_overwritten(self):
        self.stop()
        self.stage()
        env, _, log = self.fault_environment("race-final")
        result = self.command(env=env)
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(log.read_text().splitlines()[-1], "pre:raced")
        self.assertEqual(self.final.read_bytes(), b"racing-final-preserve")
        self.assertTrue((self.staged.stat().st_ino, self.staged.read_bytes()) == self.original)
        self.assert_redacted(result)

    def test_pre_and_post_rename_kills_retain_exact_candidate_and_do_not_renew_database_authority(self):
        self.stop()
        self.stage()
        database = self.database_image()
        for phase in ["pre", "post"]:
            with self.subTest(phase=phase):
                env, _, log = self.fault_environment("pause-" + phase)
                ready = self.f.root / ("ready-" + phase)
                release = self.f.root / ("release-" + phase)
                env.update(HT_TEST_KEY_READY=str(ready), HT_TEST_KEY_RELEASE=str(release))
                clean = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "LC_ALL")}
                clean.update(HOME=str(self.home), HERMES_HOME=str(self.home), TMPDIR=str(self.f.root), **env)
                child = subprocess.Popen([
                    str(recovery.BINARY), "recover-operator-key", "--state", str(self.f.state),
                    "--staging-id", self.staging_id, "--expected-operator-key-sha256", self.expected,
                    "--attest-original-key",
                ], stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=clean)
                try:
                    deadline = time.monotonic() + 4
                    while not ready.exists():
                        self.assertIsNone(child.poll(), "recovery exited before fault barrier")
                        self.assertLess(time.monotonic(), deadline, "key recovery barrier not reached")
                        time.sleep(0.005)
                    self.assertEqual(ready.read_text(), phase)
                    self.assertEqual(log.read_text().splitlines()[-1], phase + ":paused")
                    competing = self.command()
                    self.assertEqual((competing.returncode, competing.stdout), (1, b""))
                    self.assertTrue(self.database_image() == database)
                    self.assertEqual(self.staged.exists(), phase == "pre")
                    self.assertEqual(self.final.exists(), phase == "post")
                    child.kill()
                    self.assertEqual(child.wait(timeout=3), -9)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=3)
                if phase == "pre":
                    self.assertTrue((self.staged.stat().st_ino, self.staged.read_bytes()) == self.original)
                else:
                    self.assertTrue((self.final.stat().st_ino, self.final.read_bytes()) == self.original)
                self.assertTrue(self.database_image() == database)
        retried = self.command()
        self.assertEqual(retried.returncode, 0)
        self.assertEqual(json.loads(retried.stdout)["staging_provenance"], "not_established")
        self.assertTrue(self.database_image() == database)

    def test_existing_journal_and_private_parent_and_safe_lock_are_required(self):
        self.stop()
        self.stage()
        database = self.f.state / "coordination.sqlite3"
        saved = self.f.root / "retained-journal"
        database.rename(saved)
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        saved.rename(database)
        lock = self.f.state / "owner.lock"
        lock.chmod(0o644)
        before = self.observe()
        self.assertEqual(self.command().returncode, 1)
        self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        lock.chmod(0o600)
        self.f.root.chmod(0o755)
        try:
            before = self.observe()
            self.assertEqual(self.command().returncode, 1)
            self.assertTrue(self.observe() == before, "refusal mutated retained private state")
        finally:
            self.f.root.chmod(0o700)
        result = self.command()
        self.assertEqual(result.returncode, 0)
        self.assert_redacted(result)


if __name__ == "__main__":
    unittest.main()
