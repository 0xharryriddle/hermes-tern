"""Commands native evidence contracts; actual renderer run remains separate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class CommandsSmokeTests(unittest.TestCase):
    def test_scenario_requests_exact_catalog_and_two_typed_owner_reads_no_prompt(self):
        smoke=module("smoke_native_chat")
        self.assertIn("commands",smoke.CASES)
        text,shots=smoke.scenario("commands",["/private/hermes-tern","chat"],Path("/private/stderr"))
        self.assertEqual(shots,["commands","output","tools","returned"])
        self.assertIn('click ".ht-command-entry"',text)
        self.assertIn('click ".ht-command-read"',text)
        self.assertIn('plugins expect "fixture-owner-memory"',text)
        self.assertIn('click ".ht-command-list>div:nth-child(3)>.ht-command-read"',text)
        self.assertIn('plugins expect "Owner tool catalog · 2 reported tools"',text)
        rows=[request("ping",{}),request("runtime.describe",{}),request("client.capabilities",{"server_requests":True}),
              request("session.create",{"source":"tui","yolo":False}),request("commands.catalog",{"session_id":"fixture-session"}),
              request("command.dispatch",{"session_id":"fixture-session","name":"memory","arg":""}),
              request("tools.show",{"session_id":"fixture-session"}),
              request("session.detach",{"session_id":"fixture-session","subscription_id":"fixture-sub"})]
        self.assertEqual(smoke.validate_requests("commands",rows,[])["command.dispatch"],1)
        self.assertEqual(smoke.validate_requests("commands",rows,[])["tools.show"],1)
        for extra in [request("prompt.submit",{}),request("session.mutate",{}),rows[5],rows[6]]:
            with self.assertRaises(ValueError):smoke.validate_requests("commands",rows+[extra],[])
        bad=copy.deepcopy(rows);bad[5]["params"]["arg"]="approval off"
        with self.assertRaises(ValueError):smoke.validate_requests("commands",bad,[])
        for params in ({"session_id":"different"},{"session_id":"fixture-session","tool":"fixture-search"}):
            bad=copy.deepcopy(rows);bad[6]["params"]=params
            with self.assertRaises(ValueError):smoke.validate_requests("commands",bad,[])

    def test_exact_body_draft_mode_and_control_readiness_are_required(self):
        smoke=module("smoke_native_chat")
        def tree(*texts):return {"ok":True,"count":len(texts),"nodes":[{"text":s} for s in texts]}
        controls=tree("Reload command catalog","Attach text file · F5","Return to conversation","Detach")
        body=tree("Owner commands","Read /memory","Read /skills","Read /tools")
        self.assertTrue(smoke.commands_ready("commands",body,tree("command draft remainsinspect"),controls))
        self.assertFalse(smoke.commands_ready("commands",body,tree("command draft remaininspect"),controls))
        self.assertTrue(smoke.commands_ready("output",tree("fixture-owner-memory"),tree("command draft remainsinspect"),controls))
        self.assertFalse(smoke.commands_ready("output",tree("fixture-owner-memor"),tree("command draft remainsinspect"),controls))
        expected=("Owner tool catalog · 2 reported tools "
                  "Catalog entries are data, not execution permission or health checks. "
                  "Group: fx Tool: fx-read Description: Read Tool: fx-list Description: List")
        self.assertLessEqual(len(expected),200)
        self.assertTrue(smoke.commands_ready("tools",tree(expected),tree("command draft remainsinspect"),controls))
        for wrong in (expected.replace("2 reported","3 reported"),expected.replace("fx-list","fx-execute"),expected+"PRIVATE",expected[:-1]):
            self.assertFalse(smoke.commands_ready("tools",tree(wrong),tree("command draft remainsinspect"),controls))
        self.assertFalse(smoke.commands_ready("commands",tree("Owner commands","Read /memory"),tree("command draft remainsinspect"),controls))
        self.assertTrue(smoke.commands_ready("returned",{},tree("command draft remainsinsert"),tree("Send","Detach")))
        self.assertFalse(smoke.commands_ready("returned",{},tree("command draft remainsinspect"),tree("Send","Detach")))


    def test_all_navigation_entries_fit_one_visible_row_without_relaxing_main_geometry(self):
        smoke=module("smoke_native_chat")
        entries=["ht-runtime-entry","ht-model-entry","ht-command-entry"]
        data={"elements":[{"path":f"div.ht-navigation>button.ht-button.{token}",
                           "visible":True,"rect":[16+i*140,640,132,24]} for i,token in enumerate(entries)]}
        smoke.validate_navigation_geometry(data,480,760)
        for change in ("wrap","offscreen","missing","duplicate","hidden","nonfinite"):
            with self.subTest(change=change):
                bad=copy.deepcopy(data)
                if change=="wrap":bad["elements"][2]["rect"][1]=680
                if change=="offscreen":bad["elements"][2]["rect"][0]=470
                if change=="missing":bad["elements"].pop()
                if change=="duplicate":bad["elements"].append(copy.deepcopy(bad["elements"][2]))
                if change=="hidden":bad["elements"][2]["visible"]=False
                if change=="nonfinite":bad["elements"][2]["rect"][0]=float("nan")
                with self.assertRaises(ValueError):smoke.validate_navigation_geometry(bad,480,760)

    def test_tool_result_must_be_complete_inside_the_scroll_panel_not_merely_in_the_tree(self):
        smoke=module("smoke_native_chat")
        panel={"ok":True,"count":1,"nodes":[{"rect":[32,74,416,418]}]}
        body={"ok":True,"count":1,"nodes":[{"rect":[32,202,416,128]}]}
        self.assertTrue(smoke.command_output_visible(body,panel,480,760))
        for change in ("cropped","offscreen","missing","duplicate","hidden","nonfinite"):
            with self.subTest(change=change):
                bad=copy.deepcopy(body);bad_panel=copy.deepcopy(panel)
                if change=="cropped":bad["nodes"][0]["rect"][1]=394
                if change=="offscreen":bad_panel["nodes"][0]["rect"][0]=470
                if change=="missing":bad["nodes"]=[];bad["count"]=0
                if change=="duplicate":bad["nodes"].append(copy.deepcopy(bad["nodes"][0]));bad["count"]=2
                if change=="hidden":bad["nodes"][0]["rect"][3]=0
                if change=="nonfinite":bad["nodes"][0]["rect"][3]=float("nan")
                self.assertFalse(smoke.command_output_visible(bad,bad_panel,480,760))


if __name__=="__main__":unittest.main()
