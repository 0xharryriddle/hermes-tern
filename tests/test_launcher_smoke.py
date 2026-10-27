"""Native launcher matrix contracts; actual Tern execution remains separate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class LauncherSmokeTests(unittest.TestCase):
    def test_launcher_control_matrix_requires_inventory_then_one_explicit_selection(self):
        smoke = module("smoke_native_chat")
        for case in ("launcher-close", "launcher-new", "launcher-resume"):
            text, shots = smoke.scenario(case,["/private/hermes-tern","open"],Path("/private/stderr"))
            self.assertNotIn("type ",text)
            self.assertNotIn("key Enter",text)
            self.assertIn("HT_CLIENT_EXIT_0",text)
            self.assertEqual(shots,["launcher"] if case == "launcher-close" else ["launcher","opened"])
            rows = [request("ping",{}),request("runtime.describe",{}),
                    request("session.list",{"scope":"live","limit":200}),
                    request("session.list",{"scope":"stored","limit":200})]
            if case != "launcher-close":
                rows += [request("ping",{}),request("runtime.describe",{}),
                         request("client.capabilities",{"server_requests":True}),
                         request("session.create",{"source":"tui","yolo":False}) if case == "launcher-new" else
                         request("session.resume",{"session_id":"saved-exact","display_projection":True}),
                         request("session.detach",{"session_id":"fixture-session","subscription_id":"fixture-sub"})]
            smoke.validate_requests(case,rows,[])
            for extra in [request("prompt.submit",{}),request("approval.respond",{}),rows[-1]]:
                with self.assertRaises(ValueError):
                    smoke.validate_requests(case,rows+[extra],[])

    def test_launcher_capture_requires_exact_scope_controls_and_no_composer(self):
        smoke = module("smoke_native_chat")
        count={"ok":True,"count":1,"nodes":[{"text":"live · 2 listed · page 1/1"}]}
        controls={"ok":True,"count":5,"nodes":[{"text":t} for t in ("New conversation","live sessions","stored sessions","Profiles","Close")]}
        editor={"ok":True,"count":0,"nodes":[]}
        self.assertTrue(smoke.launcher_ready("launcher-close",count,controls,editor))
        for bad in [dict(count,count=0),dict(count,nodes=[{"text":"stored · 2 listed · page 1/1"}])]:
            self.assertFalse(smoke.launcher_ready("launcher-close",bad,controls,editor))
        self.assertFalse(smoke.launcher_ready("launcher-close",count,controls,dict(editor,count=1,nodes=[{}])))
        changed=copy.deepcopy(controls)
        changed["nodes"][0]["text"]="Send"
        self.assertFalse(smoke.launcher_ready("launcher-close",count,changed,editor))


if __name__ == "__main__":
    unittest.main()
