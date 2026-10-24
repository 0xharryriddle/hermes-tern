"""Positioning ACK is separate from view ACK; real SDK/PTY, synthetic gateway."""
import json
import unittest
import test_native_chat as native
import test_transcript as transcript


class KeyboardPositioningTests(unittest.TestCase):
    def test_selected_row_is_revealed_again_after_layout_change_and_resize(self):
        f = native.NativeChat()
        self.addCleanup(f.close)
        f.ready()
        f.gateway.event("tool.complete", {"tool_id":"one","name":"tool","result_text":"x" * 2400})
        f.until(lambda: transcript.TranscriptTests.button(f,"tool-2-one","Show retained text"),"fold control")
        def reveals():
            return [op for verb,frame in f.packets if verb == "f" for op in frame["ops"] if op[0] == "reveal"]
        f.send(b"\t")
        f.until(lambda: len(reveals()) == 1,"initial reveal")
        f.quiet()
        target = reveals()[0][1]
        f.gateway.event("tool.complete", {"tool_id":"two","name":"following","result_text":"next\n" * 100})
        f.until(lambda: transcript.TranscriptTests.button(f,"tool-2-two","Show retained text"),"appended row")
        f.until(lambda: len(reveals()) == 2,"reveal selected row after layout update")
        self.assertEqual(reveals()[-1][1],target)
        f.quiet()
        f.event("resize", cols=60)
        f.until(lambda: len(reveals()) == 3,"reveal after resize without text change")
        self.assertEqual(reveals()[-1][1],target)
        f.quiet()
        self.assertEqual(len(reveals()),3,"positioning loop")
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)

    def test_enter_cannot_use_view_ack_before_positioning_ack(self):
        f = native.NativeChat()
        self.addCleanup(f.close)
        f.ready()
        f.quiet()
        f.gateway.event("tool.complete", {"tool_id": "one", "name": "tool", "result_text": "x" * 2400})
        row = "tool-2-one"
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Show retained text"), "fold control")
        f.quiet()
        f.auto_ack = False
        f.send(b"\t")
        f.until(lambda: any("ht-keyboard-focus" in n.get("class", "").split() for n in f.nodes.values()) and f.pending_acks, "selection frame")
        f.event("ack", **f.pending_acks.pop(0))
        f.until(lambda: bool(f.pending_acks), "positioning frame")
        frame = [p[1] for p in f.packets if p[0] == "f"][-1]
        self.assertTrue(any(op[0] == "reveal" for op in frame["ops"]))
        self.assertFalse(any("ht-keyboard-ready" in n.get("class", "").split() for n in f.nodes.values()))
        # A later ACK in the same read must not make this Enter eligible.
        ack = f.pending_acks.pop(0)
        f.send(b"\r\x1b_tsp;e;" + json.dumps({"ev": "ack", **ack}).encode() + b"\x1b\\")
        f.quiet()
        self.assertIsNotNone(transcript.TranscriptTests.button(f, row, "Show retained text"), "Enter before Reveal ACK activated unseen selection")
        self.assertIsNone(transcript.TranscriptTests.button(f, row, "Collapse retained text"))
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.quiet()
        self.assertTrue(any("ht-keyboard-ready" in n.get("class", "").split() for n in f.nodes.values()))
        f.send(b"\r")
        f.until(lambda: transcript.TranscriptTests.button(f, row, "Collapse retained text"), "positive positioned Enter")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)


if __name__ == "__main__":
    unittest.main()
