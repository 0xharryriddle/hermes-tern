"""History native acceptance contracts; execution and pixels remain separate gates."""
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class HistorySmokeTests(unittest.TestCase):
    def test_history_scenario_has_only_one_read_and_local_navigation_close(self):
        smoke = module("smoke_native_chat")
        text, shots = smoke.scenario("history", ["/private/hermes-tern", "history"], Path("/private/stderr"))
        self.assertEqual(shots, ["first", "last", "back"])
        self.assertIn('plugins expect "History projection"', text)
        self.assertIn("key End", text)
        self.assertIn("key Home", text)
        self.assertIn('key Right\nplugins expect "history row 32"', text)
        self.assertIn('key Left\nplugins expect "history row 0"', text)
        rows = [request("ping", {}), request("runtime.describe", {}),
                request("session.history", {"session_id":"fixture-session"})]
        smoke.validate_requests("history", rows, [])
        for extra in [request("session.resume", {}), request("prompt.submit", {}), rows[-1]]:
            with self.assertRaises(ValueError):
                smoke.validate_requests("history", rows + [extra], [])

    def test_history_readiness_requires_exact_page_and_no_live_editor(self):
        smoke = module("smoke_native_chat")
        good = {"ok":True,"count":1,"nodes":[{"text":"Page 1/3 · 100 source rows · 65 projected rows"}]}
        buttons = {"ok":True,"count":2,"nodes":[{"text":"Next"},{"text":"Close"}]}
        absent = {"ok":True,"count":0,"nodes":[]}
        self.assertTrue(smoke.history_ready("first", good, buttons, absent))
        for page, controls, editor in [(dict(good,count=2),buttons,absent), (good,dict(buttons,count=1),absent),
                                      (good,buttons,dict(absent,count=1,nodes=[{}])),
                                      ({"ok":True,"count":1,"nodes":[{"text":"Page 2/3"}]},buttons,absent)]:
            self.assertFalse(smoke.history_ready("first",page,controls,editor))

    def test_history_capture_requires_first_body_inside_its_scroll_region(self):
        smoke = module("smoke_native_chat")
        body = {"ok":True,"count":1,"nodes":[{"text":'{"role":"assistant","row_id":0,"text":"history row 0"}',"rect":[32,120,400,32]}]}
        scroll = {"ok":True,"count":1,"nodes":[{"rect":[32,110,416,342]}]}
        self.assertTrue(smoke.history_body_ready("first",body,scroll))
        for rect in [[32,-900,400,32],[32,450,400,32],[500,120,400,32]]:
            bad = {"ok":True,"count":1,"nodes":[{**body["nodes"][0],"rect":rect}]}
            self.assertFalse(smoke.history_body_ready("first",bad,scroll))
        self.assertFalse(smoke.history_body_ready("last",body,scroll))


if __name__ == "__main__":
    unittest.main()
