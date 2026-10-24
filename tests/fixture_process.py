"""Bounded test startup readiness only; never retry an admitted operation."""
import math
import subprocess
import time

STARTUP_TIMEOUT = 15


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
