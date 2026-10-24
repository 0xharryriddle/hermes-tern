"""Package structure tests; these do not execute the Luau window entry.

Native plugin execution requires an isolated Tern window acceptance check.
"""
import pathlib
import re
import tomllib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


class PluginPackageTests(unittest.TestCase):
    def test_manifest_declares_existing_window_entry_inside_package(self):
        package = ROOT / "plugins/hermes-tern"
        manifest = tomllib.loads((package / "plugin.toml").read_text())
        self.assertEqual(manifest["schema"], 1)
        self.assertRegex(manifest["id"], re.compile(r"^[a-z][a-z0-9-]{0,31}$"))
        entry = (package / manifest["window"]).resolve()
        self.assertTrue(entry.is_relative_to(package.resolve()))
        self.assertTrue(entry.is_file())
        self.assertNotIn("host", manifest)
        self.assertNotIn("blocks", manifest)
        self.assertNotIn("lenses", manifest)


if __name__ == "__main__":
    unittest.main()
