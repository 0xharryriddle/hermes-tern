"""Explicit bounded local/native acceptance autorun; never inference or personal install.

Default: canonical gate, standalone packages and real Tern synthetic-gateway matrix.
--local-only intentionally omits Tern; neither mode implies complete product acceptance.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load verification module")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def prepare_output(path):
    path = Path(path)
    if not path.is_absolute():
        raise ValueError("output path must be absolute and new")
    path.mkdir(mode=0o700, parents=False, exist_ok=False)
    return path


def process_identity(pid, live_only=False):
    try:
        fields = (Path("/proc") / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()
        if live_only and fields[0] in ("Z", "X"):
            return None
        return {"pid": pid, "pgrp": int(fields[2]), "session": int(fields[3]), "start_ticks": fields[19]}
    except FileNotFoundError:
        return None


def owned_group_members(pid, start_ticks):
    # A child created with start_new_session=True owns this session/group. Do not
    # infer ownership from executable names, UID or inherited environment.
    rows = []
    for path in Path("/proc").iterdir():
        if not path.name.isdecimal():
            continue
        try:
            row = process_identity(int(path.name), live_only=True)
        except (OSError, ValueError):
            continue
        if row and row["pgrp"] == pid and row["session"] == pid and int(row["start_ticks"]) >= int(start_ticks):
            rows.append(row)
    return rows


def run_command(command, cwd, env, log, timeout=180, max_bytes=8 * 1024 * 1024):
    if not 0 < timeout <= 1800 or not 0 < max_bytes <= 128 * 1024 * 1024:
        raise ValueError("invalid command deadline/output bound")
    started = time.monotonic()
    record = {"command": list(command), "cwd": str(cwd), "status": "failed", "exit_code": None,
              "timeout_seconds": timeout, "max_output_bytes": max_bytes, "log": str(log), "reaped": False}
    with Path(log).open("xb") as stream:
        proc = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True)
        assert proc.stdout is not None
        identity = process_identity(proc.pid)
        if identity is None:
            # Do not guess a reused PID/session. The direct Popen handle can be
            # reaped, but an unknown group is never a termination target.
            proc.wait(timeout=3)
            proc.stdout.close()
            raise RuntimeError("cannot establish owned child process identity")
        record["identity"] = identity
        size = 0
        reason = None
        def child_exited():
            # Keep the leader PID reserved until group cleanup ends. poll/wait
            # here would reap it and lose the authority to signal this group.
            return os.waitid(os.P_PID, proc.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None

        def wait_for_group(seconds):
            deadline = time.monotonic() + seconds
            while owned_group_members(proc.pid, identity["start_ticks"]):
                if time.monotonic() >= deadline:
                    return
                time.sleep(0.02)

        try:
            with selectors.DefaultSelector() as selector:
                selector.register(proc.stdout, selectors.EVENT_READ)
                while selector.get_map() or not child_exited():
                    if time.monotonic() - started >= timeout:
                        reason = "timeout"
                        break
                    for key, _ in selector.select(0.05):
                        data = os.read(key.fd, 65536)
                        if not data:
                            selector.unregister(key.fileobj)
                            continue
                        keep = data[:max_bytes - size]
                        stream.write(keep)
                        stream.flush()
                        size += len(keep)
                        if len(keep) != len(data):
                            reason = "output_limit"
                            break
                    if reason:
                        break
                    if child_exited() and owned_group_members(proc.pid, identity["start_ticks"]):
                        reason = "leftover_processes"
                        break
        finally:
            # Only the directly owned child session/group; no global name matching.
            members = owned_group_members(proc.pid, identity["start_ticks"])
            if members:
                if reason is None:
                    reason = "leftover_processes"
                if process_identity(proc.pid) == identity:
                    try:
                        os.killpg(proc.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                wait_for_group(2)
                remaining = owned_group_members(proc.pid, identity["start_ticks"])
                if remaining and process_identity(proc.pid) == identity:
                    try:
                        os.killpg(proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    wait_for_group(3)
            proc.wait(timeout=3)
            proc.stdout.close()
            record["reaped"] = True
            record["exit_code"] = proc.returncode
            record["status"] = reason or ("passed" if proc.returncode == 0 else "failed")
            record["elapsed_seconds"] = round(time.monotonic() - started, 3)
            record["output_bytes"] = size
            record["output_truncated"] = reason == "output_limit"
            record["remaining_group_members"] = owned_group_members(proc.pid, identity["start_ticks"])
            if record["remaining_group_members"]:
                record["status"] = reason or "leftover_processes"
    record["log_sha256"] = hashlib.sha256(Path(log).read_bytes()).hexdigest()
    return record


def gate_status(required, rows):
    if not required or len(set(required)) != len(required) or len(rows) != len(required):
        return "failed"
    names = [r.get("name") for r in rows]
    if len(set(names)) != len(names) or set(names) != set(required):
        return "failed"
    return "passed" if all(r.get("status") == "passed" and r.get("exit_code") == 0 for r in rows) else "failed"


def validate_native_report(data):
    expected = {(case, size, theme)
                for case in ("conversation", "deny", "skip", "interrupt", "disconnect", "long-output", "keyboard-output", "keyboard-reduced", "history", "launcher-close", "launcher-new", "launcher-resume", "runtime-details", "model-picker", "profile-picker", "commands", "attachments", "mcp-reload", "plugins", "reasoning")
                for size in ("1100x760", "480x760") for theme in ("light", "dark")}
    rows = data.get("cases", [])
    identities = [(r.get("case"), r.get("size"), r.get("theme")) for r in rows]
    if data.get("status") != "passed" or data.get("expected_cases") != len(expected) or len(identities) != len(expected) or set(identities) != expected:
        raise ValueError("native matrix is missing or duplicates a required case")
    for row in rows:
        if row.get("case") == "profile-picker" and (row.get("selected_gateway_thread_joined") is not True or row.get("selected_gateway_errors") != []):
            raise ValueError("selected profile process evidence incomplete")
        if row.get("status") != "passed" or row.get("gateway_thread_joined") is not True or row.get("gateway_errors") != [] or not row.get("outputs"):
            raise ValueError("native case evidence incomplete")
        process = row.get("renderer", {})
        if process.get("status") != "passed" or process.get("exit_code") != 0 or process.get("reaped") is not True:
            raise ValueError("native process evidence incomplete")
        control = row.get("control", {})
        if control.get("status") != "passed" or not control.get("projections") or control.get("quit_unconfirmed"):
            raise ValueError("native readiness/exit evidence incomplete")
        client_exit = row.get("client_exit")
        if (not isinstance(client_exit, dict) or client_exit.get("status") != "exited"
                or type(client_exit.get("exit_code")) is not int or client_exit["exit_code"] != 0):
            raise ValueError("native zero client exit evidence incomplete")


def source_witnesses():
    run = subprocess.run(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
                         cwd=ROOT, capture_output=True, check=True, timeout=10)
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest()
            for name in run.stdout.decode().split("\0") if name and (ROOT / name).is_file()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new absolute directory under a private scratch root")
    parser.add_argument("--local-only", action="store_true", help="explicit local-only scope; no Tern checks")
    args = parser.parse_args()
    base = Path(os.environ.get("TMPDIR", str(Path.home() / ".cache/hermes-tern/tmp")))
    base.mkdir(parents=True, exist_ok=True)
    out = prepare_output(args.out) if args.out else Path(tempfile.mkdtemp(prefix="ht-auto-", dir=base))
    print("Autorun retained outputs: " + str(out), flush=True)
    # Cargo/toolchain discovery may use HOME, but provider/session variables are
    # not forwarded. Native scripts apply their stricter isolated allowlist too.
    env = {k: os.environ[k] for k in ("PATH", "HOME", "LANG", "LC_ALL", "TZ", "CARGO_HOME", "RUSTUP_HOME",
                                    "RUSTUP_TOOLCHAIN", "RUSTC_WRAPPER", "CARGO_BUILD_JOBS", "SCCACHE_DIR",
                                    "HERMES_TERN_TEST_LUAU") if k in os.environ}
    env.update(TMPDIR=str(out), PYTHONDONTWRITEBYTECODE="1")
    result = {"schema_version": 1, "status": "failed", "scope": "local_only" if args.local_only else "local_and_native_synthetic",
              "product_complete": False, "inference": False, "pixel_review": "not_performed_by_runner",
              "external_gates": {"live_hermes_provider": "not_run", "window_plugin": "not_run", "parallel_live_actors": "not_run",
                                 "omp": "not_run", "license_publication": "owner_required"}, "commands": []}
    required = []
    smoke = load("autorun_native_helpers", ROOT / "scripts/smoke_native.py")
    try:
        result["source_witnesses"] = source_witnesses()
        if not args.local_only and shutil.which("tern", path=env.get("PATH")) is None:
            raise RuntimeError("Tern required for default autorun; use explicit --local-only for a different scope")
        def run(name, argv, timeout=180, child_env=None):
            required.append(name)
            result["status"] = "running"
            result["active_gate"] = name
            result["required_gates"] = list(required)
            # An interrupted outer caller leaves an explicit incomplete report,
            # not a missing report or a success for the last completed subset.
            (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
            row = run_command(argv, ROOT, env if child_env is None else child_env, out / (name + ".log"), timeout=timeout)
            row["name"] = name
            result["commands"].append(row)
            print(name + ": " + row["status"], flush=True)
            if row["status"] != "passed":
                raise RuntimeError("requested gate failed: " + name)
            return row
        run("workspace", ["./verify.sh"], timeout=600)
        run("metadata", ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"])
        metadata = json.loads((out / "metadata.log").read_text())
        packages = sorted(p["name"] for p in metadata["packages"])
        if not packages or len(set(packages)) != len(packages):
            raise ValueError("invalid package inventory")
        for package in packages:
            run(package + "-test", ["cargo", "test", "--locked", "-p", package, "--all-targets"])
            run(package + "-clippy", ["cargo", "clippy", "--locked", "-p", package, "--all-targets", "--", "-D", "warnings"])
        if not args.local_only:
            render_base = Path.home() / ".cache/ht-native"
            render_base.mkdir(mode=0o700, parents=True, exist_ok=True)
            native_env = dict(env, TMPDIR=str(render_base))
            result["native_roots"] = []
            for name, flags in [("preview", []), ("fixture-wide", ["--fixture"]), ("fixture-narrow", ["--fixture", "--size", "480x760"])]:
                run(name, [sys.executable, "-B", "scripts/smoke_native.py", *flags], timeout=120, child_env=native_env)
                lines = (out / (name + ".log")).read_text().splitlines()
                roots = [Path(s.removeprefix("Native smoke retained outputs: ")) for s in lines if s.startswith("Native smoke retained outputs: ")]
                if len(roots) != 1 or not roots[0].is_relative_to(render_base):
                    raise ValueError("static native result missing or outside owned roots")
                static = json.loads((roots[0] / "result.json").read_text())
                if static.get("status") != "passed" or len(static.get("outputs", [])) != 2:
                    raise ValueError("static native output incomplete")
                result["native_roots"].append(str(roots[0]))
            native_out = out / "native-chat"
            run("native-chat", [sys.executable, "-B", "scripts/smoke_native_chat.py", "--out", str(native_out)], timeout=600, child_env=native_env)
            native = json.loads((native_out / "result.json").read_text())
            validate_native_report(native)
            for case in native["cases"]:
                for image in case["outputs"]:
                    if hashlib.sha256(Path(image["image"]).read_bytes()).hexdigest() != image["sha256"] or hashlib.sha256(Path(image["layout"]).read_bytes()).hexdigest() != image["layout_sha256"]:
                        raise ValueError("native evidence changed after capture")
            result["native_result"] = str(native_out / "result.json")
        if source_witnesses() != result["source_witnesses"]:
            raise ValueError("repository changed during verification; result not current")
        result["status"] = gate_status(required, result["commands"])
        result["packages"] = packages
        result["binaries"] = {name: hashlib.sha256((ROOT / "target/debug" / name).read_bytes()).hexdigest()
                              for name in ("hermes-tern", "hermes-tern-testing")}
        result["cargo_lock_sha256"] = hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        result["status"] = "failed"
        result["error"] = str(error)
    finally:
        result.pop("active_gate", None)
        result["required_gates"] = required
        (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        owned = out / "ownership.json"
        owned.touch(mode=0o600)
        owned.write_text(json.dumps({"retention": "retain;no deletion", "paths": smoke.inventory(out)}, indent=2) + "\n")
    print("Requested scope: " + result["status"] + "; full product/live acceptance remains separate.")
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
