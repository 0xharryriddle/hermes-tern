"""Native-smoke harness contracts only; synthetic geometry is not render acceptance."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("native_smoke", ROOT / "scripts/smoke_native.py")
assert spec and spec.loader
smoke = importlib.util.module_from_spec(spec)
spec.loader.exec_module(smoke)


def geometry(fixture=True):
    elements = [{"path": "div.sf-main", "visible": True, "rect": [16, 50, 448, 300]}]
    if fixture:
        elements += [
            {"path": "div.sf-dock", "visible": True, "rect": [16, 600, 448, 140]},
            {"path": "div.sf-dock>div.sf-editor", "visible": True, "rect": [16, 610, 448, 30]},
            {"path": "div.sf-dock>div.ht-controls", "visible": True, "rect": [16, 650, 448, 32]},
            {"path": "div.sf-dock>div.ht-controls>button.ht-button", "visible": True, "rect": [16, 650, 90, 24]},
            {"path": "div.sf-dock>div.ht-controls>button.ht-button", "visible": True, "rect": [120, 650, 90, 24]},
        ]
    return {"viewport": {"width": 480, "height": 760, "deviceScaleFactor": 2},
            "theme": "dark", "elements": elements}


class NativeSmokeTests(unittest.TestCase):
    def test_fixture_selection_uses_main_surface_marker_not_dock_text(self):
        binary, text, shot = smoke.scenario(ROOT, fixture=True)
        self.assertEqual(binary, ROOT / "target/debug/hermes-tern-testing")
        self.assertEqual(shot, "fixture")
        lines = text.splitlines()
        self.assertIn('plugins expect "Hermes / FIXTURE"', lines)
        self.assertNotIn('plugins expect "FIXTURE ONLY"', lines)
        self.assertNotIn("preview", lines[1])
        binary, text, shot = smoke.scenario(ROOT, fixture=False)
        self.assertEqual(binary, ROOT / "target/debug/hermes-tern")
        self.assertEqual(shot, "preview")
        self.assertIn('plugins expect "No agent is connected"', text)

    def test_isolation_drops_personal_profiles_transports_and_inline_credentials(self):
        scratch = Path(tempfile.mkdtemp(prefix="ht-smoke-env-", dir=os.environ["TMPDIR"]))
        try:
            env = smoke.isolated_environment(scratch, {
                "PATH": "/usr/bin:/bin", "LANG": "C.UTF-8", "HOME": "/personal",
                "HERMES_HOME": "/personal/hermes", "TERN_CONFIG_DIR": "/personal/tern",
                "TERN_DAEMON_SOCKET": "/personal/daemon.sock", "TERN_WINDOW_KEY": "personal",
                "TERN_TSP_RECORD": "/personal/transcript", "TERN_TSP": "0", "TMUX": "personal",
                "STY": "personal", "ZELLIJ": "personal", "OPENAI_API_KEY": "fixture-sentinel",
                "LD_PRELOAD": "/personal/interposer.so", "PYTHONPATH": "/personal/modules",
            })
            for key in ["HOME", "HERMES_HOME", "TERN_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_STATE_HOME",
                        "XDG_CACHE_HOME", "STENCIL_LOG_DIR", "XDG_RUNTIME_DIR", "TMPDIR"]:
                self.assertTrue(Path(env[key]).is_relative_to(scratch))
            self.assertTrue(Path(env["TERN_DAEMON_SOCKET"]).is_relative_to(scratch))
            for key in ["TERN_WINDOW_KEY", "TERN_TSP_RECORD", "TERN_TSP", "TMUX", "STY", "ZELLIJ",
                        "OPENAI_API_KEY", "LD_PRELOAD", "PYTHONPATH"]:
                self.assertNotIn(key, env)
            self.assertEqual(env["PATH"], "/usr/bin:/bin")
        finally:
            paths = [scratch, *scratch.iterdir(), scratch / "ownership.json"]
            (scratch / "ownership.json").write_text(json.dumps({"retention": "retain; no deletion", "paths": list(map(str, paths))}) + "\n")

    def test_fixture_requires_real_visible_main_dock_editor_and_controls_geometry(self):
        smoke.validate_layout(geometry(), fixture=True, width=480, height=760)
        smoke.validate_layout(geometry(False), fixture=False, width=480, height=760)
        for part in ["sf-main", "sf-dock", "sf-editor", "ht-controls", "ht-button"]:
            data = geometry()
            data["elements"] = [e for e in data["elements"] if part not in e["path"]]
            with self.assertRaises(ValueError, msg=part):
                smoke.validate_layout(data, fixture=True, width=480, height=760)
        for rectangle in [[0, 0, 0, 0], [500, 600, 300, 100], [0, 750, 100, 100], [0, 0, -1, 30], [0, 0, float("nan"), 30]]:
            data = geometry()
            data["elements"][2]["rect"] = rectangle
            with self.assertRaises(ValueError):
                smoke.validate_layout(data, fixture=True, width=480, height=760)
        data = geometry()
        data["elements"][2]["visible"] = False
        with self.assertRaises(ValueError):
            smoke.validate_layout(data, fixture=True, width=480, height=760)

    def test_wrong_viewport_and_empty_or_malformed_layout_refuse(self):
        for data in [{}, {"elements": []}, {**geometry(), "viewport": {"width": 1100, "height": 760}},
                     {**geometry(), "elements": "not an element list"}]:
            with self.assertRaises(ValueError):
                smoke.validate_layout(data, fixture=True, width=480, height=760)

    def test_renderer_failure_retains_inventory_and_never_reports_pass(self):
        for failure in [subprocess.CalledProcessError(1, ["tern", "shot"], stderr="fixture failure"),
                        subprocess.TimeoutExpired(["tern", "shot"], 90)]:
            scratch = Path(tempfile.mkdtemp(prefix="ht-smoke-fail-", dir=os.environ["TMPDIR"]))
            with patch.object(smoke, "render", side_effect=failure):
                with self.assertRaises((subprocess.CalledProcessError, subprocess.TimeoutExpired)):
                    smoke.run(ROOT, scratch, fixture=True, size="480x760")
            result = json.loads((scratch / "result.json").read_text())
            self.assertEqual(result["status"], "failed")
            self.assertTrue((scratch / "ownership.json").is_file())
            self.assertFalse(result.get("native_accepted", False))

    def test_zero_exit_without_rendered_output_does_not_pass(self):
        scratch = Path(tempfile.mkdtemp(prefix="ht-smoke-empty-", dir=os.environ["TMPDIR"]))
        completed = subprocess.CompletedProcess(["tern", "shot"], 0, stdout="", stderr="")
        with patch.object(smoke, "render", return_value=completed):
            with self.assertRaisesRegex(ValueError, "light and dark"):
                smoke.run(ROOT, scratch, fixture=True, size="480x760")
        result = json.loads((scratch / "result.json").read_text())
        self.assertEqual(result["status"], "failed")
        self.assertEqual(result["error_class"], "ValueError")
        owned = {p["path"] for p in json.loads((scratch / "ownership.json").read_text())["paths"]}
        self.assertIn(str(scratch / "result.json"), owned)
        self.assertIn(str(scratch / "ownership.json"), owned)
        self.assertTrue(all(Path(p).is_relative_to(scratch) for p in owned))

    def test_timeout_retains_partial_renderer_diagnostics(self):
        scratch = Path(tempfile.mkdtemp(prefix="ht-smoke-time-", dir=os.environ["TMPDIR"]))
        failure = subprocess.TimeoutExpired(["tern", "shot"], 90, output=b"partial-output\n", stderr=b"partial-error\n")
        with patch.object(smoke, "render", side_effect=failure):
            with self.assertRaises(subprocess.TimeoutExpired):
                smoke.run(ROOT, scratch, fixture=True, size="480x760")
        self.assertEqual((scratch / "renderer.log").read_text(), "partial-output\npartial-error\n")
        self.assertEqual(json.loads((scratch / "result.json").read_text())["status"], "failed")

    def test_size_validation_refuses_malformed_or_unbounded_values(self):
        self.assertEqual(smoke.parse_size("1100x760"), (1100, 760, None))
        self.assertEqual(smoke.parse_size("480x760@2"), (480, 760, 2))
        for value in ["0x760", "1x1", "99999x760", "480x760@0", "480x760@99", "480x760;quit", "480x760extra"]:
            with self.assertRaises(ValueError):
                smoke.parse_size(value)


if __name__ == "__main__":
    unittest.main()
