"""Explicit offline upgrade of frozen v1 stores; production binary, no agent calls."""
import json
from contextlib import closing
import os
import sqlite3
import subprocess
import unittest
import uuid

import test_coordinator_recovery as recovery


class CoordinatorMigrationTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        op = self.f.send_op()
        self.assertTrue(self.f.call(self.f.a, op)["ok"])
        self.assertTrue(self.f.call(self.f.b, {"type": "receive"})["ok"])
        assert self.f.server is not None
        self.f.server.kill()
        self.f.server.wait(timeout=3)
        # Freeze the previous schema, not a fresh current store labelled v1.
        # Legacy tables are copied in logical form into the literal v1 DDL below.
        self.legacy = self.logical()
        original_key = self.f.state / "operator.key"
        self.f.state = self.f.root / "legacy-state"
        self.f.state.mkdir(mode=0o700)
        # Preserve only our synthetic fixture authority. Serving a migrated
        # journal must never rely on implicit replacement of a missing key.
        key = self.f.state / "operator.key"
        key.touch(mode=0o600)
        key.write_bytes(original_key.read_bytes())
        (self.f.state / "owner.lock").touch(mode=0o600)
        path = self.f.state / "coordination.sqlite3"
        with closing(sqlite3.connect(path)) as db:
            db.executescript((recovery.ROOT / "tests/fixtures/coordination_v1.sql").read_text())
            for table, rows in self.legacy.items():
                if table == "sqlite_sequence":
                    db.execute("DELETE FROM sqlite_sequence")
                for row in rows:
                    db.execute("INSERT INTO " + table + " VALUES(" + ",".join("?" for _ in row) + ")", row)
            db.commit()
        path.chmod(0o600)
        self.assertEqual(self.logical(), self.legacy)
        self.archive_id = str(uuid.uuid4())

    def logical(self):
        return {t: self.f.rows("SELECT * FROM " + t + " ORDER BY 1,2") for t in
                ["meta", "actors", "routes", "messages", "tasks", "events", "sqlite_sequence"]}

    def migrate(self, archive_id=None):
        return subprocess.run([str(recovery.BINARY), "migrate-store", "--state", str(self.f.state),
                               "--workspace", str(self.f.root), "--archive-id", archive_id or self.archive_id],
                              capture_output=True, timeout=8)

    def test_offline_migration_preserves_submitted_rows_and_archive_and_retries_exactly(self):
        before = self.logical()
        result = self.migrate()
        self.assertEqual(result.returncode, 0, "explicit v1 migration is not implemented")
        receipt = json.loads(result.stdout)
        self.assertEqual(receipt["from_schema"], 1)
        self.assertEqual(receipt["to_schema"], 2)
        self.assertEqual(self.f.rows("PRAGMA user_version"), [(2,)])
        self.assertEqual(self.logical(), before, "offline migration performed delivery recovery or rewrote state")
        archive = self.f.state / "archives" / self.archive_id
        manifest = json.loads((archive / "manifest.json").read_text())
        self.assertEqual(manifest["database_schema"], 1)
        with closing(sqlite3.connect(f"file:{archive / 'coordination.sqlite3'}?mode=ro", uri=True)) as db:
            actual = {t: db.execute("SELECT * FROM " + t + " ORDER BY 1,2").fetchall() for t in before}
        self.assertEqual(actual, before)
        self.assertEqual(json.loads(self.migrate().stdout), receipt)
        self.assertNotEqual(self.migrate(str(uuid.uuid4())).returncode, 0)
        self.f.start()
        self.assertEqual(self.f.rows("SELECT state FROM messages"), [("ambiguous",)])
        self.assertEqual(json.loads(self.f.cli("inspect-archive", "--archive-id", self.archive_id).stdout)["database_schema"], 1)
        self.assertNotEqual(self.migrate().returncode, 0, "migration bypassed live ownership lock")

    def test_normal_open_refuses_legacy_without_changing_delivery(self):
        before = self.logical()
        try:
            result = subprocess.run([str(recovery.BINARY), "serve", "--state", str(self.f.state),
                                     "--workspace", str(self.f.root)], capture_output=True, timeout=3)
        except subprocess.TimeoutExpired:
            self.fail("legacy startup accepted before explicit migration")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"explicit migration", result.stderr)
        self.assertEqual(self.f.rows("PRAGMA user_version"), [(1,)])
        self.assertEqual(self.logical(), before)
        self.assertFalse((self.f.state / "archives").exists())

    def test_failed_upgrade_rolls_back_and_stale_archive_cannot_authorize_newer_source(self):
        path = self.f.state / "coordination.sqlite3"
        # A conflicting schema object forces transactional DDL failure after archival.
        with closing(sqlite3.connect(path)) as db:
            db.execute("CREATE VIEW schema_migrations AS SELECT 1 AS fixture")
            db.commit()
        before = self.logical()
        self.assertNotEqual(self.migrate().returncode, 0)
        self.assertEqual(self.f.rows("PRAGMA user_version"), [(1,)])
        self.assertEqual(self.logical(), before)
        self.assertEqual(self.f.rows("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='message_dispositions'"), [(0,)])
        self.assertTrue((self.f.state / "archives" / self.archive_id).is_dir(), "pre-upgrade archive missing")
        with closing(sqlite3.connect(path)) as db:
            db.execute("DROP VIEW schema_migrations")
            db.execute("UPDATE meta SET value=CAST(CAST(value AS INTEGER)+1 AS TEXT) WHERE key='clock'")
            db.commit()
        before = self.logical()
        self.assertNotEqual(self.migrate().returncode, 0, "stale pre-upgrade archive authorized newer source")
        self.assertEqual(self.logical(), before)
        self.assertEqual(self.f.rows("PRAGMA user_version"), [(1,)])
        self.assertEqual(self.migrate(str(uuid.uuid4())).returncode, 0)

    def test_committed_retry_requires_original_intact_archive_after_source_advances(self):
        receipt = json.loads(self.migrate().stdout)
        self.f.start()
        self.f.cli("route", "--sender", self.f.a["actor_id"], "--recipient", self.f.b["actor_id"])
        assert self.f.server is not None
        self.f.server.kill()
        self.f.server.wait(timeout=3)
        before = self.logical()
        self.assertEqual(json.loads(self.migrate().stdout), receipt)
        self.assertEqual(self.logical(), before)
        archive = self.f.state / "archives" / self.archive_id
        manifest = archive / "manifest.json"
        original = manifest.read_bytes()
        manifest.write_text("{}")
        self.assertNotEqual(self.migrate().returncode, 0)
        self.assertEqual(manifest.read_text(), "{}")
        self.assertEqual(self.logical(), before)
        manifest.write_bytes(original)
        retained = archive.with_name("retained-original")
        archive.rename(retained)
        self.assertNotEqual(self.migrate().returncode, 0)
        self.assertFalse(archive.exists(), "committed retry fabricated a replacement archive")
        self.assertEqual(self.logical(), before)
        retained.rename(archive)
        self.assertEqual(json.loads(self.migrate().stdout), receipt)

    def test_failed_archive_sync_leaves_v1_and_exact_retry_finishes_before_upgrade(self):
        root = self.f.root / "migration-sync"
        root.mkdir(mode=0o700)
        library, mode, log = (root / name for name in ["fault.so", "mode", "calls.log"])
        for p in [mode, log]:
            p.touch(mode=0o600)
        mode.write_text("1")
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror",
                                "-o", str(library), str(recovery.ROOT / "tests/fixtures/archive_fsync_fault.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_FSYNC_TARGET=str(self.f.state / "archives"),
                   HT_TEST_FSYNC_MODE=str(mode), HT_TEST_FSYNC_LOG=str(log))

        def faulted():
            return subprocess.run([str(recovery.BINARY), "migrate-store", "--state", str(self.f.state),
                                   "--workspace", str(self.f.root), "--archive-id", self.archive_id],
                                  env=env, capture_output=True, timeout=8)

        before = self.logical()
        for _ in range(2):
            self.assertNotEqual(faulted().returncode, 0)
            self.assertEqual(self.f.rows("PRAGMA user_version"), [(1,)])
            self.assertEqual(self.logical(), before)
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail"])
        archive = self.f.state / "archives" / self.archive_id
        original = {p.name: (p.stat().st_ino, p.read_bytes()) for p in archive.iterdir()}
        mode.write_text("0")
        receipt = json.loads(faulted().stdout)
        self.assertEqual(receipt["to_schema"], 2)
        self.assertEqual(log.read_text().splitlines(), ["fail", "fail", "ok"])
        self.assertEqual({p.name: (p.stat().st_ino, p.read_bytes()) for p in archive.iterdir()}, original)
        self.assertEqual(self.logical(), before)

    def test_unknown_schema_workspace_and_missing_store_refuse_without_upgrade(self):
        path = self.f.state / "coordination.sqlite3"
        before = self.logical()
        with closing(sqlite3.connect(path)) as db:
            db.execute("PRAGMA user_version=99")
        self.assertNotEqual(self.migrate().returncode, 0)
        self.assertEqual(self.f.rows("PRAGMA user_version"), [(99,)])
        self.assertEqual(self.logical(), before)
        self.assertFalse((self.f.state / "archives").exists())
        with closing(sqlite3.connect(path)) as db:
            db.execute("PRAGMA user_version=1")
        other = self.f.root / "other-workspace"
        other.mkdir(mode=0o700)
        result = subprocess.run([str(recovery.BINARY), "migrate-store", "--state", str(self.f.state),
                                 "--workspace", str(other), "--archive-id", self.archive_id], capture_output=True, timeout=8)
        self.assertNotEqual(result.returncode, 0)
        absent = self.f.root / "absent-state"
        result = subprocess.run([str(recovery.BINARY), "migrate-store", "--state", str(absent),
                                 "--workspace", str(self.f.root), "--archive-id", self.archive_id], capture_output=True, timeout=8)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(absent.exists())
        self.assertEqual(self.logical(), before)


if __name__ == "__main__":
    unittest.main()
