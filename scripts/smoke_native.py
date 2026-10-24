"""Optional isolated Tern rendering check. Never loads personal plugins or agents.

Run after cargo build --workspace --bins --locked. Requires the Tern binary.
Outputs remain in a private scratch directory for inspection; no global install.
"""
import argparse
import json
import math
import os
from pathlib import Path
import re
import shlex
import shutil
import stat
import struct
import subprocess
import tempfile


def parse_size(value):
    match = re.fullmatch(r"([0-9]+)x([0-9]+)(?:@([1-4]))?", value)
    if not match:
        raise ValueError("size must be WIDTHxHEIGHT with optional @1..4")
    width, height = int(match[1]), int(match[2])
    if not 320 <= width <= 4096 or not 320 <= height <= 2160:
        raise ValueError("viewport outside bounded smoke dimensions")
    return width, height, int(match[3]) if match[3] else None


def scenario(root, fixture=False):
    binary = root / "target/debug" / ("hermes-tern-testing" if fixture else "hermes-tern")
    command = shlex.join([str(binary)] + ([] if fixture else ["preview"]))
    # In Tern 0.9.5 `plugins expect` finds main-region content, not dock-only text.
    marker = "Hermes / FIXTURE" if fixture else "No agent is connected"
    shot = "fixture" if fixture else "preview"
    return binary, "ready\nrun " + json.dumps(command) + "\nplugins expect " + json.dumps(marker) + "\nshot " + shot + "\n", shot


def isolated_environment(scratch, parent):
    # Allowlist rather than carrying provider credentials, personal session metadata,
    # shell hooks, preload libraries or Python modules into the renderer's shell.
    env = {k: parent[k] for k in ("PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TERM") if k in parent}
    for key, name in {"HOME": "h", "TERN_CONFIG_DIR": "c", "XDG_CONFIG_HOME": "x",
                      "XDG_STATE_HOME": "s", "XDG_CACHE_HOME": "a", "STENCIL_LOG_DIR": "l",
                      "XDG_RUNTIME_DIR": "r", "HERMES_HOME": "hh"}.items():
        directory = scratch / name
        directory.mkdir(mode=0o700)
        env[key] = str(directory)
    env["TERN_DAEMON_SOCKET"] = str(scratch / "d.sock")
    env["TMPDIR"] = str(scratch)
    return env


def validate_layout(data, fixture, width, height):
    if not isinstance(data, dict) or not isinstance(data.get("viewport"), dict):
        raise ValueError("missing viewport")
    viewport = data["viewport"]
    if viewport.get("width") != width or viewport.get("height") != height:
        raise ValueError("wrong viewport")
    elements = data.get("elements")
    if not isinstance(elements, list) or not elements or not all(isinstance(e, dict) for e in elements):
        raise ValueError("missing layout elements")

    def inside(element):
        rect = element.get("rect")
        if element.get("visible") is not True or not isinstance(rect, list) or len(rect) != 4:
            return False
        if any(type(v) not in (float, int) or not math.isfinite(v) for v in rect):
            return False
        x, y, w, h = rect
        return x >= 0 and y >= 0 and w > 0 and h > 0 and x + w <= width + 0.5 and y + h <= height + 0.5

    def selected(token):
        return [e for e in elements if token in str(e.get("path", "")).split(">")[-1].split(".")]

    required = ["sf-main"] + (["sf-dock", "sf-editor", "ht-controls"] if fixture else [])
    for token in required:
        matches = selected(token)
        if len(matches) != 1 or not inside(matches[0]):
            raise ValueError("missing, ambiguous or offscreen native " + token)
    if fixture:
        buttons = [e for e in selected("ht-button") if "sf-dock" in str(e.get("path", ""))]
        if len(buttons) < 2 or not all(inside(e) for e in buttons):
            raise ValueError("missing or offscreen fixture controls")
    return {token: selected(token)[0]["rect"] for token in required}


def render(command, env, root):
    return subprocess.run(command, env=env, cwd=root, check=True, capture_output=True, text=True, timeout=90)


def inventory(scratch):
    rows = []
    for base, dirs, files in os.walk(scratch, followlinks=False):
        for name in [""] + dirs + files:
            path = Path(base) / name
            metadata = path.lstat()
            rows.append({"path": str(path), "dev": metadata.st_dev, "ino": metadata.st_ino,
                         "mode": stat.S_IMODE(metadata.st_mode)})
    return list({row["path"]: row for row in rows}.values())


def run(root, scratch, fixture=False, size="1100x760"):
    result = {"status": "failed", "fixture": fixture, "size": size,
              "scope": "native_render_only_not_window_plugin_or_live_agent", "pixel_review": "not_performed"}
    try:
        width, height, scale = parse_size(size)
        binary, text, shot = scenario(root, fixture)
        if not binary.is_file():
            raise ValueError("build first: cargo build --workspace --bins --locked")
        env = isolated_environment(scratch, os.environ)
        source = scratch / (shot + ".txt")
        source.write_text(text)
        command = ["tern", "shot", str(source), "--out", str(scratch / "shots"),
                   "--size", size, "--theme", "both"]
        result["command"] = command
        result["binary"] = str(binary)
        completed = render(command, env, root)
        (scratch / "renderer.log").write_text(completed.stdout + completed.stderr)
        layouts = sorted((scratch / "shots").rglob("*.layout.json"))
        images = sorted((scratch / "shots").rglob("*.png"))
        if len(layouts) != 2 or len(images) != 2:
            raise ValueError("expected exactly light and dark layout/image pairs")
        checked, themes = [], set()
        for layout in layouts:
            image = layout.with_name(layout.name.removesuffix(".layout.json") + ".png")
            if image not in images or layout.is_symlink() or image.is_symlink():
                raise ValueError("missing real image/layout pair")
            data = json.loads(layout.read_text())
            boxes = validate_layout(data, fixture, width, height)
            theme = data.get("theme")
            if theme not in ("light", "dark") or theme in themes:
                raise ValueError("duplicate or unknown theme")
            themes.add(theme)
            actual_scale = data["viewport"].get("deviceScaleFactor")
            if actual_scale not in (1, 2, 3, 4) or (scale is not None and actual_scale != scale):
                raise ValueError("unexpected device scale")
            with image.open("rb") as stream:
                header = stream.read(24)
            if len(header) != 24 or header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR":
                raise ValueError("invalid PNG header")
            if struct.unpack(">II", header[16:24]) != (int(width * actual_scale), int(height * actual_scale)):
                raise ValueError("PNG dimensions differ from layout")
            checked.append({"layout": str(layout), "image": str(image), "theme": theme, "boxes": boxes})
        result.update(status="passed", exit_code=0, outputs=checked)
        return result
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        result["error_class"] = type(error).__name__
        if isinstance(error, (subprocess.CalledProcessError, subprocess.TimeoutExpired)):
            if isinstance(error, subprocess.CalledProcessError):
                result["exit_code"] = error.returncode
            parts = [error.stdout or "", error.stderr or ""]
            diagnostic = "".join(p.decode("utf-8", errors="replace") if isinstance(p, bytes) else p for p in parts)
            (scratch / "renderer.log").write_text(diagnostic)
        raise
    finally:
        (scratch / "result.json").write_text(json.dumps(result, indent=2) + "\n")
        owned = scratch / "ownership.json"
        owned.touch(mode=0o600, exist_ok=True)
        owned.write_text(json.dumps({"retention": "retain; no deletion", "paths": inventory(scratch)}, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", action="store_true", help="render the labelled session fixture, never a live agent")
    parser.add_argument("--size", default="1100x760", help="bounded viewport WIDTHxHEIGHT, optional @1..4")
    args = parser.parse_args()
    try:
        parse_size(args.size)
    except ValueError as error:
        parser.error(str(error))
    if not shutil.which("tern"):
        raise SystemExit("Tern binary unavailable; native verification NOT ESTABLISHED")
    root = Path(__file__).resolve().parents[1]
    base = Path(os.environ.get("TMPDIR", str(Path.home() / ".cache/hermes-tern/tmp")))
    base.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="ht-n-", dir=base))
    print(f"Native smoke retained outputs: {scratch}", flush=True)
    run(root, scratch, args.fixture, args.size)
    print("Native content/render smoke passed; not a window-plugin or live-agent acceptance.")
    print("Pixel-level visual review and interactive actions are separate checks.")


if __name__ == "__main__":
    main()
