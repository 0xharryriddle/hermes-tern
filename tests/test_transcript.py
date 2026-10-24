"""Local folding through the real CLI/SDK/PTY; synthetic gateway, not live acceptance."""
import termios
import unittest

import test_native_chat as native


class TranscriptTests(unittest.TestCase):
    def fixture(self):
        f = native.NativeChat(running=True)
        self.addCleanup(f.close)
        f.ready()
        return f

    @staticmethod
    def button(f, row, choice):
        found = [key for key, value in f.nodes.items()
                 if row in key and value.get("text") == choice and value.get("actions", {}).get("click")]
        return found[0] if len(found) == 1 else None

    @staticmethod
    def body(f, row):
        found = [value.get("text") for key, value in f.nodes.items() if key.endswith(row + ".body")]
        return found[0] if len(found) == 1 else None

    def test_fold_same_name_tools_preserves_draft_and_never_dispatches(self):
        f = self.fixture()
        draft = "draft tiếng Việt 😀"
        f.send(b"\x1b[200~" + draft.encode() + b"\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == draft, "retained draft")
        values = {}
        for identity in ["a", "b"]:
            summary = identity * 2600 + "\nend-" + identity
            values[identity] = "finished: same-name\n" + summary
            f.gateway.event("tool.complete", {"tool_id": identity, "name": "same-name", "result_text": summary})
        f.until(lambda: all(self.button(f, "tool-2-" + i, "Show retained text") for i in values), "folded controls")
        before = len(f.gateway.requests)
        for identity, expected in values.items():
            row = "tool-2-" + identity
            prefix = self.body(f, row)
            assert isinstance(prefix, str), "missing retained preview"
            self.assertTrue(expected.startswith(prefix))
            self.assertLessEqual(len(prefix.encode()), 1024)
            old_show = self.button(f, row, "Show retained text")
            f.click(old_show)
            f.until(lambda: self.body(f, row) == expected, "expanded retained text")
            f.click(old_show)  # stale/duplicate show cannot become Collapse
            f.quiet()
            self.assertEqual(self.body(f, row), expected)
            if identity == "b":
                previous = self.body(f, "tool-2-a")
                assert isinstance(previous, str), "missing earlier row"
                self.assertLessEqual(len(previous.encode()), 1024)
            self.assertEqual(f.nodes["dock.ed"]["text"], draft)
        f.click(self.button(f, "tool-2-b", "Collapse retained text"))
        f.until(lambda: self.button(f, "tool-2-b", "Show retained text"), "collapsed again")
        self.assertEqual(len(f.gateway.requests), before, "local navigation dispatched upstream")
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("approval.respond"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_truncated_stream_remains_inspectable_after_disconnect(self):
        f = self.fixture()
        f.send(b"keep draft")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep draft", "draft")
        for _ in range(3):
            f.gateway.event("message.delta", {"text": "x" * 30000})
            f.quiet()
        row = "reply-observed-0"
        f.until(lambda: any(key.endswith(row + ".truncated") for key in f.nodes), "row truncation notice")
        f.click(self.button(f, row, "Show retained text"))
        f.until(lambda: len(self.body(f, row) or "") == 65536, "retained prefix inspection")
        f.gateway.close()
        f.until(lambda: "disconnected" in f.nodes.get("main.state", {}).get("text", ""), "disconnect state")
        before = len(f.gateway.requests)
        f.click(self.button(f, row, "Collapse retained text"))
        f.until(lambda: self.button(f, row, "Show retained text"), "offline collapse")
        f.click(self.button(f, row, "Show retained text"))
        f.until(lambda: len(self.body(f, row) or "") == 65536, "offline inspect")
        self.assertEqual(len(f.gateway.requests), before)
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep draft")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_old_show_cannot_expand_same_id_replacement_while_credit_is_blocked(self):
        f = self.fixture()
        row = "tool-2-same"
        f.gateway.event("tool.start", {"tool_id": "same", "name": "tool", "summary": "a" * 2600})
        f.until(lambda: self.button(f, row, "Show retained text"), "original show control")
        old_show = self.button(f, row, "Show retained text")
        f.auto_ack = False
        f.send(b"retained draft")
        f.until(lambda: bool(f.pending_acks), "occupied credit")
        f.gateway.event("tool.complete", {"tool_id": "same", "name": "tool", "result_text": "b" * 2600})
        f.quiet()
        f.click(old_show)
        f.quiet()
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: "bbbb" in (self.body(f, row) or ""), "replacement row")
        f.quiet()
        body = self.body(f, row)
        assert isinstance(body, str)
        self.assertLessEqual(len(body.encode()), 1024, "old Show expanded replacement text")
        current = self.button(f, row, "Show retained text")
        self.assertIsNotNone(current)
        self.assertNotEqual(current, old_show)
        f.click(current)
        f.until(lambda: len(self.body(f, row) or "") > 2600, "current replacement expands")
        self.assertEqual(f.nodes["dock.ed"]["text"], "retained draft")
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)


if __name__ == "__main__":
    unittest.main()
