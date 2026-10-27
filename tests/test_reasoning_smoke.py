"""Native reasoning scenario contracts; real renderer qualification is separate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class ReasoningSmokeTests(unittest.TestCase):
    def test_reasoning_scenario_expands_locally_and_never_submits(self):
        smoke = module("smoke_native_chat")
        self.assertIn("reasoning", smoke.CASES)
        text, shots = smoke.scenario("reasoning", ["/private/hermes-tern", "chat"], Path("/private/stderr"))
        self.assertEqual(shots, ["collapsed", "expanded", "refolded"])
        self.assertIn('plugins expect "Show reasoning"', text)
        self.assertIn('plugins expect "Hide reasoning"', text)
        self.assertNotIn('key Enter', text)
        rows = [request("ping", {}), request("runtime.describe", {}),
                request("client.capabilities", {"server_requests": True}),
                request("session.create", {"source": "tui", "yolo": False}),
                request("session.detach", {"session_id": "fixture-session", "subscription_id": "fixture-sub"})]
        self.assertEqual(smoke.validate_requests("reasoning", rows, [])["prompt.submit"], 0)
        with self.assertRaises(ValueError):
            smoke.validate_requests("reasoning", rows + [request("prompt.submit", {})], [])

    def test_exact_reasoning_body_and_hidden_state_draft_and_controls_are_required(self):
        smoke = module("smoke_native_chat")
        def tree(*texts):
            return {"ok": True, "count": len(texts), "nodes": [{"text": text} for text in texts]}
        draft = tree("reasoning draft remainsinsert")
        controls = tree("Send", "Detach")
        answer = tree("Retained answer.")
        self.assertTrue(smoke.reasoning_ready("collapsed", tree(), tree("Show reasoning"), answer, draft, controls))
        self.assertTrue(smoke.reasoning_ready("expanded", tree("Check the parser first."), tree("Hide reasoning"), answer, draft, controls))
        self.assertFalse(smoke.reasoning_ready("collapsed", tree("Check the parser first."), tree("Show reasoning"), answer, draft, controls))
        for invalid in (tree("Check the parser firs"), tree("Check the parser first. EXTRA"), tree()):
            self.assertFalse(smoke.reasoning_ready("expanded", invalid, tree("Hide reasoning"), answer, draft, controls))
        self.assertFalse(smoke.reasoning_ready("expanded", tree("Check the parser first."), tree("Hide reasoning"), answer, tree("wrong"), controls))
        bad = copy.deepcopy(controls)
        bad["count"] = 1
        self.assertFalse(smoke.reasoning_ready("expanded", tree("Check the parser first."), tree("Hide reasoning"), answer, draft, bad))


if __name__ == "__main__":
    unittest.main()
