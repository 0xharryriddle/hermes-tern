"""Production CLI + real PTY/SDK + bounded loopback WS fixture; no Hermes/provider.

This speaks only the source-qualified protocol subset used by these tests. It is
not a fake renderer, live gateway capture or Tern pixel/window acceptance.
"""
import base64
import copy
import errno
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import pty
import select
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"
BOUND = 2 * 1024 * 1024
_spec = importlib.util.spec_from_file_location("native_fixture_process", Path(__file__).with_name("fixture_process.py"))
assert _spec is not None and _spec.loader is not None
_process = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_process)


def snapshot(running=False):
    return {"session_id": "fixture-session", "subscription_id": "fixture-sub", "authority_epoch": 1,
            "revision": 1, "execution_generation": 2, "replay_epoch": "fixture-epoch", "last_sequence": 0,
            "running": running, "messages": [], "pending": [], "prompts": [],
            "info": {"yolo": False, "model": "fixture-model", "provider": "no-inference"}}


class Gateway:
    def __init__(self, running=False, disconnect_on_submit=False, accept_timeout=3, read_timeout=4, autostart=True, history=None):
        if not 0 < accept_timeout <= 60 or not 0 < read_timeout <= 60:
            raise ValueError("fixture deadline must be bounded within 60 seconds")
        self.accept_timeout = accept_timeout
        self.read_timeout = read_timeout
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.listener.listen(1)
        self.listener.settimeout(0.05)
        self.port = self.listener.getsockname()[1]
        self.running = running
        self.history = history
        self.disconnect_on_submit = disconnect_on_submit
        self.stop = threading.Event()
        self.lock = threading.Lock()
        self.send_lock = threading.Lock()
        self.requests = []
        self.errors = []
        self.socket = None
        self.seq = 0
        self.thread = threading.Thread(target=self.serve)
        if autostart:
            self.thread.start()

    def exact(self, count):
        assert self.socket is not None
        data = bytearray()
        deadline = time.monotonic() + self.read_timeout
        while len(data) < count:
            if self.stop.is_set():
                raise EOFError
            if time.monotonic() > deadline:
                raise TimeoutError("fixture WS read deadline")
            try:
                chunk = self.socket.recv(count - len(data))
            except socket.timeout:
                continue
            if not chunk:
                raise EOFError
            data.extend(chunk)
        return bytes(data)

    def send(self, value):
        assert self.socket is not None
        data = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode()
        assert len(data) <= BOUND
        length = bytes([len(data)]) if len(data) < 126 else b"\x7e" + struct.pack("!H", len(data))
        with self.send_lock:
            self.socket.sendall(b"\x81" + length + data)

    def reply(self, request, result):
        self.send({"jsonrpc": "2.0", "id": request["id"], "result": result})

    def event(self, kind, payload, generation=2):
        self.seq += 1
        self.send({"jsonrpc": "2.0", "method": "event", "params": {
            "type": kind, "session_id": "fixture-session", "replay_epoch": "fixture-epoch", "seq": self.seq,
            "execution_generation": generation, "authority_epoch": 1, "payload": payload}})

    def methods(self, method):
        with self.lock:
            return copy.deepcopy([r for r in self.requests if r.get("method") == method])

    def serve(self):
        try:
            deadline = time.monotonic() + self.accept_timeout
            while self.socket is None:
                if self.stop.is_set():
                    return
                if time.monotonic() >= deadline:
                    raise TimeoutError("fixture WS accept deadline")
                try:
                    self.socket, _ = self.listener.accept()
                except socket.timeout:
                    continue
            self.socket.settimeout(0.05)
            header = bytearray()
            while not header.endswith(b"\r\n\r\n"):
                header.extend(self.exact(1))
                assert len(header) <= 16384
            lines = header.decode("ascii").split("\r\n")
            assert lines[0] == "GET /api/ws HTTP/1.1"
            fields = dict(line.split(":", 1) for line in lines[1:] if ":" in line)
            fields = {k.lower(): v.strip() for k, v in fields.items()}
            assert fields["sec-websocket-protocol"].split(", ")[0] == "hermes-gateway-v1"
            key = fields["sec-websocket-key"]
            digest = base64.b64encode(hashlib.sha1((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").encode()).digest())
            self.socket.sendall(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: " + digest + b"\r\nSec-WebSocket-Protocol: hermes-gateway-v1\r\n\r\n")
            while not self.stop.is_set():
                first, second = self.exact(2)
                assert first & 0x80 and second & 0x80, "fixture expects final masked client frame"
                size = second & 127
                if size == 126:
                    size = struct.unpack("!H", self.exact(2))[0]
                elif size == 127:
                    size = struct.unpack("!Q", self.exact(8))[0]
                assert size <= BOUND
                mask = self.exact(4)
                raw = self.exact(size)
                raw = bytes(b ^ mask[i % 4] for i, b in enumerate(raw))
                if first & 15 == 8:
                    break
                assert first & 15 == 1
                request = json.loads(raw)
                with self.lock:
                    self.requests.append(request)
                    assert len(self.requests) <= 256
                method = request.get("method")
                if method == "ping":
                    self.reply(request, {"pong": True})
                elif method == "runtime.describe":
                    self.reply(request, {"profile_id": "fixture-profile", "instance_id": "fixture-instance",
                        "capabilities": ["durable-admission-v1", "event-replay-v1", "local-cli-create-v1"],
                        "session_create": {"sources": ["tui"], "parameters": ["request_id", "source", "cwd", "yolo", "model", "provider"]}})
                elif method == "client.capabilities":
                    assert request["params"]["server_requests"] is True
                    self.reply(request, {})
                elif method == "session.create":
                    assert request["params"]["yolo"] is False
                    assert request["params"]["source"] == "tui"
                    self.reply(request, snapshot(self.running))
                elif method == "session.history" and self.history is not None:
                    assert request["params"] == {"session_id": "fixture-session"}
                    self.reply(request, self.history)
                elif method == "prompt.submit":
                    if self.disconnect_on_submit:
                        break
                    self.reply(request, {"admission_id": "fixture-admission", "ref": {"profile_id": "fixture-profile", "session_id": "fixture-session"},
                        "sequence": 1, "authority_epoch": 1, "execution_generation": 2, "status": "terminal", "outcome": "completed",
                        "result": {"final_response": "fixture-complete"}})
                elif method in ("approval.respond", "clarify.respond", "session.interrupt"):
                    self.reply(request, {})
                elif method == "session.detach":
                    self.reply(request, {"session_id": "fixture-session", "subscription_id": "fixture-sub", "detached": True})
                else:
                    raise AssertionError("unexpected fixture method: " + str(method))
        except EOFError:
            pass
        except BaseException as error:
            if not self.stop.is_set():
                self.errors.append(type(error).__name__ + ": " + str(error))
        finally:
            if self.socket is not None:
                self.socket.close()
            self.listener.close()

    def close(self):
        self.stop.set()
        if self.socket is not None:
            try:
                self.socket.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
        if self.thread.ident is not None:
            self.thread.join(timeout=5)
        else:
            self.listener.close()
        assert not self.thread.is_alive(), "owned gateway thread remains"


class NativeChat:
    def __init__(self, running=False, ack=True, disconnect_on_submit=False, gateway=None, command=None):
        self.root = Path(tempfile.mkdtemp(prefix="ht-chat-", dir=os.environ["TMPDIR"]))
        self.gateway = gateway if gateway is not None else Gateway(running, disconnect_on_submit, accept_timeout=_process.STARTUP_TIMEOUT, autostart=False)
        self.master, self.slave = pty.openpty()
        self.original_termios = termios.tcgetattr(self.slave)
        self.auto_ack = ack
        self.pending_acks = []
        self.buffer = bytearray()
        self.raw = bytearray()
        self.packets = []
        self.nodes = {}
        self.surface = None
        env = {k: v for k, v in os.environ.items() if k in ("PATH", "LANG", "LC_ALL", "TERM")}
        for key in ("HOME", "HERMES_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME", "XDG_RUNTIME_DIR", "TMPDIR"):
            path = self.root / key.lower()
            path.mkdir(mode=0o700)
            env[key] = str(path)
        env["TERM"] = "xterm-256color"
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        runtime = self.root / "runtime"
        bootstrap = runtime / "ui-tui/scripts/gateway_bootstrap.py"
        bootstrap.parent.mkdir(parents=True, mode=0o700)
        grant = {"url": f"ws://127.0.0.1:{self.gateway.port}/api/ws",
                 "protocols": ["hermes-gateway-v1", "hermes-gateway-ticket.synthetic-fixture"],
                 "profile_id": "fixture-profile", "instance_id": "fixture-instance"}
        bootstrap.write_text("def bootstrap(start,recover):\n    assert (start,recover)==(False,False)\n    return " + repr(grant) + "\n")
        self.stderr_file = (self.root / "client.stderr").open("wb")
        argv = command if command is not None else ["chat", "--cwd", str(self.root)]
        self.proc = subprocess.Popen([str(BINARY), *argv, "--hermes-root", str(runtime), "--python", sys.executable,
            "--home", env["HERMES_HOME"]], stdin=self.slave, stdout=self.slave,
            stderr=self.stderr_file, env=env, cwd=ROOT, start_new_session=True)
        self.pid = self.proc.pid
        self.start_ticks = (Path("/proc") / str(self.pid) / "stat").read_text().rsplit(")", 1)[1].split()[19]
        # Setup/fsync work must not spend the accept budget before launch.
        self.gateway.thread.start()

    def send(self, raw):
        deadline = time.monotonic() + 3
        while raw:
            assert time.monotonic() < deadline, "PTY input deadline"
            _, writable, _ = select.select([], [self.master], [], 0.05)
            if writable:
                raw = raw[os.write(self.master, raw):]

    def event(self, ev, **fields):
        self.send(b"\x1b_tsp;e;" + json.dumps({"ev": ev, **fields}, ensure_ascii=False).encode() + b"\x1b\\")

    def click(self, node):
        self.event("action", sf=self.surface, id=node, act="click", mods=[])

    def install(self, node):
        self.nodes[node["id"]] = {"kind": node["k"], **node.get("p", {})}
        for child in node.get("c", []):
            self.install(child)

    def packet(self, verb, body):
        self.packets.append([verb, body])
        assert len(self.packets) <= 2048
        if verb == "q" and body.get("q") == "hello":
            hello = {"r": "hello", "v": 1, "term": "tern", "ver": "0.9.5", "apc": 1048576,
                     "kinds": ["col", "text", "el", "editor"], "features": ["send", "dock", "styles"], "credits": 1, "cols": 120}
            self.send(b"\x1b_tsp;r;" + json.dumps(hello).encode() + b"\x1b\\")
        elif verb == "o":
            self.surface = body["id"]
        elif verb == "f":
            for op in body["ops"]:
                if op[0] == "add":
                    self.install(op[4])
                elif op[0] == "set":
                    self.nodes[op[1]].update(op[2])
                elif op[0] == "text":
                    before = self.nodes[op[1]].get("text", "") if op[2] == "append" else ""
                    self.nodes[op[1]]["text"] = before + op[3]
                elif op[0] == "del":
                    self.nodes = {k: v for k, v in self.nodes.items() if k != op[1] and not k.startswith(op[1] + ".")}
                else:
                    assert op[0] in ("focus", "move", "settle", "reveal", "scroll"), op[0]
            ack = {"sf": body["sf"], "s": body["s"]}
            if self.auto_ack:
                self.event("ack", **ack)
            else:
                self.pending_acks.append(ack)

    def pump(self, seconds=0.02):
        if not select.select([self.master], [], [], seconds)[0]:
            return
        try:
            raw = os.read(self.master, 65536)
        except OSError as error:
            if error.errno == errno.EIO:
                return
            raise
        self.raw.extend(raw)
        self.buffer.extend(raw)
        assert len(self.raw) <= BOUND and len(self.buffer) <= BOUND
        while True:
            start = self.buffer.find(b"\x1b_tsp;")
            if start < 0:
                self.buffer[:] = self.buffer[-7:]
                return
            end = self.buffer.find(b"\x1b\\", start)
            if end < 0:
                return
            packet = bytes(self.buffer[start + 6:end])
            del self.buffer[:end + 2]
            verb, body = packet.split(b";", 1)
            self.packet(verb.decode(), json.loads(body))

    def until(self, predicate, label, timeout=3):
        end = time.monotonic() + timeout
        while not predicate():
            assert not self.gateway.errors, self.gateway.errors
            assert time.monotonic() < end, label + " deadline; exit=" + str(self.proc.poll())
            self.pump()

    def quiet(self, seconds=0.12):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.pump(0.01)
        assert not self.gateway.errors, self.gateway.errors

    def ready(self):
        def editor(remaining):
            assert not self.gateway.errors, self.gateway.errors
            self.pump(min(0.02, remaining))
            return "dock.ed" in self.nodes
        self.startup_seconds = _process.wait_ready(self.proc, editor)

    def wait_exit(self):
        self.until(lambda: self.proc.poll() is not None, "native exit")
        self.pump(0)
        return self.proc.wait(timeout=1)

    def close(self):
        forced = False
        try:
            if self.proc.poll() is None:
                try:
                    self.send(b"\x04")
                    self.wait_exit()
                except (AssertionError, OSError):
                    forced = True
                    self.proc.kill()
                    self.proc.wait(timeout=3)
        finally:
            self.proc.wait(timeout=3)
            self.stderr_file.close()
            self.gateway.close()
            os.close(self.master)
            os.close(self.slave)
            (self.root / "protocol.json").write_text(json.dumps({"scope": "synthetic_PTY_WS_not_live_or_pixels", "packets": self.packets,
                "requests": self.gateway.requests, "gateway_errors": self.gateway.errors, "pid": self.pid,
                "start_ticks": self.start_ticks, "exit_code": self.proc.returncode,
                "startup_seconds": getattr(self, "startup_seconds", None),
                "forced_cleanup": forced}, ensure_ascii=False, indent=2) + "\n")
            owned = self.root / "ownership.json"
            owned.touch(mode=0o600)
            paths = [{"path": str(p), "dev": p.lstat().st_dev, "ino": p.lstat().st_ino} for p in [self.root, *self.root.rglob("*")]]
            owned.write_text(json.dumps({"retention": "retain;no deletion", "paths": paths}, indent=2) + "\n")
        assert not forced, "native fixture required forced cleanup; retained " + str(self.root)


class NativeChatTests(unittest.TestCase):
    def fixture(self, **kwargs):
        f = NativeChat(**kwargs)
        self.addCleanup(f.close)
        f.ready()
        return f

    def assert_detached_cleanly(self, f):
        self.assertEqual(f.wait_exit(), 0, (f.root / "client.stderr").read_text())
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertFalse(f.gateway.methods("session.interrupt"))
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_surface_gone_detaches_cleanly_instead_of_rendering_a_closed_surface(self):
        f = self.fixture()
        f.event("gone", sf=f.surface, ids=[f.surface])
        self.assert_detached_cleanly(f)
        self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_terminal_rejection_fails_closed_and_redacts_untrusted_message(self):
        f = self.fixture()
        f.event("error", sf=f.surface, s=1, op=0, msg="UNTRUSTED-TERMINAL-DETAIL\u001b[31m")
        self.assertEqual(f.wait_exit(), 1)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertFalse(f.gateway.methods("session.interrupt"))
        self.assertNotIn("UNTRUSTED-TERMINAL-DETAIL", (f.root / "client.stderr").read_text())
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_terminal_rejection_survives_surface_loss_in_the_same_input_chunk(self):
        for scope in ("current", "global"):
            for prefix in (False, True):
                with self.subTest(scope=scope, prefix=prefix):
                    f = self.fixture()
                    error = {"ev": "error", "msg": "UNTRUSTED-BATCH-ERROR\u001b[31m", "s": 1, "op": 0}
                    if scope == "current":
                        error["sf"] = f.surface
                    events = ([{"ev": "theme", "dark": True}] if prefix else []) + [
                        error, {"ev": "gone", "sf": f.surface, "ids": [f.surface]}]
                    payload = b"".join(b"\x1b_tsp;e;" + json.dumps(e).encode() + b"\x1b\\" for e in events)
                    self.assertLess(len(payload), 8192)
                    f.send(payload)
                    self.assertEqual(f.wait_exit(), 1, "Gone must not downgrade terminal rejection to success")
                    self.assertEqual(len(f.gateway.methods("session.detach")), 1)
                    self.assertFalse(f.gateway.methods("prompt.submit"))
                    self.assertFalse(f.gateway.methods("session.interrupt"))
                    diagnostic = (f.root / "client.stderr").read_text()
                    self.assertIn("native terminal rejected output", diagnostic)
                    self.assertNotIn("UNTRUSTED-BATCH-ERROR", diagnostic)
                    self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_unrelated_rejection_with_surface_loss_still_detaches_cleanly(self):
        f = self.fixture()
        events = [{"ev": "theme", "dark": True}, {"ev": "error", "sf": "other", "msg": "unrelated"},
                  {"ev": "gone", "sf": f.surface, "ids": [f.surface]}]
        f.send(b"".join(b"\x1b_tsp;e;" + json.dumps(e).encode() + b"\x1b\\" for e in events))
        self.assert_detached_cleanly(f)
        self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_queued_send_stays_fenced_when_gone_and_error_are_composed(self):
        for order in (("send", "theme", "error", "gone"), ("send", "theme", "gone", "error"),
                      ("gone", "send", "error")):
            with self.subTest(order=order):
                f = self.fixture()
                events = {
                    "send": {"ev": "send", "sf": f.surface, "id": "dock.ed", "text": "must-not-submit"},
                    "theme": {"ev": "theme", "dark": True},
                    "error": {"ev": "error", "sf": f.surface, "msg": "UNTRUSTED-ORDER"},
                    "gone": {"ev": "gone", "sf": f.surface, "ids": [f.surface]},
                }
                f.send(b"".join(b"\x1b_tsp;e;" + json.dumps(events[k]).encode() + b"\x1b\\" for k in order))
                self.assertEqual(f.wait_exit(), 1)
                self.assertEqual(len(f.gateway.methods("session.detach")), 1)
                self.assertFalse(f.gateway.methods("prompt.submit"))
                self.assertFalse(f.gateway.methods("session.interrupt"))
                diagnostic = (f.root / "client.stderr").read_text()
                self.assertIn("native terminal rejected output", diagnostic)
                self.assertNotIn("UNTRUSTED-ORDER", diagnostic)
                self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_keyboard_and_send_button_submit_same_unicode_draft_once(self):
        for action in ("keyboard", "button"):
            with self.subTest(action=action):
                f = self.fixture()
                text = "tiếng Việt 😀"
                f.send(b"\x1b[200~" + text.encode() + b"\x1b[201~")
                f.until(lambda: f.nodes["dock.ed"].get("text") == text, "Unicode draft")
                if action == "keyboard":
                    f.send(b"\r")
                else:
                    f.click("dock.controls.send")
                f.until(lambda: len(f.gateway.methods("prompt.submit")) == 1, "one submission")
                f.until(lambda: f.nodes["dock.ed"].get("text") == "" and f.nodes["dock.ed"].get("sendable") is True, "completed and cleared")
                self.assertEqual(f.gateway.methods("prompt.submit")[0]["params"]["text"], text)
                f.send(b"\r")
                f.quiet()
                self.assertEqual(len(f.gateway.methods("prompt.submit")), 1)
                f.send(b"\x04")
                self.assert_detached_cleanly(f)

    def test_no_ack_does_not_block_exit_and_real_tty_modes_are_restored(self):
        f = self.fixture(ack=False)
        self.assertNotEqual(termios.tcgetattr(f.slave), f.original_termios)
        f.send(b"\x1b[200~retained draft\x1b[201~")
        f.quiet()
        self.assertIsNone(f.proc.poll())
        self.assertTrue(f.pending_acks)
        f.send(b"\x04")
        self.assert_detached_cleanly(f)
        self.assertFalse(f.gateway.methods("prompt.submit"))


    def test_unrelated_node_and_surface_events_do_not_exit_or_dispatch(self):
        f = self.fixture()
        for event in [
            {"ev": "gone", "sf": "other", "ids": ["other"]},
            {"ev": "gone", "sf": f.surface, "ids": ["missing-node"]},
            {"ev": "error", "sf": "other", "msg": "unrelated rejection"},
            {"ev": "theme", "dark": True},
            {"ev": "motion", "reduce": True},
            {"ev": "resize", "cols": 60},
        ]:
            ev = event.pop("ev")
            f.event(ev, **event)
            f.quiet(0.04)
        self.assertIsNone(f.proc.poll())
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assert_detached_cleanly(f)

    def test_closed_surface_drops_actions_already_decoded_in_same_input_chunk(self):
        f = self.fixture()
        events = [{"ev": "send", "sf": f.surface, "id": "dock.ed", "text": "must-not-submit"},
                  {"ev": "gone", "sf": f.surface, "ids": [f.surface]}]
        f.send(b"".join(b"\x1b_tsp;e;" + json.dumps(e).encode() + b"\x1b\\" for e in events))
        self.assert_detached_cleanly(f)
        self.assertFalse(f.gateway.methods("prompt.submit"))

    def test_native_send_and_busy_rejection_keep_independent_local_draft(self):
        for running in (False, True):
            with self.subTest(running=running):
                f = self.fixture(running=running)
                f.send(b"\x1b[200~keep local draft\x1b[201~")
                f.until(lambda: f.nodes["dock.ed"].get("text") == "keep local draft", "local draft")
                f.event("send", sf=f.surface, id="dock.ed", text="native Unicode 😀")
                if running:
                    f.until(lambda: "not ready" in f.nodes["dock.notice"].get("text", ""), "busy refusal")
                    self.assertFalse(f.gateway.methods("prompt.submit"))
                else:
                    f.until(lambda: len(f.gateway.methods("prompt.submit")) == 1, "native submission")
                    self.assertEqual(f.gateway.methods("prompt.submit")[0]["params"]["text"], "native Unicode 😀")
                self.assertEqual(f.nodes["dock.ed"]["text"], "keep local draft")
                f.send(b"\x04")
                self.assert_detached_cleanly(f)

    def test_interrupt_uses_current_generation_without_submitting_draft(self):
        f = self.fixture(running=True)
        f.send(b"\x1b[200~never send this draft\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "never send this draft", "interrupt draft")
        f.send(b"\x03")
        f.until(lambda: len(f.gateway.methods("session.interrupt")) == 1, "interrupt")
        request = f.gateway.methods("session.interrupt")[0]
        self.assertEqual(request["params"], {"session_id": "fixture-session", "execution_generation": 2})
        self.assertFalse(f.gateway.methods("prompt.submit"))
        self.assertEqual(f.nodes["dock.ed"]["text"], "never send this draft")
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(len(f.gateway.methods("session.interrupt")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_delayed_ack_coalesces_draft_and_does_not_block_gateway_progress(self):
        f = self.fixture(ack=False)
        f.send(b"\x1b[200~draft after blocked frame\x1b[201~")
        f.gateway.event("message.delta", {"text": "gateway progressed without keyboard"})
        f.quiet()
        self.assertEqual(len([p for p in f.packets if p[0] == "f"]), 1)
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: f.nodes["dock.ed"].get("text") == "draft after blocked frame", "coalesced draft")
        f.until(lambda: any("gateway progressed" in n.get("text", "") for n in f.nodes.values()), "network-only repaint")
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assert_detached_cleanly(f)

    def test_old_approval_node_cannot_be_remapped_to_a_new_generation(self):
        f = self.fixture(running=True)
        def prompt(generation, command):
            f.gateway.event("approval.request", {"kind": "approval", "prompt_id": "same-id",
                "execution_generation": generation, "choices": ["once", "deny"], "command": command}, generation)
        def buttons():
            return [node for node, props in f.nodes.items() if props.get("actions", {}).get("click") and "prompt-" in node]
        prompt(2, "first-operation")
        f.until(lambda: len(buttons()) == 2, "first approval")
        old = buttons()[0]
        f.auto_ack = False
        # Occupy the single frame credit, then change prompt authority. The newer
        # handler map must not reinterpret a click on the old rendered node.
        f.send(b"x")
        f.until(lambda: bool(f.pending_acks), "credit occupied")
        prompt(3, "second-operation")
        f.quiet(0.12)
        f.click(old)
        f.quiet(0.12)
        self.assertFalse(f.gateway.methods("approval.respond"), "old approval click authorized new generation")
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: len(buttons()) == 2 and old not in buttons(), "new generation controls")
        current = buttons()[0]
        f.click(current)
        f.until(lambda: len(f.gateway.methods("approval.respond")) == 1, "current explicit choice")
        request = f.gateway.methods("approval.respond")[0]
        self.assertEqual(request["params"]["prompt_id"], "same-id")
        self.assertEqual(request["params"]["execution_generation"], 3)
        f.click(current)
        f.quiet()
        self.assertEqual(len(f.gateway.methods("approval.respond")), 1)
        f.send(b"\x04")
        self.assert_detached_cleanly(f)


    def test_old_choice_cannot_authorize_changed_prompt_at_same_generation(self):
        f = self.fixture(running=True)
        def prompt(command, choices):
            f.gateway.event("approval.request", {"kind": "approval", "prompt_id": "same-id",
                "execution_generation": 2, "choices": choices, "command": command}, 2)
        def buttons():
            return [node for node, props in f.nodes.items() if props.get("actions", {}).get("click") and "prompt-" in node]
        prompt("original-operation", ["deny", "once"])
        f.until(lambda: len(buttons()) == 2, "original choices")
        old_deny = next(node for node in buttons() if node.endswith("choice0"))
        f.auto_ack = False
        f.send(b"x")
        f.until(lambda: bool(f.pending_acks), "credit occupied")
        prompt("replacement-operation", ["once", "deny"])
        f.quiet(0.12)
        f.click(old_deny)
        f.quiet(0.12)
        self.assertFalse(f.gateway.methods("approval.respond"), "old deny control changed meaning")
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: len(buttons()) == 2 and old_deny not in buttons(), "rebound choices")
        f.click(next(node for node in buttons() if node.endswith("choice1")))
        f.until(lambda: len(f.gateway.methods("approval.respond")) == 1, "current deny")
        self.assertEqual(f.gateway.methods("approval.respond")[0]["params"]["choice"], "deny")
        f.send(b"\x04")
        self.assert_detached_cleanly(f)


    def test_transport_loss_after_submit_never_resends_and_preserves_new_draft(self):
        f = self.fixture(disconnect_on_submit=True)
        f.event("send", sf=f.surface, id="dock.ed", text="one possibly accepted request")
        f.until(lambda: len(f.gateway.methods("prompt.submit")) == 1, "submission before loss")
        f.until(lambda: "disconnected" in f.nodes["main.state"].get("text", ""), "visible disconnected state")
        f.send(b"\x1b[200~keep after loss\x1b[201~")
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep after loss", "disconnected draft")
        f.send(b"\r")
        f.event("send", sf=f.surface, id="dock.ed", text="must refuse")
        f.quiet()
        self.assertEqual(len(f.gateway.methods("prompt.submit")), 1)
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep after loss")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)

    def test_expired_approval_never_dispatches_from_still_visible_control(self):
        f = self.fixture(running=True)
        f.gateway.event("approval.request", {"kind": "approval", "prompt_id": "expires",
            "execution_generation": 2, "choices": ["once", "deny"], "command": "expiry fixture", "timeout_s": 1})
        def buttons():
            return [node for node, props in f.nodes.items() if props.get("actions", {}).get("click") and "prompt-" in node]
        f.until(lambda: len(buttons()) == 2, "live expiring prompt")
        old = buttons()[0]
        f.quiet(1.1)
        f.click(old)
        f.until(lambda: "prompt changed" in f.nodes["dock.notice"].get("text", ""), "expired refusal")
        self.assertFalse(f.gateway.methods("approval.respond"))
        f.send(b"\x04")
        self.assert_detached_cleanly(f)

    def test_replaced_clarification_controls_refuse_but_current_answer_dispatches(self):
        f = self.fixture(running=True)
        def prompt(question):
            f.gateway.event("clarify.request", {"kind": "clarify", "prompt_id": "clarify-id",
                "execution_generation": 2, "question": question}, 2)
        def buttons():
            return [node for node, props in f.nodes.items() if props.get("actions", {}).get("click") and "prompt-" in node]
        prompt("original question")
        f.until(lambda: len(buttons()) == 2, "original clarification")
        old = list(buttons())
        f.auto_ack = False
        f.send(b"\x1b[200~Unicode answer \xf0\x9f\x98\x80\x1b[201~")
        f.until(lambda: bool(f.pending_acks), "clarification credit occupied")
        prompt("replacement question")
        f.quiet()
        for node in old:
            f.click(node)
        f.quiet()
        self.assertFalse(f.gateway.methods("clarify.respond"))
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: len(buttons()) == 2 and not set(old) & set(buttons()), "current clarification")
        f.click(next(node for node in buttons() if node.endswith("choice0")))
        f.until(lambda: len(f.gateway.methods("clarify.respond")) == 1, "current answer")
        self.assertEqual(f.gateway.methods("clarify.respond")[0]["params"], {
            "session_id": "fixture-session", "execution_generation": 2, "prompt_id": "clarify-id", "answer": "Unicode answer 😀"})
        f.until(lambda: f.nodes["dock.ed"].get("text") == "", "answer clears draft")
        f.send(b"\x04")
        self.assert_detached_cleanly(f)


    def test_old_interrupt_button_cannot_interrupt_a_later_turn(self):
        f = self.fixture(running=True)
        def interrupt_buttons():
            return [node for node, props in f.nodes.items() if props.get("actions", {}).get("click") and ".interrupt" in node]
        f.until(lambda: len(interrupt_buttons()) == 1, "original interrupt")
        old = interrupt_buttons()[0]
        f.auto_ack = False
        f.send(b"x")
        f.until(lambda: bool(f.pending_acks), "interrupt credit occupied")
        f.gateway.event("message.start", {}, 3)
        f.quiet()
        f.click(old)
        f.quiet()
        self.assertFalse(f.gateway.methods("session.interrupt"), "old button cancelled a later generation")
        f.auto_ack = True
        for ack in f.pending_acks:
            f.event("ack", **ack)
        f.pending_acks.clear()
        f.until(lambda: len(interrupt_buttons()) == 1 and old not in interrupt_buttons(), "current interrupt")
        f.click(interrupt_buttons()[0])
        f.until(lambda: len(f.gateway.methods("session.interrupt")) == 1, "current interrupt request")
        self.assertEqual(f.gateway.methods("session.interrupt")[0]["params"]["execution_generation"], 3)
        self.assertFalse(f.gateway.methods("prompt.submit"))
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)


if __name__ == "__main__":
    unittest.main()
