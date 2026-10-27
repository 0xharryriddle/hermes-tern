"""Real offline CLI over synthetic coordinator archives; never restore authority."""
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import unittest
import uuid

import test_coordinator_recovery as recovery
import test_coordinator_migration as migration


class RecoveryPreflightTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        self.archive_id = str(uuid.uuid4())
        self.receipt = self.capture(self.archive_id)
        self.key_hash = hashlib.sha256((self.f.state / "operator.key").read_bytes()).hexdigest()
        self.home = self.f.root / "offline-home"
        self.home.mkdir(mode=0o700)

    def capture(self, archive_id):
        return json.loads(self.f.cli("archive", "--archive-id", archive_id).stdout)

    def stop(self):
        self.f.cli("shutdown")
        assert self.f.server is not None
        self.assertEqual(self.f.server.wait(timeout=4), 0)

    def observe(self):
        values = {}
        for path in [self.f.state, *self.f.state.rglob("*")]:
            info = path.lstat()
            values[str(path.relative_to(self.f.state))] = (info.st_ino, stat.S_IMODE(info.st_mode),
                hashlib.sha256(path.read_bytes()).hexdigest() if stat.S_ISREG(info.st_mode) else os.readlink(path) if stat.S_ISLNK(info.st_mode) else None)
        return values

    def run_preflight(self, receipt=None, **overrides):
        receipt = receipt or self.receipt
        fields = {"state": str(self.f.state), "workspace": str(self.f.root),
                  "archive-id": receipt["archive_id"], "workspace-id": receipt["workspace_id"],
                  "database-sha256": receipt["database_sha256"], "expected-operator-key-sha256": self.key_hash}
        fields.update(overrides)
        argv = [str(recovery.BINARY), "preflight-recovery"]
        for name, value in fields.items():
            argv.extend(["--" + name, value])
        env = {key: value for key, value in os.environ.items() if key in ("PATH", "LANG", "LC_ALL")}
        env.update(HOME=str(self.home), HERMES_HOME=str(self.home), TMPDIR=str(self.f.root), HERMES_TERN_ROOT=str(self.f.root / "must-not-discover"))
        return subprocess.run(argv, capture_output=True, timeout=6, env=env)

    def test_settled_archive_diagnostic_has_no_authority_claim_or_live_sqlite_access(self):
        self.stop()
        # A raw live-file sentinel is not SQLite: success proves no live DB open.
        (self.f.state / "coordination.sqlite3").write_bytes(b"do not open or repair this live-state sentinel")
        before = self.observe()
        result = self.run_preflight()
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        body = json.loads(result.stdout)
        self.assertEqual(body["status"], "diagnostic_verified")
        self.assertEqual(body["activation_supported"], False)
        self.assertEqual(body["restore_authorized"], False)
        self.assertEqual(body["live_state_inspected"], False)
        self.assertEqual(body["key_originality"], "not_established")
        self.assertEqual(body["workers_stopped"], "not_established")
        self.assertEqual(body["archive"]["database_sha256"], self.receipt["database_sha256"])
        self.assertEqual(self.observe(), before)
        self.assertEqual(result.stderr, b"")
        self.assertNotIn((self.f.state / "operator.key").read_bytes(), result.stdout)

    def test_wrong_expectations_or_missing_key_refuse_without_creating_or_recovering_state(self):
        self.stop()
        for overrides in ({"database-sha256": "0" * 64}, {"workspace-id": str(uuid.uuid4())},
                          {"archive-id": str(uuid.uuid4())}, {"expected-operator-key-sha256": "0" * 64}):
            before = self.observe()
            result = self.run_preflight(**overrides)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertEqual(self.observe(), before)
        (self.f.state / "operator.key").rename(self.f.root / "retained-operator-key")
        before = self.observe()
        result = self.run_preflight()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(self.observe(), before)
        self.assertFalse((self.f.state / "operator.key").exists())

    def test_held_owner_lock_refuses_and_stopping_only_owned_coordinator_allows_diagnostic(self):
        result = self.run_preflight()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        assert self.f.server is not None
        self.assertIsNone(self.f.server.poll())
        self.stop()
        result = self.run_preflight()
        self.assertEqual(result.returncode, 0, result.stderr.decode())

    def test_unresolved_archived_mail_and_active_task_refuse_without_reclassification(self):
        message = self.f.send_op()
        self.assertTrue(self.f.call(self.f.a, message)["ok"])
        queued = self.capture(str(uuid.uuid4()))
        self.assertTrue(self.f.call(self.f.b, {"type": "receive"})["ok"])
        submitted = self.capture(str(uuid.uuid4()))
        self.assertTrue(self.f.call(self.f.b, {"type": "ack", "sender": self.f.a["actor_id"], "message": message["message"]["message_id"]})["ok"])
        self.assertTrue(self.f.call(self.f.a, {"type": "claim", "task": str(uuid.uuid4()), "ttl_seconds": 300})["ok"])
        active_task = self.capture(str(uuid.uuid4()))
        self.stop()
        before = self.observe()
        for receipt in (queued, submitted, active_task):
            result = self.run_preflight(receipt)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertEqual(self.observe(), before)

    def test_unsafe_key_and_absent_lock_refuse_without_replacement_or_secret_diagnostics(self):
        self.stop()
        key = self.f.state / "operator.key"
        key.chmod(0o644)
        before = self.observe()
        result = self.run_preflight()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertNotIn(key.read_bytes(), result.stderr)
        self.assertEqual(self.observe(), before)
        key.chmod(0o600)
        lock = self.f.state / "owner.lock"
        lock.rename(self.f.root / "retained-owner-lock")
        before = self.observe()
        result = self.run_preflight()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(self.observe(), before)

    def test_ambiguous_even_operator_disposed_archive_is_not_settled_or_safe_to_restore(self):
        op = self.f.send_op()
        self.assertTrue(self.f.call(self.f.a, op)["ok"])
        self.assertTrue(self.f.call(self.f.b, {"type":"receive"})["ok"])
        self.f.restart()
        message = next(row for row in json.loads(self.f.cli("messages").stdout)["messages"] if row["message_id"] == op["message"]["message_id"])
        self.f.cli("dispose-message", "--sender", message["sender_actor_id"], "--message", message["message_id"],
                   "--delivery-incarnation", message["delivery_incarnation"], "--decision-id", str(uuid.uuid4()),
                   "--action", "close", "--reason", "synthetic unknown", "--attest-outcome-unknown")
        receipt = self.capture(str(uuid.uuid4()))
        self.stop()
        before = self.observe()
        result = self.run_preflight(receipt)
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(self.observe(), before)

    def test_wrong_workspace_missing_state_and_symlink_bundle_refuse_without_creation(self):
        self.stop()
        other = self.f.root / "different-workspace"
        other.mkdir(mode=0o700)
        for overrides in ({"workspace":str(other)}, {"state":str(self.f.root / "absent-state")}):
            before = self.observe()
            result = self.run_preflight(**overrides)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertEqual(self.observe(), before)
        self.assertFalse((self.f.root / "absent-state").exists())
        bundle = self.f.state / "archives" / self.archive_id
        retained = self.f.root / "retained-bundle"
        bundle.rename(retained)
        bundle.symlink_to(retained, target_is_directory=True)
        before = self.observe()
        result = self.run_preflight()
        self.assertEqual((result.returncode, result.stdout), (1, b""))
        self.assertEqual(self.observe(), before)

    def test_verified_legacy_archive_still_refuses_offline_v2_diagnostic(self):
        legacy = migration.CoordinatorMigrationTests()
        self.addCleanup(legacy.doCleanups)
        legacy.setUp()
        migrated = legacy.migrate()
        self.assertEqual(migrated.returncode, 0)
        legacy.f.start()
        receipt = json.loads(legacy.f.cli("inspect-archive", "--archive-id", legacy.archive_id).stdout)
        self.assertEqual(receipt["database_schema"], 1, "negative control was not a verified v1 archive")
        legacy.f.cli("shutdown")
        assert legacy.f.server is not None
        self.assertEqual(legacy.f.server.wait(timeout=4), 0)
        original_f = self.f
        original_hash = self.key_hash
        try:
            self.f = legacy.f
            self.key_hash = hashlib.sha256((self.f.state / "operator.key").read_bytes()).hexdigest()
            before = self.observe()
            result = self.run_preflight(receipt)
            self.assertEqual((result.returncode, result.stdout), (1, b""))
            self.assertIn(b"schema or digest", result.stderr)
            self.assertEqual(self.observe(), before)
        finally:
            self.f = original_f
            self.key_hash = original_hash


if __name__ == "__main__":
    unittest.main()
