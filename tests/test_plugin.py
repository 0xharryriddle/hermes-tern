"""Package structure tests; these do not execute the Luau window entry.

Native plugin execution requires an isolated Tern window acceptance check.
"""
import pathlib
import json
import os
import re
import subprocess
import tempfile
import tomllib
import unittest

from luau_tool import interpreter

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

    def test_actual_luau_chat_command_opens_native_picker_without_loading_side_effects(self):
        private = pathlib.Path(tempfile.mkdtemp(prefix="ht-launch-luau-", dir=os.environ["TMPDIR"]))
        script = private / "launcher.luau"
        script.write_text('local commands = {}\nlocal tern = {command=function(c) commands[c.id]=c end}\n'
                          'local function load_plugin(tern)\n' +
                          (ROOT/"plugins/hermes-tern/window.luau").read_text() + '\nend\nload_plugin(tern)\n' + '''
local launches, failures = 0, 0
for _, value in {42, false} do
    local cx = {layout={new_tab=function(_, opts)
        launches += 1
        assert(opts.command == "hermes-tern open", "chat bypassed native session picker")
        return if value == false then nil else value
    end},toast=function(_, kind) assert(kind == "error"); failures += 1 end}
    commands.chat.run(cx)
end
assert(launches == 2 and failures == 1)
print("PASS explicit native launcher")
''')
        result = subprocess.run([str(interpreter()),str(script)],capture_output=True,timeout=10)
        (private/"ownership.json").write_text(json.dumps({"retention":"retain;no deletion","paths":[str(private),str(script),str(private/"ownership.json")]})+"\n")
        self.assertEqual(result.returncode,0,result.stdout.decode()+result.stderr.decode())
        self.assertEqual(result.stdout.decode(),"PASS explicit native launcher\n")


if __name__ == "__main__":
    unittest.main()
