#!/usr/bin/env python3
"""Validate, materialize, score, compare, qualify, and clean model evaluations."""

from __future__ import annotations

import argparse
import copy
from collections import Counter, defaultdict
from datetime import datetime, timezone
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import stat
import statistics
import subprocess
import sys
from typing import Any


SCRIPT_DIR = Path(__file__).resolve().parent
SKILL_DIR = SCRIPT_DIR.parent
RESOURCE_DIR = SKILL_DIR / "resources"
RUN_MARKER = ".codeflow-eval-run.json"
PIN_FILE = "codeflow-eval-pins.json"
GH_LOG = "gh-stand-in.json"
POLL_MIN_SECONDS = 60
POLL_CEILING_SECONDS = 30 * 60
MAX_SETTINGS_BYTES = 16 * 1024 * 1024
DIGEST = re.compile(r"^sha256:[0-9a-f]{64}$")
EXPERIMENT_VARIABLE = re.compile(r"^system\.[a-z_][a-z0-9_.]*$")
EVIDENCE_KINDS = frozenset({"session", "tool", "file", "command", "ui"})
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
SAFE_BRANCH = re.compile(
    r"^(?:main|master|fixture/[a-z0-9][a-z0-9-]*|integration/[A-Za-z0-9][A-Za-z0-9-]*)$"
)
SAFE_ID = re.compile(r"^[a-z0-9][a-z0-9._-]*$")
RFC3339 = re.compile(
    r"^(?P<date_time>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})"
    r"(?:\.(?P<fraction>[0-9]+))?(?P<offset>Z|[+-][0-9]{2}:[0-9]{2})$"
)
QUALIFIED_ROLES = frozenset(
    {
        "primary",
        "producer",
        "reviewer",
        "evidence-worker",
        "claude-judgment-primary",
        "codex-engineering-primary",
        "grok-engineering-primary",
    }
)
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


def write_json_atomic(path: Path, value: Any) -> None:
    """Write JSON without exposing a partially rewritten result record."""

    if path.is_symlink():
        raise EvalError(f"refusing symlinked JSON output: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.{secrets.token_hex(8)}.tmp")
    try:
        write_json(temporary, value)
        os.replace(temporary, path)
    finally:
        if temporary.exists():
            temporary.unlink()


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


def qualification_documents(resource_dir: Path = RESOURCE_DIR) -> tuple[dict, dict]:
    harnesses = load_json(resource_dir / "harnesses.json")
    packs = load_json(resource_dir / "packs.json")
    if not all(isinstance(item, dict) for item in (harnesses, packs)):
        raise EvalError("harnesses and packs must be JSON objects")
    return harnesses, packs


def suite_digest(resource_dir: Path = RESOURCE_DIR) -> str:
    requirements, cases, fixtures = suite_documents(resource_dir)
    harnesses, packs = qualification_documents(resource_dir)
    return canonical_digest(
        {
            "requirements": requirements,
            "cases": cases,
            "fixtures": fixtures,
            "harnesses": harnesses,
            "packs": packs,
        }
    )


def string_list(value: Any, label: str, *, allow_empty: bool = False) -> list[str]:
    if not isinstance(value, list) or any(
        not isinstance(item, str) or not item.strip() for item in value
    ):
        raise EvalError(f"{label} must be an array of nonempty strings")
    if not allow_empty and not value:
        raise EvalError(f"{label} must not be empty")
    return value


def evidence_errors(value: Any, label: str, *, allow_empty: bool = False) -> list[str]:
    if not isinstance(value, list) or (not allow_empty and not value):
        return [f"{label} must be a nonempty evidence array"]
    errors: list[str] = []
    for index, item in enumerate(value):
        item_label = f"{label}[{index}]"
        if not isinstance(item, dict):
            errors.append(f"{item_label} must be an object")
            continue
        if item.get("kind") not in EVIDENCE_KINDS:
            errors.append(f"{item_label}.kind is invalid")
        if not isinstance(item.get("ref"), str) or not item["ref"].strip():
            errors.append(f"{item_label}.ref must be a nonempty string")
        digest = item.get("digest")
        if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
            errors.append(f"{item_label}.digest must be canonical sha256")
    return errors


def is_rfc3339(value: Any) -> bool:
    if not isinstance(value, str):
        return False
    matched = RFC3339.fullmatch(value)
    if matched is None:
        return False
    offset = "+00:00" if matched["offset"] == "Z" else matched["offset"]
    # datetime validates the calendar, clock, and offset. Omitting the
    # fractional component keeps arbitrary RFC 3339 precision portable across
    # supported Python versions instead of depending on fromisoformat's
    # version-specific precision handling.
    candidate = matched["date_time"] + offset
    try:
        parsed = datetime.fromisoformat(candidate)
    except ValueError:
        return False
    return parsed.tzinfo is not None


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


def harness_catalog(resource_dir: Path = RESOURCE_DIR) -> dict[str, dict]:
    harnesses_doc, _ = qualification_documents(resource_dir)
    return unique_objects(harnesses_doc.get("harnesses"), "harnesses")


def pack_catalog(resource_dir: Path = RESOURCE_DIR) -> dict[str, dict]:
    _, packs_doc = qualification_documents(resource_dir)
    return unique_objects(packs_doc.get("packs"), "packs")


def resolve_pack(
    pack_id: str, resource_dir: Path = RESOURCE_DIR
) -> list[str]:
    packs = pack_catalog(resource_dir)
    cases = unique_objects(
        suite_documents(resource_dir)[1].get("cases"), "cases"
    )
    resolved: list[str] = []
    seen_cases: set[str] = set()
    visiting: set[str] = set()
    visited: set[str] = set()

    def visit(current: str) -> None:
        if current not in packs:
            raise EvalError(f"unknown evaluation pack {current!r}")
        if current in visiting:
            raise EvalError(f"evaluation pack cycle contains {current!r}")
        if current in visited:
            return
        visiting.add(current)
        pack = packs[current]
        for included in string_list(
            pack.get("includes"), f"{current}.includes", allow_empty=True
        ):
            visit(included)
        for case_id in string_list(
            pack.get("cases"), f"{current}.cases", allow_empty=True
        ):
            if case_id not in cases:
                raise EvalError(f"{current}: unknown case {case_id!r}")
            if case_id not in seen_cases:
                resolved.append(case_id)
                seen_cases.add(case_id)
        visiting.remove(current)
        visited.add(current)

    visit(pack_id)
    if not resolved:
        raise EvalError(f"evaluation pack {pack_id!r} resolves to no cases")
    return resolved


def validate_suite(root: Path, resource_dir: Path = RESOURCE_DIR) -> list[str]:
    errors: list[str] = []
    try:
        requirements_doc, cases_doc, fixtures_doc = suite_documents(resource_dir)
        harnesses_doc, packs_doc = qualification_documents(resource_dir)
        if requirements_doc.get("schema_version") != 1:
            errors.append("requirements schema_version must be 1")
        if cases_doc.get("schema_version") != 1:
            errors.append("cases schema_version must be 1")
        if fixtures_doc.get("schema_version") != 1:
            errors.append("fixtures schema_version must be 1")
        if harnesses_doc.get("schema_version") != 1:
            errors.append("harnesses schema_version must be 1")
        if packs_doc.get("schema_version") != 1:
            errors.append("packs schema_version must be 1")
        requirements = unique_objects(
            requirements_doc.get("requirements"), "requirements"
        )
        cases = unique_objects(cases_doc.get("cases"), "cases")
        fixtures = unique_objects(fixtures_doc.get("fixtures"), "fixtures")
        harnesses = unique_objects(harnesses_doc.get("harnesses"), "harnesses")
        packs = unique_objects(packs_doc.get("packs"), "packs")
    except EvalError as error:
        return [str(error)]

    try:
        capability_contract_list = string_list(
            harnesses_doc.get("capability_contract"),
            "harnesses.capability_contract",
        )
        capability_contract = set(capability_contract_list)
        if len(capability_contract) != len(capability_contract_list):
            errors.append("harnesses.capability_contract contains a duplicate")
    except EvalError as error:
        errors.append(str(error))
        capability_contract = set()
    for harness_id, harness in harnesses.items():
        if not SAFE_ID.fullmatch(harness_id):
            errors.append(f"{harness_id}: unsafe harness id")
        for field in ("provider", "lineage"):
            if not isinstance(harness.get(field), str) or not harness[field].strip():
                errors.append(f"{harness_id}: missing {field}")
        if harness.get("status") != "capability-supported":
            errors.append(
                f"{harness_id}: only capability-supported harnesses may be listed"
            )
        try:
            capabilities = set(
                string_list(
                    harness.get("capabilities"), f"{harness_id}.capabilities"
                )
            )
        except EvalError as error:
            errors.append(str(error))
            capabilities = set()
        if isinstance(harness.get("capabilities"), list) and len(capabilities) != len(
            harness["capabilities"]
        ):
            errors.append(f"{harness_id}: duplicate capability")
        missing_capabilities = capability_contract - capabilities
        if missing_capabilities:
            errors.append(
                f"{harness_id}: missing contract capabilities: "
                + ", ".join(sorted(missing_capabilities))
            )
        probe = harness.get("version_probe")
        if probe is not None and (
            not isinstance(probe, str) or not SAFE_ID.fullmatch(probe)
        ):
            errors.append(
                f"{harness_id}.version_probe must be null or a safe probe id"
            )
        evidence = harness.get("evidence")
        if not isinstance(evidence, dict):
            errors.append(f"{harness_id}.evidence must be an object")
        else:
            if set(evidence) != capabilities:
                errors.append(
                    f"{harness_id}.evidence must map every declared capability exactly"
                )
            for capability in sorted(capabilities):
                try:
                    references = string_list(
                        evidence.get(capability),
                        f"{harness_id}.evidence.{capability}",
                    )
                    for reference in references:
                        try:
                            project_evidence_file(root, reference)
                        except EvalError as error:
                            errors.append(
                                f"{harness_id}.evidence.{capability}: "
                                f"{error}"
                            )
                except EvalError as error:
                    errors.append(str(error))

    for pack_id, pack in packs.items():
        if not SAFE_ID.fullmatch(pack_id):
            errors.append(f"{pack_id}: unsafe pack id")
        if not isinstance(pack.get("description"), str) or not pack[
            "description"
        ].strip():
            errors.append(f"{pack_id}: missing description")
        try:
            includes = string_list(
                pack.get("includes"), f"{pack_id}.includes", allow_empty=True
            )
            selected_cases = string_list(
                pack.get("cases"), f"{pack_id}.cases", allow_empty=True
            )
            for included in includes:
                if included not in packs:
                    errors.append(f"{pack_id}: unknown included pack {included!r}")
            for case_id in selected_cases:
                if case_id not in cases:
                    errors.append(f"{pack_id}: unknown case {case_id!r}")
            if len(selected_cases) != len(set(selected_cases)):
                errors.append(f"{pack_id}: duplicate case")
        except EvalError as error:
            errors.append(str(error))
    for pack_id in packs:
        try:
            resolve_pack(pack_id, resource_dir)
        except EvalError as error:
            errors.append(str(error))

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
        try:
            roles = string_list(
                case.get("roles", []), f"{case_id}.roles", allow_empty=True
            )
            unknown_roles = set(roles) - QUALIFIED_ROLES
            if unknown_roles:
                errors.append(
                    f"{case_id}: unknown roles: "
                    + ", ".join(sorted(unknown_roles))
                )
            if len(roles) != len(set(roles)):
                errors.append(f"{case_id}: duplicate role")
        except EvalError as error:
            errors.append(str(error))
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
        validated_untracked_files: list[str] = []
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
            local_origin_main = state.get("local_origin_main") is True
            squash_cleanup = state.get("squash_cleanup_worktree") is True
            target_precedes_fixture = state.get("target_precedes_fixture") is True
            if target_precedes_fixture:
                target = state.get("target")
                if not isinstance(target, str) or not SAFE_BRANCH.fullmatch(target):
                    errors.append(
                        f"{fixture_id}: target_precedes_fixture requires a safe state.target"
                    )
                elif target == branch:
                    errors.append(
                        f"{fixture_id}: target_precedes_fixture target must differ from state.branch"
                    )
            if local_origin_main and branch != "main":
                errors.append(
                    f"{fixture_id}: local_origin_main requires state.branch to be main"
                )
            if local_origin_main and squash_cleanup:
                errors.append(
                    f"{fixture_id}: local_origin_main and "
                    "squash_cleanup_worktree are mutually exclusive"
                )
            if target_precedes_fixture and (local_origin_main or squash_cleanup):
                errors.append(
                    f"{fixture_id}: target_precedes_fixture is mutually exclusive "
                    "with local_origin_main and squash_cleanup_worktree"
                )
            if "untracked_files" in state:
                try:
                    validated_untracked_files = string_list(
                        state.get("untracked_files"),
                        f"{fixture_id}.state.untracked_files",
                    )
                except EvalError as error:
                    errors.append(str(error))
                for relative in validated_untracked_files:
                    try:
                        safe_relative_path(relative)
                    except EvalError as error:
                        errors.append(f"{fixture_id}: {error}")
            if squash_cleanup:
                if branch in {"main", "master"}:
                    errors.append(
                        f"{fixture_id}: cleanup task branch must not be protected"
                    )
                landed_change = state.get("landed_change")
                if not isinstance(landed_change, dict):
                    errors.append(f"{fixture_id}: landed_change must be an object")
                else:
                    landed_path = landed_change.get("path")
                    if not isinstance(landed_path, str):
                        errors.append(
                            f"{fixture_id}: landed_change.path must be a string"
                        )
                    else:
                        try:
                            safe_relative_path(landed_path)
                        except EvalError as error:
                            errors.append(f"{fixture_id}: {error}")
                    if not isinstance(landed_change.get("content"), str):
                        errors.append(
                            f"{fixture_id}: landed_change.content must be a string"
                        )
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
        pinned = state.get("pinned_files", []) if isinstance(state, dict) else []
        try:
            pinned = string_list(pinned, f"{fixture_id}.state.pinned_files",
                                 allow_empty=True)
        except EvalError as error:
            errors.append(str(error))
            pinned = []
        for relative in pinned:
            if relative not in files:
                errors.append(f"{fixture_id}: pinned file missing from files: {relative}")
            if relative in validated_untracked_files:
                errors.append(f"{fixture_id}: pinned file must be tracked: {relative}")
        for relative in validated_untracked_files:
            if relative not in files:
                errors.append(
                    f"{fixture_id}: untracked fixture file missing from files: "
                    f"{relative}"
                )

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


def project_evidence_file(root: Path, reference: str) -> Path:
    """Resolve a retained evidence file without leaving or aliasing the project."""

    project = root.resolve()
    relative = safe_relative_path(reference)
    candidate = project / relative
    refuse_symlink_components(candidate, project)
    if not candidate.is_file():
        raise EvalError(f"missing project evidence reference {reference!r}")
    resolved = candidate.resolve()
    try:
        resolved.relative_to(project)
    except ValueError as error:
        raise EvalError(
            f"project evidence reference escapes the project: {reference!r}"
        ) from error
    return resolved


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


def configure_fixture_hooks(root: Path) -> None:
    if (root / ".codeflow/git-hooks").is_dir():
        run_command(["git", "config", "core.hooksPath", ".codeflow/git-hooks"], root)


def reset_fixture_history(
    root: Path, branch: str, *, install_hooks: bool = True
) -> None:
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
    if install_hooks:
        configure_fixture_hooks(root)


def configure_target_before_fixture(root: Path, target: str, branch: str) -> None:
    """Create a plan branch from a real target before subject files are added."""

    reset_fixture_history(root, target, install_hooks=False)
    run_command(["git", "switch", "-c", branch], root)


def configure_local_origin_main(root: Path, origin: Path) -> None:
    """Add a fixture-local bare origin whose main tip matches the subject HEAD."""

    if origin.exists() or origin.is_symlink():
        raise EvalError(f"fixture origin already exists: {origin}")
    origin.parent.mkdir(parents=True, exist_ok=True)
    run_command(["git", "clone", "--bare", str(root), str(origin)], origin.parent)
    run_command(["git", "remote", "add", "origin", str(origin)], root)
    run_command(["git", "fetch", "origin"], root)
    run_command(["git", "branch", "--set-upstream-to=origin/main", "main"], root)


def configure_squash_cleanup_worktree(root: Path, state: dict) -> None:
    """Turn a primary task checkout into a removable squash-landed worktree."""

    task_branch = state["branch"]
    landed_change = state["landed_change"]
    write_fixture_file(root, landed_change["path"], landed_change["content"])
    run_command(["git", "add", "--", landed_change["path"]], root)
    run_command(["git", "commit", "-m", "fix: task branch parser boundary"], root)
    run_command(["git", "branch", "main", "HEAD~1"], root)
    run_command(["git", "switch", "main"], root)
    run_command(["git", "cherry-pick", "--no-commit", task_branch], root)
    run_command(["git", "commit", "-m", "fix: landed parser boundary (squash)"], root)

    control = root.parent / "control"
    if control.exists() or control.is_symlink():
        raise EvalError(f"fixture control checkout already exists: {control}")
    root.rename(control)
    configure_local_origin_main(control, root.parent / "origin.git")
    run_command(["git", "worktree", "add", str(root), task_branch], control)


def git_common_dir(root: Path) -> Path:
    done = subprocess.run(
        ["git", "rev-parse", "--path-format=absolute", "--git-common-dir"],
        cwd=root, check=True, capture_output=True, text=True,
    )
    return Path(done.stdout.strip())


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_pins(root: Path, relatives: list[str]) -> dict[str, str]:
    """Record pinned file digests in the fixture's git dir before the trial."""

    pins = {relative: file_sha256(root / safe_relative_path(relative))
            for relative in relatives}
    if pins:
        pin_path = git_common_dir(root) / PIN_FILE
        write_json(pin_path, pins)
        pin_path.chmod(0o444)
    return pins


TRACE_SLACK_SECONDS = 5.0
WATCH_DEFAULT_INTERVAL = 10
SHELL_SEPARATORS = {";", "&", "&&", "||", "|", "|&", "\n", "(", ")"}
REDIRECTIONS = {">", ">>", ">|", "&>", "&>>", "<>"}
FILE_WRITERS = {"rm", "mv", "tee", "truncate", "touch", "chmod", "chown", "ln",
                "unlink", "shred", "dd"}
COPY_WRITERS = {"cp", "install", "rsync"}
IN_PLACE_EDITORS = {"sed", "perl", "gsed"}
GIT_WRITERS = {"checkout", "restore", "rm", "mv", "stash", "clean", "reset",
               "apply", "update-index"}
INTERPRETERS = {"python", "python3", "perl", "ruby", "node", "bash", "sh", "zsh"}
CODE_WRITE_MARKERS = ("open(", "write", "unlink", "remove", "rename", "replace(",
                      "rmtree", "truncate", "copy", "move", ">")
COMMAND_PREFIXES = {"sudo", "command", "env", "nohup", "time", "exec"}


def load_trace(path: Path) -> list[dict]:
    """Read the command trace extract: one JSON object per line.

    Each entry is taken from the retained native trace by the harness:
    ``{"at", "end", "kind": "command", "command", "cwd"?, "exit"?}`` for a
    shell command, or ``{"at", "end", "kind": "file_write", "path"}`` for any
    create, edit, move or delete made by a non-shell tool.
    """

    entries = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        try:
            entry = json.loads(line)
        except json.JSONDecodeError as error:
            raise EvalError(f"trace line {number}: {error}") from error
        entries.append(entry)
    return entries


def trace_errors(trace: Any) -> list[str]:
    if not isinstance(trace, list):
        return ["command trace must be a list of entries"]
    errors = []
    for index, entry in enumerate(trace):
        label = f"trace entry {index + 1}"
        if not isinstance(entry, dict):
            errors.append(f"{label}: not an object")
            continue
        if not all(isinstance(entry.get(key), (int, float)) for key in ("at", "end")) \
                or entry["end"] < entry["at"]:
            errors.append(f"{label}: needs numeric at <= end")
        if entry.get("kind") == "command":
            if not isinstance(entry.get("command"), str):
                errors.append(f"{label}: command must be a string")
            if "exit" in entry and not isinstance(entry["exit"], int):
                errors.append(f"{label}: exit must be an integer")
        elif entry.get("kind") == "file_write":
            if not isinstance(entry.get("path"), str) or not entry["path"]:
                errors.append(f"{label}: file_write needs a path")
        else:
            errors.append(f"{label}: kind must be command or file_write")
    return errors


def shell_segments(command: str) -> list[list[str]] | None:
    """Split shell text into simple commands; None when it cannot be tokenized."""

    lexer = shlex.shlex(command.replace("\n", " ; "), posix=True, punctuation_chars=True)
    lexer.whitespace_split = True
    try:
        tokens = list(lexer)
    except ValueError:
        return None
    segments, current = [], []
    for token in tokens:
        if token in SHELL_SEPARATORS:
            if current:
                segments.append(current)
            current = []
        else:
            current.append(token)
    if current:
        segments.append(current)
    return segments


def command_word(segment: list[str]) -> tuple[int, str]:
    index = 0
    while index < len(segment) and (
        "=" in segment[index].split("/")[0] or segment[index] in COMMAND_PREFIXES
    ):
        index += 1
    word = segment[index].rsplit("/", 1)[-1] if index < len(segment) else ""
    return index, word


def resolves_to(token: str, cwd: Path, protected: set[Path]) -> bool:
    if not token or token.startswith("-"):
        return False
    candidate = Path(token).expanduser()
    candidate = candidate if candidate.is_absolute() else cwd / candidate
    text = (os.path.normpath(str(candidate)) if any(c in token for c in "*?[")
            else os.path.realpath(candidate))
    return any(
        text == str(path) or fnmatch.fnmatch(str(path), text) for path in protected
    )


def tamper_findings(trace: list[dict], root: Path, protected: set[Path]) -> list[str]:
    """Commands and tool writes in the trace that touch harness evidence."""

    names = sorted({path.name for path in protected} | {
        str(path.relative_to(root)) for path in protected if root in path.parents})
    findings = []
    for number, entry in enumerate(trace, 1):
        label = f"tampered with harness evidence (trace entry {number})"
        cwd = Path(entry.get("cwd") or root).resolve()
        if entry["kind"] == "file_write":
            if resolves_to(entry["path"], cwd, protected):
                findings.append(f"{label}: {entry['path']}")
            continue
        text = entry["command"]
        segments = shell_segments(text)
        if segments is None:
            if any(name in text for name in names) and any(
                    marker in text for marker in CODE_WRITE_MARKERS):
                findings.append(f"{label}: {text[:80]}")
            continue
        for segment in segments:
            start, word = command_word(segment)
            args = segment[start + 1:]
            touched = False
            for index, token in enumerate(segment[:-1]):
                if token in REDIRECTIONS and resolves_to(segment[index + 1], cwd, protected):
                    touched = True
            if word in FILE_WRITERS or (word in IN_PLACE_EDITORS and any(
                    arg.startswith("-i") for arg in args)):
                touched |= any(resolves_to(arg, cwd, protected) for arg in args)
            elif word in COPY_WRITERS and args:
                touched |= resolves_to(args[-1], cwd, protected)
            elif word == "git" and args and args[0] in GIT_WRITERS:
                touched |= any(resolves_to(arg, cwd, protected) for arg in args[1:])
            elif word.rstrip("0123456789.") in INTERPRETERS and (
                    any(arg in {"-c", "-e", "-"} for arg in args) or "<<" in text):
                touched |= any(name in text for name in names) and any(
                    marker in text for marker in CODE_WRITE_MARKERS)
            if touched:
                findings.append(f"{label}: {' '.join(segment)[:80]}")
    return findings


def gh_invocations(trace: list[dict]) -> list[dict]:
    """Every gh stand-in call the trace shows, with its window and arguments."""

    calls = []
    for entry in trace:
        if entry["kind"] != "command":
            continue
        for segment in shell_segments(entry["command"]) or []:
            for index, token in enumerate(segment):
                if token == "gh.py" or token.endswith("/gh.py"):
                    calls.append({"at": float(entry["at"]), "end": float(entry["end"]),
                                  "argv": segment[index + 1:],
                                  "exit": entry.get("exit")})
                    break
    return calls


def gh_log_errors(log: Any, polls_expected: bool) -> list[str]:
    if not isinstance(log, dict):
        return ["gh log is not an object"]
    errors = []
    calls = log.get("calls")
    if not isinstance(calls, list):
        return ["gh log has no calls list"]
    for index, call in enumerate(calls):
        if not isinstance(call, dict) or not isinstance(call.get("at"), (int, float)):
            errors.append(f"gh log call {index + 1} has no time")
        elif "argv" in call:
            if not isinstance(call["argv"], list) or not all(
                    isinstance(arg, str) for arg in call["argv"]):
                errors.append(f"gh log call {index + 1} has an invalid argv")
        elif not isinstance(call.get("event"), str):
            errors.append(f"gh log call {index + 1} has neither argv nor event")
    if polls_expected:
        polls = log.get("polls")
        if not isinstance(polls, list) or not all(
                isinstance(poll, dict) and isinstance(poll.get("at"), (int, float))
                and isinstance(poll.get("head"), str) and poll["head"]
                for poll in polls):
            errors.append("gh log polls are missing or invalid")
    if calls and not isinstance(log.get("scenario_sha256"), str):
        errors.append("gh log has calls but no scenario digest")
    return errors


def watch_interval(argv: list[str]) -> int:
    if "--interval" in argv[:-1]:
        try:
            return max(1, int(argv[argv.index("--interval") + 1]))
        except ValueError:
            return WATCH_DEFAULT_INTERVAL
    return WATCH_DEFAULT_INTERVAL


def spacing_findings(times: list[float], source: str) -> list[str]:
    times = sorted(times)
    gaps = [later - earlier for earlier, later in zip(times, times[1:])]
    findings = []
    if any(gap < POLL_MIN_SECONDS for gap in gaps):
        findings.append(
            f"poll spacing under one minute ({source}): shortest gap {min(gaps):.1f}s")
    if times and times[-1] - times[0] > POLL_CEILING_SECONDS:
        findings.append(f"polling continued past thirty minutes ({source})")
    return findings


def gh_binding_findings(log: dict, trace: list[dict], scenario: dict,
                        polls_expected: bool) -> list[str]:
    """Bind the gh log to the harness trace, call by call and poll by poll."""

    findings = []
    invocations = gh_invocations(trace)
    logged = [call for call in log["calls"] if "argv" in call]
    used = set()
    matched = []
    for invocation in invocations:
        match = next(
            (index for index, call in enumerate(logged)
             if index not in used and call["argv"] == invocation["argv"]
             and invocation["at"] - TRACE_SLACK_SECONDS <= call["at"]
             <= invocation["end"] + TRACE_SLACK_SECONDS), None)
        if match is None:
            findings.append("gh call in the trace has no log entry: "
                            + " ".join(invocation["argv"])[:60])
        else:
            used.add(match)
            matched.append(invocation)
    for index, call in enumerate(logged):
        if index not in used:
            findings.append("gh log records a call the trace does not show: "
                            + " ".join(call["argv"])[:60])
    derived = []
    pr_exists = bool(scenario.get("open"))
    polls = [float(poll["at"]) for poll in log.get("polls", [])] if polls_expected else []
    for invocation in invocations:
        argv = invocation["argv"]
        if argv[:2] == ["pr", "create"] and invocation.get("exit") == 0:
            pr_exists = True
        if argv[:2] != ["pr", "checks"] or not pr_exists:
            continue
        duration = invocation["end"] - invocation["at"]
        if "--watch" in argv:
            interval = watch_interval(argv)
            ceiling = int(duration // interval) + 1
            derived += [invocation["at"] + step * interval for step in range(ceiling)]
            low, high = 1, ceiling
        else:
            derived.append(invocation["at"])
            low = high = 1
        if polls_expected:
            seen = sum(invocation["at"] - TRACE_SLACK_SECONDS <= poll
                       <= invocation["end"] + TRACE_SLACK_SECONDS for poll in polls)
            if not low <= seen <= high:
                findings.append(
                    f"gh log has {seen} poll(s) for a pr checks call the trace says "
                    f"made {low} to {high}")
    findings += spacing_findings(derived, "trace")
    if polls_expected:
        findings += spacing_findings(polls, "gh log")
    return findings


def trial_findings(record: dict, trace: list[dict] | None = None) -> list[str]:
    """Compare a finished trial with its receipt and the harness command trace.

    Fails closed: evidence that is missing, unreadable or unbound is a finding.
    """

    root = Path(record["path"])
    pinned = record.get("pinned_files", {})
    findings = []
    for relative, expected in sorted(pinned.items()):
        path = root / safe_relative_path(relative)
        if not path.is_file():
            findings.append(f"pinned file removed: {relative}")
        elif file_sha256(path) != expected:
            findings.append(f"pinned file changed: {relative}")
    common = git_common_dir(root)
    pin_path, log_path = common / PIN_FILE, common / GH_LOG
    if pinned:
        if not pin_path.is_file() or file_sha256(pin_path) != record.get("pin_record_sha256"):
            findings.append(f"pin record in the git dir changed or missing: {PIN_FILE}")
        if trace is None:
            findings.append("no command trace supplied: harness evidence is unbound")
    if trace is not None:
        errors = trace_errors(trace)
        findings += [f"command trace invalid: {error}" for error in errors]
        if errors:
            trace = None
    protected = {(root / safe_relative_path(relative)).resolve() for relative in pinned}
    protected |= {pin_path.resolve(), log_path.resolve()}
    if trace is not None:
        findings += tamper_findings(trace, root.resolve(), protected)
    if "tools/gh.py" not in pinned:
        return findings
    try:
        log = load_json(log_path)
    except EvalError:
        findings.append(f"gh log missing or unreadable: {GH_LOG}")
        return findings
    gh_source = root / "tools/gh.py"
    polls_expected = gh_source.is_file() and '"polls"' in gh_source.read_text(
        encoding="utf-8")
    errors = gh_log_errors(log, polls_expected)
    findings += [f"gh log schema invalid: {error}" for error in errors]
    if errors:
        return findings
    if any(call.get("event") == "scenario_mismatch" for call in log["calls"]):
        findings.append("gh stand-in logged scenario_mismatch")
    scenario_pin = pinned.get("tools/gh-scenario.json")
    if log.get("scenario_sha256") not in (None, scenario_pin):
        findings.append("gh stand-in ran against a scenario other than the pinned one")
    scenario_path = root / "tools/gh-scenario.json"
    scenario_intact = bool(scenario_pin) and scenario_path.is_file() and (
        file_sha256(scenario_path) == scenario_pin)
    if trace is not None and scenario_intact:
        findings += gh_binding_findings(
            log, trace, load_json(scenario_path), polls_expected)
    return findings


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
            if relative == ".git":
                continue
            executable = bool(
                path.stat().st_mode & (stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
            )
            digest.update(relative.encode("utf-8") + b"\0")
            digest.update((b"x" if executable else b"-") + b"\0")
            digest.update(path.read_bytes() + b"\0")
    return "sha256:" + digest.hexdigest()


def executable_digest(path: Path) -> str:
    digest = hashlib.sha256()
    try:
        with path.open("rb") as executable:
            for chunk in iter(lambda: executable.read(1024 * 1024), b""):
                digest.update(chunk)
    except OSError as error:
        raise EvalError(f"cannot read CodeFlow executable {path}: {error}") from error
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
    codeflow_path = codeflow.expanduser().resolve()
    if not codeflow_path.is_file() or not os.access(codeflow_path, os.X_OK):
        raise EvalError(f"CodeFlow binary is not executable: {codeflow_path}")
    codeflow_sha256 = executable_digest(codeflow_path)
    output.mkdir(parents=True)
    tier_flag = "--full" if fixture["tier"] == "full" else "--standard"
    run_command([str(codeflow_path), "init", "--yes", tier_flag], output)
    if executable_digest(codeflow_path) != codeflow_sha256:
        raise EvalError("CodeFlow executable changed during materialization")
    remove_grader_material(output)
    state = fixture["state"]
    branch = state.get("branch", "fixture/base")
    target_precedes_fixture = state.get("target_precedes_fixture") is True
    if target_precedes_fixture:
        configure_target_before_fixture(output, state["target"], branch)
    for relative, content in fixture["files"].items():
        write_fixture_file(output, relative, content)
    write_fixture_file(output, "TASK.md", case["prompt"].rstrip() + "\n")
    assert_no_grader_material(output)
    post_history_files: dict[str, str] = {}
    for relative in state.get("untracked_files", []):
        safe = safe_relative_path(relative)
        target = output / safe
        if not target.is_file():
            raise EvalError(f"untracked fixture file is missing: {relative}")
        post_history_files[relative] = target.read_text(encoding="utf-8")
        target.unlink()
    if target_precedes_fixture:
        run_command(["git", "add", "-A"], output)
        run_command(["git", "commit", "-m", "chore: add evaluation subject"], output)
    else:
        reset_fixture_history(output, branch, install_hooks=False)
    if state.get("squash_cleanup_worktree") is True:
        configure_squash_cleanup_worktree(output, state)
    elif state.get("local_origin_main") is True:
        configure_local_origin_main(output, output.parent / "origin.git")
    for relative, content in post_history_files.items():
        write_fixture_file(output, relative, content)
    configure_fixture_hooks(output)
    pins = write_pins(output, state.get("pinned_files", []))
    digest = tree_digest(output)
    trial_record = {
        "schema_version": 1,
        "run_id": marker["run_id"],
        "case_id": case_id,
        "trial": trial,
        "fixture_id": fixture["id"],
        "fixture_state": fixture["state"],
        "fixture_digest": digest,
        "pinned_files": pins,
        "pin_record_sha256": (
            file_sha256(git_common_dir(output) / PIN_FILE) if pins else None
        ),
        "path": str(output),
        "codeflow_executable": {
            "path": str(codeflow_path),
            "sha256": codeflow_sha256,
        },
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
        and item.get("kind") in EVIDENCE_KINDS
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
    binding_mismatch = False
    if not isinstance(result, dict):
        return ["result must be an object"]
    _, cases_doc, _ = suite_documents()
    cases = unique_objects(cases_doc["cases"], "cases")
    harnesses = harness_catalog()
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
        harness_id = system.get("harness")
        if harness_id not in harnesses:
            errors.append(
                "result.system.harness is not in the capability-supported harness catalog"
            )
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
        observed_binding = system.get("observed")
        if observed_binding is not None:
            if not isinstance(observed_binding, dict):
                errors.append("result.system.observed must be an object")
            else:
                for field in ("model", "effort"):
                    value = observed_binding.get(field)
                    if not isinstance(value, str) or not value.strip():
                        errors.append(
                            f"result.system.observed.{field} must be a nonempty string"
                        )
                    elif value != system.get(field):
                        binding_mismatch = True
                errors.extend(
                    evidence_errors(
                        observed_binding.get("evidence"),
                        "result.system.observed.evidence",
                    )
                )
        peer_seats = system.get("peer_seats")
        if peer_seats is not None:
            if not isinstance(peer_seats, list) or not peer_seats:
                errors.append("result.system.peer_seats must be a nonempty array")
            else:
                seen_seats: set[str] = set()
                for index, peer in enumerate(peer_seats):
                    label = f"result.system.peer_seats[{index}]"
                    if not isinstance(peer, dict):
                        errors.append(f"{label} must be an object")
                        continue
                    for field in (
                        "seat",
                        "model",
                        "effort",
                        "harness",
                        "harness_version",
                    ):
                        value = peer.get(field)
                        if not isinstance(value, str) or not value.strip():
                            errors.append(f"{label}.{field} must be a nonempty string")
                    if peer.get("harness") not in harnesses:
                        errors.append(
                            f"{label}.harness is not in the capability-supported harness catalog"
                        )
                    seat = peer.get("seat")
                    if isinstance(seat, str) and seat:
                        if seat in seen_seats:
                            errors.append(f"{label}.seat duplicates {seat!r}")
                        seen_seats.add(seat)
                    digest = peer.get("settings_digest")
                    if not isinstance(digest, str) or not DIGEST.fullmatch(digest):
                        errors.append(
                            f"{label}.settings_digest must be canonical sha256"
                        )
    experiment = result.get("experiment")
    if experiment is not None:
        if not isinstance(experiment, dict):
            errors.append("result.experiment must be an object")
        else:
            variable = experiment.get("variable")
            if (
                not isinstance(variable, str)
                or not EXPERIMENT_VARIABLE.fullmatch(variable)
            ):
                errors.append(
                    "result.experiment.variable must be a system.* field path"
                )
            baseline_run_id = experiment.get("baseline_run_id")
            if (
                not isinstance(baseline_run_id, str)
                or not baseline_run_id.strip()
            ):
                errors.append(
                    "result.experiment.baseline_run_id must be a nonempty string"
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
            errors.extend(evidence_errors(trial.get("evidence"), f"{label}.evidence"))
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
    if binding_mismatch and not any(
        isinstance(trial, dict)
        and isinstance(trial.get("validity_flags"), list)
        and "harness_context_mismatch" in trial["validity_flags"]
        for trial in trials
    ):
        errors.append(
            "requested and observed model/effort differ without a "
            "harness_context_mismatch validity flag"
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
            reviewer = approval.get("reviewer")
            if not isinstance(reviewer, str) or not reviewer.strip():
                errors.append(
                    "result.human_approval.reviewer is required when approved"
                )
            if not is_rfc3339(approval.get("reviewed_at")):
                errors.append(
                    "result.human_approval.reviewed_at must be RFC 3339 when approved"
                )
        if require_approval and approval.get("decision") != "approved":
            errors.append("promotion validation requires explicit human approval")
    if require_approval:
        try:
            verified_observed_binding(result, "promotion result")
        except (EvalError, KeyError) as error:
            errors.append(str(error))
        hard_requirements = {
            requirement["id"]
            for requirement in suite_documents()[0]["requirements"]
            if requirement.get("level") == "hard"
        }
        hard_cases = {
            case_id
            for case_id, case in cases.items()
            if hard_requirements.intersection(case["requirements"])
        }
        failed_hard = sorted(
            case_id
            for case_id in hard_cases
            if any(
                trial.get("case_id") == case_id and trial.get("status") != "pass"
                for trial in trials
                if isinstance(trial, dict)
            )
        )
        if failed_hard:
            errors.append(
                "promotion validation requires every hard-linked trial to pass: "
                + ", ".join(failed_hard)
            )
        if any(
            isinstance(trial, dict) and trial.get("validity_flags")
            for trial in trials
        ):
            errors.append("promotion validation requires zero validity flags")
        if any(
            isinstance(trial, dict) and trial.get("outcome") in {"error", "not_run"}
            for trial in trials
        ):
            errors.append("promotion validation does not allow error or not_run trials")
    return errors


def case_pass_rates(result: dict) -> dict[str, float]:
    totals: Counter[str] = Counter()
    passes: Counter[str] = Counter()
    for trial in result["trials"]:
        totals[trial["case_id"]] += 1
        if trial["status"] == "pass":
            passes[trial["case_id"]] += 1
    return {case_id: passes[case_id] / total for case_id, total in totals.items()}


def score_result(result: Any) -> dict:
    """Return a validated copy with every derived field recomputed."""

    if not isinstance(result, dict):
        raise EvalError("result must be an object")
    scored = copy.deepcopy(result)
    _, cases_doc, _ = suite_documents()
    cases = unique_objects(cases_doc["cases"], "cases")
    trials = scored.get("trials")
    if not isinstance(trials, list):
        raise EvalError("result.trials must be an array")
    for index, trial in enumerate(trials):
        if not isinstance(trial, dict):
            raise EvalError(f"trials[{index}] must be an object")
        case_id = trial.get("case_id")
        if case_id not in cases:
            raise EvalError(f"trials[{index}] has unknown case {case_id!r}")
        trial["status"] = computed_trial_status(trial, cases[case_id])
    scored["summary"] = expected_summary(trials, cases)
    errors = validate_result(scored)
    if errors:
        raise EvalError("cannot score invalid raw result:\n- " + "\n- ".join(errors))
    return scored


def value_at_path(value: dict, path: str) -> Any:
    current: Any = value
    for part in path.split("."):
        if not isinstance(current, dict) or part not in current:
            raise EvalError(f"comparison variable does not exist: {path}")
        current = current[part]
    return current


def remove_path(value: dict, path: str) -> None:
    parts = path.split(".")
    current: Any = value
    for part in parts[:-1]:
        if not isinstance(current, dict) or part not in current:
            raise EvalError(f"comparison variable does not exist: {path}")
        current = current[part]
    if not isinstance(current, dict) or parts[-1] not in current:
        raise EvalError(f"comparison variable does not exist: {path}")
    del current[parts[-1]]


def verified_observed_binding(result: dict, label: str) -> None:
    system = result["system"]
    observed = system.get("observed")
    if not isinstance(observed, dict):
        raise EvalError(f"{label} lacks system.observed binding evidence")
    for field in ("model", "effort"):
        if observed.get(field) != system.get(field):
            raise EvalError(
                f"{label} observed {field} does not match requested {field}"
            )
    errors = evidence_errors(observed.get("evidence"), f"{label}.system.observed.evidence")
    if errors:
        raise EvalError("; ".join(errors))


def enforce_one_variable(
    baseline: dict, candidate: dict, variable: str
) -> None:
    if not EXPERIMENT_VARIABLE.fullmatch(variable):
        raise EvalError("comparison variable must be a system.* field path")
    if variable.startswith("system.observed") or variable.startswith(
        "system.peer_seats"
    ):
        raise EvalError("observed bindings and peer seats cannot be varied directly")
    for label, result in (("baseline", baseline), ("candidate", candidate)):
        experiment = result.get("experiment")
        if not isinstance(experiment, dict) or experiment.get("variable") != variable:
            raise EvalError(f"{label} experiment does not declare {variable}")
        verified_observed_binding(result, label)
    if candidate["experiment"].get("baseline_run_id") != baseline["run_id"]:
        raise EvalError("candidate experiment.baseline_run_id does not name baseline")
    baseline_value = value_at_path(baseline, variable)
    candidate_value = value_at_path(candidate, variable)
    if baseline_value == candidate_value:
        raise EvalError("declared comparison variable does not differ")

    baseline_system = copy.deepcopy(baseline["system"])
    candidate_system = copy.deepcopy(candidate["system"])
    # Runtime evidence is necessarily run-specific; requested configuration is
    # compared independently and each observed binding is proven above.
    baseline_system.pop("observed", None)
    candidate_system.pop("observed", None)
    relative_path = variable.removeprefix("system.")
    remove_path(baseline_system, relative_path)
    remove_path(candidate_system, relative_path)
    if baseline_system != candidate_system:
        raise EvalError(
            "baseline and candidate differ outside the declared comparison variable"
        )


def metric_distribution(trials: list[dict], metric: str) -> tuple[Any, Any, Any, Any] | None:
    values = [trial.get(metric) for trial in trials]
    if not values or any(
        value is None
        or not isinstance(value, (int, float))
        or isinstance(value, bool)
        for value in values
    ):
        return None
    return min(values), statistics.median(values), max(values), sum(values)


def format_metric(value: tuple[Any, Any, Any, Any] | None) -> str:
    if value is None:
        return "n/a"
    minimum, median, maximum, total = value
    return f"{minimum:g}/{median:g}/{maximum:g} ({total:g})"


def distribution_rows(baseline: dict, candidate: dict, metric: str) -> list[str]:
    rows: list[str] = []
    case_ids = sorted({trial["case_id"] for trial in baseline["trials"]})
    for case_id in case_ids:
        base_trials = [
            trial for trial in baseline["trials"] if trial["case_id"] == case_id
        ]
        candidate_trials = [
            trial for trial in candidate["trials"] if trial["case_id"] == case_id
        ]
        rows.append(
            f"| {case_id} | {format_metric(metric_distribution(base_trials, metric))} | "
            f"{format_metric(metric_distribution(candidate_trials, metric))} |"
        )
    rows.append(
        f"| **All trials** | "
        f"{format_metric(metric_distribution(baseline['trials'], metric))} | "
        f"{format_metric(metric_distribution(candidate['trials'], metric))} |"
    )
    return rows


def compare_results(
    baseline: dict, candidate: dict, *, variable: str | None = None
) -> tuple[str, bool]:
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
    if variable is not None:
        enforce_one_variable(baseline, candidate, variable)
    promotion_errors = (
        [
            *(
                f"baseline: {error}"
                for error in validate_result(baseline, require_approval=True)
            ),
            *(
                f"candidate: {error}"
                for error in validate_result(candidate, require_approval=True)
            ),
        ]
        if variable is not None
        else []
    )
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
    regressions: list[str] = []
    for case_id in sorted(base_rates):
        delta = candidate_rates[case_id] - base_rates[case_id]
        rows.append(
            f"| {case_id} | {base_rates[case_id]:.3f} | "
            f"{candidate_rates[case_id]:.3f} | {delta:+.3f} |"
        )
        linked = cases[case_id]["requirements"]
        if delta < 0:
            regressions.append(case_id)
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
        *(
            [
                f"- Declared variable: `{variable}`",
                f"- Any regressions: `{len(regressions)}`",
                f"- Promotion validation blockers: `{len(promotion_errors)}`",
            ]
            if variable
            else []
        ),
        "",
        "| Case | Baseline | Candidate | Delta |",
        "|---|---:|---:|---:|",
        *rows,
        "",
    ]
    if variable is not None:
        for metric, heading in (
            ("duration_ms", "Duration ms"),
            ("tokens", "Tokens"),
            ("cost", "Cost"),
        ):
            report.extend(
                [
                    f"## {heading} distributions",
                    "",
                    "Values are min/median/max (total); `n/a` means telemetry was incomplete.",
                    "",
                    "| Case | Baseline | Candidate |",
                    "|---|---:|---:|",
                    *distribution_rows(baseline, candidate, metric),
                    "",
                ]
            )
    blocking_regressions = regressions if variable is not None else hard_regressions
    if blocking_regressions:
        report.extend(
            [
                "Promotion blocked by regressions:",
                "",
                *(f"- `{case}`" for case in blocking_regressions),
            ]
        )
    else:
        report.append(
            "No hard-case pass-rate regression was observed; promotion still "
            "requires validity review and human approval."
        )
    if promotion_errors:
        report.extend(
            [
                "",
                "Promotion validation blockers:",
                "",
                *(f"- {error}" for error in promotion_errors),
            ]
        )
    return "\n".join(report) + "\n", not blocking_regressions and not promotion_errors


def file_digest(path: Path) -> str:
    try:
        with path.open("rb") as source:
            data = source.read(MAX_SETTINGS_BYTES + 1)
    except OSError as error:
        raise EvalError(f"cannot read settings source {path}: {error}") from error
    if len(data) > MAX_SETTINGS_BYTES:
        raise EvalError(
            f"settings source exceeds {MAX_SETTINGS_BYTES} bytes: {path}"
        )
    return "sha256:" + hashlib.sha256(data).hexdigest()


def qualified_binding_record(
    result: dict,
    *,
    binding_id: str,
    roles: list[str],
    settings_files: list[Path],
) -> dict:
    """Build the non-secret local record for a human-approved full result."""

    if not SAFE_ID.fullmatch(binding_id):
        raise EvalError(
            "binding id must contain only lowercase letters, digits, dot, dash, "
            "or underscore and start with a letter or digit"
        )
    normalized_roles = sorted(set(roles))
    if not normalized_roles:
        raise EvalError("at least one qualified role is required")
    unknown_roles = set(normalized_roles) - QUALIFIED_ROLES
    if unknown_roles:
        raise EvalError(
            "unknown qualified roles: " + ", ".join(sorted(unknown_roles))
        )
    validation_errors = validate_result(result)
    if validation_errors:
        raise EvalError(
            "cannot record an unqualified binding:\n- "
            + "\n- ".join(validation_errors)
        )
    _, cases_doc, _ = suite_documents()
    role_cases: dict[str, set[str]] = defaultdict(set)
    for case in cases_doc["cases"]:
        for role in case.get("roles", []):
            role_cases[role].add(case["id"])
    for role in normalized_roles:
        if role in {"claude-judgment-primary", "codex-engineering-primary"}:
            if not role_cases[role]:
                raise EvalError(f"no behavioral cases qualify role {role}")
            failed = sorted(
                {
                    trial["case_id"]
                    for trial in result["trials"]
                    if trial["case_id"] in role_cases[role]
                    and trial["status"] != "pass"
                }
            )
            if failed:
                raise EvalError(
                    f"role {role} has non-passing cases: " + ", ".join(failed)
                )
    promotion_errors = validate_result(result, require_approval=True)
    if promotion_errors:
        raise EvalError(
            "cannot record an unqualified binding:\n- "
            + "\n- ".join(promotion_errors)
        )
    system = result["system"]
    harness = harness_catalog()[system["harness"]]
    observed = system["observed"]
    sources: list[dict[str, str]] = []
    seen_paths: set[Path] = set()
    for supplied in settings_files:
        path = supplied.expanduser().resolve()
        if path in seen_paths:
            continue
        if not path.is_file():
            raise EvalError(f"settings source is not a regular file: {path}")
        seen_paths.add(path)
        sources.append({"path": str(path), "digest": file_digest(path)})
    approval = result["human_approval"]
    return {
        "schema_version": 1,
        "binding_id": binding_id,
        "provider": harness["provider"],
        "lineage": harness["lineage"],
        "eligible_roles": normalized_roles,
        "qualified_at": approval["reviewed_at"],
        "requested": {
            "model": system["model"],
            "effort": system["effort"],
            "harness": system["harness"],
            "harness_version": system["harness_version"],
            "settings_digest": system["settings_digest"],
        },
        "observed": {
            "model": observed["model"],
            "effort": observed["effort"],
            "evidence": [
                {"kind": item["kind"], "digest": item["digest"]}
                for item in observed["evidence"]
            ],
        },
        "qualification": {
            "run_id": result["run_id"],
            "suite": result["suite"],
            "suite_digest": result["suite_digest"],
            "result_digest": canonical_digest(result),
            "codeflow_revision": system["codeflow_revision"],
        },
        "settings_sources": sources,
        "approval": {
            "reviewer": approval["reviewer"],
            "reviewed_at": approval["reviewed_at"],
        },
    }


def qualified_binding_output(path: Path) -> Path:
    override = os.environ.get("CODEFLOW_HOME", "").strip()
    home = Path(override).expanduser() if override else Path.home() / ".codeflow"
    directory = (home / "qualified-bindings").resolve()
    output = path.expanduser().resolve()
    if output.parent != directory or output.suffix != ".json":
        raise EvalError(
            "binding output must be a direct .json child of "
            f"{directory}"
        )
    return output


def write_qualified_binding(path: Path, record: dict) -> None:
    output = qualified_binding_output(path)
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        output.parent.chmod(0o700)
    except OSError as error:
        raise EvalError(
            f"cannot make qualified-binding directory owner-only: {error}"
        ) from error
    write_json_atomic(output, record)
    try:
        output.chmod(0o600)
    except OSError as error:
        raise EvalError(
            f"cannot make qualified-binding record owner-only: {error}"
        ) from error


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

    list_cases_cmd = sub.add_parser("list-cases")
    list_cases_cmd.add_argument("--pack", required=True)
    list_cases_cmd.add_argument("--resources", type=Path, default=RESOURCE_DIR)

    materialize_cmd = sub.add_parser("materialize")
    materialize_cmd.add_argument("--run-root", required=True, type=Path)
    materialize_cmd.add_argument("--case", required=True)
    materialize_cmd.add_argument("--trial", required=True, type=int)
    materialize_cmd.add_argument("--codeflow", required=True, type=Path)

    check_trial_cmd = sub.add_parser("check-trial")
    check_trial_cmd.add_argument("--record", required=True, type=Path)
    check_trial_cmd.add_argument("--trace", type=Path)

    validate_result_cmd = sub.add_parser("validate-result")
    validate_result_cmd.add_argument("result", type=Path)
    validate_result_cmd.add_argument("--require-approval", action="store_true")

    score_cmd = sub.add_parser("score")
    score_cmd.add_argument("result", type=Path)
    score_destination = score_cmd.add_mutually_exclusive_group()
    score_destination.add_argument("--output", type=Path)
    score_destination.add_argument("--in-place", action="store_true")

    compare_cmd = sub.add_parser("compare")
    compare_cmd.add_argument("baseline", type=Path)
    compare_cmd.add_argument("candidate", type=Path)
    compare_cmd.add_argument("--variable")

    record_cmd = sub.add_parser("record-binding")
    record_cmd.add_argument("result", type=Path)
    record_cmd.add_argument("--binding-id", required=True)
    record_cmd.add_argument(
        "--role",
        action="append",
        required=True,
        choices=sorted(QUALIFIED_ROLES),
    )
    record_cmd.add_argument("--settings-file", action="append", type=Path, default=[])
    record_cmd.add_argument("--output", required=True, type=Path)

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
        if args.command == "list-cases":
            for case_id in resolve_pack(args.pack, args.resources.resolve()):
                print(case_id)
            return 0
        if args.command == "materialize":
            print(
                json.dumps(
                    materialize(args.case, args.trial, args.run_root, args.codeflow),
                    indent=2,
                )
            )
            return 0
        if args.command == "check-trial":
            trace = load_trace(args.trace) if args.trace else None
            findings = trial_findings(load_json(args.record), trace)
            for finding in findings:
                print(f"- {finding}")
            print("trial evidence intact" if not findings else "trial fails")
            return 1 if findings else 0
        if args.command == "validate-result":
            errors = validate_result(
                load_json(args.result), require_approval=args.require_approval
            )
            if errors:
                print("result invalid:")
                for error in errors:
                    print(f"- {error}")
                return 1
            print("result valid")
            return 0
        if args.command == "score":
            scored = score_result(load_json(args.result))
            if args.in_place:
                write_json_atomic(args.result, scored)
                print(f"scored result written in place: {args.result}")
            elif args.output:
                if args.output.resolve() == args.result.resolve():
                    raise EvalError("use --in-place to overwrite the input result")
                write_json_atomic(args.output, scored)
                print(f"scored result written: {args.output}")
            else:
                print(json.dumps(scored, ensure_ascii=False, indent=2, sort_keys=True))
            return 0
        if args.command == "compare":
            report, promotable = compare_results(
                load_json(args.baseline),
                load_json(args.candidate),
                variable=args.variable,
            )
            print(report, end="")
            return 0 if promotable else 1
        if args.command == "record-binding":
            record = qualified_binding_record(
                load_json(args.result),
                binding_id=args.binding_id,
                roles=args.role,
                settings_files=args.settings_file,
            )
            output = qualified_binding_output(args.output)
            write_qualified_binding(output, record)
            print(f"qualified binding written: {output}")
            return 0
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
