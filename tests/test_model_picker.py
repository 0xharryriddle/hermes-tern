"""Session model picker via production CLI/PTY/SDK; no real provider/settings."""
import termios
import unittest
import test_native_chat as native


class ModelPickerTests(unittest.TestCase):
    def fixture(self, mode="commit"):
        g = native.Gateway(autostart=False, accept_timeout=native._process.STARTUP_TIMEOUT)
        original = g.reply
        def reply(request, result):
            if request["method"] == "session.create":
                result["info"].update(model="old-model", provider="old-provider")
            original(request, result)
        g.reply = reply
        f = native.NativeChat(gateway=g)
        self.addCleanup(f.close)
        f.ready()
        # Extending the wire fixture is part of GREEN, not a production escape hatch.
        g.model_mode = mode
        return f

    def open(self, f):
        f.click("dock.navigation.models")
        f.until(lambda: any("model-exact" in p.get("text", "") for p in f.nodes.values()), "model inventory")
        self.assertEqual(f.gateway.methods("model.options")[0]["params"], {
            "session_id":"fixture-session", "explicit_only":True, "include_unconfigured":False, "refresh":False})

    def button(self, f, text):
        found = [key for key, p in f.nodes.items() if p.get("text") == text and p.get("actions", {}).get("click")]
        self.assertEqual(len(found), 1, (text, found))
        return found[0]

    def select(self, f):
        select = next(key for key, p in f.nodes.items() if p.get("text") == "Select model-exact")
        f.click(select)
        f.until(lambda: any(p.get("text") == "Apply to this session" for p in f.nodes.values()), "explicit apply")
        self.assertFalse(f.gateway.methods("session.mutate"))
        return self.button(f,"Apply to this session")

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("config.set"))
        self.assertFalse(f.gateway.methods("command.dispatch"))

    def test_inventory_selection_and_explicit_apply_use_fences_not_prompt_or_profile_config(self):
        f = self.fixture()
        f.send(b"draft stays")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "draft stays", "draft")
        self.open(f)
        apply = self.select(f)
        self.assertTrue(any("old-model" in p.get("text", "") for p in f.nodes.values()))
        f.click(apply)
        f.until(lambda: bool(f.gateway.methods("session.mutate")), "mutation")
        params = f.gateway.methods("session.mutate")[0]["params"]
        self.assertEqual({k:v for k,v in params.items() if k != "request_id"}, {
            "session_id":"fixture-session", "expected_revision":1, "expected_generation":2,
            "operation":"model", "payload":{"model":"model-exact","provider":"provider-exact"}})
        f.until(lambda: any("model-exact" in p.get("text", "") and "Active route:" in p.get("text", "") for p in f.nodes.values()), "owner route")
        f.click(self.button(f,"Return to conversation"))
        f.until(lambda:"dock.navigation.models" in f.nodes,"return")
        self.assertEqual(f.nodes["dock.ed"]["text"],"draft stays")
        self.finish(f)

    def test_guarded_target_requires_new_human_confirmation_and_distinct_request_id(self):
        f = self.fixture("confirmation")
        self.open(f)
        f.click(self.select(f))
        f.until(lambda: any(p.get("text") == "Confirm guarded change" for p in f.nodes.values()), "guard")
        self.assertEqual(len(f.gateway.methods("session.mutate")), 1)
        self.assertTrue(any("old-model" in p.get("text", "") and "Active route:" in p.get("text", "") for p in f.nodes.values()))
        f.click(self.button(f,"Confirm guarded change"))
        f.until(lambda: len(f.gateway.methods("session.mutate")) == 2, "confirmed mutation")
        first, second = f.gateway.methods("session.mutate")
        self.assertNotEqual(first["params"]["request_id"], second["params"]["request_id"])
        self.assertEqual(second["params"]["payload"]["confirm"], "fixture-confirm-token")
        self.finish(f)

    def test_optional_api_refusal_leaves_chat_connected_and_never_changes_model(self):
        f = self.fixture("unavailable")
        f.click("dock.navigation.models")
        f.until(lambda: any("unavailable" in p.get("text", "").lower() for p in f.nodes.values()), "optional refusal")
        self.assertFalse(f.gateway.methods("session.mutate"))
        f.click(self.button(f,"Return to conversation"))
        f.until(lambda:"dock.navigation.models" in f.nodes,"return")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.assertIn("old-model",f.nodes["main.state"]["text"])
        self.finish(f)

    def test_owner_resolved_guard_target_is_displayed_then_original_catalog_identity_is_confirmed(self):
        f = self.fixture("confirmation")
        original = f.gateway.reply
        def reply(request, result):
            if request["method"] == "session.mutate" and result.get("status") == "confirmation_required":
                result = {**result, "target_model":"resolved-model", "target_provider":"resolved-provider"}
            original(request, result)
        f.gateway.reply = reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any(p.get("text") == "Confirm guarded change" for p in f.nodes.values()), "resolved guard")
        self.assertTrue(any("resolved-model" in p.get("text", "") and "resolved-provider" in p.get("text", "") for p in f.nodes.values()))
        self.assertTrue(any("Active route: old-model" in p.get("text", "") for p in f.nodes.values()))
        f.click(self.button(f, "Confirm guarded change"))
        f.until(lambda:len(f.gateway.methods("session.mutate")) == 2, "explicit resolved consent")
        self.assertEqual(f.gateway.methods("session.mutate")[1]["params"]["payload"], {"model":"model-exact", "provider":"provider-exact", "confirm":"fixture-confirm-token"})
        self.finish(f)

    def test_deferred_guard_remains_pending_and_never_reenables_send_on_local_return(self):
        f = self.fixture("deferred")
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("deferred by Hermes" in p.get("text", "") for p in f.nodes.values()), "deferred request")
        f.gateway.event("session.updated", {"session_id":"fixture-session", "operation":"model", "status":"confirmation_required", "confirm_required":True,
            "confirm":"deferred-guard-token", "target_model":"model-exact", "target_provider":"provider-exact", "confirm_message":"guard needs human review"})
        f.until(lambda:any("guard" in p.get("text", "").lower() for p in f.nodes.values()), "deferred guard event")
        f.click(self.button(f, "Return to conversation"))
        f.until(lambda:"dock.navigation.models" in f.nodes, "return with pending deferred guard")
        self.assertFalse(f.nodes["dock.ed"]["sendable"], "owner retains guarded pending pick; Send must remain withheld")
        self.assertFalse(any(p.get("text") == "Reload model inventory" for p in f.nodes.values()))
        self.finish(f)

    def test_stale_guard_after_owner_update_refuses_without_second_mutation(self):
        f = self.fixture("confirmation")
        self.open(f)
        f.click(self.select(f))
        f.until(lambda: any(p.get("text") == "Confirm guarded change" for p in f.nodes.values()), "guard")
        old = self.button(f,"Confirm guarded change")
        f.gateway.event("session.info",{"revision":2,"model":"other-model","provider":"other-provider"})
        f.quiet()
        f.click(old)
        f.quiet()
        self.assertEqual(len(f.gateway.methods("session.mutate")),1)
        self.assertTrue(any("other-model" in p.get("text", "") for p in f.nodes.values()))
        self.finish(f)
    def test_owner_route_change_without_revision_invalidates_guard(self):
        f = self.fixture("confirmation")
        self.open(f)
        f.click(self.select(f))
        f.until(lambda: any(p.get("text") == "Confirm guarded change" for p in f.nodes.values()), "guard")
        old = self.button(f,"Confirm guarded change")
        f.gateway.event("session.info",{"model":"other-model","provider":"other-provider"})
        f.quiet()
        f.click(old)
        f.quiet()
        self.assertEqual(len(f.gateway.methods("session.mutate")),1,"guard reused after owner route changed without revision")
        self.finish(f)

    def test_deferred_pick_is_not_active_and_later_refusal_restores_controls(self):
        f = self.fixture("deferred")
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("deferred" in p.get("text","").lower() for p in f.nodes.values()),"deferred")
        self.assertEqual(len(f.gateway.methods("session.mutate")),1)
        self.assertTrue(any("Active route: old-model" in p.get("text","") for p in f.nodes.values()))
        self.assertFalse(any(p.get("text")=="Apply to this session" for p in f.nodes.values()))
        f.gateway.event("session.updated",{"session_id":"fixture-session","operation":"model","status":"refused","reason":"model_resolution_failed","target_model":"model-exact","model":"old-model","provider":"old-provider"})
        f.until(lambda:any("not applied" in p.get("text","").lower() for p in f.nodes.values()),"deferred refusal")
        self.assertTrue(any(p.get("text")=="Reload model inventory" for p in f.nodes.values()))
        self.finish(f)

    def test_malformed_mutation_outcome_is_unknown_and_never_auto_retried(self):
        f = self.fixture("malformed")
        original=f.gateway.reply
        def reply(request,result):
            if request["method"]=="session.mutate":
                result={"session_id":"fixture-session","operation":"model","revision":2}
            original(request,result)
        f.gateway.reply=reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("unknown" in p.get("text","").lower() for p in f.nodes.values()),"unknown mutation outcome")
        f.quiet()
        self.assertEqual(len(f.gateway.methods("session.mutate")),1)
        self.assertFalse(any(p.get("text")=="Reload model inventory" for p in f.nodes.values()))
        f.click(self.button(f,"Return to conversation"))
        f.until(lambda:"dock.navigation.models" in f.nodes,"return unknown")
        self.assertFalse(f.nodes["dock.ed"]["sendable"],"unknown model outcome still advertised Send")
        self.assertNotIn("dock.controls.send",f.nodes)
        self.assertIn("unknown",f.nodes["dock.notice"]["text"].lower())
        f.send(b"must stay draft\r")
        f.quiet()
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.finish(f)

    def test_old_info_response_cannot_overwrite_newer_owner_event(self):
        f = self.fixture()
        original=f.gateway.reply
        def reply(request,result):
            if request["method"]=="session.info":
                f.gateway.model_route={"model":"newer-owner-model","provider":"newer-owner-provider"}
                f.gateway.model_revision=3
                f.gateway.event("session.info",{"model":"newer-owner-model","provider":"newer-owner-provider","revision":3})
            original(request,result)
        f.gateway.reply=reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("Active route: newer-owner-model" in p.get("text","") for p in f.nodes.values()),"newer info event")
        f.quiet()
        self.assertTrue(any("Active route: newer-owner-model" in p.get("text","") for p in f.nodes.values()))
        self.finish(f)
    def test_second_model_switch_uses_committed_owner_revision_and_generation(self):
        f = self.fixture()
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("Active route: model-exact" in p.get("text","") for p in f.nodes.values()),"first route")
        f.quiet()
        f.click(next(k for k,p in f.nodes.items() if p.get("text")=="Select model-exact"))
        f.until(lambda:any(p.get("text")=="Apply to this session" for p in f.nodes.values()),"second apply")
        f.click(self.button(f,"Apply to this session"))
        f.until(lambda:len(f.gateway.methods("session.mutate"))==2,"second mutation")
        second=f.gateway.methods("session.mutate")[1]["params"]
        self.assertEqual((second["expected_revision"],second["expected_generation"]),(2,3))
        self.finish(f)

    def test_effective_route_read_refusal_keeps_model_outcome_unknown_not_retryable(self):
        f = self.fixture()
        original=f.gateway.reply
        def reply(request,result):
            if request["method"]=="session.info":
                f.gateway.send({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"untrusted diagnostic","data":{"reason":"storage_unavailable"}}})
            else:
                original(request,result)
        f.gateway.reply=reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("unknown" in p.get("text","").lower() for p in f.nodes.values()),"readback failure is unknown")
        self.assertFalse(any(p.get("text")=="Reload model inventory" for p in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("session.mutate")),1)
        self.finish(f)

    def test_precommit_replay_snapshot_cannot_complete_postcommit_model_readback(self):
        f = self.fixture()
        original = f.gateway.reply
        held = {}
        def reply(request, result):
            method = request["method"]
            if method == "session.mutate" and "mutation" not in held:
                held["mutation"] = (request, result)
                f.gateway.event("session.replay_gap", {})
            elif method == "session.resume" and "replay" not in held:
                held["replay"] = request
                original(*held["mutation"])
            elif method == "session.info" and "old_sent" not in held:
                original(request, result)
                stale = native.snapshot()
                stale["info"].update(model="old-model", provider="old-provider")
                stale["last_sequence"] = f.gateway.seq
                held["old_sent"] = True
                original(held["replay"], stale)
            else:
                original(request, result)
        f.gateway.reply = reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("reconciled with owner snapshot" in p.get("text", "") for p in f.nodes.values()), "model readback completion")
        self.assertTrue(held.get("old_sent"), "precommit snapshot was not exercised")
        self.assertEqual(len(f.gateway.methods("session.resume")), 2, "old replay request was reused as postcommit model readback")
        self.assertTrue(any("Active route: model-exact" in p.get("text", "") for p in f.nodes.values()))
        f.click(next(k for k,p in f.nodes.items() if p.get("text") == "Select model-exact"))
        f.until(lambda:any(p.get("text") == "Apply to this session" for p in f.nodes.values()), "fresh fenced selection")
        f.click(self.button(f, "Apply to this session"))
        f.until(lambda:len(f.gateway.methods("session.mutate")) == 2, "second mutation")
        params = f.gateway.methods("session.mutate")[1]["params"]
        self.assertEqual((params["expected_revision"], params["expected_generation"]), (2, 3))
        self.finish(f)

    def test_dedicated_model_snapshot_below_receipt_fences_never_reports_reconciled(self):
        f = self.fixture()
        original = f.gateway.reply
        def reply(request, result):
            if request["method"] == "session.resume":
                result = {**result, "revision":1, "execution_generation":2}
            original(request, result)
        f.gateway.reply = reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("unknown" in p.get("text", "").lower() or "predates" in p.get("text", "").lower() for p in f.nodes.values()) or f.proc.poll() is not None, "old readback refusal")
        self.assertFalse(any("reconciled with owner snapshot" in p.get("text", "") for p in f.nodes.values()))
        self.assertEqual(len(f.gateway.methods("session.mutate")), 1)
        self.finish(f)

    def test_old_model_snapshot_never_overwrites_newer_owner_events(self):
        f = self.fixture()
        original=f.gateway.reply
        def reply(request,result):
            if request["method"]=="session.resume":
                f.gateway.event("session.info",{"model":"newest-model","provider":"newest-provider","revision":3})
            original(request,result)
        f.gateway.reply=reply
        self.open(f)
        f.click(self.select(f))
        f.until(lambda:any("newest-model" in p.get("text","") for p in f.nodes.values()),"newest owner event")
        f.quiet()
        self.assertTrue(any("Active route: newest-model" in p.get("text","") for p in f.nodes.values()),"old model snapshot overwrote newer event")
        self.assertEqual(len(f.gateway.methods("session.mutate")),1)
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
