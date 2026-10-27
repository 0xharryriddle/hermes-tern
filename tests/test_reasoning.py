"""Production PTY/SDK reasoning blocks; synthetic owner, no provider."""
import termios
import unittest
import test_native_chat as native


class ReasoningTests(unittest.TestCase):
    def fixture(self, history=False):
        g = native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False)
        reply = g.reply
        def project(request, result):
            if request["method"] == "session.create" and history:
                result["messages"] = [{"row_id": 2, "role": "assistant", "text": "Retained answer.",
                                       "reasoning_content": "Check the parser first."}]
            reply(request, result)
        g.reply = project
        f = native.NativeChat(gateway=g)
        self.addCleanup(f.close)
        f.ready()
        return f

    def toggle(self, f, text):
        f.until(lambda: any(p.get("text") == text for p in f.nodes.values()), text)
        rows = [n for n, p in f.nodes.items() if p.get("text") == text]
        self.assertEqual(len(rows), 1)
        return rows[0]

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        for method in ("prompt.submit", "command.dispatch", "session.mutate", "config.set"):
            self.assertFalse(f.gateway.methods(method))
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_resumed_short_reasoning_opens_and_closes_without_rpc_or_losing_draft(self):
        f = self.fixture(history=True)
        f.send(b"\x1b[200~keep Unicode \xf0\x9f\x98\x80\x1b[201~")
        f.until(lambda: f.nodes.get("dock.ed", {}).get("text") == "keep Unicode 😀", "draft")
        toggle = self.toggle(f, "Show reasoning")
        self.assertFalse(any(p.get("text") == "Check the parser first." for p in f.nodes.values()))
        f.click(toggle)
        f.until(lambda: any(p.get("text") == "Check the parser first." for p in f.nodes.values()), "reasoning body")
        f.click(self.toggle(f, "Hide reasoning"))
        f.until(lambda: not any(p.get("text") == "Check the parser first." for p in f.nodes.values()), "collapsed body")
        self.assertTrue(any(p.get("text") == "Retained answer." for p in f.nodes.values()))
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep Unicode 😀")
        self.finish(f)

    def test_fallback_reasoning_is_retained_for_each_tool_separated_response(self):
        f = self.fixture()
        f.gateway.event("message.start", {})
        f.gateway.event("reasoning.available", {"text": "Before tool"})
        self.toggle(f, "Show reasoning")
        f.gateway.event("tool.start", {"tool_id": "t", "name": "read_file"})
        f.gateway.event("tool.complete", {"tool_id": "t", "name": "read_file"})
        f.gateway.event("reasoning.available", {"text": "After tool"})
        f.gateway.event("message.complete", {"text": "Done."})
        f.until(lambda: any(p.get("text") == "Done." for p in f.nodes.values()), "answer")
        self.assertEqual(sum(p.get("text") == "Show reasoning" for p in f.nodes.values()), 2)
        toggles = [n for n, p in f.nodes.items() if p.get("text") == "Show reasoning"]
        f.click(toggles[-1])
        f.until(lambda: any(p.get("text") == "After tool" for p in f.nodes.values()), "second reasoning")
        self.finish(f)

    def test_streamed_reasoning_stays_expanded_and_whitespace_deltas_remain_contiguous(self):
        f = self.fixture()
        f.gateway.event("message.start", {})
        f.gateway.event("reasoning.delta", {"text": "Check"})
        f.click(self.toggle(f, "Show reasoning"))
        f.until(lambda: any(p.get("text") == "Check" for p in f.nodes.values()), "expanded stream")
        f.gateway.event("reasoning.delta", {"text": " "})
        f.gateway.event("reasoning.delta", {"text": "first."})
        f.until(lambda: any(p.get("text") == "Check first." for p in f.nodes.values()), "continued reasoning")
        f.gateway.event("reasoning.available", {"text": "must not duplicate"})
        f.gateway.event("message.complete", {"text": "Final answer."})
        f.until(lambda: any(p.get("text") == "Final answer." for p in f.nodes.values()), "answer")
        self.assertFalse(any("must not duplicate" in str(p.get("text", "")) for p in f.nodes.values()))
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
