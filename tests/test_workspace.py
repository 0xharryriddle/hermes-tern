"""Executable Cargo metadata contract for the binary/library boundary."""
import json
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


class WorkspaceLayoutTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        run = subprocess.run(
            ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
            cwd=ROOT, capture_output=True, text=True, check=True, timeout=30,
        )
        cls.metadata = json.loads(run.stdout)
        cls.packages = {p["name"]: p for p in cls.metadata["packages"]}

    def test_binary_crates_live_under_bin_and_libraries_under_crates(self):
        expected = {
            "hermes-tern": "bin/hermes-tern",
            "hermes-tern-testing": "bin/testing",
            "hermes-tern-cli": "crates/cli",
            "hermes-tern-core": "crates/core",
            "hermes-tern-gateway": "crates/gateway",
            "hermes-tern-ui": "crates/ui",
            "hermes-tern-store": "crates/store",
            "hermes-tern-supervisor": "crates/supervisor",
            "hermes-tern-omp": "crates/omp",
        }
        self.assertTrue(set(expected).issubset(self.packages), self.packages.keys())
        self.assertFalse((ROOT / "crates/hermes-tern").exists())
        for name, directory in expected.items():
            self.assertEqual(Path(self.packages[name]["manifest_path"]), ROOT / directory / "Cargo.toml")
        for p in self.packages.values():
            for target in p["targets"]:
                path = Path(target["src_path"])
                self.assertTrue(path.is_relative_to(Path(p["manifest_path"]).parent))
                if "bin" in target["kind"]:
                    self.assertTrue(path.is_relative_to(ROOT / "bin"), path)
                if "lib" in target["kind"]:
                    self.assertTrue(path.is_relative_to(ROOT / "crates"), path)

    def test_dependencies_follow_subsystem_boundaries_without_facade(self):
        allowed = {
            "hermes-tern-core": set(),
            "hermes-tern-gateway": {"hermes-tern-core"},
            "hermes-tern-ui": {"hermes-tern-core", "hermes-tern-gateway"},
            "hermes-tern-store": {"hermes-tern-core"},
            "hermes-tern-supervisor": {"hermes-tern-core", "hermes-tern-store"},
            "hermes-tern-omp": {"hermes-tern-core"},
            "hermes-tern-cli": {"hermes-tern-core", "hermes-tern-gateway", "hermes-tern-ui", "hermes-tern-store", "hermes-tern-supervisor", "hermes-tern-omp"},
            "hermes-tern": {"hermes-tern-cli"},
            "hermes-tern-testing": {"hermes-tern-core", "hermes-tern-gateway", "hermes-tern-ui"},
        }
        self.assertTrue(set(allowed).issubset(self.packages), self.packages.keys())
        for name, deps in allowed.items():
            actual = {d["name"] for d in self.packages[name]["dependencies"] if d["source"] is None and d["kind"] is None}
            self.assertEqual(actual, deps, name)
        core = {d["name"] for d in self.packages["hermes-tern-core"]["dependencies"]}
        self.assertFalse(core & {"tern-sdk", "tungstenite", "rusqlite", "clap", "rustix"})


if __name__ == "__main__":
    unittest.main()
