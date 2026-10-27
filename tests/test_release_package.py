"""Versioned deployment in private scratch; never a personal install or release gate."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"
SCRIPT = ROOT / "scripts/distribution.py"


def module():
    spec = importlib.util.spec_from_file_location("distribution", SCRIPT)
    assert spec is not None and spec.loader is not None
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


class ReleasePackageTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp(prefix="ht-dist-", dir=os.environ["TMPDIR"]))
        self.plugin = self.root / "source-plugin"
        self.plugin.mkdir()
        for name in ("plugin.toml", "window.luau"):
            (self.plugin / name).write_bytes((ROOT / "plugins/hermes-tern" / name).read_bytes())
        self.prefix = self.root / "install"
        self.addCleanup(self.record)
        self.d = module()

    def record(self):
        paths = [self.root, *self.root.rglob("*")]
        (self.root / "ownership.json").write_text(json.dumps({"retention":"retain;no deletion", "paths":[str(p) for p in paths] + [str(self.root / "ownership.json")]}) + "\n")

    def stage(self, name="candidate-a"):
        return self.d.stage(BINARY, self.plugin, self.root / name, "0.1.0", ROOT / "docs/sources.json")

    def activate(self, package, expected):
        return self.d.activate(self.prefix, package, expected)

    def test_stage_verifies_exact_payload_version_compatibility_and_no_release_claim(self):
        result = self.stage()
        package = self.root / "candidate-a"
        checked = self.d.verify(package)
        self.assertEqual(checked["generation"], result["generation"])
        self.assertEqual(checked["version"], "0.1.0")
        self.assertEqual(checked["binary_sha256"], hashlib.sha256(BINARY.read_bytes()).hexdigest())
        manifest = json.loads((package / "manifest.json").read_text())
        self.assertEqual(set(manifest["files"]), {"bin/hermes-tern", "plugins/hermes-tern/plugin.toml", "plugins/hermes-tern/window.luau"})
        self.assertFalse(manifest["product_complete"])
        self.assertEqual(manifest["license"], "owner_unselected")
        self.assertEqual(manifest["compatibility"]["other_versions"], "unqualified")
        self.assertFalse(self.prefix.exists())
        self.assertEqual(self.d.stage(BINARY,self.plugin,package,"0.1.0",ROOT / "docs/sources.json"), result)

    def test_extra_files_tampered_payload_and_symlinks_refuse(self):
        self.stage()
        package = self.root / "candidate-a"
        with (package / "plugins/hermes-tern/window.luau").open("ab") as stream:
            stream.write(b"\n-- tampered\n")
        with self.assertRaises(ValueError):
            self.d.verify(package)
        (self.plugin / "window.luau").write_text("-- different allowed payload\n")
        self.stage("candidate-b")
        extra = self.root / "candidate-b/unknown.env"
        extra.write_text("not a secret; fixture extra")
        with self.assertRaises(ValueError):
            self.d.verify(self.root / "candidate-b")
        extra.rename(self.root / "retained-extra")
        target=self.root / "candidate-b/plugins/hermes-tern/window.luau"
        target.rename(self.root / "retained-window.luau")
        target.symlink_to(self.plugin / "window.luau")
        with self.assertRaises(ValueError):
            self.d.verify(self.root / "candidate-b")

    def test_version_mismatch_and_symlink_source_refuse_before_creating_output(self):
        with self.assertRaises(ValueError):
            self.d.stage(BINARY,self.plugin,self.root / "bad-version","9.9.9",ROOT / "docs/sources.json")
        self.assertFalse((self.root / "bad-version").exists())
        link=self.root / "binary-link"
        link.symlink_to(BINARY)
        with self.assertRaises(ValueError):
            self.d.stage(link,self.plugin,self.root / "bad-link","0.1.0",ROOT / "docs/sources.json")
        self.assertFalse((self.root / "bad-link").exists())

    def test_managed_install_update_and_rollback_preserve_prior_generation_and_exact_payload(self):
        first=self.stage()
        self.d.initialize(self.prefix)
        installed=self.activate(self.root / "candidate-a", None)
        self.assertEqual(installed["generation"],first["generation"])
        run=subprocess.run([str(self.prefix / "bin/hermes-tern"),"--version"],capture_output=True,text=True,timeout=5)
        self.assertEqual((run.returncode,run.stdout),(0,"hermes-tern 0.1.0\n"))
        self.assertEqual((self.prefix / "current/plugins/hermes-tern/window.luau").read_bytes(),(self.plugin / "window.luau").read_bytes())
        (self.plugin / "window.luau").write_bytes((self.plugin / "window.luau").read_bytes()+b"\n-- candidate generation b fixture\n")
        second=self.stage("candidate-b")
        self.assertNotEqual(first["generation"],second["generation"])
        self.activate(self.root / "candidate-b",first["generation"])
        self.assertEqual(self.d.status(self.prefix)["generation"],second["generation"])
        previous=self.prefix / "generations" / first["generation"]
        self.assertEqual(self.d.verify(previous)["generation"],first["generation"])
        back=self.d.rollback(self.prefix,first["generation"],second["generation"])
        self.assertEqual(back["generation"],first["generation"])
        self.assertEqual((self.prefix / "current/plugins/hermes-tern/window.luau").read_bytes(),(self.root / "candidate-a/plugins/hermes-tern/window.luau").read_bytes())
        self.assertEqual(self.d.rollback(self.prefix,first["generation"],first["generation"])["generation"],first["generation"])

    def test_unmanaged_prefix_stale_expectation_or_bad_update_never_replaces_current(self):
        self.prefix.mkdir()
        sentinel=self.prefix / "operator-file"
        sentinel.write_text("keep")
        with self.assertRaises(ValueError):self.d.initialize(self.prefix)
        self.assertEqual(sentinel.read_text(),"keep")
        safe=self.root / "managed"
        self.d.initialize(safe)
        self.prefix=safe
        first=self.stage();self.activate(self.root / "candidate-a",None)
        before=os.readlink(safe / "current")
        with self.assertRaises(ValueError):self.activate(self.root / "candidate-a","0"*64)
        self.assertEqual(os.readlink(safe / "current"),before)
        (self.plugin / "window.luau").write_text("-- changed fixture\n")
        self.stage("candidate-b")
        (self.root / "candidate-b/plugins/hermes-tern/window.luau").write_text("-- corrupt fixture\n")
        with self.assertRaises(ValueError):self.activate(self.root / "candidate-b",first["generation"])
        self.assertEqual(os.readlink(safe / "current"),before)
        self.assertEqual(self.d.status(safe)["generation"],first["generation"])

    def test_existing_stage_retry_retries_missing_parent_directory_sync(self):
        first = self.stage()
        with patch.object(self.d, "sync", side_effect=OSError("owned fsync failure")):
            with self.assertRaises(OSError):
                self.stage()
        self.assertEqual(self.d.verify(self.root / "candidate-a")["generation"], first["generation"])
        seen = []
        original = self.d.sync
        def capture(path):
            seen.append(path)
            return original(path)
        with patch.object(self.d, "sync", side_effect=capture):
            self.stage()
        self.assertIn(self.root, seen)

    def test_retained_generation_activation_retries_missing_generation_parent_sync(self):
        candidate = self.stage()
        self.d.initialize(self.prefix)
        self.activate(self.root / "candidate-a", None)
        seen = []
        original = self.d.sync
        def capture(path):
            seen.append(path)
            return original(path)
        with patch.object(self.d, "sync", side_effect=capture):
            self.activate(self.root / "candidate-a", candidate["generation"])
        self.assertIn(self.prefix / "generations", seen)
        self.assertIn(self.prefix, seen)

    def test_malformed_coherent_manifest_has_stable_refusal_not_raw_shape_exception(self):
        self.stage()
        path = self.root / "candidate-a/manifest.json"
        data = json.loads(path.read_text())
        original = json.loads(path.read_text())
        data["files"] = list(data["files"])
        data["generation"] = hashlib.sha256(self.d.canonical({k:v for k,v in data.items() if k != "generation"})).hexdigest()
        path.write_bytes(self.d.canonical(data))
        with self.assertRaises(ValueError):
            self.d.verify(path.parent)
        data = original
        data["schema_version"] = True
        data["generation"] = hashlib.sha256(self.d.canonical({k:v for k,v in data.items() if k != "generation"})).hexdigest()
        path.write_bytes(self.d.canonical(data))
        with self.assertRaises(ValueError):
            self.d.verify(path.parent)

    def test_rollback_retries_retained_generation_sync_and_capacity_never_prunes(self):
        candidate = self.stage()
        self.d.initialize(self.prefix)
        self.activate(self.root / "candidate-a", None)
        seen = []
        original = self.d.sync
        def capture(path):
            seen.append(path)
            return original(path)
        with patch.object(self.d, "sync", side_effect=capture):
            self.d.rollback(self.prefix, candidate["generation"], candidate["generation"])
        self.assertIn(self.prefix / "generations", seen)
        (self.plugin / "window.luau").write_text("-- second bounded fixture\n")
        self.stage("candidate-b")
        before = os.readlink(self.prefix / "current")
        with patch.object(self.d, "MAX_GENERATIONS", 1):
            with self.assertRaises(ValueError):
                self.activate(self.root / "candidate-b", candidate["generation"])
        self.assertEqual(os.readlink(self.prefix / "current"), before)
        self.assertEqual(len(list((self.prefix / "generations").iterdir())), 1)

    def test_cli_refusal_is_nonzero_structured_and_contains_no_raw_payload(self):
        self.stage()
        (self.root / "candidate-a/manifest.json").write_text('[]')
        run = subprocess.run(["python3", "-B", str(SCRIPT), "verify", "--package", str(self.root / "candidate-a")], capture_output=True, text=True, timeout=5)
        self.assertEqual(run.returncode, 1)
        self.assertEqual(json.loads(run.stdout)["status"], "refused")
        self.assertEqual(run.stderr, "")

    def test_generation_parent_sync_failure_preserves_old_current_then_explicit_retry_completes(self):
        first = self.stage()
        self.d.initialize(self.prefix)
        self.activate(self.root / "candidate-a", None)
        (self.plugin / "window.luau").write_text("-- generation b fault fixture\n")
        second = self.stage("candidate-b")
        original = self.d.sync
        parent = self.prefix / "generations"
        armed = [True]
        def fail_once(path):
            if armed[0] and path == parent and (parent / second["generation"]).exists():
                armed[0] = False
                raise OSError("owned post-rename fsync failure")
            return original(path)
        with patch.object(self.d, "sync", side_effect=fail_once):
            with self.assertRaises(OSError):
                self.activate(self.root / "candidate-b", first["generation"])
        self.assertFalse(armed[0], "fault injection was not reached")
        self.assertEqual(self.d.status(self.prefix)["generation"], first["generation"])
        self.activate(self.root / "candidate-b", first["generation"])
        self.assertEqual(self.d.status(self.prefix)["generation"], second["generation"])

    def test_post_switch_sync_failure_is_readable_unknown_and_never_automatically_reverted(self):
        candidate = self.stage()
        self.d.initialize(self.prefix)
        original = self.d.sync
        armed = [True]
        def fail_once(path):
            if path == self.prefix and armed[0]:
                armed[0] = False
                raise OSError("owned post-switch fsync failure")
            return original(path)
        with patch.object(self.d, "sync", side_effect=fail_once):
            with self.assertRaises(OSError):
                self.activate(self.root / "candidate-a", None)
        self.assertFalse(armed[0], "fault injection was not reached")
        self.assertEqual(self.d.status(self.prefix)["generation"], candidate["generation"])
        with self.assertRaises(ValueError):
            self.activate(self.root / "candidate-a", None)
        self.activate(self.root / "candidate-a", candidate["generation"])
        self.assertEqual(self.d.status(self.prefix)["generation"], candidate["generation"])

    def test_coherently_rehashed_unsupported_plugin_schema_cannot_verify_or_replace_current(self):
        first = self.stage()
        self.d.initialize(self.prefix)
        self.activate(self.root / "candidate-a", None)
        self.stage("candidate-b")
        package = self.root / "candidate-b"
        plugin = package / "plugins/hermes-tern/plugin.toml"
        original_plugin = plugin.read_bytes()
        manifest = package / "manifest.json"
        original_manifest = manifest.read_bytes()
        for replacement in (b"schema = 2", b"schema = true", b"# schema omitted"):
            with self.subTest(schema=replacement):
                raw = original_plugin.replace(b"schema = 1", replacement)
                self.assertNotEqual(raw, original_plugin)
                plugin.write_bytes(raw)
                data = json.loads(original_manifest)
                data["files"]["plugins/hermes-tern/plugin.toml"].update(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest())
                data["generation"] = hashlib.sha256(self.d.canonical({k:v for k,v in data.items() if k != "generation"})).hexdigest()
                manifest.write_bytes(self.d.canonical(data))
                with self.assertRaises(ValueError):
                    self.d.verify(package)
                with self.assertRaises(ValueError):
                    self.activate(package, first["generation"])
                self.assertEqual(self.d.status(self.prefix)["generation"], first["generation"])
        self.assertEqual(len(list((self.prefix / "generations").iterdir())), 1)

    def test_stage_refuses_noninteger_or_unknown_plugin_schema_before_output_exists(self):
        path = self.plugin / "plugin.toml"
        original = path.read_bytes()
        for index, replacement in enumerate((b"schema = true", b"schema = 2", b"# schema omitted")):
            with self.subTest(schema=replacement):
                path.write_bytes(original.replace(b"schema = 1", replacement))
                output = self.root / f"invalid-schema-{index}"
                with self.assertRaises(ValueError):
                    self.d.stage(BINARY, self.plugin, output, "0.1.0", ROOT / "docs/sources.json")
                self.assertFalse(output.exists())

    def test_cooperative_lock_refuses_parallel_activation_without_creating_a_generation(self):
        self.stage()
        self.d.initialize(self.prefix)
        with self.d.locked(self.prefix):
            run = subprocess.run(["python3", "-B", str(SCRIPT), "activate", "--prefix", str(self.prefix), "--package", str(self.root / "candidate-a"), "--expected-current", "none"], capture_output=True, text=True, timeout=5)
        self.assertEqual(run.returncode, 1)
        self.assertEqual(json.loads(run.stdout)["status"], "refused")
        self.assertEqual(self.d.status(self.prefix)["generation"], None)
        self.assertEqual(list((self.prefix / "generations").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
