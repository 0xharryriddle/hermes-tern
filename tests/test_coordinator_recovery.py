"""Actual binary/Unix IPC recovery; synthetic identities only, no agent inference."""
import concurrent.futures
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import socket
import sqlite3
import subprocess
import time
import unittest
from unittest.mock import patch
import uuid

from fixture_process import wait_supervisor

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/debug/hermes-tern"


class CoordinatorRecoveryTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(os.environ["TMPDIR"]) / ("ht-ipc-recovery-" + uuid.uuid4().hex)
        self.root.mkdir(mode=0o700)
        self.state = self.root / "state"
        self.server = None
        self.addCleanup(self.stop_and_inventory)
        self.start()
        self.a = self.register("a")
        self.b = self.register("b")
        self.cli("route", "--sender", self.a["actor_id"], "--recipient", self.b["actor_id"], "--allow")

    def start(self):
        self.server = subprocess.Popen(
            [str(BINARY), "serve", "--state", str(self.state), "--workspace", str(self.root)],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        self.ready()

    def ready(self):
        self.startup_seconds = wait_supervisor(self.server, BINARY, self.state)

    def restart(self):
        assert self.server is not None
        self.server.kill()
        self.server.wait(timeout=3)
        self.start()

    def stop_and_inventory(self):
        if self.server is not None:
            if self.server.poll() is None:
                self.server.kill()
            self.server.wait(timeout=3)
        paths = [self.root, *self.root.rglob("*")]
        (self.root / "ownership.json").write_text(json.dumps({
            "retention": "retain; no deletion", "manifest": str(self.root / "ownership.json"),
            "paths": [{"path": str(p), "inode": p.lstat().st_ino} for p in paths],
        }, indent=2) + "\n")

    def cli(self, *args, check=True):
        result = subprocess.run([str(BINARY), "coord", "--state", str(self.state), *args],
                                capture_output=True, timeout=6)
        if check:
            self.assertEqual(result.returncode, 0, "fixture operator command failed")
        return result

    def register(self, session):
        actor = json.loads(self.cli("register", "--kind", "hermes", "--profile", "fixture",
                                    "--session", session, "--model", "fixture", "--provider", "no-inference").stdout)
        actor["token"] = json.loads(Path(actor["credential_file"]).read_text())["credential"]
        return actor

    def connect(self):
        address = "\0hermes-tern-" + str(os.geteuid()) + "-" + hashlib.sha256(os.fsencode(self.state.resolve())).hexdigest()
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.settimeout(4)
        sock.connect(address)
        return sock

    def frame(self, actor, op):
        request = {"v": 1, "id": str(uuid.uuid4()), "auth": actor["token"], "op": op}
        return request["id"], json.dumps(request).encode() + b"\n"

    def lose_response(self, actor, op):
        _, frame = self.frame(actor, op)
        with self.connect() as sock:
            sock.sendall(frame)
            # Close without reading; never infer rollback from lost response.

    def call(self, actor, op):
        request_id, frame = self.frame(actor, op)
        with self.connect() as sock:
            sock.sendall(frame)
            with sock.makefile("rb") as stream:
                response = json.loads(stream.readline(524289))
            self.assertEqual(response["id"], request_id)
            return response

    def rows(self, sql, args=()):
        # Read-only observation is a synchronization barrier, not a simulated response.
        with closing(sqlite3.connect(f"file:{self.state / 'coordination.sqlite3'}?mode=ro", uri=True)) as db:
            return db.execute(sql, args).fetchall()

    def wait_rows(self, sql, expected, args=()):
        deadline = time.monotonic() + 5
        while True:
            rows = self.rows(sql, args)
            if rows == expected:
                return
            self.assertLess(time.monotonic(), deadline, "durable state not observed")
            time.sleep(0.01)

    def send_op(self):
        return {"type": "send", "message": {
            "message_id": str(uuid.uuid4()), "recipient": self.b["actor_id"], "kind": "context",
            "payload": {"text": "untrusted recovery fixture"}, "ttl_seconds": 300, "correlation_id": None,
        }}

    def test_lost_send_receive_and_ack_responses_survive_killed_process(self):
        op = self.send_op()
        mid = op["message"]["message_id"]
        self.lose_response(self.a, op)
        self.wait_rows("SELECT state FROM messages WHERE id=?", [("queued",)], (mid,))
        self.restart()
        retry = self.call(self.a, op)
        self.assertTrue(retry["ok"])
        self.assertTrue(retry["result"]["duplicate"])
        self.assertEqual(self.rows("SELECT count(*) FROM messages"), [(1,)])
        self.lose_response(self.b, {"type": "receive"})
        self.wait_rows("SELECT state FROM messages WHERE id=?", [("submitted",)], (mid,))
        self.restart()
        self.assertEqual(self.rows("SELECT state FROM messages WHERE id=?", (mid,)), [("ambiguous",)])
        response = self.call(self.b, {"type": "receive"})
        self.assertTrue(response["ok"])
        self.assertEqual(response["result"]["messages"], [])
        # Fixture reconciles via its independent SQLite observation, not a live agent ACK.
        ack = {"type": "ack", "sender": self.a["actor_id"], "message": mid}
        self.lose_response(self.b, ack)
        self.wait_rows("SELECT state FROM messages WHERE id=?", [("consumed",)], (mid,))
        self.restart()
        self.assertEqual(self.call(self.b, ack)["result"]["state"], "consumed")
        self.assertEqual(self.rows("SELECT count(*) FROM events WHERE kind='message.consumed'"), [(1,)])

    def test_concurrent_task_claims_have_one_owner_and_stale_completion_is_fenced(self):
        task = str(uuid.uuid4())
        claim = {"type": "claim", "task": task, "ttl_seconds": 300}
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            responses = list(pool.map(lambda actor: self.call(actor, claim), [self.a, self.b]))
        self.assertEqual(sum(r["ok"] for r in responses), 1)
        winner, loser = (self.a, self.b) if responses[0]["ok"] else (self.b, self.a)
        self.assertEqual(self.call(winner, claim)["result"]["generation"], "1")
        self.restart()
        self.assertFalse(self.call(loser, claim)["ok"])
        lease = json.loads(self.cli("inspect-task", "--task", task).stdout)["task"]
        self.cli("release-stopped", "--task", task, "--owner", lease["owner_actor_id"],
                 "--incarnation", lease["incarnation"], "--generation", lease["generation"], "--attest-stopped")
        self.assertEqual(self.call(loser, claim)["result"]["generation"], "2")
        self.assertFalse(self.call(winner, {"type": "complete", "task": task, "generation": "1", "result": "stale"})["ok"])
        complete = {"type": "complete", "task": task, "generation": "2", "result": "fixture result"}
        self.lose_response(loser, complete)
        self.wait_rows("SELECT state FROM tasks WHERE id=?", [("complete",)], (task,))
        self.restart()
        self.assertEqual(self.call(loser, complete)["result"]["state"], "complete")
        self.assertFalse(self.call(loser, {**complete, "result": "conflicting result"})["ok"])
        self.assertEqual(self.rows("SELECT count(*) FROM events WHERE kind='task.complete'"), [(1,)])

    def test_lost_registration_and_rotation_reply_recover_via_operator_snapshot(self):
        # Private fixture key only; never log a credential or scan personal state.
        operator = {"token": (self.state / "operator.key").read_text()}
        self.lose_response(operator, {"type": "register", "kind": "hermes", "profile": "fixture",
                                     "session": "lost-registration", "model": "fixture", "provider": "no-inference"})
        self.wait_rows("SELECT count(*) FROM actors WHERE session='lost-registration'", [(1,)])
        self.restart()
        rows = json.loads(self.cli("actors").stdout)["actors"]
        first = next(row for row in rows if row["session_id"] == "lost-registration")
        self.assertNotIn("credential", first)
        first["token"] = json.loads(Path(first["credential_file"]).read_text())["credential"]
        self.assertTrue(self.call(first, {"type": "receive"})["ok"])
        self.lose_response(operator, {"type": "rotate", "actor": first["actor_id"]})
        self.wait_rows("SELECT count(*) FROM actors WHERE id=? AND incarnation<>?", [(1,)],
                       (first["actor_id"], first["incarnation"]))
        self.restart()
        latest = next(row for row in json.loads(self.cli("actors").stdout)["actors"] if row["actor_id"] == first["actor_id"])
        self.assertNotEqual(latest["incarnation"], first["incarnation"])
        latest["token"] = json.loads(Path(latest["credential_file"]).read_text())["credential"]
        self.assertFalse(self.call(first, {"type": "receive"})["ok"])
        self.assertTrue(self.call(latest, {"type": "receive"})["ok"])
        self.assertEqual(self.rows("SELECT count(*) FROM events WHERE kind='actor.rotated'"), [(1,)])

    def test_incomplete_frame_cannot_become_accepted_mail(self):
        op = self.send_op()
        request = {"v": 1, "id": "partial", "auth": self.a["token"], "op": op}
        with self.connect() as sock:
            sock.sendall(json.dumps(request).encode())  # Valid JSON but no frame delimiter.
            sock.shutdown(socket.SHUT_WR)
            with sock.makefile("rb") as stream:
                response = json.loads(stream.readline(524289))
            self.assertFalse(response["ok"])
        self.assertEqual(self.rows("SELECT count(*) FROM messages"), [(0,)])
        self.restart()
        self.assertTrue(self.call(self.a, op)["ok"])
        self.assertEqual(self.rows("SELECT count(*) FROM messages"), [(1,)])

    def test_read_only_observer_closes_its_real_sqlite_connection(self):
        real_connect = sqlite3.connect
        connections = []

        def remember(*args, **kwargs):
            connection = real_connect(*args, **kwargs)
            connections.append(connection)
            return connection

        try:
            with patch.object(sqlite3, "connect", side_effect=remember):
                self.assertEqual(self.rows("SELECT count(*) FROM actors"), [(2,)])
            self.assertEqual(len(connections), 1)
            with self.assertRaises(sqlite3.ProgrammingError, msg="observer leaked its SQLite handle"):
                connections[0].execute("SELECT 1")
        finally:
            for connection in connections:
                connection.close()


if __name__ == "__main__":
    unittest.main()
