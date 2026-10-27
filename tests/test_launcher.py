"""Explicit native launcher through production CLI/SDK and synthetic owner only."""
import json
import termios
import unittest

import test_native_chat as native


def inventory():
    row = {"session_id":"fixture-session", "title":"Duplicate title", "model":"fixture-model", "running":False}
    return {"live":{"scope":"live","sessions":[row]},
            "stored":{"scope":"stored","sessions":[dict(row, session_id="saved-exact")]}}


class LauncherTests(unittest.TestCase):
    def fixture(self, catalog=None, ack=True, ready=True):
        g = native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False,
                           catalog=inventory() if catalog is None else catalog)
        f = native.NativeChat(gateway=g, ack=ack, command=["open"])
        self.addCleanup(f.close)
        if ready:
            def opened(remaining):
                f.pump(min(0.02, remaining))
                return "main.controls.new" in f.nodes
            native._process.wait_ready(f.proc, opened)
        return f

    def assert_only_inventory(self, f):
        self.assertEqual([r["method"] for r in f.gateway.requests],
                         ["ping","runtime.describe","session.list","session.list"])
        self.assertEqual([r["params"] for r in f.gateway.methods("session.list")],
                         [{"scope":"live","limit":200},{"scope":"stored","limit":200}])

    def close(self, f):
        f.click("main.controls.close")
        self.assertEqual(f.wait_exit(),0,(f.root/"client.stderr").read_text())
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_listing_and_close_never_attach_create_or_submit(self):
        f = self.fixture()
        self.assert_only_inventory(f)
        self.assertNotIn("dock.ed",f.nodes)
        self.assertIn("200",f.nodes["dock.notice"]["text"])
        f.send(b"\t")
        f.until(lambda:"stored" in f.nodes["main.count"]["text"],"stored list")
        self.close(f)
        self.assert_only_inventory(f)

    def test_new_keeps_owner_socket_and_only_opens_after_human_click(self):
        f = self.fixture()
        self.assert_only_inventory(f)
        f.click("main.controls.new")
        f.until(lambda:"dock.ed" in f.nodes,"chat opened")
        create = f.gateway.methods("session.create")
        self.assertEqual(len(create),1)
        self.assertIs(create[0]["params"]["yolo"],False)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("session.resume"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)
        self.assertEqual(len(f.gateway.methods("session.detach")),1)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_stored_resume_uses_exact_id_not_duplicate_title(self):
        f = self.fixture()
        f.click("main.controls.stored")
        f.until(lambda:"main.list-stored-0.entry-0.resume" in f.nodes,"stored exact action")
        f.click("main.list-stored-0.entry-0.resume")
        f.until(lambda:"dock.ed" in f.nodes,"resumed composer")
        self.assertEqual(f.gateway.methods("session.resume")[0]["params"],
                         {"session_id":"saved-exact","display_projection":True})
        self.assertFalse(f.gateway.methods("session.create"))
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)

    def test_early_keyboard_enter_and_no_ack_cannot_create_or_resume(self):
        f = self.fixture(ack=False)
        f.send(b"\r\x1b[B\r\x1b[A\r")
        f.quiet()
        self.assert_only_inventory(f)
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)
        self.assert_only_inventory(f)

    def test_malformed_inventory_fails_without_session_mutation_or_untrusted_diagnostics(self):
        data = inventory()
        data["stored"]["sessions"][0]["session_id"] = "UNTRUSTED\u001bBAD"
        f = self.fixture(catalog=data,ready=False)
        self.assertEqual(f.wait_exit(),1)
        self.assert_only_inventory(f)
        self.assertNotIn("UNTRUSTED",(f.root/"client.stderr").read_text())
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_error_and_surface_loss_fence_queued_new(self):
        f = self.fixture()
        events = [{"ev":"action","sf":f.surface,"id":"main.controls.new","act":"click","mods":[]},
                  {"ev":"gone","sf":f.surface,"ids":[f.surface]},
                  {"ev":"error","sf":f.surface,"msg":"UNTRUSTED-LAUNCHER"}]
        f.send(b"".join(b"\x1b_tsp;e;"+json.dumps(v).encode()+b"\x1b\\" for v in events))
        self.assertEqual(f.wait_exit(),1)
        self.assert_only_inventory(f)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)
    def test_disconnected_inventory_cannot_open_a_session_and_restores_tty(self):
        f = self.fixture()
        f.gateway.close()
        self.assertEqual(f.wait_exit(),1)
        self.assert_only_inventory(f)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_keyboard_new_after_ack_uses_exact_displayed_workspace(self):
        f = self.fixture()
        f.quiet()
        f.send(b"\r")
        f.until(lambda:"dock.ed" in f.nodes,"keyboard new")
        self.assertEqual(f.gateway.methods("session.create")[0]["params"]["cwd"],str(native.ROOT))
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)
    def test_keyboard_resume_requires_positioning_ack_not_just_drawn_selection(self):
        f = self.fixture()
        f.quiet()
        f.send(b"\x1b[B")
        f.until(lambda:"ht-launch-selected" in f.nodes["main.list-live-0.entry-0"].get("class", ""),"selected row")
        f.quiet()
        reveals = [op for verb, body in f.packets if verb == "f" for op in body["ops"]
                   if op[0] == "reveal" and op[1] == "main.list-live-0.entry-0"]
        self.assertTrue(reveals,"keyboard selection was never revealed")
        f.send(b"\r")
        f.until(lambda:"dock.ed" in f.nodes,"keyboard resumed")
        self.assertEqual(f.gateway.methods("session.resume")[0]["params"]["session_id"],"fixture-session")
        self.assertFalse(f.gateway.methods("session.create"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)


    def test_terminal_rejection_queued_after_selection_prevents_session_open(self):
        for scope in ("current", "global"):
            with self.subTest(scope=scope):
                f = self.fixture()
                error = {"ev":"error","msg":"UNTRUSTED-AFTER-SELECTION"}
                if scope == "current":
                    error["sf"] = str(f.surface)
                events = [{"ev":"action","sf":f.surface,"id":"main.controls.new","act":"click","mods":[]},error]
                f.send(b"".join(b"\x1b_tsp;e;"+json.dumps(v).encode()+b"\x1b\\" for v in events))
                f.until(lambda:f.proc.poll() is not None or bool(f.gateway.methods("session.create")),"selection or rejection")
                if f.proc.poll() is None:
                    f.send(b"\x04")
                self.assertEqual(f.wait_exit(),1,"selection discarded queued terminal rejection")
                self.assert_only_inventory(f)
                self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)


    def test_layout_change_invalidates_keyboard_new_until_fresh_positioning_ack(self):
        for ev,params in (("resize",{"cols":60}),("theme",{"dark":True}),("motion",{"reduce":True})):
            with self.subTest(event=ev):
                f = self.fixture()
                f.quiet()
                f.auto_ack=False
                f.event(ev,**params)
                f.send(b"\r")
                f.quiet()
                self.assertFalse(f.gateway.methods("session.create"),"layout change reused stale New positioning")
                f.send(b"\x04")
                self.assertEqual(f.wait_exit(),0)


if __name__ == "__main__":
    unittest.main()
