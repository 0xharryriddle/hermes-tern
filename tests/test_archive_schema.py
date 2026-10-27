"""Production offline diagnostic over coherently rehashed synthetic DDL changes."""
from contextlib import closing
import hashlib
import json
from pathlib import Path
import sqlite3
import unittest

import test_recovery_preflight as preflight


class ArchiveSchemaTests(unittest.TestCase):
    def setUp(self):
        self.t = preflight.RecoveryPreflightTests()
        self.addCleanup(self.t.doCleanups)
        self.t.setUp()
        self.t.stop()
        self.bundle = self.t.f.state / "archives" / self.t.archive_id
        self.database = self.bundle / "coordination.sqlite3"
        self.manifest = self.bundle / "manifest.json"
        self.original_database = self.database.read_bytes()
        self.original_manifest = self.manifest.read_bytes()
        self.original_receipt = dict(self.t.receipt)

    def rehash(self):
        body = self.database.read_bytes()
        digest = hashlib.sha256(body).hexdigest()
        manifest = json.loads(self.manifest.read_bytes())
        manifest.update(database_bytes=str(len(body)), database_sha256=digest)
        self.manifest.write_text(json.dumps(manifest, separators=(",", ":")))
        self.t.receipt = dict(self.original_receipt, database_bytes=str(len(body)), database_sha256=digest)

    def execute(self, sql):
        with closing(sqlite3.connect(self.database)) as db:
            db.executescript(sql)
            db.commit()
        self.rehash()

    def change_schema_sql(self, name, before, after):
        with closing(sqlite3.connect(self.database)) as db:
            row = db.execute("SELECT sql FROM sqlite_schema WHERE type='table' AND name=?", (name,)).fetchone()
            self.assertIsNotNone(row, "synthetic target schema missing")
            assert row is not None
            self.assertEqual(row[0].count(before), 1, "synthetic mutation anchor is not unique")
            db.execute("PRAGMA writable_schema=ON")
            db.execute("UPDATE sqlite_schema SET sql=? WHERE type='table' AND name=?", (row[0].replace(before, after), name))
            db.execute("PRAGMA writable_schema=OFF")
            db.commit()
        self.rehash()

    def assert_refused_unchanged(self):
        before = self.t.observe()
        result = self.t.run_preflight()
        # Neither the synthetic key nor its digest may appear in failure output.
        output = result.stdout + result.stderr
        key = (self.t.f.state / "operator.key").read_bytes()
        self.assertFalse(key in output, "diagnostic exposed synthetic private key")
        self.assertFalse(self.t.key_hash.encode() in output, "diagnostic exposed private-key expectation")
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertIn(b"archive schema definition differs", result.stderr)
        self.assertTrue(self.t.observe() == before, "diagnostic mutated retained state or archive")

    def restore_and_assert_success(self):
        self.database.write_bytes(self.original_database)
        self.manifest.write_bytes(self.original_manifest)
        self.t.receipt = dict(self.original_receipt)
        before = self.t.observe()
        result = self.t.run_preflight()
        self.assertEqual(result.returncode, 0, "repaired synthetic archive diagnostic failed")
        body = json.loads(result.stdout)
        self.assertEqual(body["status"], "diagnostic_verified")
        self.assertEqual(body["archive_schema_definition"], "exact_supported_v2")
        self.assertEqual(body["activation_supported"], False)
        self.assertEqual(body["restore_authorized"], False)
        self.assertEqual(body["live_state_inspected"], False)
        self.assertTrue(self.t.observe() == before, "valid diagnostic mutated retained state")

    def test_extra_trigger_is_not_a_supported_archive_even_when_digest_and_rows_match(self):
        self.execute("CREATE TRIGGER synthetic_restore_trap BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'synthetic'); END;")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_extra_view_is_not_a_supported_archive_even_when_digest_and_rows_match(self):
        self.execute("CREATE VIEW synthetic_projection AS SELECT id FROM actors;")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_extra_index_is_not_silently_adopted_as_supported_archive_ddl(self):
        self.execute("CREATE INDEX synthetic_actor_model ON actors(model);")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_missing_unique_constraint_is_refused_without_repairing_accepted_actor_rows(self):
        # Rebuild rather than corrupting sqlite_schema: a removed UNIQUE must
        # remove its automatic index too, or quick_check masks the DDL guard.
        with closing(sqlite3.connect(self.database)) as db:
            ddl = db.execute("SELECT sql FROM sqlite_schema WHERE name='actors' AND type='table'").fetchone()[0]
            self.assertEqual(ddl.count("token_hash TEXT UNIQUE NOT NULL"), 1)
            replacement = ddl.replace("CREATE TABLE actors", "CREATE TABLE synthetic_actors").replace(
                "token_hash TEXT UNIQUE NOT NULL", "token_hash TEXT NOT NULL")
            db.execute("PRAGMA foreign_keys=OFF")
            db.executescript("BEGIN;" + replacement + "; INSERT INTO synthetic_actors SELECT * FROM actors;"
                             "DROP TABLE actors; ALTER TABLE synthetic_actors RENAME TO actors; COMMIT;")
            self.assertEqual(db.execute("PRAGMA quick_check").fetchall(), [("ok",)])
            self.assertEqual(db.execute("PRAGMA foreign_key_check").fetchall(), [])
        self.rehash()
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_missing_foreign_key_is_refused_with_the_same_valid_route_rows(self):
        self.change_schema_sql("routes", "recipient TEXT REFERENCES actors(id)", "recipient TEXT")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_weakened_check_is_refused_even_when_existing_rows_satisfy_original_rule(self):
        self.change_schema_sql("tasks", "CHECK(generation>0)", "CHECK(generation>=0)")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_changed_column_type_is_refused_even_with_no_pending_messages(self):
        self.change_schema_sql("messages", "expires INTEGER NOT NULL", "expires TEXT NOT NULL")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_equivalent_alternate_spelling_is_refused_not_normalized_or_repaired(self):
        self.change_schema_sql("tasks", "generation>0", "0<generation")
        self.assert_refused_unchanged()
        self.restore_and_assert_success()

    def test_exact_supported_schema_is_nonmutating_and_does_not_open_live_sqlite(self):
        Path(self.t.f.state / "coordination.sqlite3").write_bytes(b"synthetic live sentinel; never open SQLite here")
        self.restore_and_assert_success()

    def test_archive_after_explicit_frozen_v1_migration_has_supported_v2_ddl(self):
        legacy = preflight.migration.CoordinatorMigrationTests()
        self.addCleanup(legacy.doCleanups)
        legacy.setUp()
        self.assertEqual(legacy.migrate().returncode, 0, "synthetic explicit migration failed")
        legacy.f.start()
        sender, message = legacy.f.rows("SELECT sender,id FROM messages")[0]
        self.assertTrue(legacy.f.call(legacy.f.b, {"type": "ack", "sender": sender, "message": message})["ok"])
        receipt = json.loads(legacy.f.cli("archive", "--archive-id", str(preflight.uuid.uuid4())).stdout)
        legacy.f.cli("shutdown")
        assert legacy.f.server is not None
        self.assertEqual(legacy.f.server.wait(timeout=4), 0)
        original = self.t.f, self.t.receipt, self.t.key_hash
        try:
            self.t.f, self.t.receipt = legacy.f, receipt
            self.t.key_hash = hashlib.sha256((legacy.f.state / "operator.key").read_bytes()).hexdigest()
            before = self.t.observe()
            result = self.t.run_preflight()
            self.assertEqual(result.returncode, 0, "frozen-v1 migrated schema was not supported")
            body = json.loads(result.stdout)
            self.assertEqual(body["archive_schema_definition"], "exact_supported_v2")
            self.assertEqual(body["activation_supported"], False)
            self.assertTrue(self.t.observe() == before, "migrated diagnostic changed retained state")
        finally:
            self.t.f, self.t.receipt, self.t.key_hash = original


if __name__ == "__main__":
    unittest.main()
