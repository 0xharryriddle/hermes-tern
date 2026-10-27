"""Existing profile choice over production SDK/PTY; roster is not authority."""
import json
import socket
import termios
import unittest
from unittest.mock import patch
import test_native_chat as native
import test_launcher as launcher


class ProfilePickerTests(unittest.TestCase):
    def fixture(self, mode="valid", ack=True):
        g = native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False, catalog=launcher.inventory())
        g.profile_id = "/fixture-profile"
        g.profiles = {"profiles":[{"name":"default","path":"/fixture-profile","display_name":"Main"},
                    {"name":"other","path":"/fixture-other","display_name":"Other fixture"}]}
        original = g.reply
        def reply(request, result):
            if request["method"] == "profiles.list":
                assert g.profiles is not None
                result = g.profiles
                if mode == "disconnect":
                    assert g.socket is not None
                    g.socket.shutdown(socket.SHUT_RDWR)
                    return
                if mode == "unavailable":
                    g.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,"message":"PRIVATE-UNAVAILABLE","data":{"reason":"unsupported_method"}}})
                    return
                if mode == "malformed":result["profiles"][1]["path"] = "/fixture/../other"
            original(request, result)
        g.reply = reply
        f = native.NativeChat(gateway=g, ack=ack, command=["open"])
        self.addCleanup(f.close)
        native._process.wait_ready(f.proc, lambda remaining: (f.pump(min(0.02,remaining)) or "main.controls.new" in f.nodes))
        return f

    def open_profiles(self, f):
        f.click("main.controls.profiles")
        f.until(lambda:f.nodes.get("main.heading",{}).get("text") == "Hermes profiles · existing owners only", "profile roster")
        self.assertEqual(f.gateway.methods("profiles.list")[0]["params"], {"include_sessions":False})
        self.assertFalse(f.gateway.methods("session.create"))
        self.assertFalse(f.gateway.methods("session.resume"))

    def close_picker(self, f):
        f.click("dock.controls.close")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_roster_is_explicit_read_only_and_back_keeps_same_owner(self):
        f = self.fixture()
        self.assertFalse(f.gateway.methods("profiles.list"))
        self.open_profiles(f)
        self.assertIn("/fixture-other", f.nodes["main.profiles-0.entry-1.home"]["text"])
        f.click("dock.controls.back")
        f.until(lambda:"main.controls.new" in f.nodes, "back to sessions")
        self.assertEqual(len(f.gateway.methods("profiles.list")), 1)
        self.assertEqual(len(f.gateway.methods("session.list")), 4)
        closes = [body for verb,body in f.packets if verb == "x" and body.get("id") == "hermes-tern-profiles"]
        self.assertEqual(len(closes),1)
        self.assertIs(closes[0].get("keep"),False,"profile picker retained behind next surface")
        f.click("main.controls.close")
        self.assertEqual(f.wait_exit(), 0)
        self.assertFalse(f.gateway.methods("session.create"))

    def test_unavailable_or_malformed_roster_is_not_empty_success_and_preserves_back_close(self):
        for mode in ("unavailable","malformed"):
            with self.subTest(mode=mode):
                f = self.fixture(mode)
                self.open_profiles(f)
                self.assertIn("unavailable",f.nodes["main.notice"]["text"].lower())
                self.assertFalse(any(key.endswith(".select") for key in f.nodes))
                self.assertNotIn("PRIVATE-",json.dumps(f.nodes))
                self.close_picker(f)

    def test_terminal_rejection_queued_after_profiles_prevents_inventory_rpc(self):
        f = self.fixture()
        events = [{"ev":"action","sf":f.surface,"id":"main.controls.profiles","act":"click","mods":[]},
                  {"ev":"error","sf":f.surface,"msg":"PRIVATE-QUEUED-PROFILE-ERROR"}]
        f.send(b"".join(b"\x1b_tsp;e;" + json.dumps(value).encode() + b"\x1b\\" for value in events))
        self.assertEqual(f.wait_exit(), 1)
        self.assertFalse(f.gateway.methods("profiles.list"))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def target(self, f, wrong_grant=False):
        home = f.root / "existing-other"
        home.mkdir(mode=0o700)
        sentinel = home / "config-not-read"
        sentinel.write_text("private scratch sentinel; not runtime configuration")
        g = native.Gateway(accept_timeout=60, autostart=False, catalog=launcher.inventory())
        g.profile_id = "/wrong-grant" if wrong_grant else str(home)
        g.instance_id = "second-fixture-instance"
        self.addCleanup(g.close)
        assert f.gateway.profiles is not None
        f.gateway.profiles["profiles"][1]["path"] = str(home)
        trace = f.root / "selected-discovery.json"
        grant = {"url":f"ws://127.0.0.1:{g.port}/api/ws",
                 "protocols":["hermes-gateway-v1","hermes-gateway-ticket.synthetic-other"],
                 "profile_id":g.profile_id,"instance_id":g.instance_id}
        bootstrap = f.root / "runtime/ui-tui/scripts/gateway_bootstrap.py"
        bootstrap.write_text("import os,json\nfrom pathlib import Path\ndef bootstrap(start,recover):\n"
            "    assert (start,recover)==(False,False)\n"
            f"    assert os.environ['HERMES_HOME']=={str(home)!r}\n"
            f"    Path({str(trace)!r}).write_text(json.dumps({{'home':os.environ['HERMES_HOME'],'start':start,'recover':recover}}))\n"
            f"    return {grant!r}\n")
        g.thread.start()
        return g, home, trace, sentinel

    def test_switch_mints_selected_profile_grant_and_requires_separate_new_action(self):
        f = self.fixture()
        other, home, trace, sentinel = self.target(f)
        before = sentinel.stat(), sentinel.read_bytes()
        self.open_profiles(f)
        f.click("main.profiles-0.entry-1.select")
        f.until(lambda:f.nodes.get("main.profile",{}).get("text") == "Qualified profile: " + str(home), "selected owner's session picker")
        self.assertEqual(json.loads(trace.read_text()),{"home":str(home),"start":False,"recover":False})
        self.assertEqual([r["method"] for r in other.requests],["ping","runtime.describe","session.list","session.list"])
        self.assertFalse(f.gateway.methods("session.create"))
        assert f.gateway.socket is not None
        self.assertEqual(f.gateway.socket.fileno(),-1,"previous socket still open")
        f.click("main.controls.new")
        f.until(lambda:"dock.ed" in f.nodes, "new on selected owner")
        self.assertEqual(len(other.methods("session.create")),1)
        self.assertFalse(other.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)
        self.assertEqual(len(other.methods("session.detach")),1)
        self.assertEqual(before,(sentinel.stat(),sentinel.read_bytes()))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_wrong_profile_grant_refuses_before_connecting_or_mutating_either_owner(self):
        f = self.fixture()
        other, _, trace, _ = self.target(f,wrong_grant=True)
        self.open_profiles(f)
        f.click("main.profiles-0.entry-1.select")
        self.assertEqual(f.wait_exit(),1)
        self.assertTrue(trace.exists())
        self.assertIsNone(other.socket)
        self.assertFalse(other.requests)
        self.assertFalse(f.gateway.methods("session.create"))
        self.assertIn("selected profile differs from grant",(f.root / "client.stderr").read_text())
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_profile_choice_followed_by_terminal_error_cannot_start_fresh_discovery(self):
        f = self.fixture()
        other, _, trace, _ = self.target(f)
        self.open_profiles(f)
        events = [{"ev":"action","sf":f.surface,"id":"main.profiles-0.entry-1.select","act":"click","mods":[]},
                  {"ev":"error","sf":f.surface,"msg":"PRIVATE-SELECTION-ERROR"}]
        f.send(b"".join(b"\x1b_tsp;e;" + json.dumps(value).encode() + b"\x1b\\" for value in events))
        self.assertEqual(f.wait_exit(),1)
        self.assertFalse(trace.exists())
        self.assertIsNone(other.socket)
        self.assertFalse(f.gateway.methods("session.create"))

    def test_current_profile_enter_without_positioning_ack_is_inert(self):
        f = self.fixture()
        self.open_profiles(f)
        f.quiet()
        f.auto_ack = False
        f.send(b"\x1b[B\r")
        f.quiet()
        self.assertEqual(f.nodes["main.heading"]["text"],"Hermes profiles · existing owners only")
        self.assertFalse(f.gateway.methods("session.create"))
        self.close_picker(f)


    def test_missing_native_handshake_refuses_before_discovery_or_owner_connection(self):
        g = native.Gateway(accept_timeout=60,autostart=False,catalog=launcher.inventory())
        with patch.object(native.NativeChat,"packet",lambda f,verb,body:f.packets.append([verb,body])):
            f = native.NativeChat(gateway=g,command=["open"])
            self.addCleanup(f.close)
            self.assertEqual(f.wait_exit(),1)
        self.assertIsNone(g.socket,"discovery opened an owner socket before native negotiation")
        self.assertFalse(g.requests)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)


    def test_profile_resize_requires_fresh_positioning_ack_before_keyboard_switch(self):
        f = self.fixture()
        _, _, trace, _ = self.target(f)
        self.open_profiles(f)
        f.send(b"\x1b[B")
        f.until(lambda:"ht-profile-selected" in f.nodes["main.profiles-0.entry-1"].get("class",""),"target selected")
        f.quiet()
        f.auto_ack=False
        f.event("resize",cols=60)
        f.send(b"\r")
        f.quiet()
        self.assertFalse(trace.exists(),"resize reused stale profile positioning authority")
        self.assertEqual(f.nodes["main.heading"]["text"],"Hermes profiles · existing owners only")
        self.close_picker(f)


    def test_transport_loss_during_roster_read_does_not_open_a_false_back_available_picker(self):
        f = self.fixture("disconnect")
        f.click("main.controls.profiles")
        self.assertEqual(f.wait_exit(),1)
        self.assertFalse(any(verb=="o" and body.get("id")=="hermes-tern-profiles" for verb,body in f.packets))
        self.assertFalse(f.gateway.methods("session.create"))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)


if __name__ == "__main__":
    unittest.main()
