"""Retained current-credential publication; synthetic secrets stay inside private fixtures."""
import hashlib
from contextlib import closing
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import time
import unittest
import uuid

import test_coordinator_recovery as recovery
import test_peer_bridge as bridge


class CredentialRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()

    def stage(self, actor=None):
        actor = actor or self.f.a
        sid = str(uuid.uuid4())
        final = Path(actor["credential_file"])
        staged = final.parent / (".pending-" + sid)
        final.rename(staged)
        return sid, staged, final

    def command(self, verb, sid, actor=None, check=True):
        actor = actor or self.f.a
        return self.f.cli(verb, "--staging-id", sid, "--actor", actor["actor_id"],
                          "--incarnation", actor["incarnation"], check=check)

    def index(self):
        return json.loads(self.f.cli("state-index").stdout)

    def library(self, source, name):
        path = self.f.root / name
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(path),
                                str(recovery.ROOT / "tests/fixtures" / source), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        path.chmod(0o600)
        return path

    def launch(self, env):
        f = self.f
        assert f.server is not None
        if f.server.poll() is None:
            f.server.kill()
        f.server.wait(timeout=3)
        f.server = subprocess.Popen([str(recovery.BINARY), "serve", "--state", str(f.state),
                                     "--workspace", str(f.root)], env=env, stdin=subprocess.DEVNULL,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        f.ready()

    def fault_environment(self, staged, final):
        lib = self.library("credential_recovery_fault.c", "credential-fault.so")
        arm, mode, log = [self.f.root / n for n in ["fault-arm", "fault-mode", "fault-log"]]
        mode.touch(mode=0o600)
        log.touch(mode=0o600)
        mode.write_text("off")
        env = dict(os.environ, LD_PRELOAD=str(lib), HT_TEST_CRED_ARM=str(arm), HT_TEST_CRED_MODE=str(mode),
                   HT_TEST_CRED_LOG=str(log), HT_TEST_CRED_STATE=str(self.f.state),
                   HT_TEST_CRED_DIR=str(final.parent), HT_TEST_CRED_STAGED=str(staged), HT_TEST_CRED_FINAL=str(final))
        return env, arm, mode, log

    def raw(self, actor, op):
        _, frame = self.f.frame(actor, op)
        with self.f.connect() as sock:
            sock.sendall(frame)
            with sock.makefile("rb") as stream:
                return json.loads(stream.readline(524289))

    def assert_redacted(self, response):
        # Never print actual fixture secrets in an assertion failure.
        encoded = json.dumps(response)
        secrets = [self.f.a["token"], self.f.b["token"],
                   hashlib.sha256(self.f.a["token"].encode()).hexdigest()]
        self.assertTrue(all(value not in encoded for value in secrets), "credential leaked in receipt")
        self.assertNotIn("token_hash", encoded)
        self.assertNotIn("credential", response.keys())

    def test_valid_staging_recovers_current_file_and_exact_retry_without_new_authority(self):
        f = self.f
        sid, staged, final = self.stage()
        original = (staged.stat().st_ino, staged.read_bytes())
        before = self.index()
        inspected = json.loads(self.command("inspect-staged-credential", sid).stdout)
        self.assertEqual(inspected["status"], "staged_verified")
        self.assertFalse(inspected["published"])
        self.assertEqual(inspected["actor_id"], f.a["actor_id"])
        self.assert_redacted(inspected)
        self.assertFalse(final.exists())
        self.assertEqual(self.index(), before)
        result = json.loads(self.command("recover-credential", sid).stdout)
        self.assertEqual(result["status"], "credential_recovered")
        self.assertTrue(result["published"])
        self.assertEqual(result["credential_file"], str(final))
        self.assertFalse(result["authority_changed"])
        self.assert_redacted(result)
        self.assertFalse(staged.exists())
        self.assertTrue((final.stat().st_ino, final.read_bytes()) == original, "recovery replaced original bytes or inode")
        self.assertEqual(self.index(), before)
        self.assertEqual(json.loads(self.command("recover-credential", sid).stdout), result)
        f.restart()
        after_restart = self.index()
        self.assertEqual(json.loads(self.command("recover-credential", sid).stdout), result)
        self.assertEqual(self.index(), after_restart)
        # Use the recovered file through the actual peer CLI, not just an in-memory secret.
        self.assertEqual(f.cli("--credential-file", str(final), "receive").returncode, 0)

    def test_stale_wrong_partial_or_unsafe_candidates_refuse_and_preserve_evidence(self):
        f = self.f
        sid, staged, final = self.stage()
        original = staged.read_bytes()
        self.assertEqual(self.command("inspect-staged-credential", sid).returncode, 0)
        before = self.index()
        valid = json.loads(original)
        for field, value in [("schema_version", 2), ("workspace_id", str(uuid.uuid4())),
                             ("actor_id", f.b["actor_id"]), ("incarnation", str(uuid.uuid4())),
                             ("credential", "0" * 64), ("extra", "not allowed")]:
            mutated = {**valid, field: value}
            staged.write_text(json.dumps(mutated))
            self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
            self.assertNotEqual(self.command("inspect-staged-credential", sid, check=False).returncode, 0)
            self.assertFalse(final.exists())
            self.assertEqual(self.index(), before)
        for raw in [b"{", b"x" * 4097, b"\xff", original[:-1] + b',"schema_version":1}']:
            staged.write_bytes(raw)
            result = self.command("recover-credential", sid, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
        staged.write_bytes(original)
        staged.chmod(0o644)
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        staged.chmod(0o600)
        os.link(staged, f.root / "retained-link")
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        staged.rename(f.root / "retained-linked-candidate")
        staged.symlink_to(f.root / "retained-linked-candidate")
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        staged.rename(f.root / "retained-symlink")
        staged.touch(mode=0o600)
        staged.write_bytes(original)
        self.assertEqual(self.command("inspect-staged-credential", sid).returncode, 0)
        latest = json.loads(f.cli("rotate", "--actor", f.a["actor_id"]).stdout)
        before = self.index()
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        self.assertNotEqual(self.command("inspect-staged-credential", sid, check=False).returncode, 0)
        self.assertFalse(final.exists())
        self.assertTrue(staged.read_bytes() == original)
        self.assertTrue(Path(latest["credential_file"]).is_file())
        self.assertEqual(self.index(), before)

    def test_peer_forgery_conflict_and_capacity_refusals_have_valid_operator_controls(self):
        f = self.f
        sid, staged, final = self.stage()
        original = staged.read_bytes()
        operator = {"token": (f.state / "operator.key").read_text()}
        op = {"type": "recover_credential", "staging_id": sid, "actor": f.a["actor_id"],
              "incarnation": f.a["incarnation"]}
        inspect = {**op, "type": "inspect_staged_credential"}
        self.assertTrue(self.raw(operator, inspect)["ok"])
        before = self.index()
        for actor in [f.a, f.b, {"token": "0" * 64}]:
            self.assertFalse(self.raw(actor, op)["ok"])
            self.assertFalse(self.raw(actor, inspect)["ok"])
        for wrong in [{**op, "actor": str(uuid.uuid4())}, {**op, "staging_id": "../escape"},
                      {**op, "incarnation": "bad"}, {**op, "path": str(staged)},
                      {**op, "approve": True}, {"type": "recover_credential"}]:
            self.assertFalse(self.raw(operator, wrong)["ok"])
        final.touch(mode=0o600)
        final.write_bytes(original)
        inode = final.stat().st_ino
        # Even two identical candidates are a conflict; select neither implicitly.
        self.assertFalse(self.raw(operator, op)["ok"])
        self.assertEqual(final.stat().st_ino, inode)
        self.assertTrue(staged.is_file())
        final.rename(f.root / "retained-final-conflict")
        for n in range(1024 - len(list(final.parent.iterdir()))):
            (final.parent / ("retained-" + str(n))).touch(mode=0o600)
        self.assertEqual(len(list(final.parent.iterdir())), 1024)
        self.assertTrue(self.raw(operator, op)["ok"])
        self.assertEqual(len(list(final.parent.iterdir())), 1024)
        self.assertEqual(self.index(), before)
        self.assertTrue(self.raw(operator, op)["ok"])

    def test_lost_recovery_response_and_kill_reconcile_final_identity_only(self):
        f = self.f
        sid, staged, final = self.stage()
        original = (staged.stat().st_ino, staged.read_bytes())
        self.assertEqual(self.command("inspect-staged-credential", sid).returncode, 0)
        operator = {"token": (f.state / "operator.key").read_text()}
        before = self.index()
        f.lose_response(operator, {"type": "recover_credential", "staging_id": sid,
                                  "actor": f.a["actor_id"], "incarnation": f.a["incarnation"]})
        deadline = time.monotonic() + 4
        while not final.exists():
            self.assertLess(time.monotonic(), deadline, "recovery did not publish final credential")
            time.sleep(0.005)
        f.restart()
        result = json.loads(self.command("recover-credential", sid).stdout)
        self.assertFalse(result["authority_changed"])
        self.assertTrue((final.stat().st_ino, final.read_bytes()) == original)
        self.assertFalse(staged.exists())
        self.assertEqual(self.index()["revision"], before["revision"])
        other_staging = str(uuid.uuid4())
        self.assertEqual(json.loads(self.command("recover-credential", other_staging).stdout), result)
        self.assertEqual(result["staging_provenance"], "not_established")

    def test_each_sync_failure_refuses_and_readonly_inspection_never_finishes_publication(self):
        sid, staged, final = self.stage()
        original = (staged.stat().st_ino, staged.read_bytes())
        env, arm, mode, log = self.fault_environment(staged, final)
        self.launch(env)
        arm.touch(mode=0o600)  # Arm after startup's unrelated private-directory syncs.
        before = self.index()
        for phase in ["entry", "file", "pre"]:
            mode.write_text(phase)
            calls = log.read_text()
            self.assertEqual(self.command("inspect-staged-credential", sid).returncode, 0)
            self.assertEqual(log.read_text(), calls, "inspection performed a file sync")
            failed = self.command("recover-credential", sid, check=False)
            self.assertNotEqual(failed.returncode, 0)
            self.assertEqual(failed.stdout, b"")
            self.assertEqual(log.read_text().splitlines()[-1], phase + ":fail")
            self.assertTrue((staged.stat().st_ino, staged.read_bytes()) == original)
            self.assertFalse(final.exists())
            self.assertEqual(self.index(), before)
        mode.write_text("post")
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        self.assertEqual(log.read_text().splitlines()[-1], "post:fail")
        self.assertFalse(staged.exists())
        self.assertTrue((final.stat().st_ino, final.read_bytes()) == original)
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        for phase in ["file", "entry", "post"]:
            mode.write_text(phase)
            self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
            self.assertEqual(log.read_text().splitlines()[-1], phase + ":fail")
            self.assertEqual(self.index(), before)
        mode.write_text("off")
        self.launch(env)
        mode.write_text("post")
        before = self.index()
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        mode.write_text("off")
        result = json.loads(self.command("recover-credential", sid).stdout)
        self.assertEqual(result["status"], "credential_recovered")
        self.assertEqual(log.read_text().splitlines()[-4:], ["entry:ok", "file:ok", "post:ok", "post:ok"])
        self.assertEqual(self.index(), before)
        self.assertTrue((final.stat().st_ino, final.read_bytes()) == original)

    def test_real_failed_registration_orphan_cannot_be_promoted_to_authority(self):
        f = self.f
        unused = f.state / "credentials" / (".pending-" + str(uuid.uuid4()))
        env, arm, mode, log = self.fault_environment(unused, unused.with_suffix(".json"))
        self.launch(env)
        arm.touch(mode=0o600)
        mode.write_text("post")
        before = self.index()
        entries = set(unused.parent.iterdir())
        failed = f.cli("register", "--kind", "hermes", "--profile", "fixture", "--session", "uncommitted",
                       "--model", "fixture", "--provider", "no-inference", check=False)
        self.assertNotEqual(failed.returncode, 0)
        self.assertEqual(failed.stdout, b"")
        self.assertEqual(log.read_text().splitlines()[-1], "post:fail")
        extra = set(unused.parent.iterdir()) - entries
        self.assertEqual(len(extra), 1)
        orphan = extra.pop()
        value = json.loads(orphan.read_bytes())
        identity = {k: value[k] for k in ["actor_id", "incarnation"]}
        sid = str(uuid.uuid4())
        staged = orphan.parent / (".pending-" + sid)
        orphan.rename(staged)
        calls = log.read_text()
        for verb in ["inspect-staged-credential", "recover-credential"]:
            result = self.command(verb, sid, identity, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
        self.assertEqual(log.read_text(), calls, "uncommitted credential reached filesystem publication")
        self.assertFalse(orphan.exists())
        self.assertTrue(staged.is_file())
        self.assertEqual(self.index(), before)
        mode.write_text("off")
        current_sid, _, _ = self.stage()
        self.assertEqual(self.command("recover-credential", current_sid).returncode, 0)
        self.assertEqual(self.index(), before)

    def test_post_sync_changes_and_racing_final_refuse_while_registry_writer_is_fenced(self):
        f = self.f
        lib = self.library("migration_archive_barrier.c", "credential-barrier.so")
        for phase, variants in [
            ("file", ["unchanged", "content", "inode", "mode", "directory", "state-directory", "race"]),
            ("post", ["unchanged", "content", "inode", "directory", "state-directory"]),
            ("retry", ["unchanged", "content", "inode", "directory", "state-directory"]),
        ]:
            for variant in variants:
                with self.subTest(phase=phase, variant=variant):
                    # A fresh current candidate for each arm; leave all prior files retained.
                    sid, staged, final = self.stage()
                    if phase == "retry":
                        self.assertEqual(self.command("recover-credential", sid).returncode, 0)
                    selected = staged if phase == "file" else final
                    original = selected.read_bytes() if selected.exists() else staged.read_bytes()
                    marker = f.root / (phase + "-" + variant + "-armed")
                    ready = marker.with_suffix(".ready")
                    release = marker.with_suffix(".release")
                    env = dict(os.environ, LD_PRELOAD=str(lib), HT_TEST_MIGRATION_MANIFEST=str(marker if phase == "file" else final),
                               HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                               HT_TEST_MIGRATION_SYNC=str(staged if phase == "file" else final.parent))
                    # For post/retry, final must not arm startup's credentials-directory sync.
                    if phase == "retry":
                        env["HT_TEST_MIGRATION_MANIFEST"] = str(marker)
                    self.launch(env)
                    marker.touch(mode=0o600)
                    before = self.index()
                    args = [str(recovery.BINARY), "coord", "--state", str(f.state), "recover-credential",
                            "--staging-id", sid, "--actor", f.a["actor_id"], "--incarnation", f.a["incarnation"]]
                    child = subprocess.Popen(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                    try:
                        winner = None
                        deadline = time.monotonic() + 4
                        while not ready.exists():
                            self.assertIsNone(child.poll(), "credential recovery exited before sync barrier")
                            self.assertLess(time.monotonic(), deadline, "credential sync barrier not reached")
                            time.sleep(0.005)
                        self.assertEqual(ready.read_text(), "synced\n")
                        with closing(sqlite3.connect(f.state / "coordination.sqlite3", timeout=0)) as db:
                            with self.assertRaises(sqlite3.OperationalError):
                                db.execute("UPDATE actors SET model='cannot-race'")
                            db.rollback()
                        if variant == "content":
                            selected.write_text("{}")
                        elif variant == "inode":
                            selected.rename(f.root / (phase + "-retained-file"))
                            selected.touch(mode=0o600)
                            selected.write_bytes(original)
                        elif variant == "mode":
                            selected.chmod(0o644)
                        elif variant == "directory":
                            retained = f.root / (phase + "-retained-directory")
                            final.parent.rename(retained)
                            final.parent.mkdir(mode=0o700)
                            # Deliberately keep every original child inode: directory identity must catch it.
                            for entry in list(retained.iterdir()):
                                entry.rename(final.parent / entry.name)
                        elif variant == "state-directory":
                            retained = f.root / (phase + "-retained-state-directory")
                            f.state.rename(retained)
                            f.state.mkdir(mode=0o700)
                            for entry in list(retained.iterdir()):
                                entry.rename(f.state / entry.name)
                        elif variant == "race":
                            final.touch(mode=0o600)
                            final.write_text("winner")
                            winner = final.stat().st_ino
                        release.touch(mode=0o600)
                        stdout, _ = child.communicate(timeout=8)
                        if variant == "unchanged":
                            self.assertEqual(child.returncode, 0)
                            self.assertEqual(json.loads(stdout)["status"], "credential_recovered")
                        else:
                            self.assertNotEqual(child.returncode, 0, "changed credential received success")
                            self.assertEqual(stdout, b"")
                        self.assertEqual(self.index(), before)
                        if variant == "race":
                            assert winner is not None
                            self.assertEqual(final.stat().st_ino, winner)
                            self.assertEqual(final.read_bytes(), b"winner")
                            final.rename(f.root / "retained-race-final")
                        elif variant == "content":
                            selected.write_bytes(original)  # Repair only this synthetic mutation.
                        elif variant == "mode":
                            selected.chmod(0o600)
                        self.assertEqual(self.command("recover-credential", sid).returncode, 0)
                        self.assertEqual(self.index(), before)
                    finally:
                        if child.poll() is None:
                            child.kill()
                        child.wait(timeout=3)
                        for stream in [child.stdout, child.stderr]:
                            if stream is not None:
                                stream.close()

    def test_recovered_current_credential_preserves_existing_peer_binding_and_schema_checks(self):
        f = bridge.PeerBridgeTests()
        self.addCleanup(f.doCleanups)
        f.setUp()
        name = f.bind(f.a, "physical-a")
        assert isinstance(name, str)
        binding = Path(name)
        binding_before = binding.stat().st_ino, binding.read_bytes()
        final = Path(f.a["credential_file"])
        original = final.read_bytes()
        self.assertEqual(f.ok(name, "physical-a", {"type": "status"})["actor_id"], f.a["actor_id"])
        sid = str(uuid.uuid4())
        staged = final.parent / (".pending-" + sid)
        final.rename(staged)
        self.assertNotEqual(f.request(name, "physical-a", {"type": "status"}).returncode, 0)
        args = ["--staging-id", sid, "--actor", f.a["actor_id"], "--incarnation", f.a["incarnation"]]
        before = json.loads(f.cli("state-index").stdout)
        # Same token in a wrong envelope must fail both recovery and the bound bridge.
        for value in [{**json.loads(original), "schema_version": 2},
                      {**json.loads(original), "extra": "reject"},
                      {**json.loads(original), "workspace_id": str(uuid.uuid4())}]:
            staged.write_text(json.dumps(value))
            self.assertNotEqual(f.cli("recover-credential", *args, check=False).returncode, 0)
            staged.rename(final)
            self.assertNotEqual(f.request(name, "physical-a", {"type": "status"}).returncode, 0)
            final.rename(staged)
        staged.write_bytes(original)
        receipt = json.loads(f.cli("recover-credential", *args).stdout)
        self.assertFalse(receipt["authority_changed"])
        self.assertEqual(f.ok(name, "physical-a", {"type": "status"})["actor_id"], f.a["actor_id"])
        self.assertTrue((binding.stat().st_ino, binding.read_bytes()) == binding_before)
        self.assertEqual(json.loads(f.cli("state-index").stdout), before)

    def test_absent_candidate_and_nonregular_files_do_not_recreate_or_block(self):
        sid, staged, final = self.stage()
        retained = self.f.root / "retained-missing-source"
        staged.rename(retained)
        before = self.index()
        for verb in ["inspect-staged-credential", "recover-credential"]:
            result = self.command(verb, sid, check=False)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, b"")
        self.assertFalse(final.exists())
        self.assertFalse(staged.exists())
        os.mkfifo(staged, mode=0o600)
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        staged.rename(self.f.root / "retained-fifo")
        staged.mkdir(mode=0o700)
        self.assertNotEqual(self.command("recover-credential", sid, check=False).returncode, 0)
        staged.rename(self.f.root / "retained-directory-candidate")
        retained.rename(staged)
        self.assertEqual(self.command("recover-credential", sid).returncode, 0)
        self.assertEqual(self.index(), before)


if __name__ == "__main__":
    unittest.main()
