"""Autorunner contracts: real bounded children and strict synthetic RPC expectations."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]


def module(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / "scripts" / (name + ".py"))
    assert spec is not None and spec.loader is not None
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def request(method, params):
    return {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}


def requests(case):
    rows = [request("session.create", {"source": "tui", "yolo": False})]
    if case in ("conversation", "disconnect"):
        rows += [request("prompt.submit", {"session_id": "fixture-session", "text": "fixture typed message",
                   "input_id": "fixture-input", "clarify_batch": True})]
    elif case == "deny":
        rows += [request("approval.respond", {"session_id": "fixture-session", "execution_generation": 2,
                   "prompt_id": "fixture-approval", "choice": "deny"})]
    elif case == "skip":
        rows += [request("clarify.respond", {"session_id": "fixture-session", "execution_generation": 2,
                   "prompt_id": "fixture-clarify", "answer": ""})]
    elif case == "interrupt":
        rows += [request("session.interrupt", {"session_id": "fixture-session", "execution_generation": 2})]
    if case != "disconnect":
        rows += [request("session.detach", {"session_id": "fixture-session", "subscription_id": "fixture-sub"})]
    return rows


class RuntimeAutorunTests(unittest.TestCase):
    def scratch(self):
        path = Path(tempfile.mkdtemp(prefix="ht-ar-test-", dir=os.environ["TMPDIR"]))
        def retain():
            owned = path / "ownership.json"
            owned.touch(mode=0o600)
            owned.write_text(json.dumps({"retention": "retain;no deletion", "paths": [str(p) for p in [path, *path.rglob("*")]]}))
        self.addCleanup(retain)
        return path

    def test_real_child_failure_cannot_become_success(self):
        runner = module("verify_runtime")
        scratch = self.scratch()
        row = runner.run_command([sys.executable, "-c", "import sys;print('positive-control');sys.exit(7)"],
                                 scratch, dict(os.environ), scratch / "child.log", timeout=3)
        self.assertEqual(row["exit_code"], 7)
        self.assertEqual(row["status"], "failed")
        self.assertIn("positive-control", (scratch / "child.log").read_text())
        self.assertTrue(row["reaped"])
        self.assertEqual(runner.run_command([sys.executable, "-c", "print('ok')"], scratch,
                         dict(os.environ), scratch / "positive.log", timeout=3)["status"], "passed")

    def test_real_child_timeout_is_bounded_and_reaped(self):
        runner = module("verify_runtime")
        scratch = self.scratch()
        row = runner.run_command([sys.executable, "-c", "import time;print('started',flush=True);time.sleep(30)"],
                                 scratch, dict(os.environ), scratch / "timeout.log", timeout=0.2)
        self.assertEqual(row["status"], "timeout")
        self.assertTrue(row["reaped"])
        self.assertIn("started", (scratch / "timeout.log").read_text())

    def test_real_child_output_overflow_refuses_without_unbounded_storage(self):
        runner = module("verify_runtime")
        scratch = self.scratch()
        row = runner.run_command([sys.executable, "-c", "import os;os.write(1,b'x'*100000)"],
                                 scratch, dict(os.environ), scratch / "overflow.log", timeout=3, max_bytes=1024)
        self.assertEqual(row["status"], "output_limit")
        self.assertTrue(row["reaped"])
        self.assertLessEqual((scratch / "overflow.log").stat().st_size, 1024)

    def test_exited_parent_cannot_orphan_a_pipe_holding_descendant(self):
        runner = module("verify_runtime")
        scratch = self.scratch()
        script = ("import os,signal,time\nfrom pathlib import Path\n"
                  "r,w=os.pipe()\npid=os.fork()\n"
                  "if pid==0:\n"
                  " signal.signal(signal.SIGTERM,signal.SIG_IGN)\n"
                  " Path('descendant').write_text(str(os.getpid()))\n"
                  " os.write(w,b'1')\n time.sleep(30)\n"
                  "else:\n os.read(r,1)\n"
                  "os._exit(0)\n")
        row = runner.run_command([sys.executable, "-c", script], scratch,
                                 dict(os.environ), scratch / "orphan.log", timeout=3)
        descendant = int((scratch / "descendant").read_text())
        identity = runner.process_identity(descendant)
        try:
            self.assertEqual(row["status"], "leftover_processes")
            self.assertTrue(row["reaped"])
            self.assertFalse(row["remaining_group_members"])
            if identity is not None:
                state = (Path("/proc") / str(descendant) / "stat").read_text().rsplit(")", 1)[1].split()[0]
                self.assertIn(state, ("Z", "X"), "owned descendant is still running")
        finally:
            # RED also owns this exact child: pin before signaling, never names/UID.
            if identity is not None:
                try:
                    fd = os.pidfd_open(descendant)
                    try:
                        if runner.process_identity(descendant) == identity:
                            signal.pidfd_send_signal(fd, signal.SIGKILL)
                    finally:
                        os.close(fd)
                except ProcessLookupError:
                    pass

    def test_existing_output_root_is_never_overwritten(self):
        runner = module("verify_runtime")
        scratch = self.scratch()
        sentinel = scratch / "result.json"
        sentinel.write_text("retain prior result")
        with self.assertRaises((FileExistsError, ValueError)):
            runner.prepare_output(scratch)
        self.assertEqual(sentinel.read_text(), "retain prior result")
        new = scratch / "new"
        self.assertEqual(runner.prepare_output(new), new)
        self.assertEqual(new.stat().st_mode & 0o777, 0o700)

    def test_requested_gates_never_promote_missing_or_failed_cases(self):
        runner = module("verify_runtime")
        good = [{"name": "local", "status": "passed", "exit_code": 0}]
        self.assertEqual(runner.gate_status(["local"], good), "passed")
        for rows in [[], good * 2, good + [{"name": "native", "status": "failed", "exit_code": 1}]]:
            self.assertEqual(runner.gate_status(["local", "native"], rows), "failed")
        self.assertEqual(runner.gate_status(["local"], [{"name": "local", "status": "passed", "exit_code": 1}]), "failed")
        self.assertEqual(runner.gate_status([], []), "failed")

    def test_native_receipts_require_exact_control_and_one_dispatch(self):
        smoke = module("smoke_native_chat")
        for case in ("conversation", "deny", "skip", "interrupt", "disconnect"):
            with self.subTest(case=case):
                rows = requests(case)
                smoke.validate_requests(case, rows, [])
                with self.assertRaises(ValueError):
                    smoke.validate_requests(case, rows, ["fixture error"])
                altered = copy.deepcopy(rows)
                altered.insert(1, copy.deepcopy(altered[1]))
                with self.assertRaises(ValueError):
                    smoke.validate_requests(case, altered, [])
                with self.assertRaises(ValueError):
                    smoke.validate_requests(case, rows + [request("approval.respond", {"choice": "once"})], [])
        for field, value in [("choice", "once"), ("execution_generation", 3), ("prompt_id", "other")]:
            altered = requests("deny")
            altered[1]["params"][field] = value
            with self.assertRaises(ValueError):
                smoke.validate_requests("deny", altered, [])
        with self.assertRaises(ValueError):
            smoke.validate_requests("conversation", requests("conversation")[:-1], [])

    def test_scenarios_exercise_only_explicit_safe_controls(self):
        smoke = module("smoke_native_chat")
        for case in ("conversation", "deny", "skip", "interrupt", "disconnect"):
            text, shots = smoke.scenario(case, ["/private/a b/hermes-tern", "chat"], Path("/private/a b/stderr"))
            self.assertIn('expect "HT_CLIENT_EXIT_0"', text)
            self.assertTrue(shots)
            selector = '.ht-controls>button:first-child' if case == "disconnect" else '.ht-controls>button:nth-of-type(2)'
            self.assertIn("click " + json.dumps(selector), text)
            self.assertNotIn('click "once"', text)
            self.assertNotIn("--yolo", text)
        with self.assertRaises(ValueError):
            smoke.scenario("unknown", ["binary"], Path("stderr"))

    def test_client_exit_sidecar_captures_real_status_before_terminal_marker(self):
        smoke = module("smoke_native_chat")
        scratch = self.scratch() / "with space"
        scratch.mkdir(mode=0o700)
        text, _ = smoke.scenario("deny", [sys.executable, "-c", "raise SystemExit(7)"], scratch / "client.stderr")
        shell = json.loads(next(line[4:] for line in text.splitlines() if line.startswith("run ")))
        completed = subprocess.run(["/bin/sh", "-c", shell], capture_output=True, text=True, timeout=3)
        self.assertEqual(completed.stdout, "HT_CLIENT_EXIT_7\n")
        sidecar = scratch / "client.exit"
        self.assertEqual(sidecar.read_bytes(), b"7\n")
        self.assertEqual(sidecar.stat().st_mode & 0o777, 0o600)
        self.assertEqual(smoke.client_exit_status(sidecar), {"status": "exited", "exit_code": 7})

    def test_client_exit_diagnostic_is_bounded_strict_and_never_follows_a_symlink(self):
        smoke = module("smoke_native_chat")
        scratch = self.scratch()
        sidecar = scratch / "client.exit"
        self.assertEqual(smoke.client_exit_status(sidecar), {"status": "not_observed"})
        for content in (b"0\n", b"255\n", b"7\n"):
            sidecar.write_bytes(content)
            self.assertEqual(smoke.client_exit_status(sidecar), {"status": "exited", "exit_code": int(content)})
        for content in (b"", b"00\n", b"256\n", b"-1\n", b"0\nextra", b"PRIVATE" * 1000):
            sidecar.write_bytes(content)
            self.assertEqual(smoke.client_exit_status(sidecar), {"status": "invalid"})
        alias = scratch / "symlink.exit"
        alias.symlink_to(sidecar)
        self.assertEqual(smoke.client_exit_status(alias), {"status": "invalid"})

    def test_failed_control_records_attempt_and_socket_phase_without_retry_or_raw_data(self):
        smoke = module("smoke_native_chat")
        scratch = self.scratch()
        endpoint = scratch / "ctl.sock"
        lines = []
        errors = []
        with socket.socket(socket.AF_UNIX) as listener:
            listener.bind(str(endpoint))
            listener.listen(1)
            listener.settimeout(3)
            def serve():
                try:
                    for _ in range(6):
                        conn, _ = listener.accept()
                        with conn:
                            conn.settimeout(3)
                            data = bytearray()
                            while not data.endswith(b"\n"):
                                data.extend(conn.recv(4096))
                            line = data.decode().strip()
                            lines.append(line)
                            if line.startswith("expect "):
                                # Peer closes after receiving the attempt, not before send.
                                continue
                            conn.sendall(b'{"ok":true}\n')
                except Exception as error:
                    errors.append(type(error).__name__)
            peer = threading.Thread(target=serve)
            peer.start()
            record = {}
            smoke.drive_control(endpoint, 'ready\nclick ".fixture"\nexpect "PRIVATE-TEXT"\n',
                                "deny", "dark", "1100x760", record)
            peer.join(timeout=4)
        self.assertFalse(peer.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(sum(line.startswith("click ") for line in lines), 1)
        self.assertEqual(sum(line.startswith("expect ") for line in lines), 1)
        self.assertEqual(record["status"], "failed")
        self.assertEqual(record["failed_call"]["command"], "expect")
        self.assertEqual(record["failed_call"]["phase"], "receive")
        self.assertEqual(record["failed_call"]["error_class"], "ValueError")
        self.assertGreater(record["failed_call"]["remaining_seconds"], 0)
        self.assertLessEqual(record["failed_call"]["remaining_seconds"], 55)
        self.assertNotIn("PRIVATE-TEXT", json.dumps(record))
        self.assertEqual(record["calls"][-1]["command"], "quit")
        self.assertEqual(record["calls"][-1]["status"], "passed")

    def test_native_zero_exit_without_image_output_is_not_acceptance(self):
        smoke = module("smoke_native_chat")
        scratch = self.scratch()
        with self.assertRaises(ValueError):
            smoke.validate_outputs(scratch / "shots", "conversation", ["completed"], "light", "480x760")

    def test_renderer_gateway_deadlines_are_bounded_and_close_interrupts_accept(self):
        smoke = module("smoke_native_chat")
        gateway = smoke.fixtures.Gateway(accept_timeout=60, read_timeout=60)
        started = time.monotonic()
        gateway.close()
        self.assertLess(time.monotonic() - started, 1)
        self.assertFalse(gateway.thread.is_alive())
        self.assertFalse(gateway.errors)
        for limit in (0, -1, 61, float("inf")):
            with self.assertRaises(ValueError):
                smoke.fixtures.Gateway(accept_timeout=limit)

    def test_control_readiness_requires_exact_complete_editor_and_controls(self):
        smoke = module("smoke_native_chat")
        editor = {"ok": True, "count": 1, "nodes": [{"text": "retained after disconnectinsert"}]}
        buttons = {"ok": True, "count": 1, "nodes": [{"text": "Detach"}]}
        self.assertTrue(smoke.ready_projection("disconnect", "disconnected", editor, buttons))
        for bad in [{"ok": True, "count": 1, "nodes": [{"text": "retained after disconnecinsert"}]},
                    {"ok": True, "count": 1, "nodes": [{"text": "retained after disconnect"}]},
                    {"ok": False}, {"ok": True, "count": 0, "nodes": []}]:
            self.assertFalse(smoke.ready_projection("disconnect", "disconnected", bad, buttons))
        self.assertFalse(smoke.ready_projection("disconnect", "disconnected", editor,
                         {"ok": True, "count": 2, "nodes": [{"text": "Send"}, {"text": "Detach"}]}))
        empty = {"ok": True, "count": 1, "nodes": [{"class": "sf sf-editor focused empty", "text": "keyboard help placeholder"}]}
        ready = {"ok": True, "count": 2, "nodes": [{"text": "Send"}, {"text": "Detach"}]}
        self.assertTrue(smoke.ready_projection("conversation", "completed", empty, ready))
        self.assertFalse(smoke.ready_projection("conversation", "completed", editor, ready))

    def test_native_matrix_requires_each_distinct_case_not_self_declared_totals(self):
        runner = module("verify_runtime")
        rows = [{"case": c, "size": s, "theme": t, "status": "passed", "gateway_thread_joined": True,
                 "gateway_errors": [], "outputs": [{"image": "fixture", "layout": "fixture", "sha256": "fixture"}],
                 "client_exit": {"status": "exited", "exit_code": 0},
                 "control": {"status": "passed", "projections": [{"editor": "synthetic-checker-control"}]},
                 "selected_gateway_thread_joined": True, "selected_gateway_errors": [],
                 "renderer": {"status": "passed", "exit_code": 0, "reaped": True}}
                for c in ("conversation", "deny", "skip", "interrupt", "disconnect", "long-output", "keyboard-output", "keyboard-reduced", "history", "launcher-close", "launcher-new", "launcher-resume", "runtime-details", "model-picker", "profile-picker", "commands", "attachments", "mcp-reload", "plugins", "reasoning")
                for s in ("1100x760", "480x760") for t in ("light", "dark")]
        good = {"status": "passed", "expected_cases": 80, "cases": rows}
        runner.validate_native_report(good)
        for bad in [{"status": "passed", "expected_cases": 0, "cases": []},
                    {"status": "passed", "expected_cases": 76, "cases": [r for r in rows if r["case"] != "reasoning"]},
                    {**good, "cases": rows[:-1]}, {**good, "cases": [rows[0]] * 68},
                    {"status": "passed", "expected_cases": 64, "cases": [r for r in rows if r["case"] != "attachments"]},
                    {"status": "passed", "expected_cases": 60, "cases": [r for r in rows if r["case"] != "commands"]},
                    {"status": "passed", "expected_cases": 56, "cases": [r for r in rows if r["case"] != "profile-picker"]},
                    {"status": "passed", "expected_cases": 52, "cases": [r for r in rows if r["case"] != "model-picker"]},
                    {"status": "passed", "expected_cases": 48, "cases": [r for r in rows if r["case"] != "runtime-details"]},
                    {"status": "passed", "expected_cases": 32, "cases": [r for r in rows if r["case"] != "history"]},
                    {"status": "passed", "expected_cases": 28, "cases": [r for r in rows if r["case"] != "long-output"]},
                    {"status": "passed", "expected_cases": 24, "cases": [r for r in rows if not r["case"].startswith("keyboard-")]}]:
            with self.assertRaises(ValueError):
                runner.validate_native_report(bad)
        for field, value in [("status", "failed"), ("outputs", []), ("gateway_thread_joined", False), ("control", {})]:
            bad = copy.deepcopy(good)
            bad["cases"][0][field] = value
            with self.assertRaises(ValueError):
                runner.validate_native_report(bad)
        for exit_status in (None, {}, {"status": "not_observed"}, {"status": "invalid"},
                            {"status": "exited", "exit_code": 7}, {"status": "exited", "exit_code": False}):
            bad = copy.deepcopy(good)
            bad["cases"][0]["client_exit"] = exit_status
            with self.assertRaises(ValueError):
                runner.validate_native_report(bad)


if __name__ == "__main__":
    unittest.main()
