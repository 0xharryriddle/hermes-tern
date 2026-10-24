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
import struct
import sys
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
CASES = ("conversation", "deny", "skip", "interrupt", "disconnect", "long-output", "keyboard-output", "keyboard-reduced", "history")
KEYBOARD_CASES = ("keyboard-output", "keyboard-reduced")
KEYBOARD_DRAFT = "keyboard draft remains"
# Keep normalized text below the measured Tern tree text cap while exceeding
# the production twelve-line fold boundary. Full raw text is checked by PTY.
LONG_TEXT = "".join(f"row{i:02d}\n" for i in range(1, 24)) + "fixture retained tail"


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
                         accept_timeout=60, read_timeout=60, history=history)

    def reply(self, request, result):
        method = request["method"]
        if method == "session.create":
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
    shell = shlex.join(command) + " 2>" + shlex.quote(str(stderr)) + '; rc=$?; printf "HT_CLIENT_EXIT_%s\\n" "$rc"'
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


def validate_requests(case, rows, errors):
    if case not in CASES or errors or not isinstance(rows, list):
        raise ValueError("invalid fixture evidence")
    if case == "history":
        if [r.get("method") for r in rows] != ["ping", "runtime.describe", "session.history"] or [r.get("params") for r in rows] != [{}, {}, {"session_id": "fixture-session"}]:
            raise ValueError("history must only qualify and read once; no resume or mutation")
        return {"session.history": 1}
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
        smoke.validate_layout(data, fixture=not only_detach and case != "history", width=width, height=height)
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


def drive_control(endpoint, text, case, theme, size, record):
    """Bounded documented Tern ctl protocol; not an application renderer."""
    deadline = time.monotonic() + 55
    def call(line):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("native control deadline")
        with socket.socket(socket.AF_UNIX) as sock:
            sock.settimeout(min(22, remaining))
            sock.connect(str(endpoint))
            sock.sendall((line + "\n").encode())
            data = bytearray()
            while not data.endswith(b"\n"):
                chunk = sock.recv(4096)
                if not chunk:
                    raise ValueError("Tern control closed before reply")
                data.extend(chunk)
                if len(data) > 262144:
                    raise ValueError("control reply limit exceeded")
            value = json.loads(data)
            if value.get("ok") is not True:
                raise ValueError("native control refused: " + line.split(" ", 1)[0])
            return value
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
            if line.startswith("shot "):
                shot = line.removeprefix("shot ")
                while True:
                    editor = call("tree .sf-editor")
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
    try:
        env = smoke.isolated_environment(scratch, os.environ)
        gateway = ScenarioGateway(case)
        grant = {"url": f"ws://127.0.0.1:{gateway.port}/api/ws", "protocols": ["hermes-gateway-v1", "hermes-gateway-ticket.synthetic-fixture"],
                 "profile_id": "fixture-profile", "instance_id": "fixture-instance"}
        runtime = scratch / "rt"
        bootstrap = runtime / "ui-tui/scripts/gateway_bootstrap.py"
        bootstrap.parent.mkdir(mode=0o700, parents=True)
        bootstrap.write_text("def bootstrap(start,recover):\n    assert (start,recover)==(False,False)\n    return " + repr(grant) + "\n")
        binary = ROOT / "target/debug/hermes-tern"
        result["binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
        argv = [str(binary), "chat", "--hermes-root", str(runtime), "--python", sys.executable, "--home", env["HERMES_HOME"], "--cwd", str(scratch)]
        if case == "history":
            argv = [str(binary), "history", "--session", "fixture-session", "--hermes-root", str(runtime), "--python", sys.executable, "--home", env["HERMES_HOME"]]
        text, shots = scenario(case, argv, scratch / "client.stderr")
        script = scratch / (case + ".txt")
        script.write_text(text)
        # Real work and editor echo are asynchronous to scenario simulated time.
        # A control endpoint permits exact text/value readiness before each shot.
        endpoint = scratch / "ctl.sock"
        control = {"status": "failed"}
        driver = threading.Thread(target=drive_control, args=(endpoint, text, case, theme, size, control))
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
        if run["status"] != "passed" or control["status"] != "passed":
            raise ValueError("renderer failed: " + run["status"])
        result["request_counts"] = validate_requests(case, gateway.requests, gateway.errors)
        result["outputs"] = validate_outputs(scratch / "shots", case, shots, theme, size)
        if hashlib.sha256(binary.read_bytes()).hexdigest() != result["binary_sha256"]:
            raise ValueError("binary changed during native run")
        result["status"] = "passed"
    except (OSError, ValueError, AssertionError) as error:
        result["error"] = str(error)
    finally:
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
