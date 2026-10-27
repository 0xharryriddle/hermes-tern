"""Explicit owner plugin inventory through the production CLI, PTY and loopback WS."""
import copy
import json
import socket
import termios
from typing import Any
import unittest

import test_native_chat as native


def catalog() -> dict[str, Any]:
    return {"pairs":[["/plugins","Plugin inventory"]],"canon":{"/plugins":"/plugins"},
            "commands":{"/plugins":{}},"skills":{},"plugin_commands":{}}


def inventory() -> dict[str, Any]:
    return {"plugins":[{"name":"fixture-plugin","key":"agent:fixture-plugin","version":"1","enabled":True,
                         "tools":2,"hooks":1,"commands":0,"error":"PRIVATE load credential diagnostic",
                         "path":"PRIVATE local path","settings":{"token":"PRIVATE value"}}],
            "toolsets":[{"key":"fixture-tools","label":"Fixture tools","description":"Reported tools"}],
            "portable_mcp_servers":["fixture-mcp"],"config":{"PRIVATE":"not displayable"}}


class PluginsTests(unittest.TestCase):
    def fixture(self, mode="valid"):
        self.held = None
        g = native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False)
        g.command_mode = "valid"
        original = g.reply
        def reply(request, result):
            if request["method"] == "commands.catalog":
                result = catalog()
                if mode == "collision": result["plugin_commands"]["plugins"] = "PRIVATE plugin override"
            elif request["method"] == "plugins.list":
                self.assertEqual(request["params"], {}, "canonical handler accepts no session/profile parameters")
                result = inventory()
                if mode in ("held", "disconnect"):
                    self.held = copy.deepcopy(request)
                    if mode == "disconnect":
                        assert g.socket is not None
                        g.socket.shutdown(socket.SHUT_RDWR)
                    return
                if mode == "malformed": result["plugins"][0]["tools"] = True
                if mode == "wrong-variant": result = {"type":"exec","output":"PRIVATE command text"}
                if mode == "empty": result = {"plugins":[],"toolsets":[],"portable_mcp_servers":[]}
                if mode == "long":
                    result["toolsets"] = [{"key":f"group{n}","label":"Label","description":"x"*1000} for n in range(5)]
                    result["portable_mcp_servers"] = ["plugin-output-tail"]
                if mode == "unavailable":
                    g.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":4001,
                            "message":"PRIVATE diagnostic","data":{"reason":"permission_denied"}}})
                    return
            original(request, result)
        g.reply = reply
        f = native.NativeChat(gateway=g)
        self.addCleanup(f.close)
        f.ready()
        return f

    def open(self, f):
        f.click("dock.navigation.commands")
        f.until(lambda: "main.commands.page" in f.nodes, "command catalog")
        matches = [key for key, node in f.nodes.items() if node.get("text") == "Read /plugins"]
        self.assertEqual(len(matches), 1, "missing explicit owner plugin read")
        return matches[0]

    def output(self, f):
        return f.nodes.get("main.commands.output", {}).get("text", "")

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        for method in ("prompt.submit","command.dispatch","plugins.manage","config.set","session.mutate","reload.mcp"):
            self.assertFalse(f.gateway.methods(method), method)

    def test_explicit_profile_read_discloses_discovery_preserves_draft_and_never_runs_management(self):
        f = self.fixture()
        f.send("\x1b[200~plugin draft 😀\x1b[201~".encode())
        f.until(lambda: f.nodes["dock.ed"]["text"] == "plugin draft 😀", "draft")
        old = self.open(f)
        scope = f.nodes["main.commands.scope"]["text"]
        for term in ("/plugins", "profile", "discovery", "load"):
            self.assertIn(term, scope)
        f.send(b"\r"); f.quiet()
        self.assertFalse(f.gateway.methods("plugins.list"))
        f.click(old)
        f.until(lambda: "Owner plugins · 1 plugins · 1 toolsets · 1 portable MCP" in self.output(f), "plugin output")
        for term in ("fixture-plugin", "tools=2 hooks=1 commands=0", "load error=reported", "fixture-tools", "fixture-mcp"):
            self.assertIn(term, self.output(f))
        self.assertNotIn("PRIVATE", json.dumps(f.nodes))
        f.click(old); f.quiet()
        self.assertEqual(len(f.gateway.methods("plugins.list")), 1)
        f.click("dock.controls.return")
        f.until(lambda: "dock.navigation.commands" in f.nodes, "return")
        self.assertEqual(f.nodes["dock.ed"]["text"], "plugin draft 😀")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_empty_report_is_not_missing_or_unavailable_and_wrong_variants_do_not_display_raw_data(self):
        for mode in ("empty", "malformed", "unavailable", "wrong-variant"):
            with self.subTest(mode=mode):
                f = self.fixture(mode)
                f.click(self.open(f))
                if mode == "empty":
                    f.until(lambda: "No plugins reported by this owner" in self.output(f), "reported zero")
                    self.assertIn("0 plugins · 0 toolsets · 0 portable MCP", self.output(f))
                else:
                    f.until(lambda: "unavailable" in f.nodes["main.commands.status"]["text"].lower(), "plugin unavailable")
                    self.assertFalse(self.output(f))
                self.assertNotIn("PRIVATE", json.dumps(f.nodes))
                self.assertEqual(len(f.gateway.methods("plugins.list")), 1)
                self.finish(f)

    def test_plugin_command_collision_does_not_grant_builtin_read_authority(self):
        f = self.fixture("collision")
        f.click("dock.navigation.commands")
        f.until(lambda: "main.commands.page" in f.nodes, "colliding catalog")
        self.assertFalse(any(node.get("text") == "Read /plugins" for node in f.nodes.values()))
        f.send(b"\r"); f.quiet()
        self.assertFalse(f.gateway.methods("plugins.list"))
        self.finish(f)

    def test_prior_opening_and_late_generation_results_cannot_reuse_plugin_read(self):
        f = self.fixture("held")
        old = self.open(f)
        f.click("dock.controls.return")
        f.until(lambda: "dock.navigation.commands" in f.nodes, "closed")
        current = self.open(f)
        self.assertNotEqual(old, current)
        f.click(old); f.quiet()
        self.assertFalse(f.gateway.methods("plugins.list"))
        f.click(current)
        f.until(lambda: self.held is not None, "pending read")
        f.click("dock.controls.return")
        f.until(lambda: "dock.navigation.commands" in f.nodes, "return pending")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.gateway.event("session.info", {"revision":2,"yolo":False})
        f.quiet()
        assert self.held is not None
        f.gateway.send({"jsonrpc":"2.0","id":self.held["id"],"result":inventory()})
        f.click("dock.navigation.commands")
        f.until(lambda: "unavailable" in f.nodes.get("main.commands.status", {}).get("text", "").lower(), "stale result")
        self.assertFalse(self.output(f))
        self.assertEqual(len(f.gateway.methods("plugins.list")), 1)
        self.finish(f)

    def test_local_output_chunks_do_not_repeat_profile_discovery(self):
        f = self.fixture("long")
        f.click(self.open(f))
        f.until(lambda: "chunk 1/2" in f.nodes.get("main.commands.output-page", {}).get("text", ""), "first chunk")
        self.assertNotIn("plugin-output-tail", self.output(f))
        f.click("dock.controls.Next output")
        f.until(lambda: "plugin-output-tail" in self.output(f), "tail chunk")
        self.assertEqual(len(f.gateway.methods("plugins.list")), 1)
        self.finish(f)

    def test_pending_disconnect_has_no_retry_or_optimistic_send(self):
        f = self.fixture("disconnect")
        f.click(self.open(f))
        f.until(lambda: "disconnect" in f.nodes["main.commands.status"]["text"].lower(), "disconnect")
        self.assertFalse(self.output(f))
        f.send(b"\r"); f.quiet()
        self.assertEqual(len(f.gateway.methods("plugins.list")), 1)
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
