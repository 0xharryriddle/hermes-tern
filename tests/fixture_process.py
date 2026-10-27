"""Bounded fixture readiness and closed failure diagnostics; never retry mutations."""
import json
import math
import os
from pathlib import Path
import subprocess
import time

STARTUP_TIMEOUT = 15

_CLI_ERRORS = {
    b"local I/O error (operation interrupted)": "io_interrupted",
    b"local I/O error (operation would block)": "io_would_block",
    b"local I/O error (timed out)": "io_timed_out",
    b"local I/O error (connection reset)": "io_connection_reset",
    b"local I/O error (broken pipe)": "io_broken_pipe",
    b"local I/O error (connection refused)": "io_connection_refused",
    b"local I/O error (entity not found)": "io_not_found",
    b"protocol error: partial IPC frame at EOF": "ipc_partial_eof",
    b"protocol error: IPC byte limit": "ipc_byte_limit",
    b"protocol error: one request per connection": "ipc_frame_trailing_bytes",
    b"protocol error: IPC response identity mismatch": "ipc_identity_mismatch",
    b"IPC frame timed out; submitted work may have an unknown outcome": "ipc_deadline",
    b"storage error: supervisor refused request; check identity, route, bounds and lifecycle": "ipc_refused",
}
_CLI_OPERATIONS = frozenset({"register", "rotate", "route", "status", "actors", "messages",
    "snapshot", "events", "inspect-message", "inspect-task", "state-index", "state-page",
    "archive", "inspect-archive", "recover-archive", "shutdown"})


def cli_failure_diagnostic(result, operation):
    """Return closed metadata only, never raw invocation, output, paths or secrets."""
    stderr = result.stderr if type(result.stderr) is bytes else b""
    category = "diagnostic_unavailable"
    if len(stderr) <= 1024:
        for text, name in _CLI_ERRORS.items():
            if stderr == b"hermes-tern: " + text + b"\n":
                category = name
                break
    return {"schema_version": 1, "operation": operation if operation in _CLI_OPERATIONS else "unclassified",
            "category": category, "exit_code": result.returncode,
            "stdout_bytes": min(len(result.stdout or b""), 65536), "stderr_bytes": min(len(result.stderr or b""), 65536),
            "outcome_unknown": True, "automatic_retry": False}


def record_cli_failure(root, result, operation):
    """Retain at most 32 closed receipts in the owned fixture, never replace a file."""
    body = (json.dumps(cli_failure_diagnostic(result, operation), sort_keys=True) + "\n").encode()
    for index in range(1, 33):
        path = Path(root) / f"ipc-failure-{index:02}.json"
        try:
            fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        except FileExistsError:
            continue
        with os.fdopen(fd, "wb") as stream:
            stream.write(body)
        return path
    return None


def wait_ready(child, probe, timeout=STARTUP_TIMEOUT):
    """Probe a read-only condition with its remaining startup budget.

    The probe must honor its supplied budget. Do not pass mutation commands or
    error-swallowing callbacks. Both process exit and a late success refuse.
    """
    if isinstance(timeout, bool) or not math.isfinite(timeout) or not 0 < timeout <= STARTUP_TIMEOUT:
        raise ValueError("invalid fixture startup allowance")
    started = time.monotonic()
    deadline = started + timeout
    while True:
        if child.poll() is not None:
            raise AssertionError("fixture exited before readiness")
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise AssertionError("fixture startup deadline")
        ready = probe(remaining)
        if child.poll() is not None:
            raise AssertionError("fixture exited before readiness")
        if time.monotonic() >= deadline:
            raise AssertionError("fixture startup deadline")
        if ready:
            return time.monotonic() - started
        time.sleep(min(0.02, max(0, deadline - time.monotonic())))


def wait_supervisor(child, binary, state, timeout=STARTUP_TIMEOUT, probe_timeout=6):
    """Only the fixed read-only status command is retried, never mutations."""
    if isinstance(probe_timeout, bool) or not math.isfinite(probe_timeout) or not 0 < probe_timeout <= 6:
        raise ValueError("invalid fixture probe allowance")
    def status(remaining):
        deadline = time.monotonic() + min(probe_timeout, remaining)
        probe = subprocess.Popen([str(binary), "coord", "--state", str(state), "status"],
                                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                 stderr=subprocess.DEVNULL)
        try:
            while True:
                if child.poll() is not None:
                    raise AssertionError("fixture exited before readiness")
                if time.monotonic() >= deadline:
                    return False
                code = probe.poll()
                if code is not None:
                    return code == 0
                time.sleep(min(0.02, max(0, deadline - time.monotonic())))
        finally:
            # Reap this exact read-only probe before another attempt. Never
            # signal the supervisor or add retries to operation helpers.
            if probe.poll() is None:
                probe.kill()
            probe.wait(timeout=3)
    return wait_ready(child, status, timeout=timeout)
