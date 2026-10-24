"""Exact-lease operator release through the production binary and Unix IPC."""
import json
import unittest
import uuid

import test_coordinator_recovery as recovery


class CoordinatorReleaseTests(unittest.TestCase):
    def setUp(self):
        self.f = recovery.CoordinatorRecoveryTests()
        self.addCleanup(self.f.doCleanups)
        self.f.setUp()
        self.operator = {"token": (self.f.state / "operator.key").read_text()}

    def task(self, actor):
        task = str(uuid.uuid4())
        self.assertTrue(self.f.call(actor, {"type": "claim", "task": task, "ttl_seconds": 300})["ok"])
        return task, json.loads(self.f.cli("inspect-task", "--task", task).stdout)["task"]

    @staticmethod
    def operation(task, lease):
        return {"type": "release_stopped", "task": task, "owner": lease["owner_actor_id"],
                "incarnation": lease["incarnation"], "generation": lease["generation"], "attest_stopped": True}

    @staticmethod
    def receipt(task, lease):
        return {"task_id": task, "owner_actor_id": lease["owner_actor_id"],
                "incarnation": lease["incarnation"], "generation": lease["generation"],
                "state": "released", "released": True, "process_stopped": "operator_attested_not_enforced"}

    def logical(self):
        return {t: self.f.rows("SELECT * FROM " + t) for t in
                ["meta", "actors", "routes", "messages", "tasks", "events", "sqlite_sequence"]}

    def release(self, task, lease):
        return json.loads(self.f.cli("release-stopped", "--task", task,
                                    "--owner", lease["owner_actor_id"], "--incarnation", lease["incarnation"],
                                    "--generation", lease["generation"], "--attest-stopped").stdout)

    def raw(self, op):
        _, frame = self.f.frame(self.operator, op)
        with self.f.connect() as sock:
            sock.sendall(frame)
            with sock.makefile("rb") as stream:
                return json.loads(stream.readline(524289))

    def test_legacy_and_forged_release_are_nonmutating_with_valid_operator_control(self):
        task, lease = self.task(self.f.a)
        op = self.operation(task, lease)
        before = self.logical()
        legacy = self.raw({"type": "release_stopped", "task": task, "attest_stopped": True})
        self.assertFalse(legacy["ok"], "unfenced task-only release still accepted")
        for key in ["owner", "incarnation", "generation", "attest_stopped"]:
            missing = {k: v for k, v in op.items() if k != key}
            self.assertFalse(self.raw(missing)["ok"], key)
        self.assertFalse(self.raw({**op, "approve": True})["ok"])
        for changed in [{"attest_stopped": False}, {"owner": self.f.b["actor_id"]},
                        {"incarnation": str(uuid.uuid4())}, {"generation": "2"},
                        *({"generation": g} for g in ["0", "-1", "+1", "01", "1.0", "9223372036854775808"]),
                        {"owner": "not-a-uuid"}, {"incarnation": "not-a-uuid"}]:
            self.assertFalse(self.f.call(self.operator, {**op, **changed})["ok"], changed)
        for actor in [self.f.a, self.f.b, {"token": "0" * 64}]:
            self.assertFalse(self.f.call(actor, op)["ok"])
        old_cli = self.f.cli("release-stopped", "--task", task, "--attest-stopped", check=False)
        self.assertNotEqual(old_cli.returncode, 0)
        self.assertEqual(old_cli.stdout, b"")
        self.assertEqual(self.logical(), before, "refused release mutated logical state")
        expected = self.receipt(task, lease)
        self.assertEqual(self.release(task, lease), expected, "valid operator control")
        released = self.logical()
        self.assertEqual(self.release(task, lease), expected)
        self.assertEqual(self.logical(), released, "idempotent retry mutated state")
        self.assertEqual(self.f.rows("SELECT count(*) FROM events WHERE kind='task.operator_released'"), [(1,)])

    def test_old_generation_cannot_release_same_actor_reclaimed_lease_or_completed_task(self):
        task, first = self.task(self.f.a)
        self.assertEqual(self.release(task, first), self.receipt(task, first))
        second = self.f.call(self.f.a, {"type": "claim", "task": task, "ttl_seconds": 300})
        self.assertEqual(second["result"]["generation"], "2")
        current = json.loads(self.f.cli("inspect-task", "--task", task).stdout)["task"]
        self.assertEqual(current["incarnation"], first["incarnation"])
        before = self.logical()
        self.assertFalse(self.f.call(self.operator, self.operation(task, first))["ok"])
        self.assertEqual(self.logical(), before)
        self.assertTrue(self.f.call(self.f.a, {"type": "complete", "task": task, "generation": "2", "result": "fixture"})["ok"])
        before = self.logical()
        self.assertFalse(self.f.call(self.operator, self.operation(task, current))["ok"])
        self.assertEqual(self.logical(), before, "completed task was reopened or changed")

    def test_lost_release_reply_survives_restart_but_never_releases_a_new_owner(self):
        task, first = self.task(self.f.a)
        op = self.operation(task, first)
        # Task ownership is the persisted lease incarnation, not the actor's current credential.
        rotated = json.loads(self.f.cli("rotate", "--actor", self.f.a["actor_id"]).stdout)
        before = self.logical()
        self.assertFalse(self.f.call(self.operator, {**op, "incarnation": rotated["incarnation"]})["ok"])
        self.assertEqual(self.logical(), before)
        self.f.lose_response(self.operator, op)
        self.f.wait_rows("SELECT state FROM tasks WHERE id=?", [("released",)], (task,))
        self.f.restart()
        before = self.logical()
        self.assertEqual(self.release(task, first), self.receipt(task, first))
        self.assertEqual(self.logical(), before)
        self.assertEqual(self.f.call(self.f.b, {"type": "claim", "task": task, "ttl_seconds": 300})["result"]["generation"], "2")
        before = self.logical()
        self.assertFalse(self.f.call(self.operator, op)["ok"])
        self.assertEqual(self.logical(), before)
        self.assertTrue(self.f.call(self.f.b, {"type": "complete", "task": task, "generation": "2", "result": "fixture"})["ok"])
        self.assertEqual(self.f.rows("SELECT count(*) FROM events WHERE kind='task.operator_released'"), [(1,)])


if __name__ == "__main__":
    unittest.main()
