"""Actual Tern + production client + synthetic gateway input/control acceptance.

Never a Hermes/provider session or real-window/plugin proof. Only synthetic Deny,
Skip and Interrupt actions are driven; no affirmative tool approval is automated.
"""
import argparse
import hashlib
import importlib.util
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import socket
import stat
import struct
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
CASES = ("conversation", "deny", "skip", "interrupt", "disconnect", "long-output", "keyboard-output", "keyboard-reduced", "history", "launcher-close", "launcher-new", "launcher-resume", "runtime-details", "model-picker", "profile-picker", "commands", "attachments", "mcp-reload", "plugins")
KEYBOARD_CASES = ("keyboard-output", "keyboard-reduced")
KEYBOARD_DRAFT = "keyboard draft remains"
# Keep normalized text below the measured Tern tree text cap while exceeding
# the production twelve-line fold boundary. Full raw text is checked by PTY.
LONG_TEXT = "".join(f"row{i:02d}\n" for i in range(1, 24)) + "fixture retained tail"
# Tern's control tree normalizes whitespace and caps text at 200 characters.
# Keep this fixture complete below that cap; full raw output is checked by PTY.
TOOL_CATALOG_TEXT = ("Owner tool catalog · 2 reported tools "
                     "Catalog entries are data, not execution permission or health checks. "
                     "Group: fx Tool: fx-read Description: Read Tool: fx-list Description: List")


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load test support")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load("native_chat_runner", ROOT / "scripts/verify_runtime.py")
smoke = load("native_chat_smoke", ROOT / "scripts/smoke_native.py")
fixtures = load("native_chat_fixture", ROOT / "tests/test_native_chat.py")


class ScenarioGateway(fixtures.Gateway):
    def __init__(self, case):
        self.case = case
        history = {"count": 100, "messages": [{"role": "assistant", "row_id": n, "text": f"history row {n}"}
                                             for n in range(65)]} if case == "history" else None
        super().__init__(running=case in ("deny", "skip", "interrupt"), disconnect_on_submit=case == "disconnect",
                         accept_timeout=60, read_timeout=60, history=history,
                         catalog={"live":{"scope":"live","sessions":[{"session_id":"fixture-session","title":"Fixture conversation","running":False},{"session_id":"other-session","title":"Fixture conversation","running":False}]},"stored":{"scope":"stored","sessions":[{"session_id":"saved-exact","title":"Saved fixture","running":False}]}} if case.startswith("launcher-") else None)
        if case == "model-picker":
            self.model_mode = "commit"
        if case in ("commands", "plugins"):
            self.command_mode = "valid"
        if case == "attachments":
            self.attachment_mode = "valid"
        if case == "mcp-reload":
            self.mcp_mode = "applied"
        if case == "runtime-details":
            self.delegated_mode = "valid"

    def reply(self, request, result):
        method = request["method"]
        if method == "commands.catalog" and self.case == "plugins":
            result = {"pairs":[["/plugins","Plugin inventory"]], "canon":{"/plugins":"/plugins"},
                      "commands":{"/plugins":{}}, "skills":{}, "plugin_commands":{}}
        if method == "plugins.list" and self.case == "plugins":
            result = {"plugins":[{"name":"fx","key":"fx","version":"1","enabled":True,
                                  "tools":1,"hooks":0,"commands":0,"error":""}],
                      "toolsets":[], "portable_mcp_servers":[]}
        if method == "subagent.list" and self.case == "runtime-details":
            result = {"subagents":[{"subagent_id":"fx-child","parent_id":None,"depth":0,
                       "goal":"Review parser","delegation_id":"fx-batch","model":"fx-model",
                       "started_at":100.5,"status":"running","tool_count":2,"last_tool":"read_file",
                       "accepting_steer":False}],"delegations":[]}
        if method == "tools.show" and self.case == "commands":
            result = {"sections":[{"name":"fx","tools":[
                {"name":"fx-read","description":"Read"},
                {"name":"fx-list","description":"List"}]}],"total":2}
        if method == "session.create":
            if self.case == "model-picker":
                result["info"].update(self.model_route)
            if self.case == "runtime-details":
                result["info"].update(tools={"core":["read_file","terminal"]}, skills={"rust":["safe-rust"]}, mcp_servers=[{"name":"catalog","status":"connected"}])
                result["todo_state"] = {"revision":2,"todos":[{"id":"fx-task","content":"Check parser","status":"in_progress"}]}
            if self.case == "long-output" or self.case in KEYBOARD_CASES:
                result["messages"] = [{"row_id": 1, "role": "tool", "text": LONG_TEXT}]
            if self.case == "deny":
                result["prompts"] = [{"kind": "approval", "prompt_id": "fixture-approval", "execution_generation": 2,
                                      "choices": ["once", "deny"], "command": "fixture-only command; never executed"}]
            elif self.case == "skip":
                result["prompts"] = [{"kind": "clarify", "prompt_id": "fixture-clarify", "execution_generation": 2,
                                      "question": "Fixture question — choose Skip; no real data requested"}]
        super().reply(request, result)
        if method == "session.create" and self.case == "interrupt":
            self.event("message.start", {})
        elif method in ("approval.respond", "clarify.respond", "session.interrupt"):
            if method == "approval.respond":
                if request["params"].get("choice") != "deny":
                    raise ValueError("fixture refused affirmative approval")
                self.event("approval.settled", {"prompt_id": "fixture-approval"})
                marker = "fixture-denied"
            elif method == "clarify.respond":
                if request["params"].get("answer") != "":
                    raise ValueError("fixture expected explicit Skip")
                self.event("clarify.settled", {"prompt_id": "fixture-clarify"})
                marker = "fixture-skipped"
            else:
                marker = "fixture-interrupted-observed"
            self.event("session.info", {"running": False})
            self.event("notice", {"text": marker})


def scenario(case, command, stderr):
    if case not in CASES:
        raise ValueError("unknown native case")
    shell = ("umask 077; " + shlex.join(command) + " 2>" + shlex.quote(str(stderr))
             + '; rc=$?; printf "%s\\n" "$rc" >' + shlex.quote(str(stderr.with_name("client.exit")))
             + '; printf "HT_CLIENT_EXIT_%s\\n" "$rc"')
    if case == "profile-picker":
        return ('ready\nrun ' + json.dumps(shell) + '\nplugins expect "New conversation"\n'
                'click ".ht-profiles-entry"\nplugins expect "Hermes profiles"\nshot profiles\n'
                'click ".ht-profile-list>div:last-child>button"\nplugins expect "New conversation"\nshot switched\n'
                'click ".ht-launch-controls>button:first-child"\nplugins expect "Hermes / fixture-session"\nshot opened\n'
                'click ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n', ["profiles","switched","opened"])
    if case.startswith("launcher-"):
        text = "ready\nrun " + json.dumps(shell) + '\nplugins expect "New conversation"\nshot launcher\n'
        if case == "launcher-close":
            return text + 'click ".ht-launch-controls>button:last-child"\nexpect "HT_CLIENT_EXIT_0"\n', ["launcher"]
        if case == "launcher-new":
            text += 'click ".ht-launch-controls>button:first-child"\n'
        else:
            text += 'click ".ht-launch-controls>button:nth-of-type(3)"\nplugins expect "Saved fixture"\nclick ".ht-launch-row button"\n'
        return text + 'plugins expect "Hermes / fixture-session"\nshot opened\nclick ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n', ["launcher", "opened"]
    if case == "history":
        return ("ready\nrun " + json.dumps(shell) + '\nplugins expect "History projection"\n'
                'plugins expect "history row 0"\nshot first\n'
                'key Right\nplugins expect "history row 32"\n'
                'key Left\nplugins expect "history row 0"\nkey End\n'
                'plugins expect "history row 64"\nshot last\nkey Home\n'
                'plugins expect "history row 0"\nshot back\n'
                'click ".ht-history-controls>button:last-child"\nexpect "HT_CLIENT_EXIT_0"\n',
                ["first", "last", "back"])
    text = "ready\nrun " + json.dumps(shell) + '\nplugins expect "Hermes / fixture-session"\n'
    if case == "plugins":
        return (text + 'type "plugin draft remains"\nclick ".ht-command-entry"\n'
                'plugins expect "Read /plugins"\nshot plugins\nclick ".ht-command-read"\n'
                'plugins expect "Owner plugins · 1 plugins"\nshot output\n'
                'click ".ht-command-return"\nplugins expect "Hermes / fixture-session"\nshot returned\n'
                'click ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n',
                ["plugins", "output", "returned"])
    if case == "mcp-reload":
        return (text + 'type "mcp draft remains"\nclick ".ht-runtime-entry"\n'
                'plugins expect "Profile: fixture-profile"\nclick ".ht-mcp-entry"\n'
                'plugins expect "MCP reload · profile-wide operation"\nshot review\n'
                'click ".ht-mcp-confirm"\nplugins expect "MCP reattach readback complete"\nshot reconciled\n'
                'click ".ht-mcp-return"\nplugins expect "Hermes / fixture-session"\nshot returned\n'
                'click ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n',
                ["review", "reconciled", "returned"])
    if case == "attachments":
        path = str(stderr.parent / "review.txt")
        return (text + 'type "attachment draft remains"\nkey F5\n'
                'plugins expect "Attach a text file"\ntype ' + json.dumps(path) + '\n'
                'click ".ht-attachment-review"\nplugins expect "Reviewed: review.txt · 18 bytes"\nshot reviewed\n'
                'click ".ht-attachment-stage"\nplugins expect "Staged: review.txt · attachments/review.txt"\nshot staged\n'
                'click ".ht-attachment-add"\nplugins expect "@file:attachments/review.txt"\nshot added\n'
                'click ".ht-attachment-return"\nplugins expect "Hermes / fixture-session"\nshot returned\n'
                'click ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n',
                ["reviewed","staged","added","returned"])
    if case == "commands":
        return (text + 'type "command draft remains"\nclick ".ht-command-entry"\n'
                'plugins expect "Owner commands"\nshot commands\nclick ".ht-command-read"\n'
                'plugins expect "fixture-owner-memory"\nshot output\n'
                'click ".ht-command-list>div:nth-child(3)>.ht-command-read"\n'
                'plugins expect "Owner tool catalog · 2 reported tools"\nshot tools\nclick ".ht-command-return"\n'
                'plugins expect "Hermes / fixture-session"\nshot returned\n'
                'click ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n', ["commands","output","tools","returned"])
    if case == "model-picker":
        return (text + 'type "model draft remains"\nclick ".ht-model-entry"\n'
                'plugins expect "Select model-exact"\nshot models\nclick ".ht-model-select"\n'
                'shot selected\nclick ".ht-model-apply"\n'
                'plugins expect "Active route: model-exact"\nshot applied\n'
                'click ".ht-controls>button:nth-last-child(2)"\nplugins expect "Hermes / fixture-session"\n'
                'shot returned\nclick ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n',
                ["models","selected","applied","returned"])
    if case == "runtime-details":
        return (text + 'type "runtime draft remains"\nclick ".ht-runtime-entry"\n'
                'plugins expect "Profile: fixture-profile"\nshot runtime\n'
                'click ".ht-delegated-read"\nplugins expect "1 reported delegated agents"\nshot delegated\n'
                'click ".ht-plan-entry"\nplugins expect "1 reported tasks"\nshot plan\n'
                'click ".ht-runtime-return"\nplugins expect "Hermes / fixture-session"\n'
                'shot returned\nclick ".ht-controls>button:nth-of-type(2)"\nexpect "HT_CLIENT_EXIT_0"\n', ["runtime","delegated","plan","returned"])
    if case in KEYBOARD_CASES:
        text += ('plugins expect "Show retained text"\ntype ' + json.dumps(KEYBOARD_DRAFT) + '\n'
                 'key Tab\nshot selected\nkey Enter\nplugins expect "Collapse retained text"\nshot expanded\n'
                 'key Space\nplugins expect "Show retained text"\nshot refolded\nkey Escape\n')
        shots = ["selected", "expanded", "refolded"]
    elif case == "long-output":
        text += ('plugins expect "Show retained text"\nshot folded\n'
                 'click ".ht-transcript-toggle"\nplugins expect "Collapse retained text"\nshot expanded\n'
                 'click ".ht-transcript-toggle"\nplugins expect "Show retained text"\nshot refolded\n')
        shots = ["folded", "expanded", "refolded"]
    elif case in ("conversation", "disconnect"):
        marker = "fixture-complete" if case == "conversation" else "disconnected; resume to reconcile"
        text += 'type "fixture typed message"\nkey Enter\nplugins expect ' + json.dumps(marker) + '\n'
        if case == "disconnect":
            # Active resubmit refusal is covered by the production PTY suite.
            # The second-Enter variant had timing-sensitive click failures;
            # this native case measures draft/display and explicit exit only.
            text += 'type "retained after disconnect"\n'
        shots = ["completed" if case == "conversation" else "disconnected"]
        text += "shot " + shots[0] + "\n"
    else:
        marker = {"deny": "Human approval", "skip": "Human clarify", "interrupt": "running"}[case]
        text += "plugins expect " + json.dumps(marker) + "\nshot waiting\n"
        selector = '.ht-controls>button:first-child' if case == "interrupt" else '.sf-main button:nth-of-type(2)'
        completed = {"deny": "fixture-denied", "skip": "fixture-skipped", "interrupt": "fixture-interrupted-observed"}[case]
        text += "click " + json.dumps(selector) + "\nplugins expect " + json.dumps(completed) + "\nshot completed\n"
        shots = ["waiting", "completed"]
    # Explicit positions passed the isolated controls. The discarded second-Enter
    # variant also exposed timing-sensitive clicks; no universal selector claim.
    detach = '.ht-controls>button:first-child' if case == "disconnect" else '.ht-controls>button:nth-of-type(2)'
    text += "click " + json.dumps(detach) + '\nexpect "HT_CLIENT_EXIT_0"\n'
    return text, shots


def validate_profile_requests(first, second, errors):
    inventory = ["ping","runtime.describe","session.list","session.list"]
    wanted = [inventory + ["profiles.list"], inventory + ["ping","runtime.describe","client.capabilities","session.create","session.detach"]]
    if errors or any([r.get("method") for r in rows] != methods for rows,methods in zip((first,second),wanted)):
        raise ValueError("profile switch request ownership/order differs")
    if first[-1].get("params") != {"include_sessions":False}:
        raise ValueError("profile roster scope differs")
    for rows in (first,second):
        if [r.get("params") for r in rows[2:4]] != [{"scope":"live","limit":200},{"scope":"stored","limit":200}]:
            raise ValueError("profile session inventory scope differs")
    if second[6].get("params") != {"server_requests":True} or second[7]["params"].get("source") != "tui" or second[7]["params"].get("yolo") is not False:
        raise ValueError("unsafe selected profile session creation")
    if second[-1].get("params") != {"session_id":"fixture-session","subscription_id":"fixture-sub"}:
        raise ValueError("selected profile detach differs")
    return {m:sum(r.get("method")==m for r in first+second) for m in set(wanted[0]+wanted[1])}


def validate_requests(case, rows, errors):
    if case not in CASES or errors or not isinstance(rows, list):
        raise ValueError("invalid fixture evidence")
    if case.startswith("launcher-"):
        expected = ["ping","runtime.describe","session.list","session.list"]
        if case != "launcher-close":
            expected += ["ping","runtime.describe","client.capabilities",
                         "session.create" if case == "launcher-new" else "session.resume","session.detach"]
        if [r.get("method") for r in rows] != expected:
            raise ValueError("launcher inventory/activation dispatch differs")
        if [r.get("params") for r in rows[2:4]] != [{"scope":"live","limit":200},{"scope":"stored","limit":200}]:
            raise ValueError("launcher inventory scope differs")
        if case == "launcher-new" and (rows[-2]["params"].get("source") != "tui" or rows[-2]["params"].get("yolo") is not False):
            raise ValueError("unsafe launcher creation")
        if case == "launcher-resume" and rows[-2]["params"] != {"session_id":"saved-exact","display_projection":True}:
            raise ValueError("launcher resume identity differs")
        if case != "launcher-close" and rows[-1]["params"] != {"session_id":"fixture-session","subscription_id":"fixture-sub"}:
            raise ValueError("launcher detach identity differs")
        return {method:expected.count(method) for method in expected}
    if case == "history":
        if [r.get("method") for r in rows] != ["ping", "runtime.describe", "session.history"] or [r.get("params") for r in rows] != [{}, {}, {"session_id": "fixture-session"}]:
            raise ValueError("history must only qualify and read once; no resume or mutation")
        return {"session.history": 1}
    if case == "plugins":
        methods=["ping","runtime.describe","client.capabilities","session.create","commands.catalog","plugins.list","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("plugin inventory must read once without mutation/management or retry")
        if rows[3]["params"].get("source") != "tui" or rows[3]["params"].get("yolo") is not False:
            raise ValueError("unsafe plugin fixture creation")
        if [r.get("params") for r in rows[4:]] != [{"session_id":"fixture-session"},{},
                {"session_id":"fixture-session","subscription_id":"fixture-sub"}]:
            raise ValueError("plugin profile-scoped request/detach differs")
        return {method:methods.count(method) for method in methods}
    if case == "mcp-reload":
        methods=["ping","runtime.describe","client.capabilities","session.create","reload.mcp","session.resume","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("MCP must confirm once then reattach without prompt/config/other mutations")
        if rows[3]["params"].get("source") != "tui" or rows[3]["params"].get("yolo") is not False:
            raise ValueError("unsafe MCP fixture creation")
        if [r.get("params") for r in rows[4:]] != [{"session_id":"fixture-session","confirm":True},
                {"session_id":"fixture-session","display_projection":True},
                {"session_id":"fixture-session","subscription_id":"fixture-sub"}]:
            raise ValueError("MCP consent/reattach/detach identity differs")
        return {method:methods.count(method) for method in methods}
    if case == "attachments":
        methods=["ping","runtime.describe","client.capabilities","session.create","file.attach","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("attachment must stage once without prompt, command or model mutation")
        if rows[3]["params"].get("source")!="tui" or rows[3]["params"].get("yolo") is not False:
            raise ValueError("unsafe attachment fixture creation")
        if rows[4].get("params") != {"session_id":"fixture-session","name":"review.txt","data_url":"data:text/plain;base64,bmF0aXZlIGF0dGFjaG1lbnQK"}:
            raise ValueError("attachment snapshot bytes or exact owner request differs")
        if rows[5].get("params") != {"session_id":"fixture-session","subscription_id":"fixture-sub"}:
            raise ValueError("attachment detach target differs")
        return {method:methods.count(method) for method in methods}
    if case == "runtime-details":
        methods = ["ping","runtime.describe","client.capabilities","session.create","subagent.list","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("runtime details must explicitly read delegated agents once without other control dispatch")
        if rows[3]["params"].get("source") != "tui" or rows[3]["params"].get("yolo") is not False:
            raise ValueError("unsafe runtime fixture creation")
        if rows[4].get("params") != {"session_id":"fixture-session"} or rows[5].get("params") != {"session_id":"fixture-session","subscription_id":"fixture-sub"}:
            raise ValueError("delegated read or detach identity differs")
        return {method:methods.count(method) for method in methods}
    if case == "commands":
        methods = ["ping","runtime.describe","client.capabilities","session.create","commands.catalog","command.dispatch","tools.show","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("command panel must explicitly read one handler and one tool catalog without prompt or config mutation")
        if rows[3]["params"].get("source") != "tui" or rows[3]["params"].get("yolo") is not False:
            raise ValueError("unsafe command fixture creation")
        if [r.get("params") for r in rows[4:]] != [{"session_id":"fixture-session"},{"session_id":"fixture-session","name":"memory","arg":""},{"session_id":"fixture-session"},{"session_id":"fixture-session","subscription_id":"fixture-sub"}]:
            raise ValueError("command catalog/handler/tool catalog/detach identity differs")
        return {method:methods.count(method) for method in methods}
    if case == "model-picker":
        methods = ["ping","runtime.describe","client.capabilities","session.create","model.options","session.mutate","session.info","session.resume","session.detach"]
        if [r.get("method") for r in rows] != methods:
            raise ValueError("model picker must apply exactly once then reconcile owner before detach")
        if rows[3]["params"].get("yolo") is not False or rows[3]["params"].get("source") != "tui":
            raise ValueError("unsafe model fixture creation")
        if rows[4]["params"] != {"session_id":"fixture-session","explicit_only":True,"include_unconfigured":False,"refresh":False}:
            raise ValueError("model inventory scope differs")
        params = rows[5]["params"]
        if not isinstance(params.get("request_id"),str) or not params["request_id"] or {k:v for k,v in params.items() if k != "request_id"} != {"session_id":"fixture-session","expected_revision":1,"expected_generation":2,"operation":"model","payload":{"model":"model-exact","provider":"provider-exact"}}:
            raise ValueError("model mutation identity/fences differ")
        for row,wanted in zip(rows[6:], ({"session_id":"fixture-session"},{"session_id":"fixture-session","display_projection":True},{"session_id":"fixture-session","subscription_id":"fixture-sub"})):
            if row.get("params") != wanted:
                raise ValueError("model owner readback/detach target differs")
        return {method:methods.count(method) for method in methods}
    methods = {}
    for row in rows:
        if not isinstance(row, dict) or row.get("method") not in (
                "ping", "runtime.describe", "client.capabilities", "session.create", "prompt.submit",
                "approval.respond", "clarify.respond", "session.interrupt", "session.detach"):
            raise ValueError("unexpected fixture request")
        methods.setdefault(row["method"], []).append(row.get("params"))
    create = methods.get("session.create", [])
    if len(create) != 1 or create[0].get("yolo") is not False or create[0].get("source") != "tui":
        raise ValueError("unsafe or duplicate session creation")
    expected = {"prompt.submit": 0, "approval.respond": 0, "clarify.respond": 0, "session.interrupt": 0,
                "session.detach": 0 if case == "disconnect" else 1}
    action = {"conversation": "prompt.submit", "disconnect": "prompt.submit", "deny": "approval.respond",
              "skip": "clarify.respond", "interrupt": "session.interrupt"}.get(case)
    if action is not None:
        expected[action] = 1
    if any(len(methods.get(method, [])) != count for method, count in expected.items()):
        raise ValueError("missing, duplicate or unintended control dispatch")
    if case in ("conversation", "disconnect"):
        p = methods[action][0]
        if p.get("session_id") != "fixture-session" or p.get("text") != "fixture typed message" or not p.get("input_id") or p.get("clarify_batch") is not True:
            raise ValueError("submission body differs")
    elif action is not None:
        expected_params = {"session_id": "fixture-session", "execution_generation": 2}
        if case == "deny":
            expected_params.update(prompt_id="fixture-approval", choice="deny")
        elif case == "skip":
            expected_params.update(prompt_id="fixture-clarify", answer="")
        if methods[action][0] != expected_params:
            raise ValueError("control identity or decision differs")
    if case != "disconnect" and methods["session.detach"][0] != {"session_id": "fixture-session", "subscription_id": "fixture-sub"}:
        raise ValueError("detach target differs")
    return {method: len(methods.get(method, [])) for method in expected}


def validate_profile_geometry(data, shot, width, height):
    required = {"sf-dock":1,"ht-profile-back":1,"ht-profile-close":1,"ht-profile-select":2} if shot == "profiles" else {"ht-launch-button":5}
    for token,count in required.items():
        matches = [e for e in data["elements"] if token in str(e.get("path","")).split(">")[-1].split(".")
                   and (shot == "profiles" or any("ht-launch-controls" in part.split(".") for part in str(e.get("path","")).split(">")[0:-1]))]
        if len(matches)!=count or not all(e.get("visible") and len(e.get("rect",[]))==4 and
            all(type(n) in (int,float) and math.isfinite(n) for n in e["rect"]) and
            e["rect"][0]>=0 and e["rect"][1]>=0 and e["rect"][2]>0 and e["rect"][3]>0 and
            e["rect"][0]+e["rect"][2]<=width+0.5 and e["rect"][1]+e["rect"][3]<=height+0.5 for e in matches):
            raise ValueError("missing or offscreen profile navigation controls")


def validate_navigation_geometry(data, width, height):
    rects = []
    for token in ("ht-runtime-entry", "ht-model-entry", "ht-command-entry"):
        matches = [e for e in data["elements"] if token in str(e.get("path", "")).split(">")[-1].split(".")]
        if len(matches) != 1:
            raise ValueError("missing or ambiguous native navigation entry")
        entry = matches[0]
        rect = entry.get("rect", [])
        if not entry.get("visible") or len(rect) != 4 or any(type(n) not in (int, float) or not math.isfinite(n) for n in rect):
            raise ValueError("invalid native navigation geometry")
        x, y, w, h = rect
        if x < 0 or y < 0 or w <= 0 or h <= 0 or x+w > width+0.5 or y+h > height+0.5:
            raise ValueError("offscreen native navigation entry")
        rects.append(rect)
    if any(abs(rect[1]-rects[0][1]) > 0.5 for rect in rects[1:]) or any(
        left[0]+left[2] > right[0]+0.5 for left, right in zip(rects, rects[1:])
    ):
        raise ValueError("native navigation wrapped or overlaps; preserve conversation viewport")


def validate_outputs(directory, case, shots, theme, size):
    width, height, scale = smoke.parse_size(size)
    layouts = sorted(Path(directory).rglob("*.layout.json"))
    images = sorted(Path(directory).rglob("*.png"))
    if len(layouts) != len(shots) or len(images) != len(shots):
        raise ValueError("missing or extra native image/layout output")
    seen = set()
    rows = []
    for layout in layouts:
        if layout.is_symlink():
            raise ValueError("symlink layout")
        data = json.loads(layout.read_text())
        name = layout.name.removesuffix(".layout.json")
        shot = name.removesuffix("-dark") if theme == "dark" else name
        if shot not in shots or shot in seen or data.get("theme") != theme:
            raise ValueError("unexpected shot/theme")
        seen.add(shot)
        # Disconnected state correctly has only Detach; require main/dock/editor
        # geometry independently and the remaining button, not Send readiness.
        only_detach = case == "disconnect" or case in KEYBOARD_CASES
        profile_navigation = case == "profile-picker" and shot != "opened"
        smoke.validate_layout(data, fixture=not only_detach and case != "history" and not profile_navigation and not (case.startswith("launcher-") and shot == "launcher"), width=width, height=height)
        if profile_navigation:
            validate_profile_geometry(data,shot,width,height)
        if case == "long-output" or (case == "commands" and shot == "returned"):
            validate_navigation_geometry(data,width,height)
        if case.startswith("launcher-") and shot == "launcher":
            for token in ("sf-dock","ht-launch-controls"):
                if not any(token in str(e.get("path", "")) and e.get("visible") for e in data["elements"]):
                    raise ValueError("missing launcher geometry")
        if case == "history":
            for token in ("sf-dock", "ht-history-controls", "ht-history-button"):
                selected = [e for e in data["elements"] if token in str(e.get("path", "")).split(">")[-1].split(".")]
                if not selected or not all(e.get("visible") and len(e.get("rect", [])) == 4 and
                                          e["rect"][0] >= 0 and e["rect"][1] >= 0 and
                                          e["rect"][2] > 0 and e["rect"][3] > 0 and
                                          e["rect"][0]+e["rect"][2] <= width+0.5 and
                                          e["rect"][1]+e["rect"][3] <= height+0.5 for e in selected):
                    raise ValueError("missing or offscreen history controls")
        if only_detach:
            for token in ("sf-dock", "sf-editor", "ht-controls", "ht-button"):
                if not any(token in str(e.get("path", "")) and e.get("visible") for e in data["elements"]):
                    raise ValueError("missing disconnected control geometry")
        image = layout.with_name(name + ".png")
        if image not in images or image.is_symlink():
            raise ValueError("missing real image")
        with image.open("rb") as stream:
            header = stream.read(24)
        actual_scale = data["viewport"].get("deviceScaleFactor")
        if actual_scale not in (1, 2, 3, 4) or (scale and actual_scale != scale):
            raise ValueError("invalid scale")
        if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR" or struct.unpack(">II", header[16:24]) != (width * actual_scale, height * actual_scale):
            raise ValueError("invalid image/dimensions")
        rows.append({"image": str(image), "layout": str(layout), "sha256": hashlib.sha256(image.read_bytes()).hexdigest(),
                     "layout_sha256": hashlib.sha256(layout.read_bytes()).hexdigest(), "shot": shot})
    return rows


def history_body_ready(shot, body, scroll):
    if shot not in ("first", "last", "back") or any(v.get("ok") is not True or v.get("count") != 1 or len(v.get("nodes", [])) != 1 for v in (body, scroll)):
        return False
    row = 64 if shot == "last" else 0
    expected = json.dumps({"role":"assistant","row_id":row,"text":f"history row {row}"}, separators=(",",":"))
    if body["nodes"][0].get("text") != expected:
        return False
    rects = [v["nodes"][0].get("rect", []) for v in (body, scroll)]
    if any(len(r) != 4 or any(type(n) not in (float, int) or not math.isfinite(n) for n in r) for r in rects):
        return False
    (x,y,w,h),(sx,sy,sw,sh) = rects
    return w > 0 and h > 0 and sw > 0 and sh > 0 and x >= sx and y >= sy and x+w <= sx+sw+0.5 and y+h <= sy+sh+0.5


def history_ready(shot, page, controls, editor):
    if shot not in ("first", "last", "back"):
        return False
    if any(v.get("ok") is not True for v in (page, controls, editor)):
        return False
    expected_page = "Page " + ("3" if shot == "last" else "1") + "/3 · 100 source rows · 65 projected rows"
    expected_controls = ["Previous" if shot == "last" else "Next", "Close"]
    return (page.get("count") == 1 and len(page.get("nodes", [])) == 1 and
            page["nodes"][0].get("text") == expected_page and controls.get("count") == 2 and
            [n.get("text") for n in controls.get("nodes", [])] == expected_controls and
            editor.get("count") == 0 and editor.get("nodes") == [])


def launcher_ready(case, count, controls, editor):
    return (case.startswith("launcher-") and count.get("ok") is True and count.get("count") == 1
            and len(count.get("nodes",[])) == 1 and count["nodes"][0].get("text") == "live · 2 listed · page 1/1"
            and controls.get("ok") is True and controls.get("count") == 5
            and [n.get("text") for n in controls.get("nodes",[])] == ["New conversation","live sessions","stored sessions","Profiles","Close"]
            and editor.get("ok") is True and editor.get("count") == 0 and editor.get("nodes") == [])


def profiles_ready(fields, controls, editor, current, selected):
    def exact(tree,texts):
        return tree.get("ok") is True and tree.get("count")==len(texts) and [n.get("text") for n in tree.get("nodes",[])]==texts
    return (exact(fields,["Current profile: "+current,"2 profiles · page 1/1 · 12 per page",current,selected])
            and exact(controls,["Back to sessions","Close"]) and exact(editor,[]))


def runtime_ready(shot, fields, editor, controls):
    if shot not in ("runtime", "returned"):
        return False
    def exact(tree, texts):
        return (tree.get("ok") is True and tree.get("count") == len(texts)
                and [n.get("text") for n in tree.get("nodes", [])] == texts)
    if shot == "returned":
        return exact(editor,["runtime draft remainsinsert"]) and exact(controls,["Send","Detach"])
    return (exact(fields,["Profile: fixture-profile","Model: fixture-model","Tools: 2 · Skills: 1 · MCP servers: 1"])
            and exact(editor,["runtime draft remainsinspect"]) and exact(controls,["Read delegated agents","Review MCP reload","Show task plan","Return to conversation","Detach"]))


def delegated_ready(body, editor, controls):
    def exact(tree, texts):
        return tree.get("ok") is True and tree.get("count") == len(texts) and [n.get("text") for n in tree.get("nodes", [])] == texts
    return (exact(body,["1 reported delegated agents · snapshot, not independent peers", "Delegated: fx-child",
                        "Goal: Review parser", "Owner-reported data only; snapshot may be stale. No Steer, Stop, transcript or peer controls."])
            and exact(editor,["runtime draft remainsinspect"])
            and exact(controls,["Read delegated agents","Review MCP reload","Show task plan","Return to conversation","Detach"]))


def plan_ready(body, editor, controls):
    def exact(tree, texts):
        return tree.get("ok") is True and tree.get("count") == len(texts) and [n.get("text") for n in tree.get("nodes", [])] == texts
    return (exact(body,["1 reported tasks · revision 2", "pending: 0 · in_progress: 1 · completed: 0 · cancelled: 0",
                        "Task: fx-task", "Status: in_progress", "Check parser",
                        "Reported task status is data, not proof of success or live work. No task edits, approvals or automatic prompts."])
            and exact(editor,["runtime draft remainsinspect"])
            and exact(controls,["Show runtime details","Return to conversation","Detach"]))


def mcp_ready(shot, fields, editor, controls):
    def exact(tree, texts):
        return (tree.get("ok") is True and tree.get("count") == len(texts)
                and [n.get("text") for n in tree.get("nodes", [])] == texts)
    if shot == "returned":
        return exact(editor,["mcp draft remainsinsert"]) and exact(controls,["Send","Detach"])
    if shot == "review":
        return (exact(fields,["MCP reload · profile-wide operation","Profile: fixture-profile",
                    "Owner: fixture-instance · Session: fixture-session",
                    "Reload affects every chat; reattach can refresh policy, adoption and hooks."])
                and exact(editor,["mcp draft remainsinspect"])
                and exact(controls,["Confirm profile reload and reattach","Return to conversation","Detach"]))
    if shot == "reconciled":
        return (exact(fields,["MCP reattach readback complete","Reload report · chat policy: applied (owner-reported)",
                    "Owner reports are not proof of tool health. Reattach reads current owner state, not an exact reload receipt."])
                and exact(editor,["mcp draft remainsinspect"])
                and exact(controls,["Review another reload","Return to conversation","Detach"]))
    return False


def attachment_path_ready(editor, controls, path):
    return (editor.get("ok") is True and editor.get("count")==1 and len(editor.get("nodes",[]))==1
            and editor["nodes"][0].get("text")==path+"inspect" and controls.get("ok") is True
            and controls.get("count")==len(controls.get("nodes",[]))
            and [n.get("text") for n in controls["nodes"]]==["Review local file","Return to conversation","Detach"])


def attachments_ready(shot, body, draft, editor, controls, path):
    def exact(tree,texts):
        return tree.get("ok") is True and tree.get("count")==len(texts) and [n.get("text") for n in tree.get("nodes",[])]==texts
    added="attachment draft remains @file:attachments/review.txt"
    if shot=="returned":
        return exact(editor,[added+" insert"]) and exact(controls,["Send","Detach"])
    if shot not in ("reviewed","staged","added") or not exact(editor,[path+"inspect"]):
        return False
    expected_body="Reviewed: review.txt · 18 bytes" if shot=="reviewed" else "Staged: review.txt · attachments/review.txt"
    expected_controls=["Review local file"]
    if shot!="added":expected_controls.append("Stage reviewed bytes" if shot=="reviewed" else "Add reference to draft")
    expected_controls.extend(["Return to conversation","Detach"])
    return exact(body,[expected_body]) and exact(draft,[added if shot=="added" else "attachment draft remains"]) and exact(controls,expected_controls)


def command_output_visible(body, panel, width, height):
    if any(t.get("ok") is not True or t.get("count") != 1 or len(t.get("nodes",[])) != 1 for t in (body,panel)):
        return False
    rects = [t["nodes"][0].get("rect",[]) for t in (body,panel)]
    if any(len(r)!=4 or any(type(n) not in (int,float) or not math.isfinite(n) for n in r) for r in rects):
        return False
    (x,y,w,h),(px,py,pw,ph) = rects
    return (w>0 and h>0 and pw>0 and ph>0 and px>=0 and py>=0 and
            px+pw<=width+0.5 and py+ph<=height+0.5 and
            x>=px and y>=py and x+w<=px+pw+0.5 and y+h<=py+ph+0.5)


def commands_ready(shot, body, editor, controls):
    if shot not in ("commands","output","tools","returned"):
        return False
    def exact(tree,texts):
        return tree.get("ok") is True and tree.get("count")==len(texts) and [n.get("text") for n in tree.get("nodes",[])]==texts
    if shot == "returned":
        return exact(editor,["command draft remainsinsert"]) and exact(controls,["Send","Detach"])
    if not exact(editor,["command draft remainsinspect"]) or not exact(controls,["Reload command catalog","Attach text file · F5","Return to conversation","Detach"]):
        return False
    if body.get("ok") is not True or body.get("count")!=len(body.get("nodes",[])):
        return False
    texts=[n.get("text") for n in body["nodes"]]
    if shot == "commands":
        return texts==["Owner commands","Read /memory","Read /skills","Read /tools"]
    return texts==[TOOL_CATALOG_TEXT if shot=="tools" else "fixture-owner-memory"]


def plugins_ready(shot, fields, editor, controls):
    def exact(tree, texts):
        return (tree.get("ok") is True and tree.get("count") == len(texts)
                and [node.get("text") for node in tree.get("nodes", [])] == texts)
    if shot == "returned":
        return exact(editor,["plugin draft remainsinsert"]) and exact(controls,["Send","Detach"])
    if not exact(editor,["plugin draft remainsinspect"]) or not exact(controls,
            ["Reload command catalog","Attach text file · F5","Return to conversation","Detach"]):
        return False
    if shot == "plugins":
        return exact(fields,["Plugin profile: fixture-profile", "Plugin owner: fixture-instance",
            "Profile-wide read; initial discovery may load/register plugins and refresh secret sources.", "Read /plugins"])
    if shot == "output":
        return exact(fields,["Owner plugins · 1 plugins · 0 toolsets · 0 portable MCP Report only, not health or execution permission. Plugin: fx [fx] v1 enabled; tools=1 hooks=0 commands=0; load error=no"])
    return False


def models_ready(shot, body, editor, controls):
    if shot not in ("models","selected","applied","returned"):
        return False
    for tree in (editor,controls):
        if tree.get("ok") is not True or tree.get("count") != len(tree.get("nodes",[])):
            return False
    if [n.get("text") for n in editor["nodes"]] != ["model draft remains" + ("insert" if shot == "returned" else "inspect")]:
        return False
    buttons = [n.get("text") for n in controls["nodes"]]
    if shot == "returned":
        return buttons == ["Send","Detach"]
    if body.get("ok") is not True or body.get("count") != len(body.get("nodes",[])):
        return False
    texts = [n.get("text","") for n in body["nodes"]]
    route = "Active route: " + ("model-exact · provider-exact" if shot == "applied" else "old-model · old-provider")
    if route not in texts or "Return to conversation" not in buttons or "Detach" not in buttons:
        return False
    if shot == "models":
        return "Select model-exact" in texts and "Apply to this session" not in buttons
    if shot == "selected":
        return "Apply to this session" in buttons
    return any(text.startswith("Model control reconciled with owner snapshot") for text in texts) and "Apply to this session" not in buttons


def ready_projection(case, shot, editor, controls):
    if editor.get("ok") is not True or controls.get("ok") is not True:
        return False
    if editor.get("count") != 1 or len(editor.get("nodes", [])) != 1:
        return False
    node = editor["nodes"][0]
    # Tern tree reports placeholder text for an empty editor, qualified by the
    # actual `empty` class; it does not expose an empty string value here.
    # Nonempty tree text includes the current insert-mode badge as well.
    if case == "disconnect" and node.get("text") != "retained after disconnectinsert":
        return False
    if case != "disconnect" and "empty" not in node.get("class", "").split():
        return False
    expected = ["Detach"] if case == "disconnect" else ["Interrupt turn" if shot == "waiting" else "Send", "Detach"]
    return controls.get("count") == len(expected) and [n.get("text") for n in controls.get("nodes", [])] == expected


def long_output_ready(shot, body, buttons):
    if shot not in ("folded", "expanded", "refolded"):
        return False
    for tree in (body, buttons):
        if tree.get("ok") is not True or tree.get("count") != 1 or len(tree.get("nodes", [])) != 1:
            return False
    expected = LONG_TEXT if shot == "expanded" else "".join(LONG_TEXT.splitlines(keepends=True)[:12])
    label = "Collapse retained text" if shot == "expanded" else "Show retained text"
    # Real Tern 0.9.5 tree collapses whitespace; compare its measured projection,
    # not raw newlines. No substring/prefix pass can hide a missing tail.
    return body["nodes"][0].get("text") == " ".join(expected.split()) and buttons["nodes"][0].get("text") == label


def motion_choices_ready(tree, reduced):
    rows = tree.get("nodes", [])
    return (tree.get("ok") is True and tree.get("count") == 3 and len(rows) == 3
            and [r.get("text") for r in rows] == ["System", "On", "Off"]
            and ["on" in r.get("class", "").split() for r in rows] == [False, reduced, not reduced])


def keyboard_ready(case, shot, editor, controls, body, toggle, surface):
    if case not in KEYBOARD_CASES or shot not in ("selected", "expanded", "refolded"):
        return False
    for tree in (editor, controls, surface, toggle):
        if tree.get("ok") is not True or tree.get("count") != 1 or len(tree.get("nodes", [])) != 1:
            return False
    # Pinned real tree includes the mode badge; still compare the whole value,
    # never a prefix that could hide a missing final draft character.
    return (editor["nodes"][0].get("text") == KEYBOARD_DRAFT + "inspect"
            and controls["nodes"][0].get("text") == "Detach"
            and "ht-keyboard-focus" in toggle["nodes"][0].get("class", "").split()
            and "ht-keyboard-ready" in toggle["nodes"][0].get("class", "").split()
            and ("sf-still" in surface["nodes"][0].get("class", "").split()) == (case == "keyboard-reduced")
            and long_output_ready("folded" if shot == "selected" else shot, body, toggle))


def keyboard_composer_ready(editor, controls):
    return (editor.get("ok") is True and editor.get("count") == 1 and len(editor.get("nodes", [])) == 1
            and editor["nodes"][0].get("text") == KEYBOARD_DRAFT + "insert"
            and "focused" in editor["nodes"][0].get("class", "").split()
            and controls.get("ok") is True and controls.get("count") == 2
            and [n.get("text") for n in controls.get("nodes", [])] == ["Send", "Detach"])


def client_exit_status(path):
    """Closed bounded diagnostic independent of the terminal's exit marker."""
    try:
        fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    except FileNotFoundError:
        return {"status": "not_observed"}
    except OSError:
        return {"status": "invalid"}
    try:
        meta = os.fstat(fd)
        if not stat.S_ISREG(meta.st_mode) or not 2 <= meta.st_size <= 4:
            return {"status": "invalid"}
        raw = os.read(fd, 5)
        if not raw.endswith(b"\n") or not raw[:-1].isdigit():
            return {"status": "invalid"}
        code = int(raw[:-1])
        if code > 255 or raw != f"{code}\n".encode():
            return {"status": "invalid"}
        return {"status": "exited", "exit_code": code}
    except OSError:
        return {"status": "invalid"}
    finally:
        os.close(fd)


def drive_control(endpoint, text, case, theme, size, record, profiles=None):
    """Bounded documented Tern ctl protocol; not an application renderer."""
    width, height, _ = smoke.parse_size(size)
    deadline = time.monotonic() + 55
    record.update(calls=[], calls_total=0, calls_omitted=0)
    def call(line):
        started = time.monotonic()
        remaining = deadline - time.monotonic()
        verb = line.split(" ", 1)[0]
        command = verb if verb in {"size", "appearance", "prefs", "tree", "click", "key", "plugins",
                                   "shot", "run", "expect", "ready", "quit", "type"} else "unknown"
        row = {"command": command, "phase": "budget", "status": "pending",
               "remaining_seconds": round(max(0, remaining), 3)}
        record["calls_total"] += 1
        if len(record["calls"]) == 128:
            record["calls"].pop(0)
            record["calls_omitted"] += 1
        record["calls"].append(row)
        try:
            if remaining <= 0:
                raise TimeoutError("native control deadline")
            with socket.socket(socket.AF_UNIX) as sock:
                sock.settimeout(min(22, remaining))
                row["phase"] = "connect"
                sock.connect(str(endpoint))
                row["phase"] = "send"
                sock.sendall((line + "\n").encode())
                data = bytearray()
                row["phase"] = "receive"
                while not data.endswith(b"\n"):
                    chunk = sock.recv(4096)
                    if not chunk:
                        raise ValueError("Tern control closed before reply")
                    data.extend(chunk)
                    if len(data) > 262144:
                        raise ValueError("control reply limit exceeded")
                row["phase"] = "decode"
                value = json.loads(data)
                row["phase"] = "validate"
                if not isinstance(value, dict) or value.get("ok") is not True:
                    raise ValueError("native control refused")
            row.update(phase="complete", status="passed")
            return value
        except (OSError, ValueError, AssertionError) as error:
            row.update(status="failed", error_class=type(error).__name__)
            record.setdefault("failed_call", row)
            raise
        finally:
            row["elapsed_seconds"] = round(time.monotonic() - started, 3)
    try:
        while not endpoint.exists():
            if time.monotonic() >= deadline:
                raise TimeoutError("native control endpoint missing")
            time.sleep(0.02)
        width, height, scale = smoke.parse_size(size)
        call(f"size {width} {height} {scale or 2}")
        call("appearance " + theme)
        record["steps"] = []
        record["projections"] = []
        if case in KEYBOARD_CASES:
            # Only this server's isolated settings: require exact labels before clicking.
            call('prefs search "Reduce Motion"')
            choices = call("tree .pf-seg>button")
            if choices.get("ok") is not True or choices.get("count") != 3 or [n.get("text") for n in choices.get("nodes", [])] != ["System", "On", "Off"]:
                raise ValueError("ambiguous reduced-motion preference")
            reduced = case == "keyboard-reduced"
            call('click ".pf-seg>button:nth-of-type(' + ("2" if reduced else "3") + ')"')
            choices = call("tree .pf-seg>button")
            if not motion_choices_ready(choices, reduced):
                raise ValueError("motion preference not applied")
            record["motion_preference"] = choices
            call("prefs close")
        for line in text.splitlines():
            if case=="attachments" and line=='click ".ht-attachment-review"':
                # Path edits rotate callback IDs. Wait for the exact path and its
                # current button to be painted, then click once; never retry a read.
                path=str(Path(endpoint).parent / "review.txt")
                while True:
                    editor=call("tree .sf-editor")
                    controls=call("tree .ht-controls>button")
                    record["path_readiness"]={"editor":editor,"controls":controls}
                    if attachment_path_ready(editor,controls,path):
                        break
                    time.sleep(0.02)
            if line.startswith("shot "):
                shot = line.removeprefix("shot ")
                while True:
                    editor = call("tree .sf-editor")
                    if case == "profile-picker" and shot != "opened":
                        if profiles is None:
                            raise ValueError("profile identities unavailable")
                        current,selected = profiles
                        if shot == "profiles":
                            fields = call("tree [data-id='main.current'],[data-id='main.count'],.ht-profile-list [data-id$='.home']")
                            controls = call("tree .ht-profile-controls>button")
                            ready = profiles_ready(fields,controls,editor,current,selected)
                        else:
                            fields = call("tree [data-id='main.profile']")
                            count = call("tree [data-id='main.count']")
                            controls = call("tree .ht-launch-controls>button")
                            ready = (launcher_ready("launcher-close",count,controls,editor) and
                                fields.get("ok") is True and fields.get("count")==1 and
                                [n.get("text") for n in fields.get("nodes",[])]==["Qualified profile: "+selected])
                        record["last_readiness"] = {"shot":shot,"fields":fields,"controls":controls,"editor":editor}
                        if ready:
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case.startswith("launcher-") and shot == "launcher":
                        count = call("tree [data-id='main.count']")
                        controls = call("tree .ht-launch-controls>button")
                        record["last_readiness"] = {"shot":shot,"count":count,"controls":controls,"editor":editor}
                        if launcher_ready(case,count,controls,editor):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "history":
                        page = call("tree .sf-dock>.sf-text:first-child")
                        controls = call("tree .ht-history-controls>button")
                        body = call("tree .ht-history-scroll>.sf-col:first-child>.ht-history-body")
                        scroll = call("tree .ht-history-scroll")
                        record["last_readiness"] = {"shot": shot, "page": page, "controls": controls, "editor": editor, "body": body, "scroll": scroll}
                        if history_ready(shot, page, controls, editor) and history_body_ready(shot, body, scroll):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    controls = call("tree .ht-controls>button")
                    if case == "plugins":
                        fields = (call("tree [data-id='main.commands.plugin-profile'],[data-id='main.commands.plugin-owner'],[data-id='main.commands.plugin-disclosure'],.ht-command-read")
                                  if shot == "plugins" else call("tree [data-id='main.commands.output']") if shot == "output" else {})
                        panel = call("tree .ht-runtime-panel") if shot != "returned" else {}
                        record["last_readiness"] = {"shot":shot,"fields":fields,"editor":editor,"controls":controls,"panel":panel}
                        if plugins_ready(shot,fields,editor,controls) and (shot == "returned" or
                                all(command_output_visible({"ok":True,"count":1,"nodes":[node]},panel,width,height) for node in fields["nodes"])):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "mcp-reload":
                        selector = ("tree [data-id='main.mcp.heading'],[data-id='main.mcp.profile'],[data-id='main.mcp.owner'],[data-id='main.mcp.consent']"
                                    if shot == "review" else "tree [data-id='main.mcp.complete'],[data-id='main.mcp.reported'],[data-id='main.mcp.report-scope']")
                        fields = call(selector) if shot != "returned" else {}
                        panel = call("tree .ht-mcp-body") if shot != "returned" else {}
                        record["last_readiness"]={"shot":shot,"fields":fields,"editor":editor,"controls":controls,"panel":panel}
                        if mcp_ready(shot,fields,editor,controls) and (shot == "returned" or
                                all(command_output_visible({"ok":True,"count":1,"nodes":[node]},panel,width,height) for node in fields["nodes"])):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "attachments":
                        body=call("tree [data-id='main.attachment.review']" if shot=="reviewed" else "tree [data-id='main.attachment.receipt']") if shot!="returned" else {}
                        draft=call("tree [data-id='main.attachment.draft']") if shot!="returned" else {}
                        path=str(Path(endpoint).parent / "review.txt")
                        panel=call("tree .ht-attachment-panel") if shot!="returned" else {}
                        record["last_readiness"]={"shot":shot,"body":body,"draft":draft,"editor":editor,"controls":controls,"panel":panel}
                        if attachments_ready(shot,body,draft,editor,controls,path) and (shot=="returned" or
                            (command_output_visible(body,panel,width,height) and command_output_visible(draft,panel,width,height))):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "commands":
                        body = (call("tree [data-id='main.commands.heading'],.ht-command-read") if shot=="commands" else
                                call("tree [data-id='main.commands.output']") if shot in ("output","tools") else {})
                        panel = call("tree .ht-runtime-panel") if shot in ("output","tools") else {}
                        record["last_readiness"]={"shot":shot,"body":body,"editor":editor,"controls":controls,"panel":panel}
                        if commands_ready(shot,body,editor,controls) and (shot not in ("output","tools") or command_output_visible(body,panel,width,height)):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "model-picker":
                        body = call("tree .ht-runtime-panel .sf-text,.ht-model-select") if shot != "returned" else {}
                        record["last_readiness"] = {"shot":shot,"body":body,"editor":editor,"controls":controls}
                        if models_ready(shot,body,editor,controls):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    if case == "runtime-details":
                        if shot == "plan":
                            fields = call("tree [data-id='main.runtime.plan.count'],[data-id='main.runtime.plan.totals'],[data-id='main.runtime.plan.row.line-0'],[data-id='main.runtime.plan.row.line-1'],[data-id='main.runtime.plan.row.line-3'],[data-id='main.runtime.plan.scope']")
                            panel = call("tree .ht-runtime-panel")
                            record["last_readiness"] = {"shot":shot,"fields":fields,"editor":editor,"controls":controls,"panel":panel}
                            if plan_ready(fields,editor,controls) and all(command_output_visible({"ok":True,"count":1,"nodes":[node]}, panel, width, height) for node in fields["nodes"]):
                                record["projections"].append(record["last_readiness"])
                                break
                            time.sleep(0.02)
                            continue
                        if shot == "delegated":
                            fields = call("tree [data-id='main.runtime.delegated.count'],[data-id='main.runtime.delegated.row.field-0'],[data-id='main.runtime.delegated.row.field-2'],[data-id='main.runtime.delegated.scope']")
                            panel = call("tree .ht-runtime-panel")
                            record["last_readiness"] = {"shot":shot,"fields":fields,"editor":editor,"controls":controls,"panel":panel}
                            if delegated_ready(fields,editor,controls) and all(command_output_visible({"ok":True,"count":1,"nodes":[node]}, panel, width, height) for node in fields["nodes"]):
                                record["projections"].append(record["last_readiness"])
                                break
                            time.sleep(0.02)
                            continue
                        fields = call("tree [data-id='main.runtime.profile'],[data-id='main.runtime.model'],[data-id='main.runtime.counts']") if shot == "runtime" else {}
                        record["last_readiness"] = {"shot":shot,"fields":fields,"editor":editor,"controls":controls}
                        if runtime_ready(shot,fields,editor,controls):
                            record["projections"].append(record["last_readiness"])
                            break
                        time.sleep(0.02)
                        continue
                    extra = {}
                    text_ready = True
                    if case == "long-output" or case in KEYBOARD_CASES:
                        extra = {"body": call("tree .sf-main>.sf-col>.sf-text:last-child"),
                                 "toggle": call("tree .ht-transcript-toggle")}
                        text_ready = long_output_ready(shot, extra["body"], extra["toggle"])
                    if case in KEYBOARD_CASES:
                        extra["surface"] = call("tree .sf-main")
                        ready = keyboard_ready(case, shot, editor, controls, extra["body"], extra["toggle"], extra["surface"])
                    else:
                        ready = ready_projection(case, shot, editor, controls) and text_ready
                    record["last_readiness"] = {"shot": shot, "editor": editor, "controls": controls, **extra}
                    if ready:
                        record["projections"].append({"shot": shot, "editor": editor, "controls": controls, **extra})
                        break
                    time.sleep(0.02)
            if case in KEYBOARD_CASES and line.startswith('click ".ht-controls'):
                # Escape must return to composer before interpreting the exit selector.
                while True:
                    editor = call("tree .sf-editor")
                    controls = call("tree .ht-controls>button")
                    record["last_composer_readiness"] = {"editor": editor, "controls": controls}
                    if keyboard_composer_ready(editor, controls):
                        record["composer_restored"] = {"editor": editor, "controls": controls}
                        break
                    time.sleep(0.02)
            record["steps"].append({"command": line.split(" ", 1)[0], "reply": call(line)})
        record["status"] = "passed"
    except (OSError, ValueError, AssertionError) as error:
        record.update(status="failed", error=str(error))
    finally:
        try:
            call("quit")
        except (OSError, ValueError):
            record["quit_unconfirmed"] = True


def run_case(case, theme, size, base):
    scratch = Path(tempfile.mkdtemp(prefix="ht-nc-", dir=base))
    result = {"case": case, "theme": theme, "size": size, "root": str(scratch), "status": "failed",
              "scope": "production_client_real_Tern_synthetic_gateway", "pixel_review": "not_performed"}
    gateway = None
    other = None
    try:
        env = smoke.isolated_environment(scratch, os.environ)
        if case == "attachments":
            (scratch / "review.txt").write_bytes(b"native attachment\n")
        gateway = ScenarioGateway(case)
        profile_homes = None
        if case == "profile-picker":
            other = ScenarioGateway("launcher-new")
            selected = scratch / "selected-profile"
            selected.mkdir(mode=0o700)
            gateway.profile_id = env["HERMES_HOME"]
            other.profile_id = str(selected)
            other.instance_id = "selected-fixture-instance"
            gateway.catalog = other.catalog
            gateway.profiles = {"profiles":[{"name":"default","path":gateway.profile_id}, {"name":"selected","path":other.profile_id}]}
            profile_homes = (gateway.profile_id,other.profile_id)
        grant = {"url": f"ws://127.0.0.1:{gateway.port}/api/ws", "protocols": ["hermes-gateway-v1", "hermes-gateway-ticket.synthetic-fixture"],
                 "profile_id": gateway.profile_id, "instance_id": gateway.instance_id}
        runtime = scratch / "rt"
        bootstrap = runtime / "ui-tui/scripts/gateway_bootstrap.py"
        bootstrap.parent.mkdir(mode=0o700, parents=True)
        bootstrap.write_text("def bootstrap(start,recover):\n    assert (start,recover)==(False,False)\n    return " + repr(grant) + "\n")
        if other is not None:
            assert profile_homes is not None
            target_grant = {**grant,"url":f"ws://127.0.0.1:{other.port}/api/ws","profile_id":other.profile_id,"instance_id":other.instance_id}
            bootstrap.write_text("import os\ndef bootstrap(start,recover):\n    assert (start,recover)==(False,False)\n"
                f"    grants={dict(zip(profile_homes,(grant,target_grant)))!r}\n    return grants[os.environ['HERMES_HOME']]\n")
        binary = ROOT / "target/debug/hermes-tern"
        result["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        argv = [str(binary), "chat", "--hermes-root", str(runtime), "--python", sys.executable, "--home", env["HERMES_HOME"], "--cwd", str(scratch)]
        if case.startswith("launcher-") or case == "profile-picker":
            argv[1] = "open"
        if case == "history":
            argv = [str(binary), "history", "--session", "fixture-session", "--hermes-root", str(runtime), "--python", sys.executable, "--home", env["HERMES_HOME"]]
        text, shots = scenario(case, argv, scratch / "client.stderr")
        script = scratch / (case + ".txt")
        script.write_text(text)
        # Real work and editor echo are asynchronous to scenario simulated time.
        # A control endpoint permits exact text/value readiness before each shot.
        endpoint = scratch / "ctl.sock"
        control = {"status": "failed"}
        driver = threading.Thread(target=drive_control, args=(endpoint, text, case, theme, size, control, profile_homes))
        command = ["tern", "serve", "--control", str(endpoint), "--out", str(scratch / "shots")]
        driver.start()
        try:
            run = runner.run_command(command, ROOT, env, scratch / "renderer.log", timeout=60)
        finally:
            driver.join(timeout=60)
        if driver.is_alive():
            raise ValueError("native control thread did not finish")
        result["control"] = control
        result["renderer"] = run
        result["client_exit"] = client_exit_status(scratch / "client.exit")
        if run["status"] != "passed" or control["status"] != "passed":
            raise ValueError("renderer failed: " + run["status"])
        if result["client_exit"] != {"status": "exited", "exit_code": 0}:
            raise ValueError("zero client exit not confirmed by sidecar")
        result["request_counts"] = (validate_profile_requests(gateway.requests,other.requests,gateway.errors+other.errors)
                                    if other is not None else validate_requests(case, gateway.requests, gateway.errors))
        result["outputs"] = validate_outputs(scratch / "shots", case, shots, theme, size)
        if hashlib.sha256(binary.read_bytes()).hexdigest() != result["binary_sha256"]:
            raise ValueError("binary changed during native run")
        result["status"] = "passed"
    except (OSError, ValueError, AssertionError) as error:
        result["error"] = str(error)
    finally:
        result.setdefault("client_exit", client_exit_status(scratch / "client.exit"))
        if other is not None:
            try:
                other.close()
            except AssertionError as error:
                result.update(status="failed", error=str(error))
            result["selected_requests"] = other.requests
            result["selected_gateway_errors"] = other.errors
            result["selected_gateway_thread_joined"] = not other.thread.is_alive()
        if gateway:
            try:
                gateway.close()
            except AssertionError as error:
                result.update(status="failed", error=str(error))
            result["requests"] = gateway.requests
            result["gateway_errors"] = gateway.errors
            result["gateway_thread_joined"] = not gateway.thread.is_alive()
        (scratch / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        owned = scratch / "ownership.json"
        owned.touch(mode=0o600)
        owned.write_text(json.dumps({"retention": "retain;no deletion", "paths": smoke.inventory(scratch)}, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, help="new absolute result directory")
    parser.add_argument("--case", choices=CASES, help="one case; default all")
    parser.add_argument("--theme", choices=("light", "dark", "both"), default="both")
    parser.add_argument("--size", help="one bounded viewport; default wide and narrow")
    args = parser.parse_args()
    if args.size:
        smoke.parse_size(args.size)
    base = Path(os.environ.get("TMPDIR", str(Path.home() / ".cache/hermes-tern/tmp")))
    base.mkdir(parents=True, exist_ok=True)
    out = runner.prepare_output(args.out) if args.out else Path(tempfile.mkdtemp(prefix="ht-ncm-", dir=base))
    print("Native chat retained outputs: " + str(out), flush=True)
    result = {"status": "failed", "scope": "real_Tern_production_client_synthetic_gateway_not_live_Hermes", "cases": [],
              "pixel_review": "not_performed_by_runner", "inference": False, "window_plugin": False}
    try:
        if not shutil.which("tern"):
            raise ValueError("Tern unavailable; native acceptance not established")
        # Keep actual Unix socket roots short even when aggregate --out is nested.
        short = Path.home() / ".cache/ht-native"
        short.mkdir(mode=0o700, parents=True, exist_ok=True)
        cases = (args.case,) if args.case else CASES
        themes = ("light", "dark") if args.theme == "both" else (args.theme,)
        sizes = (args.size,) if args.size else ("1100x760", "480x760")
        result["expected_cases"] = len(cases) * len(themes) * len(sizes)
        for case in cases:
            for size in sizes:
                for theme in themes:
                    row = run_case(case, theme, size, short)
                    result["cases"].append(row)
                    print(case + " " + size + " " + theme + ": " + row["status"], flush=True)
                    if row["status"] != "passed":
                        raise ValueError("native case failed; inspect retained case result")
        result["status"] = "passed"
    except (OSError, ValueError) as error:
        result["error"] = str(error)
    finally:
        (out / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        owned = out / "ownership.json"
        owned.touch(mode=0o600)
        owned.write_text(json.dumps({"retention": "retain;no deletion", "paths": smoke.inventory(out),
                                    "case_ownership": [r["root"] + "/ownership.json" for r in result["cases"]]}, indent=2) + "\n")
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
