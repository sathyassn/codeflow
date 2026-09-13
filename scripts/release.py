#!/usr/bin/env python3
"""Repository-owned same-PR release-state sync and verification."""
from __future__ import annotations

import argparse
from dataclasses import dataclass
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib
from typing import Any, NoReturn

ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CONFIG = ROOT / ".release/config.json"
IMPACT_ORDER = {"none": 0, "patch": 1, "minor": 2, "major": 3}
PLACEHOLDERS = {"", "n/a", "none", "todo", "tbd", "-"}
STABLE_TAG = re.compile(r"v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
IMPACT_MARKER = re.compile(
    r"<!--\s*codeflow:release-impact\s+(none|patch|minor|major)"
    r"(?:\s+legacy-group=([a-z0-9][a-z0-9.-]*)\s+sha256=([0-9a-f]{64}))?\s*-->"
)
LEGACY_END = "<!-- codeflow:legacy-group-end -->"
SOURCE_MARKER = "<!-- codeflow-release-source: {source} -->"


class ReleaseError(RuntimeError):
    """A fail-closed release contract violation."""


def fail(message: str) -> NoReturn:
    raise ReleaseError(message)


def run(args: list[str], *, cwd: Path = ROOT, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        args, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if check and result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no output"
        fail(f"command failed ({' '.join(args)}): {detail}")
    return result


def git(*args: str, cwd: Path = ROOT, check: bool = True) -> str:
    return run(["git", *args], cwd=cwd, check=check).stdout.strip()


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def load_config(path: Path) -> dict[str, Any]:
    value = load_json(path)
    if (
        not isinstance(value, dict)
        or value.get("schema_version") != 2
        or value.get("release_unit") != "codeflow"
    ):
        fail("release config must be schema v2 for codeflow")
    for key in ["main_branch", "bootstrap", "watched_contract_paths"]:
        if key not in value:
            fail(f"release config is missing {key}")
    if not isinstance(value["bootstrap"], dict):
        fail("release bootstrap must be an object")
    return value


def normalize_value(value: str) -> str:
    value = value.strip()
    marker = chr(96)
    if len(value) >= 2 and value[0] == value[-1] == marker:
        value = value[1:-1].strip()
    return value


def is_placeholder(value: str) -> bool:
    return normalize_value(value).casefold() in PLACEHOLDERS


def parse_release_impact(body: str) -> dict[str, str]:
    sections = list(re.finditer(r"(?m)^## Release impact\s*$", body))
    if len(sections) != 1:
        fail("PR body must contain exactly one '## Release impact' section")
    start = sections[0].end()
    next_heading = re.search(r"(?m)^## ", body[start:])
    section = body[start : start + next_heading.start()] if next_heading else body[start:]
    fields: dict[str, str] = {}
    for match in re.finditer(r"(?m)^- ([A-Za-z ]+):[ \t]*(.*?)[ \t]*$", section):
        key = match.group(1).strip().casefold().replace(" ", "_")
        if key in fields:
            fail(f"release impact field {match.group(1)} is duplicated")
        fields[key] = normalize_value(match.group(2))
    for key in ["unit", "impact", "rationale", "evidence", "contract"]:
        if key not in fields or (
            key in {"rationale", "evidence"} and is_placeholder(fields[key])
        ):
            fail(f"release impact field {key} is required and substantive")
    if fields["unit"] != "codeflow" or fields["impact"] not in IMPACT_ORDER:
        fail("release unit/impact must be codeflow and none, patch, minor, or major")
    if fields["contract"] not in {"not-applicable", "compatible", "breaking"}:
        fail("release contract must be not-applicable, compatible, or breaking")
    return fields


def semver(value: str) -> tuple[int, int, int]:
    match = re.fullmatch(r"v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value)
    if not match:
        fail(f"unsupported stable release version: {value}")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def bump(version: str, impact: str) -> str:
    major, minor, patch = semver(version)
    if impact == "major":
        return f"{major + 1}.0.0"
    if impact == "minor":
        return f"{major}.{minor + 1}.0"
    if impact == "patch":
        return f"{major}.{minor}.{patch + 1}"
    return version.removeprefix("v")


def commit_impact(base: str, head: str, *, cwd: Path = ROOT) -> str:
    """Return a conservative conventional-marker floor, never a version."""
    raw = git("log", "--no-merges", "--format=%s%x1f%b%x1e", f"{base}..{head}", cwd=cwd)
    impact = "none"
    for entry in raw.split("\x1e"):
        entry = entry.lstrip("\r\n")
        if not entry.strip():
            continue
        subject, _, body = entry.partition("\x1f")
        if re.match(r"^[a-z][a-z0-9-]*(?:\([^\n)]*\))?!:", subject) or re.search(
            r"(?m)^BREAKING[ -]CHANGE:\s*\S", body
        ):
            candidate = "major"
        elif re.match(r"^feat(?:\([^\n)]*\))?:", subject):
            candidate = "minor"
        elif re.match(r"^(?:fix|perf)(?:\([^\n)]*\))?:", subject):
            candidate = "patch"
        else:
            candidate = "none"
        if IMPACT_ORDER[candidate] > IMPACT_ORDER[impact]:
            impact = candidate
    return impact


def file_at_ref(ref: str, path: str, *, cwd: Path) -> bytes:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"], cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if result.returncode != 0:
        fail(f"{ref} is missing required release file {path}")
    return result.stdout


def changed_paths(base: str, head: str, *, cwd: Path) -> list[str]:
    return [
        line
        for line in git("diff", "--name-only", f"{base}...{head}", "--", cwd=cwd).splitlines()
        if line
    ]


def matches_any(path: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def github_json(endpoint: str, *, cwd: Path) -> Any:
    result = run(["gh", "api", endpoint], cwd=cwd)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"GitHub returned malformed state for {endpoint}: {error}")


def github_pages(endpoint: str, *, cwd: Path) -> list[Any]:
    result = run(["gh", "api", "--paginate", "--slurp", endpoint], cwd=cwd)
    try:
        pages = json.loads(result.stdout)
    except json.JSONDecodeError as error:
        fail(f"GitHub returned malformed paginated state for {endpoint}: {error}")
    if not isinstance(pages, list) or any(not isinstance(page, list) for page in pages):
        fail(f"GitHub pagination shape is invalid for {endpoint}")
    return [item for page in pages for item in page]


def discover_host_state(
    repository: str, *, cwd: Path, drafts_visible: bool = False
) -> dict[str, Any]:
    if not repository:
        try:
            raw = run(["gh", "repo", "view", "--json", "nameWithOwner"], cwd=cwd).stdout
            repository = json.loads(raw)["nameWithOwner"]
        except (ReleaseError, json.JSONDecodeError, KeyError, TypeError) as error:
            fail(f"cannot determine GitHub repository: {error}")
    try:
        releases = github_pages(f"repos/{repository}/releases?per_page=100", cwd=cwd)
        refs = github_pages(f"repos/{repository}/git/matching-refs/tags/v", cwd=cwd)
    except ReleaseError as error:
        fail(f"published state unavailable; authenticate gh or pass --host-state: {error}")
    if not isinstance(releases, list) or not isinstance(refs, list):
        fail("GitHub release/tag inventory is malformed")
    tags: dict[str, str] = {}
    for item in refs:
        ref = item.get("ref") if isinstance(item, dict) else None
        tag = ref.removeprefix("refs/tags/") if isinstance(ref, str) else ""
        if STABLE_TAG.fullmatch(tag):
            obj = item.get("object") if isinstance(item, dict) else None
            for _ in range(5):
                if not isinstance(obj, dict):
                    break
                object_type, sha = obj.get("type"), obj.get("sha")
                if object_type == "commit":
                    break
                if object_type != "tag" or not isinstance(sha, str):
                    obj = None
                    break
                peeled = github_json(f"repos/{repository}/git/tags/{sha}", cwd=cwd)
                obj = peeled.get("object") if isinstance(peeled, dict) else None
            else:
                obj = None
            sha = obj.get("sha") if isinstance(obj, dict) and obj.get("type") == "commit" else None
            if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha):
                fail(f"cannot resolve stable tag {tag}")
            tags[tag] = sha
    compact = []
    for item in releases:
        if not isinstance(item, dict) or not STABLE_TAG.fullmatch(str(item.get("tag_name", ""))):
            continue
        compact.append(
            {
                "tag": item["tag_name"],
                "draft": item.get("draft"),
                "prerelease": item.get("prerelease"),
                "target": item.get("target_commitish"),
                "body": item.get("body"),
                "assets": [
                    {"name": asset.get("name"), "size": asset.get("size"), "digest": asset.get("digest")}
                    for asset in item.get("assets", [])
                    if isinstance(asset, dict)
                ],
            }
        )
    return {
        "schema_version": 1,
        "drafts_visible": drafts_visible,
        "tags": tags,
        "releases": compact,
    }


def get_host_state(args: argparse.Namespace) -> dict[str, Any]:
    if getattr(args, "host_state", None):
        value = load_json(args.host_state)
    else:
        repository = getattr(args, "repository", "") or os.getenv("GITHUB_REPOSITORY", "")
        value = discover_host_state(
            repository,
            cwd=args.root,
            drafts_visible=getattr(args, "drafts_visible", False),
        )
    if (
        not isinstance(value, dict)
        or value.get("schema_version") != 1
        or not isinstance(value.get("drafts_visible"), bool)
        or not isinstance(value.get("tags"), dict)
        or not isinstance(value.get("releases"), list)
    ):
        fail("host state must be schema v1 with tag and release inventories")
    return value


@dataclass(frozen=True)
class Baseline:
    version: str
    tag: str
    source: str
    comparison_commit: str


def validate_bootstrap(config: dict[str, Any], *, cwd: Path) -> Baseline:
    published = config["bootstrap"].get("published")
    comparison = config["bootstrap"].get("comparison")
    if not isinstance(published, dict) or not isinstance(comparison, dict):
        fail("bootstrap must record published and distinct comparison provenance")
    version, tag = str(published.get("version", "")), comparison.get("tag")
    commit, source = comparison.get("commit"), published.get("source_commit")
    if tag != f"v{version}" or not isinstance(commit, str) or not isinstance(source, str):
        fail("bootstrap version, comparison tag, and source are malformed")
    semver(version)
    actual = git("rev-parse", f"{tag}^{{commit}}", cwd=cwd)
    if actual != commit:
        fail(f"bootstrap tag {tag} resolves to {actual}, expected {commit}; never move it")
    checksum = str(published.get("source_archive_sha256", ""))
    if not re.fullmatch(r"[0-9a-f]{64}", checksum):
        fail("bootstrap archive checksum must be 64 lowercase hex characters")
    return Baseline(version, tag, source, commit)


def resolve_baseline(
    config: dict[str, Any],
    state: dict[str, Any],
    *,
    cwd: Path,
    allow_attempt: tuple[str, str] | None = None,
) -> Baseline:
    bootstrap = validate_bootstrap(config, cwd=cwd)
    tags, releases = state["tags"], state["releases"]
    bootstrap_release = next(
        (
            item
            for item in releases
            if isinstance(item, dict)
            and item.get("tag") == bootstrap.tag
            and item.get("draft") is False
            and item.get("prerelease") is False
        ),
        None,
    )
    expected_target = config["bootstrap"]["published"].get("release_target_commit")
    if (
        bootstrap_release is None
        or tags.get(bootstrap.tag) != bootstrap.comparison_commit
        or bootstrap_release.get("target") != expected_target
    ):
        fail("live host state does not prove the recorded public bootstrap release")
    public = [bootstrap]
    for item in releases:
        if not isinstance(item, dict):
            fail("host release inventory has a malformed entry")
        tag = item.get("tag")
        if (
            not isinstance(tag, str)
            or not STABLE_TAG.fullmatch(tag)
            or item.get("draft") is not False
            or item.get("prerelease") is not False
        ):
            continue
        version = tag.removeprefix("v")
        if semver(version) <= semver(bootstrap.version):
            continue
        source = tags.get(tag)
        if not isinstance(source, str) or not re.fullmatch(r"[0-9a-f]{40}", source):
            fail(f"public {tag} lacks immutable peeled-tag source identity")
        if SOURCE_MARKER.format(source=source) not in str(item.get("body") or ""):
            fail(f"public {tag} lacks the exact-source marker")
        assets = item.get("assets")
        if not isinstance(assets, list) or not assets:
            fail(f"public {tag} lacks verified assets")
        if any(
            not isinstance(asset, dict)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", str(asset.get("digest", "")))
            for asset in assets
        ):
            fail(f"public {tag} has an asset without SHA-256 identity")
        public.append(Baseline(version, tag, source, source))
    baseline = max(public, key=lambda item: semver(item.version))
    allowed_tag, allowed_source = allow_attempt or ("", "")
    for tag, target in tags.items():
        if (
            STABLE_TAG.fullmatch(tag)
            and semver(tag) > semver(baseline.version)
            and (tag, target) != (allowed_tag, allowed_source)
        ):
            fail(f"{tag} exists without a verified public release; resolve that attempt first")
    if state["drafts_visible"]:
        for item in releases:
            tag = str(item.get("tag", "")) if isinstance(item, dict) else ""
            if (
                isinstance(item, dict)
                and item.get("draft") is True
                and STABLE_TAG.fullmatch(tag)
                and semver(tag) > semver(baseline.version)
            ):
                if (tag, item.get("target")) != (allowed_tag, allowed_source):
                    fail(f"draft {tag} is an unresolved publication attempt")
                if item.get("assets"):
                    fail(f"draft {tag} already has assets; refuse a concurrent or rebuilt upload")
    return baseline


@dataclass(frozen=True)
class ChangelogSection:
    version: str
    date: str | None
    start: int
    body_start: int
    end: int
    body: str


def changelog_sections(
    text: str, *, allow_legacy_unreleased: bool = False
) -> list[ChangelogSection]:
    if not allow_legacy_unreleased and re.search(r"(?m)^## \[Unreleased\]\s*$", text):
        fail("CHANGELOG.md uses [Unreleased]; use one undated pending version")
    headings = list(
        re.finditer(r"(?m)^## \[(\d+\.\d+\.\d+)\](?: - (\d{4}-\d{2}-\d{2}))?\s*$", text)
    )
    if not headings:
        fail("CHANGELOG.md has no stable version sections")
    sections = []
    for index, heading in enumerate(headings):
        end = headings[index + 1].start() if index + 1 < len(headings) else len(text)
        sections.append(
            ChangelogSection(
                heading.group(1),
                heading.group(2),
                heading.start(),
                heading.end(),
                end,
                text[heading.end() : end].strip(),
            )
        )
    versions = [semver(section.version) for section in sections]
    if versions != sorted(versions, reverse=True) or len(set(versions)) != len(versions):
        fail("CHANGELOG.md sections must be unique and newest-first")
    return sections


def section_markers(section: ChangelogSection, config: dict[str, Any]) -> list[str]:
    matches = list(IMPACT_MARKER.finditer(section.body))
    legacy = [match for match in matches if match.group(2)]
    if legacy:
        policy = config.get("legacy_pending_group")
        end = section.body.find(LEGACY_END, legacy[0].end())
        legacy_bytes = section.body[legacy[0].end() : end].strip().encode()
        actual_digest = hashlib.sha256(legacy_bytes).hexdigest()
        if (
            len(legacy) != 1
            or not isinstance(policy, dict)
            or end < 0
            or section.version != policy.get("version")
            or legacy[0].group(1) != policy.get("impact")
            or legacy[0].group(2) != policy.get("id")
            or legacy[0].group(3) != policy.get("sha256")
            or actual_digest != policy.get("sha256")
        ):
            fail("legacy marker is not the bounded bootstrap group")
        ordinary_body = section.body[: legacy[0].start()] + section.body[end + len(LEGACY_END) :]
        ordinary = list(IMPACT_MARKER.finditer(ordinary_body))
    else:
        ordinary_body = section.body
        ordinary = matches
    bullets = list(re.finditer(r"(?m)^- \S", ordinary_body))
    if len(ordinary) != len(bullets):
        fail("each pending changelog entry needs one adjacent impact marker")
    for marker, bullet in zip(ordinary, bullets, strict=True):
        if not re.fullmatch(r"\s*", ordinary_body[marker.end() : bullet.start()]):
            fail("impact marker must be directly adjacent to its changelog entry")
    return [match.group(1) for match in matches]


def annotated_entries(section: ChangelogSection, config: dict[str, Any]) -> list[tuple[str, str]]:
    section_markers(section, config)
    body = section.body
    legacy = next((match for match in IMPACT_MARKER.finditer(body) if match.group(2)), None)
    entries: list[tuple[str, str]] = []
    if legacy is not None:
        end = body.index(LEGACY_END, legacy.end())
        entries.append((legacy.group(1), f"legacy:{legacy.group(2)}:{legacy.group(3)}"))
        body = body[: legacy.start()] + body[end + len(LEGACY_END) :]
    markers = list(IMPACT_MARKER.finditer(body))
    for index, marker in enumerate(markers):
        end = markers[index + 1].start() if index + 1 < len(markers) else len(body)
        entry = body[marker.end() : end].strip()
        if not entry.startswith("- "):
            fail("impact marker is not followed by a changelog entry")
        entries.append((marker.group(1), entry))
    return entries


def expected_pending(
    text: str, baseline: Baseline, config: dict[str, Any], *, strict: bool = True
) -> tuple[str, ChangelogSection | None, list[str]]:
    sections = changelog_sections(text)
    if baseline.version not in {section.version for section in sections}:
        fail(f"CHANGELOG.md lacks published baseline {baseline.version}")
    pending = [
        section for section in sections if semver(section.version) > semver(baseline.version)
    ]
    if len(pending) > 1:
        fail("CHANGELOG.md has more than one section above the published baseline")
    if not pending:
        return baseline.version, None, []
    section = pending[0]
    if section.date is not None:
        fail("pending version heading must be undated")
    impacts = section_markers(section, config)
    highest = max(impacts, key=lambda item: IMPACT_ORDER[item], default="none")
    expected = bump(baseline.version, highest)
    if expected == baseline.version:
        fail("none-only work must not create a pending section")
    if strict and section.version != expected:
        fail(f"pending heading {section.version} disagrees with cumulative target {expected}")
    return expected, section, impacts


VERSION_STAMP_PATHS = [
    "Cargo.toml",
    "Cargo.lock",
    ".codeflow/project.toml",
    ".codeflow/manifest.json",
    "AGENTS.md",
    "CLAUDE.md",
]


def version_stamp_values(files: dict[str, bytes]) -> dict[str, str]:
    try:
        cargo = tomllib.loads(files["Cargo.toml"].decode())["workspace"]["package"]["version"]
        lock = tomllib.loads(files["Cargo.lock"].decode())
        project = tomllib.loads(files[".codeflow/project.toml"].decode())
        manifest = json.loads(files[".codeflow/manifest.json"])
    except (KeyError, UnicodeDecodeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse coupled stamps: {error}")
    if not isinstance(cargo, str):
        fail("workspace package version is not a string")
    own = {
        package["name"]: package["version"]
        for package in lock.get("package", [])
        if package.get("name") in {"codeflow-cli", "codeflow-core", "codeflow-present"}
    }
    if len(own) != 3:
        fail(f"workspace lock is missing package stamps: {own}")
    values = {"Cargo.toml": cargo, **{f"Cargo.lock:{name}": value for name, value in own.items()}}
    for name, value in [
        (".codeflow/project.toml", project.get("scaffold_version")),
        (".codeflow/manifest.json", manifest.get("scaffold_version")),
    ]:
        if not isinstance(value, str):
            fail(f"{name} scaffold stamp is not a string")
        values[name] = value
    for name in ["AGENTS.md", "CLAUDE.md"]:
        markers = re.findall(rb"<!-- codeflow:managed:begin scaffold=(\d+\.\d+\.\d+) -->", files[name])
        if len(markers) != 1:
            fail(f"{name} must contain exactly one managed scaffold stamp")
        values[name] = markers[0].decode()
    for value in values.values():
        semver(value)
    return values


def validate_version_stamps(files: dict[str, bytes]) -> str:
    values = version_stamp_values(files)
    versions = set(values.values())
    if len(versions) != 1:
        fail(f"coupled version stamps disagree: {values}")
    cargo = values["Cargo.toml"]
    return cargo


def validate_release_tree(
    ref: str,
    config: dict[str, Any],
    state: dict[str, Any],
    *,
    cwd: Path,
    allow_attempt: tuple[str, str] | None = None,
) -> dict[str, Any]:
    baseline = resolve_baseline(config, state, cwd=cwd, allow_attempt=allow_attempt)
    changelog = file_at_ref(ref, "CHANGELOG.md", cwd=cwd).decode()
    validate_published_sections(changelog, baseline, config, cwd=cwd)
    version, pending, impacts = expected_pending(
        changelog, baseline, config
    )
    stamped = validate_version_stamps(
        {path: file_at_ref(ref, path, cwd=cwd) for path in VERSION_STAMP_PATHS}
    )
    if stamped != version:
        fail(f"coupled stamps {stamped} disagree with pending target {version}")
    return {
        "baseline": baseline.version,
        "version": version,
        "tag": f"v{version}",
        "pending": pending is not None,
        "impacts": impacts,
    }


def merge_tree(base: str, head: str, *, cwd: Path) -> str:
    result = run(["git", "merge-tree", "--write-tree", base, head], cwd=cwd, check=False)
    if result.returncode != 0:
        fail(f"proposed merge is not clean: {result.stderr.strip() or result.stdout.strip()}")
    tree = result.stdout.splitlines()[0] if result.stdout else ""
    if not re.fullmatch(r"[0-9a-f]{40}", tree):
        fail("git did not produce a proposed merge tree")
    return tree


def section_bytes(text: str, version: str) -> str:
    section = next(
        (item for item in changelog_sections(text) if item.version == version), None
    )
    if section is None:
        fail(f"CHANGELOG.md lacks section {version}")
    return text[section.start : section.end].rstrip()


def published_snapshot(
    text: str, through: str, *, allow_legacy_unreleased: bool = False
) -> str:
    sections = []
    for section in changelog_sections(
        text, allow_legacy_unreleased=allow_legacy_unreleased
    ):
        if semver(section.version) <= semver(through):
            raw = text[section.start : section.end].rstrip()
            raw = re.sub(r"\n(?:\[[^\]]+\]: [^\n]+\n?)+\Z", "", raw).rstrip()
            sections.append(raw)
    return "\n\n".join(sections)


def validate_published_sections(
    text: str, baseline: Baseline, config: dict[str, Any], *, cwd: Path
) -> None:
    bootstrap_version = str(config["bootstrap"]["published"].get("version", ""))
    current = published_snapshot(text, baseline.version)
    if baseline.version == bootstrap_version:
        expected = config["bootstrap"]["published"].get("changelog_sha256")
        if not isinstance(expected, str) or hashlib.sha256(current.encode()).hexdigest() != expected:
            fail("published changelog sections differ from the bounded historical bootstrap")
        return
    source_text = file_at_ref(baseline.source, "CHANGELOG.md", cwd=cwd).decode()
    source = published_snapshot(
        source_text, baseline.version, allow_legacy_unreleased=True
    )
    if not source or current != source:
        fail("published changelog sections differ from their exact public source")


def check_pr(args: argparse.Namespace) -> None:
    config, state = load_config(args.config), get_host_state(args)
    target = git("rev-parse", f"{args.target_ref}^{{commit}}", cwd=args.root)
    base = git("rev-parse", f"{args.base}^{{commit}}", cwd=args.root)
    head = git("rev-parse", f"{args.head}^{{commit}}", cwd=args.root)
    if base != target or git("merge-base", base, head, cwd=args.root) != base:
        fail("PR base is stale relative to current target; refresh and recompute")
    body = (
        args.body_file.read_text(encoding="utf-8")
        if args.body_file
        else os.getenv(args.body_env, "")
    )
    fields = parse_release_impact(body)
    floor = commit_impact(base, head, cwd=args.root)
    if IMPACT_ORDER[fields["impact"]] < IMPACT_ORDER[floor]:
        changed = set(changed_paths(base, head, cwd=args.root))
        metadata = set(VERSION_STAMP_PATHS) | {"CHANGELOG.md", ".release/config.json"}
        if fields["impact"] != "none" or changed - metadata:
            fail(f"declared impact {fields['impact']} is below marker floor {floor}")
    paths = changed_paths(base, head, cwd=args.root)
    watched = sorted(
        path for path in paths if matches_any(path, config["watched_contract_paths"])
    )
    if watched and fields["contract"] == "not-applicable":
        fail("watched contract changes require compatible or breaking assessment")
    if fields["contract"] == "breaking" and fields["impact"] != "major":
        fail("breaking contract requires major impact")
    if fields["impact"] == "major" and is_placeholder(fields.get("migration", "")):
        fail("major impact requires migration guidance")
    proposed = merge_tree(base, head, cwd=args.root)
    after = validate_release_tree(proposed, config, state, cwd=args.root)
    baseline = resolve_baseline(config, state, cwd=args.root)
    before_text = file_at_ref(base, "CHANGELOG.md", cwd=args.root).decode()
    after_text = file_at_ref(proposed, "CHANGELOG.md", cwd=args.root).decode()
    adopting = "[Unreleased]" in before_text and "legacy-group=" in after_text
    if adopting:
        before = {"version": after["version"]}
        comparable_before = re.sub(r"(?m)^## \[Unreleased\]\s*$", "", before_text)
    else:
        before = validate_release_tree(base, config, state, cwd=args.root)
        comparable_before = before_text
    for section in changelog_sections(comparable_before):
        if (
            semver(section.version) <= semver(baseline.version)
            and section_bytes(comparable_before, section.version)
            != section_bytes(after_text, section.version)
        ):
            fail("published changelog sections are frozen; label a true erratum outside them")
    before_section = next(
        (item for item in changelog_sections(comparable_before) if semver(item.version) > semver(baseline.version)),
        None,
    )
    after_section = next(
        (item for item in changelog_sections(after_text) if semver(item.version) > semver(baseline.version)),
        None,
    )
    before_entries = annotated_entries(before_section, config) if before_section and not adopting else []
    after_entries = annotated_entries(after_section, config) if after_section else []
    removed, added = list(before_entries), list(after_entries)
    for entry in after_entries:
        if entry in removed:
            removed.remove(entry)
    for entry in before_entries:
        if entry in added:
            added.remove(entry)
    # Pair same-label replacements without guessing whether prose is "similar".
    edited_labels = False
    for impact in IMPACT_ORDER:
        old = [entry for entry in removed if entry[0] == impact]
        new = [entry for entry in added if entry[0] == impact]
        for _ in range(min(len(old), len(new))):
            removed.remove(old.pop())
            added.remove(new.pop())
            edited_labels = True
    declared_additions = [entry for entry in added if not (adopting and entry[1].startswith("legacy:"))]
    added_impact = max(
        (item[0] for item in declared_additions),
        key=lambda item: IMPACT_ORDER[item],
        default="none",
    )
    if fields["impact"] != added_impact:
        fail(
            f"declared impact {fields['impact']} must equal newly added changelog impact "
            f"{added_impact}"
        )
    if edited_labels and fields["impact"] == "none":
        eligible = all(path == "CHANGELOG.md" or path.startswith("docs/") for path in paths)
        if not eligible:
            fail("no-impact pending-note refinement is limited to documentation changes")
    if fields["impact"] != "none":
        if "CHANGELOG.md" not in paths or "changelog" not in fields["evidence"].casefold():
            fail("non-none impact requires curated changelog change and evidence")
    if (
        removed or semver(after["version"]) < semver(before["version"])
    ) and is_placeholder(fields.get("withdrawal", "")):
        fail("removing pending content or lowering target requires withdrawal rationale")
    print(
        json.dumps(
            {
                "status": "ok",
                "declared": fields["impact"],
                "version": after["version"],
                "watched": watched,
            },
            sort_keys=True,
        )
    )


def replace_workspace_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$.*?(?=^\[|\Z)", text)
    if not section:
        fail("Cargo.toml lacks [workspace.package]")
    replacement, count = re.subn(
        r'(?m)^version\s*=\s*"[^"]+"\s*$', f'version = "{version}"', section.group(0)
    )
    if count != 1:
        fail("workspace package must contain exactly one version")
    path.write_text(text[: section.start()] + replacement + text[section.end() :], encoding="utf-8")


def validate_metadata_paths(root: Path) -> None:
    """Keep the fixed sync destinations inside the selected repository."""
    repository = root.resolve()
    for relative in ["CHANGELOG.md", *VERSION_STAMP_PATHS]:
        destination = root / relative
        try:
            resolved_parent = destination.parent.resolve(strict=True)
            resolved = destination.resolve(strict=True)
        except OSError as error:
            fail(f"cannot resolve release metadata path {relative}: {error}")
        if not resolved_parent.is_relative_to(repository) or not resolved.is_relative_to(repository):
            fail(f"release metadata path escapes repository: {relative}")


def sync(args: argparse.Namespace) -> None:
    # Resolve and validate every metadata input before the first write.
    config, state = load_config(args.config), get_host_state(args)
    baseline = resolve_baseline(config, state, cwd=args.root)
    validate_metadata_paths(args.root)
    changelog = args.root / "CHANGELOG.md"
    text = changelog.read_text(encoding="utf-8")
    validate_published_sections(text, baseline, config, cwd=args.root)
    version, pending, _ = expected_pending(text, baseline, config, strict=False)
    files = {path: (args.root / path).read_bytes() for path in VERSION_STAMP_PATHS}
    values = version_stamp_values(files)
    current = version if set(values.values()) == {version} else None
    synced_text = text
    if pending is not None and pending.version != version:
        heading = text[pending.start : pending.body_start]
        synced_text = (
            text[: pending.start]
            + heading.replace(f"[{pending.version}]", f"[{version}]", 1)
            + text[pending.body_start :]
        )
    if synced_text != text:
        changelog.write_text(synced_text, encoding="utf-8")
    if current != version:
        replace_workspace_version(args.root / "Cargo.toml", version)
        run([args.cargo, "check", "--workspace"], cwd=args.root)
        run([args.cargo, "build", "--locked", "-p", "codeflow-cli"], cwd=args.root)
        binary = args.root / "target/debug" / ("codeflow.exe" if os.name == "nt" else "codeflow")
        if run([str(binary), "--version"], cwd=args.root).stdout.strip() != f"codeflow {version}":
            fail("rebuilt updater version disagrees with pending target")
        run([str(binary), "update"], cwd=args.root)
        stamped = validate_version_stamps(
            {path: (args.root / path).read_bytes() for path in VERSION_STAMP_PATHS}
        )
        if stamped != version:
            fail(f"sync left stamps at {stamped}, expected {version}")
    print(json.dumps({"status": "synchronized", "baseline": baseline.version, "version": version}, sort_keys=True))


def check_state(args: argparse.Namespace) -> None:
    result = validate_release_tree(
        args.ref, load_config(args.config), get_host_state(args), cwd=args.root
    )
    print(json.dumps({"status": "ok", **result}, sort_keys=True))


def release_notes(root: Path, ref: str, tag: str, source: str) -> str:
    text = file_at_ref(ref, "CHANGELOG.md", cwd=root).decode()
    section = next(
        (item for item in changelog_sections(text) if item.version == tag.removeprefix("v")),
        None,
    )
    if section is None or not section.body:
        fail(f"CHANGELOG.md lacks curated notes for {tag}")
    body = IMPACT_MARKER.sub("", section.body)
    body = body.replace(LEGACY_END, "")
    body = re.sub(r"(?m)^_Staging evidence:.*_\s*$", "", body)
    body = re.sub(r"\n{3,}", "\n\n", body).strip()
    return f"{body}\n\n{SOURCE_MARKER.format(source=source)}\n"


def write_release_notes(args: argparse.Namespace) -> None:
    args.output.write_text(
        release_notes(args.root, args.ref, args.tag, args.source), encoding="utf-8"
    )
    print(json.dumps({"status": "prepared", "output": str(args.output)}))


def verify_checks(args: argparse.Namespace) -> None:
    pages = load_json(args.state)
    required = load_config(args.config).get("required_publication_checks")
    if (
        not isinstance(pages, list)
        or not isinstance(required, list)
        or not required
        or len(set(required)) != len(required)
        or any(not isinstance(name, str) or not name for name in required)
    ):
        fail("publication check inventory or configured requirements are malformed")
    runs: list[Any] = []
    for page in pages:
        if not isinstance(page, dict) or not isinstance(page.get("check_runs"), list):
            fail("publication check page is malformed")
        runs.extend(page["check_runs"])
    passed = {
        run.get("name")
        for run in runs
        if isinstance(run, dict)
        and run.get("head_sha") == args.source
        and run.get("status") == "completed"
        and run.get("conclusion") == "success"
    }
    missing = [name for name in required if name not in passed]
    if missing:
        fail(f"selected source lacks successful required checks: {', '.join(missing)}")
    print(json.dumps({"status": "verified", "checks": required}, sort_keys=True))


def verify_publication(args: argparse.Namespace) -> None:
    config, state = load_config(args.config), get_host_state(args)
    if not state["drafts_visible"]:
        fail("publication verification requires write-visible draft inventory")
    source = git("rev-parse", f"{args.source}^{{commit}}", cwd=args.root)
    main = (
        args.main_source
        if args.main_source
        else git("rev-parse", f"{args.main_ref}^{{commit}}", cwd=args.root)
    )
    if not re.fullmatch(r"[0-9a-f]{40}", main):
        fail("current main source is malformed")
    if source != main:
        fail("selected snapshot is no longer current main; redispatch")
    result = validate_release_tree(
        source, config, state, cwd=args.root, allow_attempt=(args.tag, source)
    )
    if not result["pending"] or args.tag != result["tag"]:
        fail("requested tag does not match selected main pending state")
    print(json.dumps({"status": "authorized", "source": source, "tag": args.tag}, sort_keys=True))


def verify_authority(args: argparse.Namespace) -> None:
    event = load_json(args.event)
    users = load_json(args.users)
    permissions = load_json(args.permissions)
    pulls = load_json(args.pulls)
    if not all(isinstance(value, dict) for value in [event, users, permissions]) or not isinstance(pulls, list):
        fail("authority fixtures are malformed")
    if event.get("event_name") != "workflow_dispatch" or event.get("ref") != "refs/heads/main":
        fail("publication requires explicit workflow_dispatch on main")
    for role in ["actor", "triggering_actor"]:
        login = event.get(role)
        if (
            not isinstance(login, str)
            or users.get(login) != "User"
            or permissions.get(login) not in {"write", "maintain", "admin"}
        ):
            fail(f"{role} must be a GitHub User with current write, maintain, or admin permission")
    matches = [
        pull
        for pull in pulls
        if isinstance(pull, dict)
        and pull.get("merged_at")
        and pull.get("state") == "closed"
        and (pull.get("base") or {}).get("ref") == "main"
        and ((pull.get("base") or {}).get("repo") or {}).get("full_name") == args.repository
        and pull.get("merge_commit_sha") == args.source
        and (pull.get("merged_by") or {}).get("type") == "User"
    ]
    if not matches:
        fail("selected main source lacks an ordinary human-merged same-repository PR")
    print(json.dumps({"status": "authorized", "source": args.source}, sort_keys=True))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_host_state(args: argparse.Namespace) -> None:
    state = load_json(args.state)
    expected_notes = args.notes.read_text(encoding="utf-8").strip()
    tag_target, release = state.get("tag_target"), state.get("release")
    if tag_target is not None and tag_target != args.source:
        fail("existing tag does not target selected source")
    if release is None:
        print(json.dumps({"status": "fresh" if tag_target is None else "resume-tag"}))
        return
    if not isinstance(release, dict) or release.get("draft") is not True:
        fail("release is public; verify it separately and never overwrite")
    if not isinstance(release.get("assets", []), list) or release.get("assets"):
        fail("partial draft assets cannot be proven byte-identical; refuse overwrite")
    if (
        release.get("tag_name") != args.tag
        or release.get("name") != args.tag
        or release.get("target_commitish") != args.source
    ):
        fail("empty draft identity disagrees with selected release")
    if not isinstance(release.get("body"), str) or release["body"].strip() != expected_notes:
        fail("empty draft notes disagree with reviewed notes")
    print(json.dumps({"status": "resume-draft"}))


def verify_published_assets(args: argparse.Namespace) -> None:
    state = load_json(args.state)
    release = state.get("release")
    if state.get("tag_target") != args.source:
        fail("published tag does not resolve to selected source")
    if (
        not isinstance(release, dict)
        or release.get("draft") is not False
        or release.get("tag_name") != args.tag
        or release.get("target_commitish") != args.source
    ):
        fail("published release identity disagrees with selected source")
    expected: dict[str, dict[str, Any]] = {}
    for path in sorted(args.artifacts_dir.iterdir()):
        if path.name.endswith("-dist-manifest.json"):
            continue
        if path.is_symlink() or not path.is_file():
            fail(f"artifact is not a regular file: {path.name}")
        expected[path.name] = {"size": path.stat().st_size, "digest": f"sha256:{sha256(path)}"}
    if not expected:
        fail("artifact set is empty")
    actual: dict[str, dict[str, Any]] = {}
    if not isinstance(release.get("assets"), list):
        fail("published assets are missing")
    for asset in release["assets"]:
        if (
            not isinstance(asset, dict)
            or not isinstance(asset.get("name"), str)
            or asset["name"] in actual
            or asset.get("state") != "uploaded"
        ):
            fail("published asset metadata is malformed, duplicate, or incomplete")
        actual[asset["name"]] = {"size": asset.get("size"), "digest": asset.get("digest")}
    if actual != expected:
        fail(f"published assets disagree with same-run files: expected={expected}, actual={actual}")
    print(json.dumps({"status": "verified", "tag": args.tag, "assets": len(actual)}))


def add_host_args(value: argparse.ArgumentParser) -> None:
    value.add_argument("--host-state", type=Path)
    value.add_argument("--repository", default="")
    value.add_argument("--drafts-visible", action="store_true")


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--root", type=Path, default=ROOT)
    value.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    sub = value.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check-pr")
    check.add_argument("--base", required=True)
    check.add_argument("--head", required=True)
    check.add_argument("--target-ref", required=True)
    source = check.add_mutually_exclusive_group(required=True)
    source.add_argument("--body-file", type=Path)
    source.add_argument("--body-env")
    add_host_args(check)
    check.set_defaults(func=check_pr)

    prepare = sub.add_parser("sync")
    prepare.add_argument("--cargo", default="cargo")
    add_host_args(prepare)
    prepare.set_defaults(func=sync)

    state = sub.add_parser("check-state")
    state.add_argument("--ref", default="HEAD")
    add_host_args(state)
    state.set_defaults(func=check_state)

    publication = sub.add_parser("verify-publication")
    publication.add_argument("--source", required=True)
    current_main = publication.add_mutually_exclusive_group(required=True)
    current_main.add_argument("--main-ref")
    current_main.add_argument("--main-source")
    publication.add_argument("--tag", required=True)
    add_host_args(publication)
    publication.set_defaults(func=verify_publication)

    checks = sub.add_parser("verify-checks")
    checks.add_argument("--state", type=Path, required=True)
    checks.add_argument("--source", required=True)
    checks.set_defaults(func=verify_checks)

    authority = sub.add_parser("verify-authority")
    authority.add_argument("--event", type=Path, required=True)
    authority.add_argument("--users", type=Path, required=True)
    authority.add_argument("--permissions", type=Path, required=True)
    authority.add_argument("--pulls", type=Path, required=True)
    authority.add_argument("--repository", required=True)
    authority.add_argument("--source", required=True)
    authority.set_defaults(func=verify_authority)

    notes = sub.add_parser("release-notes")
    notes.add_argument("--ref", default="HEAD")
    notes.add_argument("--source", required=True)
    notes.add_argument("--tag", required=True)
    notes.add_argument("--output", type=Path, required=True)
    notes.set_defaults(func=write_release_notes)

    host = sub.add_parser("verify-host-state")
    host.add_argument("--state", type=Path, required=True)
    host.add_argument("--source", required=True)
    host.add_argument("--tag", required=True)
    host.add_argument("--notes", type=Path, required=True)
    host.set_defaults(func=verify_host_state)

    published = sub.add_parser("verify-published-assets")
    published.add_argument("--state", type=Path, required=True)
    published.add_argument("--artifacts-dir", type=Path, required=True)
    published.add_argument("--source", required=True)
    published.add_argument("--tag", required=True)
    published.set_defaults(func=verify_published_assets)
    return value


def main() -> int:
    args = parser().parse_args()
    args.root = args.root.resolve()
    if not args.config.is_absolute():
        args.config = args.root / args.config
    try:
        args.func(args)
    except (ReleaseError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"release error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
