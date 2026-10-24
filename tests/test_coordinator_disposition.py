"""Operator-only disposition and distinct-ID re-offer over the production IPC path."""
from contextlib import closing
import concurrent.futures
import json
import sqlite3
import unittest
import uuid

import test_coordinator_recovery as recovery


class CoordinatorDispositionTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        self.operator = {"token": (self.f.state / "operator.key").read_text()}
        self.send = self.f.send_op()
        self.assertTrue(self.f.call(self.f.a, self.send)["ok"])
        self.assertTrue(self.f.call(self.f.b, {"type": "receive"})["ok"])
        self.f.restart()
        self.op = {"type": "dispose_message", "disposition": {
            "sender": self.f.a["actor_id"], "message": self.send["message"]["message_id"],
            "delivery_incarnation": self.f.b["incarnation"], "decision_id": str(uuid.uuid4()),
            "action": "reoffer", "successor_id": str(uuid.uuid4()), "ttl_seconds": 300,
            "reason": "operator accepts duplicate risk; fixture only", "attest_outcome_unknown": True,
        }}

    def logical(self):
        return {t: self.f.rows("SELECT * FROM " + t + " ORDER BY 1,2") for t in
                ["meta", "messages", "events", "message_dispositions", "sqlite_sequence"]}

    def invoke_cli(self, op):
        d = op["disposition"]
        args = ["dispose-message", "--sender", d["sender"], "--message", d["message"],
                "--delivery-incarnation", d["delivery_incarnation"], "--decision-id", d["decision_id"],
                "--action", d["action"], "--reason", d["reason"], "--attest-outcome-unknown"]
        if d["successor_id"] is not None:
            args += ["--successor-id", d["successor_id"], "--ttl", str(d["ttl_seconds"])]
        return json.loads(self.f.cli(*args).stdout)

    def test_lost_decision_reply_restarts_with_one_successor_and_fences_late_ack(self):
        f = self.f
        d = self.op["disposition"]
        before = f.rows("SELECT * FROM messages")
        f.lose_response(self.operator, self.op)
        f.wait_rows("SELECT count(*) FROM messages", [(2,)])
        f.restart()
        receipt = self.invoke_cli(self.op)
        logical = self.logical()
        self.assertEqual(self.invoke_cli(self.op), receipt)
        self.assertEqual(self.logical(), logical)
        self.assertEqual(f.rows("SELECT * FROM messages WHERE id=?", (d["message"],)), before)
        self.assertFalse(f.call(f.b, {"type": "ack", "sender": d["sender"], "message": d["message"]})["ok"])
        successor = f.call(f.b, {"type": "receive"})["result"]["messages"]
        self.assertEqual(len(successor), 1)
        self.assertEqual(successor[0]["message_id"], d["successor_id"])
        self.assertEqual(successor[0]["origin"]["decision_id"], d["decision_id"])
        self.assertEqual(successor[0]["origin"]["authorization"], "operator_reoffer_not_new_sender_intent")
        self.assertTrue(f.call(f.b, {"type": "ack", "sender": d["sender"], "message": d["successor_id"]})["ok"])
        self.assertEqual(f.call(f.a, self.send)["result"]["disposition"]["decision_id"], d["decision_id"])
        archived = json.loads(f.cli("archive", "--archive-id", str(uuid.uuid4())).stdout)
        self.assertEqual(archived["counts"]["message_dispositions"], "1")

    def test_only_operator_with_exact_attested_tuple_can_dispose_and_conflicts_are_inert(self):
        before = self.logical()
        for actor in [self.f.a, self.f.b, {"token": "0" * 64}]:
            self.assertFalse(self.f.call(actor, self.op)["ok"])
        for key, value in [("attest_outcome_unknown", False), ("delivery_incarnation", str(uuid.uuid4())),
                           ("reason", " "), ("action", "consumed"), ("ttl_seconds", 0),
                           ("successor_id", self.op["disposition"]["message"])]:
            op = {"type": "dispose_message", "disposition": {**self.op["disposition"], key: value}}
            self.assertFalse(self.f.call(self.operator, op)["ok"])
        self.assertEqual(self.logical(), before)
        # Unknown fields fail before dispatch with otherwise valid operator authentication.
        forged = {"type": "dispose_message", "disposition": {**self.op["disposition"], "force": True}}
        _, frame = self.f.frame(self.operator, forged)
        with self.f.connect() as sock:
            sock.sendall(frame)
            with sock.makefile("rb") as stream:
                self.assertFalse(json.loads(stream.readline(524289))["ok"])
        receipt = self.invoke_cli(self.op)
        self.assertEqual(receipt["action"], "reoffer")
        before = self.logical()
        for field in ["reason", "decision_id", "successor_id"]:
            op = {"type": "dispose_message", "disposition": {**self.op["disposition"], field: str(uuid.uuid4())}}
            self.assertFalse(self.f.call(self.operator, op)["ok"])
        self.assertEqual(self.logical(), before)

    def test_revoked_route_and_event_failure_refuse_without_mutation_but_close_preserves_evidence(self):
        f = self.f
        f.cli("route", "--sender", f.a["actor_id"], "--recipient", f.b["actor_id"])
        before = self.logical()
        self.assertFalse(f.call(self.operator, self.op)["ok"])
        self.assertEqual(self.logical(), before)
        f.cli("route", "--sender", f.a["actor_id"], "--recipient", f.b["actor_id"], "--allow")
        before = self.logical()
        with closing(sqlite3.connect(f.state / "coordination.sqlite3")) as db:
            db.execute("CREATE TRIGGER fail_event BEFORE INSERT ON events WHEN NEW.kind='message.reoffered' BEGIN SELECT RAISE(ABORT,'fixture'); END")
            db.commit()
        self.assertFalse(f.call(self.operator, self.op)["ok"])
        self.assertEqual(self.logical(), before)
        close = {"type": "dispose_message", "disposition": {**self.op["disposition"],
                 "action": "close", "successor_id": None, "ttl_seconds": None}}
        receipt = self.invoke_cli(close)
        self.assertEqual(receipt["action"], "close")
        self.assertEqual(f.rows("SELECT state FROM messages"), [("ambiguous",)])
        self.assertEqual(f.rows("SELECT count(*) FROM messages"), [(1,)])
        f.restart()
        self.assertEqual(self.invoke_cli(close), receipt)
        self.assertEqual(f.call(f.b, {"type": "receive"})["result"]["messages"], [])

    def test_metadata_listings_distinguish_retained_ambiguity_from_unresolved_work(self):
        f = self.f
        d = self.op["disposition"]
        self.invoke_cli(self.op)
        snapshot = json.loads(f.cli("snapshot").stdout)
        listed = json.loads(f.cli("messages").stdout)["messages"]
        self.assertEqual(snapshot["messages"]["items"], listed)
        self.assertEqual(snapshot["messages"]["total"], "2")
        self.assertEqual(snapshot["unresolved_message_count"], "1")
        old = next(m for m in listed if m["message_id"] == d["message"])
        successor = next(m for m in listed if m["message_id"] == d["successor_id"])
        self.assertEqual(old["state"], "ambiguous")
        self.assertEqual(old["disposition_action"], "reoffer")
        self.assertEqual(old["decision_id"], d["decision_id"])
        self.assertEqual(old["successor_id"], d["successor_id"])
        self.assertEqual(successor["reoffer_source_id"], d["message"])
        self.assertEqual(successor["reoffer_decision_id"], d["decision_id"])
        self.assertNotIn(d["reason"], json.dumps(snapshot))
        self.assertNotIn(self.send["message"]["payload"]["text"], json.dumps(listed))
        f.call(f.b, {"type": "receive"})
        f.restart()
        close = {"type": "dispose_message", "disposition": {**d, "message": d["successor_id"],
                 "decision_id": str(uuid.uuid4()), "action": "close", "successor_id": None, "ttl_seconds": None}}
        self.invoke_cli(close)
        snapshot = json.loads(f.cli("snapshot").stdout)
        self.assertEqual(snapshot["unresolved_message_count"], "0")
        self.assertEqual(snapshot["messages"]["total"], "2")

    def test_ack_and_disposition_race_has_one_committed_outcome(self):
        f = self.f
        d = self.op["disposition"]
        ack = {"type": "ack", "sender": d["sender"], "message": d["message"]}
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            outcomes = list(pool.map(lambda args: f.call(*args), [(f.b, ack), (self.operator, self.op)]))
        self.assertEqual(sum(o["ok"] for o in outcomes), 1)
        if outcomes[0]["ok"]:
            self.assertEqual(f.rows("SELECT state FROM messages"), [("consumed",)])
            self.assertEqual(f.rows("SELECT count(*) FROM message_dispositions"), [(0,)])
        else:
            self.assertEqual(f.rows("SELECT state FROM messages ORDER BY sequence"), [("ambiguous",), ("queued",)])
            self.assertEqual(f.rows("SELECT count(*) FROM message_dispositions"), [(1,)])
        before = self.logical()
        f.restart()
        self.assertEqual(self.logical(), before)

    def test_sender_scoped_decision_and_duplicate_successor_collisions_do_not_cross_rows(self):
        f = self.f
        d = self.op["disposition"]
        c = f.register("c")
        f.cli("route", "--sender", c["actor_id"], "--recipient", f.b["actor_id"], "--allow")
        self.assertTrue(f.call(c, self.send)["ok"])
        self.assertTrue(f.call(f.b, {"type": "receive"})["ok"])
        f.restart()
        self.invoke_cli(self.op)
        other = {"type": "dispose_message", "disposition": {**d, "sender": c["actor_id"]}}
        before = self.logical()
        self.assertFalse(f.call(self.operator, other)["ok"], "decision ID reused for a different source")
        self.assertEqual(self.logical(), before)
        other["disposition"]["decision_id"] = str(uuid.uuid4())
        self.invoke_cli(other)
        successors = f.call(f.b, {"type": "receive"})["result"]["messages"]
        self.assertEqual({m["sender_actor_id"] for m in successors}, {f.a["actor_id"], c["actor_id"]})
        self.assertTrue(f.call(f.b, {"type": "ack", "sender": c["actor_id"], "message": d["successor_id"]})["ok"])
        self.assertEqual(f.rows("SELECT state FROM messages WHERE sender=? AND id=?", (f.a["actor_id"], d["successor_id"])), [("submitted",)])


if __name__ == "__main__":
    unittest.main()
