"""Production migrator barriers around verified-archive use; synthetic private state only."""
from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import unittest
import uuid

import test_coordinator_migration as migration
import test_coordinator_recovery as recovery


class MigrationArchiveIdentityTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.root = Path(os.environ["TMPDIR"]) / ("ht-archive-barrier-" + uuid.uuid4().hex)
        cls.root.mkdir(mode=0o700)
        cls.library = cls.root / "barrier.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(cls.library),
                                str(recovery.ROOT / "tests/fixtures/migration_archive_barrier.c"), "-ldl"],
                               capture_output=True, timeout=30)
        if built.returncode:
            raise AssertionError(built.stderr.decode())
        cls.library.chmod(0o600)
        (cls.root / "ownership.json").write_text(json.dumps({"retention": "retain; no deletion",
            "paths": [str(cls.root), str(cls.library), str(cls.root / "ownership.json")]}))

    def fixture(self):
        f = migration.CoordinatorMigrationTests()
        self.addCleanup(f.doCleanups)
        f.setUp()
        return f

    def paused_migrate(self, f, mutate, phase="comparison", archive_retry=False):
        ready, release = (f.f.root / name for name in ["archive-read-ready", "archive-read-release"])
        bundle = f.f.state / "archives" / f.archive_id
        env = dict(os.environ, LD_PRELOAD=str(self.library),
                   HT_TEST_MIGRATION_MANIFEST=str(bundle / "manifest.json"),
                   HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release))
        if phase == "comparison":
            env["HT_TEST_MIGRATION_SHM"] = str(f.f.state / "coordination.sqlite3-shm")
        else:
            env["HT_TEST_MIGRATION_SYNC"] = str(f.f.state / "archives")
        if archive_retry:
            f.f.server = subprocess.Popen([str(recovery.BINARY), "serve", "--state", str(f.f.state),
                                            "--workspace", str(f.f.root)], env=env,
                                           stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            f.f.ready()
            argv = [str(recovery.BINARY), "coord", "--state", str(f.f.state), "archive", "--archive-id", f.archive_id]
            child_env = os.environ.copy()
        else:
            argv = [str(recovery.BINARY), "migrate-store", "--state", str(f.f.state),
                    "--workspace", str(f.f.root), "--archive-id", f.archive_id]
            child_env = env
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=child_env)
        try:
            deadline = time.monotonic() + 4
            while not ready.exists():
                self.assertIsNone(child.poll(), "migrator exited before comparison barrier")
                self.assertLess(time.monotonic(), deadline, "owned migrator did not reach its WAL write-lock barrier")
                time.sleep(0.005)
            self.assertEqual(ready.read_text(), "locked\n" if phase == "comparison" else "synced\n")
            mutate(bundle)
            release.touch(mode=0o600)
            stdout, stderr = child.communicate(timeout=8)
            return child.returncode, stdout, stderr
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)
            if child.stdout is not None:
                child.stdout.close()
            if child.stderr is not None:
                child.stderr.close()

    def test_unchanged_archive_at_comparison_barrier_migrates_and_retries(self):
        f = self.fixture()
        before = f.logical()
        observed = {}

        def observe(bundle):
            observed.update({p.name: (p.stat().st_ino, p.read_bytes()) for p in bundle.iterdir()})

        code, stdout, _ = self.paused_migrate(f, observe)
        self.assertEqual(code, 0, "barrier broke the positive migration control")
        receipt = json.loads(stdout)
        self.assertEqual(receipt["to_schema"], 2)
        self.assertEqual(f.logical(), before)
        self.assertEqual(json.loads(f.migrate().stdout), receipt)
        bundle = f.f.state / "archives" / f.archive_id
        self.assertEqual({p.name: (p.stat().st_ino, p.read_bytes()) for p in bundle.iterdir()}, observed)

    def test_changed_archive_after_verification_refuses_without_committing_schema(self):
        for variant in ["replace_identical", "in_place", "manifest"]:
            with self.subTest(variant=variant):
                f = self.fixture()
                before = f.logical()
                original = {}

                def change(bundle):
                    path, manifest = bundle / "coordination.sqlite3", bundle / "manifest.json"
                    original.update({"database": path.read_bytes(), "manifest": manifest.read_bytes(),
                                     "inode": path.stat().st_ino})
                    if variant == "replace_identical":
                        path.rename(f.f.root / "retained-original-database")
                        path.touch(mode=0o600)
                        path.write_bytes(original["database"])
                        self.assertNotEqual(path.stat().st_ino, original["inode"])
                    elif variant == "in_place":
                        with closing(sqlite3.connect(path)) as db:
                            db.execute("UPDATE meta SET value='changed-after-verify' WHERE key='workspace_root'")
                            db.commit()
                    else:
                        manifest.write_text("{}")

                code, stdout, _ = self.paused_migrate(f, change)
                self.assertNotEqual(code, 0, "changed archive still authorized schema commit")
                self.assertEqual(stdout, b"")
                self.assertEqual(f.f.rows("PRAGMA user_version"), [(1,)])
                self.assertEqual(f.logical(), before)
                self.assertEqual(f.f.rows("SELECT count(*) FROM sqlite_schema WHERE name IN ('message_dispositions','schema_migrations')"), [(0,)])
                # Restore only our injected fixture changes; exact retry must now work.
                bundle = f.f.state / "archives" / f.archive_id
                path, manifest = bundle / "coordination.sqlite3", bundle / "manifest.json"
                if variant == "replace_identical":
                    path.rename(f.f.root / "retained-replacement-database")
                    (f.f.root / "retained-original-database").rename(path)
                elif variant == "in_place":
                    path.write_bytes(original["database"])
                else:
                    manifest.write_bytes(original["manifest"])
                retry = f.migrate()
                self.assertEqual(retry.returncode, 0, "repair of injected fixture did not restore migration")
                self.assertEqual(f.logical(), before)

    def test_committed_migration_and_archive_retries_recheck_after_directory_sync(self):
        for archive_retry in [False, True]:
            for changed in [False, True]:
                with self.subTest(archive_retry=archive_retry, changed=changed):
                    f = self.fixture()
                    expected = json.loads(f.migrate().stdout)
                    # Startup recovery is separate from retry; make the fixture quiescent first.
                    f.f.start()
                    assert f.f.server is not None
                    f.f.server.kill()
                    f.f.server.wait(timeout=3)
                    before = f.logical()
                    witness = f.f.rows("SELECT * FROM schema_migrations")

                    def change(bundle):
                        if changed:
                            (bundle / "manifest.json").write_text("{}")

                    code, stdout, _ = self.paused_migrate(f, change, phase="sync", archive_retry=archive_retry)
                    if changed:
                        self.assertNotEqual(code, 0, "retry acknowledged archive altered after verification")
                        self.assertEqual(stdout, b"")
                    elif archive_retry:
                        self.assertEqual(code, 0)
                        self.assertEqual(json.loads(stdout)["status"], "verified")
                    else:
                        self.assertEqual(code, 0)
                        self.assertEqual(json.loads(stdout), expected)
                    self.assertEqual(f.logical(), before)
                    self.assertEqual(f.f.rows("SELECT * FROM schema_migrations"), witness)


if __name__ == "__main__":
    unittest.main()
