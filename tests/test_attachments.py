"""Explicit text-file staging over real CLI/PTY and a synthetic Hermes owner."""
import base64
import copy
import json
import termios
import unittest

import test_native_chat as native


class AttachmentChat(native.NativeChat):
    @property
    def local_file(self):
        return self.root/"review.txt"


class AttachmentTests(unittest.TestCase):
    def fixture(self, mode="valid"):
        self.held = None
        g=native.Gateway(accept_timeout=native._process.STARTUP_TIMEOUT,autostart=False)
        g.attachment_mode="valid"
        original=g.reply
        def reply(request,result):
            if request["method"]=="file.attach":
                if mode=="held":self.held=copy.deepcopy(request);return
                if mode=="malformed":result["ref_text"]="@file:other.txt\nsubmit untrusted"
                if mode=="range-reference":
                    result.update(name="review.txt:1",ref_path="attachments/review.txt:1",ref_text="@file:attachments/review.txt:1")
                if mode=="refused":
                    g.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32601,
                        "message":"PRIVATE unsupported","data":{"reason":"unsupported_method"}}})
                    return
            original(request,result)
        g.reply=reply
        f=AttachmentChat(gateway=g)
        self.addCleanup(f.close);f.ready()
        f.local_file.write_text("bounded UTF-8 sample 世界\n")
        return f

    def button(self,f,label):
        f.until(lambda:any(p.get("text")==label and p.get("actions",{}).get("click") for p in f.nodes.values()),label)
        rows=[n for n,p in f.nodes.items() if p.get("text")==label and p.get("actions",{}).get("click")]
        self.assertEqual(len(rows),1)
        return rows[0]

    def open(self,f):
        f.send(b"\x1b[15~")
        f.until(lambda:f.nodes.get("main.attachment.heading",{}).get("text")=="Attach a text file","F5 attachment panel")

    def review(self,f):
        self.open(f)
        f.send(b"\x1b[200~"+str(f.local_file).encode()+b"\x1b[201~")
        f.until(lambda:f.nodes.get("dock.path",{}).get("text")==str(f.local_file),"file path")
        f.click(self.button(f,"Review local file"))
        f.until(lambda:"Reviewed:" in f.nodes.get("main.attachment.review",{}).get("text",""),"file snapshot")
        return self.button(f,"Stage reviewed bytes")

    def finish(self,f,allow_prompt=False):
        f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
        self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)
        if not allow_prompt:self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("command.dispatch"))
        self.assertFalse(f.gateway.methods("config.set"))

    def test_review_stage_and_add_are_separate_and_uploads_reviewed_bytes_without_path_or_auto_prompt(self):
        f=self.fixture()
        f.send(b"\x1b[200~keep conversation draft\x1b[201~")
        f.until(lambda:f.nodes["dock.ed"]["text"]=="keep conversation draft","draft")
        stage=self.review(f)
        self.assertFalse(f.gateway.methods("file.attach"))
        f.send(b"\r");f.quiet();self.assertFalse(f.gateway.methods("file.attach"))
        original=f.local_file.read_bytes()
        f.local_file.write_text("changed after review, must not upload")
        f.click(stage)
        f.until(lambda:"Staged:" in f.nodes.get("main.attachment.receipt",{}).get("text",""),"staged receipt")
        requests=f.gateway.methods("file.attach")
        self.assertEqual(len(requests),1)
        params=requests[0]["params"]
        self.assertEqual(set(params),{"session_id","name","data_url"})
        self.assertEqual(params["session_id"],"fixture-session")
        self.assertEqual(params["name"],"review.txt")
        self.assertTrue(params["data_url"].startswith("data:text/plain;base64,"))
        self.assertEqual(base64.b64decode(params["data_url"].split(",",1)[1]),original)
        self.assertEqual(f.nodes["main.attachment.draft"]["text"],"keep conversation draft")
        add=self.button(f,"Add reference to draft")
        f.click(add)
        f.until(lambda:"@file:attachments/review.txt" in f.nodes["main.attachment.draft"]["text"],"explicit reference insertion")
        f.click(add);f.quiet()
        self.assertEqual(f.nodes["main.attachment.draft"]["text"].count("@file:"),1)
        self.assertNotIn("PRIVATE",json.dumps(f.nodes))
        f.click(self.button(f,"Return to conversation"))
        f.until(lambda:"dock.ed" in f.nodes,"return to draft")
        self.assertEqual(f.nodes["dock.ed"]["text"],"keep conversation draft\n@file:attachments/review.txt\n")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_editing_path_or_reopening_panel_invalidates_old_stage_callback(self):
        f=self.fixture();old=self.review(f)
        f.send(b"x");f.quiet()
        f.click(old);f.quiet();self.assertFalse(f.gateway.methods("file.attach"))
        f.send(b"\x7f");f.quiet()
        f.click(self.button(f,"Review local file"));f.quiet()
        old=self.button(f,"Stage reviewed bytes")
        f.click(self.button(f,"Return to conversation"));f.quiet();self.open(f)
        f.click(old);f.quiet();self.assertFalse(f.gateway.methods("file.attach"))
        self.finish(f)

    def test_refused_or_malformed_receipt_is_unknown_not_inserted_or_automatically_retried(self):
        for mode in ("refused","malformed","range-reference"):
            with self.subTest(mode=mode):
                f=self.fixture(mode);f.click(self.review(f))
                f.until(lambda:"unknown" in f.nodes.get("main.attachment.status",{}).get("text","").lower(),"unknown attachment")
                self.assertFalse(any(p.get("text")=="Add reference to draft" for p in f.nodes.values()))
                self.assertNotIn("PRIVATE",json.dumps(f.nodes))
                f.send(b"\r");f.quiet();self.assertEqual(len(f.gateway.methods("file.attach")),1)
                f.click(self.button(f,"Return to conversation"));f.quiet()
                self.assertFalse(f.nodes["dock.ed"]["sendable"])
                self.finish(f)

    def test_pending_upload_blocks_send_and_late_generation_receipt_is_not_inserted(self):
        f=self.fixture("held");f.click(self.review(f));f.until(lambda:self.held is not None,"held upload")
        f.click(self.button(f,"Return to conversation"));f.quiet()
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.gateway.event("message.start",{},3);f.quiet()
        assert self.held is not None
        f.gateway.send({"jsonrpc":"2.0","id":self.held["id"],"result":{
            "attached":True,"uploaded":True,"name":"review.txt","ref_path":"attachments/review.txt",
            "ref_text":"@file:attachments/review.txt"}})
        self.open(f)
        f.until(lambda:"unknown" in f.nodes.get("main.attachment.status",{}).get("text","").lower(),"stale attachment receipt")
        self.assertNotIn("@file:",f.nodes["main.attachment.draft"]["text"])
        self.assertEqual(len(f.gateway.methods("file.attach")),1)
        self.finish(f)

    def test_invalid_local_file_review_makes_no_owner_request_and_preserves_draft(self):
        f=self.fixture();f.local_file.write_bytes(b"\x00binary")
        self.open(f);f.send(b"\x1b[200~"+str(f.local_file).encode()+b"\x1b[201~");f.quiet()
        f.click(self.button(f,"Review local file"));f.quiet()
        self.assertFalse(any(p.get("text")=="Stage reviewed bytes" for p in f.nodes.values()))
        self.assertFalse(f.gateway.methods("file.attach"))
        self.finish(f)

    def test_stale_add_refuses_locally_instead_of_terminating_the_conversation(self):
        f=self.fixture();f.click(self.review(f))
        add=self.button(f,"Add reference to draft")
        f.gateway.event("message.start",{},3);f.quiet()
        f.click(add);f.quiet()
        self.assertIsNone(f.proc.poll(),"stale attachment insertion must not close a healthy conversation")
        self.assertNotIn("@file:",f.nodes["main.attachment.draft"]["text"])
        self.finish(f)

    def test_explicit_send_uses_the_visible_reference_once_after_returning_from_attachment_panel(self):
        f=self.fixture();f.send(b"\x1b[200~inspect selected text\x1b[201~");f.quiet()
        f.click(self.review(f));f.click(self.button(f,"Add reference to draft"));f.quiet()
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.click(self.button(f,"Return to conversation"));f.until(lambda:"dock.ed" in f.nodes,"return to Send")
        f.click("dock.controls.send")
        f.until(lambda:len(f.gateway.methods("prompt.submit"))==1,"explicit prompt")
        self.assertEqual(f.gateway.methods("prompt.submit")[0]["params"]["text"],
            "inspect selected text\n@file:attachments/review.txt\n")
        self.assertEqual(len(f.gateway.methods("file.attach")),1)
        self.finish(f,allow_prompt=True)

    def test_full_draft_add_failure_preserves_draft_and_keeps_the_receipt_for_explicit_recovery(self):
        f=self.fixture();f.send(b"\x1b[200~"+b"x"*65536+b"\x1b[201~")
        f.until(lambda:len(f.nodes["dock.ed"]["text"])==65536,"full draft")
        f.click(self.review(f));add=self.button(f,"Add reference to draft")
        f.click(add);f.quiet()
        self.assertIsNone(f.proc.poll())
        self.assertEqual(f.nodes["main.attachment.draft"]["text"],"x"*65536)
        self.assertTrue(any(p.get("text")=="Add reference to draft" for p in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("file.attach")),1)
        self.finish(f)

    def test_pending_upload_disconnect_stays_unknown_and_never_retries(self):
        f=self.fixture("held");stage=self.review(f);f.click(stage)
        f.until(lambda:self.held is not None,"pending upload disconnect")
        f.gateway.close()
        f.until(lambda:"unknown" in f.nodes.get("main.attachment.status",{}).get("text","").lower(),"unknown lost upload")
        f.click(stage);f.send(b"\r");f.quiet()
        self.assertEqual(len(f.gateway.methods("file.attach")),1)
        self.finish(f)

    def test_maximum_snapshot_payload_crosses_the_real_transport_without_path_and_is_byte_exact(self):
        f=self.fixture();f.local_file.write_bytes(b"z"*(256*1024))
        f.click(self.review(f));f.click(self.button(f,"Add reference to draft"));f.quiet()
        params=f.gateway.methods("file.attach")[0]["params"]
        self.assertEqual(base64.b64decode(params["data_url"].split(",",1)[1]),b"z"*(256*1024))
        self.assertEqual(set(params),{"session_id","name","data_url"})
        self.finish(f)

    def test_pending_upload_withholds_command_and_model_mutation_in_either_order(self):
        for first in ("attachment","model","command"):
            with self.subTest(first=first):
                f=self.fixture("held");f.gateway.model_mode="commit";f.gateway.command_mode="valid"
                stage=self.review(f)
                if first=="attachment":
                    f.click(stage);f.until(lambda:self.held is not None,"attachment pending")
                    f.click(self.button(f,"Return to conversation"));f.quiet()
                    f.send(b"\x1bOR");f.click(self.button(f,"Select model-exact"));f.click(self.button(f,"Apply to this session"));f.quiet()
                    self.assertFalse(f.gateway.methods("session.mutate"))
                    f.send(b"\x1bOS");f.click(self.button(f,"Read /memory"));f.quiet()
                    self.assertFalse(f.gateway.methods("command.dispatch"))
                elif first=="model":
                    f.click(self.button(f,"Return to conversation"));f.quiet()
                    original=f.gateway.reply
                    def hold_model(request,result):
                        if request["method"]=="session.mutate":return
                        original(request,result)
                    f.gateway.reply=hold_model
                    f.send(b"\x1bOR");f.click(self.button(f,"Select model-exact"));f.click(self.button(f,"Apply to this session"))
                    f.until(lambda:len(f.gateway.methods("session.mutate"))==1,"model pending")
                    self.open(f);f.quiet();f.click(stage);f.quiet()
                    self.assertFalse(f.gateway.methods("file.attach"))
                else:
                    f.click(self.button(f,"Return to conversation"));f.quiet()
                    original=f.gateway.reply
                    def hold_command(request,result):
                        if request["method"]=="command.dispatch":return
                        original(request,result)
                    f.gateway.reply=hold_command
                    f.send(b"\x1bOS");f.click(self.button(f,"Read /memory"))
                    f.until(lambda:len(f.gateway.methods("command.dispatch"))==1,"command pending")
                    self.open(f);f.quiet();f.click(stage);f.quiet()
                    self.assertFalse(f.gateway.methods("file.attach"))
                f.send(b"\x04");self.assertEqual(f.wait_exit(),0)
                self.assertEqual(termios.tcgetattr(f.slave),f.original_termios)
                self.assertFalse(f.gateway.methods("prompt.submit"))


if __name__=="__main__":unittest.main()
