"""Profile native-matrix contracts; cannot substitute for actual renderer execution."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class ProfilePickerSmokeTests(unittest.TestCase):
    def test_native_profile_scenario_uses_explicit_roster_selection_then_separate_new(self):
        smoke = module("smoke_native_chat")
        self.assertIn("profile-picker",smoke.CASES)
        text, shots = smoke.scenario("profile-picker",["/private/hermes-tern","open"],Path("/private/stderr"))
        self.assertEqual(shots,["profiles","switched","opened"])
        self.assertIn('click ".ht-profiles-entry"',text)
        self.assertIn('click ".ht-profile-list>div:last-child>button"',text)
        self.assertIn('click ".ht-launch-controls>button:first-child"',text)
        self.assertIn('expect "HT_CLIENT_EXIT_0"',text)
        inventory = [request("ping",{}),request("runtime.describe",{}),request("session.list",{"scope":"live","limit":200}),request("session.list",{"scope":"stored","limit":200})]
        first = inventory + [request("profiles.list",{"include_sessions":False})]
        second = inventory + [request("ping",{}),request("runtime.describe",{}),request("client.capabilities",{"server_requests":True}),request("session.create",{"source":"tui","yolo":False}),request("session.detach",{"session_id":"fixture-session","subscription_id":"fixture-sub"})]
        self.assertEqual(smoke.validate_profile_requests(first,second,[])["profiles.list"],1)
        for a,b in [(first+[request("session.create",{})],second),(first,second+[request("prompt.submit",{})]),(first,second[:-1])]:
            with self.assertRaises(ValueError):smoke.validate_profile_requests(a,b,[])
        changed=copy.deepcopy(first);changed[-1]["params"]["include_sessions"]=True
        with self.assertRaises(ValueError):smoke.validate_profile_requests(changed,second,[])

    def test_profile_readiness_requires_exact_home_visible_controls_and_no_composer(self):
        smoke = module("smoke_native_chat")
        def tree(*texts):return {"ok":True,"count":len(texts),"nodes":[{"text":t} for t in texts]}
        fields=tree("Current profile: /private/current","2 profiles · page 1/1 · 12 per page","/private/current","/private/selected")
        controls=tree("Back to sessions","Close")
        self.assertTrue(smoke.profiles_ready(fields,controls,tree(),"/private/current","/private/selected"))
        self.assertFalse(smoke.profiles_ready(fields,controls,tree("composer"),"/private/current","/private/selected"))
        self.assertFalse(smoke.profiles_ready(fields,controls,tree(),"/other","/private/selected"))
        self.assertFalse(smoke.profiles_ready(fields,tree("Back to sessions","Send"),tree(),"/private/current","/private/selected"))


    def test_profile_layout_checks_live_buttons_not_an_empty_reused_container(self):
        smoke = module("smoke_native_chat")
        rows = [{"path":"div.ht-launch-controls","rect":[0,0,50,0],"visible":False}]
        rows += [{"path":"div.ht-launch-controls>button.ht-launch-button","rect":[n*60,20,50,25],"visible":True} for n in range(5)]
        rows += [{"path":"div.ht-launch-row>button.ht-launch-button","rect":[0,80,50,25],"visible":True}]
        smoke.validate_profile_geometry({"elements":rows},"switched",480,760)
        rows[1]["rect"][1]=-1
        with self.assertRaises(ValueError):smoke.validate_profile_geometry({"elements":rows},"switched",480,760)


if __name__ == "__main__":unittest.main()
