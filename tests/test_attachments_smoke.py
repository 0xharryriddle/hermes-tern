"""Native attachment acceptance must review/stage/add explicitly, never send."""
import base64
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module,request


class AttachmentSmokeTests(unittest.TestCase):
    def calls(self):
        return [request("ping",{}),request("runtime.describe",{}),
            request("client.capabilities",{"server_requests":True}),
            request("session.create",{"source":"tui","yolo":False}),
            request("file.attach",{"session_id":"fixture-session","name":"review.txt",
                "data_url":"data:text/plain;base64,"+base64.b64encode(b"native attachment\n").decode()}),
            request("session.detach",{"session_id":"fixture-session","subscription_id":"fixture-sub"})]

    def test_scenario_has_separate_local_review_owner_stage_reference_add_and_detach(self):
        s=module("smoke_native_chat")
        self.assertIn("attachments",s.CASES)
        text,shots=s.scenario("attachments",["/private/hermes-tern","chat"],Path("/private/stderr"))
        self.assertEqual(shots,["reviewed","staged","added","returned"])
        for step in ('key F5','click ".ht-attachment-review"','click ".ht-attachment-stage"',
                     'click ".ht-attachment-add"','click ".ht-attachment-return"'):
            self.assertIn(step,text)
        self.assertIn('type "/private/review.txt"',text)
        self.assertNotIn('click ".ht-controls>button:first-child"',text)
        self.assertIn('expect "HT_CLIENT_EXIT_0"',text)
        self.assertEqual(s.validate_requests("attachments",self.calls(),[])["file.attach"],1)
        for extra in (self.calls()[4],request("prompt.submit",{}),request("session.mutate",{}),request("command.dispatch",{})):
            with self.assertRaises(ValueError):s.validate_requests("attachments",self.calls()+[extra],[])
        for field,value in (("session_id","other"),("name","other.txt"),("path","/private/source"),("data_url","data:text/plain;base64,YmFk")):
            bad=self.calls();bad[4]["params"][field]=value
            with self.assertRaises(ValueError):s.validate_requests("attachments",bad,[])

    def test_readiness_requires_exact_review_or_staged_receipt_and_unchanged_or_explicitly_added_draft(self):
        s=module("smoke_native_chat")
        def tree(*texts):return {"ok":True,"count":len(texts),"nodes":[{"text":t} for t in texts]}
        editor=tree("/private/review.txtinspect")
        draft=tree("attachment draft remains")
        controls=tree("Review local file","Stage reviewed bytes","Return to conversation","Detach")
        body=tree("Reviewed: review.txt · 18 bytes")
        self.assertTrue(s.attachments_ready("reviewed",body,draft,editor,controls,"/private/review.txt"))
        self.assertFalse(s.attachments_ready("staged",body,draft,editor,controls,"/private/review.txt"))
        staged=tree("Staged: review.txt · attachments/review.txt")
        controls=tree("Review local file","Add reference to draft","Return to conversation","Detach")
        self.assertTrue(s.attachments_ready("staged",staged,draft,editor,controls,"/private/review.txt"))
        self.assertFalse(s.attachments_ready("staged",staged,tree("changed"),editor,controls,"/private/review.txt"))
        added=tree("attachment draft remains @file:attachments/review.txt")
        controls=tree("Review local file","Return to conversation","Detach")
        self.assertTrue(s.attachments_ready("added",staged,added,editor,controls,"/private/review.txt"))
        self.assertFalse(s.attachments_ready("added",staged,draft,editor,controls,"/private/review.txt"))
        self.assertTrue(s.attachments_ready("returned",{}, {},tree("attachment draft remains @file:attachments/review.txt insert"),tree("Send","Detach"),"/private/review.txt"))
        self.assertFalse(s.attachments_ready("returned",{}, {},editor,tree("Send","Detach"),"/private/review.txt"))

    def test_review_waits_for_exact_path_and_current_control_before_the_single_click(self):
        s=module("smoke_native_chat")
        def tree(*texts):return {"ok":True,"count":len(texts),"nodes":[{"text":t} for t in texts]}
        editor=tree("/private/review.txtinspect")
        controls=tree("Review local file","Return to conversation","Detach")
        self.assertTrue(s.attachment_path_ready(editor,controls,"/private/review.txt"))
        for bad in (tree("/private/review.txinspect"),tree("/private/review.txtinsert"),tree()):
            self.assertFalse(s.attachment_path_ready(bad,controls,"/private/review.txt"))
        self.assertFalse(s.attachment_path_ready(editor,tree("Return to conversation","Detach"),"/private/review.txt"))


if __name__=="__main__":unittest.main()
