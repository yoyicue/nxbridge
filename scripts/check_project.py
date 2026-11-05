#!/usr/bin/env python3
"""Offline checks for documentation and planning metadata, not product tests."""
from __future__ import annotations

import ast
import json
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
ERRORS: list[str] = []


def require(condition: bool, message: str) -> None:
    if not condition:
        ERRORS.append(message)


def read_json(relative: str) -> dict:
    try:
        data = json.loads((ROOT / relative).read_text(encoding="utf-8"))
        require(isinstance(data, dict), f"{relative}: expected an object")
        if not isinstance(data, dict):
            return {}
        require(data.get("schema_version") == 1, f"{relative}: unsupported schema_version")
        return data
    except (OSError, ValueError) as exc:
        ERRORS.append(f"{relative}: {exc}")
        return {}


def local_file(value: str, context: str) -> None:
    path = (ROOT / value).resolve()
    require(path.is_relative_to(ROOT), f"{context}: evidence must stay in the repository: {value}")
    require(path.is_file(), f"{context}: missing evidence file: {value}")


def check_links() -> int:
    paths = [ROOT / "README.md", ROOT / "AGENTS.md"]
    paths += sorted((ROOT / "docs").rglob("*.md"))
    paths += sorted((ROOT / "templates").rglob("*.md"))
    for path in paths:
        if not path.is_file():
            ERRORS.append(f"Missing document: {path.relative_to(ROOT)}")
            continue
        text = path.read_text(encoding="utf-8")
        text = re.sub(r"(?ms)^```.*?^```[^\n]*$", "", text)
        for value in re.findall(r"\[[^\]\n]*\]\(([^\n)]+)\)", text):
            value = value.strip().strip("<>")
            parsed = urlsplit(value)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            target = (path.parent / unquote(parsed.path)).resolve()
            require(target.is_relative_to(ROOT), f"{path.relative_to(ROOT)}: non-portable local link {value}")
            require(target.exists(), f"{path.relative_to(ROOT)}: broken link {value}")
    return len(paths)


def check_tasks(data: dict) -> int:
    tasks = data.get("tasks", [])
    require(isinstance(tasks, list) and bool(tasks), "backlog: tasks must be a non-empty list")
    if not isinstance(tasks, list):
        return 0
    by_id = {}
    active = []
    statuses = {"todo", "in_progress", "blocked", "done", "skipped"}
    for task in tasks:
        if not isinstance(task, dict):
            ERRORS.append("backlog: invalid task object")
            continue
        task_id = task.get("id", "")
        require(bool(re.fullmatch(r"NXB-\d{3}", task_id)), f"backlog: invalid ID {task_id!r}")
        require(task_id not in by_id, f"backlog: duplicate ID {task_id}")
        by_id[task_id] = task
        require(bool(task.get("title")), f"{task_id}: missing title")
        require(task.get("status") in statuses, f"{task_id}: invalid status")
        require(bool(task.get("acceptance")), f"{task_id}: missing acceptance criteria")
        require(isinstance(task.get("depends_on"), list), f"{task_id}: depends_on must be a list")
        require(isinstance(task.get("evidence"), list), f"{task_id}: evidence must be a list")
        if task.get("status") == "in_progress":
            active.append(task_id)
            require(bool(task.get("owner")), f"{task_id}: active task needs an owner")
        if task.get("status") == "blocked":
            require(bool(task.get("blockers")), f"{task_id}: blocked task needs a reason")
        if task.get("status") == "done":
            require(bool(task.get("evidence")), f"{task_id}: completed task needs evidence")
        for evidence in task.get("evidence", []):
            local_file(evidence, task_id)
    require(len(active) <= 1, "backlog: this baseline permits one active task")
    for task_id, task in by_id.items():
        for dependency in task.get("depends_on", []):
            require(dependency in by_id, f"{task_id}: unknown dependency {dependency}")
            if task.get("status") in {"in_progress", "done"} and dependency in by_id:
                require(by_id[dependency].get("status") == "done", f"{task_id}: dependency {dependency} not done")
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(task_id: str) -> None:
        if task_id in visiting:
            ERRORS.append(f"backlog: dependency cycle at {task_id}")
            return
        if task_id in visited or task_id not in by_id:
            return
        visiting.add(task_id)
        for dependency in by_id[task_id].get("depends_on", []):
            visit(dependency)
        visiting.remove(task_id)
        visited.add(task_id)

    for task_id in by_id:
        visit(task_id)
    return len(by_id)


def main() -> int:
    required = ["README.md", "AGENTS.md", "Makefile", ".gitignore", "docs/design.md",
                "docs/architecture.md", "docs/sessions.md", "docs/input-bridge.md",
                "docs/validation.md", "docs/agent-workflow.md", "docs/status.md",
                "docs/references.md", "docs/decisions/0001-native-sessions.md",
                "templates/task-report.md", "scripts/preflight.py"]
    for relative in required:
        require((ROOT / relative).is_file(), f"Missing file: {relative}")
    document_count = check_links()
    task_count = check_tasks(read_json("tasks/backlog.json"))
    upstreams = read_json("config/upstreams.json")
    require(upstreams.get("lock_status") == "research_snapshot_not_build_lock",
            "upstreams: keep research snapshot distinct from a verified build lock")
    for component in upstreams.get("components", []):
        require(bool(re.fullmatch(r"[0-9a-f]{40}", component.get("revision", ""))),
                f"upstreams: missing exact revision for {component.get('id')}")
        require(component.get("repository", "").startswith("https://"), "upstreams: invalid repository URL")
    config = read_json("config/local.example.json")
    require(config.get("adb_serial") is None, "example config must not contain a real device serial")
    sessions = config.get("sessions", {})
    initial = sessions.get("initial_session_limit")
    target = sessions.get("target_concurrent_players")
    require(isinstance(initial, int) and isinstance(target, int) and 1 <= initial <= target,
            "example config: invalid initial session limit / target")
    measured = sessions.get("measured_session_capacity")
    require(measured is None, "example config: do not invent measured capacity")
    profiles = config.get("profiles", [])
    ids = [profile.get("id") for profile in profiles]
    require(len(ids) == len(set(ids)), "example config: duplicate profiles")
    experiment = read_json("templates/experiment.json")
    require(experiment.get("result") == "not_run", "experiment template must not report a measurement")
    for session in experiment.get("sessions", []):
        require(session.get("valid_sample") is None, "experiment template must not claim a valid sample")
        require(all(value is None for value in session.get("metrics", {}).values()),
                "experiment template must have null metric values")
    for path in (ROOT / "scripts").glob("*.py"):
        try:
            ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        except (OSError, SyntaxError) as exc:
            ERRORS.append(str(exc))
    if ERRORS:
        print("Project metadata checks failed:", file=sys.stderr)
        for message in ERRORS:
            print(f"- {message}", file=sys.stderr)
        return 1
    print(f"PASS: {document_count} Markdown files, {task_count} tasks, JSON metadata and Python syntax")
    print("This checks project documentation; runtime and performance remain unverified.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
