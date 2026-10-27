"""Owner-reported plan is display-only in production CLI/PTY; no real owner/provider."""
import copy
import socket
import termios
import unittest

import test_native_chat as native


def plan(revision=1, label="Review parser 😀", count=1):
    return {"revision": revision, "todos": [
        {"id": f"task-{n + 1}", "content": label if n == 0 else f"Child task {n + 1}",
         "status": "in_progress" if n == 0 else "pending", **({"parent": "task-1"} if n else {})}
        for n in range(count)
    ]}


class TaskPlanTests(unittest.TestCase):
    def fixture(self, value=None):
        f = native.NativeChat()
        self.addCleanup(f.close)
        original = f.gateway.reply
        self.initial = copy.deepcopy(value)
        def reply(request, result):
            if request["method"] in ("session.create", "session.resume"):
                result = copy.deepcopy(result)
                result["todo_state"] = copy.deepcopy(self.initial)
            original(request, result)
        f.gateway.reply = reply
        f.ready()
        return f

    def button(self, f, label):
        found = [key for key, node in f.nodes.items() if node.get("kind") == "el" and node.get("text") == label]
        self.assertEqual(len(found), 1, label)
        return found[0]

    def open(self, f):
        f.send(b"\x1bOQ")
        f.until(lambda: "main.runtime.profile" in f.nodes, "runtime details")
        f.click(self.button(f, "Show task plan"))
        f.until(lambda: "main.runtime.plan.heading" in f.nodes, "task plan")

    def texts(self, f):
        return "\n".join(node.get("text", "") for node in f.nodes.values())

    def finish(self, f):
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertEqual(len(f.gateway.methods("session.detach")), 1)
        for name in ("prompt.submit", "session.mutate", "command.dispatch", "subagent.list", "subagent.steer", "subagent.interrupt", "process.list", "approval.respond"):
            self.assertFalse(f.gateway.methods(name), name)

    def test_snapshot_is_visible_locally_and_inspect_only_without_extra_rpc(self):
        value = plan(count=2)
        value["todos"][0]["private"] = "PRIVATE-TODO"
        f = self.fixture(value)
        f.send("\x1b[200~keep draft 😀\x1b[201~".encode())
        f.until(lambda: f.nodes["dock.ed"].get("text") == "keep draft 😀", "draft")
        before = len(f.gateway.requests)
        self.open(f)
        self.assertIn("Review parser 😀", self.texts(f))
        self.assertIn("2 reported tasks", self.texts(f))
        self.assertNotIn("PRIVATE-TODO", self.texts(f))
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\rdo not send")
        f.event("send", sf=f.surface, id="dock.ed", text="never a prompt")
        f.quiet()
        self.assertEqual(len(f.gateway.requests), before)
        f.click(self.button(f, "Next task"))
        f.until(lambda: "Child task 2" in self.texts(f), "second task")
        f.send(b"\x1b")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "conversation")
        self.assertEqual(f.nodes["dock.ed"]["text"], "keep draft 😀")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.assertEqual(len(f.gateway.requests), before)
        self.finish(f)

    def test_versioned_full_updates_replace_locally_and_old_revision_never_regresses(self):
        f = self.fixture(plan(2, "Initial"))
        self.open(f)
        f.gateway.event("todo.updated", plan(4, "Current"))
        f.until(lambda: "Current" in self.texts(f), "new task revision")
        f.gateway.event("todo.updated", plan(3, "STALE"))
        f.quiet()
        self.assertIn("Current", self.texts(f))
        self.assertNotIn("STALE", self.texts(f))
        f.gateway.event("todo.updated", {"revision": 5, "todos": []})
        f.until(lambda: "0 reported tasks" in self.texts(f), "explicit empty plan")
        self.finish(f)

    def test_unreported_and_malformed_are_unavailable_not_empty_or_session_failure(self):
        f = self.fixture()
        self.open(f)
        self.assertIn("Task plan unavailable", self.texts(f))
        self.assertNotIn("0 reported tasks", self.texts(f))
        f.gateway.event("todo.updated", {"revision": 1, "todos": [{"id": "a", "content": "PRIVATE-MALFORMED", "status": True}]})
        f.quiet()
        self.assertIn("Task plan unavailable", self.texts(f))
        self.assertNotIn("PRIVATE-MALFORMED", self.texts(f))
        f.gateway.event("todo.updated", plan(2, "Recovered"))
        f.until(lambda: "Recovered" in self.texts(f), "valid recovery")
        f.send(b"\x1b")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "conversation")
        self.assertTrue(f.nodes["dock.ed"]["sendable"])
        self.finish(f)

    def test_old_snapshot_page_and_panel_callbacks_do_not_navigate_current_plan(self):
        f = self.fixture(plan(1, "Original", count=3))
        self.open(f)
        old_next = self.button(f, "Next task")
        old_back = self.button(f, "Show runtime details")
        f.click(old_next)
        f.until(lambda: "Task page 2/3" in self.texts(f), "old second page")
        f.gateway.event("todo.updated", plan(2, "Fresh first", count=2))
        f.until(lambda: "Fresh first" in self.texts(f), "fresh plan")
        self.assertIn("Task page 1/2", self.texts(f))
        f.click(old_next)
        f.quiet()
        self.assertIn("Fresh first", self.texts(f))
        f.click(self.button(f, "Show runtime details"))
        f.until(lambda: "main.runtime.profile" in f.nodes, "runtime return")
        f.click(self.button(f, "Show task plan"))
        f.until(lambda: "main.runtime.plan.heading" in f.nodes, "new plan opening")
        f.click(old_back)
        f.quiet()
        self.assertIn("Task plan · owner-reported snapshot", self.texts(f))
        f.click(self.button(f, "Next task"))
        f.until(lambda: "Task page 2/2" in self.texts(f), "current second page")
        self.finish(f)

    def test_generation_and_disconnect_remove_current_rows_without_automatic_requests(self):
        f = self.fixture(plan(3, "OLD-PLAN"))
        self.open(f)
        f.gateway.event("session.info", {}, generation=3)
        f.until(lambda: "Task plan unavailable" in self.texts(f), "generation invalidation")
        self.assertNotIn("OLD-PLAN", self.texts(f))
        f.gateway.event("todo.updated", plan(1, "Fresh generation"), generation=3)
        f.until(lambda: "Fresh generation" in self.texts(f), "generation report")
        assert f.gateway.socket is not None
        f.gateway.socket.shutdown(socket.SHUT_RDWR)
        f.until(lambda: "Task plan unavailable" in self.texts(f), "disconnected plan")
        self.assertNotIn("Fresh generation", self.texts(f))
        f.send(b"\x1b")
        f.until(lambda: "dock.navigation.runtime" in f.nodes, "disconnected conversation")
        self.assertFalse(f.nodes["dock.ed"]["sendable"])
        f.send(b"\x04")
        self.assertEqual(f.wait_exit(), 0)
        self.assertEqual(termios.tcgetattr(f.slave), f.original_termios)
        self.assertFalse(f.gateway.methods("prompt.submit") or f.gateway.methods("session.resume"))

    def test_complete_count_content_bounds_and_unknown_private_fields_do_not_become_partial_success(self):
        value = plan(1, "😀" * 4000, count=256)
        value["todos"][0]["private"] = "PRIVATE-DO-NOT-RENDER"
        f = self.fixture(value)
        self.open(f)
        self.assertIn("256 reported tasks", self.texts(f))
        self.assertIn("pending: 255", self.texts(f))
        self.assertIn("😀" * 4000, self.texts(f))
        self.assertNotIn("PRIVATE-DO-NOT-RENDER", self.texts(f))
        f.gateway.event("todo.updated", plan(2, "OVER-CAP", count=257))
        f.until(lambda: "Task plan unavailable" in self.texts(f), "whole over-cap refusal")
        self.assertNotIn("256 reported tasks", self.texts(f))
        self.assertNotIn("OVER-CAP", self.texts(f))
        f.gateway.event("todo.updated", plan(3, "Recovered after cap"))
        f.until(lambda: "Recovered after cap" in self.texts(f), "bounded report")
        self.finish(f)


if __name__ == "__main__":
    unittest.main()
