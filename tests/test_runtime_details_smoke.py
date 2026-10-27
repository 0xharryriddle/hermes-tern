"""Runtime panel must be exercised by the actual Tern matrix, not just unit render."""
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class RuntimeDetailsSmokeTests(unittest.TestCase):
    def test_runtime_scenario_opens_local_panel_returns_draft_and_detaches(self):
        smoke = module("smoke_native_chat")
        self.assertIn("runtime-details", smoke.CASES)
        text, shots = smoke.scenario("runtime-details", ["/private/hermes-tern", "chat"], Path("/private/stderr"))
        self.assertEqual(shots, ["runtime", "delegated", "plan", "returned"])
        self.assertIn('click ".ht-runtime-entry"', text)
        self.assertIn('plugins expect "Profile: fixture-profile"', text)
        self.assertIn('click ".ht-delegated-read"', text)
        self.assertIn('plugins expect "1 reported delegated agents"', text)
        self.assertIn('click ".ht-plan-entry"', text)
        self.assertIn('plugins expect "1 reported tasks"', text)
        self.assertIn('expect "HT_CLIENT_EXIT_0"', text)
        rows = [request("ping", {}), request("runtime.describe", {}),
                request("client.capabilities", {"server_requests": True}),
                request("session.create", {"source": "tui", "yolo": False}),
                request("subagent.list", {"session_id": "fixture-session"}),
                request("session.detach", {"session_id": "fixture-session", "subscription_id": "fixture-sub"})]
        smoke.validate_requests("runtime-details", rows, [])
        for method in ("prompt.submit", "session.mutate", "approval.respond", "command.dispatch", "subagent.steer", "subagent.interrupt", "subagent.tail", "process.list"):
            with self.assertRaises(ValueError):
                smoke.validate_requests("runtime-details", rows + [request(method, {})], [])
        with self.assertRaises(ValueError):
            smoke.validate_requests("runtime-details", rows[:-2] + rows[-1:], [])
        bad = [dict(row) for row in rows]
        bad[-2] = request("subagent.list", {"session_id": "other", "profile": "other"})
        with self.assertRaises(ValueError):
            smoke.validate_requests("runtime-details", bad, [])

    def test_runtime_readiness_requires_exact_identity_inventory_draft_and_no_send(self):
        smoke = module("smoke_native_chat")
        def tree(*texts):
            return {"ok": True, "count": len(texts), "nodes": [{"text": s} for s in texts]}
        fields = tree("Profile: fixture-profile", "Model: fixture-model", "Tools: 2 · Skills: 1 · MCP servers: 1")
        editor = tree("runtime draft remainsinspect")
        controls = tree("Read delegated agents", "Review MCP reload", "Show task plan", "Return to conversation", "Detach")
        self.assertTrue(smoke.runtime_ready("runtime", fields, editor, controls))
        self.assertFalse(smoke.runtime_ready("runtime", tree("Profile: other", "Model: fixture-model", "Tools: 2 · Skills: 1 · MCP servers: 1"), editor, controls))
        self.assertFalse(smoke.runtime_ready("runtime", fields, tree("runtime draft remaininspect"), controls))
        self.assertFalse(smoke.runtime_ready("runtime", fields, editor, tree("Send", "Detach")))
        self.assertTrue(smoke.runtime_ready("returned", {}, tree("runtime draft remainsinsert"), tree("Send", "Detach")))

    def test_delegated_readiness_requires_exact_reported_rows_scope_and_unchanged_draft(self):
        smoke = module("smoke_native_chat")
        def tree(*texts):
            return {"ok": True, "count": len(texts), "nodes": [{"text": s} for s in texts]}
        body = tree("1 reported delegated agents · snapshot, not independent peers", "Delegated: fx-child",
                    "Goal: Review parser", "Owner-reported data only; snapshot may be stale. No Steer, Stop, transcript or peer controls.")
        editor = tree("runtime draft remainsinspect")
        controls = tree("Read delegated agents", "Review MCP reload", "Show task plan", "Return to conversation", "Detach")
        self.assertTrue(smoke.delegated_ready(body, editor, controls))
        for wrong in (body["nodes"][:-1], body["nodes"] + [{"text": "PRIVATE"}]):
            self.assertFalse(smoke.delegated_ready({"ok": True, "count": len(wrong), "nodes": wrong}, editor, controls))
        self.assertFalse(smoke.delegated_ready(body, tree("editedinspect"), controls))
        self.assertFalse(smoke.delegated_ready(body, editor, tree("Steer", "Stop", "Detach")))

    def test_plan_readiness_requires_reported_totals_scope_and_unchanged_inspect_draft(self):
        smoke = module("smoke_native_chat")
        def tree(*texts):
            return {"ok": True, "count": len(texts), "nodes": [{"text": text} for text in texts]}
        expected = ("1 reported tasks · revision 2", "pending: 0 · in_progress: 1 · completed: 0 · cancelled: 0",
                    "Task: fx-task", "Status: in_progress", "Check parser",
                    "Reported task status is data, not proof of success or live work. No task edits, approvals or automatic prompts.")
        editor = tree("runtime draft remainsinspect")
        controls = tree("Show runtime details", "Return to conversation", "Detach")
        self.assertTrue(smoke.plan_ready(tree(*expected), editor, controls))
        for wrong in (expected[:-1], expected + ("PRIVATE",), ("2 reported tasks · revision 2",) + expected[1:]):
            self.assertFalse(smoke.plan_ready(tree(*wrong), editor, controls))
        self.assertFalse(smoke.plan_ready(tree(*expected), tree("changedinspect"), controls))
        self.assertFalse(smoke.plan_ready(tree(*expected), editor, tree("Complete task", "Send", "Detach")))


if __name__ == "__main__":
    unittest.main()
