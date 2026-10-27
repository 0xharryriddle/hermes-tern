"""Real Tern MCP scenario must prove scope, single consent and dedicated reattach."""
import copy
from pathlib import Path
import unittest
from test_runtime_autorun import module, request


class McpSmokeTests(unittest.TestCase):
    def calls(self):
        return [request("ping", {}), request("runtime.describe", {}),
                request("client.capabilities", {"server_requests": True}),
                request("session.create", {"source": "tui", "yolo": False}),
                request("reload.mcp", {"session_id": "fixture-session", "confirm": True}),
                request("session.resume", {"session_id": "fixture-session", "display_projection": True}),
                request("session.detach", {"session_id": "fixture-session", "subscription_id": "fixture-sub"})]

    def test_native_scenario_reviews_confirms_once_reattaches_returns_draft_and_detaches(self):
        s = module("smoke_native_chat")
        self.assertIn("mcp-reload", s.CASES)
        text, shots = s.scenario("mcp-reload", ["/private/hermes-tern", "chat"], Path("/private/stderr"))
        self.assertEqual(shots, ["review", "reconciled", "returned"])
        for step in ('click ".ht-runtime-entry"', 'click ".ht-mcp-entry"',
                     'click ".ht-mcp-confirm"', 'click ".ht-mcp-return"',
                     'plugins expect "MCP reattach readback complete"', 'expect "HT_CLIENT_EXIT_0"'):
            self.assertIn(step, text)
        self.assertEqual(text.count('click ".ht-mcp-confirm"'), 1)
        self.assertEqual(s.validate_requests("mcp-reload", self.calls(), [])["reload.mcp"], 1)

    def test_mcp_trace_rejects_duplicate_confirmation_missing_readback_and_unqualified_params(self):
        s = module("smoke_native_chat")
        self.assertIn("mcp-reload", s.CASES)
        for extra in (self.calls()[4], request("config.get", {}), request("prompt.submit", {}), request("session.mutate", {})):
            with self.assertRaises(ValueError):
                s.validate_requests("mcp-reload", self.calls() + [extra], [])
        for field, value in (("confirm", False), ("always", False), ("rev", "000000000000"), ("session_id", "other")):
            bad = self.calls()
            bad[4]["params"][field] = value
            with self.assertRaises(ValueError):
                s.validate_requests("mcp-reload", bad, [])
        with self.assertRaises(ValueError):
            s.validate_requests("mcp-reload", self.calls()[:5] + self.calls()[6:], [])
        bad = self.calls()
        bad[5], bad[4] = bad[4], bad[5]
        with self.assertRaises(ValueError):
            s.validate_requests("mcp-reload", bad, [])

    def test_mcp_readiness_checks_complete_short_scope_identity_state_and_unchanged_draft(self):
        s = module("smoke_native_chat")
        def tree(*texts):
            return {"ok": True, "count": len(texts), "nodes": [{"text": t} for t in texts]}
        # Selected short complete nodes, not the renderer's capped whole-panel text.
        fields = tree("MCP reload · profile-wide operation", "Profile: fixture-profile",
                      "Owner: fixture-instance · Session: fixture-session",
                      "Reload affects every chat; reattach can refresh policy, adoption and hooks.")
        editor = tree("mcp draft remainsinspect")
        controls = tree("Confirm profile reload and reattach", "Return to conversation", "Detach")
        self.assertTrue(s.mcp_ready("review", fields, editor, controls))
        for bad in (tree(), tree(*[n["text"] for n in fields["nodes"][:-1]]),
                    tree("MCP reload · profile-wide operation", "Profile: other", *[n["text"] for n in fields["nodes"][2:]])):
            self.assertFalse(s.mcp_ready("review", bad, editor, controls))
        self.assertFalse(s.mcp_ready("review", fields, tree("changedinspect"), controls))
        self.assertFalse(s.mcp_ready("review", fields, editor, tree("Send", "Detach")))
        done = tree("MCP reattach readback complete", "Reload report · chat policy: applied (owner-reported)",
                    "Owner reports are not proof of tool health. Reattach reads current owner state, not an exact reload receipt.")
        self.assertTrue(s.mcp_ready("reconciled", done, editor, tree("Review another reload", "Return to conversation", "Detach")))
        self.assertFalse(s.mcp_ready("reconciled", done, editor, controls))
        self.assertTrue(s.mcp_ready("returned", {}, tree("mcp draft remainsinsert"), tree("Send", "Detach")))

    def test_autorun_requires_all_four_mcp_width_theme_cases(self):
        r = module("verify_runtime")
        s = module("smoke_native_chat")
        self.assertIn("mcp-reload", s.CASES)
        rows = []
        for case in s.CASES:
            for size in ("1100x760", "480x760"):
                for theme in ("light", "dark"):
                    rows.append({"case": case, "size": size, "theme": theme, "status": "passed",
                                 "gateway_thread_joined": True, "gateway_errors": [],
                                 "selected_gateway_thread_joined": True, "selected_gateway_errors": [],
                                 "outputs": [{"image": "x"}],
                                 "renderer": {"status": "passed", "exit_code": 0, "reaped": True},
                                 "control": {"status": "passed", "projections": [{"shot": "x"}]},
                                 "client_exit": {"status": "exited", "exit_code": 0}})
        data = {"status": "passed", "expected_cases": len(rows), "cases": rows}
        r.validate_native_report(data)
        bad = copy.deepcopy(data)
        bad["cases"] = [x for x in rows if x["case"] != "mcp-reload"]
        bad["expected_cases"] = len(bad["cases"])
        with self.assertRaises(ValueError):
            r.validate_native_report(bad)


if __name__ == "__main__":
    unittest.main()
