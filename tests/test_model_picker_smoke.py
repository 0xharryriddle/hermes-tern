"""Model controls must execute in the real-Tern synthetic gateway matrix."""
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


def calls():
    return [request("ping", {}), request("runtime.describe", {}),
            request("client.capabilities", {"server_requests": True}),
            request("session.create", {"source":"tui","yolo":False}),
            request("model.options", {"session_id":"fixture-session","explicit_only":True,"include_unconfigured":False,"refresh":False}),
            request("session.mutate", {"session_id":"fixture-session","request_id":"explicit-once","expected_revision":1,"expected_generation":2,"operation":"model","payload":{"model":"model-exact","provider":"provider-exact"}}),
            request("session.info", {"session_id":"fixture-session"}),
            request("session.resume", {"session_id":"fixture-session","display_projection":True}),
            request("session.detach", {"session_id":"fixture-session","subscription_id":"fixture-sub"})]


class ModelPickerSmokeTests(unittest.TestCase):
    def test_scenario_requires_select_apply_owner_readback_return_and_detach(self):
        smoke=module("smoke_native_chat")
        self.assertIn("model-picker",smoke.CASES)
        text,shots=smoke.scenario("model-picker",["/private/hermes-tern","chat"],Path("/private/stderr"))
        self.assertEqual(shots,["models","selected","applied","returned"])
        for selector in (".ht-model-entry", ".ht-model-select", ".ht-model-apply"):
            self.assertIn('click "'+selector+'"',text)
        self.assertIn('plugins expect "Active route: model-exact',text)
        smoke.validate_requests("model-picker",calls(),[])
        for method in ("prompt.submit","config.set","command.dispatch","approval.respond"):
            with self.assertRaises(ValueError):
                smoke.validate_requests("model-picker",calls()+[request(method,{})],[])
        for field in ("expected_revision","expected_generation"):
            bad=calls();bad[5]["params"][field]=999
            with self.assertRaises(ValueError):smoke.validate_requests("model-picker",bad,[])
        bad=calls();bad.insert(6,bad[5])
        with self.assertRaises(ValueError):smoke.validate_requests("model-picker",bad,[])

    def test_readiness_distinguishes_proposal_and_reconciled_owner_state(self):
        smoke=module("smoke_native_chat")
        def tree(*texts):
            return {"ok":True,"count":len(texts),"nodes":[{"text":s} for s in texts]}
        draft=tree("model draft remainsinspect")
        body=tree("Active route: old-model · old-provider", "Select model-exact")
        controls=tree("Return to conversation","Detach")
        self.assertTrue(smoke.models_ready("models",body,draft,controls))
        self.assertFalse(smoke.models_ready("applied",body,draft,controls))
        self.assertTrue(smoke.models_ready("selected",tree("Active route: old-model · old-provider"),draft,tree("Apply to this session","Return to conversation","Detach")))
        self.assertFalse(smoke.models_ready("selected",body,draft,controls))
        self.assertTrue(smoke.models_ready("applied",tree("Active route: model-exact · provider-exact","Model control reconciled with owner snapshot"),draft,controls))
        self.assertFalse(smoke.models_ready("applied",tree("Active route: model-exact · provider-exact"),draft,controls))
        self.assertTrue(smoke.models_ready("returned",{},tree("model draft remainsinsert"),tree("Send","Detach")))


if __name__=="__main__":
    unittest.main()
