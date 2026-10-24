"""Operator-key startup policy on owned synthetic state; no secrets in diagnostics."""
import json
import os
from pathlib import Path

import subprocess
import tempfile
import time
import unittest
import uuid

from fixture_process import wait_supervisor
import test_coordinator_recovery as recovery

ROOT, BINARY = recovery.ROOT, recovery.BINARY


class OperatorKeyPolicyTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="ht-key-test-", dir=os.environ["TMPDIR"]))
        self.state = self.root / "state"
        self.children = []
        self.addCleanup(self.stop_and_inventory)

    def stop_and_inventory(self):
        for child in self.children:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)
        entries = [self.root, *self.root.rglob("*")]
        (self.root / "ownership.json").write_text(json.dumps({"retention": "retain;no deletion",
            "paths": [{"path": str(p), "ino": p.lstat().st_ino} for p in entries]}, indent=2) + "\n")

    def start(self, ready=True, env=None):
        child = subprocess.Popen([str(BINARY), "serve", "--state", str(self.state), "--workspace", str(self.root)],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env)
        self.children.append(child)
        if ready:
            wait_supervisor(child, BINARY, self.state)
        return child

    def refuse(self, env=None):
        child = self.start(ready=False, env=env)
        try:
            code = child.wait(timeout=4)
        except subprocess.TimeoutExpired:
            self.stop(child)
            self.fail("startup accepted missing/invalid operator authority")
        self.assertNotEqual(code, 0)
        return child

    @staticmethod
    def stop(child):
        child.kill()
        child.wait(timeout=3)

    def fixture_mail(self):
        # Reuse the existing synthetic IPC fixture without inheriting its test methods.
        f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(f.doCleanups)
        f.setUp()
        op = f.send_op()
        self.assertTrue(f.call(f.a, op)["ok"])
        self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
        assert f.server is not None
        self.stop(f.server)
        self.state, self.root = f.state, f.root
        return f

    def database_image(self):
        # Byte/metadata comparison never emits credentials or token hashes.
        return {p.name: (p.stat().st_ino, p.read_bytes()) for p in self.state.iterdir()
                if p.name.startswith("coordination.sqlite3")}

    def test_missing_established_key_refuses_without_recovery_or_replacement(self):
        f = self.fixture_mail()
        key = self.state / "operator.key"
        saved = self.root / "retained-original-key"
        key.rename(saved)
        before = self.database_image()
        self.refuse()
        self.assertFalse(key.exists(), "startup fabricated replacement operator authority")
        self.assertTrue(self.database_image() == before, "database changed despite missing-key refusal")
        self.assertEqual(f.rows("SELECT state FROM messages"), [("submitted",)])
        saved.rename(key)  # Restore only this test's injected loss; not a recovery API.
        child = self.start()
        self.assertEqual(f.rows("SELECT state FROM messages"), [("ambiguous",)])
        self.stop(child)

    def test_invalid_or_unsafe_key_refuses_before_database_recovery(self):
        for kind in ["invalid", "public", "symlink", "dangling", "fifo"]:
            with self.subTest(kind=kind):
                f = self.fixture_mail()
                key = self.state / "operator.key"
                saved = self.root / "retained-key"
                key.rename(saved)
                if kind in ["invalid", "public"]:
                    key.touch(mode=0o600)
                    key.write_bytes(b"invalid" if kind == "invalid" else saved.read_bytes())
                    if kind == "public":
                        key.chmod(0o644)
                elif kind in ["symlink", "dangling"]:
                    key.symlink_to(saved if kind == "symlink" else self.root / "absent-key")
                else:
                    os.mkfifo(key, 0o600)
                before = self.database_image()
                self.refuse()
                self.assertTrue(self.database_image() == before, "invalid key reached SQLite recovery")
                self.assertEqual(f.rows("SELECT state FROM messages"), [("submitted",)])
                key.rename(self.root / ("retained-" + kind))
                saved.rename(key)
                child = self.start()
                self.assertEqual(f.rows("SELECT state FROM messages"), [("ambiguous",)])
                self.stop(child)

    def test_fresh_and_restart_keep_one_original_key(self):
        child = self.start()
        key = self.state / "operator.key"
        identity = key.stat().st_ino, key.read_bytes()
        self.stop(child)
        child = self.start()
        self.assertTrue(identity == (key.stat().st_ino, key.read_bytes()), "restart replaced operator key")
        self.stop(child)
        self.assertEqual(key.stat().st_mode & 0o777, 0o600)
        self.assertEqual((self.state / "credentials").stat().st_mode & 0o777, 0o700)

    def test_nonempty_keyless_bootstrap_and_empty_database_refuse(self):
        parent = self.root
        for kind in ["pending", "empty_database", "unknown", "wal"]:
            with self.subTest(kind=kind):
                self.state = parent / kind
                self.state.mkdir(mode=0o700)
                name = {"pending": ".pending-" + str(uuid.uuid4()), "empty_database": "coordination.sqlite3",
                        "unknown": "retained-evidence", "wal": "coordination.sqlite3-wal"}[kind]
                (self.state / name).touch(mode=0o600)
                self.refuse()
                self.assertFalse((self.state / "operator.key").exists())
                self.assertEqual((self.state / name).stat().st_size, 0)
                if kind != "empty_database":
                    self.assertFalse((self.state / "coordination.sqlite3").exists())

    def test_complete_pre_database_key_reuses_without_generating_another(self):
        # A static fixture token is not a copied personal credential.
        self.state.mkdir(mode=0o700)
        key = self.state / "operator.key"
        key.touch(mode=0o600)
        key.write_bytes(b"a" * 64)
        original = key.stat().st_ino
        child = self.start()
        self.assertEqual(key.stat().st_ino, original)
        self.assertTrue(key.read_bytes() == b"a" * 64)
        self.stop(child)

    def test_missing_database_with_retained_actor_files_refuses_reinitialization(self):
        self.state.mkdir(mode=0o700)
        key = self.state / "operator.key"
        key.touch(mode=0o600)
        key.write_bytes(b"a" * 64)
        (self.state / "credentials").mkdir(mode=0o700)
        self.refuse()
        self.assertFalse((self.state / "coordination.sqlite3").exists())
        self.assertTrue(key.read_bytes() == b"a" * 64)

    def test_competing_owner_does_not_validate_or_mutate_key(self):
        child = self.start()
        key = self.state / "operator.key"
        saved = self.root / "retained-owner-key"
        key.rename(saved)
        before = self.database_image()
        self.refuse()
        self.assertFalse(key.exists())
        self.assertTrue(self.database_image() == before, "competing opener touched journal")
        saved.rename(key)
        self.stop(child)

    def library(self, source, name):
        path = self.root / name
        result = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(path),
                                 str(ROOT / "tests/fixtures" / source), "-ldl"], capture_output=True, timeout=30)
        self.assertEqual(result.returncode, 0, "fixture interposer compilation failed")
        path.chmod(0o600)
        return path

    def test_key_file_and_directory_sync_failure_precedes_existing_database_recovery(self):
        for boundary in ["file", "directory", "parent"]:
            with self.subTest(boundary=boundary):
                f = self.fixture_mail()
                key = self.state / "operator.key"
                library = self.library("archive_fsync_fault.c", "key-fault.so")
                mode, log = self.root / "mode", self.root / "sync.log"
                for p in [mode, log]:
                    p.touch(mode=0o600)
                mode.write_text("1")
                target = {"file": key, "directory": self.state, "parent": self.root}[boundary]
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_FSYNC_TARGET=str(target),
                           HT_TEST_FSYNC_MODE=str(mode), HT_TEST_FSYNC_LOG=str(log))
                before = self.database_image()
                original = key.stat().st_ino, key.read_bytes()
                self.refuse(env)
                self.assertEqual(log.read_text(), "fail\n", "intended sync boundary not reached")
                self.assertTrue(self.database_image() == before, "sync failure reached SQLite")
                self.assertTrue((key.stat().st_ino, key.read_bytes()) == original)
                self.assertEqual(f.rows("SELECT state FROM messages"), [("submitted",)])
                mode.write_text("0")
                child = self.start(env=env)
                self.assertEqual(f.rows("SELECT state FROM messages"), [("ambiguous",)])
                self.assertTrue((key.stat().st_ino, key.read_bytes()) == original)
                self.stop(child)

    def test_sigkill_after_final_key_sync_before_database_creation_reuses_key(self):
        library = self.library("migration_archive_barrier.c", "key-barrier.so")
        key = self.state / "operator.key"
        ready, release = self.root / "key-ready", self.root / "key-release"
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(key),
                   HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                   HT_TEST_MIGRATION_SYNC=str(key))
        child = self.start(ready=False, env=env)
        deadline = time.monotonic() + 4
        while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
            time.sleep(0.005)
        self.assertTrue(ready.exists(), "did not reach final-key sync barrier")
        self.assertEqual(ready.read_bytes(), b"synced\n")
        self.assertFalse((self.state / "coordination.sqlite3").exists(), "SQLite opened before authority sync")
        original = key.stat().st_ino, key.read_bytes()
        self.stop(child)
        child = self.start()
        self.assertTrue((key.stat().st_ino, key.read_bytes()) == original, "crash retry minted new authority")
        self.stop(child)

    def test_key_change_at_sync_barrier_refuses_before_database_creation(self):
        library = self.library("migration_archive_barrier.c", "key-change-barrier.so")
        self.state.mkdir(mode=0o700)
        key = self.state / "operator.key"
        key.touch(mode=0o600)
        key.write_bytes(b"a" * 64)
        ready, release = self.root / "change-ready", self.root / "change-release"
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(key),
                   HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                   HT_TEST_MIGRATION_SYNC=str(key))
        child = self.start(ready=False, env=env)
        deadline = time.monotonic() + 4
        while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
            time.sleep(0.005)
        self.assertTrue(ready.exists(), "did not reach key revalidation barrier")
        key.write_bytes(b"b" * 64)
        release.touch(mode=0o600)
        self.assertNotEqual(child.wait(timeout=3), 0)
        self.assertFalse((self.state / "coordination.sqlite3").exists())
        key.write_bytes(b"a" * 64)  # repair only the exact fixture mutation
        self.stop(self.start())


if __name__ == "__main__":
    unittest.main()
