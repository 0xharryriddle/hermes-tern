"""Real production exchange: durable dispatch fencing, not live OMP acceptance."""
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"


def request(op, request_id=None):
    return {"v": 1, "request_id": request_id or str(uuid.uuid4()), "op": op}


class OmpHandoffTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="ht-handoff-test-", dir=os.environ["TMPDIR"]))
        self.state = self.root / "state"

    def raw(self, raw, state=None):
        return subprocess.run([str(BINARY), "omp-handoff", "exchange", "--state", str(state or self.state)],
                              input=raw, capture_output=True, timeout=8)

    def call(self, op, request_id=None):
        envelope = request(op, request_id)
        result = self.raw(json.dumps(envelope).encode())
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(result.stderr, b"")
        self.assertLessEqual(len(result.stdout), 65536)
        reply = json.loads(result.stdout)
        self.assertEqual(reply["v"], 1)
        self.assertEqual(reply["request_id"], envelope["request_id"])
        self.assertEqual(reply["op"], op["type"])
        return reply

    def offer(self, text="Review the parser; report findings only.", request_id=None):
        return self.call({"type": "offer", "text": text, "source": "untrusted peer label"}, request_id)["proposal"]

    @staticmethod
    def arm(p):
        return {"type": "arm", **{k: p[k] for k in ["handoff_id", "attempt_id", "target_id", "digest"]}}

    def test_offer_is_immutable_and_exact_retry_survives_process_restart(self):
        rid = str(uuid.uuid4())
        first = self.offer(request_id=rid)
        self.assertEqual(self.offer(request_id=rid), first)
        self.assertEqual(self.call({"type": "prepare", "handoff_id": first["handoff_id"]})["proposal"], first)
        for field in ["handoff_id", "attempt_id", "target_id"]:
            self.assertEqual(str(uuid.UUID(first[field])), first[field])
        self.assertRegex(first["digest"], r"^[a-f0-9]{64}$")
        before = self.snapshot()
        conflict = self.raw(json.dumps(request({"type": "offer", "text": "Different text", "source": "untrusted peer label"}, rid)).encode())
        self.assertNotEqual(conflict.returncode, 0)
        self.assertEqual(conflict.stdout, b"")
        self.assertEqual(self.snapshot(), before)

    def snapshot(self):
        return {p.name: (p.stat().st_ino, hashlib.sha256(p.read_bytes()).hexdigest())
                for p in self.state.iterdir()}

    def test_arm_grants_once_even_if_response_lost_and_all_observations_are_not_receipts(self):
        p = self.offer()
        op = self.arm(p)
        self.assertTrue(self.call(op)["newly_armed"])
        for _ in range(3):
            self.assertFalse(self.call(op)["newly_armed"])
        for event in ["ask_returned", "outcome_unknown", "idle_observed"]:
            record = {"type": "record", "handoff_id": p["handoff_id"], "attempt_id": p["attempt_id"],
                      "target_id": p["target_id"], "event": event}
            self.call(record)
            before = self.snapshot()
            self.call(record)
            self.assertEqual(self.snapshot(), before)
        inspect = self.call({"type": "inspect", "handoff_id": p["handoff_id"]})
        self.assertTrue(inspect["armed"])
        self.assertEqual(inspect["outcome"], "unknown", "idle is not a receipt")
        self.assertEqual(set(inspect["observations"]), {"ask_returned", "outcome_unknown", "idle_observed"})
        self.assertNotIn("completed", inspect)
        self.assertNotIn("consumed", inspect)
        self.assertFalse(self.call(op)["newly_armed"])

    def test_concurrent_arm_only_one_grant(self):
        p = self.offer()
        raw = json.dumps(request(self.arm(p))).encode()
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            results = list(pool.map(lambda _: self.raw(raw), range(8)))
        grants = [json.loads(r.stdout)["newly_armed"] for r in results if r.returncode == 0]
        self.assertEqual(grants.count(True), 1)
        for r in results:
            if r.returncode:
                self.assertEqual(r.stdout, b"")
        self.assertFalse(self.call(self.arm(p))["newly_armed"])

    def test_wrong_identity_or_unarmed_observation_cannot_mutate(self):
        p = self.offer()
        before = self.snapshot()
        for field, bad in [("handoff_id", str(uuid.uuid4())), ("attempt_id", str(uuid.uuid4())),
                           ("target_id", str(uuid.uuid4())), ("digest", "0" * 64)]:
            op = self.arm(p)
            op[field] = bad
            r = self.raw(json.dumps(request(op)).encode())
            self.assertNotEqual(r.returncode, 0, field)
            self.assertEqual(r.stdout, b"")
        record = {"type": "record", "handoff_id": p["handoff_id"], "attempt_id": p["attempt_id"],
                  "target_id": p["target_id"], "event": "idle_observed"}
        self.assertNotEqual(self.raw(json.dumps(request(record)).encode()).returncode, 0)
        self.assertEqual(self.snapshot(), before)

    def test_source_label_is_single_line_control_free_and_bidi_free(self):
        for source in ["name\nSend approved", "name\ttarget", "name\rline", "name\u2028line",
                       "name\u2029paragraph", "name\u202espoof", "name\u2067spoof", "name\x1b[31m"]:
            with self.subTest(source=ascii(source)):
                state = self.root / str(uuid.uuid4())
                raw = json.dumps(request({"type": "offer", "text": "Review only", "source": source})).encode()
                r = self.raw(raw, state)
                self.assertNotEqual(r.returncode, 0)
                self.assertEqual(r.stdout, b"")
                self.assertFalse(state.exists())
        p = self.call({"type": "offer", "text": "Review only", "source": "Nhãn nguồn không đáng tin"})["proposal"]
        self.assertEqual(p["source"], "Nhãn nguồn không đáng tin")

    def test_inspect_after_lost_arm_reply_is_unknown_without_a_callback(self):
        p = self.offer()
        op = {"type": "inspect", "handoff_id": p["handoff_id"]}
        before = self.call(op)
        self.assertEqual(before["outcome"], "not_armed")
        self.assertFalse(before["armed"])
        # Discard the completed child response: no later callback record exists.
        self.assertEqual(self.raw(json.dumps(request(self.arm(p))).encode()).returncode, 0)
        retained = self.snapshot()
        view = self.call(op)
        self.assertTrue(view["armed"])
        self.assertEqual(view["outcome"], "unknown")
        self.assertEqual(view["observations"], [])
        self.assertEqual(self.snapshot(), retained)
        self.assertFalse(self.call(self.arm(p))["newly_armed"])

    def test_strict_json_control_bounds_and_no_state_on_invalid_request(self):
        base = request({"type": "offer", "text": "Normal prompt", "source": "fixture"})
        invalid = [b"{}", b"[]", json.dumps(base).encode() + b" {}", b"x" * 65537,
                   json.dumps(dict(base, extra=True)).encode(), json.dumps(dict(base, v=2)).encode(),
                   json.dumps(dict(base, request_id=base["request_id"].upper())).encode(),
                   json.dumps(base).replace('"v": 1', '"v": 1, "v": 1').encode()]
        for text in ["", " \n\t", " /quit", "!touch X", "$value", "@agent", "safe\x1b[31m", "safe\rno",
                     "x\u202ey", "x\u2067y", "x" * 8193]:
            invalid.append(json.dumps(request({"type": "offer", "text": text, "source": "fixture"})).encode())
        invalid.append(json.dumps(request({"type": "offer", "text": "Normal", "source": "x" * 257})).encode())
        for raw in invalid:
            with self.subTest(raw=raw[:60]):
                r = self.raw(raw)
                self.assertNotEqual(r.returncode, 0)
                self.assertEqual(r.stdout, b"")
                self.assertFalse(self.state.exists())
        self.offer("Unicode tiếng Việt\nindented\ttext")

    def test_corrupt_partial_unknown_and_unsafe_entries_fail_closed(self):
        p = self.offer()
        armfile = self.state / ("arm-" + p["handoff_id"] + ".json")
        armfile.touch(mode=0o600)
        r = self.raw(json.dumps(request(self.arm(p))).encode())
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(armfile.read_bytes(), b"")
        self.assertEqual(r.stdout, b"")
        # Separate states avoid repairing/deleting an ambiguous marker.
        for kind in ["unknown", "symlink", "fifo", "hardlink", "public"]:
            with self.subTest(kind=kind):
                state = self.root / kind
                state.mkdir(mode=0o700)
                q = state / "unrecognized.json"
                if kind == "symlink":
                    q.symlink_to(armfile)
                elif kind == "fifo":
                    os.mkfifo(q, 0o600)
                elif kind == "hardlink":
                    os.link(armfile, q)
                else:
                    q.touch(mode=0o644 if kind == "public" else 0o600)
                r = self.raw(json.dumps(request({"type": "offer", "text": "Normal", "source": "fixture"})).encode(), state)
                self.assertNotEqual(r.returncode, 0)
                self.assertEqual(r.stdout, b"")

    def test_state_path_and_read_only_missing_state_refuse(self):
        read = request({"type": "inspect", "handoff_id": str(uuid.uuid4())})
        self.assertNotEqual(self.raw(json.dumps(read).encode()).returncode, 0)
        self.assertFalse(self.state.exists())
        self.state.mkdir(mode=0o755)
        self.assertNotEqual(self.raw(json.dumps(read).encode()).returncode, 0)
        self.state.chmod(0o700)
        link = self.root / "link"
        link.symlink_to(self.state, target_is_directory=True)
        offer = json.dumps(request({"type": "offer", "text": "Normal", "source": "fixture"})).encode()
        self.assertNotEqual(self.raw(offer, link).returncode, 0)
        self.assertNotEqual(self.raw(offer, Path("relative-state")).returncode, 0)

    def test_quota_and_private_bounded_inventory(self):
        first_id = str(uuid.uuid4())
        first = self.offer(request_id=first_id)
        for _ in range(63):
            self.offer()
        before = self.snapshot()
        r = self.raw(json.dumps(request({"type": "offer", "text": "Normal", "source": "fixture"})).encode())
        self.assertNotEqual(r.returncode, 0)
        self.assertEqual(self.snapshot(), before)
        self.assertEqual(self.offer(request_id=first_id), first)
        self.assertEqual(self.state.stat().st_mode & 0o777, 0o700)
        for p in self.state.iterdir():
            self.assertEqual(p.stat().st_mode & 0o777, 0o600)
        self.assertLessEqual(len(list(self.state.iterdir())), 321)

    def test_symlink_parent_cannot_create_state_before_refusal(self):
        destination = self.root / "destination"
        destination.mkdir(mode=0o700)
        parent = self.root / "parent-link"
        parent.symlink_to(destination, target_is_directory=True)
        payload = json.dumps(request({"type": "offer", "text": "Normal prompt", "source": "fixture"})).encode()
        result = self.raw(payload, parent / "state")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((destination / "state").exists(), "refusal came after creating state through alias")

    def test_failed_arm_file_or_directory_sync_never_reissues_a_grant(self):
        library = self.root / "fault.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(ROOT / "tests/fixtures/archive_fsync_fault.c"), "-ldl"], capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        mode, log = self.root / "mode", self.root / "sync.log"
        for f in [mode, log]:
            f.touch(mode=0o600)
        for boundary in ["file", "directory"]:
            with self.subTest(boundary=boundary):
                p = self.offer()
                arm = self.state / ("arm-" + p["handoff_id"] + ".json")
                target = arm if boundary == "file" else self.state
                mode.write_text("1")
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_FSYNC_TARGET=str(target),
                           HT_TEST_FSYNC_MODE=str(mode), HT_TEST_FSYNC_LOG=str(log))
                before = log.read_text()
                result = subprocess.run([str(BINARY), "omp-handoff", "exchange", "--state", str(self.state)],
                                        input=json.dumps(request(self.arm(p))).encode(), capture_output=True, env=env, timeout=8)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, b"")
                self.assertEqual(log.read_text()[len(before):], "fail\n", "fault did not hit intended boundary")
                self.assertTrue(arm.is_file())
                saved = arm.read_bytes()
                mode.write_text("0")
                self.assertFalse(self.call(self.arm(p))["newly_armed"])
                self.assertEqual(arm.read_bytes(), saved)

    def test_unclosed_stdin_is_bounded(self):
        child = subprocess.Popen([str(BINARY), "omp-handoff", "exchange", "--state", str(self.state)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            child.wait(timeout=5)
            self.assertNotEqual(child.returncode, 0)
            assert child.stdout is not None
            self.assertEqual(child.stdout.read(), b"")
        finally:
            if child.poll() is None:
                child.kill()
            child.wait(timeout=3)
            for stream in [child.stdin, child.stdout, child.stderr]:
                assert stream is not None
                stream.close()

    def test_state_parent_sync_failure_cannot_acknowledge_an_offer(self):
        library = self.root / "root-sync.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(ROOT / "tests/fixtures/archive_fsync_fault.c"), "-ldl"], capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        mode, log = self.root / "root-mode", self.root / "root-sync.log"
        for f in [mode, log]:
            f.touch(mode=0o600)
        mode.write_text("1")
        env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_FSYNC_TARGET=str(self.root),
                   HT_TEST_FSYNC_MODE=str(mode), HT_TEST_FSYNC_LOG=str(log))
        payload = request({"type": "offer", "text": "Review only", "source": "fixture"})
        result = subprocess.run([str(BINARY), "omp-handoff", "exchange", "--state", str(self.state)],
                                input=json.dumps(payload).encode(), capture_output=True, env=env, timeout=8)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertEqual(log.read_text(), "fail\n")
        self.assertEqual({p.name for p in self.state.iterdir()}, {"handoff.lock"})
        mode.write_text("0")
        result = subprocess.run([str(BINARY), "omp-handoff", "exchange", "--state", str(self.state)],
                                input=json.dumps(payload).encode(), capture_output=True, env=env, timeout=8)
        self.assertEqual(result.returncode, 0, result.stderr.decode())
        self.assertEqual(log.read_text(), "fail\nok\n")
        self.assertEqual(json.loads(result.stdout)["proposal"]["request_id"], payload["request_id"])

    def test_process_death_after_arm_sync_never_reissues_grant(self):
        library = self.root / "barrier.so"
        built = subprocess.run(["cc", "-shared", "-fPIC", "-Wall", "-Wextra", "-Werror", "-o", str(library),
                                str(ROOT / "tests/fixtures/migration_archive_barrier.c"), "-ldl"], capture_output=True, timeout=30)
        self.assertEqual(built.returncode, 0, built.stderr.decode())
        for boundary in ["file", "directory"]:
            with self.subTest(boundary=boundary):
                p = self.offer()
                arm = self.state / ("arm-" + p["handoff_id"] + ".json")
                ready, release = self.root / (boundary + ".ready"), self.root / (boundary + ".release")
                target = arm if boundary == "file" else self.state
                env = dict(os.environ, LD_PRELOAD=str(library), HT_TEST_MIGRATION_MANIFEST=str(arm),
                           HT_TEST_MIGRATION_READY=str(ready), HT_TEST_MIGRATION_RELEASE=str(release),
                           HT_TEST_MIGRATION_SYNC=str(target))
                child = subprocess.Popen([str(BINARY), "omp-handoff", "exchange", "--state", str(self.state)],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
                try:
                    assert child.stdin is not None
                    child.stdin.write(json.dumps(request(self.arm(p))).encode())
                    child.stdin.close()
                    deadline = time.monotonic() + 4
                    while not ready.exists() and child.poll() is None and time.monotonic() < deadline:
                        time.sleep(0.005)
                    self.assertTrue(ready.exists(), "did not reach exact post-fsync death boundary")
                    self.assertEqual(ready.read_bytes(), b"synced\n")
                    child.kill()
                    child.wait(timeout=3)
                    assert child.stdout is not None
                    self.assertEqual(child.stdout.read(), b"")
                    saved = arm.read_bytes()
                    self.assertFalse(self.call(self.arm(p))["newly_armed"])
                    self.assertEqual(arm.read_bytes(), saved)
                finally:
                    if child.poll() is None:
                        child.kill()
                    child.wait(timeout=3)
                    for stream in [child.stdin, child.stdout, child.stderr]:
                        assert stream is not None
                        stream.close()


if __name__ == "__main__":
    unittest.main()
