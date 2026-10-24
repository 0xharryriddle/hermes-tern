"""Long-output autorun contract; actual renderer invocation remains a separate gate."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class LongOutputSmokeTests(unittest.TestCase):
    def test_long_output_scenario_and_receipts_are_inspection_only(self):
        smoke = module("smoke_native_chat")
        text, shots = smoke.scenario("long-output", ["/private/hermes-tern", "chat"], Path("/private/stderr"))
        self.assertEqual(shots, ["folded", "expanded", "refolded"])
        self.assertEqual(text.count('click ".ht-transcript-toggle"'), 2)
        self.assertNotIn("key Enter", text)
        self.assertIn("HT_CLIENT_EXIT_0", text)
        rows = [request("session.create", {"source": "tui", "yolo": False}),
                request("session.detach", {"session_id": "fixture-session", "subscription_id": "fixture-sub"})]
        smoke.validate_requests("long-output", rows, [])
        for method in ["prompt.submit", "approval.respond", "clarify.respond", "session.interrupt"]:
            with self.assertRaises(ValueError):
                smoke.validate_requests("long-output", rows + [request(method, {})], [])

    def test_long_output_readiness_checks_exact_text_and_toggle(self):
        smoke = module("smoke_native_chat")
        prefix = " ".join(smoke.LONG_TEXT.splitlines()[:12])
        tree = {"ok": True, "count": 1, "nodes": [{"text": prefix}]}
        buttons = {"ok": True, "count": 1, "nodes": [{"text": "Show retained text"}]}
        for shot in ["folded", "refolded"]:
            self.assertTrue(smoke.long_output_ready(shot, tree, buttons))
            bad = copy.deepcopy(tree)
            bad["nodes"][0]["text"] = " ".join(smoke.LONG_TEXT.split())
            self.assertFalse(smoke.long_output_ready(shot, bad, buttons))
        full = {"ok": True, "count": 1, "nodes": [{"text": " ".join(smoke.LONG_TEXT.split())}]}
        self.assertLess(len(full["nodes"][0]["text"]), 192, "fixture would exceed real tree text cap")
        collapse = {"ok": True, "count": 1, "nodes": [{"text": "Collapse retained text"}]}
        self.assertTrue(smoke.long_output_ready("expanded", full, collapse))
        self.assertFalse(smoke.long_output_ready("expanded", tree, collapse))
        self.assertFalse(smoke.long_output_ready("expanded", full, buttons))
        truncated = copy.deepcopy(full)
        truncated["nodes"][0]["text"] = truncated["nodes"][0]["text"][:-1]
        self.assertFalse(smoke.long_output_ready("expanded", truncated, collapse))
        for malformed in [{"ok": False}, {"ok": True, "count": 0, "nodes": []},
                          {"ok": True, "count": 2, "nodes": full["nodes"] * 2}]:
            self.assertFalse(smoke.long_output_ready("expanded", malformed, collapse))


if __name__ == "__main__":
    unittest.main()
