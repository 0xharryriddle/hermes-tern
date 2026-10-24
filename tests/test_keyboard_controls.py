"""Keyboard-only production client/SDK/PTY controls; synthetic gateway, not pixels."""
import json
import termios
import unittest

import test_native_chat as native
import test_transcript as transcript


class KeyboardControlTests(unittest.TestCase):
    def fixture(self, running=True):
        f = native.NativeChat(running=running)
        self.addCleanup(f.close)
        f.ready()
        return f

    @staticmethod
    def selected(f):
        rows = [(key, value) for key, value in f.nodes.items()
                if "ht-keyboard-focus" in value.get("class", "").split()]
        return rows[0] if len(rows) == 1 else None

    @staticmethod
    def positioned(f, node):
        # Positive controls wait for the positioning frame, ACKed by the fixture.
        # Merely seeing the selection view was the old, too-early condition.
        f.until(lambda: any(verb == "f" and any(op[0] == "reveal" and op[1] == node for op in frame["ops"])
                            for verb, frame in f.packets), "positioned selected control")

    def test_tab_enter_space_and_escape_inspect_without_editing_or_dispatch(self):
        f = self.fixture()
        draft = "draft tiếng Việt 😀"
        f.send(b"\x1b[200~" + draft.encode() + b"\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"]["text"] == draft, "draft")
        f.gateway.event("tool.complete", {"tool_id": "one", "name": "tool", "result_text": "x" * 2400})
        row = "tool-2-one"
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Show retained text"), "fold control")
        f.send(b"\t")
        f.until(lambda: self.selected(f) is not None, "keyboard selection")
        selection = self.selected(f)
        assert selection is not None
        self.assertEqual(selection[1]["text"], "Show retained text")
        self.positioned(f, selection[0])
        f.send(b"\r")
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Collapse retained text"), "keyboard expansion")
        selection = self.selected(f)
        assert selection is not None
        self.assertEqual(selection[1]["text"], "Collapse retained text")
        self.positioned(f, selection[0])
        f.send(b" ")
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Show retained text"), "keyboard collapse")
        f.send(b"\x1b")
        f.until(lambda: self.selected(f) is None, "return to composer")
        f.send(b"!")
        f.until(lambda: f.nodes["dock.ed"]["text"] == draft + "!", "composer restored")
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("approval.respond"))
        self.assertFalse(f.gateway.methods("session.interrupt"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_enter_before_selection_ack_does_not_activate_even_if_ack_follows_in_same_write(self):
        f = self.fixture()
        f.gateway.event("tool.complete", {"tool_id": "one", "name": "tool", "result_text": "x" * 2400})
        row = "tool-2-one"
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Show retained text"), "fold control")
        f.auto_ack = False
        f.send(b"\t")
        f.until(lambda: self.selected(f) is not None and bool(f.pending_acks), "unacknowledged selection")
        ack = f.pending_acks.pop(0)
        f.send(b"\r\x1b_tsp;e;" + json.dumps({"ev":"ack",**ack}).encode() + b"\x1b\\")
        f.quiet()
        self.assertIsNotNone(transcript.TranscriptTests.button(f, row, "Show retained text"))
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.quiet()
        f.send(b"\r")
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Collapse retained text"), "acknowledged selection")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)


    def test_replaced_selection_does_not_fall_through_to_send_and_offline_navigation_works(self):
        f = self.fixture(running=False)
        f.send(b"draft must not send")
        f.until(lambda: f.nodes["dock.ed"]["text"] == "draft must not send", "draft")
        for tool in ["one", "two"]:
            f.gateway.event("tool.complete", {"tool_id": tool, "name": "same-name", "result_text": tool * 1000})
        f.until(lambda: transcript.TranscriptTests.button(f, "tool-2-two", "Show retained text"), "two controls")
        f.send(b"\t")
        f.until(lambda: self.selected(f) is not None, "selection")
        f.gateway.event("tool.complete", {"tool_id": "one", "name": "same-name", "result_text": "replacement" * 300})
        f.until(lambda: self.selected(f) is None, "stale selection cleared visually")
        f.send(b"\r \x1b[200~ignored paste\x1b[201~")
        f.quiet()
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertEqual(f.nodes["dock.ed"]["text"], "draft must not send")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.gateway.close()
        f.until(lambda: "disconnected" in f.nodes["main.state"]["text"], "offline")
        f.send(b"\x1b[Z")
        f.until(lambda: self.selected(f) is not None, "reverse select last")
        selected = self.selected(f)
        assert selected is not None
        self.assertIn("tool-2-two", selected[0])
        self.positioned(f, selected[0])
        f.send(b"\r")
        f.until(lambda: transcript.TranscriptTests.button(f, "tool-2-two", "Collapse retained text"), "offline expansion")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_repeated_navigation_without_ack_coalesces_and_still_exits(self):
        f = self.fixture()
        f.gateway.event("tool.complete", {"tool_id": "one", "name": "tool", "result_text": "x" * 2400})
        f.until(lambda: transcript.TranscriptTests.button(f, "tool-2-one", "Show retained text"), "control")
        f.auto_ack = False
        before = len([p for p in f.packets if p[0] == "f"])
        f.send(b"\t" * 401)
        f.quiet(.5)
        self.assertLessEqual(len([p for p in f.packets if p[0] == "f"]) - before, 1)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)


if __name__ == "__main__":
    unittest.main()
