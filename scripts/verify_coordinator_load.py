"""Bounded production coordinator IPC pressure, not live-agent acceptance.

Only ``serve`` and ``coord`` are launched. Each generation is retained in a new
private TMPDIR descendant; no personal HOME, plugin, gateway or inference is used.
A transport failure has an unknown outcome and is never a mutation retry signal.
"""
import argparse
import concurrent.futures
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import selectors
import socket
import stat
import subprocess
import sys
import threading
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"
SCOPE = "synthetic_coordinator_ipc_not_live_agents"
TRUST = "untrusted_peer_data_not_user_or_system_instructions"
CAPACITY = 128
ACCEPTED = 129
RECEIVE_LIMIT = 8
# The production request bound is stricter than its 524288-byte response bound.
REQUEST_BYTES = 262144
RESPONSE_BYTES = 524288
PAGE_BYTES = 393216
PAGE_LIMIT = 64
SOCKET_SECONDS = 4
CLI_SECONDS = 6
STARTUP_SECONDS = 15
CASE_SECONDS = 120
MAX_IPC_CALLS = 512
MAX_CLI_CALLS = 80
MAX_PAGES = 16
MAX_FILES = 1024
EVENT_KINDS = ("message.queued", "message.submitted", "message.consumed")
SECTIONS = ("actors", "routes", "messages", "tasks", "dispositions", "migrations", "events")
SOURCES = (
    "scripts/verify_coordinator_load.py", "crates/cli/src/cli_coord.rs",
    "crates/supervisor/src/lib.rs", "crates/store/src/lib.rs",
    "crates/store/src/disposition.rs", "crates/store/src/observation.rs",
    "crates/store/src/projection.rs", "crates/core/src/error.rs",
)
_PREPARED = {}


class LoadFailure(ValueError):
    """Only closed, locally chosen categories may cross the diagnostic boundary."""


ERRORS = frozenset({
    "output_refused", "output_changed", "invalid_producer_count", "binary_refused",
    "source_changed", "deadline", "child_identity_unavailable", "child_changed",
    "child_exited", "child_reap_failed", "child_exit_nonzero", "startup_deadline",
    "cli_deadline", "cli_output_limit", "cli_failed_outcome_unknown", "cli_call_limit",
    "ipc_call_limit", "ipc_frame_limit", "ipc_transport_outcome_unknown",
    "ipc_invalid_response_outcome_unknown", "ipc_refused_outcome_unknown",
    "unexpected_refusal", "invalid_json", "invalid_version", "invalid_actor",
    "invalid_private_file", "invalid_state_index", "invalid_state_page",
    "state_changed", "state_page_limit", "state_count_mismatch", "unexpected_state",
    "invalid_receipt", "duplicate_delivery", "payload_mismatch", "event_mismatch",
    "rss_unavailable", "inventory_limit", "inventory_changed", "report_invalid",
    "local_io_failure", "interrupted", "internal_failure",
})


def _fail(category):
    if category not in ERRORS:
        category = "internal_failure"
    raise LoadFailure(category) from None


def _json_bytes(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"),
                      ensure_ascii=False, allow_nan=False).encode("utf-8")


def _parse(data):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                _fail("invalid_json")
            result[key] = value
        return result

    def constant(_):
        _fail("invalid_json")

    try:
        return json.loads(data, object_pairs_hook=pairs, parse_constant=constant)
    except (ValueError, UnicodeError, RecursionError):
        _fail("invalid_json")


def _sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _hex(value):
    return type(value) is str and re.fullmatch(r"[0-9a-f]{64}", value) is not None


def _uuid(value):
    if type(value) is not str or len(value) != 36:
        return False
    try:
        return str(uuid.UUID(value)) == value
    except ValueError:
        return False


def _decimal(value, maximum=1000000):
    if type(value) is not str or re.fullmatch(r"0|[1-9][0-9]{0,18}", value) is None:
        _fail("report_invalid")
    number = int(value)
    if number > maximum:
        _fail("report_invalid")
    return number


def _identity(st):
    return (st.st_dev, st.st_ino)


def _directories(path):
    """Reject symlinks, including ancestors, rather than canonicalizing them away."""
    path = Path(path)
    if not path.is_absolute() or ".." in path.parts:
        _fail("output_refused")
    for current in (Path(path.anchor), *reversed(path.parents[:-1]), path):
        info = current.lstat()
        if not stat.S_ISDIR(info.st_mode):
            _fail("output_refused")
    return path


def _tmp_root():
    value = os.environ.get("TMPDIR")
    if not value:
        _fail("output_refused")
    root = _directories(Path(value))
    info = root.lstat()
    if info.st_uid != os.geteuid() or stat.S_IMODE(info.st_mode) != 0o700:
        _fail("output_refused")
    return root


def prepare_output(path):
    """Create exactly one absent, absolute, non-symlink private output directory."""
    path = Path(path)
    tmp = _tmp_root()
    if not path.is_absolute() or ".." in path.parts or path == tmp or not path.is_relative_to(tmp):
        _fail("output_refused")
    _directories(path.parent)
    parent = path.parent.lstat()
    if parent.st_uid != os.geteuid() or stat.S_IMODE(parent.st_mode) & 0o077:
        _fail("output_refused")
    path.mkdir(mode=0o700, parents=False, exist_ok=False)
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
    try:
        os.fchmod(fd, 0o700)
        info = os.fstat(fd)
        if _identity(info) != _identity(path.lstat()):
            _fail("output_changed")
        _PREPARED[str(path)] = _identity(info)
    finally:
        os.close(fd)
    return path


def _check_output(out, empty=False):
    _directories(out)
    info = out.lstat()
    if (_PREPARED.get(str(out)) != _identity(info) or info.st_uid != os.geteuid()
            or stat.S_IMODE(info.st_mode) != 0o700):
        _fail("output_changed")
    if empty and any(out.iterdir()):
        _fail("output_refused")


def _write_new(path, value):
    data = _json_bytes(value) + b"\n"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as stream:
        stream.write(data)


def _private_text(path, parent, limit=4096):
    path = Path(path)
    if not path.is_absolute() or not path.is_relative_to(parent):
        _fail("invalid_private_file")
    _directories(path.parent)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        before = os.fstat(stream.fileno())
        if (not stat.S_ISREG(before.st_mode) or before.st_uid != os.geteuid()
                or stat.S_IMODE(before.st_mode) != 0o600 or before.st_nlink != 1
                or before.st_size > limit):
            _fail("invalid_private_file")
        body = stream.read(limit + 1)
        after = os.fstat(stream.fileno())
        if (len(body) > limit or _identity(before) != _identity(path.lstat())
                or (before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                != (after.st_size, after.st_mtime_ns, after.st_ctime_ns)):
            _fail("invalid_private_file")
    try:
        return body.decode("utf-8")
    except UnicodeError:
        _fail("invalid_private_file")


def _process_identity(pid):
    try:
        fields = (Path("/proc") / str(pid) / "stat").read_text().rsplit(")", 1)[1].split()
        return {"pid": pid, "ppid": int(fields[1]), "pgrp": int(fields[2]),
                "session": int(fields[3]), "start_ticks": fields[19]}
    except (OSError, ValueError, IndexError):
        return None


def _reap(child, allowance):
    """Signal only the exact direct Popen child; never search or signal a group."""
    if child.poll() is None:
        child.kill()
    try:
        return child.wait(timeout=max(0.01, allowance))
    except subprocess.TimeoutExpired:
        _fail("child_reap_failed")


def _scan(out):
    _check_output(out)
    entries = []
    stack = [out]
    while stack:
        path = stack.pop()
        info = path.lstat()
        if info.st_uid != os.geteuid() or not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode)):
            _fail("inventory_changed")
        entries.append({"path": str(path), "device": info.st_dev, "inode": info.st_ino,
                        "mode": stat.S_IMODE(info.st_mode),
                        "kind": "directory" if stat.S_ISDIR(info.st_mode) else "regular_file",
                        "bytes": info.st_size if stat.S_ISREG(info.st_mode) else 0})
        if len(entries) > MAX_FILES:
            _fail("inventory_limit")
        if stat.S_ISDIR(info.st_mode):
            with os.scandir(path) as children:
                for child in children:
                    if len(entries) + len(stack) >= MAX_FILES:
                        _fail("inventory_limit")
                    stack.append(Path(child.path))
    return sorted(entries, key=lambda item: item["path"])


def _inventory(out, children):
    # Include the manifest's own exact inode without self-referential byte lengths.
    path = out / "ownership.json"
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "wb") as stream:
        paths = [{key: value for key, value in item.items() if key != "bytes"}
                 for item in _scan(out)]
        stream.write(_json_bytes({"schema_version": 1, "root": str(out),
                                  "retention": "retain; no deletion", "paths": paths,
                                  "direct_children": children}) + b"\n")


class _Run:
    def __init__(self, out, binary):
        self.out, self.binary = out, binary
        self.started = time.monotonic()
        # Reserve three seconds for exact-child failure reaping and final receipts.
        self.deadline = self.started + CASE_SECONDS - 3
        self.state, self.workspace = out / "state", out / "workspace"
        names = ("home", "workspace", "tmp", "config", "cache", "data", "xdg-state", "runtime")
        for name in names:
            (out / name).mkdir(mode=0o700)
        (out / "home/.hermes").mkdir(mode=0o700)
        self.env = {"PATH": os.defpath, "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
                    "HOME": str(out / "home"), "HERMES_HOME": str(out / "home/.hermes"),
                    "TMPDIR": str(out / "tmp"), "TMP": str(out / "tmp"), "TEMP": str(out / "tmp"),
                    "XDG_CONFIG_HOME": str(out / "config"), "XDG_CACHE_HOME": str(out / "cache"),
                    "XDG_DATA_HOME": str(out / "data"), "XDG_STATE_HOME": str(out / "xdg-state"),
                    "XDG_RUNTIME_DIR": str(out / "runtime")}
        self.child = None
        self.child_identity = None
        self.children = []
        self.lock = threading.Lock()
        self.ipc_calls = 0
        self.cli_calls = 0
        self.latencies = {}
        self.rss = None
        self.rss_samples = 0
        self.page_calls = 0

    def remaining(self, bound):
        remaining = self.deadline - time.monotonic()
        if remaining <= 0:
            _fail("deadline")
        return min(bound, remaining)

    def sample(self):
        if self.child is None or self.child.poll() is not None:
            return
        if _process_identity(self.child.pid) != self.child_identity:
            _fail("child_changed")
        try:
            with (Path("/proc") / str(self.child.pid) / "status").open("rb") as stream:
                raw = stream.read(65537)
            match = re.search(rb"^VmHWM:\s+([0-9]+) kB$", raw, re.MULTILINE)
            if len(raw) > 65536 or match is None:
                _fail("rss_unavailable")
            value = int(match[1]) * 1024
            if value <= 0 or _process_identity(self.child.pid) != self.child_identity:
                _fail("rss_unavailable")
            with self.lock:
                self.rss = max(self.rss or 0, value)
                self.rss_samples += 1
        except OSError:
            _fail("rss_unavailable")

    def cli(self, args, timeout=CLI_SECONDS, readiness=False):
        if self.cli_calls >= MAX_CLI_CALLS:
            _fail("cli_call_limit")
        self.cli_calls += 1
        deadline = time.monotonic() + self.remaining(timeout)
        child = subprocess.Popen([str(self.binary), *args], cwd=self.workspace, env=self.env,
                                 stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE, start_new_session=True)
        record = {"operation": "status" if readiness else args[0],
                  "identity": _process_identity(child.pid), "reaped": False, "exit_code": None}
        self.children.append(record)
        buffers = {"stdout": bytearray(), "stderr": bytearray()}
        try:
            if record["identity"] is None or record["identity"]["ppid"] != os.getpid():
                _fail("child_identity_unavailable")
            with selectors.DefaultSelector() as selector:
                selector.register(child.stdout, selectors.EVENT_READ, "stdout")
                selector.register(child.stderr, selectors.EVENT_READ, "stderr")
                while selector.get_map():
                    left = deadline - time.monotonic()
                    if left <= 0:
                        _fail("cli_deadline")
                    for key, _ in selector.select(min(left, 0.05)):
                        chunk = os.read(key.fileobj.fileno(), 65536)
                        if not chunk:
                            selector.unregister(key.fileobj)
                        else:
                            buf = buffers[key.data]
                            if len(buf) + len(chunk) > (RESPONSE_BYTES if key.data == "stdout" else 8192):
                                _fail("cli_output_limit")
                            buf.extend(chunk)
                    if readiness and self.child.poll() is not None:
                        _fail("child_exited")
            left = deadline - time.monotonic()
            if left <= 0:
                _fail("cli_deadline")
            code = child.wait(timeout=left)
            if not readiness and code != 0:
                _fail("cli_failed_outcome_unknown")
            return code, bytes(buffers["stdout"])
        except subprocess.TimeoutExpired:
            _fail("cli_deadline")
        finally:
            # Cleanup must run even when the operation consumed its deadline.
            record["exit_code"] = _reap(child, 2)
            record["reaped"] = child.returncode is not None
            child.stdout.close()
            child.stderr.close()

    def start(self):
        self.child = subprocess.Popen(
            [str(self.binary), "serve", "--state", str(self.state), "--workspace", str(self.workspace)],
            cwd=self.workspace, env=self.env, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
        self.child_identity = _process_identity(self.child.pid)
        self.children.append({"operation": "serve", "identity": self.child_identity,
                              "reaped": False, "exit_code": None})
        if self.child_identity is None or self.child_identity["ppid"] != os.getpid():
            _fail("child_identity_unavailable")
        # Same fixed read-only status/readiness contract as fixture_process.wait_supervisor.
        began = time.monotonic()
        deadline = began + self.remaining(STARTUP_SECONDS)
        for _ in range(64):
            if self.child.poll() is not None:
                _fail("child_exited")
            left = deadline - time.monotonic()
            if left <= 0:
                _fail("startup_deadline")
            code, _ = self.cli(["coord", "--state", str(self.state), "status"],
                               timeout=min(CLI_SECONDS, left), readiness=True)
            if time.monotonic() >= deadline or self.child.poll() is not None:
                _fail("startup_deadline")
            if code == 0:
                self.sample()
                return time.monotonic() - began
            time.sleep(min(0.02, max(0, deadline - time.monotonic())))
        _fail("startup_deadline")

    def call(self, secret, op):
        with self.lock:
            if self.ipc_calls >= MAX_IPC_CALLS:
                _fail("ipc_call_limit")
            self.ipc_calls += 1
        started = time.monotonic()
        deadline = started + self.remaining(SOCKET_SECONDS)
        request_id = str(uuid.uuid4())
        frame = _json_bytes({"v": 1, "id": request_id, "auth": secret, "op": op}) + b"\n"
        if len(frame) > REQUEST_BYTES:
            _fail("ipc_frame_limit")
        address = "\0hermes-tern-" + str(os.geteuid()) + "-" + hashlib.sha256(os.fsencode(self.state)).hexdigest()
        try:
            with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as stream:
                stream.settimeout(max(0.001, deadline - time.monotonic()))
                stream.connect(address)
                stream.settimeout(max(0.001, deadline - time.monotonic()))
                stream.sendall(frame)
                body = bytearray()
                while True:
                    left = deadline - time.monotonic()
                    if left <= 0:
                        _fail("ipc_transport_outcome_unknown")
                    stream.settimeout(left)
                    chunk = stream.recv(min(65536, RESPONSE_BYTES + 1 - len(body)))
                    if not chunk or len(body) + len(chunk) > RESPONSE_BYTES:
                        _fail("ipc_invalid_response_outcome_unknown")
                    body.extend(chunk)
                    if b"\n" in chunk:
                        if body[-1:] != b"\n" or body.count(b"\n") != 1:
                            _fail("ipc_invalid_response_outcome_unknown")
                        break
            response = _parse(body)
            if (type(response) is not dict or type(response.get("v")) is not int
                    or response["v"] != 1 or response.get("id") != request_id
                    or type(response.get("ok")) is not bool
                    or (response["ok"] and type(response.get("result")) is not dict)
                    or (not response["ok"] and type(response.get("error")) is not str)):
                _fail("ipc_invalid_response_outcome_unknown")
            return response
        except OSError:
            _fail("ipc_transport_outcome_unknown")
        finally:
            elapsed = time.monotonic() - started
            with self.lock:
                self.latencies.setdefault(op["type"], []).append(elapsed)
            # Shutdown can exit before the last sample; sample before that call instead.
            if op["type"] != "shutdown":
                self.sample()

    def ok(self, secret, op):
        response = self.call(secret, op)
        if response["ok"] is not True:
            _fail("ipc_refused_outcome_unknown")
        return response["result"]

    def index(self, operator):
        index = self.ok(operator, {"type": "state_index"})
        if (index.get("schema_version") != 1 or index.get("database_schema") != 2
                or not _uuid(index.get("workspace_id")) or not _uuid(index.get("observation_epoch"))
                or not _hex(index.get("revision")) or index.get("event_replay_complete") is not True
                or index.get("response_bytes") != PAGE_BYTES or index.get("max_page_limit") != 128
                or index.get("trust") != TRUST or set(index.get("counts", {})) != set(SECTIONS)):
            _fail("invalid_state_index")
        for section in SECTIONS:
            _decimal(index["counts"][section], 512)
        return index

    def enumerate(self, operator, sections=SECTIONS):
        index = self.index(operator)
        result = {}
        for section in sections:
            total = _decimal(index["counts"][section], 512)
            items = []
            for _ in range(MAX_PAGES):
                offset = len(items)
                page = self.ok(operator, {"type": "state_page", "workspace": index["workspace_id"],
                    "epoch": index["observation_epoch"], "revision": index["revision"],
                    "section": section, "offset": str(offset), "limit": PAGE_LIMIT})
                self.page_calls += 1
                if page.get("status") == "restart_required":
                    _fail("state_changed")
                if (len(_json_bytes(page)) > PAGE_BYTES or page.get("status") != "ok"
                        or page.get("schema_version") != 1 or page.get("trust") != TRUST
                        or any(page.get(k) != index[k] for k in ("workspace_id", "observation_epoch", "revision"))
                        or page.get("section") != section or page.get("offset") != str(offset)
                        or page.get("limit") != PAGE_LIMIT or page.get("total") != str(total)
                        or type(page.get("items")) is not list or len(page["items"]) > PAGE_LIMIT
                        or type(page.get("has_more")) is not bool or type(page.get("byte_limited")) is not bool):
                    _fail("invalid_state_page")
                items.extend(page["items"])
                if (len(items) > total or page.get("next_offset") != str(len(items))
                        or page["has_more"] != (len(items) < total)
                        or (page["has_more"] and len(items) == offset)):
                    _fail("invalid_state_page")
                if not page["has_more"]:
                    break
            else:
                _fail("state_page_limit")
            if len(items) != total:
                _fail("state_count_mismatch")
            result[section] = items
        # No mutable assembly/restart/retry: every page and the final index must bind.
        if self.index(operator) != index:
            _fail("state_changed")
        return index, result

    def finish(self):
        if self.child is None:
            return
        record = next(item for item in self.children if item["operation"] == "serve")
        try:
            if self.child.poll() is None:
                self.sample()
        finally:
            # Sampling failure must not skip exact-child reaping. No operation retry.
            record["exit_code"] = _reap(self.child, 2)
            record["reaped"] = self.child.returncode is not None


def _proof(message, state):
    if (type(message) is not dict or not all(_uuid(message.get(k)) for k in
            ("sender_actor_id", "recipient_actor_id", "message_id"))
            or message.get("state") != state or message.get("kind") != "context"
            or message.get("correlation_id") is not None or type(message.get("payload")) is not dict):
        _fail("unexpected_state")
    if _decimal(message.get("sequence"), ACCEPTED) == 0:
        _fail("invalid_receipt")
    return {"sender_actor_id": message["sender_actor_id"], "message_id": message["message_id"],
            "recipient_actor_id": message["recipient_actor_id"], "sequence": message["sequence"],
            "payload_sha256": hashlib.sha256(_json_bytes(message["payload"])).hexdigest(), "state": state}


def _key(proof):
    return (proof["sender_actor_id"], proof["message_id"])


def _same_proof(left, right):
    return all(left.get(k) == right.get(k) for k in
               ("sender_actor_id", "message_id", "recipient_actor_id", "sequence", "payload_sha256"))


def _pending(run, operator, expected, retained):
    # Counts are full store counts, not the snapshot's 128-row metadata overview.
    snapshot = run.ok(operator, {"type": "snapshot"})
    if (snapshot.get("unresolved_message_count") != str(expected)
            or snapshot.get("counts", {}).get("messages") != str(retained)):
        _fail("unexpected_state")
    return _decimal(snapshot["unresolved_message_count"], CAPACITY)


def _refuse(run, secret, op, expected):
    response = run.call(secret, op)
    if response["ok"] is not False or response.get("error") != expected:
        _fail("unexpected_refusal")


def _unchanged_counts(before, after):
    if (before["counts"] != after["counts"] or before["event_watermark"] != after["event_watermark"]
            or before["message_watermark"] != after["message_watermark"]):
        _fail("state_count_mismatch")


def _latencies(samples):
    result = {}
    for operation, values in sorted(samples.items()):
        values = sorted(values)
        result[operation] = {"count": len(values), "min": values[0], "max": values[-1],
                             "mean": sum(values) / len(values),
                             "p50": values[(len(values) - 1) // 2],
                             "p95": values[math.ceil(len(values) * 0.95) - 1]}
    return result


def _exercise(run, producers):
    startup = run.start()
    _, version = run.cli(["--version"])
    if re.fullmatch(rb"hermes-tern [0-9]+\.[0-9]+\.[0-9]+\n", version) is None:
        _fail("invalid_version")
    # Persist only the parsed closed version grammar, never arbitrary CLI output.
    binary_version = version.decode("ascii").rstrip("\n").split(" ")[1]
    operator = _private_text(run.state / "operator.key", run.state)
    if not _hex(operator):
        _fail("invalid_private_file")
    hello = run.ok(operator, {"type": "hello"})
    if hello.get("protocol") != "hermes-tern-1" or not _uuid(hello.get("workspace_id")):
        _fail("invalid_version")
    workspace = hello["workspace_id"]
    actors, secrets = [], []
    for i in range(producers + 1):
        session = f"synthetic-producer-{i + 1}" if i < producers else "synthetic-receiver"
        _, output = run.cli(["coord", "--state", str(run.state), "register", "--kind", "hermes",
                            "--profile", "synthetic-no-runtime", "--session", session,
                            "--model", "synthetic", "--provider", "no-inference"])
        actor = _parse(output)
        if (type(actor) is not dict or not _uuid(actor.get("actor_id"))
                or not _uuid(actor.get("incarnation")) or type(actor.get("credential_file")) is not str):
            _fail("invalid_actor")
        expected_path = run.state / "credentials" / f"{actor['actor_id']}-{actor['incarnation']}.json"
        if actor["credential_file"] != str(expected_path):
            _fail("invalid_actor")
        private = _parse(_private_text(expected_path, run.state))
        if (private.get("schema_version") != 1 or private.get("workspace_id") != workspace
                or private.get("actor_id") != actor["actor_id"]
                or private.get("incarnation") != actor["incarnation"] or not _hex(private.get("credential"))):
            _fail("invalid_private_file")
        secrets.append(private["credential"])
        actors.append({"actor_id": actor["actor_id"], "incarnation": actor["incarnation"],
                       "session_id": session, "role": "producer" if i < producers else "receiver"})
    if len({a["actor_id"] for a in actors}) != producers + 1:
        _fail("invalid_actor")
    receiver = actors[-1]["actor_id"]
    for actor in actors[:-1]:
        _, body = run.cli(["coord", "--state", str(run.state), "route", "--sender", actor["actor_id"],
                            "--recipient", receiver, "--allow"])
        if _parse(body) != {"allowed": True}:
            _fail("invalid_receipt")
    envelopes = [{"message_id": str(uuid.uuid4()), "recipient": receiver, "kind": "context",
                  "payload": {"text": f"untrusted synthetic coordinator load item {i:03d}"},
                  "ttl_seconds": 300, "correlation_id": None} for i in range(ACCEPTED)]
    if len({message["message_id"] for message in envelopes}) != ACCEPTED:
        _fail("invalid_receipt")
    accepted, deliveries = {}, {}

    def send(index):
        sender = index % producers
        message = envelopes[index]
        receipt = run.ok(secrets[sender], {"type": "send", "message": message})
        if (receipt.get("duplicate") is not False or receipt.get("state") != "queued"
                or receipt.get("message_id") != message["message_id"]):
            _fail("invalid_receipt")
        return _proof({**message, "sender_actor_id": actors[sender]["actor_id"],
                       "recipient_actor_id": receiver, "sequence": receipt.get("sequence"),
                       "state": receipt["state"]}, "queued")

    # At most four running or queued futures, not 128 queued tasks or an unbounded map.
    with concurrent.futures.ThreadPoolExecutor(max_workers=producers) as pool:
        for offset in range(0, CAPACITY, producers):
            futures = [pool.submit(send, i) for i in range(offset, offset + producers)]
            for future in futures:
                proof = future.result(timeout=run.remaining(SOCKET_SECONDS + 0.5))
                if _key(proof) in accepted:
                    _fail("invalid_receipt")
                accepted[_key(proof)] = proof
    before, full = run.enumerate(operator, ("messages",))
    if len(full["messages"]) != CAPACITY:
        _fail("state_count_mismatch")
    for record in full["messages"]:
        proof = _proof(record.get("message"), "queued")
        if (_key(proof) not in accepted or not _same_proof(proof, accepted[_key(proof)])
                or record.get("trust") != TRUST or record.get("workspace_id") != workspace):
            _fail("payload_mismatch")
    pending_at_capacity = _pending(run, operator, CAPACITY, CAPACITY)
    candidate = {"type": "send", "message": envelopes[CAPACITY]}
    candidate_sender = CAPACITY % producers
    capacity_error = "storage error: mailbox capacity; accepted work preserved"
    _refuse(run, secrets[candidate_sender], candidate, capacity_error)
    duplicate = run.ok(secrets[0], {"type": "send", "message": envelopes[0]})
    first_key = (actors[0]["actor_id"], envelopes[0]["message_id"])
    if (duplicate.get("duplicate") is not True or duplicate.get("state") != "queued"
            or duplicate.get("message_id") != first_key[1]
            or duplicate.get("sequence") != accepted[first_key]["sequence"]):
        _fail("invalid_receipt")
    conflicting = {**envelopes[0], "payload": {"text": "different untrusted synthetic content"}}
    _refuse(run, secrets[0], {"type": "send", "message": conflicting},
            "storage error: message id conflicts with accepted content")
    _unchanged_counts(before, run.index(operator))
    max_batch = 0

    def receive():
        nonlocal max_batch
        result = run.ok(secrets[-1], {"type": "receive"})
        messages = result.get("messages")
        if result.get("limit") != RECEIVE_LIMIT or type(messages) is not list or len(messages) > RECEIVE_LIMIT:
            _fail("unexpected_state")
        max_batch = max(max_batch, len(messages))
        batch = []
        for message in messages:
            proof = _proof(message, "submitted")
            key = _key(proof)
            if key in deliveries:
                _fail("duplicate_delivery")
            if (key not in accepted or not _same_proof(proof, accepted[key])
                    or message.get("workspace_id") != workspace or message.get("trust") != TRUST):
                _fail("payload_mismatch")
            deliveries[key] = proof
            batch.append(proof)
        return batch

    def ack(proof):
        result = run.ok(secrets[-1], {"type": "ack", "sender": proof["sender_actor_id"],
                                    "message": proof["message_id"]})
        if result != {"message_id": proof["message_id"], "state": "consumed"}:
            _fail("invalid_receipt")

    first_batch = receive()
    if len(first_batch) != RECEIVE_LIMIT:
        _fail("unexpected_state")
    pending_after_receive = _pending(run, operator, CAPACITY, CAPACITY)
    _refuse(run, secrets[candidate_sender], candidate, capacity_error)
    ack(first_batch[0])
    pending_after_ack = _pending(run, operator, CAPACITY - 1, CAPACITY)
    # This deliberate resubmission follows two definitive capacity refusals, not I/O uncertainty.
    proof = send(CAPACITY)
    accepted[_key(proof)] = proof
    for delivered in first_batch[1:]:
        ack(delivered)
    for _ in range(math.ceil(ACCEPTED / RECEIVE_LIMIT)):
        batch = receive()
        for delivered in batch:
            ack(delivered)
        if not batch:
            break
    else:
        _fail("unexpected_state")
    if len(deliveries) != ACCEPTED or set(deliveries) != set(accepted):
        _fail("state_count_mismatch")
    before_repeat = run.index(operator)
    ack(first_batch[0])
    if receive():
        _fail("unexpected_state")
    _unchanged_counts(before_repeat, run.index(operator))
    pending_final = _pending(run, operator, 0, ACCEPTED)
    index, full = run.enumerate(operator)
    counts = {section: _decimal(index["counts"][section], 512) for section in SECTIONS}
    expected_counts = {"actors": producers + 1, "routes": producers, "messages": ACCEPTED,
                       "tasks": 0, "dispositions": 0, "migrations": 0,
                       "events": ACCEPTED * 3 + producers * 2 + 1}
    if counts != expected_counts or index["workspace_id"] != workspace:
        _fail("state_count_mismatch")
    if ({(a.get("actor_id"), a.get("incarnation")) for a in full["actors"]}
            != {(a["actor_id"], a["incarnation"]) for a in actors}
            or {(r.get("sender_actor_id"), r.get("recipient_actor_id")) for r in full["routes"]}
            != {(a["actor_id"], receiver) for a in actors[:-1]}):
        _fail("invalid_actor")
    observed = []
    for record in full["messages"]:
        message = record.get("message")
        proof = _proof(message, "consumed")
        if (record.get("trust") != TRUST or record.get("workspace_id") != workspace
                or record.get("observation_epoch") != index["observation_epoch"]
                or record.get("disposition") is not None or record.get("origin") is not None
                or message.get("delivery_incarnation") != actors[-1]["incarnation"]
                or message.get("ttl_seconds") != 300 or _key(proof) not in accepted
                or not _same_proof(proof, accepted[_key(proof)])):
            _fail("payload_mismatch")
        observed.append(proof)
    events = {kind: [] for kind in EVENT_KINDS}
    for event in full["events"]:
        if event.get("kind") in events:
            events[event["kind"]].append(event)
        elif event.get("kind") not in ("actor.registered", "route.allowed"):
            _fail("event_mismatch")
    run.sample()
    shutdown = run.ok(operator, {"type": "shutdown"})
    if shutdown != {"stopping": True}:
        _fail("invalid_receipt")
    try:
        code = run.child.wait(timeout=run.remaining(3))
    except subprocess.TimeoutExpired:
        _fail("child_reap_failed")
    if code != 0:
        _fail("child_exit_nonzero")
    serve = next(item for item in run.children if item["operation"] == "serve")
    serve.update(reaped=True, exit_code=code)
    return {"schema_version": 1, "status": "passed", "scope": SCOPE,
            "producers": producers, "registered_actor_records": len(actors), "actors": actors,
            "workspace_id": workspace, "observation_epoch": index["observation_epoch"],
            "state_revision": index["revision"], "state_counts": counts,
            "accepted_unique": len(accepted), "received_unique": len(deliveries),
            "consumed_unique": len(observed), "capacity_refusals": 2, "duplicate_admissions": 1,
            "conflicting_admissions_refused": 1, "duplicate_deliveries": 0,
            "lost_accepted": len(set(accepted) - {_key(p) for p in observed}),
            "pending_at_capacity": pending_at_capacity, "pending_after_receive": pending_after_receive,
            "pending_after_one_ack": pending_after_ack, "pending_final": pending_final,
            "max_receive_batch": max_batch, "repeat_ack_idempotent": True, "empty_receive": True,
            "accepted": sorted(accepted.values(), key=lambda p: int(p["sequence"])),
            "deliveries": sorted(deliveries.values(), key=lambda p: int(p["sequence"])),
            "observed": observed, "event_sets": events,
            "event_counts": {kind: len(events[kind]) for kind in EVENT_KINDS},
            "event_identity_scope": "entity_message_uuid_bound_to_globally_distinct_fixture_ids_not_sender_attribution",
            "capacity_refused_message_id": envelopes[CAPACITY]["message_id"],
            "consumer_policy": "withhold_all_acks_through_capacity_and_first_eight_submissions",
            "child_identity": run.child_identity, "child_reaped": True, "child_exit_code": code,
            "direct_children": run.children, "inference": False, "live_actors": False,
            "startup_seconds": startup, "binary_version": binary_version,
            "rss_high_water_bytes": run.rss, "rss_samples": run.rss_samples,
            "rss_scope": "linux_proc_direct_child_VmHWM_sampled_high_water_not_enforced_limit",
            "latency_seconds": _latencies(run.latencies), "ipc_calls": run.ipc_calls,
            "cli_calls": run.cli_calls, "state_page_calls": run.page_calls,
            "bounds": {"request_frame_bytes": REQUEST_BYTES, "response_frame_bytes": RESPONSE_BYTES,
                       "state_page_bytes": PAGE_BYTES, "receive_batch": RECEIVE_LIMIT,
                       "pending_per_recipient": CAPACITY, "workers": producers,
                       "max_inflight_futures": producers, "socket_seconds": SOCKET_SECONDS,
                       "cli_seconds": CLI_SECONDS, "startup_seconds": STARTUP_SECONDS,
                       "case_seconds": CASE_SECONDS, "max_ipc_calls": MAX_IPC_CALLS,
                       "max_cli_calls": MAX_CLI_CALLS, "max_pages_per_section": MAX_PAGES},
            "limitations": ["synthetic_records_not_live_agents", "no_inference_or_native_rendering",
                            "three_bounded_cases_not_sustained_load_qualification",
                            "no_ENOSPC_power_loss_maximum_size_storage_or_restore_qualification",
                            "no_latency_benchmark_threshold_or_enforced_RSS_or_disk_quota"]}


def validate_case(row):
    """Reject nonfinite, boolean-as-count, incomplete or identity-unbound evidence."""
    try:
        _validate_case(row)
    except (KeyError, TypeError, ValueError, OverflowError, RecursionError):
        raise ValueError("report_invalid") from None


def _fields(value, names):
    if type(value) is not dict or set(value) != set(names.split()):
        _fail("report_invalid")


def _validate_case(row):
    _fields(row, """schema_version status scope producers registered_actor_records actors
        workspace_id observation_epoch state_revision state_counts accepted_unique
        received_unique consumed_unique capacity_refusals duplicate_admissions
        conflicting_admissions_refused duplicate_deliveries lost_accepted pending_at_capacity
        pending_after_receive pending_after_one_ack pending_final max_receive_batch
        repeat_ack_idempotent empty_receive accepted deliveries observed event_sets event_counts
        event_identity_scope capacity_refused_message_id consumer_policy child_identity
        child_reaped child_exit_code direct_children inference live_actors startup_seconds
        binary_version rss_high_water_bytes rss_samples rss_scope latency_seconds ipc_calls
        cli_calls state_page_calls bounds limitations production_binary_sha256 source_sha256
        host retained_files retained_file_bytes retained_file_bytes_scope wall_seconds""")
    if type(row["schema_version"]) is not int or row["schema_version"] != 1 or row.get("status") != "passed" or row.get("scope") != SCOPE:
        _fail("report_invalid")
    producers = row["producers"]
    if type(producers) is not int or producers not in (1, 2, 4):
        _fail("report_invalid")
    fixed = {"registered_actor_records": producers + 1, "accepted_unique": ACCEPTED,
             "received_unique": ACCEPTED, "consumed_unique": ACCEPTED, "capacity_refusals": 2,
             "duplicate_admissions": 1, "conflicting_admissions_refused": 1, "duplicate_deliveries": 0,
             "lost_accepted": 0, "pending_at_capacity": CAPACITY, "pending_after_receive": CAPACITY,
             "pending_after_one_ack": CAPACITY - 1, "pending_final": 0, "child_exit_code": 0}
    for key, expected in fixed.items():
        if type(row[key]) is not int or row[key] != expected:
            _fail("report_invalid")
    for key, expected in {"child_reaped": True, "inference": False, "live_actors": False,
                          "repeat_ack_idempotent": True, "empty_receive": True}.items():
        if row[key] is not expected:
            _fail("report_invalid")
    for key, maximum in {"max_receive_batch": RECEIVE_LIMIT, "ipc_calls": MAX_IPC_CALLS,
                         "cli_calls": MAX_CLI_CALLS, "state_page_calls": MAX_IPC_CALLS}.items():
        if type(row[key]) is not int or not 0 < row[key] <= maximum:
            _fail("report_invalid")
    for key in ("rss_high_water_bytes", "rss_samples", "retained_file_bytes"):
        if type(row[key]) is not int or row[key] <= 0:
            _fail("report_invalid")
    for key, maximum in (("wall_seconds", CASE_SECONDS), ("startup_seconds", STARTUP_SECONDS)):
        value = row[key]
        if type(value) not in (int, float) or not math.isfinite(value) or not 0 < value <= maximum:
            _fail("report_invalid")
    for key in ("workspace_id", "observation_epoch"):
        if not _uuid(row[key]):
            _fail("report_invalid")
    if not _hex(row["production_binary_sha256"]) or not _hex(row["state_revision"]):
        _fail("report_invalid")
    if (type(row["source_sha256"]) is not dict or set(row["source_sha256"]) != set(SOURCES)
            or not all(_hex(value) for value in row["source_sha256"].values())):
        _fail("report_invalid")
    actors = row["actors"]
    if type(actors) is not list or len(actors) != producers + 1:
        _fail("report_invalid")
    for actor in actors:
        _fields(actor, "actor_id incarnation role session_id")
    actor_ids = [a["actor_id"] for a in actors]
    if (len(set(actor_ids)) != producers + 1 or any(not _uuid(a["actor_id"]) or not _uuid(a["incarnation"]) for a in actors)
            or any(a["role"] != ("producer" if i < producers else "receiver") for i, a in enumerate(actors))):
        _fail("report_invalid")
    inventories = {}
    for name, state in (("accepted", "queued"), ("deliveries", "submitted"), ("observed", "consumed")):
        proofs = row[name]
        if type(proofs) is not list or len(proofs) != ACCEPTED:
            _fail("report_invalid")
        by_key = {}
        for proof in proofs:
            _fields(proof, "sender_actor_id message_id recipient_actor_id sequence payload_sha256 state")
            if (type(proof) is not dict or proof.get("state") != state
                    or not _uuid(proof.get("sender_actor_id")) or not _uuid(proof.get("message_id"))
                    or proof["sender_actor_id"] not in actor_ids[:-1]
                    or proof.get("recipient_actor_id") != actor_ids[-1]
                    or not _hex(proof.get("payload_sha256")) or _key(proof) in by_key
                    or _decimal(proof.get("sequence"), ACCEPTED) == 0):
                _fail("report_invalid")
            by_key[_key(proof)] = proof
        if ({_decimal(p["sequence"], ACCEPTED) for p in proofs} != set(range(1, ACCEPTED + 1))
                or len({p["message_id"] for p in proofs}) != ACCEPTED):
            _fail("report_invalid")
        inventories[name] = by_key
    accepted = inventories["accepted"]
    for name in ("deliveries", "observed"):
        observed = inventories[name]
        if set(observed) != set(accepted) or any(not _same_proof(p, observed[key]) for key, p in accepted.items()):
            _fail("report_invalid")
    ids = {proof["message_id"] for proof in accepted.values()}
    if row["capacity_refused_message_id"] not in ids:
        _fail("report_invalid")
    event_counts, event_sets = row["event_counts"], row["event_sets"]
    if set(event_counts) != set(EVENT_KINDS) or set(event_sets) != set(EVENT_KINDS):
        _fail("report_invalid")
    all_sequences, transitions = set(), {}
    for kind in EVENT_KINDS:
        events = event_sets[kind]
        if type(event_counts[kind]) is not int or event_counts[kind] != ACCEPTED or type(events) is not list or len(events) != ACCEPTED:
            _fail("report_invalid")
        if {e["entity_id"] for e in events} != ids:
            _fail("report_invalid")
        for event in events:
            _fields(event, "sequence kind entity_id")
            seq = _decimal(event["sequence"], 512)
            if event["kind"] != kind or seq == 0 or seq in all_sequences:
                _fail("report_invalid")
            all_sequences.add(seq)
            transitions.setdefault(event["entity_id"], []).append(seq)
    if any(not (values[0] < values[1] < values[2]) for values in transitions.values()):
        _fail("report_invalid")
    expected_counts = {"actors": producers + 1, "routes": producers, "messages": ACCEPTED,
                       "tasks": 0, "dispositions": 0, "migrations": 0,
                       "events": ACCEPTED * 3 + producers * 2 + 1}
    if (type(row["state_counts"]) is not dict or set(row["state_counts"]) != set(expected_counts)
            or any(type(row["state_counts"][k]) is not int or row["state_counts"][k] != v for k, v in expected_counts.items())):
        _fail("report_invalid")
    total_latencies = 0
    if type(row["latency_seconds"]) is not dict or not row["latency_seconds"]:
        _fail("report_invalid")
    for values in row["latency_seconds"].values():
        _fields(values, "count min max mean p50 p95")
        count = values["count"]
        if type(count) is not int or not 0 < count <= MAX_IPC_CALLS:
            _fail("report_invalid")
        total_latencies += count
        for key in ("min", "max", "mean", "p50", "p95"):
            value = values[key]
            if type(value) not in (int, float) or not math.isfinite(value) or not 0 <= value <= SOCKET_SECONDS:
                _fail("report_invalid")
        if not values["min"] <= values["p50"] <= values["p95"] <= values["max"] or not values["min"] <= values["mean"] <= values["max"]:
            _fail("report_invalid")
    if total_latencies != row["ipc_calls"]:
        _fail("report_invalid")
    files = row["retained_files"]
    if type(files) is not list:
        _fail("report_invalid")
    for item in files:
        _fields(item, "path device inode mode kind bytes")
    if (type(files) is not list or not 0 < len(files) <= MAX_FILES
            or any(type(item["bytes"]) is not int or item["bytes"] < 0 for item in files)
            or sum(item["bytes"] for item in files) != row["retained_file_bytes"]
            or len({item["path"] for item in files}) != len(files)):
        _fail("report_invalid")
    children = row["direct_children"]
    if type(children) is not list or len(children) != row["cli_calls"] + 1:
        _fail("report_invalid")
    for child in children:
        _fields(child, "operation identity reaped exit_code")
        identity = child["identity"]
        _fields(identity, "pid ppid pgrp session start_ticks")
        if (child["reaped"] is not True or type(child["exit_code"]) is not int
                or child["operation"] not in ("serve", "status", "--version", "coord")
                or child["exit_code"] not in ((0, 1) if child["operation"] == "status" else (0,))
                or type(identity) is not dict or type(identity["pid"]) is not int or identity["pid"] <= 0
                or type(identity["ppid"]) is not int or identity["ppid"] <= 0
                or _decimal(identity["start_ticks"], (1 << 63) - 1) == 0):
            _fail("report_invalid")
    serve = [child for child in children if child["operation"] == "serve"]
    if len(serve) != 1 or serve[0]["identity"] != row["child_identity"] or serve[0]["exit_code"] != 0:
        _fail("report_invalid")
    _fields(row["host"], "system release machine python")
    if any(type(value) is not str or not 0 < len(value.encode("utf-8")) <= 256 for value in row["host"].values()):
        _fail("report_invalid")
    expected_bounds = {"request_frame_bytes": REQUEST_BYTES, "response_frame_bytes": RESPONSE_BYTES,
                       "state_page_bytes": PAGE_BYTES, "receive_batch": RECEIVE_LIMIT,
                       "pending_per_recipient": CAPACITY, "workers": producers,
                       "max_inflight_futures": producers, "socket_seconds": SOCKET_SECONDS,
                       "cli_seconds": CLI_SECONDS, "startup_seconds": STARTUP_SECONDS,
                       "case_seconds": CASE_SECONDS, "max_ipc_calls": MAX_IPC_CALLS,
                       "max_cli_calls": MAX_CLI_CALLS, "max_pages_per_section": MAX_PAGES}
    if (type(row["bounds"]) is not dict or row["bounds"] != expected_bounds
            or any(type(value) is not int for value in row["bounds"].values())):
        _fail("report_invalid")
    # Also reject nonfinite or secret-bearing extra fields, rather than silently ignoring them.
    serialized = _json_bytes(row)
    if len(serialized) > 2 * 1024 * 1024:
        _fail("report_invalid")
    def inspect(value):
        if type(value) is dict:
            if set(value) & {"credential", "auth", "token", "payload", "text", "stdout", "stderr"}:
                _fail("report_invalid")
            for child in value.values():
                inspect(child)
        elif type(value) is list:
            for child in value:
                inspect(child)
    inspect(row)


def run_case(out, producers, binary=BINARY):
    """Run one real 128-slot pressure generation and retain a sanitized proof."""
    if type(producers) is not int or producers not in (1, 2, 4):
        _fail("invalid_producer_count")
    out = Path(out)
    _check_output(out, empty=True)
    binary = Path(binary)
    if not binary.is_absolute() or not binary.is_file() or not os.access(binary, os.X_OK):
        _fail("binary_refused")
    run = None
    row = None
    failure = None
    try:
        binary_sha = _sha(binary)
        sources = {name: _sha(ROOT / name) for name in SOURCES}
        run = _Run(out, binary)
        row = _exercise(run, producers)
        if binary_sha != _sha(binary) or sources != {name: _sha(ROOT / name) for name in SOURCES}:
            _fail("source_changed")
        files = [item for item in _scan(out) if item["kind"] == "regular_file"]
        row.update({"production_binary_sha256": binary_sha, "source_sha256": sources,
                    "host": {"system": platform.system(), "release": platform.release(),
                             "machine": platform.machine(), "python": platform.python_version()},
                    "retained_files": files, "retained_file_bytes": sum(item["bytes"] for item in files),
                    "retained_file_bytes_scope": "regular_file_lengths_after_child_exit_before_result_and_ownership_not_quota_or_peak",
                    "wall_seconds": time.monotonic() - run.started})
        validate_case(row)
    except BaseException as error:
        if isinstance(error, LoadFailure):
            failure = str(error) if str(error) in ERRORS else "internal_failure"
        elif isinstance(error, (KeyboardInterrupt, SystemExit)):
            failure = "interrupted"
        elif isinstance(error, OSError):
            failure = "local_io_failure"
        else:
            failure = "internal_failure"
    finally:
        if run is not None:
            try:
                run.finish()
            except (ValueError, OSError):
                failure = "child_reap_failed"
        if failure is not None:
            row = {"schema_version": 1, "status": "failed", "scope": SCOPE, "producers": producers,
                   "category": failure, "outcome_unknown": True, "automatic_retry": False,
                   "direct_children": run.children if run is not None else []}
        if row is not None:
            _check_output(out)
            _write_new(out / "result.json", row)
            _inventory(out, run.children if run is not None else [])
    if failure is not None:
        _fail(failure)
    return row


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new absolute private TMPDIR descendant; never overwritten")
    args = parser.parse_args(argv)
    out = None
    try:
        out = prepare_output(args.out if args.out is not None else _tmp_root() / ("ht-load-" + uuid.uuid4().hex))
        rows = [run_case(prepare_output(out / f"producers-{count}"), count) for count in (1, 2, 4)]
        result = {"schema_version": 1, "status": "passed", "scope": SCOPE,
                  "case_count": len(rows), "inference": False, "live_actors": False,
                  "cases": [{"producers": row["producers"], "status": row["status"],
                             "result": str(out / f"producers-{row['producers']}" / "result.json")} for row in rows]}
        _write_new(out / "matrix.json", result)
        _inventory(out, [child for row in rows for child in row["direct_children"]])
        print(json.dumps({"status": "passed", "scope": SCOPE, "out": str(out), "case_count": len(rows)}))
        return 0
    except (ValueError, OSError):
        # No arbitrary exception, peer payload, credential, or subprocess output is printed.
        if out is not None and not (out / "ownership.json").exists():
            try:
                _inventory(out, [])
            except (ValueError, OSError):
                pass
        print(json.dumps({"status": "failed", "scope": SCOPE, "category": "load_acceptance_failed",
                          "automatic_retry": False}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
