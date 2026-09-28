#!/usr/bin/env python3
"""Repository-owned same-PR release-state sync and verification."""
from __future__ import annotations

import argparse
from collections import Counter
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
CONFIG_PATH = ".release/config.json"
DEFAULT_CONFIG = ROOT / CONFIG_PATH
# The one path-set table (SPC-013 R-114): the adopter-facing set, the
# direct-change floor and the behaviour paths of the local preflight (R-93).
PATH_SETS = "crates/codeflow-core/src/workgraph/path_sets.toml"
HOST_UNCHECKED = "not checked against the host"
IMPACT_ORDER = {"none": 0, "patch": 1, "minor": 2, "major": 3}
PLACEHOLDERS = {"", "n/a", "na", "none", "todo", "tbd", "-"}
CONTRACT_BREAKING = {"not-applicable": "no", "compatible": "no", "breaking": "yes"}
# The PR template's Migration choice text; Impact and Breaking alternatives
# already fail their value checks.
UNRESOLVED_ALTERNATIVES = {
    "none, steps, or see breaking change",
    "none | steps | see breaking change",
    "steps",
}
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
    return validate_config(load_json(path))


def config_at_ref(ref: str, *, cwd: Path) -> dict[str, Any]:
    """The release configuration a commit carries."""
    try:
        value = json.loads(file_at_ref(ref, CONFIG_PATH, cwd=cwd))
    except json.JSONDecodeError as error:
        fail(f"cannot read {CONFIG_PATH} at {ref}: {error}")
    return validate_config(value)


def validate_config(value: Any) -> dict[str, Any]:
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
    return not substantive_text(value, [])


def guidance_text(value: str) -> str:
    """Casefolded text with Markdown quoting and repeated whitespace removed."""
    return " ".join(re.sub(r"[`\"']", "", value).split()).casefold()


def is_unresolved_alternative(value: str) -> bool:
    """The PR template's choice text left in place instead of a chosen value."""
    return guidance_text(value) in UNRESOLVED_ALTERNATIVES


def substantive_text(value: str, rejected: list[str] | set[str]) -> bool:
    """Real text: not empty, not a whole `<placeholder>`, not a placeholder
    word, and not one of `rejected`. The Rust PR-body check applies the same
    rule (`crates/codeflow-cli/src/cmd/ci/pr_body.rs`)."""
    normalized = guidance_text(value)
    whole_placeholder = (
        normalized.startswith("<")
        and normalized.endswith(">")
        and not re.search(r"[<>]", normalized[1:-1])
    )
    return (
        not whole_placeholder
        and any(character.isalnum() for character in normalized)
        and normalized not in PLACEHOLDERS
        and normalized not in rejected
    )


def lacks_migration_guidance(value: str) -> bool:
    return not substantive_text(value, UNRESOLVED_ALTERNATIVES | {"see breaking change"})


# Markdown structure of a PR body, read the way the Rust PR-body check reads
# it: fenced code and HTML comments are never fields, only ATX headings at
# column zero open sections, and a section's fields end at its first
# subsection. `scripts/fixtures/release_impact_cases.json` holds the cases
# both parsers must agree on.
FENCE_OPEN = re.compile(r"^ {0,3}(`{3,}|~{3,})")
ATX_HEADING = re.compile(r"^(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$")


def markdown_lines(body: str, *, include_code: bool) -> list[tuple[str, tuple[int, str] | None]]:
    """Visible lines, each with its (depth, name) when it is a heading."""
    lines: list[tuple[str, tuple[int, str] | None]] = []
    fence: str | None = None
    in_comment = False
    for raw in body.splitlines():
        line = raw
        if fence is not None:
            closing = re.match(rf"^ {{0,3}}{re.escape(fence[0])}{{{len(fence)},}}[ \t]*$", line)
            if closing:
                fence = None
            elif include_code:
                lines.append((line, None))
            continue
        if in_comment:
            end = line.find("-->")
            if end < 0:
                continue
            line, in_comment = line[end + 3 :], False
        while (start := line.find("<!--")) >= 0:
            end = line.find("-->", start + 4)
            if end < 0:
                line, in_comment = line[:start], True
                break
            line = line[:start] + line[end + 3 :]
        opening = FENCE_OPEN.match(line)
        if opening:
            fence = opening.group(1)
            continue
        heading = ATX_HEADING.match(line)
        if heading:
            name = re.sub(r"[*_`]", "", heading.group(2) or "").strip()
            lines.append((line, (len(heading.group(1)), name)))
        else:
            lines.append((line, None))
    return lines


def markdown_section(body: str, name: str, *, include_code: bool, own_only: bool) -> list[list[str]]:
    """Each section named `name` (at depth 2, else depth 3) as its lines.

    `own_only` stops at the first subsection; otherwise a section runs to the
    next heading at its depth or above.
    """
    lines = markdown_lines(body, include_code=include_code)
    headings = [
        (index, depth, title)
        for index, (_, heading) in enumerate(lines)
        if heading is not None
        for depth, title in [heading]
    ]
    wanted = name.casefold()
    depth = 2 if any(d == 2 and t.casefold() == wanted for _, d, t in headings) else 3
    found = []
    for index, heading_depth, title in headings:
        if heading_depth != depth or title.casefold() != wanted:
            continue
        end = len(lines)
        for next_index, next_depth, _ in headings:
            if next_index > index and (own_only or next_depth <= depth):
                end = next_index
                break
        found.append([line for line, heading in lines[index + 1 : end] if heading is None])
    return found


RELEASE_FIELDS = {
    "unit", "impact", "breaking", "contract", "rationale", "migration", "evidence", "withdrawal",
}
FIELD_LINE = re.compile(r"^\s*(?:[-*+]|\d+[.)])?\s*([A-Za-z*_ ]+?)[*_]*\s*:[*_]*\s*(.*?)\s*$")


def parse_release_impact(body: str) -> dict[str, str]:
    sections = markdown_section(body, "Release impact", include_code=False, own_only=True)
    if len(sections) != 1:
        fail("PR body must contain exactly one '## Release impact' section")
    fields: dict[str, str] = {}
    for line in sections[0]:
        match = FIELD_LINE.match(line)
        if not match:
            continue
        label = re.sub(r"[*_]", "", match.group(1)).strip()
        if not label:
            continue
        key = label.casefold().replace(" ", "_")
        if key in fields:
            if key in RELEASE_FIELDS:
                fail(f"release impact field {label} is duplicated")
            continue
        fields[key] = normalize_value(re.sub(r"^[*_]+|[*_]+$", "", match.group(2)))
    for key in ["impact", "breaking"]:
        if key in fields:
            fields[key] = fields[key].casefold()
    for key in ["unit", "impact", "rationale", "evidence"]:
        if key not in fields or (
            key in {"rationale", "evidence"} and is_placeholder(fields[key])
        ):
            fail(f"release impact field {key} is required and substantive")
    if fields["unit"] != "codeflow" or fields["impact"] not in IMPACT_ORDER:
        fail("release unit/impact must be codeflow and none, patch, minor, or major")
    # Breaking replaces the legacy three-state Contract field. During the
    # transition either is accepted; when both appear they must agree.
    if "breaking" not in fields and "contract" not in fields:
        fail("release impact field breaking is required")
    if "contract" in fields and fields["contract"] not in CONTRACT_BREAKING:
        fail("release contract must be not-applicable, compatible, or breaking")
    if "breaking" in fields:
        if fields["breaking"] not in {"yes", "no"}:
            fail("release breaking must be yes or no")
        if "contract" in fields and CONTRACT_BREAKING[fields["contract"]] != fields["breaking"]:
            fail("legacy contract disagrees with breaking")
        if not fields.get("migration"):
            fail("release impact field migration is required")
    else:
        fields["breaking"] = CONTRACT_BREAKING[fields["contract"]]
    if "migration" in fields and is_unresolved_alternative(fields["migration"]):
        fail("release impact field migration still holds the template alternatives")
    fields[MIGRATION_GUIDANCE] = "yes" if migration_guidance(body, fields.get("migration", "")) else "no"
    if (fields["breaking"] == "yes") != (fields["impact"] == "major"):
        fail("breaking must be yes if and only if impact is major")
    if fields["breaking"] == "yes" and fields[MIGRATION_GUIDANCE] != "yes":
        fail("a breaking change requires migration guidance")
    return fields


# Parsed, not declared: whether Migration gives real guidance, directly or by
# pointing at one populated Breaking change section.
MIGRATION_GUIDANCE = "_migration_guidance"


def migration_guidance(body: str, migration: str) -> bool:
    if guidance_text(migration) == "see breaking change":
        sections = markdown_section(body, "Breaking change", include_code=True, own_only=False)
        return len(sections) == 1 and substantive_text("\n".join(sections[0]), [])
    return not lacks_migration_guidance(migration)


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
                "name": item.get("name"),
                "target": item.get("target_commitish"),
                "body": item.get("body"),
                "assets": [
                    {
                        "name": asset.get("name"),
                        "size": asset.get("size"),
                        "state": asset.get("state"),
                        "digest": asset.get("digest"),
                    }
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


def bootstrap_shape(config: dict[str, Any]) -> None:
    """Fail when the bootstrap record is not in the shape this checker reads,
    before anything is compared with the repository or the host."""
    published = config["bootstrap"].get("published")
    comparison = config["bootstrap"].get("comparison")
    if not isinstance(published, dict) or not isinstance(comparison, dict):
        fail("bootstrap must record published and distinct comparison provenance")
    version, tag = str(published.get("version", "")), comparison.get("tag")
    tree, source = comparison.get("tree"), published.get("source_commit")
    if (
        tag != f"v{version}"
        or not isinstance(tree, str)
        or not re.fullmatch(r"[0-9a-f]{40}", tree)
        or not isinstance(source, str)
    ):
        fail("bootstrap version, comparison tag, tree, and source are malformed")
    semver(version)
    if not re.fullmatch(r"[0-9a-f]{64}", str(published.get("source_archive_sha256", ""))):
        fail("bootstrap archive checksum must be 64 lowercase hex characters")


def validate_bootstrap(config: dict[str, Any], *, cwd: Path) -> Baseline:
    bootstrap_shape(config)
    published, comparison = config["bootstrap"]["published"], config["bootstrap"]["comparison"]
    version, tag = str(published["version"]), comparison["tag"]
    tree, source = comparison["tree"], published["source_commit"]
    # The tag is identified by its content. A history rewrite that keeps the
    # tagged tree (the public repository's path filter) changes the commit
    # id, never the content every later check compares against.
    actual = git("rev-parse", f"{tag}^{{commit}}", cwd=cwd)
    actual_tree = git("rev-parse", f"{actual}^{{tree}}", cwd=cwd)
    if actual_tree != tree:
        fail(f"bootstrap tag {tag} resolves to tree {actual_tree}, expected {tree}; never move it")
    return Baseline(version, tag, source, actual)


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
    # The published source archive digest, not the mutable release target
    # string, identifies the bootstrap release on every host that carries it.
    archive_digest = f"sha256:{config['bootstrap']['published']['source_archive_sha256']}"
    if (
        bootstrap_release is None
        or tags.get(bootstrap.tag) != bootstrap.comparison_commit
        or not any(
            isinstance(asset, dict)
            and asset.get("name") == "source.tar.gz"
            and asset.get("digest") == archive_digest
            for asset in bootstrap_release.get("assets") or []
        )
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
        drafts = [
            item
            for item in releases
            if isinstance(item, dict)
            and item.get("draft") is True
            and STABLE_TAG.fullmatch(str(item.get("tag", "")))
        ]
        draft_counts = Counter(str(item["tag"]) for item in drafts)
        duplicates = sorted(tag for tag, count in draft_counts.items() if count > 1)
        if duplicates:
            fail(f"duplicate draft releases are ambiguous: {', '.join(duplicates)}")
        for item in drafts:
            tag = str(item.get("tag", ""))
            if semver(tag) > semver(baseline.version):
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


@dataclass(frozen=True)
class PendingItem:
    """One pending changelog item and its identity (R-92): the bold label of
    an ordinary entry, or the legacy group's explicit id."""

    key: str
    label: str
    impact: str
    text: str


ENTRY_LABEL = re.compile(r"- \*\*(.+?)\*\*", re.S)


def entry_label(entry: str) -> str:
    match = ENTRY_LABEL.match(entry)
    label = " ".join(match.group(1).split()) if match else ""
    if not label:
        first = entry.splitlines()[0] if entry else ""
        fail(f"a pending changelog entry needs a bold label (- **Label.** ...): {first[:72]}")
    return label


def label_key(label: str) -> str:
    return label.rstrip(".").casefold()


# A line that starts a new Markdown block, so it can never be a lazy
# continuation of the entry's paragraph (CommonMark 0.31.2, 5.2).
BLOCK_START = re.compile(
    r"#{1,6}(?:\s|$)|[-*+](?:\s|$)|\d{1,9}[.)](?:\s|$)|>|<!--|```|~~~"
    r"|(?:-[ \t]*){3,}$|(?:\*[ \t]*){3,}$|(?:_[ \t]*){3,}$"
)


def entry_extent(text: str) -> str:
    """One bullet, as Markdown renders it: its first line, the indented or
    blank lines after it, and lazy continuation lines of its paragraph. It
    ends at a blank line followed by an unindented line, or at an unindented
    line that starts a new block (a heading, list item, quote, fence, rule
    or marker)."""
    lines = text.splitlines()
    kept = lines[:1]
    for line in lines[1:]:
        unindented = bool(line.strip()) and not line[0].isspace()
        if unindented and (not kept[-1].strip() or BLOCK_START.match(line)):
            break
        kept.append(line)
    return "\n".join(kept).strip()


def pending_items(section: ChangelogSection, config: dict[str, Any]) -> dict[str, PendingItem]:
    items: dict[str, PendingItem] = {}
    for impact, text in annotated_entries(section, config):
        if text.startswith("legacy:"):
            # The group's bytes are pinned by its digest in the configuration.
            key = label = body = "legacy:" + text.split(":")[1]
        else:
            label, body = entry_label(text), entry_extent(text)
            key = label_key(label)
        if key in items:
            fail(f"pending entry label '{label}' is not unique; each pending entry needs its own label")
        items[key] = PendingItem(key, label, impact, body)
    return items


def lenient_items(section: ChangelogSection) -> dict[str, PendingItem]:
    """The labelled items of a pending section that fails validation, read
    marker by marker, for assessing a typed repair against its broken base."""
    items: dict[str, PendingItem] = {}
    for marker in IMPACT_MARKER.finditer(section.body):
        if marker.group(2):
            key = "legacy:" + marker.group(2)
            items.setdefault(key, PendingItem(key, key, marker.group(1), key))
            continue
        text = section.body[marker.end() :].lstrip()
        match = ENTRY_LABEL.match(text)
        if match:
            label = " ".join(match.group(1).split())
            body = entry_extent(text)
            items.setdefault(label_key(label), PendingItem(label_key(label), label, marker.group(1), body))
    return items


def lenient_target(section: ChangelogSection, baseline: Baseline) -> str | None:
    impacts = [match.group(1) for match in IMPACT_MARKER.finditer(section.body)]
    if not impacts:
        return None
    return bump(baseline.version, max(impacts, key=lambda item: IMPACT_ORDER[item]))


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
    pending_items(section, config)
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
    validate_errata(changelog)
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


ERRATA_HEADING = re.compile(r"(?m)^## Errata[ \t]*$")
ERRATUM = re.compile(r"- (\d{4}-\d{2}-\d{2}), (\d+\.\d+\.\d+): \S")


def validate_errata(text: str) -> None:
    """R-96: an erratum is a dated note in the `## Errata` block before the
    first version section, outside every frozen byte. It names the published
    version it corrects."""
    headings = list(ERRATA_HEADING.finditer(text))
    if not headings:
        return
    first = re.search(r"(?m)^## \[", text)
    if len(headings) > 1 or first is None or headings[0].start() > first.start():
        fail("CHANGELOG.md errata live in one `## Errata` block before the first version section")
    block = text[headings[0].end() : first.start()]
    published = {
        section.version for section in changelog_sections(text) if section.date is not None
    }
    for line in block.splitlines():
        if not line.startswith("- "):
            continue
        match = ERRATUM.match(line)
        if not match:
            fail(f"an erratum reads `- YYYY-MM-DD, X.Y.Z: note`: {line[:72]}")
        if match.group(2) not in published:
            fail(f"erratum names {match.group(2)}, which is not a published section")


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
        metadata = set(VERSION_STAMP_PATHS) | {"CHANGELOG.md", CONFIG_PATH}
        if fields["impact"] != "none" or changed - metadata:
            fail(f"declared impact {fields['impact']} is below marker floor {floor}")
    paths = changed_paths(base, head, cwd=args.root)
    watched = sorted(
        path for path in paths if matches_any(path, config["watched_contract_paths"])
    )
    if watched and fields.get("contract") == "not-applicable":
        fail("watched contract changes require compatible or breaking assessment")
    proposed = merge_tree(base, head, cwd=args.root)
    base_config, base_configuration = carried_config(base, config, cwd=args.root)
    before_text = file_at_ref(base, "CHANGELOG.md", cwd=args.root).decode()
    after_text = file_at_ref(proposed, "CHANGELOG.md", cwd=args.root).decode()
    adopting = bool(re.search(r"(?m)^## \[Unreleased\]\s*$", before_text)) and "legacy-group=" in after_text
    repair: str | None = None
    if adopting:
        after = validate_release_tree(proposed, config, state, cwd=args.root)
        before_version: str | None = after["version"]
        comparable_before = re.sub(r"(?m)^## \[Unreleased\]\s*$", "", before_text)
    else:
        try:
            before_version = validate_release_tree(base, base_config, state, cwd=args.root)["version"]
        except ReleaseError as error:
            # R-95: a base that fails its own release state is repaired only
            # by a typed repair, judged with the base's configuration.
            config = base_config
            repair = typed_repair(base, head, proposed, paths, config, str(error), cwd=args.root)
            before_version = None
        after = validate_release_tree(proposed, config, state, cwd=args.root)
        if repair is not None:
            repair_baselines(base, proposed, paths, after["version"], repair, cwd=args.root)
        comparable_before = before_text
    baseline = resolve_baseline(config, state, cwd=args.root)
    if repair is None:
        for section in changelog_sections(comparable_before):
            if (
                semver(section.version) <= semver(baseline.version)
                and section_bytes(comparable_before, section.version)
                != section_bytes(after_text, section.version)
            ):
                fail("published changelog sections are frozen; label a true erratum outside them")
    # A repair's published sections are held to their exact public source by
    # the proposed tree's own validation above, which lets a repair restore
    # frozen bytes and never change them.
    before_section = pending_section(comparable_before, baseline, repair=repair is not None)
    after_section = pending_section(after_text, baseline, repair=False)
    if before_section is None or adopting:
        before_items: dict[str, PendingItem] = {}
    elif repair is not None:
        before_items = lenient_items(before_section)
        if before_version is None:
            before_version = lenient_target(before_section, baseline)
    else:
        before_items = pending_items(before_section, config)
    after_items = pending_items(after_section, config) if after_section else {}
    change = entry_change(before_items, after_items, adopting=adopting)
    assessed_impact = max(
        (item.impact for item in change.added + [after for _, after in change.edited]),
        key=lambda item: IMPACT_ORDER[item],
        default="none",
    )
    if repair is not None and change.edited:
        # A repair restores the release state; it never rewrites what an
        # existing entry says (R-95). Edits wait for their own pull request.
        fail(
            f"base release state is invalid ({repair}); a repair keeps every existing pending "
            f"entry's text and impact, and this PR edits: "
            f"{', '.join(after.label for _, after in change.edited)}"
        )
    if fields["impact"] != assessed_impact:
        fail(
            f"declared impact {fields['impact']} must equal the impact of the pending entries "
            f"this PR adds or edits ({assessed_impact}); an edit under an existing label is "
            "assessed at its impact, and any change to its text is an edit"
        )
    if assessed_impact == "major" and fields[MIGRATION_GUIDANCE] != "yes":
        fail("an added or edited major entry requires migration guidance")
    if fields["impact"] != "none":
        if "CHANGELOG.md" not in paths or "changelog" not in fields["evidence"].casefold():
            fail("non-none impact requires curated changelog change and evidence")
    lowered = [
        after.label
        for before, after in change.edited
        if IMPACT_ORDER[after.impact] < IMPACT_ORDER[before.impact]
    ]
    target_lowered = before_version is not None and semver(after["version"]) < semver(before_version)
    if (change.removed or lowered or target_lowered) and is_placeholder(fields.get("withdrawal", "")):
        fail("removing pending content or lowering target requires withdrawal rationale")
    result = {
        "status": "ok",
        "declared": fields["impact"],
        "version": after["version"],
        "watched": watched,
        "added": [item.label for item in change.added],
        "edited": [after.label for _, after in change.edited],
        "withdrawn": [item.label for item in change.removed],
    }
    if repair is not None:
        result["repair"] = repair
    if base_configuration is not None:
        result["base_configuration"] = base_configuration
    print(json.dumps(result, sort_keys=True))


def carried_config(
    base: str, config: dict[str, Any], *, cwd: Path
) -> tuple[dict[str, Any], str | None]:
    """The configuration the base is judged by (R-95): always the one it
    carries, so a pull request never supplies the authority for the history
    it is judged against. One older shape is read: a bootstrap record
    without its comparison tree, whose tree is derived from the recorded
    comparison commit once the tag corroborates it. Any other configuration
    this checker cannot read refuses the pull request. A base with no
    configuration has no authority to replace; the pull request adopts one."""
    if file_at_optional(base, CONFIG_PATH, cwd=cwd) is None:
        return config, "the pull request's; the base carries none, so this pull request adopts it"
    try:
        carried = config_at_ref(base, cwd=cwd)
        note = None
        comparison = carried["bootstrap"].get("comparison")
        if isinstance(comparison, dict) and "tree" not in comparison:
            carried = with_comparison_tree(carried, cwd=cwd)
            note = "the base's, its comparison tree derived from the recorded comparison commit"
        bootstrap_shape(carried)
    except ReleaseError as error:
        fail(f"the base's release configuration cannot be read, and a pull request never replaces it: {error}")
    return carried, note


def with_comparison_tree(config: dict[str, Any], *, cwd: Path) -> dict[str, Any]:
    """`config` with the tree of its recorded comparison commit, when that
    commit exists and the recorded tag carries the same tree."""
    comparison = config["bootstrap"]["comparison"]
    commit, tag = comparison.get("commit"), comparison.get("tag")
    if not isinstance(commit, str) or not re.fullmatch(r"[0-9a-f]{40}", commit) or not isinstance(tag, str):
        fail("bootstrap records neither a comparison tree nor a comparison commit and tag")
    found = run(["git", "rev-parse", "--verify", "--quiet", f"{commit}^{{tree}}"], cwd=cwd, check=False)
    tagged = run(["git", "rev-parse", "--verify", "--quiet", f"{tag}^{{tree}}"], cwd=cwd, check=False)
    tree = found.stdout.strip()
    if found.returncode != 0 or tagged.returncode != 0 or tagged.stdout.strip() != tree:
        fail(f"bootstrap comparison commit {commit[:12]} does not corroborate tag {tag}")
    migrated = json.loads(json.dumps(config))
    migrated["bootstrap"]["comparison"]["tree"] = tree
    return migrated


def pending_section(text: str, baseline: Baseline, *, repair: bool) -> ChangelogSection | None:
    """The section above the published baseline. A repaired base may hold a
    malformed changelog; its first pending section is read as far as it can be."""
    try:
        sections = changelog_sections(text)
    except ReleaseError:
        if not repair:
            raise
        return None
    return next(
        (item for item in sections if semver(item.version) > semver(baseline.version)),
        None,
    )


@dataclass(frozen=True)
class EntryChange:
    added: list[PendingItem]
    removed: list[PendingItem]
    edited: list[tuple[PendingItem, PendingItem]]


def entry_change(
    before: dict[str, PendingItem], after: dict[str, PendingItem], *, adopting: bool
) -> EntryChange:
    """Pending entries matched by identity (R-92): a label new to the section
    is an addition, a label gone is a withdrawal, and a changed body or impact
    under a kept label is an edit of that item. Entries compare byte for byte:
    no whitespace change is treated as neutral, since in code and nested
    Markdown it carries meaning."""
    added = [
        item
        for key, item in after.items()
        if key not in before and not (adopting and item.key.startswith("legacy:"))
    ]
    removed = [item for key, item in before.items() if key not in after]
    edited = [
        (before[key], item)
        for key, item in after.items()
        if key in before
        and (before[key].impact, before[key].text) != (item.impact, item.text)
    ]
    return EntryChange(added, removed, edited)


# The coupled files a typed repair may change, and only in their parsed stamp
# fields. The managed baselines of AGENTS.md and CLAUDE.md, and their hashes in
# the manifest, carry the same stamp: `codeflow update`, which `sync` runs,
# writes them together.
REPAIR_BASELINES = [".codeflow/.baseline/AGENTS.md", ".codeflow/.baseline/CLAUDE.md"]


def typed_repair(
    base: str,
    head: str,
    proposed: str,
    paths: list[str],
    config: dict[str, Any],
    invariant: str,
    *,
    cwd: Path,
) -> str:
    """Accept a PR that repairs a base failing `validate_release_tree` (R-95),
    or fail naming why it is not a repair. The caller then validates the
    proposed tree with the base's configuration."""
    if CONFIG_PATH in paths:
        fail(
            f"base release state is invalid ({invariant}); a repair cannot change "
            f"{CONFIG_PATH}: the configuration is read from the base"
        )
    if config != config_at_ref(head, cwd=cwd):
        fail(f"base release state is invalid ({invariant}); the head's release configuration differs")
    allowed = {"CHANGELOG.md", *VERSION_STAMP_PATHS, *REPAIR_BASELINES}
    other = sorted(path for path in paths if path not in allowed)
    if other:
        fail(
            f"base release state is invalid ({invariant}); a repair changes only CHANGELOG.md "
            f"and coupled version stamps, and this PR also changes: {', '.join(other[:5])}"
        )
    for path in sorted(set(paths) - {"CHANGELOG.md"}):
        before = stamp_neutral(path, file_at_optional(base, path, cwd=cwd))
        after = stamp_neutral(path, file_at_optional(proposed, path, cwd=cwd))
        if before != after:
            fail(
                f"base release state is invalid ({invariant}); a repair changes only the "
                f"version stamp of {path}"
            )
    return invariant


MANAGED_STAMP = re.compile(rb"<!-- codeflow:managed:begin scaffold=(\d+\.\d+\.\d+) -->")


def repair_baselines(
    base: str, proposed: str, paths: list[str], version: str, invariant: str, *, cwd: Path
) -> None:
    """A repair that touches a managed baseline or the manifest leaves the
    three consistent, as `sync` writes them: each baseline carries the one
    managed stamp of the release version, and the manifest records its exact
    hash. Neither may drift from the other or from the live stamps."""
    if not any(path in paths for path in [*REPAIR_BASELINES, ".codeflow/manifest.json"]):
        return
    manifest = json.loads(file_at_ref(proposed, ".codeflow/manifest.json", cwd=cwd))
    files = manifest.get("files") if isinstance(manifest, dict) else None
    for baseline in REPAIR_BASELINES:
        name = baseline.removeprefix(".codeflow/.baseline/")
        actual = file_at_optional(proposed, baseline, cwd=cwd)
        if actual is None:
            if file_at_optional(base, baseline, cwd=cwd) is not None:
                fail(f"base release state is invalid ({invariant}); a repair cannot remove {baseline}")
            continue
        stamps = [stamp.decode() for stamp in MANAGED_STAMP.findall(actual)]
        if stamps != [version]:
            fail(
                f"base release state is invalid ({invariant}); {baseline} must carry one "
                f"managed stamp of the release version {version}, not {stamps}"
            )
        entry = files.get(name) if isinstance(files, dict) else None
        recorded = entry.get("sha256") if isinstance(entry, dict) else None
        if recorded != hashlib.sha256(actual).hexdigest():
            fail(
                f"base release state is invalid ({invariant}); the manifest hash of "
                f"{name} must be its managed baseline's"
            )


def file_at_optional(ref: str, path: str, *, cwd: Path) -> bytes | None:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"], cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    return result.stdout if result.returncode == 0 else None


STAMP = "<stamp>"


def stamp_neutral(path: str, data: bytes | None) -> Any:
    """`data` with its version stamp fields replaced, so two versions of a
    coupled file compare equal exactly when only their stamps differ."""
    if data is None:
        return None
    try:
        text = data.decode()
    except UnicodeDecodeError:
        return data
    if path == "Cargo.toml":
        section = re.search(r"(?ms)^\[workspace\.package\]\s*$.*?(?=^\[|\Z)", text)
        if section:
            neutral = re.sub(r'(?m)^version\s*=\s*"[^"]*"\s*$', f'version = "{STAMP}"', section.group(0))
            text = text[: section.start()] + neutral + text[section.end() :]
        return text
    if path == "Cargo.lock":
        return re.sub(
            r'(?m)^(name = "codeflow-(?:cli|core|present)"\nversion = )"[^"]*"$',
            rf'\1"{STAMP}"',
            text,
        )
    if path == ".codeflow/project.toml":
        return re.sub(r'(?m)^(scaffold_version\s*=\s*)"[^"]*"', rf'\1"{STAMP}"', text)
    if path == ".codeflow/manifest.json":
        try:
            value = json.loads(text)
        except json.JSONDecodeError:
            return text
        if not isinstance(value, dict):
            return text
        value["scaffold_version"] = STAMP
        files = value.get("files")
        for name in ["AGENTS.md", "CLAUDE.md"]:
            entry = files.get(name) if isinstance(files, dict) else None
            if isinstance(entry, dict) and "sha256" in entry:
                # Derived from the managed baseline, compared stamp-free there.
                entry["sha256"] = STAMP
        return value
    return re.sub(
        r"<!-- codeflow:managed:begin scaffold=\d+\.\d+\.\d+ -->",
        f"<!-- codeflow:managed:begin scaffold={STAMP} -->",
        text,
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
    config = load_config(args.config)
    if args.structural:
        # R-94: `codeflow integrate` and the local preflight judge the tree
        # against the recorded baseline and local tags only.
        result = validate_release_tree(args.ref, config, structural_state(config, cwd=args.root), cwd=args.root)
        result["host"] = HOST_UNCHECKED
    else:
        result = validate_release_tree(args.ref, config, get_host_state(args), cwd=args.root)
    print(json.dumps({"status": "ok", **result}, sort_keys=True))


def structural_state(config: dict[str, Any], *, cwd: Path) -> dict[str, Any]:
    """A host state built from the recorded bootstrap and the local stable
    tags: enough to check the tree's structure, never publication. Every
    local stable tag above the bootstrap counts as published, so a pending
    version can never reuse one."""
    bootstrap = validate_bootstrap(config, cwd=cwd)
    tags: dict[str, str] = {}
    for tag in git("tag", "--list", "v*", cwd=cwd).splitlines():
        if STABLE_TAG.fullmatch(tag):
            tags[tag] = git("rev-parse", f"{tag}^{{commit}}", cwd=cwd)
    archive = config["bootstrap"]["published"].get("source_archive_sha256", "")
    releases: list[dict[str, Any]] = [
        {
            "tag": bootstrap.tag,
            "draft": False,
            "prerelease": False,
            "body": "",
            "assets": [{"name": "source.tar.gz", "digest": f"sha256:{archive}"}],
        }
    ]
    for tag, source in tags.items():
        if semver(tag) > semver(bootstrap.version):
            releases.append(
                {
                    "tag": tag,
                    "draft": False,
                    "prerelease": False,
                    "body": SOURCE_MARKER.format(source=source),
                    "assets": [{"name": "local-tag", "digest": "sha256:" + "0" * 64}],
                }
            )
    return {"schema_version": 1, "drafts_visible": False, "tags": tags, "releases": releases}


def load_path_sets(ref: str, *, cwd: Path) -> dict[str, Any]:
    """The path-set table `ref` carries, else the one beside this script."""
    data = file_at_optional(ref, PATH_SETS, cwd=cwd)
    try:
        return tomllib.loads((data or (ROOT / PATH_SETS).read_bytes()).decode())
    except (OSError, UnicodeDecodeError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot read the path sets {PATH_SETS}: {error}")


def is_behaviour_path(path: str, sets: dict[str, Any]) -> bool:
    """R-93, R-114: every path outside the documentation, the records and the
    record templates is behaviour; the skill trees always are."""
    def member_matches(section: str) -> bool:
        return any(
            matches_any(path, list(member.get("patterns", [])))
            for member in sets.get(section, [])
        )

    return member_matches("behaviour_always") or not member_matches("not_behaviour")


def target_for_branch(branch: str, head: str, config: dict[str, Any], *, cwd: Path) -> str:
    """The branch a pull request from `branch` targets: a task branch's
    recorded integration target, else the configured main branch."""
    task = re.match(r"task/(TSK-\d+)-", branch or "")
    if task:
        record = file_at_optional(head, f"project-management/tasks/{task.group(1)}.md", cwd=cwd)
        found = re.search(
            r"(?m)^integration_target:\s*[\"']?([^\"'#\s]+)", (record or b"").decode(errors="replace")
        )
        if found:
            return found.group(1)
    return str(config["main_branch"])


def resolve_pr_base(args: argparse.Namespace, head: str, config: dict[str, Any]) -> tuple[str | None, str]:
    if args.base:
        return git("rev-parse", f"{args.base}^{{commit}}", cwd=args.root), args.base
    target = args.target or target_for_branch(args.branch, head, config, cwd=args.root)
    for ref in [f"refs/remotes/{args.remote}/{target}", f"refs/heads/{target}"]:
        if run(["git", "rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}"], cwd=args.root, check=False).returncode == 0:
            base = run(["git", "merge-base", ref, head], cwd=args.root, check=False).stdout.strip()
            if base:
                return base, ref
    return None, target


def tree_error(ref: str, config: dict[str, Any], state: dict[str, Any], *, cwd: Path) -> str | None:
    try:
        validate_release_tree(ref, config, state, cwd=cwd)
    except ReleaseError as error:
        return str(error)
    return None


def draft_intent(args: argparse.Namespace, notes: list[str]) -> str | None:
    """The declared Impact of the local PR draft, when one is given."""
    path = args.body_file or (Path(os.environ["CODEFLOW_PR_DRAFT"]) if os.getenv("CODEFLOW_PR_DRAFT") else None)
    if path is None:
        return None
    try:
        return parse_release_impact(path.read_text(encoding="utf-8"))["impact"]
    except (OSError, ReleaseError) as error:
        notes.append(f"the PR draft {path} gives no readable Release impact: {error}")
        return None


def preflight(args: argparse.Namespace) -> None:
    """R-93: the cheap local release checks pre-push runs. It blocks only a
    push that breaks a release tree its base kept valid; a missing entry only
    warns, and the PR job's full `check-pr` stays the gate."""
    config = load_config(args.config)
    head = git("rev-parse", f"{args.head}^{{commit}}", cwd=args.root)
    notes: list[str] = []
    status = "ok"
    try:
        state = structural_state(config, cwd=args.root)
    except ReleaseError as error:
        print(json.dumps({"status": "warn", "notes": [
            f"release preflight could not read the recorded baseline ({error}); CI checks it"
        ]}, sort_keys=True))
        return
    base, target = resolve_pr_base(args, head, config)
    if base is None:
        notes.append(f"release range unresolved: no local or {args.remote} ref for {target}; CI checks it")
    head_error = tree_error(head, config, state, cwd=args.root)
    if head_error is None:
        notes.append(f"release tree valid at {head[:12]} against the recorded baseline ({HOST_UNCHECKED})")
    elif base is not None and tree_error(base, config, state, cwd=args.root) is None:
        status = "blocked"
        notes.append(f"this push breaks the release tree its base kept valid ({HOST_UNCHECKED}): {head_error}")
    else:
        status = "warn"
        notes.append(
            f"release tree invalid at {head[:12]} and at its base ({HOST_UNCHECKED}): {head_error}; "
            "repair it with a typed repair (docs/releasing.md)"
        )
    if base is not None:
        sets = load_path_sets(head, cwd=args.root)
        behaviour = [path for path in changed_paths(base, head, cwd=args.root) if is_behaviour_path(path, sets)]
        intent = draft_intent(args, notes)
        if behaviour and intent != "none" and not entry_changed(base, head, config, state, cwd=args.root):
            status = "blocked" if status == "blocked" else "warn"
            shown = ", ".join(behaviour[:3]) + (f" and {len(behaviour) - 3} more" if len(behaviour) > 3 else "")
            notes.append(
                f"behaviour paths changed ({shown}) with no pending changelog entry added or edited "
                "and no `Impact: none` in the PR draft (CODEFLOW_PR_DRAFT); add a labelled entry "
                "or declare the intent. This warning never blocks; the PR job does"
            )
    print(json.dumps({"status": status, "notes": notes, "host": HOST_UNCHECKED}, sort_keys=True))
    if status == "blocked":
        args.exit_code = 1


def entry_changed(base: str, head: str, config: dict[str, Any], state: dict[str, Any], *, cwd: Path) -> bool:
    """Whether `head` adds or edits a pending entry relative to `base`."""
    baseline = resolve_baseline(config, state, cwd=cwd)
    items = []
    for ref in [base, head]:
        text = file_at_ref(ref, "CHANGELOG.md", cwd=cwd).decode()
        section = pending_section(text, baseline, repair=True)
        items.append(lenient_items(section) if section else {})
    change = entry_change(items[0], items[1], adopting=False)
    return bool(change.added or change.edited)


def show_host_state(args: argparse.Namespace) -> None:
    print(json.dumps(get_host_state(args), sort_keys=True))


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
    # The staging note is one paragraph however it wraps: drop it to the blank line.
    body = re.sub(r"(?m)^_Staging evidence:.*(?:\n(?![ \t]*$).*)*", "", body)
    body = merge_subsections(body)
    if re.search(r"<!--\s*codeflow:", body) or "staging evidence" in body.lower():
        fail(f"{tag} release notes would publish an internal marker or staging note")
    return f"{body}\n\n{SOURCE_MARKER.format(source=source)}\n"


def merge_subsections(body: str) -> str:
    """Render each `### ` kind once, in first-appearance order, bodies in source order."""
    preamble, *parts = re.split(r"(?m)^(### .*?)[ \t]*$", body)
    kinds: dict[str, list[str]] = {}
    for heading, text in zip(parts[::2], parts[1::2], strict=True):
        kinds.setdefault(heading, []).append(text.strip())
    blocks = [preamble.strip()] + [
        "\n\n".join([heading, *filter(None, texts)]) for heading, texts in kinds.items()
    ]
    return re.sub(r"\n{3,}", "\n\n", "\n\n".join(filter(None, blocks))).strip()


def write_release_notes(args: argparse.Namespace) -> None:
    args.output.write_text(
        release_notes(args.root, args.ref, args.tag, args.source), encoding="utf-8"
    )
    print(json.dumps({"status": "prepared", "output": str(args.output)}))


DEFAULT_PUBLICATION_WORKFLOWS = [".github/workflows/codeflow-ci.yml"]


def verify_checks(args: argparse.Namespace) -> None:
    pages = load_json(args.state)
    run_files = args.runs_state if isinstance(args.runs_state, list) else [args.runs_state]
    config = load_config(args.config)
    required = config.get("required_publication_checks")
    workflows = config.get("publication_workflows", DEFAULT_PUBLICATION_WORKFLOWS)
    if (
        not isinstance(pages, list)
        or not isinstance(required, list)
        or not required
        or len(set(required)) != len(required)
        or any(not isinstance(name, str) or not name for name in required)
        or not isinstance(workflows, list)
        or not workflows
        or len(set(workflows)) != len(workflows)
        or any(not isinstance(path, str) or not path for path in workflows)
    ):
        fail("publication check inventory or configured requirements are malformed")
    runs: list[Any] = []
    for page in pages:
        if not isinstance(page, dict) or not isinstance(page.get("check_runs"), list):
            fail("publication check page is malformed")
        runs.extend(page["check_runs"])
    workflow_runs: list[Any] = []
    for run_file in run_files:
        run_pages = load_json(run_file)
        if not isinstance(run_pages, list):
            fail("publication workflow-run inventory is malformed")
        for page in run_pages:
            if not isinstance(page, dict) or not isinstance(page.get("workflow_runs"), list):
                fail("publication workflow-run page is malformed")
            workflow_runs.extend(page["workflow_runs"])
    # Each configured workflow's latest main-push run of the source must have
    # succeeded; the required checks are read only from those runs' suites.
    suites: set[int] = set()
    for workflow in workflows:
        candidates = [
            run
            for run in workflow_runs
            if isinstance(run, dict)
            and run.get("event") == "push"
            and run.get("head_branch") == "main"
            and run.get("head_sha") == args.source
            and run.get("path") == workflow
            and isinstance(run.get("id"), int)
            and isinstance(run.get("run_number"), int)
            and isinstance(run.get("run_attempt"), int)
            and isinstance(run.get("check_suite_id"), int)
        ]
        name = Path(workflow).stem
        if not candidates:
            fail(f"selected source lacks its {name} main-push workflow run")
        selected = max(
            candidates,
            key=lambda run: (run["run_number"], run["run_attempt"], run["id"]),
        )
        if selected.get("status") != "completed" or selected.get("conclusion") != "success":
            fail(f"latest {name} main-push workflow run is not successful")
        suites.add(selected["check_suite_id"])
    latest: dict[str, dict[str, Any]] = {}
    for run in runs:
        if (
            not isinstance(run, dict)
            or run.get("name") not in required
            or run.get("head_sha") != args.source
            or (run.get("app") or {}).get("slug") != "github-actions"
            or (run.get("check_suite") or {}).get("id") not in suites
            or not isinstance(run.get("id"), int)
        ):
            continue
        name = str(run["name"])
        if name not in latest or run["id"] > latest[name]["id"]:
            latest[name] = run
    blocked = [
        name
        for name in required
        if name not in latest
        or latest[name].get("status") != "completed"
        or latest[name].get("conclusion") != "success"
    ]
    if blocked:
        fail(f"selected source lacks latest trusted successful checks: {', '.join(blocked)}")
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
        fail("selected main source lacks an ordinary human-merged PR into this repository")
    print(json.dumps({"status": "authorized", "source": args.source}, sort_keys=True))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_host_state(args: argparse.Namespace) -> None:
    state = load_json(args.state)
    if not isinstance(state, dict):
        fail("publication state must be an object")
    expected_notes = args.notes.read_text(encoding="utf-8").strip()
    if state.get("schema_version") == 1:
        if state.get("drafts_visible") is not True:
            fail("publication attempt requires write-visible draft inventory")
        matches = [
            item
            for item in state.get("releases", [])
            if isinstance(item, dict) and item.get("tag") == args.tag
        ]
        if len(matches) > 1:
            fail(f"duplicate releases for {args.tag} are ambiguous")
        item = matches[0] if matches else None
        tag_target = state.get("tags", {}).get(args.tag)
        release = (
            {
                "draft": item.get("draft"),
                "tag_name": item.get("tag"),
                "name": item.get("name"),
                "target_commitish": item.get("target"),
                "body": item.get("body"),
                "assets": item.get("assets"),
            }
            if item
            else None
        )
    else:
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
    state.add_argument(
        "--structural",
        action="store_true",
        help=f"check against the recorded baseline and local tags ({HOST_UNCHECKED})",
    )
    add_host_args(state)
    state.set_defaults(func=check_state)

    local = sub.add_parser("preflight", help="the local release checks pre-push runs")
    local.add_argument("--head", default="HEAD")
    local.add_argument("--branch", default="")
    local.add_argument("--remote", default="origin")
    local.add_argument("--target", default="")
    local.add_argument("--base", default="")
    local.add_argument("--body-file", type=Path)
    local.set_defaults(func=preflight)

    inventory = sub.add_parser("host-state")
    add_host_args(inventory)
    inventory.set_defaults(func=show_host_state)

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
    checks.add_argument("--runs-state", type=Path, action="append", required=True)
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
    return int(getattr(args, "exit_code", 0))


if __name__ == "__main__":
    raise SystemExit(main())
