"""Native owner tool catalog: real CLI/PTY, synthetic owner, no tool execution."""
import copy
import json
import termios
import unittest

import test_native_chat as native


def tool_catalog():
    return {"sections":[{"name":"fixture-toolset","tools":[
        {"name":"fixture-search","description":"Reads bounded fixture data", "parameters":{"PRIVATE":"schema"}},
        {"name":"fixture-fetch","description":"Fetches fixture text\u001b[31m\u202e"}]}],
        "total":2,"api_key":"PRIVATE credential","config":{"PRIVATE":"not displayable"}}


class ToolCatalogTests(unittest.TestCase):
    def fixture(self, mode="valid"):
        self.held = None
        g = native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT, autostart=False)
        g.command_mode = "valid"
        original = g.reply
        def reply(request,result):
            if request["method"] == "commands.catalog":
                result = {"pairs":[["/tools","Tool catalog"]],"canon":{"/tools":"/tools"},
                    "commands":{"/tools":{}},"skills":{},"plugin_commands":{}}
            elif request["method"] == "tools.show":
                result = tool_catalog()
                if mode == "held":
                    self.held=copy.deepcopy(request)
                    return
                if mode == "malformed": result["total"] = 3
                if mode == "empty": result = {"sections":[],"total":0}
                if mode == "long":
                    result = {"sections":[{"name":"fixture-toolset","tools":[
                        {"name":f"fixture-tool-{n}","description":"x"*900 if n<5 else "tool-catalog-tail"}
                        for n in range(6)]}],"total":6}
                if mode == "unavailable":
                    g.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,
                        "message":"PRIVATE unsupported","data":{"reason":"unsupported_method"}}})
                    return
            original(request,result)
        g.reply=reply
        f=native.NativeChat(gateway=g)
        self.addCleanup(f.close)
        f.ready()
        return f

    def open(self,f):
        f.click("dock.navigation.commands")
        f.until(lambda:any(p.get("text")=="Read /tools" for p in f.nodes.values()),"explicit tools read")
        found=[n for n,p in f.nodes.items() if p.get("text")=="Read /tools"]
        self.assertEqual(len(found),1)
        return found[0]

    def output(self,f):
        return f.nodes.get("main.commands.output",{}).get("text","")

    def finish(self,f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(),0)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("command.dispatch"))
        self.assertFalse(f.gateway.methods("session.mutate"))
        self.assertFalse(f.gateway.methods("config.set"))
        self.assertFalse(f.gateway.methods("reload.mcp"))

    def test_explicit_tools_row_uses_one_session_scoped_read_not_slash_dispatch_or_execution(self):
        f=self.fixture()
        f.send(b"\x1b[200~keep tools draft\x1b[201~")
        f.until(lambda:f.nodes["dock.ed"]["text"]=="keep tools draft","draft")
        old=self.open(f)
        f.send(b"\r")
        f.quiet()
        self.assertFalse(f.gateway.methods("tools.show"))
        f.click(old)
        f.until(lambda:"Owner tool catalog · 2 reported tools" in self.output(f),"owner tool catalog")
        self.assertIn("fixture-search",self.output(f))
        self.assertIn("fixture-fetch",self.output(f))
        self.assertNotIn("PRIVATE",json.dumps(f.nodes))
        self.assertNotIn("\u001b",self.output(f))
        self.assertNotIn("\u202e",self.output(f))
        self.assertEqual([r["params"] for r in f.gateway.methods("tools.show")],[{"session_id":"fixture-session"}])
        f.click(old);f.quiet()
        self.assertEqual(len(f.gateway.methods("tools.show")),1)
        f.click("dock.controls.return")
        f.until(lambda:"dock.navigation.commands" in f.nodes,"return")
        self.assertEqual(f.nodes["dock.ed"]["text"],"keep tools draft")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_zero_report_is_visible_and_not_an_unavailable_catalog(self):
        f=self.fixture("empty")
        f.click(self.open(f))
        f.until(lambda:"No tools reported by this owner" in self.output(f),"empty owner report")
        self.assertIn("0 reported tools",self.output(f))
        self.finish(f)

    def test_unavailable_or_contradictory_report_never_fabricates_success_or_leaks_diagnostics(self):
        for mode in ("unavailable","malformed"):
            with self.subTest(mode=mode):
                f=self.fixture(mode)
                f.click(self.open(f))
                f.until(lambda:"unavailable" in f.nodes["main.commands.status"]["text"].lower(),"unsupported tool report")
                self.assertFalse(self.output(f))
                self.assertNotIn("PRIVATE",json.dumps(f.nodes))
                self.assertFalse(any(p.get("text")=="Read /tools" for p in f.nodes.values()))
                self.finish(f)

    def test_late_generation_result_is_not_accepted_and_pending_read_has_no_send(self):
        f=self.fixture("held")
        f.click(self.open(f))
        f.until(lambda:self.held is not None,"pending tools read")
        f.click("dock.controls.return")
        f.until(lambda:"dock.navigation.commands" in f.nodes,"return pending")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.gateway.event("message.start",{},3)
        f.quiet()
        assert self.held is not None
        f.gateway.send({"jsonrpc":"2.0","id":self.held["id"],"result":tool_catalog()})
        f.click("dock.navigation.commands")
        f.until(lambda:"unavailable" in f.nodes.get("main.commands.status",{}).get("text","").lower(),"stale tool catalog")
        self.assertFalse(self.output(f))
        self.assertEqual(len(f.gateway.methods("tools.show")),1)
        self.finish(f)

    def test_tool_catalog_chunks_are_local_and_do_not_reread_the_owner(self):
        f=self.fixture("long")
        f.click(self.open(f))
        f.until(lambda:"chunk 1/2" in f.nodes.get("main.commands.output-page",{}).get("text",""),"first tool chunk")
        self.assertNotIn("tool-catalog-tail",self.output(f))
        f.click("dock.controls.Next output")
        f.until(lambda:"tool-catalog-tail" in self.output(f),"last tool chunk")
        self.assertEqual(len(f.gateway.methods("tools.show")),1)
        self.finish(f)

    def test_tools_callback_from_prior_panel_opening_does_not_issue_a_read(self):
        f=self.fixture()
        old=self.open(f)
        f.click("dock.controls.return")
        f.until(lambda:"dock.navigation.commands" in f.nodes,"close tools")
        current=self.open(f)
        self.assertNotEqual(old,current)
        f.click(old);f.quiet()
        self.assertFalse(f.gateway.methods("tools.show"))
        f.click(current)
        f.until(lambda:"2 reported tools" in self.output(f),"current tools control")
        self.assertEqual(len(f.gateway.methods("tools.show")),1)
        self.finish(f)

    def test_tool_read_disconnect_does_not_retry_or_submit(self):
        f=self.fixture("held")
        f.click(self.open(f))
        f.until(lambda:self.held is not None,"pending tools disconnect")
        f.gateway.close()
        f.until(lambda:"disconnect" in f.nodes["main.commands.status"]["text"],"tools read disconnected")
        f.send(b"\r");f.quiet()
        self.assertEqual(len(f.gateway.methods("tools.show")),1)
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
