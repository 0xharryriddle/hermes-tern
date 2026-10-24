"""Owner-only private publication completion; isolated keys, never logged or installed."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import unittest

import test_peer_bridge as bridge


class PrivatePublicationSyncTests(unittest.TestCase):
    def setUp(self):
        self.f = bridge.PeerBridgeTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        fault = self.f.root / "private-sync-fault"
        fault.mkdir(mode=0o700)
        self.library, self.mode, self.log = (fault / n for n in ["fault.so", "mode", "calls.log"])
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(self.library),
                                str(bridge.ROOT / "tests/fixtures/archive_fsync_fault.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        self.library.chmod(0o600)
        for p in [self.mode, self.log]:
            p.touch(mode=0o600)
        self.mode.write_text("0")

    def launch(self, target):
        if self.f.server.poll() is None:
            self.f.server.kill()
        self.f.server.wait(timeout=3)
        env = dict(os.environ, LD_PRELOAD=str(self.library), HT_TEST_FSYNC_TARGET=str(target),
                   HT_TEST_FSYNC_MODE=str(self.mode), HT_TEST_FSYNC_LOG=str(self.log))
        self.f.server = subprocess.Popen([str(bridge.BINARY), "serve", "--state", str(self.f.state),
                                           "--workspace", str(self.f.root)], env=env,
                                          stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def ready(self):
        self.f.ready()

    def stopped(self):
        try:
            code = self.f.server.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.fail("owner startup accepted a private file before completing failed publication sync")
        self.assertNotEqual(code, 0)

    def binding_path(self):
        return self.f.state / "bindings" / (self.f.a["actor_id"] + "-" + self.f.a["incarnation"] + ".json")

    def identity(self, p):
        return p.stat().st_ino, hashlib.sha256(p.read_bytes()).hexdigest()

    def test_matching_binding_retry_must_finish_parent_sync_including_restart(self):
        self.launch(self.f.state / "bindings")
        self.ready()
        self.mode.write_text("1")
        before = json.loads(self.f.cli("snapshot").stdout)
        initial = self.f.bind(self.f.a, "physical-a", check=False)
        self.assertNotEqual(initial.returncode, 0)
        self.assertEqual(initial.stdout, b"")
        path = self.binding_path()
        original = self.identity(path)
        self.assertEqual(self.log.read_text().splitlines(), ["fail"])
        retry = self.f.bind(self.f.a, "physical-a", check=False)
        self.assertNotEqual(retry.returncode, 0, "binding retry acknowledged unsynced parent")
        self.assertEqual(retry.stdout, b"")
        self.assertEqual(self.log.read_text().splitlines(), ["fail", "fail"])
        self.assertEqual(json.loads(self.f.cli("snapshot").stdout), before)
        self.launch(self.f.state / "bindings")
        self.ready()
        self.assertNotEqual(self.f.bind(self.f.a, "physical-a", check=False).returncode, 0)
        self.assertEqual(self.log.read_text().splitlines(), ["fail", "fail", "fail"])
        self.mode.write_text("0")
        binding = self.f.bind(self.f.a, "physical-a")
        assert isinstance(binding, str)
        self.assertEqual(Path(binding), path)
        self.assertEqual(self.identity(path), original)
        self.assertEqual(self.log.read_text().splitlines(), ["fail", "fail", "fail", "ok"])
        # Ordinary bridge/client reads must not fsync or require write authority.
        self.mode.write_text("1")
        self.assertEqual(self.f.ok(binding, "physical-a", {"type": "status"})["actor_id"], self.f.a["actor_id"])
        self.assertEqual(self.log.read_text().splitlines(), ["fail", "fail", "fail", "ok"])

    def test_existing_operator_key_restart_completes_failed_publication_not_new_authority(self):
        self.f.server.kill()
        self.f.server.wait(timeout=3)
        # A new key is legal only before first database creation. Keep the
        # established fixture intact and inject failure into a fresh bootstrap.
        self.f.state = self.f.root / "fresh-key-state"
        self.f.state.mkdir(mode=0o700)
        key = self.f.state / "operator.key"
        self.mode.write_text("1")
        self.launch(self.f.state)
        self.stopped()
        self.assertTrue(key.is_file(), "fault did not occur after operator-key rename")
        self.assertFalse((self.f.state / "coordination.sqlite3").exists())
        original = self.identity(key)
        first_calls = self.log.read_text().splitlines()
        self.assertIn("fail", first_calls)
        self.launch(self.f.state)
        self.stopped()
        self.assertGreater(len(self.log.read_text().splitlines()), len(first_calls))
        self.assertEqual(self.identity(key), original)
        self.mode.write_text("0")
        self.launch(self.f.state)
        self.ready()
        self.assertEqual(self.identity(key), original)
        self.assertEqual(self.log.read_text().splitlines()[-1], "ok")
        # No token content appears in diagnostic output or assertion messages.
        self.assertEqual(self.f.cli("status").returncode, 0)

    def test_binding_directory_entry_is_synced_before_publication_acknowledges(self):
        self.launch(self.f.state)
        self.ready()
        self.mode.write_text("1")
        before = json.loads(self.f.cli("snapshot").stdout)
        result = self.f.bind(self.f.a, "physical-a", check=False)
        self.assertNotEqual(result.returncode, 0, "new binding directory entry was not synced in state parent")
        self.assertEqual(result.stdout, b"")
        self.assertFalse(self.binding_path().exists(), "published before parent entry sync")
        self.assertEqual(json.loads(self.f.cli("snapshot").stdout), before)
        self.mode.write_text("0")
        binding = self.f.bind(self.f.a, "physical-a")
        assert isinstance(binding, str)
        self.assertEqual(Path(binding), self.binding_path())
        self.assertEqual(json.loads(self.f.cli("snapshot").stdout), before)

    def test_existing_file_sync_errors_refuse_and_ordinary_reads_remain_read_only(self):
        binding = self.f.bind(self.f.a, "physical-a")
        assert isinstance(binding, str)
        path = Path(binding)
        original = self.identity(path)
        self.launch(path)
        self.ready()
        self.mode.write_text("1")
        before = json.loads(self.f.cli("snapshot").stdout)
        result = self.f.bind(self.f.a, "physical-a", check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(self.log.read_text().splitlines()[-1], "fail")
        calls = self.log.read_text()
        self.assertEqual(self.f.ok(binding, "physical-a", {"type": "status"})["actor_id"], self.f.a["actor_id"])
        self.assertEqual(self.log.read_text(), calls)
        self.mode.write_text("0")
        self.assertEqual(self.f.bind(self.f.a, "physical-a"), binding)
        self.assertEqual(self.identity(path), original)
        self.assertEqual(json.loads(self.f.cli("snapshot").stdout), before)

        key = self.f.state / "operator.key"
        original_key = self.identity(key)
        self.mode.write_text("1")
        self.launch(key)
        self.stopped()
        # Client reads still parse the unchanged key even though no owner is available.
        self.assertNotEqual(self.f.cli("status", check=False).returncode, 0)
        self.assertEqual(self.identity(key), original_key)
        self.mode.write_text("0")
        self.launch(key)
        self.ready()
        self.assertEqual(self.identity(key), original_key)

    def test_invalid_private_contents_refuse_before_sync_and_capacity_retry_keeps_inode(self):
        binding = self.f.bind(self.f.a, "physical-a")
        assert isinstance(binding, str)
        path = Path(binding)
        original = path.read_bytes()
        self.launch(path.parent)
        self.ready()
        self.mode.write_text("1")
        for variant in ["content", "mode", "hardlink"]:
            with self.subTest(variant=variant):
                if variant == "content":
                    path.write_text("{}")
                elif variant == "mode":
                    path.chmod(0o644)
                else:
                    os.link(path, self.f.root / "retained-binding-link")
                calls = self.log.read_text()
                result = self.f.bind(self.f.a, "physical-a", check=False)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(self.log.read_text(), calls, "invalid binding reached fsync")
                if variant == "content":
                    path.write_bytes(original)
                elif variant == "mode":
                    path.chmod(0o600)
                else:
                    # Retain both links; restore a new private fixture file, never unlink evidence.
                    path.rename(self.f.root / "retained-linked-binding")
                    path.touch(mode=0o600)
                    path.write_bytes(original)
        self.mode.write_text("0")
        before_inode = path.stat().st_ino
        for i in range(1023):
            (path.parent / ("retained-" + str(i))).touch(mode=0o600)
        self.assertEqual(self.f.bind(self.f.a, "physical-a"), binding)
        self.assertEqual(path.stat().st_ino, before_inode)
        self.assertEqual(len(list(path.parent.iterdir())), 1024)
        self.assertNotEqual(self.f.bind(self.f.b, "physical-b", check=False).returncode, 0)

        self.f.server.kill()
        self.f.server.wait(timeout=3)
        key = self.f.state / "operator.key"
        original_key = key.read_bytes()
        key.write_text("invalid-fixture")
        self.mode.write_text("1")
        calls = self.log.read_text()
        # SQLite can sync the state directory while opening before key validation.
        # Match the key descriptor, not those unrelated database sync operations.
        self.launch(key)
        self.stopped()
        self.assertEqual(self.log.read_text(), calls, "invalid operator key reached fsync")
        key.write_bytes(original_key)  # repair only our injected fixture change
        self.mode.write_text("0")
        self.launch(self.f.state)
        self.ready()

    def test_reused_binding_rechecks_content_inode_and_directory_after_sync(self):
        binding = self.f.bind(self.f.a, "physical-a")
        assert isinstance(binding, str)
        path = Path(binding)
        library = self.f.root / "private-barrier.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(bridge.ROOT / "tests/fixtures/migration_archive_barrier.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        for variant in ["unchanged", "content", "inode", "directory"]:
            with self.subTest(variant=variant):
                self.f.server.kill()
                self.f.server.wait(timeout=3)
                ready, release = (self.f.root / (variant + suffix) for suffix in ["-ready", "-release"])
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(path),
                           HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                           HT_TEST_MIGRATION_SYNC=str(path.parent))
                self.f.server = subprocess.Popen([str(bridge.BINARY), "serve", "--state", str(self.f.state),
                                                   "--workspace", str(self.f.root)], env=env,
                                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                self.ready()
                before = json.loads(self.f.cli("snapshot").stdout)
                argv = [str(bridge.BINARY), "coord", "--state", str(self.f.state), "bind-peer",
                        "--actor", self.f.a["actor_id"], "--hermes-home", str(self.f.home),
                        "--physical-session", "physical-a", "--attest-binding"]
                child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 4
                    while not ready.exists():
                        self.assertIsNone(child.poll(), "binding retry exited before revalidation barrier")
                        self.assertLess(time.monotonic(), deadline, "binding retry did not reach directory fsync")
                        time.sleep(0.005)
                    self.assertEqual(ready.read_text(), "synced\n")
                    original = path.read_bytes()
                    if variant == "content":
                        path.write_text("{}")
                    elif variant == "inode":
                        path.rename(self.f.root / "retained-binding-original")
                        path.touch(mode=0o600)
                        path.write_bytes(original)
                    elif variant == "directory":
                        retained = self.f.root / "retained-binding-directory"
                        path.parent.rename(retained)
                        path.parent.mkdir(mode=0o700)
                        (retained / path.name).rename(path)
                    release.touch(mode=0o600)
                    stdout, _ = child.communicate(timeout=8)
                    if variant == "unchanged":
                        self.assertEqual(child.returncode, 0)
                        self.assertEqual(json.loads(stdout)["binding_file"], binding)
                    else:
                        self.assertNotEqual(child.returncode, 0, "changed binding acknowledged after sync")
                        self.assertEqual(stdout, b"")
                    self.assertEqual(json.loads(self.f.cli("snapshot").stdout), before)
                    if variant == "content":
                        path.write_bytes(original)
                    self.assertEqual(self.f.bind(self.f.a, "physical-a"), binding)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=3)
                    for stream in [child.stdout, child.stderr]:
                        if stream is not None:
                            stream.close()

    def test_operator_startup_rechecks_key_after_file_sync_before_listening(self):
        library = self.f.root / "operator-barrier.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(bridge.ROOT / "tests/fixtures/migration_archive_barrier.c"), "-ldl"],
                               capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        library.chmod(0o600)
        key = self.f.state / "operator.key"
        for variant in ["unchanged", "valid-content", "inode"]:
            with self.subTest(variant=variant):
                if self.f.server.poll() is None:
                    self.f.server.kill()
                self.f.server.wait(timeout=3)
                original = key.read_bytes()
                ready, release = (self.f.root / ("operator-" + variant + suffix) for suffix in ["-ready", "-release"])
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(key),
                           HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                           HT_TEST_MIGRATION_SYNC=str(key))
                self.f.server = subprocess.Popen([str(bridge.BINARY), "serve", "--state", str(self.f.state),
                                                   "--workspace", str(self.f.root)], env=env,
                                                  stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                deadline = time.monotonic() + 4
                while not ready.exists():
                    self.assertIsNone(self.f.server.poll(), "startup exited before key-sync barrier")
                    self.assertLess(time.monotonic(), deadline, "startup did not sync its validated key")
                    time.sleep(0.005)
                self.assertEqual(ready.read_text(), "synced\n")
                if variant == "valid-content":
                    changed = bytearray(original)
                    changed[0] = ord('1') if changed[0] == ord('0') else ord('0')
                    key.write_bytes(changed)
                elif variant == "inode":
                    key.rename(self.f.root / "retained-key-at-barrier")
                    key.touch(mode=0o600)
                    key.write_bytes(original)
                release.touch(mode=0o600)
                if variant == "unchanged":
                    self.ready()
                else:
                    self.stopped()
                    self.assertNotEqual(self.f.cli("status", check=False).returncode, 0)
                    if variant == "valid-content":
                        key.write_bytes(original)  # repair only this test's injected mutation
                    self.mode.write_text("0")
                    self.launch(key)
                    self.ready()


if __name__ == "__main__":
    unittest.main()
