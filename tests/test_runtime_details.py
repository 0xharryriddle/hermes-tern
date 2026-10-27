"""Production native runtime inspection; synthetic owner, no provider or user state."""
import termios
import unittest

import test_native_chat as native


class RuntimeDetailsTests(unittest.TestCase):
    def fixture(self, **kwargs):
        f = native.NativeChat(**kwargs)
        self.addCleanup(f.close)
        f.ready()
        return f

    def report(self, f, **fields):
        f.gateway.event("session.info", fields)
        f.quiet()

    def open(self, f):
        f.click("dock.navigation.runtime")
        f.until(lambda: "main.runtime.profile" in f.nodes, "runtime details")

    def close(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        forbidden = ("prompt.submit", "approval.respond", "clarify.respond", "session.interrupt", "session.mutate", "command.dispatch")
        self.assertFalse(any(f.gateway.methods(method) for method in forbidden))

    def test_allowlisted_inventory_live_updates_and_return_preserve_exact_draft_without_rpc(self):
        f = self.fixture()
        f.send(b"\x1b[200~keep Unicode \xf0\x9f\x98\x80\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep Unicode 😀", "draft")
        self.report(f, profile_id="forged-profile", model="exact-model", provider="exact-provider",
                    cwd="/workspace", tools={"core": ["terminal", "read_file"]}, skills={"rust": ["safe-rust"]},
                    mcp_servers=[{"name": "catalog", "status": "connected", "error": "PRIVATE-MCP"}],
                    system_prompt="PRIVATE-SYSTEM", api_key="PRIVATE-KEY", credential_hint="PRIVATE-HINT")
        before = len(f.gateway.requests)
        self.open(f)
        self.assertEqual(f.nodes["main.runtime.profile"]["text"], "Profile: fixture-profile")
        self.assertIn("Tools: 2", f.nodes["main.runtime.counts"]["text"])
        self.assertIn("Skills: 1", f.nodes["main.runtime.counts"]["text"])
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep Unicode 😀")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        all_text = "\n".join(str(p.get("text", "")) for p in f.nodes.values())
        self.assertNotIn("PRIVATE-", all_text)
        f.event("send", sf=f.surface, id="dock.ed", text="must not submit")
        f.send(b"\rnot draft input")
        f.quiet()
        self.assertEqual(len(f.gateway.requests), before)
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep Unicode 😀")
        self.report(f, model="changed-model", tools={}, skills={}, mcp_servers=[])
        f.until(lambda: f.nodes["main.runtime.model"]["text"] == "Model: changed-model", "runtime update")
        self.assertIn("Tools: 0", f.nodes["main.runtime.counts"]["text"])
        f.click("dock.controls.return")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "conversation return")
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep Unicode 😀")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.assertEqual(len(f.gateway.requests), before)
        self.close(f)

    def test_keyboard_runtime_inspection_does_not_grant_interrupt_or_submit(self):
        f = self.fixture(running=True)
        f.quiet()
        f.send(b"\x1bOQ")  # xterm F2, decoded by the pinned SDK
        f.until(lambda: "main.runtime.profile" in f.nodes, "keyboard details")
        f.send(b"\r\x03")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "Ctrl+C returns locally")
        self.assertFalse(f.gateway.methods("session.interrupt"))
        self.close(f)

    def test_inventory_shrink_recovers_page_navigation_and_missing_sections_remain_unavailable(self):
        f = self.fixture()
        self.report(f, tools={"core": [f"tool-{n:02}" for n in range(50)]}, skills={}, mcp_servers=[])
        self.open(f)
        f.click("dock.controls.Next")
        f.until(lambda: "Inventory page 2/3" in f.nodes["main.runtime.page"]["text"], "second page")
        self.report(f, tools={}, skills=None, mcp_servers=None)
        f.until(lambda: "Inventory page 1/1" in f.nodes["main.runtime.page"]["text"], "shrunk inventory")
        self.assertIn("Skills: unavailable", f.nodes["main.runtime.counts"]["text"])
        self.report(f, tools={"core": [f"later-{n:02}" for n in range(50)]})
        f.until(lambda: "Inventory page 1/3" in f.nodes["main.runtime.page"]["text"], "page reset after shrink")
        f.click("dock.controls.Next")
        f.until(lambda: "Inventory page 2/3" in f.nodes["main.runtime.page"]["text"], "new next page")
        self.close(f)


if __name__ == "__main__":
    unittest.main()
