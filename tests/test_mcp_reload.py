"""Explicit profile MCP reload plus consented reattach through production CLI/PTY."""
import copy
import socket
import termios
import unittest

import test_native_chat as native


class McpReloadTests(unittest.TestCase):
    def fixture(self, mode="applied", running=False):
        f = native.NativeChat(running=running)
        self.addCleanup(f.close)
        f.ready()
        f.gateway.mcp_mode = mode
        return f

    def texts(self, f):
        return "\n".join(node.get("text", "") for node in f.nodes.values())

    def button(self, f, label):
        names = [name for name, node in f.nodes.items()
                 if node.get("text") == label and node.get("actions", {}).get("click")]
        self.assertEqual(len(names), 1, label)
        return names[0]

    def open(self, f):
        f.send(b"\x1bOQ")
        f.until(lambda: "main.runtime.profile" in f.nodes, "runtime details")
        names = [name for name, node in f.nodes.items()
                 if node.get("text") == "Review MCP reload"]
        self.assertEqual(len(names), 1, "missing explicit MCP reload review control")
        f.click(names[0])
        f.until(lambda: "main.mcp.heading" in f.nodes, "MCP review")

    def confirm(self, f):
        f.click(self.button(f, "Confirm profile reload and reattach"))

    def return_to_chat(self, f):
        f.send(b"\x1b")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "conversation")

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        for method in ("prompt.submit", "config.get", "config.set", "command.dispatch", "approval.respond"):
            self.assertFalse(f.gateway.methods(method), method)

    def test_review_is_local_confirm_sends_once_then_reads_owner_snapshot_and_preserves_draft(self):
        f = self.fixture()
        f.send("\x1b[200~unchanged draft 😀\x1b[201~".encode())
        f.until(lambda: f.nodes["dock.ed"].get("text") == "unchanged draft 😀", "draft")
        before = len(f.gateway.requests)
        self.open(f)
        for text in ("fixture-profile", "fixture-instance", "every chat", "reattach", "hooks"):
            self.assertIn(text, self.texts(f))
        self.assertEqual(len(f.gateway.requests), before)
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\r")
        f.event("send", sf=f.surface, id="dock.ed", text="not consent")
        f.quiet()
        self.assertFalse(f.gateway.methods("reload.mcp"))
        old = self.button(f, "Confirm profile reload and reattach")
        f.click(old)
        f.until(lambda: "MCP reattach readback complete" in self.texts(f), "reattach complete")
        self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
        self.assertEqual(f.gateway.methods("reload.mcp")[0]["params"], {"session_id": "fixture-session", "confirm": True})
        self.assertEqual(f.gateway.methods("session.resume")[0]["params"], {"session_id": "fixture-session", "display_projection": True})
        self.assertLess(next(i for i, r in enumerate(f.gateway.requests) if r["method"] == "reload.mcp"),
                        next(i for i, r in enumerate(f.gateway.requests) if r["method"] == "session.resume"))
        self.assertIn("not proof of tool health", self.texts(f))
        self.assertNotIn("PRIVATE-", self.texts(f))
        f.click(old)
        f.quiet()
        self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
        self.return_to_chat(f)
        self.assertEqual(f.nodes["dock.ed"]["text"], "unchanged draft 😀")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_closed_review_and_owner_change_invalidate_old_confirmation_without_reload(self):
        f = self.fixture()
        self.open(f)
        old = self.button(f, "Confirm profile reload and reattach")
        self.return_to_chat(f)
        self.open(f)
        f.click(old)
        f.quiet()
        self.assertFalse(f.gateway.methods("reload.mcp"))
        current = self.button(f, "Confirm profile reload and reattach")
        f.gateway.event("session.info", {"revision": 2, "model": "external-route"})
        f.quiet()
        f.click(current)
        f.quiet()
        self.assertFalse(f.gateway.methods("reload.mcp"))
        self.finish(f)

    def test_busy_and_unavailable_are_visible_not_rollback_or_automatic_retry(self):
        f = self.fixture("busy")
        self.open(f)
        self.confirm(f)
        f.until(lambda: "MCP reattach readback complete" in self.texts(f), "busy readback")
        self.assertIn("busy", self.texts(f))
        self.assertIn("unavailable-server", self.texts(f))
        self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
        self.assertEqual(len(f.gateway.methods("session.resume")), 1)
        self.assertNotIn("PRIVATE-", self.texts(f))
        self.return_to_chat(f)
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_refused_or_malformed_admitted_result_remains_unknown_after_panel_close(self):
        for mode in ("refused", "malformed"):
            with self.subTest(mode=mode):
                f = self.fixture(mode)
                self.open(f)
                self.confirm(f)
                f.until(lambda: "outcome unknown" in self.texts(f), "unknown outcome")
                self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
                self.assertFalse(f.gateway.methods("session.resume"))
                self.return_to_chat(f)
                self.assertFalse(f.nodes["dock.ed"]["sendable"])
                self.assertNotIn("PRIVATE-", self.texts(f))
                f.send(b"\r")
                f.event("send", sf=f.surface, id="dock.ed", text="not admitted")
                f.quiet()
                self.assertFalse(f.gateway.methods("prompt.submit"))
                self.open(f)
                self.assertFalse(any(n.get("text") == "Confirm profile reload and reattach" for n in f.nodes.values()))
                self.finish(f)

    def test_applied_readback_below_floor_foreign_identity_or_yolo_cannot_unlock_send(self):
        for mutation in ("generation", "authority", "session", "profile", "yolo"):
            with self.subTest(mutation=mutation):
                f = self.fixture()
                reply = f.gateway.reply
                def changed(request, result, field=mutation):
                    if request["method"] == "session.resume":
                        result = copy.deepcopy(result)
                        if field == "generation": result["execution_generation"] = 2
                        if field == "authority": result["authority_epoch"] = 2
                        if field == "session": result["session_id"] = "foreign-session"
                        if field == "profile": result["info"]["profile_id"] = "foreign-profile"
                        if field == "yolo": result["info"]["yolo"] = True
                    reply(request, result)
                f.gateway.reply = changed
                self.open(f)
                self.confirm(f)
                f.until(lambda: "outcome unknown" in self.texts(f), "bad readback")
                self.return_to_chat(f)
                self.assertFalse(f.nodes["dock.ed"]["sendable"])
                self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
                self.assertEqual(len(f.gateway.methods("session.resume")), 1)
                self.finish(f)

    def test_running_turn_never_offers_confirmation_or_reload(self):
        f = self.fixture(running=True)
        self.open(f)
        self.assertFalse(any(n.get("text") == "Confirm profile reload and reattach" for n in f.nodes.values()))
        f.send(b"\r")
        f.quiet()
        self.assertFalse(f.gateway.methods("reload.mcp"))
        self.finish(f)


    def test_pending_reload_and_dedicated_readback_block_other_mutations_without_automatic_retry(self):
        f = self.fixture()
        f.gateway.command_mode = "valid"
        f.gateway.model_mode = "commit"
        held = []
        original = f.gateway.reply
        def hold(request, result):
            if request["method"] in ("reload.mcp", "session.resume"):
                held.append((copy.deepcopy(request), copy.deepcopy(result)))
            else:
                original(request, result)
        f.gateway.reply = hold
        self.open(f)
        self.confirm(f)
        f.until(lambda: len(held) == 1, "admitted reload held")
        self.return_to_chat(f)
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\x1bOR")
        f.quiet()
        self.assertFalse(f.gateway.methods("model.options"), "MCP pending must block model controls")
        self.return_to_chat(f)
        f.send(b"\x1bOS")
        f.until(lambda: bool(f.gateway.methods("commands.catalog")), "read-only catalog")
        f.quiet()
        self.assertFalse(f.gateway.methods("command.dispatch"))
        self.return_to_chat(f)
        request, result = held[0]
        original(request, result)
        f.until(lambda: len(held) == 2, "dedicated readback held")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        self.open(f)
        self.assertFalse(any(n.get("text") == "Confirm profile reload and reattach" for n in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
        request, result = held[1]
        original(request, result)
        f.until(lambda: "MCP reattach readback complete" in self.texts(f), "postresult reattach")
        self.return_to_chat(f)
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_transport_loss_after_reload_does_not_enqueue_reattach_or_second_reload(self):
        f = self.fixture()
        original = f.gateway.reply
        def disconnect(request, result):
            if request["method"] == "reload.mcp":
                assert f.gateway.socket is not None
                f.gateway.socket.shutdown(socket.SHUT_RDWR)
            else:
                original(request, result)
        f.gateway.reply = disconnect
        self.open(f)
        self.confirm(f)
        f.until(lambda: "outcome unknown" in self.texts(f), "transport loss")
        self.assertEqual(len(f.gateway.methods("reload.mcp")), 1)
        self.assertFalse(f.gateway.methods("session.resume"))
        self.return_to_chat(f)
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit"))


if __name__ == "__main__":
    unittest.main()
