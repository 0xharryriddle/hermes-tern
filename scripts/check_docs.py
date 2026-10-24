"""Check local Markdown navigation and task references, not prose correctness.

Supported links: inline Markdown links outside fenced code. No network calls.
Author external factual claims and review semantics separately from this gate.
"""
from __future__ import annotations

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

EXCLUDED = {".git", "target", ".local", ".hermes", "__pycache__"}
STATUSES = {"planned", "blocked", "in_progress", "done", "deferred"}
LINK = re.compile(r"!?\[[^\]\n]*\]\(([^\s)]+)(?:\s+\"[^\"]*\")?\)")


def markdown_files(root: Path) -> list[Path]:
    # Never recurse into dependencies/build trees.
    result = []
    for directory, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in EXCLUDED)
        result.extend(Path(directory) / f for f in sorted(files) if f.endswith(".md"))
    return result


def prose(text: str) -> str:
    lines = []
    fence = None
    for line in text.splitlines():
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker.group(1)
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            continue
        if fence is None:
            lines.append(line)
    return "\n".join(lines)


def anchors(text: str) -> set[str]:
    found = set()
    duplicates: Counter[str] = Counter()
    for line in prose(text).splitlines():
        heading = re.match(r"^#{1,6}\s+(.+?)(?:\s+#+)?$", line)
        if not heading:
            continue
        slug = re.sub(r"[^\w\- ]", "", heading.group(1).lower()).replace(" ", "-")
        n = duplicates[slug]
        duplicates[slug] += 1
        found.add(f"{slug}-{n}" if n else slug)
    return found


def validate_tasks(root: Path) -> list[str]:
    errors = []
    try:
        data = json.loads((root / "docs/tasks.json").read_text(encoding="utf-8"))
        if not isinstance(data, dict) or data.get("schema_version") != 1:
            return ["tasks.json: unsupported schema_version"]
        tasks = data["tasks"]
        requirements = data["requirements"]
        if not isinstance(tasks, list) or not tasks or not isinstance(requirements, list) or not requirements:
            return ["tasks.json: non-empty tasks and requirements arrays required"]
        task_ids = [t["id"] for t in tasks]
        req_ids = [r["id"] for r in requirements]
        if any(not isinstance(i, str) or not i for i in task_ids + req_ids):
            return ["tasks.json: ids must be non-empty strings"]
        for kind, ids in (("task", task_ids), ("requirement", req_ids)):
            for ident, count in Counter(ids).items():
                if count != 1:
                    errors.append(f"duplicate {kind}: {ident}")
        mapped = set()
        graph = {}
        for task in tasks:
            ident = task["id"]
            if task.get("status") not in STATUSES:
                errors.append(f"invalid status: {ident}")
            for key in ("depends_on", "requirements", "acceptance", "evidence", "blockers"):
                if not isinstance(task.get(key), list) or any(not isinstance(x, str) or not x.strip() for x in task.get(key, [])):
                    return errors + [f"tasks.json: {ident}.{key} must be a string array"]
            if not task["acceptance"]:
                errors.append(f"empty acceptance: {ident}")
            if not task["requirements"]:
                errors.append(f"empty requirements: {ident}")
            if task["status"] == "done" and not task["evidence"]:
                errors.append(f"done without evidence: {ident}")
            if task["status"] == "blocked" and not task["blockers"]:
                errors.append(f"blocked without reason: {ident}")
            for dep in task["depends_on"]:
                if dep not in task_ids:
                    errors.append(f"unknown dependency: {ident} -> {dep}")
            for req in task["requirements"]:
                if req not in req_ids:
                    errors.append(f"unknown requirement: {ident} -> {req}")
                mapped.add(req)
            graph[ident] = task["depends_on"]
        for req in req_ids:
            if req not in mapped:
                errors.append(f"unmapped requirement: {req}")
        visiting, visited = set(), set()

        def walk(ident: str) -> None:
            if ident in visiting:
                errors.append(f"dependency cycle: {ident}")
                return
            if ident in visited or ident not in graph:
                return
            visiting.add(ident)
            for dep in graph[ident]:
                walk(dep)
            visiting.remove(ident)
            visited.add(ident)

        for ident in graph:
            walk(ident)
    except (OSError, ValueError, KeyError, TypeError) as error:
        errors.append(f"tasks.json: {error}")
    return errors


def validate(root: Path) -> list[str]:
    root = root.resolve()
    errors = validate_tasks(root)
    docs = markdown_files(root)
    texts = {p.resolve(): p.read_text(encoding="utf-8") for p in docs}
    graph: dict[Path, set[Path]] = {p: set() for p in texts}
    for path, text in texts.items():
        for target in LINK.findall(prose(text)):
            parsed = urlsplit(target.strip("<>"))
            if parsed.scheme or parsed.netloc:
                continue
            destination = (path.parent / unquote(parsed.path)).resolve() if parsed.path else path
            if not destination.is_relative_to(root):
                errors.append(f"link escapes repo: {path.relative_to(root)} -> {target}")
                continue
            if not destination.exists():
                errors.append(f"missing link: {path.relative_to(root)} -> {target}")
                continue
            if destination in texts:
                graph[path].add(destination)
                if parsed.fragment and unquote(parsed.fragment) not in anchors(texts[destination]):
                    errors.append(f"missing fragment: {path.relative_to(root)} -> {target}")
    entry = root / "docs/README.md"
    reachable, pending = set(), [entry]
    while pending:
        current = pending.pop()
        if current in reachable:
            continue
        reachable.add(current)
        pending.extend(graph.get(current, set()) - reachable)
    for path in texts:
        if path.is_relative_to(root / "docs") and path not in reachable:
            errors.append(f"unreachable document: {path.relative_to(root)}")
    if entry not in texts:
        errors.append("missing docs/README.md")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    errors = validate(args.root)
    if errors:
        for error in errors:
            print(f"FAIL: {error}")
        return 1
    data = json.loads((args.root / "docs/tasks.json").read_text(encoding="utf-8"))
    print(f"Docs integrity: {len(markdown_files(args.root))} Markdown files; "
          f"{len(data['tasks'])} tasks; {len(data['requirements'])} mapped requirements; "
          "local links/fragments/navigation and task DAG valid.")
    print("Structural result only; external facts and runtime acceptance require separate evidence.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
