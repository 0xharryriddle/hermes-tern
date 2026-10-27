"""Explicit current-session child roster via production CLI/PTY, synthetic owner only."""
import copy
import termios
import unittest

import test_native_chat as native


def row(name="child-1", **changes):
    value = {"subagent_id": name, "parent_id": None, "depth": 0, "goal": "Review parser 😀",
             "delegation_id": "batch-1", "model": "fixture-model", "started_at": 100.5,
             "status": "running", "tool_count": 2, "last_tool": "read_file", "accepting_steer": True}
    value.update(changes)
    return value


class DelegatedAgentsTests(unittest.TestCase):
    def fixture(self, mode="valid", **kwargs):
        f = native.NativeChat(**kwargs)
        self.addCleanup(f.close)
        f.gateway.delegated_mode = mode
        self.held = None
        self.value = {"subagents": [row(), row("child-2", parent_id="child-1", depth=1)], "delegations": []}
        original = f.gateway.reply
        def reply(request, result):
            if request["method"] == "subagent.list":
                if mode == "held":
                    self.held = copy.deepcopy(request)
                    return
                if mode == "refused":
                    f.gateway.send({"jsonrpc": "2.0", "id": request["id"], "error": {"code": -32601, "message": "unsupported_projection PRIVATE-ERROR"}})
                    return
                result = copy.deepcopy(self.value)
                if mode == "empty":
                    result["subagents"] = []
                if mode == "malformed":
                    result["subagents"][0]["tool_count"] = True
            original(request, result)
        f.gateway.reply = reply
        f.ready()
        return f

    def button(self, f, label):
        ids = [k for k, v in f.nodes.items() if v.get("text") == label and v.get("kind") == "el"]
        self.assertEqual(len(ids), 1, label)
        return ids[0]

    def open(self, f):
        f.send(b"\x1bOQ")  # F2
        f.until(lambda: "main.runtime.profile" in f.nodes, "runtime panel")

    def read(self, f):
        f.click(self.button(f, "Read delegated agents"))

    def wait_result(self, f):
        f.until(lambda: "reported delegated agents" in f.nodes.get("main.runtime.delegated.count", {}).get("text", ""), "delegated roster")

    def back(self, f):
        f.click(self.button(f, "Return to conversation"))
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "return")

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        for method in ("prompt.submit", "session.mutate", "approval.respond", "clarify.respond", "session.interrupt", "command.dispatch", "subagent.steer", "subagent.interrupt", "subagent.tail", "process.list", "process.stop", "process.kill"):
            self.assertFalse(f.gateway.methods(method), method)

    def test_explicit_roster_is_session_scoped_inert_and_preserves_draft_without_auto_read(self):
        f = self.fixture()
        f.send(b"\x1b[200~keep draft \xf0\x9f\x98\x80\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep draft 😀", "draft")
        self.open(f)
        f.quiet()
        self.assertFalse(f.gateway.methods("subagent.list"))
        self.read(f)
        self.wait_result(f)
        self.assertEqual([r["params"] for r in f.gateway.methods("subagent.list")], [{"session_id": "fixture-session"}])
        text = "\n".join(n.get("text", "") for n in f.nodes.values())
        self.assertIn("Delegated: child-1", text)
        self.assertIn("Review parser 😀", text)
        self.assertIn("not independent peers", text)
        f.click(self.button(f, "Next child"))
        f.until(lambda: any("Delegated: child-2" in v.get("text", "") for v in f.nodes.values()), "second child")
        self.assertIn("Parent: child-1", "\n".join(n.get("text", "") for n in f.nodes.values()))
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep draft 😀")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\rnot draft input")
        f.event("send", sf=f.surface, id="dock.ed", text="must not submit")
        f.quiet()
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        self.back(f)
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep draft 😀")
        self.open(f)
        f.quiet()
        self.assertFalse(any("Delegated: child-1" in v.get("text", "") for v in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        self.finish(f)

    def test_prior_panel_read_callback_cannot_run_after_reopen_even_without_pending_read(self):
        f = self.fixture()
        self.open(f)
        old = self.button(f, "Read delegated agents")
        self.back(f)
        self.open(f)
        f.click(old)
        f.quiet()
        self.assertFalse(f.gateway.methods("subagent.list"))
        self.read(f)
        self.wait_result(f)
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        self.finish(f)

    def test_inventory_read_is_available_during_running_turn_without_submit_or_cancellation(self):
        f = self.fixture(running=True)
        self.open(f)
        self.read(f)
        self.wait_result(f)
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        self.finish(f)

    def test_empty_is_distinct_from_refused_or_malformed_and_private_fields_never_render(self):
        for mode in ("empty", "refused", "malformed", "valid"):
            with self.subTest(mode=mode):
                f = self.fixture(mode)
                self.value["api_key"] = "PRIVATE-TOP"
                self.value["subagents"][0].update(api_key="PRIVATE-KEY", transcript="PRIVATE-TRANSCRIPT", agent={"credential": "PRIVATE-AGENT"})
                self.open(f)
                self.read(f)
                if mode in ("refused", "malformed"):
                    f.until(lambda: "unavailable" in f.nodes.get("main.runtime.delegated.status", {}).get("text", ""), "unavailable")
                    self.assertNotIn("main.runtime.delegated.count", f.nodes)
                else:
                    self.wait_result(f)
                    self.assertIn(("0" if mode == "empty" else "2") + " reported delegated agents", f.nodes["main.runtime.delegated.count"]["text"])
                self.assertNotIn("PRIVATE-", "\n".join(v.get("text", "") for v in f.nodes.values()))
                self.finish(f)

    def test_old_opening_callback_and_pending_old_result_cannot_populate_reopened_panel(self):
        f = self.fixture("held")
        self.open(f)
        old = self.button(f, "Read delegated agents")
        self.read(f)
        f.until(lambda: self.held is not None, "held read")
        self.back(f)
        self.assertTrue(f.nodes["dock.ed"]["sendable"], "read-only inventory must not lock ordinary Send")
        self.open(f)
        f.click(old)
        f.quiet()
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        assert self.held is not None
        f.gateway.send({"jsonrpc": "2.0", "id": self.held["id"], "result": self.value})
        f.quiet()
        self.assertFalse(any("Delegated: child-1" in v.get("text", "") for v in f.nodes.values()))
        self.assertEqual(f.proc.poll(), None)
        self.read(f)
        f.until(lambda: len(f.gateway.methods("subagent.list")) == 2, "fresh explicit read")
        self.finish(f)

    def test_generation_advance_rejects_old_response_without_breaking_healthy_session(self):
        f = self.fixture("held")
        self.open(f)
        self.read(f)
        f.until(lambda: self.held is not None, "held read")
        f.gateway.event("session.info", {"running": False}, generation=3)
        f.quiet()
        assert self.held is not None
        f.gateway.send({"jsonrpc": "2.0", "id": self.held["id"], "result": self.value})
        f.quiet()
        self.assertFalse(any("Delegated: child-1" in v.get("text", "") for v in f.nodes.values()))
        self.back(f)
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_ready_snapshot_is_removed_when_generation_advances_without_automatic_refresh(self):
        f = self.fixture()
        self.open(f)
        self.read(f)
        self.wait_result(f)
        f.gateway.event("session.info", {"running": False}, generation=3)
        f.until(lambda: "main.runtime.delegated.count" not in f.nodes, "remove stale roster")
        f.quiet()
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        self.assertFalse(any("Delegated: child-1" in v.get("text", "") for v in f.nodes.values()))
        self.finish(f)

    def test_explicit_reload_resets_child_page_after_inventory_shrinks_without_reusing_old_page(self):
        f = self.fixture()
        self.value["subagents"].append(row("child-3"))
        self.open(f)
        self.read(f)
        self.wait_result(f)
        f.click(self.button(f, "Next child"))
        f.until(lambda: any("Delegated: child-2" in v.get("text", "") for v in f.nodes.values()), "second child")
        f.click(self.button(f, "Next child"))
        f.until(lambda: any("Delegated: child-3" in v.get("text", "") for v in f.nodes.values()), "third child")
        self.value["subagents"] = [row("new-first"), row("new-second")]
        self.read(f)
        f.until(lambda: len(f.gateway.methods("subagent.list")) == 2, "second explicit read")
        f.until(lambda: any("Delegated: new-first" in v.get("text", "") for v in f.nodes.values()), "new first page")
        self.assertIn("Delegated page 1/2", f.nodes["main.runtime.delegated.page"]["text"])
        f.click(self.button(f, "Next child"))
        f.until(lambda: any("Delegated: new-second" in v.get("text", "") for v in f.nodes.values()), "new second page")
        self.assertEqual(len(f.gateway.methods("subagent.list")), 2)
        self.finish(f)

    def test_lost_read_cannot_show_an_empty_success_or_retry_after_reopen(self):
        f = self.fixture("held")
        self.open(f)
        self.read(f)
        f.until(lambda: self.held is not None, "held read")
        assert f.gateway.socket is not None
        f.gateway.socket.shutdown(native.socket.SHUT_RDWR)
        f.until(lambda: "unavailable after disconnect" in f.nodes.get("main.runtime.delegated.status", {}).get("text", ""), "read disconnected")
        self.assertNotIn("main.runtime.delegated.count", f.nodes)
        self.back(f)
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        self.open(f)
        f.quiet()
        self.assertFalse(any(v.get("text") == "Read delegated agents" for v in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("subagent.list")), 1)
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_old_snapshot_page_callback_cannot_navigate_a_new_explicit_roster(self):
        f = self.fixture()
        self.open(f)
        self.read(f)
        self.wait_result(f)
        old = self.button(f, "Next child")
        self.value["subagents"] = [row("fresh-first"), row("fresh-second")]
        self.read(f)
        f.until(lambda: any("Delegated: fresh-first" in v.get("text", "") for v in f.nodes.values()), "fresh first")
        f.click(old)
        f.quiet()
        self.assertTrue(any("Delegated: fresh-first" in v.get("text", "") for v in f.nodes.values()))
        self.assertFalse(any("Delegated: fresh-second" in v.get("text", "") for v in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("subagent.list")), 2)
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
