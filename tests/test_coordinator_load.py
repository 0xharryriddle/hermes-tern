"""Bounded real coordinator pressure; synthetic records, never live-agent acceptance."""
import copy
import importlib.util
import json
import math
import os
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"


def runner():
    path = ROOT / "scripts/verify_coordinator_load.py"
    if not path.is_file():
        raise AssertionError("bounded coordinator load runner is missing")
    spec = importlib.util.spec_from_file_location("coordinator_load_tests", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class CoordinatorLoadTests(unittest.TestCase):
    def scratch(self):
        root = Path(tempfile.mkdtemp(prefix="ht-load-test-", dir=os.environ["TMPDIR"]))
        root.chmod(0o700)
        def retain():
            paths = [root, *root.rglob("*")]
            (root / "ownership.json").write_text(json.dumps({
                "retention": "retain;no deletion",
                "paths": [{"path": str(p), "inode": p.lstat().st_ino} for p in paths],
            }, indent=2) + "\n")
        self.addCleanup(retain)
        return root

    def test_new_output_refuses_existing_relative_and_symlink_targets(self):
        module = runner()
        root = self.scratch()
        sentinel = root / "retained.json"
        sentinel.write_text("keep prior evidence\n")
        for bad in [root, Path("relative-output")]:
            with self.subTest(path=str(bad)):
                with self.assertRaises((ValueError, FileExistsError)):
                    module.prepare_output(bad)
        link = root / "alias"
        link.symlink_to(root, target_is_directory=True)
        with self.assertRaises((ValueError, FileExistsError)):
            module.prepare_output(link)
        with self.assertRaises((ValueError, FileExistsError)):
            module.prepare_output(link / "outside-alias")
        self.assertFalse((root / "outside-alias").exists())
        self.assertEqual(sentinel.read_text(), "keep prior evidence\n")
        path = module.prepare_output(root / "new")
        self.assertEqual(path.stat().st_mode & 0o777, 0o700)
        self.assertFalse(path.is_symlink())

    def test_invalid_producer_counts_refuse_before_launch_or_output_mutation(self):
        module = runner()
        root = self.scratch()
        for count in [0, 3, 5, -1, True, 1.0, "1", None]:
            with self.subTest(producers=count):
                out = module.prepare_output(root / ("case-" + str(len(list(root.iterdir())))))
                before = list(out.iterdir())
                with self.assertRaises(ValueError):
                    module.run_case(out, count, binary=root / "absent-binary")
                self.assertEqual(list(out.iterdir()), before)

    def check_case(self, producers):
        module = runner()
        out = module.prepare_output(self.scratch() / "case")
        row = module.run_case(out, producers, binary=BINARY)
        module.validate_case(row)
        self.assertEqual(row["status"], "passed")
        self.assertEqual(row["scope"], "synthetic_coordinator_ipc_not_live_agents")
        self.assertEqual(row["producers"], producers)
        self.assertEqual(row["registered_actor_records"], producers + 1)
        for key, expected in {
            "accepted_unique": 129, "received_unique": 129, "consumed_unique": 129,
            "capacity_refusals": 2, "duplicate_admissions": 1,
            "conflicting_admissions_refused": 1, "duplicate_deliveries": 0,
            "lost_accepted": 0, "pending_at_capacity": 128,
            "pending_after_receive": 128, "pending_final": 0,
        }.items():
            self.assertEqual(row[key], expected, key)
        self.assertEqual(row["event_counts"], {
            "message.queued": 129, "message.submitted": 129, "message.consumed": 129,
        })
        self.assertLessEqual(row["max_receive_batch"], 8)
        self.assertTrue(row["child_reaped"])
        self.assertEqual(row["child_exit_code"], 0)
        self.assertFalse(row["inference"])
        self.assertFalse(row["live_actors"])
        self.assertTrue(math.isfinite(row["wall_seconds"]) and row["wall_seconds"] > 0)
        self.assertGreater(row["retained_file_bytes"], 0)
        self.assertGreater(row["rss_high_water_bytes"], 0)
        self.assertEqual(len(row["accepted"]), 129)
        self.assertEqual(len(row["observed"]), 129)
        persisted = json.loads((out / "result.json").read_text())
        self.assertEqual(persisted, row)
        proof = row["accepted"][0]
        key = (proof["sender_actor_id"], proof["message_id"])
        observed = {(r["sender_actor_id"], r["message_id"]): r for r in row["observed"]}
        self.assertEqual(observed[key]["payload_sha256"], proof["payload_sha256"])
        self.assertEqual(observed[key]["state"], "consumed")
        serialized = json.dumps(row)
        self.assertNotIn('"credential"', serialized)
        self.assertNotIn('"auth"', serialized)
        self.assertNotIn('"token"', serialized)
        self.assertEqual(row["production_binary_sha256"], __import__("hashlib").sha256(BINARY.read_bytes()).hexdigest())
        return module, row

    def test_one_producer_slow_consumer_preserves_all_accepted_work(self):
        self.check_case(1)

    def test_two_concurrent_producers_slow_consumer_preserves_all_accepted_work(self):
        self.check_case(2)

    def test_four_concurrent_producers_and_report_negative_controls(self):
        module, row = self.check_case(4)
        changes = [
            ("schema_version", True),
            ("status", "failed"), ("accepted_unique", 128),
            ("capacity_refusals", 0), ("pending_after_receive", 120),
            ("child_reaped", False), ("child_exit_code", 1),
            ("wall_seconds", float("nan")), ("rss_high_water_bytes", 0),
            ("live_actors", True), ("inference", True),
            ("producers", True), ("lost_accepted", True),
            ("registered_actor_records", 4),
            ("retained_file_bytes", -1), ("rss_high_water_bytes", True),
            ("max_receive_batch", 9), ("child_exit_code", False),
            ("api_key", "synthetic-nonsecret-marker"),
        ]
        for name, value in changes:
            bad = copy.deepcopy(row)
            bad[name] = value
            with self.subTest(field=name):
                with self.assertRaises(ValueError):
                    module.validate_case(bad)
        for mutate in (
            lambda r: r["observed"].pop(),
            lambda r: r["observed"].__setitem__(0, copy.deepcopy(r["observed"][1])),
            lambda r: r["observed"][0].__setitem__("payload_sha256", "0" * 64),
            lambda r: r["accepted"].__setitem__(0, copy.deepcopy(r["accepted"][1])),
            lambda r: r["event_counts"].__setitem__("message.consumed", 128),
            lambda r: next(c for c in r["direct_children"] if c["operation"] == "coord").__setitem__("exit_code", 7),
            lambda r: r["accepted"][0].__setitem__("api_key", "synthetic-nonsecret-marker"),
            lambda r: r["host"].__setitem__("api_key", "synthetic-nonsecret-marker"),
        ):
            bad = copy.deepcopy(row)
            mutate(bad)
            with self.assertRaises(ValueError):
                module.validate_case(bad)


if __name__ == "__main__":
    unittest.main()
