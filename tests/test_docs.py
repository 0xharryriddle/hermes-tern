"""Negative controls for document links and the machine-readable task DAG."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "scripts/check_docs.py"
spec = importlib.util.spec_from_file_location("check_docs", SOURCE)
assert spec is not None and spec.loader is not None
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class DocsChecks(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=os.environ.get("TMPDIR"))
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / "docs").mkdir()
        (self.root / "docs/README.md").write_text("# Index\n[Scope](scope.md#purpose)\n")
        (self.root / "docs/scope.md").write_text("# Scope\n## Purpose\nAn example.\n")
        self.data = {
            "schema_version": 1,
            "requirements": [{"id": "R-01", "title": "Example requirement"}],
            "tasks": [{"id": "T-01", "title": "Example task", "status": "planned",
                       "depends_on": [], "requirements": ["R-01"],
                       "acceptance": ["A concrete outcome"], "evidence": [],
                       "blockers": []}],
        }
        self.save()

    def save(self):
        (self.root / "docs/tasks.json").write_text(json.dumps(self.data))

    def errors(self):
        return "\n".join(checker.validate(self.root))

    def test_valid_documents_and_dag_pass(self):
        self.assertEqual(checker.validate(self.root), [])

    def test_missing_link_target_rejected(self):
        (self.root / "docs/scope.md").write_text("# Scope\n## Purpose\n[Bad](lost.md)\n")
        self.assertIn("missing link", self.errors())

    def test_missing_fragment_rejected(self):
        (self.root / "docs/README.md").write_text("# Index\n[Bad](scope.md#absent)\n")
        self.assertIn("missing fragment", self.errors())

    def test_unreachable_page_rejected(self):
        (self.root / "docs/orphan.md").write_text("# Orphan\n")
        self.assertIn("unreachable", self.errors())

    def test_unknown_dependency_rejected(self):
        self.data["tasks"][0]["depends_on"] = ["T-99"]
        self.save()
        self.assertIn("unknown dependency", self.errors())

    def test_dependency_cycle_rejected(self):
        self.data["tasks"][0]["depends_on"] = ["T-01"]
        self.save()
        self.assertIn("dependency cycle", self.errors())

    def test_duplicate_task_id_rejected(self):
        self.data["tasks"].append(dict(self.data["tasks"][0]))
        self.save()
        self.assertIn("duplicate task", self.errors())

    def test_unmapped_requirement_rejected(self):
        self.data["requirements"].append({"id": "R-02", "title": "Unmapped"})
        self.save()
        self.assertIn("unmapped requirement", self.errors())

    def test_done_task_without_evidence_rejected(self):
        self.data["tasks"][0]["status"] = "done"
        self.save()
        self.assertIn("done without evidence", self.errors())

    def test_unknown_requirement_rejected(self):
        self.data["tasks"][0]["requirements"] = ["R-99"]
        self.save()
        self.assertIn("unknown requirement", self.errors())

    def test_invalid_status_rejected(self):
        self.data["tasks"][0]["status"] = "probably-done"
        self.save()
        self.assertIn("invalid status", self.errors())

    def test_fenced_example_link_is_not_a_real_link(self):
        with (self.root / "docs/scope.md").open("a") as f:
            f.write("```text\n[Example](not-real.md)\n```\n")
        self.assertEqual(checker.validate(self.root), [])

    def test_bad_json_fails_closed(self):
        (self.root / "docs/tasks.json").write_text("not json")
        self.assertIn("tasks.json", self.errors())


if __name__ == "__main__":
    unittest.main()
