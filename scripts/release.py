#!/usr/bin/env python3
"""CodeFlow's repository-local release candidate and authorization checks.

This intentionally implements CodeFlow's one-unit release policy, not a
general release engine.  git-cliff remains the version calculator and
cargo-dist remains the publisher.
"""

from __future__ import annotations

import argparse
import datetime as dt
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
DEFAULT_CONFIG = ROOT / ".release" / "config.json"
DEFAULT_RECORD = ROOT / ".release" / "candidate.json"
IMPACT_ORDER = {"none": 0, "patch": 1, "minor": 2, "major": 3}
PLACEHOLDERS = {"", "n/a", "none", "todo", "tbd", "-"}


class ReleaseError(RuntimeError):
    """A fail-closed release contract violation."""


def fail(message: str) -> NoReturn:
    raise ReleaseError(message)


def run(
    args: list[str], *, cwd: Path = ROOT, check: bool = True
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(
        args, cwd=cwd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if check and result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "no output"
        fail(f"command failed ({' '.join(args)}): {detail}")
    return result


def git(*args: str, cwd: Path = ROOT, check: bool = True) -> str:
    return run(["git", *args], cwd=cwd, check=check).stdout.strip()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")
    if not isinstance(value, dict):
        fail(f"{path} must contain a JSON object")
    return value


def write_json(path: Path, value: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def load_config(path: Path) -> dict[str, Any]:
    config = load_json(path)
    if config.get("schema_version") != 1 or config.get("release_unit") != "codeflow":
        fail("release config must be schema v1 for the codeflow release unit")
    for key in ["main_branch", "candidate_branch", "comparison", "published"]:
        if key not in config:
            fail(f"release config is missing {key}")
    return config


def normalize_value(value: str) -> str:
    value = value.strip()
    if len(value) >= 2 and value[0] == value[-1] == "`":
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
    for match in re.finditer(r"(?m)^- ([A-Za-z ]+):\s*(.+?)\s*$", section):
        key = match.group(1).strip().casefold().replace(" ", "_")
        if key in fields:
            fail(f"Release impact field {match.group(1)} is duplicated")
        fields[key] = normalize_value(match.group(2))
    required = ["unit", "impact", "rationale", "evidence", "contract"]
    for key in required:
        if key not in fields or (key in {"rationale", "evidence"} and is_placeholder(fields[key])):
            fail(f"Release impact field {key} is required and must be substantive")
    if fields["unit"] != "codeflow":
        fail("Release impact unit must be codeflow")
    if fields["impact"] not in IMPACT_ORDER:
        fail("Release impact must be none, patch, minor, or major")
    if fields["contract"] not in {"not-applicable", "compatible", "breaking"}:
        fail("Release impact contract must be not-applicable, compatible, or breaking")
    return fields


def commit_impact(base: str, head: str, *, cwd: Path = ROOT) -> str:
    raw = git("log", "--no-merges", "--format=%s%x1f%b%x1e", f"{base}..{head}", cwd=cwd)
    impact = "none"
    for entry in raw.split("\x1e"):
        # git terminates each formatted record with our separator and then its
        # own newline. Remove only that record boundary; body whitespace stays
        # intact for BREAKING CHANGE detection.
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


def changed_paths(base: str, head: str, *, cwd: Path = ROOT) -> list[str]:
    raw = git("diff", "--name-only", f"{base}...{head}", "--", cwd=cwd)
    return [line for line in raw.splitlines() if line]


def matches_any(path: str, patterns: list[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def check_pr(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    if args.body_file:
        body = args.body_file.read_text(encoding="utf-8")
    else:
        body = os.environ.get(args.body_env, "")
    fields = parse_release_impact(body)
    derived = commit_impact(args.base, args.head, cwd=args.root)
    if fields["impact"] != derived:
        fail(
            f"declared impact {fields['impact']} contradicts landed conventional "
            f"markers ({derived})"
        )
    paths = changed_paths(args.base, args.head, cwd=args.root)
    watched = sorted(
        path for path in paths if matches_any(path, config["watched_contract_paths"])
    )
    if watched and fields["contract"] == "not-applicable":
        fail("watched contract changes require a compatible or breaking assessment")
    if fields["contract"] == "breaking" and fields["impact"] != "major":
        fail("a breaking contract assessment requires major impact")
    if fields["impact"] == "major":
        migration = fields.get("migration", "")
        if is_placeholder(migration):
            fail("major impact requires substantive migration guidance")
    if fields["impact"] != "none":
        if "CHANGELOG.md" not in paths:
            fail("non-none impact requires a curated CHANGELOG.md change")
        if "changelog" not in fields["evidence"].casefold():
            fail("non-none impact evidence must identify the curated changelog entry")
    print(
        json.dumps(
            {"status": "ok", "declared": fields["impact"], "watched": watched},
            sort_keys=True,
        )
    )


def semver(value: str) -> tuple[int, int, int]:
    match = re.fullmatch(r"v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", value)
    if not match:
        fail(f"unsupported release version: {value}")
    return tuple(int(part) for part in match.groups())  # type: ignore[return-value]


def object_exists(value: str, *, cwd: Path) -> bool:
    return run(["git", "cat-file", "-e", f"{value}^{{commit}}"], cwd=cwd, check=False).returncode == 0


def validate_provenance(config: dict[str, Any], *, cwd: Path) -> None:
    comparison = config["comparison"]
    actual = git("rev-parse", f"{comparison['tag']}^{{commit}}", cwd=cwd)
    if actual != comparison["commit"]:
        fail(
            f"comparison tag {comparison['tag']} resolves to {actual}, expected "
            f"{comparison['commit']}; do not move or silently repair the tag"
        )
    published = config["published"]
    if not object_exists(published["source_commit"], cwd=cwd):
        fail("verified published source commit is absent from this repository")
    if not re.fullmatch(r"[0-9a-f]{64}", published["source_archive_sha256"]):
        fail("published source archive SHA-256 must be a distinct 64-hex checksum")


def workspace_version(path: Path) -> str:
    try:
        value = tomllib.loads(path.read_text(encoding="utf-8"))["workspace"]["package"][
            "version"
        ]
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot read workspace version: {error}")
    if not isinstance(value, str):
        fail("workspace version must be a string")
    semver(value)
    return value


def replace_workspace_version(path: Path, version: str) -> None:
    text = path.read_text(encoding="utf-8")
    section = re.search(r"(?ms)^\[workspace\.package\]\s*$.*?(?=^\[|\Z)", text)
    if not section:
        fail("Cargo.toml lacks [workspace.package]")
    replacement, count = re.subn(
        r'(?m)^version\s*=\s*"[^"]+"\s*$', f'version = "{version}"', section.group(0)
    )
    if count != 1:
        fail("[workspace.package] must contain exactly one version")
    path.write_text(text[: section.start()] + replacement + text[section.end() :], encoding="utf-8")


def changelog_parts(text: str) -> tuple[re.Match[str], int, str]:
    unreleased = re.search(r"(?m)^## \[Unreleased\]\s*$", text)
    if not unreleased:
        fail("CHANGELOG.md lacks an Unreleased section")
    next_heading = re.search(r"(?m)^## \[", text[unreleased.end() :])
    if not next_heading:
        fail("CHANGELOG.md lacks a version section after Unreleased")
    end = unreleased.end() + next_heading.start()
    body = text[unreleased.end() : end].strip()
    return unreleased, end, body


def promote_changelog(path: Path, tag: str, date: str) -> None:
    text = path.read_text(encoding="utf-8")
    unreleased, body_end, body = changelog_parts(text)
    if not body:
        fail("no curated Unreleased notes exist; no release candidate is needed")
    version = tag.removeprefix("v")
    version_heading = re.search(
        rf"(?m)^## \[{re.escape(version)}\](?: - (?P<date>\d{{4}}-\d{{2}}-\d{{2}}))?\s*$",
        text,
    )
    empty_unreleased = text[: unreleased.end()] + "\n\n"
    if version_heading:
        # The staged v3 section predates automation.  Its date remains the
        # candidate-cut date; new curated notes are prepended without replacing
        # the existing reviewed prose.
        rest = text[body_end:]
        heading_in_rest = re.search(
            rf"(?m)^## \[{re.escape(version)}\](?: - \d{{4}}-\d{{2}}-\d{{2}})?\s*$", rest
        )
        if not heading_in_rest:
            fail(f"cannot locate staged {tag} section after Unreleased")
        insertion = heading_in_rest.end()
        rest = rest[:insertion] + "\n\n" + body + rest[insertion:]
        result = empty_unreleased + rest.lstrip("\n")
    else:
        rest = text[body_end:].lstrip("\n")
        result = (
            empty_unreleased
            + f"## [{version}] - {date}\n\n"
            + body
            + "\n\n"
            + rest
        )
    path.write_text(result.rstrip() + "\n", encoding="utf-8")


def release_notes(root: Path, tag: str, candidate: str) -> str:
    version = tag.removeprefix("v")
    text = (root / "CHANGELOG.md").read_text(encoding="utf-8")
    heading = re.search(
        rf"(?m)^## \[{re.escape(version)}\](?: - \d{{4}}-\d{{2}}-\d{{2}})?\s*$",
        text,
    )
    if not heading:
        fail(f"CHANGELOG.md lacks the reviewed {tag} section")
    next_heading = re.search(r"(?m)^## \[", text[heading.end() :])
    end = heading.end() + next_heading.start() if next_heading else len(text)
    body = text[heading.end() : end].strip()
    if not body:
        fail(f"CHANGELOG.md {tag} section has no curated notes")
    return f"{body}\n\n<!-- codeflow-release-candidate: {candidate} -->\n"


def write_release_notes(args: argparse.Namespace) -> None:
    notes = release_notes(args.root, args.tag, args.candidate)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(notes, encoding="utf-8")
    print(json.dumps({"status": "prepared", "output": str(args.output)}))


def cliff_version(executable: str, *, cwd: Path) -> str:
    result = run([executable, "--bumped-version"], cwd=cwd)
    matches = re.findall(r"\bv\d+\.\d+\.\d+\b", result.stdout)
    if len(matches) != 1:
        fail("git-cliff must report exactly one stable bumped version")
    return matches[0]


def tag_exists(tag: str, *, cwd: Path) -> bool:
    return run(["git", "rev-parse", "--verify", f"refs/tags/{tag}^{{commit}}"], cwd=cwd, check=False).returncode == 0


def latest_stable_tag(*, cwd: Path) -> tuple[str, str]:
    raw = git("tag", "--merged", "HEAD", "--list", "v*", "--sort=-version:refname", cwd=cwd)
    for tag in raw.splitlines():
        if re.fullmatch(r"v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)", tag):
            return tag, git("rev-parse", f"{tag}^{{commit}}", cwd=cwd)
    fail("no reachable stable release tag exists")


def find_recorded_merge(record: dict[str, Any], main_ref: str, config: dict[str, Any], *, cwd: Path) -> str:
    source = record.get("source_commit")
    matches: list[str] = []
    for line in git("rev-list", "--merges", "--parents", main_ref, cwd=cwd).splitlines():
        values = line.split()
        if len(values) != 3 or values[1] != source:
            continue
        merge, candidate = values[0], values[2]
        if git("rev-parse", f"{merge}^{{tree}}", cwd=cwd) != git("rev-parse", f"{candidate}^{{tree}}", cwd=cwd):
            continue
        try:
            at_candidate = load_record_at_ref(candidate, config["candidate_record"], cwd=cwd)
            verify_ref_candidate(candidate, config, at_candidate, cwd=cwd)
        except ReleaseError:
            continue
        if at_candidate == record:
            matches.append(merge)
    if len(matches) != 1:
        fail("pending candidate record is not bound to one exact equal-tree main merge")
    return matches[0]


def prepare(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    if args.record.exists():
        prior = load_json(args.record)
        prior_tag = prior.get("tag")
        if prior.get("status") == "candidate" and isinstance(prior_tag, str) and not tag_exists(prior_tag, cwd=args.root):
            find_recorded_merge(prior, "HEAD", config, cwd=args.root)
            print(
                json.dumps(
                    {
                        "status": "pending-publication",
                        "tag": prior_tag,
                        "message": "publish or explicitly resolve this candidate before preparing another",
                    }
                )
            )
            return
    changelog = args.root / "CHANGELOG.md"
    _, _, notes = changelog_parts(changelog.read_text(encoding="utf-8"))
    if not notes:
        print(json.dumps({"status": "none"}))
        return
    comparison_tag, comparison_commit = latest_stable_tag(cwd=args.root)
    if commit_impact(comparison_commit, "HEAD", cwd=args.root) == "none":
        print(json.dumps({"status": "none", "reason": "no release-impacting commits"}))
        return
    validate_provenance(config, cwd=args.root)
    tag = cliff_version(args.git_cliff, cwd=args.root)
    if semver(tag) <= semver(comparison_tag):
        fail(f"git-cliff result {tag} does not advance reachable {comparison_tag}")
    current = workspace_version(args.root / "Cargo.toml")
    next_version = tag.removeprefix("v")
    if semver(current) > semver(next_version):
        fail(f"workspace {current} is ahead of git-cliff result {next_version}")
    if current != next_version:
        replace_workspace_version(args.root / "Cargo.toml", next_version)
    date = args.date or dt.date.today().isoformat()
    promote_changelog(changelog, tag, date)
    record = {
        "schema_version": 1,
        "status": "prepared",
        "release_unit": "codeflow",
        "version": next_version,
        "tag": tag,
        "candidate_date": date,
        "source_commit": git("rev-parse", "HEAD", cwd=args.root),
        "source_tree": git("rev-parse", "HEAD^{tree}", cwd=args.root),
        "comparison": {"tag": comparison_tag, "commit": comparison_commit},
        "published_provenance": config["published"],
        "generated_files": {},
    }
    write_json(args.record, record)
    print(json.dumps({"status": "prepared", "tag": tag, "version": next_version}))


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


VERSION_STAMP_PATHS = [
    "Cargo.toml",
    "Cargo.lock",
    ".codeflow/project.toml",
    ".codeflow/manifest.json",
    "AGENTS.md",
    "CLAUDE.md",
]


def validate_version_stamps(files: dict[str, bytes]) -> str:
    try:
        cargo = tomllib.loads(files["Cargo.toml"].decode("utf-8"))
        cargo_version = cargo["workspace"]["package"]["version"]
        lock = tomllib.loads(files["Cargo.lock"].decode("utf-8"))
        project = tomllib.loads(files[".codeflow/project.toml"].decode("utf-8"))
        manifest = json.loads(files[".codeflow/manifest.json"])
    except (KeyError, UnicodeDecodeError, json.JSONDecodeError, tomllib.TOMLDecodeError) as error:
        fail(f"cannot parse coupled version stamps: {error}")
    if not isinstance(cargo_version, str):
        fail("workspace version must be a string")
    semver(cargo_version)
    own = {
        package["name"]: package["version"]
        for package in lock.get("package", [])
        if package.get("name") in {"codeflow-cli", "codeflow-core", "codeflow-present"}
    }
    if set(own.values()) != {cargo_version} or len(own) != 3:
        fail(f"workspace package versions disagree: Cargo={cargo_version}, lock={own}")
    scaffold = project.get("scaffold_version")
    if scaffold != cargo_version:
        fail(f"managed scaffold stamp {scaffold!r} disagrees with {cargo_version}")
    manifest_scaffold = manifest.get("scaffold_version") if isinstance(manifest, dict) else None
    if manifest_scaffold != cargo_version:
        fail(f"managed manifest stamp {manifest_scaffold!r} disagrees with {cargo_version}")
    marker = f"<!-- codeflow:managed:begin scaffold={cargo_version} -->".encode()
    for name in ["AGENTS.md", "CLAUDE.md"]:
        if marker not in files[name]:
            fail(f"{name} managed version stamp disagrees with {cargo_version}")
    return cargo_version


def version_stamps(root: Path) -> dict[str, str]:
    version = validate_version_stamps(
        {path: (root / path).read_bytes() for path in VERSION_STAMP_PATHS}
    )
    return {"workspace": version, "scaffold": version}


def diff_paths(source: str, *, cwd: Path) -> list[str]:
    raw = git("diff", "--name-only", source, "--", cwd=cwd)
    untracked = git("ls-files", "--others", "--exclude-standard", cwd=cwd)
    return sorted({line for line in (raw + "\n" + untracked).splitlines() if line})


def finalize(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    record = load_json(args.record)
    if record.get("status") != "prepared":
        fail("candidate must be in prepared state before finalization")
    if git("rev-parse", "HEAD", cwd=args.root) != record.get("source_commit"):
        fail("candidate source changed during preparation")
    stamps = version_stamps(args.root)
    if stamps["workspace"] != record.get("version"):
        fail("candidate version disagrees with regenerated version stamps")
    _, _, unreleased = changelog_parts((args.root / "CHANGELOG.md").read_text(encoding="utf-8"))
    if unreleased:
        fail("candidate changelog still contains Unreleased notes")
    paths = diff_paths(record["source_commit"], cwd=args.root)
    record_rel = args.record.relative_to(args.root).as_posix()
    for path in paths:
        if path == record_rel:
            continue
        if not matches_any(path, config["generated_allowlist"]):
            fail(f"candidate preparation changed non-generated path: {path}")
    if "CHANGELOG.md" not in paths:
        fail("candidate must promote the curated changelog")
    generated = {
        path: sha256(args.root / path)
        for path in sorted(paths)
        if path != record_rel and (args.root / path).is_file()
    }
    record["status"] = "candidate"
    record["generated_files"] = generated
    write_json(args.record, record)
    verify_worktree_candidate(args.root, config, record, args.record)
    print(json.dumps({"status": "candidate", "tag": record["tag"], "files": len(generated)}))


def file_at_ref(ref: str, path: str, *, cwd: Path) -> bytes:
    result = subprocess.run(
        ["git", "show", f"{ref}:{path}"], cwd=cwd, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )
    if result.returncode != 0:
        fail(f"{ref} is missing recorded candidate file {path}")
    return result.stdout


def load_record_at_ref(ref: str, record_path: str, *, cwd: Path) -> dict[str, Any]:
    raw = file_at_ref(ref, record_path, cwd=cwd)
    try:
        value = json.loads(raw)
    except json.JSONDecodeError as error:
        fail(f"candidate record at {ref} is invalid: {error}")
    if not isinstance(value, dict):
        fail("candidate record at ref must be a JSON object")
    return value


def verify_ref_candidate(ref: str, config: dict[str, Any], record: dict[str, Any], *, cwd: Path) -> None:
    if record.get("schema_version") != 1 or record.get("status") != "candidate":
        fail("candidate record must be finalized schema v1")
    version = record.get("version")
    if not isinstance(version, str) or record.get("tag") != f"v{version}":
        fail("candidate tag must be exactly v plus its stable version")
    semver(version)
    source = record.get("source_commit")
    if not isinstance(source, str) or not object_exists(source, cwd=cwd):
        fail("candidate source commit is missing")
    if record.get("source_tree") != git("rev-parse", f"{source}^{{tree}}", cwd=cwd):
        fail("candidate source tree does not match its source commit")
    if record.get("published_provenance") != config["published"]:
        fail("candidate published provenance differs from reviewed configuration")
    comparison = record.get("comparison")
    if not isinstance(comparison, dict) or set(comparison) != {"tag", "commit"}:
        fail("candidate comparison must contain only tag and commit")
    if git("rev-parse", f"{comparison['tag']}^{{commit}}", cwd=cwd) != comparison["commit"]:
        fail("candidate comparison tag and commit disagree")
    if run(["git", "merge-base", "--is-ancestor", comparison["commit"], source], cwd=cwd, check=False).returncode != 0:
        fail("candidate comparison is not reachable from its source")
    generated_files = record.get("generated_files")
    if not isinstance(generated_files, dict) or "CHANGELOG.md" not in generated_files:
        fail("candidate generated files must include the curated changelog")
    for path, digest in generated_files.items():
        if not isinstance(path, str) or not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            fail("candidate generated file entries must be path-to-SHA-256 strings")
        if not matches_any(path, config["generated_allowlist"]):
            fail(f"candidate record contains non-allowlisted generated path: {path}")
    changed = set(git("diff", "--name-only", source, ref, "--", cwd=cwd).splitlines())
    expected = set(generated_files) | {config["candidate_record"]}
    if changed != expected:
        fail(f"candidate changed paths disagree with record: changed={sorted(changed)}, expected={sorted(expected)}")
    for path, expected_hash in generated_files.items():
        actual = hashlib.sha256(file_at_ref(ref, path, cwd=cwd)).hexdigest()
        if actual != expected_hash:
            fail(f"candidate file {path} was edited after generation")
    actual_version = validate_version_stamps(
        {path: file_at_ref(ref, path, cwd=cwd) for path in VERSION_STAMP_PATHS}
    )
    if actual_version != version:
        fail("candidate coupled version stamps disagree with its record")
    changelog = file_at_ref(ref, "CHANGELOG.md", cwd=cwd).decode("utf-8")
    _, _, unreleased = changelog_parts(changelog)
    if unreleased or not re.search(rf"(?m)^## \[{re.escape(version)}\](?: - \d{{4}}-\d{{2}}-\d{{2}})?$", changelog):
        fail("candidate changelog version or Unreleased state disagrees with its record")


def verify_worktree_candidate(root: Path, config: dict[str, Any], record: dict[str, Any], record_path: Path) -> None:
    if record.get("schema_version") != 1 or record.get("status") != "candidate":
        fail("candidate record must be finalized schema v1")
    source = record.get("source_commit")
    paths = set(diff_paths(str(source), cwd=root))
    record_rel = record_path.relative_to(root).as_posix()
    expected = set(record.get("generated_files", {})) | {record_rel}
    if paths != expected:
        fail(f"candidate worktree paths disagree with record: changed={sorted(paths)}, expected={sorted(expected)}")
    for path, expected_hash in record.get("generated_files", {}).items():
        actual = sha256(root / path)
        if actual != expected_hash:
            fail(f"candidate worktree file {path} changed after finalization")


def guard_refresh(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    exists = run(["git", "rev-parse", "--verify", f"{args.ref}^{{commit}}"], cwd=args.root, check=False)
    if exists.returncode != 0:
        print(json.dumps({"status": "absent"}))
        return
    record_path = config["candidate_record"]
    record = load_record_at_ref(args.ref, record_path, cwd=args.root)
    verify_ref_candidate(args.ref, config, record, cwd=args.root)
    print(json.dumps({"status": "clean", "tag": record["tag"]}))


def merged_candidate(candidate_ref: str, main_ref: str, config: dict[str, Any], *, cwd: Path) -> dict[str, Any]:
    candidate = git("rev-parse", f"{candidate_ref}^{{commit}}", cwd=cwd)
    record = load_record_at_ref(candidate, config["candidate_record"], cwd=cwd)
    verify_ref_candidate(candidate, config, record, cwd=cwd)
    if record.get("source_commit") != git("rev-parse", f"{candidate}^", cwd=cwd):
        fail("candidate must be a single generated commit directly above its recorded source")
    matches: list[str] = []
    for line in git("rev-list", "--merges", "--parents", main_ref, cwd=cwd).splitlines():
        values = line.split()
        if len(values) == 3 and values[2] == candidate:
            matches.append(values[0])
    if len(matches) != 1:
        fail("candidate head must be the unique second parent of a main merge commit")
    merge = matches[0]
    parents = git("show", "-s", "--format=%P", merge, cwd=cwd).split()
    if parents != [record["source_commit"], candidate]:
        fail("main merge parents do not match candidate source and reviewed head")
    if git("rev-parse", f"{merge}^{{tree}}", cwd=cwd) != git("rev-parse", f"{candidate}^{{tree}}", cwd=cwd):
        fail("main merge tree differs from the reviewed candidate tree")
    tag = record["tag"]
    if tag_exists(tag, cwd=cwd):
        target = git("rev-parse", f"refs/tags/{tag}^{{commit}}", cwd=cwd)
        if target != candidate:
            fail(f"existing {tag} targets {target}, not reviewed candidate {candidate}")
    return {"candidate": candidate, "merge": merge, "tag": tag, "record": record}


def authorize_event(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    event = load_json(args.event)
    event_pull = event.get("pull_request")
    if event.get("action") != "closed" or not isinstance(event_pull, dict) or event_pull.get("merged") is not True:
        fail("publication authorization requires a merged pull_request_target event")
    pull = load_json(args.pull)
    try:
        reviews = json.loads(args.reviews.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read review evidence: {error}")
    if not isinstance(reviews, list):
        fail("review evidence must be a JSON array")
    permissions = load_json(args.permissions)
    head_sha = pull.get("head", {}).get("sha")
    verify_review_data(pull, reviews, permissions, head_sha, args.repository)
    if pull.get("base", {}).get("ref") != config["main_branch"]:
        fail("release candidate was not merged into configured main")
    if pull.get("head", {}).get("ref") != config["candidate_branch"]:
        fail("only the configured release candidate branch can authorize publication")
    merged_by = pull.get("merged_by") or {}
    if merged_by.get("type") != "User" or not merged_by.get("login"):
        fail("release candidate must be merged by an identified human GitHub user")
    merge_sha = pull.get("merge_commit_sha")
    if merge_sha != event_pull.get("merge_commit_sha") or head_sha != event_pull.get("head", {}).get("sha"):
        fail("fresh pull request state disagrees with the triggering event")
    if git("rev-parse", "HEAD", cwd=args.root) != merge_sha:
        fail("authorization checkout is not the event's exact merge commit")
    result = merged_candidate(head_sha, "HEAD", config, cwd=args.root)
    if result["merge"] != merge_sha:
        fail("event merge commit disagrees with the verified candidate merge")
    print(json.dumps({"status": "authorized", "candidate": head_sha, "tag": result["tag"]}, sort_keys=True))


def verify_dispatch(args: argparse.Namespace) -> None:
    config = load_config(args.config)
    result = merged_candidate(args.candidate_ref, args.main_ref, config, cwd=args.root)
    print(json.dumps({"status": "authorized", "candidate": result["candidate"], "tag": result["tag"]}, sort_keys=True))


def verify_host_state(args: argparse.Namespace) -> None:
    state = load_json(args.state)
    candidate = args.candidate
    expected_notes = args.notes.read_text(encoding="utf-8").strip()
    tag_target = state.get("tag_target")
    release = state.get("release")
    if tag_target is not None and tag_target != candidate:
        fail("existing release tag does not target the authorized candidate")
    if release is None:
        print(json.dumps({"status": "fresh" if tag_target is None else "resume-tag"}))
        return
    if not isinstance(release, dict):
        fail("hosting release state must be null or an object")
    if release.get("draft") is not True:
        fail("release already exists publicly; publication will not overwrite it")
    assets = release.get("assets", [])
    if not isinstance(assets, list):
        fail("hosting asset state must be a list")
    if assets:
        fail("partial draft contains assets whose bytes cannot be proven here; refuse overwrite")
    if release.get("tag_name") != args.tag or release.get("name") != args.tag:
        fail("existing empty draft does not match the authorized release tag and title")
    if release.get("target_commitish") != candidate:
        fail("existing empty draft does not target the authorized candidate")
    body = release.get("body")
    if not isinstance(body, str) or body.strip() != expected_notes:
        fail("existing empty draft does not contain the exact reviewed release notes")
    print(json.dumps({"status": "resume-draft"}))


def verify_review_data(
    pull: dict[str, Any],
    reviews: list[Any],
    permissions: dict[str, Any],
    candidate: str,
    repository: str,
) -> None:
    if pull.get("merged") is not True or pull.get("state") != "closed":
        fail("candidate pull request is not merged")
    head = pull.get("head") or {}
    base = pull.get("base") or {}
    head_repo = (head.get("repo") or {}).get("full_name")
    base_repo = (base.get("repo") or {}).get("full_name")
    if head_repo != repository or base_repo != repository:
        fail("release candidate must originate in the same repository")
    if head.get("sha") != candidate:
        fail("pull request head is not the exact candidate commit")
    merged_by = pull.get("merged_by") or {}
    if merged_by.get("type") != "User" or not merged_by.get("login"):
        fail("candidate must be merged by an identified human GitHub user")
    decisive: dict[str, dict[str, Any]] = {}
    author = (pull.get("user") or {}).get("login")
    for item in reviews:
        if not isinstance(item, dict) or item.get("state") not in {
            "APPROVED",
            "CHANGES_REQUESTED",
            "DISMISSED",
        }:
            continue
        user = item.get("user") or {}
        login = user.get("login")
        if user.get("type") != "User" or not login or login == author:
            continue
        if permissions.get(login) not in {"write", "maintain", "admin"}:
            continue
        current = decisive.get(login)
        ordering = (item.get("submitted_at") or "", int(item.get("id") or 0))
        prior = ((current or {}).get("submitted_at") or "", int((current or {}).get("id") or 0))
        if current is None or ordering > prior:
            decisive[login] = item
    blocking = sorted(login for login, item in decisive.items() if item["state"] == "CHANGES_REQUESTED")
    if blocking:
        fail(f"latest eligible review still requests changes: {', '.join(blocking)}")
    approvals = [
        item
        for item in decisive.values()
        if item["state"] == "APPROVED" and item.get("commit_id") == candidate
    ]
    if not approvals:
        fail("candidate needs a current eligible human approval on its exact head")


def verify_review(args: argparse.Namespace) -> None:
    pull = load_json(args.pull)
    try:
        reviews = json.loads(args.reviews.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read review evidence: {error}")
    if not isinstance(reviews, list):
        fail("review evidence must be a JSON array")
    permissions = load_json(args.permissions)
    verify_review_data(pull, reviews, permissions, args.candidate, args.repository)
    print(json.dumps({"status": "approved", "candidate": args.candidate}))


def parser() -> argparse.ArgumentParser:
    value = argparse.ArgumentParser(description=__doc__)
    value.add_argument("--root", type=Path, default=ROOT)
    value.add_argument("--config", type=Path, default=DEFAULT_CONFIG)
    sub = value.add_subparsers(dest="command", required=True)

    check = sub.add_parser("check-pr")
    check.add_argument("--base", required=True)
    check.add_argument("--head", required=True)
    source = check.add_mutually_exclusive_group(required=True)
    source.add_argument("--body-file", type=Path)
    source.add_argument("--body-env")
    check.set_defaults(func=check_pr)

    prep = sub.add_parser("prepare")
    prep.add_argument("--record", type=Path, default=DEFAULT_RECORD)
    prep.add_argument("--git-cliff", default="git-cliff")
    prep.add_argument("--date")
    prep.set_defaults(func=prepare)

    finish = sub.add_parser("finalize")
    finish.add_argument("--record", type=Path, default=DEFAULT_RECORD)
    finish.set_defaults(func=finalize)

    guard = sub.add_parser("guard-refresh")
    guard.add_argument("--ref", required=True)
    guard.set_defaults(func=guard_refresh)

    event = sub.add_parser("authorize-event")
    event.add_argument("--event", type=Path, required=True)
    event.add_argument("--pull", type=Path, required=True)
    event.add_argument("--reviews", type=Path, required=True)
    event.add_argument("--permissions", type=Path, required=True)
    event.add_argument("--repository", required=True)
    event.set_defaults(func=authorize_event)

    review = sub.add_parser("verify-review")
    review.add_argument("--pull", type=Path, required=True)
    review.add_argument("--reviews", type=Path, required=True)
    review.add_argument("--permissions", type=Path, required=True)
    review.add_argument("--candidate", required=True)
    review.add_argument("--repository", required=True)
    review.set_defaults(func=verify_review)

    dispatch = sub.add_parser("verify-dispatch")
    dispatch.add_argument("--candidate-ref", default="HEAD")
    dispatch.add_argument("--main-ref", default="origin/main")
    dispatch.set_defaults(func=verify_dispatch)

    host = sub.add_parser("verify-host-state")
    host.add_argument("--state", type=Path, required=True)
    host.add_argument("--candidate", required=True)
    host.add_argument("--tag", required=True)
    host.add_argument("--notes", type=Path, required=True)
    host.set_defaults(func=verify_host_state)

    notes = sub.add_parser("release-notes")
    notes.add_argument("--candidate", required=True)
    notes.add_argument("--tag", required=True)
    notes.add_argument("--output", type=Path, required=True)
    notes.set_defaults(func=write_release_notes)

    return value


def main() -> int:
    args = parser().parse_args()
    args.root = args.root.resolve()
    if not args.config.is_absolute():
        args.config = args.root / args.config
    if hasattr(args, "record") and not args.record.is_absolute():
        args.record = args.root / args.record
    try:
        args.func(args)
    except (ReleaseError, OSError, ValueError) as error:
        print(f"release error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
