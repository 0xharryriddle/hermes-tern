"""Production native commands; synthetic owner, no provider/personal configuration."""
import copy
import json
import termios
import unittest
import test_native_chat as native


def catalog():
    return {"pairs":[["/memory","Memory inspection"],["/skills","Skill inspection"],["/yolo","PRIVATE config"],
                      *[[f"/unknown-{n}","PRIVATE plugin body"] for n in range(12)]],
            "canon":{"/memory":"/memory","/skills":"/skills","/yolo":"/yolo"},
            "commands":{"/memory":{},"/skills":{},"/yolo":{}},"skills":{},"plugin_commands":{},
            "warning":"PRIVATE warning","api_key":"PRIVATE secret"}


class CommandTests(unittest.TestCase):
    held: dict | None = None
    held_model: dict | None = None
    def fixture(self, mode="valid"):
        g=native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT,autostart=False)
        g.command_mode="valid"
        original=g.reply
        def reply(request,result):
            if request["method"]=="commands.catalog":
                result=catalog()
                if mode=="canonical-skills":
                    result["pairs"].extend([["/plugin:review","x"*1038],["/小说拆条","Skill"]])
                    result["skills"].update({"/plugin:review":{},"/小说拆条":{}})
                if mode=="malformed":result["pairs"].append(result["pairs"][0])
                if mode=="unavailable":
                    g.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,"message":"PRIVATE unavailable","data":{"reason":"unsupported_method"}}})
                    return
            elif request["method"]=="command.dispatch":
                if mode=="directive":result={"type":"send","message":"PRIVATE must-not-submit"}
                if mode=="held":self.held=copy.deepcopy(request);return
                if mode=="long":result={"type":"exec","output":"x"*4096+"owner-tail"}
            elif request["method"]=="session.mutate" and mode=="model-held":
                self.held_model=copy.deepcopy(request)
                return
            original(request,result)
        g.reply=reply
        f=native.NativeChat(gateway=g);self.addCleanup(f.close);f.ready();return f

    def open(self,f):
        f.click("dock.navigation.commands")
        f.until(lambda:f.nodes.get("main.commands.heading",{}).get("text")=="Owner commands","command panel")
        f.until(lambda:bool(f.gateway.methods("commands.catalog")),"command catalog request")

    def read_node(self,f):
        nodes=[n for n,p in f.nodes.items() if p.get("text")=="Read /memory"]
        f.until(lambda:any(p.get("text")=="Read /memory" for p in f.nodes.values()),"memory read")
        nodes=[n for n,p in f.nodes.items() if p.get("text")=="Read /memory"]
        self.assertEqual(len(nodes),1);return nodes[0]

    def close(self,f):
        f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
        self.assertEqual(len(f.gateway.methods("session.detach")),1)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("session.mutate"))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)

    def test_f4_and_pointer_accept_owner_namespaced_skills_without_dispatching_them(self):
        for keyboard in (True,False):
            with self.subTest(keyboard=keyboard):
                f=self.fixture("canonical-skills")
                f.send(b"\x1b[200~preserve draft\x1b[201~")
                f.until(lambda:f.nodes.get("dock.ed",{}).get("text")=="preserve draft","draft")
                if keyboard:f.send(b"\x1bOS")
                else:f.click("dock.navigation.commands")
                node=self.read_node(f)
                self.assertFalse(any(p.get("text")=="Command catalog unavailable or malformed" for p in f.nodes.values()))
                self.assertFalse(f.gateway.methods("command.dispatch"))
                f.click(node)
                f.until(lambda:f.nodes.get("main.commands.output",{}).get("text")=="fixture-owner-memory","read output")
                self.assertEqual(f.nodes["dock.ed"]["text"],"preserve draft")
                self.assertEqual(len(f.gateway.methods("commands.catalog")),1)
                self.assertEqual(len(f.gateway.methods("command.dispatch")),1)
                self.close(f)

    def test_explicit_catalog_and_pointer_read_keep_draft_and_never_auto_submit(self):
        f=self.fixture();self.assertFalse(f.gateway.methods("commands.catalog"))
        f.send(b"\x1b[200~keep Unicode \xf0\x9f\x98\x80\x1b[201~")
        f.until(lambda:f.nodes["dock.ed"]["text"]=="keep Unicode 😀","draft")
        self.open(f);node=self.read_node(f)
        self.assertEqual(f.gateway.methods("commands.catalog")[0]["params"],{"session_id":"fixture-session"})
        self.assertFalse(f.gateway.methods("command.dispatch"))
        f.send(b"\rnot draft\x03")
        f.until(lambda:"dock.navigation.commands" in f.nodes,"local Ctrl+C return")
        self.open(f);node=self.read_node(f);f.click(node)
        f.until(lambda:f.nodes.get("main.commands.output",{}).get("text")=="fixture-owner-memory","owner output")
        self.assertEqual([r["params"] for r in f.gateway.methods("command.dispatch")],[{"session_id":"fixture-session","name":"memory","arg":""}])
        self.assertEqual(f.nodes["dock.ed"]["text"],"keep Unicode 😀")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        self.assertNotIn("PRIVATE",json.dumps(f.nodes))
        f.click(node);f.quiet();self.assertEqual(len(f.gateway.methods("command.dispatch")),1)
        f.click("dock.controls.return");f.until(lambda:"dock.navigation.commands" in f.nodes,"return to draft")
        self.assertEqual(f.nodes["dock.ed"]["text"],"keep Unicode 😀");self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.close(f)

    def test_unavailable_or_malformed_catalog_preserves_healthy_chat_and_no_raw_diagnostics(self):
        for mode in ("unavailable","malformed"):
            with self.subTest(mode=mode):
                f=self.fixture(mode);self.open(f)
                f.until(lambda:"unavailable" in f.nodes.get("main.commands.status",{}).get("text","").lower(),"unavailable")
                self.assertNotIn("PRIVATE",json.dumps(f.nodes));self.assertFalse(f.gateway.methods("command.dispatch"))
                f.click("dock.controls.return");f.until(lambda:"dock.navigation.commands" in f.nodes,"return")
                self.assertTrue(f.nodes["dock.ed"]["sendable"]);self.close(f)

    def test_reload_and_offpage_old_read_callbacks_cannot_dispatch(self):
        f=self.fixture();self.open(f);old=self.read_node(f)
        f.click("dock.controls.Next");f.until(lambda:"page 2/2" in f.nodes["main.commands.page"]["text"],"second page")
        f.click(old);f.quiet();self.assertFalse(f.gateway.methods("command.dispatch"))
        f.click("dock.controls.Previous");f.until(lambda:"page 1/2" in f.nodes["main.commands.page"]["text"],"first page")
        f.click("dock.controls.reload");f.until(lambda:len(f.gateway.methods("commands.catalog"))==2,"reload")
        new=self.read_node(f);self.assertNotEqual(new,old);f.click(old);f.quiet();self.assertFalse(f.gateway.methods("command.dispatch"))
        self.close(f)

    def test_owner_send_directive_is_unknown_not_a_prompt_or_retryable_read(self):
        f=self.fixture("directive");self.open(f);f.click(self.read_node(f))
        f.until(lambda:"no directive" in f.nodes.get("main.commands.status",{}).get("text",""),"directive refusal")
        self.assertNotIn("PRIVATE",json.dumps(f.nodes));self.assertFalse(f.gateway.methods("prompt.submit"))
        f.click("dock.controls.return");f.until(lambda:"dock.navigation.commands" in f.nodes,"return")
        self.assertFalse(f.nodes["dock.ed"]["sendable"]);self.close(f)

    def test_pending_read_blocks_send_and_late_new_generation_output_is_not_accepted(self):
        f=self.fixture("held");self.open(f);f.click(self.read_node(f));f.until(lambda:self.held is not None,"held read")
        f.click("dock.controls.return");f.until(lambda:"dock.navigation.commands" in f.nodes,"return pending")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.gateway.event("message.start",{},3);f.quiet()
        assert self.held is not None
        f.gateway.send({"jsonrpc":"2.0","id":self.held["id"],"result":{"type":"exec","output":"PRIVATE late-owner-output"}})
        self.open(f);f.until(lambda:"no directive" in f.nodes["main.commands.status"]["text"],"stale result")
        self.assertNotIn("PRIVATE",json.dumps(f.nodes));self.close(f)

    def test_owner_output_chunks_page_without_another_rpc(self):
        f=self.fixture("long");self.open(f);f.click(self.read_node(f))
        f.until(lambda:"chunk 1/2" in f.nodes.get("main.commands.output-page",{}).get("text",""),"output first chunk")
        f.click("dock.controls.Next output");f.until(lambda:f.nodes["main.commands.output"]["text"]=="owner-tail","tail chunk")
        self.assertEqual(len(f.gateway.methods("command.dispatch")),1);self.close(f)


    def test_keyboard_f4_is_inspection_and_read_disconnect_never_retries(self):
        f=self.fixture("held");f.send(b"\x1bOS")
        f.until(lambda:"main.commands.heading" in f.nodes,"keyboard F4")
        f.send(b"\r")
        f.quiet();self.assertFalse(f.gateway.methods("command.dispatch"))
        f.click(self.read_node(f));f.until(lambda:self.held is not None,"read dispatched")
        f.gateway.close()
        f.until(lambda:"disconnect" in f.nodes["main.commands.status"]["text"],"read loss")
        f.send(b"\r");f.quiet();self.assertEqual(len(f.gateway.methods("command.dispatch")),1)
        f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)


    def test_prior_opening_read_callback_is_inert_after_close_and_reopen_without_reload(self):
        f=self.fixture();self.open(f);old=self.read_node(f)
        f.click("dock.controls.return");f.until(lambda:"dock.navigation.commands" in f.nodes,"close panel")
        self.open(f);current=self.read_node(f)
        f.click(old);f.quiet()
        self.assertFalse(f.gateway.methods("command.dispatch"),"prior panel incarnation reused command callback authority")
        self.assertNotEqual(old,current)
        self.assertEqual(len(f.gateway.methods("commands.catalog")),1,"reopening is local,not an implicit catalog read")
        f.click(current)
        f.until(lambda:f.nodes.get("main.commands.output",{}).get("text")=="fixture-owner-memory","current explicit read")
        self.assertEqual(len(f.gateway.methods("command.dispatch")),1)
        self.close(f)

    def model_button(self,f,text):
        f.until(lambda:any(p.get("text")==text for p in f.nodes.values()),text)
        nodes=[n for n,p in f.nodes.items() if p.get("text")==text and p.get("actions",{}).get("click")]
        self.assertEqual(len(nodes),1)
        return nodes[0]

    def test_command_read_and_model_change_cannot_overlap_in_either_order(self):
        for first in ("command","model"):
            with self.subTest(first=first):
                self.held=None
                f=self.fixture("held" if first=="command" else "model-held")
                f.gateway.model_mode="commit"
                self.open(f);read=self.read_node(f)
                if first=="command":
                    f.click(read);f.until(lambda:self.held is not None,"pending command")
                    f.send(b"\x1bOR");f.click(self.model_button(f,"Select model-exact"))
                    f.click(self.model_button(f,"Apply to this session"));f.quiet()
                    self.assertFalse(f.gateway.methods("session.mutate"))
                else:
                    f.send(b"\x1bOR");f.click(self.model_button(f,"Select model-exact"))
                    f.click(self.model_button(f,"Apply to this session"))
                    f.until(lambda:self.held_model is not None,"pending model")
                    f.send(b"\x1bOS");f.click(self.read_node(f));f.quiet()
                    self.assertFalse(f.gateway.methods("command.dispatch"))
                f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
                self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)
                self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_deferred_model_receipt_must_settle_before_owner_command_read(self):
        f=self.fixture();f.gateway.model_mode="deferred"
        self.open(f);self.read_node(f)
        f.send(b"\x1bOR");f.click(self.model_button(f,"Select model-exact"))
        f.click(self.model_button(f,"Apply to this session"))
        f.until(lambda:any("deferred by Hermes" in p.get("text","") for p in f.nodes.values()),"deferred model")
        f.send(b"\x1bOS");f.click(self.read_node(f));f.quiet()
        self.assertFalse(f.gateway.methods("command.dispatch"),"deferred owner selection must settle before handler/hook reads")
        f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)


if __name__=="__main__":unittest.main()
