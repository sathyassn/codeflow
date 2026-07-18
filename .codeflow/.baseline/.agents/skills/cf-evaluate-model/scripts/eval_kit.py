#!/usr/bin/env python3
"""Validate, materialize, score, compare, and clean CodeFlow model evaluations."""

from __future__ import annotations

import argparse
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import stat
import subprocess
import sys
from typing import Any


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_DIR = SCRIPT_DIR.parent
RESOURCE_DIR = SKILL_DIR / "resources"
RUN_MARKER = ".codeflow-eval-run.json"
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
KNOWN_VALIDITY_FLAGS = {
    "ambiguous_task",
    "baseline_contamination",
    "budget_exhaustion",
    "broken_fixture",
    "evaluation_awareness",
    "grader_false_negative",
    "grader_false_positive",
    "grader_material_exposed",
    "harness_context_mismatch",
    "missing_trace",
    "refusal",
    "reward_hacking",
    "retry_contamination",
    "reused_session",
    "sandbagging",
    "unavailable_fixture_tool",
    "unresolved_grader_disagreement",
}
EVALUATION_ROUTE_PREFIX = "| Qualify a model or harness change |"
SAFE_BRANCH = re.compile(r"^(?:main|master|fixture/[a-z0-9][a-z0-9-]*)$")
GRADER_MATERIAL_DIRS = (
    ".agents/skills/cf-evaluate-model",
    ".claude/skills/cf-evaluate-model",
    ".codeflow/.baseline/.agents/skills/cf-evaluate-model",
    ".codeflow/.baseline/.claude/skills/cf-evaluate-model",
)
GRADER_ROUTE_FILES = ("AGENTS.md", ".codeflow/.baseline/AGENTS.md")


class EvalError(ValueError):
    """A user-addressable evaluation-contract error."""


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise EvalError(f"cannot read {path}: {error}") from error


def write_json(path: Path, value: Any) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


def canonical_digest(value: Any) -> str:
    encoded = json.dumps(
        value, ensure_ascii=False, separators=(",", ":"), sort_keys=True
    ).encode("utf-8")
    return "sha256:" + hashlib.sha256(encoded).hexdigest()


def normalized(value: str) -> str:
    return " ".join(value.split())


def project_root(start: Path | None = None) -> Path:
    candidate = (start or Path.cwd()).resolve()
    for current in (candidate, *candidate.parents):
        if (current / "AGENTS.md").is_file() and (
            (current / ".agents/skills/cf-model-orchestrator/SKILL.md").is_file()
            or (current / ".claude/skills/cf-model-orchestrator/SKILL.md").is_file()
        ):
            return current
    raise EvalError("run from a CodeFlow standard/full project root")


def suite_documents(resource_dir: Path = RESOURCE_DIR) -> tuple[dict, dict, dict]:
    requirements = load_json(resource_dir / "requirements.json")
    cases = load_json(resource_dir / "cases.json")
    fixtures = load_json(resource_dir / "fixtures.json")
    if not all(isinstance(item, dict) for item in (requirements, cases, fixtures)):
        raise EvalError("requirements, cases, and fixtures must be JSON objects")
    return requirements, cases, fixtures


def suite_digest(resource_dir: Path = RESOURCE_DIR) -> str:
    requirements, cases, fixtures = suite_documents(resource_dir)
    return canonical_digest(
        {"requirements": requirements, "cases": cases, "fixtures": fixtures}
    )


def string_list(value: Any, label: str, *, allow_empty: bool = False) -> list[str]:
    if not isinstance(value, list) or any(
        not isinstance(item, str) or not item.strip() for item in value
    ):
        raise EvalError(f"{label} must be an array of nonempty strings")
    if not allow_empty and not value:
        raise EvalError(f"{label} must not be empty")
    return value


def unique_objects(items: Any, label: str) -> dict[str, dict]:
    if not isinstance(items, list):
        raise EvalError(f"{label} must be an array")
    indexed: dict[str, dict] = {}
    for index, item in enumerate(items):
        if not isinstance(item, dict) or not isinstance(item.get("id"), str):
            raise EvalError(f"{label}[{index}] requires a string id")
        item_id = item["id"]
        if not item_id or item_id in indexed:
            raise EvalError(f"{label} contains duplicate or empty id {item_id!r}")
        indexed[item_id] = item
    return indexed


def validate_suite(root: Path, resource_dir: Path = RESOURCE_DIR) -> list[str]:
    errors: list[str] = []
    try:
        requirements_doc, cases_doc, fixtures_doc = suite_documents(resource_dir)
        if requirements_doc.get("schema_version") != 1:
            errors.append("requirements schema_version must be 1")
        if cases_doc.get("schema_version") != 1:
            errors.append("cases schema_version must be 1")
        if fixtures_doc.get("schema_version") != 1:
            errors.append("fixtures schema_version must be 1")
        requirements = unique_objects(
            requirements_doc.get("requirements"), "requirements"
        )
        cases = unique_objects(cases_doc.get("cases"), "cases")
        fixtures = unique_objects(fixtures_doc.get("fixtures"), "fixtures")
    except EvalError as error:
        return [str(error)]

    covered_requirements: set[str] = set()
    used_fixtures: set[str] = set()
    categories: set[str] = set()
    canary_requirements: set[str] = set()
    for requirement_id, requirement in requirements.items():
        if requirement.get("level") not in {"hard", "preference"}:
            errors.append(f"{requirement_id}: invalid level")
        if not isinstance(requirement.get("statement"), str) or not requirement[
            "statement"
        ].strip():
            errors.append(f"{requirement_id}: missing statement")
        sources = requirement.get("sources")
        if not isinstance(sources, list) or not sources:
            errors.append(f"{requirement_id}: requires at least one source")
            continue
        for source_index, source in enumerate(sources):
            if not isinstance(source, dict) or not isinstance(source.get("path"), str):
                errors.append(f"{requirement_id}.sources[{source_index}]: invalid source")
                continue
            source_path = root / source["path"]
            if not source_path.is_file():
                errors.append(f"{requirement_id}: source missing: {source['path']}")
                continue
            try:
                markers = string_list(
                    source.get("markers"), f"{requirement_id}.sources.markers"
                )
            except EvalError as error:
                errors.append(str(error))
                continue
            content = normalized(source_path.read_text(encoding="utf-8"))
            for marker in markers:
                if normalized(marker) not in content:
                    errors.append(
                        f"{requirement_id}: marker missing from {source['path']}: {marker}"
                    )

    for case_id, case in cases.items():
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", case_id):
            errors.append(f"{case_id}: unsafe case id")
        if case.get("kind") not in {"regression", "capability"}:
            errors.append(f"{case_id}: invalid kind")
        category = case.get("category")
        if not isinstance(category, str) or not category:
            errors.append(f"{case_id}: missing category")
        else:
            categories.add(category)
        if not isinstance(case.get("prompt"), str) or not case["prompt"].strip():
            errors.append(f"{case_id}: missing prompt")
        fixture_id = case.get("fixture")
        if fixture_id not in fixtures:
            errors.append(f"{case_id}: unknown fixture {fixture_id!r}")
        else:
            used_fixtures.add(fixture_id)
        try:
            linked = string_list(case.get("requirements"), f"{case_id}.requirements")
        except EvalError as error:
            errors.append(str(error))
            linked = []
        for requirement_id in linked:
            if requirement_id not in requirements:
                errors.append(f"{case_id}: unknown requirement {requirement_id}")
            else:
                covered_requirements.add(requirement_id)
                if case.get("canary") is True:
                    canary_requirements.add(requirement_id)
        expected = case.get("expected")
        if not isinstance(expected, dict):
            errors.append(f"{case_id}: expected must be an object")
            continue
        for field in ("routes", "signals", "references", "must_not"):
            try:
                string_list(
                    expected.get(field),
                    f"{case_id}.expected.{field}",
                    allow_empty=field == "references",
                )
            except EvalError as error:
                errors.append(str(error))
        if not isinstance(case.get("canary"), bool):
            errors.append(f"{case_id}: canary must be boolean")

    for fixture_id, fixture in fixtures.items():
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", fixture_id):
            errors.append(f"{fixture_id}: unsafe fixture id")
        if fixture.get("tier") not in {"standard", "full"}:
            errors.append(f"{fixture_id}: invalid tier")
        state = fixture.get("state")
        if not isinstance(state, dict):
            errors.append(f"{fixture_id}: state must be an object")
        else:
            branch = state.get("branch", "fixture/base")
            if not isinstance(branch, str) or not SAFE_BRANCH.fullmatch(branch):
                errors.append(f"{fixture_id}: unsafe branch {branch!r}")
        files = fixture.get("files")
        if not isinstance(files, dict) or not files:
            errors.append(f"{fixture_id}: files must be a nonempty object")
            continue
        for rel_path, content in files.items():
            try:
                safe_path = safe_relative_path(rel_path).as_posix()
            except EvalError as error:
                errors.append(f"{fixture_id}: {error}")
                safe_path = ""
            if not isinstance(content, str):
                errors.append(f"{fixture_id}: {rel_path} content must be a string")
                continue
            if any(
                safe_path == grader_dir or safe_path.startswith(grader_dir + "/")
                for grader_dir in GRADER_MATERIAL_DIRS
            ):
                errors.append(f"{fixture_id}: overlay restores grader material: {rel_path}")
            if safe_path in GRADER_ROUTE_FILES and EVALUATION_ROUTE_PREFIX in content:
                errors.append(f"{fixture_id}: overlay restores evaluation route: {rel_path}")

    hard = {
        requirement_id
        for requirement_id, requirement in requirements.items()
        if requirement.get("level") == "hard"
    }
    for requirement_id in sorted(hard - covered_requirements):
        errors.append(f"hard requirement has no behavioral case: {requirement_id}")
    critical_canary = {"CF-MM-002", "CF-QA-001", "CF-SEC-001", "CF-EVAL-001"}
    for requirement_id in sorted(critical_canary - canary_requirements):
        errors.append(f"critical requirement missing from canary: {requirement_id}")
    unused = set(fixtures) - used_fixtures
    if unused:
        errors.append(f"unused fixtures: {', '.join(sorted(unused))}")
    for required_category in {
        "routing",
        "orchestration",
        "design",
        "governance",
        "verification",
        "security",
        "parallelism",
        "references",
        "outputs",
        "shipping",
        "artifact_quality",
    }:
        if required_category not in categories:
            errors.append(f"missing evaluation category: {required_category}")
    if not isinstance(cases_doc.get("full_trials"), int) or cases_doc["full_trials"] < 3:
        errors.append("full_trials must be an integer of at least 3")
    return errors


def safe_relative_path(value: str) -> Path:
    path = Path(value)
    if not value or path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts):
        raise EvalError(f"unsafe relative fixture path: {value!r}")
    return path


def refuse_symlink_components(path: Path, stop: Path) -> None:
    current = path
    while current != stop and current != current.parent:
        if current.is_symlink():
            raise EvalError(f"refusing symlink path component: {current}")
        current = current.parent


def run_command(args: list[str], cwd: Path) -> None:
    completed = subprocess.run(
        args,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=180,
        check=False,
    )
    if completed.returncode != 0:
        tail = "\n".join(completed.stdout.splitlines()[-30:])
        raise EvalError(f"command failed ({' '.join(args)}):\n{tail}")


def ensure_run_root(run_root: Path) -> dict:
    raw = run_root.expanduser()
    if raw.is_symlink():
        raise EvalError("evaluation run root must not be a symlink")
    run_root = raw.resolve()
    if run_root == Path(run_root.anchor) or run_root == Path.home().resolve():
        raise EvalError("refusing root or home directory as an evaluation run root")
    marker_path = run_root / RUN_MARKER
    if run_root.exists():
        if not marker_path.is_file():
            raise EvalError("existing run root lacks the CodeFlow evaluation marker")
        marker = load_json(marker_path)
        if not isinstance(marker, dict) or marker.get("schema_version") != 1:
            raise EvalError("invalid evaluation run marker")
        if marker.get("suite_digest") != suite_digest():
            raise EvalError("evaluation run marker belongs to a different suite revision")
        return marker
    run_root.mkdir(parents=True)
    marker = {
        "schema_version": 1,
        "run_id": datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        + "-"
        + secrets.token_hex(4),
        "suite_digest": suite_digest(),
        "created_at": datetime.now(timezone.utc).isoformat(),
    }
    write_json(marker_path, marker)
    return marker


def remove_grader_material(fixture_root: Path) -> None:
    for relative in GRADER_MATERIAL_DIRS:
        target = fixture_root / relative
        if target.is_symlink():
            raise EvalError(f"refusing grader-material symlink: {target}")
        if target.exists():
            shutil.rmtree(target)
    for relative in GRADER_ROUTE_FILES:
        target = fixture_root / relative
        if not target.is_file():
            continue
        lines = target.read_text(encoding="utf-8").splitlines(keepends=True)
        scrubbed = [
            line for line in lines if not line.startswith(EVALUATION_ROUTE_PREFIX)
        ]
        if len(scrubbed) != len(lines):
            target.write_text("".join(scrubbed), encoding="utf-8")
    manifest_path = fixture_root / ".codeflow/manifest.json"
    if manifest_path.is_file():
        manifest = load_json(manifest_path)
        files = manifest.get("files") if isinstance(manifest, dict) else None
        if isinstance(files, dict):
            manifest["files"] = {
                path: record
                for path, record in files.items()
                if "/cf-evaluate-model/" not in f"/{path}"
            }
            baseline_agents = fixture_root / ".codeflow/.baseline/AGENTS.md"
            agents_record = manifest["files"].get("AGENTS.md")
            if isinstance(agents_record, dict) and baseline_agents.is_file():
                agents_record["sha256"] = hashlib.sha256(
                    baseline_agents.read_bytes()
                ).hexdigest()
            write_json(manifest_path, manifest)
    assert_no_grader_material(fixture_root)


def assert_no_grader_material(fixture_root: Path) -> None:
    """Fail when a subject fixture exposes evaluator-only material."""

    manifest_path = fixture_root / ".codeflow/manifest.json"
    for relative in GRADER_MATERIAL_DIRS:
        target = fixture_root / relative
        if target.is_symlink() or target.exists():
            raise EvalError(f"grader material survived scrub: {relative}")
    for relative in GRADER_ROUTE_FILES:
        target = fixture_root / relative
        if target.is_file() and EVALUATION_ROUTE_PREFIX in target.read_text(
            encoding="utf-8"
        ):
            raise EvalError(f"evaluation routing row survived scrub: {relative}")
    if manifest_path.is_file():
        scrubbed_manifest = load_json(manifest_path)
        files = (
            scrubbed_manifest.get("files")
            if isinstance(scrubbed_manifest, dict)
            else None
        )
        if isinstance(files, dict) and any(
            "/cf-evaluate-model/" in f"/{path}" for path in files
        ):
            raise EvalError("evaluation skill entry survived manifest scrub")


def write_fixture_file(root: Path, relative: str, content: str) -> None:
    rel = safe_relative_path(relative)
    target = root / rel
    refuse_symlink_components(target.parent, root)
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.is_symlink():
        raise EvalError(f"refusing fixture symlink target: {relative}")
    target.write_text(content, encoding="utf-8")


def reset_fixture_history(root: Path, branch: str) -> None:
    if not SAFE_BRANCH.fullmatch(branch):
        raise EvalError(f"unsafe fixture branch: {branch!r}")
    git_dir = root / ".git"
    if git_dir.is_symlink():
        raise EvalError("refusing symlinked fixture .git")
    if git_dir.exists():
        shutil.rmtree(git_dir)
    run_command(["git", "init", "-b", branch], root)
    run_command(["git", "config", "user.name", "CodeFlow Eval"], root)
    run_command(["git", "config", "user.email", "eval@codeflow.invalid"], root)
    run_command(["git", "add", "-A"], root)
    run_command(["git", "commit", "-m", "chore: materialize evaluation fixture"], root)
    if (root / ".codeflow/git-hooks").is_dir():
        run_command(["git", "config", "core.hooksPath", ".codeflow/git-hooks"], root)


def tree_digest(root: Path) -> str:
    digest = hashlib.sha256()
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        for dirname in dirnames:
            path = Path(dirpath) / dirname
            if path.is_symlink():
                raise EvalError(f"fixture tree contains symlink: {path}")
        dirnames[:] = sorted(name for name in dirnames if name != ".git")
        for filename in sorted(filenames):
            path = Path(dirpath) / filename
            if path.is_symlink():
                raise EvalError(f"fixture tree contains symlink: {path}")
            relative = path.relative_to(root).as_posix()
            executable = bool(
                path.stat().st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
            )
            digest.update(relative.encode("utf-8") + b"\0")
            digest.update((b"x" if executable else b"-") + b"\0")
            digest.update(path.read_bytes() + b"\0")
    return "sha256:" + digest.hexdigest()


def materialize(case_id: str, trial: int, run_root: Path, codeflow: Path) -> dict:
    if trial < 1:
        raise EvalError("trial must be at least 1")
    root = project_root()
    suite_errors = validate_suite(root)
    if suite_errors:
        raise EvalError("suite invalid:\n- " + "\n- ".join(suite_errors))
    _, cases_doc, fixtures_doc = suite_documents()
    cases = unique_objects(cases_doc["cases"], "cases")
    fixtures = unique_objects(fixtures_doc["fixtures"], "fixtures")
    if case_id not in cases:
        raise EvalError(f"unknown case: {case_id}")
    case = cases[case_id]
    fixture_id = case.get("fixture")
    fixture = fixtures.get(fixture_id)
    if fixture is None:
        raise EvalError(f"case {case_id} refers to unknown fixture {fixture_id!r}")
    marker = ensure_run_root(run_root)
    resolved_run_root = run_root.expanduser().resolve()
    opaque_id = hashlib.sha256(
        f"{marker['run_id']}\0{case_id}\0{trial}".encode("utf-8")
    ).hexdigest()[:20]
    output = resolved_run_root / "workspaces" / opaque_id / "repository"
    record_path = resolved_run_root / "records" / f"{opaque_id}.fixture.json"
    refuse_symlink_components(output.parent, resolved_run_root)
    if output.exists():
        raise EvalError(f"trial fixture already exists: {output}")
    if record_path.exists():
        raise EvalError(f"trial record already exists: {record_path}")
    output.mkdir(parents=True)
    codeflow_path = codeflow.expanduser().resolve()
    if not codeflow_path.is_file() or not os.access(codeflow_path, os.X_OK):
        raise EvalError(f"CodeFlow binary is not executable: {codeflow_path}")
    tier_flag = "--full" if fixture["tier"] == "full" else "--standard"
    run_command([str(codeflow_path), "init", "--yes", tier_flag], output)
    remove_grader_material(output)
    for relative, content in fixture["files"].items():
        write_fixture_file(output, relative, content)
    write_fixture_file(output, "TASK.md", case["prompt"].rstrip() + "\n")
    assert_no_grader_material(output)
    branch = fixture["state"].get("branch", "fixture/base")
    reset_fixture_history(output, branch)
    digest = tree_digest(output)
    trial_record = {
        "schema_version": 1,
        "run_id": marker["run_id"],
        "case_id": case_id,
        "trial": trial,
        "fixture_id": fixture["id"],
        "fixture_state": fixture["state"],
        "fixture_digest": digest,
        "path": str(output),
    }
    record_path.parent.mkdir(parents=True, exist_ok=True)
    write_json(record_path, trial_record)
    return trial_record


def computed_trial_status(trial: dict, case: dict) -> str:
    outcome = trial.get("outcome")
    if outcome in {"error", "not_run"}:
        return outcome
    if outcome != "completed":
        return "error"
    observed = trial.get("observed")
    if not isinstance(observed, dict):
        return "fail"
    route = observed.get("route")
    signals = (
        set(observed.get("signals", []))
        if isinstance(observed.get("signals"), list)
        else set()
    )
    violations = (
        set(observed.get("violations", []))
        if isinstance(observed.get("violations"), list)
        else set()
    )
    references = (
        set(observed.get("references", []))
        if isinstance(observed.get("references"), list)
        else set()
    )
    expected = case["expected"]
    prohibited = set(expected["must_not"])
    evidence = trial.get("evidence")
    valid_evidence = isinstance(evidence, list) and bool(evidence) and all(
        isinstance(item, dict)
        and item.get("kind") in {"session", "tool", "file", "command", "ui"}
        and isinstance(item.get("ref"), str)
        and bool(item["ref"])
        and isinstance(item.get("digest"), str)
        and bool(DIGEST.fullmatch(item["digest"]))
        for item in evidence
    )
    flags = (
        set(trial.get("validity_flags", []))
        if isinstance(trial.get("validity_flags"), list)
        else {"invalid_validity_flags"}
    )
    passed = (
        route in expected["routes"]
        and set(expected["signals"]) <= signals
        and set(expected["references"]) <= references
        and not (prohibited & signals)
        and not violations
        and valid_evidence
        and isinstance(trial.get("trace_ref"), str)
        and bool(trial["trace_ref"])
        and not flags
    )
    return "pass" if passed else "fail"


def expected_trial_pairs(suite: str, cases_doc: dict) -> set[tuple[str, int]]:
    cases = cases_doc["cases"]
    selected = cases if suite == "full" else [case for case in cases if case["canary"]]
    repetitions = cases_doc["full_trials"] if suite == "full" else 1
    return {(case["id"], trial) for case in selected for trial in range(1, repetitions + 1)}


def expected_summary(trials: list[dict], cases: dict[str, dict]) -> dict:
    statuses = Counter(trial["status"] for trial in trials)
    categories: dict[str, Counter[str]] = defaultdict(Counter)
    per_case: dict[str, list[str]] = defaultdict(list)
    for trial in trials:
        categories[cases[trial["case_id"]]["category"]][trial["status"]] += 1
        per_case[trial["case_id"]].append(trial["status"])
    total = len(trials)
    return {
        "total": total,
        "pass": statuses["pass"],
        "fail": statuses["fail"],
        "error": statuses["error"],
        "not_run": statuses["not_run"],
        "pass_rate": statuses["pass"] / total if total else 0.0,
        "consistent_case_rate": (
            sum(all(status == "pass" for status in values) for values in per_case.values())
            / len(per_case)
            if per_case
            else 0.0
        ),
        "categories": {
            category: {
                status: counts[status]
                for status in ("pass", "fail", "error", "not_run")
            }
            for category, counts in sorted(categories.items())
        },
    }


def validate_result(result: Any, *, require_approval: bool = False) -> list[str]:
    errors: list[str] = []
    if not isinstance(result, dict):
        return ["result must be an object"]
    _, cases_doc, _ = suite_documents()
    cases = unique_objects(cases_doc["cases"], "cases")
    if result.get("schema_version") != 1:
        errors.append("result.schema_version must be 1")
    if not isinstance(result.get("run_id"), str) or not result["run_id"].strip():
        errors.append("result.run_id must be a nonempty string")
    suite = result.get("suite")
    if suite not in {"canary", "full"}:
        errors.append("result.suite must be canary or full")
        return errors
    if require_approval and suite != "full":
        errors.append("promotion validation requires the full suite")
    if result.get("suite_digest") != suite_digest():
        errors.append("result.suite_digest does not match the current suite")
    system = result.get("system")
    required_system = {
        "model",
        "effort",
        "harness",
        "harness_version",
        "codeflow_revision",
        "settings_digest",
        "permission_profile",
        "tools",
        "network",
        "budget",
    }
    if not isinstance(system, dict) or not required_system <= set(system):
        errors.append("result.system is missing required harness metadata")
    else:
        for field in (
            "model",
            "effort",
            "harness_version",
            "codeflow_revision",
            "permission_profile",
            "network",
        ):
            if not isinstance(system.get(field), str) or not system[field].strip():
                errors.append(f"result.system.{field} must be a nonempty string")
        if system.get("harness") not in {
            "codex-app",
            "codex-cli",
            "claude-code",
        }:
            errors.append("result.system.harness is not a native interactive harness")
        settings_digest = system.get("settings_digest")
        if not isinstance(settings_digest, str) or not DIGEST.fullmatch(
            settings_digest
        ):
            errors.append("result.system.settings_digest must be canonical sha256")
        try:
            string_list(system.get("tools"), "result.system.tools")
        except EvalError as error:
            errors.append(str(error))
        budget = system.get("budget")
        if not isinstance(budget, dict):
            errors.append("result.system.budget must be an object")
        else:
            for field in ("turns", "wall_seconds", "retries"):
                value = budget.get(field)
                if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                    errors.append(
                        f"result.system.budget.{field} must be a nonnegative integer"
                    )
            token_budget = budget.get("tokens")
            if token_budget is not None and (
                not isinstance(token_budget, int)
                or isinstance(token_budget, bool)
                or token_budget < 0
            ):
                errors.append(
                    "result.system.budget.tokens must be null or a nonnegative integer"
                )
    trials = result.get("trials")
    if not isinstance(trials, list):
        return errors + ["result.trials must be an array"]
    seen: set[tuple[str, int]] = set()
    for index, trial in enumerate(trials):
        label = f"trials[{index}]"
        if not isinstance(trial, dict):
            errors.append(f"{label} must be an object")
            continue
        case_id = trial.get("case_id")
        number = trial.get("trial")
        if case_id not in cases:
            errors.append(f"{label} has unknown case {case_id!r}")
            continue
        if not isinstance(number, int) or isinstance(number, bool) or number < 1:
            errors.append(f"{label}.trial must be a positive integer")
            continue
        pair = (case_id, number)
        if pair in seen:
            errors.append(f"duplicate trial tuple {pair!r}")
        seen.add(pair)
        fixture_digest = trial.get("fixture_digest")
        if not isinstance(fixture_digest, str) or not DIGEST.fullmatch(fixture_digest):
            errors.append(f"{label}.fixture_digest must be canonical sha256")
        for field in ("duration_ms",):
            if not isinstance(trial.get(field), int) or trial[field] < 0:
                errors.append(f"{label}.{field} must be a nonnegative integer")
        for field in ("tokens", "cost"):
            value = trial.get(field)
            if value is not None and (
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or value < 0
            ):
                errors.append(f"{label}.{field} must be null or nonnegative")
        outcome = trial.get("outcome")
        if outcome not in {"completed", "error", "not_run"}:
            errors.append(f"{label}.outcome is invalid")
        if outcome == "error" and (
            not isinstance(trial.get("error_message"), str)
            or not trial["error_message"].strip()
        ):
            errors.append(f"{label}.error_message is required for an error")
        if outcome == "not_run" and (
            not isinstance(trial.get("not_run_reason"), str)
            or not trial["not_run_reason"].strip()
        ):
            errors.append(f"{label}.not_run_reason is required when not run")
        flags = trial.get("validity_flags")
        try:
            flag_values = string_list(
                flags, f"{label}.validity_flags", allow_empty=True
            )
            unknown_flags = set(flag_values) - KNOWN_VALIDITY_FLAGS
            if unknown_flags:
                errors.append(
                    f"{label}.validity_flags contains unknown values: "
                    + ", ".join(sorted(unknown_flags))
                )
        except EvalError as error:
            errors.append(str(error))
        if outcome == "completed":
            observed = trial.get("observed")
            if not isinstance(observed, dict):
                errors.append(f"{label}.observed must be an object")
            else:
                if not isinstance(observed.get("route"), str) or not observed[
                    "route"
                ].strip():
                    errors.append(f"{label}.observed.route must be nonempty")
                for field in ("signals", "violations", "references"):
                    try:
                        string_list(
                            observed.get(field),
                            f"{label}.observed.{field}",
                            allow_empty=True,
                        )
                    except EvalError as error:
                        errors.append(str(error))
        computed = computed_trial_status(trial, cases[case_id])
        if trial.get("status") != computed:
            errors.append(
                f"{label}.status is {trial.get('status')!r}; "
                f"expected {computed!r} from observations"
            )
    expected_pairs = expected_trial_pairs(suite, cases_doc)
    missing = expected_pairs - seen
    extra = seen - expected_pairs
    if missing:
        errors.append(f"missing trial tuples: {sorted(missing)!r}")
    if extra:
        errors.append(f"unexpected trial tuples: {sorted(extra)!r}")
    valid_statuses = {"pass", "fail", "error", "not_run"}
    if all(
        isinstance(trial, dict)
        and trial.get("case_id") in cases
        and trial.get("status") in valid_statuses
        for trial in trials
    ):
        expected = expected_summary(trials, cases)
        if result.get("summary") != expected:
            errors.append("result.summary does not match recomputed trial outcomes")
    approval = result.get("human_approval")
    if not isinstance(approval, dict) or approval.get("decision") not in {
        "pending",
        "approved",
        "rejected",
    }:
        errors.append("result.human_approval is missing or invalid")
    else:
        if approval.get("decision") == "approved":
            for field in ("reviewer", "reviewed_at"):
                if not isinstance(approval.get(field), str) or not approval[
                    field
                ].strip():
                    errors.append(
                        f"result.human_approval.{field} is required when approved"
                    )
        if require_approval and approval.get("decision") != "approved":
            errors.append("promotion validation requires explicit human approval")
    return errors


def case_pass_rates(result: dict) -> dict[str, float]:
    totals: Counter[str] = Counter()
    passes: Counter[str] = Counter()
    for trial in result["trials"]:
        totals[trial["case_id"]] += 1
        if trial["status"] == "pass":
            passes[trial["case_id"]] += 1
    return {case_id: passes[case_id] / total for case_id, total in totals.items()}


def compare_results(baseline: dict, candidate: dict) -> tuple[str, bool]:
    baseline_errors = validate_result(baseline)
    candidate_errors = validate_result(candidate)
    if baseline_errors or candidate_errors:
        joined = [
            *(f"baseline: {error}" for error in baseline_errors),
            *(f"candidate: {error}" for error in candidate_errors),
        ]
        raise EvalError("cannot compare invalid results:\n- " + "\n- ".join(joined))
    if baseline["suite"] != candidate["suite"]:
        raise EvalError("baseline and candidate suites differ")
    base_rates = case_pass_rates(baseline)
    candidate_rates = case_pass_rates(candidate)
    requirements_doc, cases_doc, _ = suite_documents()
    cases = unique_objects(cases_doc["cases"], "cases")
    hard_requirements = {
        requirement["id"]
        for requirement in requirements_doc["requirements"]
        if requirement.get("level") == "hard"
    }
    rows: list[str] = []
    hard_regressions: list[str] = []
    for case_id in sorted(base_rates):
        delta = candidate_rates[case_id] - base_rates[case_id]
        rows.append(
            f"| {case_id} | {base_rates[case_id]:.3f} | "
            f"{candidate_rates[case_id]:.3f} | {delta:+.3f} |"
        )
        linked = cases[case_id]["requirements"]
        if delta < 0 and any(requirement in hard_requirements for requirement in linked):
            hard_regressions.append(case_id)
    summary_delta = candidate["summary"]["pass_rate"] - baseline["summary"]["pass_rate"]
    report = [
        "# CodeFlow model evaluation comparison",
        "",
        f"- Baseline: `{baseline['run_id']}`",
        f"- Candidate: `{candidate['run_id']}`",
        f"- Suite: `{candidate['suite']}`",
        f"- Aggregate pass-rate delta: `{summary_delta:+.3f}`",
        f"- Hard regressions: `{len(hard_regressions)}`",
        "",
        "| Case | Baseline | Candidate | Delta |",
        "|---|---:|---:|---:|",
        *rows,
        "",
    ]
    if hard_regressions:
        report.extend(
            [
                "Promotion blocked by hard regressions:",
                "",
                *(f"- `{case}`" for case in hard_regressions),
            ]
        )
    else:
        report.append(
            "No hard-case pass-rate regression was observed; promotion still "
            "requires validity review and human approval."
        )
    return "\n".join(report) + "\n", not hard_regressions


def cleanup_run(run_root: Path, confirmation: str) -> None:
    raw = run_root.expanduser()
    if raw.is_symlink():
        raise EvalError("refusing symlinked run root")
    resolved = raw.resolve()
    if resolved == Path(resolved.anchor) or resolved == Path.home().resolve():
        raise EvalError("refusing root or home cleanup")
    marker_path = resolved / RUN_MARKER
    if not marker_path.is_file():
        raise EvalError("refusing unmarked cleanup target")
    if (resolved / ".git").exists():
        raise EvalError("refusing cleanup target that is itself a git repository")
    marker = load_json(marker_path)
    run_id = marker.get("run_id") if isinstance(marker, dict) else None
    if not isinstance(run_id, str) or confirmation != run_id:
        raise EvalError("cleanup confirmation must exactly match the run_id")
    shutil.rmtree(resolved)


def parser() -> argparse.ArgumentParser:
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="command", required=True)
    validate_suite_cmd = sub.add_parser("validate-suite")
    validate_suite_cmd.add_argument("--project-root", type=Path)
    validate_suite_cmd.add_argument("--resources", type=Path, default=RESOURCE_DIR)

    materialize_cmd = sub.add_parser("materialize")
    materialize_cmd.add_argument("--run-root", required=True, type=Path)
    materialize_cmd.add_argument("--case", required=True)
    materialize_cmd.add_argument("--trial", required=True, type=int)
    materialize_cmd.add_argument("--codeflow", required=True, type=Path)

    validate_result_cmd = sub.add_parser("validate-result")
    validate_result_cmd.add_argument("result", type=Path)
    validate_result_cmd.add_argument("--require-approval", action="store_true")

    compare_cmd = sub.add_parser("compare")
    compare_cmd.add_argument("baseline", type=Path)
    compare_cmd.add_argument("candidate", type=Path)

    cleanup_cmd = sub.add_parser("cleanup")
    cleanup_cmd.add_argument("--run-root", required=True, type=Path)
    cleanup_cmd.add_argument("--confirm", required=True)
    return cli


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "validate-suite":
            root = args.project_root.resolve() if args.project_root else project_root()
            errors = validate_suite(root, args.resources.resolve())
            if errors:
                print("suite invalid:")
                for error in errors:
                    print(f"- {error}")
                return 1
            print(f"suite valid: {suite_digest(args.resources.resolve())}")
            return 0
        if args.command == "materialize":
            print(
                json.dumps(
                    materialize(args.case, args.trial, args.run_root, args.codeflow),
                    indent=2,
                )
            )
            return 0
        if args.command == "validate-result":
            errors = validate_result(load_json(args.result), require_approval=args.require_approval)
            if errors:
                print("result invalid:")
                for error in errors:
                    print(f"- {error}")
                return 1
            print("result valid")
            return 0
        if args.command == "compare":
            report, promotable = compare_results(
                load_json(args.baseline), load_json(args.candidate)
            )
            print(report, end="")
            return 0 if promotable else 1
        if args.command == "cleanup":
            cleanup_run(args.run_root, args.confirm)
            print("evaluation run root removed")
            return 0
    except (EvalError, OSError, subprocess.SubprocessError, KeyError) as error:
        print(f"evaluation error: {error}", file=sys.stderr)
        return 2
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
