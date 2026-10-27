"""Native plugin inventory contracts; real renderer evidence is a separate gate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class PluginsSmokeTests(unittest.TestCase):
    def calls(self):
        return [request("ping", {}), request("runtime.describe", {}),
                request("client.capabilities", {"server_requests":True}),
                request("session.create", {"source":"tui","yolo":False}),
                request("commands.catalog", {"session_id":"fixture-session"}),
                request("plugins.list", {}),
                request("session.detach", {"session_id":"fixture-session","subscription_id":"fixture-sub"})]

    def test_native_scenario_explicit_read_then_draft_return_and_detach(self):
        s = module("smoke_native_chat")
        self.assertIn("plugins", s.CASES)
        text, shots = s.scenario("plugins", ["/private/hermes-tern","chat"], Path("/private/stderr"))
        self.assertEqual(shots, ["plugins", "output", "returned"])
        for step in ('click ".ht-command-entry"', 'plugins expect "Read /plugins"',
                     'click ".ht-command-read"', 'plugins expect "Owner plugins · 1 plugins"',
                     'click ".ht-command-return"', 'expect "HT_CLIENT_EXIT_0"'):
            self.assertIn(step, text)
        self.assertEqual(text.count('click ".ht-command-read"'), 1)
        self.assertEqual(s.validate_requests("plugins", self.calls(), [])["plugins.list"], 1)

    def test_plugin_trace_refuses_extra_params_duplicate_discovery_and_management(self):
        s = module("smoke_native_chat")
        self.assertIn("plugins", s.CASES)
        for extra in (self.calls()[5], request("plugins.manage", {}), request("prompt.submit", {}), request("command.dispatch", {})):
            with self.assertRaises(ValueError):
                s.validate_requests("plugins", self.calls() + [extra], [])
        for params in ({"session_id":"fixture-session"}, {"profile":"fixture-profile"}, {"force":False}):
            bad = self.calls()
            bad[5]["params"] = params
            with self.assertRaises(ValueError):
                s.validate_requests("plugins", bad, [])
        with self.assertRaises(ValueError):
            s.validate_requests("plugins", self.calls()[:5] + self.calls()[6:], [])

    def test_native_readiness_requires_complete_profile_disclosure_output_and_draft(self):
        s = module("smoke_native_chat")
        def tree(*texts):
            return {"ok":True,"count":len(texts),"nodes":[{"text":text} for text in texts]}
        fields = tree("Plugin profile: fixture-profile", "Plugin owner: fixture-instance",
                      "Profile-wide read; initial discovery may load/register plugins and refresh secret sources.", "Read /plugins")
        editor = tree("plugin draft remainsinspect")
        controls = tree("Reload command catalog","Attach text file · F5","Return to conversation","Detach")
        self.assertTrue(s.plugins_ready("plugins", fields, editor, controls))
        self.assertFalse(s.plugins_ready("plugins", tree(*[n["text"] for n in fields["nodes"][:-1]]), editor, controls))
        self.assertFalse(s.plugins_ready("plugins", fields, tree("wronginspect"), controls))
        output = tree("Owner plugins · 1 plugins · 0 toolsets · 0 portable MCP Report only, not health or execution permission. Plugin: fx [fx] v1 enabled; tools=1 hooks=0 commands=0; load error=no")
        self.assertTrue(s.plugins_ready("output", output, editor, controls))
        for wrong in (output["nodes"][0]["text"].replace("1 plugins", "0 plugins"), output["nodes"][0]["text"] + "PRIVATE"):
            self.assertFalse(s.plugins_ready("output", tree(wrong), editor, controls))
        self.assertTrue(s.plugins_ready("returned", {}, tree("plugin draft remainsinsert"), tree("Send","Detach")))

    def test_autorun_requires_each_plugin_width_theme_combination(self):
        r = module("verify_runtime")
        s = module("smoke_native_chat")
        self.assertIn("plugins", s.CASES)
        rows = [{"case":case,"size":size,"theme":theme,"status":"passed","gateway_thread_joined":True,"gateway_errors":[],
                 "selected_gateway_thread_joined":True,"selected_gateway_errors":[],"outputs":[{"image":"x"}],
                 "renderer":{"status":"passed","exit_code":0,"reaped":True},
                 "control":{"status":"passed","projections":[{"shot":"x"}]},
                 "client_exit":{"status":"exited","exit_code":0}}
                for case in s.CASES for size in ("1100x760","480x760") for theme in ("light","dark")]
        data = {"status":"passed","expected_cases":len(rows),"cases":rows}
        r.validate_native_report(data)
        bad = copy.deepcopy(data)
        bad["cases"] = [row for row in rows if row["case"] != "plugins"]
        bad["expected_cases"] = len(bad["cases"])
        with self.assertRaises(ValueError):
            r.validate_native_report(bad)


if __name__ == "__main__":
    unittest.main()
