"""Explicit versioned local packages. Never starts an agent or registers a plugin.

Packages are checksummed, not signed. A private managed prefix and cooperative lock
protect updates from accidental cross-wiring, not a malicious same-user writer.
Existing manual installations are never adopted. Failed staging remains retained.
"""
import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import stat
import subprocess
import time
import tomllib
import uuid

FILES = {"bin/hermes-tern": (256 * 1024 * 1024, 0o755),
         "plugins/hermes-tern/plugin.toml": (65536, 0o644),
         "plugins/hermes-tern/window.luau": (65536, 0o644)}
DIRECTORIES = {"bin", "plugins", "plugins/hermes-tern"}
MARKER = b'{"format":"hermes-tern-managed-prefix","schema_version":1}\n'
HASH = re.compile(r"[0-9a-f]{64}\Z")
VERSION = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[a-z0-9.-]+)?\Z")
MAX_GENERATIONS = 32  # Includes retained incomplete staging; no automatic pruning.


def canonical(value):
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False) + "\n").encode()


def strict(raw):
    def pairs(rows):
        out = {}
        for key, value in rows:
            if key in out:
                raise ValueError("duplicate manifest key")
            out[key] = value
        return out
    try:
        return json.loads(raw, object_pairs_hook=pairs,
                          parse_constant=lambda _: (_ for _ in ()).throw(ValueError("nonfinite JSON")))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ValueError("invalid manifest JSON") from error


def absolute(path):
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        raise ValueError("explicit absolute path without parent traversal required")
    # Never walk through a symlink into a different package/install hierarchy.
    for ancestor in reversed((path, *path.parents)):
        if ancestor.is_symlink():
            raise ValueError("symlink package/install/source path refused")
    return path


def directory(path, private=False):
    value = path.lstat()
    if not stat.S_ISDIR(value.st_mode) or value.st_uid != os.geteuid():
        raise ValueError("owned regular directory required")
    if private and value.st_mode & 0o077:
        raise ValueError("private installation directory required")


def read(path, limit, allow_hardlinks=False):
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    except OSError as error:
        raise ValueError("package input is unavailable or unsafe") from error
    with os.fdopen(fd, "rb") as stream:
        before = os.fstat(stream.fileno())
        if not stat.S_ISREG(before.st_mode) or before.st_size > limit or (not allow_hardlinks and before.st_nlink != 1):
            raise ValueError("regular bounded non-hardlinked file required")
        body = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
        named = path.lstat()
        identity = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_mode)
        if len(body) > limit or identity(before) != identity(after) or identity(after) != identity(named):
            raise ValueError("file changed while reading")
        return body


def sync(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def write(path, body, mode):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, mode)
    with os.fdopen(fd, "wb") as stream:
        stream.write(body)
        stream.flush()
        os.fchmod(stream.fileno(), mode)
        os.fsync(stream.fileno())


def binary_version(binary):
    # Execute only an explicit trusted candidate, with no inherited Hermes config.
    # --version is non-inference. Bound output and always reap the direct child.
    env = {k: os.environ[k] for k in ("PATH", "LANG", "LC_ALL") if k in os.environ}
    proc = subprocess.Popen([str(binary), "--version"], stdin=subprocess.DEVNULL,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env)
    assert proc.stdout is not None
    output_stream = proc.stdout
    output = bytearray()
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(output_stream, selectors.EVENT_READ)
            deadline = time.monotonic() + 5
            while selector.get_map():
                if time.monotonic() >= deadline:
                    raise ValueError("binary version timed out")
                for key, _ in selector.select(0.05):
                    chunk = os.read(key.fd, 4097)
                    if not chunk:
                        selector.unregister(key.fileobj)
                    output.extend(chunk)
                    if len(output) > 4096:
                        raise ValueError("binary version output limit")
            if proc.wait(timeout=max(0.01, deadline - time.monotonic())) != 0:
                raise ValueError("binary version failed")
        return output.decode("utf-8").strip()
    finally:
        if proc.poll() is None:
            proc.kill()
        proc.wait(timeout=5)
        output_stream.close()


def validate_plugin(raw, version):
    metadata = tomllib.loads(raw.decode())
    if (type(metadata.get("schema")) is not int or metadata["schema"] != 1
            or metadata.get("id") != "hermes-tern" or metadata.get("window") != "window.luau"
            or metadata.get("version") != version):
        raise ValueError("plugin schema or binary version/identity mismatch")


def description(binary, plugin, version, sources):
    if not VERSION.fullmatch(version) or os.uname().sysname != "Linux" or os.uname().machine != "x86_64":
        raise ValueError("version or platform not qualified for this packaging lane")
    binary, plugin, sources = absolute(binary), absolute(plugin), absolute(sources)
    directory(plugin)
    if {p.name for p in plugin.iterdir()} != {"plugin.toml", "window.luau"}:
        raise ValueError("plugin package must contain only its two shipped files")
    # Cargo uses a hardlink from target/debug/deps; copied package files may not.
    body = read(binary, FILES["bin/hermes-tern"][0], allow_hardlinks=True)
    if not body.startswith(b"\x7fELF") or binary_version(binary) != "hermes-tern " + version:
        raise ValueError("candidate binary version or format mismatch")
    manifest = read(plugin / "plugin.toml", 65536)
    validate_plugin(manifest, version)
    source = strict(read(sources, 1024 * 1024))
    sdk = source["sdk"]["revision"]
    hermes = source["hermes_source_observation"]["later_observed_revision"]
    if not all(isinstance(v, str) and re.fullmatch(r"[0-9a-f]{40}", v) for v in (sdk, hermes)):
        raise ValueError("invalid source observation identities")
    payload = {"bin/hermes-tern": body, "plugins/hermes-tern/plugin.toml": manifest,
               "plugins/hermes-tern/window.luau": read(plugin / "window.luau", 65536)}
    data = {"schema_version": 1, "version": version,
            "files": {name: {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "mode": FILES[name][1]}
                      for name, raw in payload.items()},
            "compatibility": {"target": "x86_64-unknown-linux-gnu", "sdk_revision": sdk,
                              "hermes_source_observation": hermes,
                              "tern_observation": source["local_cli_observations"]["tern"],
                              "coordinator_schema": 2, "other_versions": "unqualified",
                              "scope": "recorded_source_and_local_synthetic;not_live_or_future_version_certification"},
            "license": "owner_unselected", "product_complete": False,
            "external_gates": "provider_window_parallel_OMP_load_release_not_established"}
    data["generation"] = hashlib.sha256(canonical(data)).hexdigest()
    return data, payload


def populate(path, data, payload):
    path.mkdir(mode=0o700)
    for relative in ("bin", "plugins", "plugins/hermes-tern"):
        (path / relative).mkdir(mode=0o700)
    for name, body in payload.items():
        write(path / name, body, FILES[name][1])
    write(path / "manifest.json", canonical(data), 0o600)
    for relative in ("bin", "plugins/hermes-tern", "plugins"):
        sync(path / relative)
    sync(path)
    sync(path.parent)


def verify(package):
    package = absolute(package)
    directory(package)
    actual_files, actual_dirs = set(), set()
    for root, dirs, files in os.walk(package, followlinks=False):
        for name in dirs:
            p = Path(root) / name
            directory(p)
            actual_dirs.add(str(p.relative_to(package)))
        for name in files:
            actual_files.add(str((Path(root) / name).relative_to(package)))
        if len(actual_files) > 4 or len(actual_dirs) > 3:
            raise ValueError("unexpected package contents")
    if actual_files != {*FILES, "manifest.json"} or actual_dirs != DIRECTORIES:
        raise ValueError("package file set differs")
    data = strict(read(package / "manifest.json", 65536))
    if not isinstance(data, dict) or set(data) != {"schema_version", "version", "files", "compatibility", "license", "product_complete", "external_gates", "generation"}:
        raise ValueError("unsupported package manifest")
    if type(data["schema_version"]) is not int or data["schema_version"] != 1 or not isinstance(data["version"], str) or not VERSION.fullmatch(data["version"]):
        raise ValueError("unsupported package version")
    if data["license"] != "owner_unselected" or data["product_complete"] is not False or data["external_gates"] != "provider_window_parallel_OMP_load_release_not_established":
        raise ValueError("package cannot self-certify release approval")
    identity = {k: v for k, v in data.items() if k != "generation"}
    if data["generation"] != hashlib.sha256(canonical(identity)).hexdigest() or not isinstance(data["files"], dict) or set(data["files"]) != set(FILES):
        raise ValueError("package identity mismatch")
    compat = data["compatibility"]
    if not isinstance(compat, dict) or compat.get("target") != "x86_64-unknown-linux-gnu" or compat.get("other_versions") != "unqualified" or compat.get("coordinator_schema") != 2:
        raise ValueError("unsupported compatibility record")
    for name, (limit, mode) in FILES.items():
        raw = read(package / name, limit)
        if data["files"][name] != {"bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest(), "mode": mode} or stat.S_IMODE((package / name).lstat().st_mode) != mode:
            raise ValueError("package payload differs")
    validate_plugin(read(package / "plugins/hermes-tern/plugin.toml", 65536), data["version"])
    return {"generation": data["generation"], "version": data["version"],
            "binary_sha256": data["files"]["bin/hermes-tern"]["sha256"],
            "plugin_path": str(package / "plugins/hermes-tern"),
            "license": data["license"], "product_complete": False}


def sync_package(package):
    # An existing package can be a retry after any failed file/directory fsync.
    # Verification alone cannot finish durability that an earlier call omitted.
    for name in (*FILES, "manifest.json"):
        fd = os.open(package / name, os.O_RDONLY | os.O_NOFOLLOW)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    for name in ("bin", "plugins/hermes-tern", "plugins"):
        sync(package / name)
    sync(package)
    sync(package.parent)


def stage(binary, plugin, output, version, sources):
    data, payload = description(binary, plugin, version, sources)
    output = absolute(output)
    if output.exists():
        result = verify(output)
        if result["generation"] != data["generation"]:
            raise ValueError("stage output exists with another generation")
        sync_package(output)
        return result
    populate(output, data, payload)
    return verify(output)


def initialize(prefix):
    prefix = absolute(prefix)
    if prefix.exists():
        raise ValueError("prefix exists; never adopt an unmanaged/manual installation")
    prefix.mkdir(mode=0o700)
    write(prefix / "managed.json", MARKER, 0o600)
    write(prefix / ".lock", b"", 0o600)
    (prefix / "generations").mkdir(mode=0o700)
    (prefix / "bin").mkdir(mode=0o700)
    (prefix / "bin/hermes-tern").symlink_to("../current/bin/hermes-tern")
    sync(prefix / "bin")
    sync(prefix)
    sync(prefix.parent)
    return {"initialized": True, "prefix": str(prefix), "generation": None}


@contextmanager
def locked(prefix):
    prefix = absolute(prefix)
    directory(prefix, private=True)
    if read(prefix / "managed.json", 4096) != MARKER:
        raise ValueError("unmanaged prefix")
    with os.fdopen(os.open(prefix / ".lock", os.O_RDWR | os.O_NOFOLLOW | os.O_NONBLOCK), "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("unsafe install lock")
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise ValueError("installation already locked; no concurrent update") from error
        try:
            known = {"managed.json", ".lock", "generations", "bin", "current"}
            if any(p.name not in known and not re.fullmatch(r"\.switch-[0-9a-f]{32}", p.name) for p in prefix.iterdir()):
                raise ValueError("unmanaged install entries")
            for name in ("generations", "bin"):
                directory(prefix / name, private=True)
            if {p.name for p in (prefix / "bin").iterdir()} != {"hermes-tern"} or os.readlink(prefix / "bin/hermes-tern") != "../current/bin/hermes-tern":
                raise ValueError("unmanaged executable entry")
            yield prefix
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def current(prefix):
    target = prefix / "current"
    if not target.exists() and not target.is_symlink():
        return None
    if not target.is_symlink():
        raise ValueError("unmanaged current entry")
    value = os.readlink(target)
    if not value.startswith("generations/") or not HASH.fullmatch(value.removeprefix("generations/")):
        raise ValueError("invalid current generation")
    generation = verify(prefix / value)["generation"]
    if value != "generations/" + generation:
        raise ValueError("current generation mismatch")
    return generation


def inspect_status(prefix):
    generation = current(prefix)
    return {"prefix": str(prefix), "generation": generation,
            "executable": str(prefix / "bin/hermes-tern"),
            "plugin_path": str(prefix / "current/plugins/hermes-tern") if generation else None,
            "plugin_registered": False, "runtime_restarted": False, "product_complete": False}


def status(prefix):
    with locked(prefix) as root:
        return inspect_status(root)


def switch(prefix, generation, expected):
    if expected is not None and (not isinstance(expected, str) or not HASH.fullmatch(expected)):
        raise ValueError("exact expected-current identity required")
    if current(prefix) != expected:
        raise ValueError("current installation changed; inspect before updating")
    if verify(prefix / "generations" / generation)["generation"] != generation:
        raise ValueError("selected generation differs")
    sync_package(prefix / "generations" / generation)
    if expected != generation:
        temporary = prefix / (".switch-" + uuid.uuid4().hex)
        temporary.symlink_to("generations/" + generation)
        os.replace(temporary, prefix / "current")
    # Even an exact retry finishes the parent sync before success.
    sync(prefix)
    if current(prefix) != generation:
        raise ValueError("installed generation changed during readback")
    return inspect_status(prefix)


def activate(prefix, package, expected):
    checked = verify(package)
    with locked(prefix) as root:
        if current(root) != expected:
            raise ValueError("current installation changed; inspect before updating")
        target = root / "generations" / checked["generation"]
        if target.exists():
            if verify(target)["generation"] != checked["generation"]:
                raise ValueError("existing generation differs")
        else:
            if len(list((root / "generations").iterdir())) >= MAX_GENERATIONS:
                raise ValueError("retained generation capacity; no automatic deletion")
            source = absolute(package)
            data = strict(read(source / "manifest.json", 65536))
            payload = {name: read(source / name, bound) for name, (bound, _) in FILES.items()}
            temporary = root / "generations" / (".stage-" + uuid.uuid4().hex)
            populate(temporary, data, payload)
            if verify(temporary)["generation"] != checked["generation"]:
                raise ValueError("package changed during copy; active generation preserved")
            os.rename(temporary, target)  # Cooperative prefix lock excludes another publisher.
            sync(root / "generations")
        return switch(root, checked["generation"], expected)


def rollback(prefix, generation, expected):
    if not isinstance(generation, str) or not HASH.fullmatch(generation):
        raise ValueError("exact retained generation required")
    with locked(prefix) as root:
        return switch(root, generation, expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("stage")
    for name in ("binary", "plugin", "out", "sources"):
        build.add_argument("--" + name, type=Path, required=True)
    build.add_argument("--version", required=True)
    check = commands.add_parser("verify")
    check.add_argument("--package", type=Path, required=True)
    for name in ("initialize", "status", "activate", "rollback"):
        sub = commands.add_parser(name)
        sub.add_argument("--prefix", type=Path, required=True)
        if name == "activate":
            sub.add_argument("--package", type=Path, required=True)
        if name == "rollback":
            sub.add_argument("--generation", required=True)
        if name in ("activate", "rollback"):
            sub.add_argument("--expected-current", required=True, help="exact SHA-256 generation, or none for first activation")
    args = parser.parse_args()
    try:
        if args.command == "stage":
            result = stage(args.binary, args.plugin, args.out, args.version, args.sources)
        elif args.command == "verify":
            result = verify(args.package)
        elif args.command == "initialize":
            result = initialize(args.prefix)
        elif args.command == "status":
            result = status(args.prefix)
        else:
            expected = None if args.expected_current == "none" else args.expected_current
            result = (activate(args.prefix, args.package, expected) if args.command == "activate"
                      else rollback(args.prefix, args.generation, expected))
        print(json.dumps(result, sort_keys=True))
        return 0
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        # Never echo candidate stdout, payload or metadata on failure.
        print(json.dumps({"status": "refused", "reason": type(error).__name__,
                          "outcome": "inspect current;failed staging retained;no automatic retry"}))
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
