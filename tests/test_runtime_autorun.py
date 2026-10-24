"""Autorunner contracts: real bounded children and strict synthetic RPC expectations."""
import copy
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
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
                 "control": {"status": "passed", "projections": [{"editor": "synthetic-checker-control"}]},
                 "renderer": {"status": "passed", "exit_code": 0, "reaped": True}}
                for c in ("conversation", "deny", "skip", "interrupt", "disconnect", "long-output", "keyboard-output", "keyboard-reduced", "history")
                for s in ("1100x760", "480x760") for t in ("light", "dark")]
        good = {"status": "passed", "expected_cases": 36, "cases": rows}
        runner.validate_native_report(good)
        for bad in [{"status": "passed", "expected_cases": 0, "cases": []},
                    {**good, "cases": rows[:-1]}, {**good, "cases": [rows[0]] * 36},
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


if __name__ == "__main__":
    unittest.main()
