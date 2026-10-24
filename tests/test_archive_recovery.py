"""Complete retained archive publication only; never restore coordinator authority."""
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import unittest
import uuid

import test_coordinator_archive as archive
import test_coordinator_migration as migration
import test_coordinator_recovery as recovery


class ArchiveRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.a = archive.CoordinatorArchiveTests()
        self.addCleanup(self.a.doCleanups)
        self.a.setUp()
        self.f = self.a.f
        self.aid = str(uuid.uuid4())

    def prepare_manual(self):
        receipt = self.a.cli("archive", "--archive-id", self.aid)
        sid = str(uuid.uuid4())
        staged = self.f.state / "archives" / (".pending-" + sid)
        Path(receipt["archive_dir"]).rename(staged)
        return sid, staged, receipt

    def recover(self, sid, digest, check=True):
        return self.f.cli("recover-archive", "--staging-id", sid, "--archive-id", self.aid,
                          "--database-sha256", digest, check=check)

    def inspect(self, sid, check=True):
        return self.f.cli("inspect-staged-archive", "--staging-id", sid, "--archive-id", self.aid, check=check)

    def install_fault(self):
        root = self.f.root / "archive-fault"
        root.mkdir(mode=0o700)
        library, mode, log = (root / n for n in ["fault.so", "mode", "calls.log"])
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(recovery.ROOT / "tests/fixtures/archive_recovery_fault.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        for p in [mode, log]:
            p.touch(mode=0o600)
        mode.write_text("directory")
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_RECOVERY_ROOT=str(self.f.state / "archives"),
                   HT_TEST_RECOVERY_MODE=str(mode), HT_TEST_RECOVERY_LOG=str(log),
                   HT_TEST_RECOVERY_FINAL=str(self.f.state / "archives" / self.aid))

        def start():
            self.f.server = subprocess.Popen([str(recovery.BINARY), "serve", "--state", str(self.f.state),
                                               "--workspace", str(self.f.root)], env=env,
                                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.f.ready()
        self.f.start = start
        self.f.restart()
        return mode, log

    def test_complete_staging_after_real_sync_failure_recovers_without_recapture(self):
        mode, log = self.install_fault()
        before = self.a.logical()
        initial = self.f.cli("archive", "--archive-id", self.aid, check=False)
        self.assertNotEqual(initial.returncode, 0)
        entries = list((self.f.state / "archives").iterdir())
        self.assertEqual(len(entries), 1)
        staged = entries[0]
        self.assertTrue(staged.name.startswith(".pending-"))
        sid = staged.name.removeprefix(".pending-")
        initial_calls = log.read_text().splitlines()
        self.assertEqual(initial_calls[-3:], ["database:ok", "manifest:ok", "directory:fail"])
        self.assertEqual([call for call in initial_calls if call.endswith(":fail")], ["directory:fail"])
        original = {p.name: (p.stat().st_ino, p.read_bytes()) for p in staged.iterdir()}
        self.assertEqual(set(original), {"coordination.sqlite3", "manifest.json"})
        inspect = self.inspect(sid, check=False)
        self.assertEqual(inspect.returncode, 0, "staged archive inspection not implemented")
        observed = json.loads(inspect.stdout)
        self.assertEqual(observed["status"], "staged_verified")
        self.assertEqual(observed["published"], False)
        digest = observed["database_sha256"]
        self.assertEqual(log.read_text().splitlines(), initial_calls, "inspection synced files")
        for step in ["database", "manifest", "directory"]:
            mode.write_text(step)
            refused = self.recover(sid, digest, check=False)
            self.assertNotEqual(refused.returncode, 0)
            self.assertEqual(refused.stdout, b"")
            self.assertEqual(log.read_text().splitlines()[-1], step + ":fail")
            self.assertTrue(staged.exists())
            self.assertFalse((staged.parent / self.aid).exists())
            self.assertEqual(self.a.logical(), before)
        mode.write_text("root")
        self.assertNotEqual(self.recover(sid, digest, check=False).returncode, 0)
        final = staged.parent / self.aid
        self.assertFalse(staged.exists())
        self.assertTrue(final.is_dir())
        self.assertEqual(log.read_text().splitlines()[-1], "root:fail")
        self.assertNotEqual(self.recover(sid, digest, check=False).returncode, 0)
        self.f.restart()
        self.assertNotEqual(self.recover(sid, digest, check=False).returncode, 0)
        mode.write_text("off")
        receipt = json.loads(self.recover(sid, digest).stdout)
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["source_pruned"], False)
        self.assertEqual(receipt["restoration_supported"], False)
        self.assertEqual(json.loads(self.recover(sid, digest).stdout), receipt)
        self.assertEqual({p.name: (p.stat().st_ino, p.read_bytes()) for p in final.iterdir()}, original)
        self.assertEqual(self.a.logical(), before)
        self.assertEqual(len(list(final.parent.iterdir())), 1)
        self.assertEqual(log.read_text().splitlines()[-4:], ["database:ok", "manifest:ok", "directory:ok", "root:ok"])

    def test_operator_only_exact_identity_and_capacity_controls(self):
        sid, staged, old_receipt = self.prepare_manual()
        before = self.a.logical()
        digest = old_receipt["database_sha256"]
        op = {"type": "recover_archive", "staging_id": sid, "archive_id": self.aid, "database_sha256": digest}
        for actor in [self.f.a, self.f.b, {"token": "0" * 64}]:
            self.assertFalse(self.a.raw(actor, op)["ok"])
            self.assertFalse(self.a.raw(actor, {"type": "inspect_staged_archive", "staging_id": sid, "archive_id": self.aid})["ok"])
        for forged in [{**op, "staging_id": "../escape"}, {**op, "archive_id": str(uuid.uuid4())},
                       {**op, "database_sha256": "0" * 64}, {**op, "database_sha256": "A" * 64},
                       {**op, "path": str(staged)}, {"type": "recover_archive"}]:
            self.assertFalse(self.a.raw(self.a.operator, forged)["ok"])
        self.assertTrue(staged.is_dir())
        self.assertEqual(self.a.logical(), before)
        for i in range(7):
            (staged.parent / ("retained-" + str(i))).mkdir(mode=0o700)
        self.assertNotEqual(self.f.cli("archive", "--archive-id", str(uuid.uuid4()), check=False).returncode, 0)
        result = self.recover(sid, digest, check=False)
        self.assertEqual(result.returncode, 0, "complete staging cannot be recovered at entry capacity")
        self.assertEqual(json.loads(result.stdout), old_receipt)
        self.assertEqual(len(list(staged.parent.iterdir())), 8)
        self.assertEqual(self.a.logical(), before)

    def test_racing_final_target_is_not_overwritten_or_selected(self):
        sid, staged, receipt = self.prepare_manual()
        mode, log = self.install_fault()
        mode.write_text("race")
        before = self.a.logical()
        original = {p.name: (p.stat().st_ino, p.read_bytes()) for p in staged.iterdir()}
        result = self.recover(sid, receipt["database_sha256"], check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("directory:race", log.read_text().splitlines(), "recovery did not reach real publication race")
        final = staged.parent / self.aid
        inode = final.stat().st_ino
        self.assertEqual((final / "winner").read_bytes(), b"winner")
        self.assertEqual({p.name: (p.stat().st_ino, p.read_bytes()) for p in staged.iterdir()}, original)
        mode.write_text("off")
        self.assertNotEqual(self.recover(sid, receipt["database_sha256"], check=False).returncode, 0)
        self.assertEqual(final.stat().st_ino, inode)
        final.rename(self.f.root / "retained-race-winner")
        self.assertEqual(json.loads(self.recover(sid, receipt["database_sha256"]).stdout), receipt)
        self.assertEqual(self.a.logical(), before)

    def test_lost_recovery_reply_reconciles_final_identity_after_kill(self):
        sid, staged, receipt = self.prepare_manual()
        before = self.a.logical()
        self.f.lose_response(self.a.operator, {"type": "recover_archive", "staging_id": sid,
                                              "archive_id": self.aid, "database_sha256": receipt["database_sha256"]})
        final = staged.parent / self.aid
        deadline = time.monotonic() + 4
        while not final.exists():
            self.assertLess(time.monotonic(), deadline, "retained staging recovery not implemented")
            time.sleep(0.01)
        self.f.restart()
        self.assertEqual(json.loads(self.recover(sid, receipt["database_sha256"]).stdout), receipt)
        self.assertEqual(self.a.logical(), before)
        self.assertFalse(staged.exists())

    def test_changes_after_sync_refuse_before_or_after_publication_and_repair_retries(self):
        library = self.f.root / "recovery-barrier.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(recovery.ROOT / "tests/fixtures/migration_archive_barrier.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        for phase in ["staging-sync", "root-sync", "final-retry-sync"]:
            with self.subTest(phase=phase):
                self.aid = str(uuid.uuid4())
                sid, staged, expected = self.prepare_manual()
                final = staged.parent / self.aid
                if phase == "final-retry-sync":
                    self.assertEqual(json.loads(self.recover(sid, expected["database_sha256"]).stdout), expected)
                selected = staged if phase == "staging-sync" else final
                ready, release = (self.f.root / (phase + suffix) for suffix in ["-ready", "-release"])
                assert self.f.server is not None
                self.f.server.kill()
                self.f.server.wait(timeout=3)
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(selected / "manifest.json"),
                           HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                           HT_TEST_MIGRATION_SYNC=str(staged if phase == "staging-sync" else staged.parent))
                self.f.server = subprocess.Popen([str(recovery.BINARY), "serve", "--state", str(self.f.state),
                                                   "--workspace", str(self.f.root)], env=env,
                                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                self.f.ready()
                before = self.a.logical()
                child = subprocess.Popen([str(recovery.BINARY), "coord", "--state", str(self.f.state),
                                          "recover-archive", "--staging-id", sid, "--archive-id", self.aid,
                                          "--database-sha256", expected["database_sha256"]],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 4
                    while not ready.exists():
                        self.assertIsNone(child.poll(), "recovery exited before post-sync barrier")
                        self.assertLess(time.monotonic(), deadline, "recovery never reached post-sync barrier")
                        time.sleep(0.005)
                    self.assertEqual(ready.read_text(), "synced\n")
                    manifest = selected / "manifest.json"
                    original = manifest.read_bytes()
                    manifest.write_text("{}")
                    release.touch(mode=0o600)
                    stdout, _ = child.communicate(timeout=8)
                    self.assertNotEqual(child.returncode, 0, "post-sync mutation received success")
                    self.assertEqual(stdout, b"")
                    self.assertEqual(self.a.logical(), before)
                    self.assertEqual(staged.exists(), phase == "staging-sync")
                    self.assertEqual(final.exists(), phase != "staging-sync")
                    manifest.write_bytes(original)  # repair only the explicit injected fixture mutation
                    self.assertEqual(json.loads(self.recover(sid, expected["database_sha256"]).stdout), expected)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=3)
                    if child.stdout is not None:
                        child.stdout.close()
                    if child.stderr is not None:
                        child.stderr.close()

    def test_legacy_v1_archive_can_be_recovered_without_activating_it(self):
        f = migration.CoordinatorMigrationTests()
        self.addCleanup(f.doCleanups)
        f.setUp()
        path = f.f.state / "coordination.sqlite3"
        with closing(sqlite3.connect(path)) as db:
            db.execute("CREATE VIEW schema_migrations AS SELECT 1 AS fixture")
            db.commit()
        self.assertNotEqual(f.migrate().returncode, 0)
        sid = str(uuid.uuid4())
        staged = f.f.state / "archives" / (".pending-" + sid)
        (staged.parent / f.archive_id).rename(staged)
        digest = json.loads((staged / "manifest.json").read_text())["database_sha256"]
        with closing(sqlite3.connect(path)) as db:
            db.execute("DROP VIEW schema_migrations")
            db.commit()
        self.assertEqual(f.migrate(str(uuid.uuid4())).returncode, 0)
        f.f.start()
        before = f.logical()
        result = f.f.cli("recover-archive", "--staging-id", sid, "--archive-id", f.archive_id,
                         "--database-sha256", digest, check=False)
        self.assertEqual(result.returncode, 0, "legacy archive recovery not implemented")
        self.assertEqual(json.loads(result.stdout)["database_schema"], 1)
        self.assertEqual(f.f.rows("PRAGMA user_version"), [(2,)])
        self.assertEqual(f.logical(), before)


if __name__ == "__main__":
    unittest.main()
