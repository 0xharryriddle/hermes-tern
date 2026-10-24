"""Production history CLI over synthetic WS and real PTY/SDK; no real user history."""
import json
import termios
import unittest

import test_native_chat as native


def projection():
    return {"count": 400, "messages": [{"role": "assistant", "row_id": n, "text": f"history row {n}"}
                                        for n in range(300)]}


class HistoryViewerTests(unittest.TestCase):
    def fixture(self, data=None, ack=True, extra=(), ready=True):
        gateway = native.Gateway(history=projection() if data is None else data,
                                 accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False)
        f = native.NativeChat(gateway=gateway, ack=ack,
                              command=["history", "--session", "fixture-session", *extra])
        self.addCleanup(f.close)
        if ready:
            def page(remaining):
                assert not f.gateway.errors, f.gateway.errors
                f.pump(min(0.02, remaining))
                return "dock.page" in f.nodes
            native._process.wait_ready(f.proc, page)
        return f

    def readonly(self, f):
        f.gateway.thread.join(timeout=3)
        self.assertFalse(f.gateway.thread.is_alive(), "history socket still held during browsing")
        self.assertEqual([r["method"] for r in f.gateway.requests], ["ping", "runtime.describe", "session.history"])
        self.assertEqual(f.gateway.methods("session.history")[0]["params"], {"session_id": "fixture-session"})

    def test_pages_are_local_immutable_and_keyboard_pointer_do_not_dispatch(self):
        f = self.fixture()
        self.readonly(f)
        self.assertIn("Page 1/10", f.nodes["dock.page"]["text"])
        old = next(k for k,v in f.nodes.items() if v.get("text") == "Next")
        f.click(old)
        f.until(lambda: "Page 2/10" in f.nodes["dock.page"]["text"], "next page")
        f.click(old)
        f.quiet()
        self.assertIn("Page 2/10", f.nodes["dock.page"]["text"])
        f.send(b"\x1b[F")
        f.until(lambda: "Page 10/10" in f.nodes["dock.page"]["text"], "last page")
        self.assertTrue(any('history row 299' in v.get("text", "") for v in f.nodes.values()))
        self.assertFalse(any(v.get("kind") == "editor" for v in f.nodes.values()))
        f.send(b"\x1b[H")
        f.until(lambda: "Page 1/10" in f.nodes["dock.page"]["text"], "first page")
        self.readonly(f)
        f.click("dock.controls.close")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_page_up_down_use_real_sdk_sequences_without_live_control(self):
        f = self.fixture()
        f.send(b"\x1b[6~")
        f.until(lambda: "Page 2/10" in f.nodes["dock.page"]["text"], "PageDown")
        f.send(b"\x1b[5~")
        f.until(lambda: "Page 1/10" in f.nodes["dock.page"]["text"], "PageUp")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.readonly(f)

    def test_no_ack_key_flood_still_closes_without_live_control(self):
        f = self.fixture(ack=False, ready=False)
        f.until(lambda: f.surface is not None, "native surface")
        f.send(b"\x1b[C" * 80 + b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.readonly(f)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_closed_surface_preserves_same_chunk_terminal_rejection(self):
        f = self.fixture()
        raw = b"".join(b"\x1b_tsp;e;" + json.dumps(v).encode() + b"\x1b\\" for v in [
            {"ev":"gone", "sf":f.surface, "ids":[f.surface]},
            {"ev":"error", "sf":f.surface, "msg":"UNTRUSTED-HISTORY-ERROR"}])
        f.send(raw)
        self.assertEqual(f.wait_exit(), 1)
        self.readonly(f)
        f.stderr_file.flush()
        self.assertNotIn("UNTRUSTED-HISTORY-ERROR", (f.root / "client.stderr").read_text())
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_direct_tty_line_scroll_is_current_page_bound_and_backpressured(self):
        f = self.fixture()
        f.quiet()
        def scrolls():
            return [op for verb, frame in f.packets if verb == "f" for op in frame["ops"] if op[0] == "scroll"]
        f.auto_ack = False
        f.send(b"\x1b[B")
        f.until(lambda: len(scrolls()) == 1, "one local line scroll")
        self.assertEqual(scrolls()[0], ["scroll", "main.page-0", "line-down"])
        f.send(b"\x1b[B" * 100)
        f.quiet()
        self.assertEqual(len(scrolls()), 1, "unbounded raw scroll queue")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.readonly(f)

    def test_unfinished_paste_is_bounded_even_without_live_composer(self):
        f = self.fixture()
        f.send(b"\x1b[200~" + b"x" * (130 * 1024))
        self.assertEqual(f.wait_exit(), 1)
        self.readonly(f)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_plain_page_is_bounded_and_malformed_projection_refuses_without_partial_output(self):
        f = self.fixture(extra=("--plain", "--page", "10"), ready=False)
        self.assertEqual(f.wait_exit(), 0)
        self.readonly(f)
        text = f.raw.decode()
        self.assertIn("Page 10/10", text)
        self.assertIn("400 source rows", text)
        self.assertIn("300 projected rows", text)
        self.assertIn('history row 299', text)
        self.assertNotIn('history row 0"', text)
        for data in [{"count":1,"messages":["bad"]}, {"count":1,"messages":[{}]*4097}]:
            bad = self.fixture(data=data, extra=("--plain",), ready=False)
            self.assertEqual(bad.wait_exit(), 1)
            self.readonly(bad)
            self.assertFalse(bad.raw, "malformed history produced partial stdout")


if __name__ == "__main__":
    unittest.main()
