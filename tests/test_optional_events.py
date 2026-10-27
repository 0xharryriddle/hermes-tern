"""Unknown owner events cannot erase human controls in the production native loop."""
import termios
import unittest

import test_native_chat as native


class OptionalEventTests(unittest.TestCase):
    def fixture(self, **kwargs):
        f = native.NativeChat(**kwargs)
        self.addCleanup(f.close)
        f.ready()
        return f

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        for method in ("prompt.submit", "session.mutate", "command.dispatch", "session.interrupt"):
            self.assertFalse(f.gateway.methods(method), method)

    def test_unknown_future_generation_preserves_exact_human_deny_control(self):
        f = self.fixture(running=True)
        f.send(b"\x1b[200~keep this draft\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep this draft", "draft")
        f.gateway.event("approval.request", {
            "kind": "approval", "prompt_id": "current-human", "execution_generation": 2,
            "choices": ["deny", "once"], "command": "review current operation",
        })
        def deny_nodes():
            return [name for name, props in f.nodes.items()
                    if "prompt-" in name and name.endswith("choice0")
                    and props.get("actions", {}).get("click")]
        f.until(lambda: len(deny_nodes()) == 1, "current human deny")
        original = deny_nodes()[0]
        f.gateway.event("future.optional", {"running": False, "revision": 999}, 999)
        f.quiet()
        self.assertIn(original, deny_nodes(), "unknown event erased the current human denial authority")
        self.assertFalse(f.gateway.methods("approval.respond"))
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep this draft")
        f.click(original)
        f.until(lambda: len(f.gateway.methods("approval.respond")) == 1, "explicit original deny")
        self.assertEqual(f.gateway.methods("approval.respond")[0]["params"], {
            "session_id": "fixture-session", "execution_generation": 2,
            "prompt_id": "current-human", "choice": "deny",
        })
        self.finish(f)

    def test_unknown_future_generation_does_not_suppress_following_current_status(self):
        f = self.fixture()
        f.send(b"\x1b[200~unchanged draft\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "unchanged draft", "draft")
        f.gateway.event("session.usage", {"usage": {"total": 7}}, 999)
        f.gateway.event("status.update", {"text": "known-after-optional"}, 2)
        f.until(lambda: any("known-after-optional" in str(node.get("text", ""))
                            for node in f.nodes.values()), "known event after optional sequence")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.assertEqual(f.nodes["dock.ed"]["text"], "unchanged draft")
        self.assertFalse(f.gateway.methods("session.resume"))
        self.assertFalse(f.gateway.methods("approval.respond"))
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
