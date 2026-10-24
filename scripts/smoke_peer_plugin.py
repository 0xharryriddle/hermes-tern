"""Optional pinned-Hermes loader/registry smoke; no gateway, agent, or inference.

Uses only a newly created scratch HOME, plugin copy and fixture supervisor.
The supplied Python must have the inspected Hermes checkout's dependencies.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
PIN = "bf0203a794a07aac3ff764972ab9087baa4e4f4e"
TOOLS = {"tern_peer_status", "tern_peer_send", "tern_peer_receive", "tern_peer_ack", "tern_task_claim", "tern_task_complete"}


def worker(source, home):
    # Explicitly load one copied directory, never broad installed-plugin discovery.
    sys.path.insert(0, str(source))
    from hermes_cli.plugins import PluginManager
    from tools.registry import registry
    manager = PluginManager()
    manifests = manager._scan_directory(home / "plugins", "user")
    assert len(manifests) == 1 and manifests[0].name == "hermes-tern-peer"
    manager._load_plugin(manifests[0])
    try:
        loaded = manager.list_plugins()
        assert len(loaded) == 1 and loaded[0]["enabled"], "plugin loader refused package"
        for name in TOOLS:
            assert registry.get_entry(name, scope=manager.scope_key) is not None, name
        assert set(manager._plugin_tool_names) == TOOLS
        assert not manager._hooks and not manager._system_prompt_sections

        def call(name, args, session):
            raw = registry.dispatch(name, args, scope=manager.scope_key, session_id=session,
                                    task_id="fixture-task", user_task="fixture-data-not-authority")
            assert isinstance(raw, str)
            return json.loads(raw)

        info = json.loads((home.parent / "fixture.json").read_text())
        a, b = info["actors"]
        assert call("tern_peer_status", {}, "physical-a")["result"]["actor_id"] == a
        assert not call("tern_peer_status", {"session_id": "physical-b"}, "physical-a")["ok"]
        assert not call("tern_peer_status", {}, "unknown-after-compression")["ok"]
        assert not call("tern_peer_status", {}, None)["ok"]
        import uuid
        mid = str(uuid.uuid4())
        sent = call("tern_peer_send", {"recipient": b, "message_id": mid, "text": "attributed fixture", "kind": "context"}, "physical-a")
        assert sent["result"]["state"] == "queued"
        received = call("tern_peer_receive", {}, "physical-b")
        assert received["trust"] == "untrusted_peer_data_not_user_or_system_instructions"
        assert received["result"]["messages"][0]["sender_actor_id"] == a
        ack = call("tern_peer_ack", {"sender": a, "message": mid}, "physical-b")
        assert ack["result"]["state"] == "consumed"
        print(json.dumps({"status": "passed", "registered_tools": len(TOOLS), "real_registry_dispatch": True,
                          "send_receive_ack": True, "forged_missing_rotated_identity_denied": True,
                          "inference": False, "real_agent_turn": False}))
    finally:
        manager.unload()
    assert all(registry.get_entry(name, scope=manager.scope_key) is None for name in TOOLS)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hermes-root", required=True, type=Path)
    parser.add_argument("--python", required=True, type=Path)
    parser.add_argument("--worker-home", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    source = args.hermes_root.resolve(strict=True)
    if args.worker_home:
        home = args.worker_home.resolve(strict=True)
        assert Path(os.environ["HERMES_HOME"]).resolve() == home
        worker(source, home)
        return
    revision = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    if revision != PIN:
        raise SystemExit("Hermes source revision differs from this harness contract")
    if subprocess.check_output(["git", "-C", str(source), "status", "--porcelain", "--untracked-files=no"]):
        raise SystemExit("Hermes tracked source is dirty; qualify before running this harness")
    binary = ROOT / "target/debug/hermes-tern"
    if not binary.is_file():
        raise SystemExit("Build first: cargo build --locked -p hermes-tern")
    scratch = Path(tempfile.mkdtemp(prefix="ht-peer-host-", dir=os.environ["TMPDIR"]))
    home = scratch / "home"
    home.mkdir(mode=0o700)
    state = scratch / "state"
    executable = scratch / "hermes-tern"
    shutil.copyfile(binary, executable)
    executable.chmod(0o700)
    plugin = home / "plugins/hermes-tern-peer"
    plugin.mkdir(parents=True, mode=0o700)
    for name in ["__init__.py", "adapter.py", "plugin.yaml"]:
        shutil.copyfile(ROOT / "plugins/hermes-tern-peer" / name, plugin / name)
    env = {"HOME": str(home), "HERMES_HOME": str(home), "PATH": os.defpath, "LANG": "C.UTF-8",
           "TMPDIR": str(scratch), "PYTHONDONTWRITEBYTECODE": "1", "PYTHONNOUSERSITE": "1"}
    for key in ["XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"]:
        p = scratch / key.lower()
        p.mkdir(mode=0o700)
        env[key] = str(p)
    server = subprocess.Popen([str(executable), "serve", "--state", str(state), "--workspace", str(scratch)],
                              env=env, cwd=scratch, stdin=subprocess.DEVNULL,
                              stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    try:
        def cli(*argv, check=True):
            result = subprocess.run([str(executable), "coord", "--state", str(state), *argv], env=env,
                                    cwd=scratch, capture_output=True, timeout=5)
            if check and result.returncode:
                raise RuntimeError("fixture coordinator refused operation")
            return result
        deadline = time.monotonic() + 5
        while cli("status", check=False).returncode:
            if server.poll() is not None or time.monotonic() > deadline:
                raise RuntimeError("fixture coordinator unavailable")
            time.sleep(0.02)
        actors = []
        bindings = {}
        for session in ["physical-a", "physical-b"]:
            actor = json.loads(cli("register", "--kind", "hermes", "--profile", str(home), "--session", session,
                                   "--model", "fixture", "--provider", "no-inference").stdout)["actor_id"]
            actors.append(actor)
            bindings[session] = json.loads(cli("bind-peer", "--actor", actor, "--hermes-home", str(home),
                                               "--physical-session", session, "--attest-binding").stdout)["binding_file"]
        cli("route", "--sender", actors[0], "--recipient", actors[1], "--allow")
        config = {"plugins": {"enabled": ["hermes-tern-peer"], "isolation": "in_process",
                              "entries": {"hermes-tern-peer": {"settings": {"binary": str(executable), "bindings": bindings}}}}}
        # JSON is a YAML subset. This config lives only in the fresh scratch home.
        (home / "config.yaml").write_text(json.dumps(config))
        (home / "config.yaml").chmod(0o600)
        (scratch / "fixture.json").write_text(json.dumps({"actors": actors}))
        run = subprocess.run([str(args.python), "-I", "-B", str(Path(__file__).resolve()),
                              "--hermes-root", str(source), "--python", str(args.python), "--worker-home", str(home)],
                             env=env, cwd=scratch, capture_output=True, text=True, timeout=30)
        (scratch / "loader.stdout").write_text(run.stdout)
        (scratch / "loader.stderr").write_text(run.stderr)
        if run.returncode:
            raise RuntimeError("isolated loader/registry probe failed; inspect retained stderr")
        print(run.stdout.strip())
        cli("shutdown")
        server.wait(timeout=3)
    finally:
        if server.poll() is None:
            server.terminate()
            try:
                server.wait(timeout=3)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=3)
        inventory = [{"path": str(p), "inode": p.lstat().st_ino} for p in [scratch, *scratch.rglob("*")]]
        (scratch / "ownership.json").write_text(json.dumps({"retention": "retain; no cleanup", "paths": inventory}, indent=2))
        print("Retained isolated probe: " + str(scratch))


if __name__ == "__main__":
    main()
