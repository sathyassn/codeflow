#!/usr/bin/env python3
"""Judge OSV base/head scans and trusted exceptions with one precedence table.

The CI workflow embeds this file so code from the pull request cannot replace
its judge. Python 3.11+ is provided by the hosted Ubuntu runner.
"""
import datetime
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import tomllib


class AuditError(ValueError):
    """Missing evidence or an invalid exception; fatal even at warn."""


MIGRATION = ('Replace bare TSV finding lines with one JSON object per line: '
             '{"id":"GHSA-...","package":"demo","ecosystem":"crates.io",'
             '"owner":"team","rationale":"reason","expiry":"YYYY-MM-DD"}. '
             'High/critical findings also require "reachability":"not-reachable" '
             'and nonempty "evidence". New advisories still fail the table.')
SEVERITIES = {"informational": 0, "low": 1, "medium": 2, "high": 3, "critical": 4}


def nonempty(value, field):
    if not isinstance(value, str) or not value.strip():
        raise AuditError(f"{field} must be a nonempty string")
    return value


def identity(entry):
    if not isinstance(entry, dict):
        raise AuditError("a finding or triage entry must be an object")
    return tuple(nonempty(entry.get(key), key) for key in ("id", "package", "ecosystem"))


def expiry(value):
    if isinstance(value, datetime.datetime):
        return value.date()
    if isinstance(value, datetime.date):
        return value
    if not isinstance(value, str):
        raise AuditError("expiry must be an ISO date (YYYY-MM-DD)")
    try:
        return datetime.date.fromisoformat(value)
    except ValueError as error:
        raise AuditError("expiry must be an ISO date (YYYY-MM-DD)") from error


def validate_entries(entries, today, findings=()):
    """Both file entries and every hook output line use this validator."""
    if not isinstance(entries, list):
        raise AuditError("triage must be a JSON array of entries")
    present = {identity(row) for row in findings}
    high = {identity(row) for row in findings if row["severity"] >= 3}
    result = {}
    allowed = {"id", "package", "ecosystem", "owner", "rationale", "expiry", "reachability", "evidence"}
    for entry in entries:
        key = identity(entry)
        if set(entry) - allowed:
            raise AuditError("unknown triage fields; " + MIGRATION)
        nonempty(entry.get("owner"), "owner")
        nonempty(entry.get("rationale"), "rationale")
        until = expiry(entry.get("expiry"))
        reachability = entry.get("reachability", "unknown")
        if reachability not in ("unknown", "reachable", "not-reachable"):
            raise AuditError("reachability must be unknown, reachable or not-reachable")
        if reachability == "not-reachable":
            nonempty(entry.get("evidence"), "not-reachable evidence")
        if key in high and (reachability != "not-reachable" or not entry.get("evidence")):
            raise AuditError(f"high/critical triage needs not-reachable evidence for {key}")
        if key in result:
            raise AuditError(f"duplicate triage entry for {key}")
        if until < today and key in present:
            raise AuditError(f"expired triage entry for {key}, expiry {until}; "
                             "renewal while the advisory remains live requires an operator merge")
        result[key] = entry
    return result


def parse_hook(text):
    entries = []
    for number, line in enumerate(text.splitlines(), 1):
        try:
            entry = json.loads(line)
            if not isinstance(entry, dict):
                raise ValueError("not an object")
        except ValueError as error:
            raise AuditError(f"audit-exceptions.sh line {number} is not a triage object. {MIGRATION}") from error
        entries.append(entry)
    return entries


def severity(vulnerability, groups):
    values = []
    stated = vulnerability.get("database_specific", {}).get("severity", "").lower()
    if stated in SEVERITIES:
        values.append(SEVERITIES[stated])
    if vulnerability.get("database_specific", {}).get("informational"):
        values.append(0)
    for group in groups:
        if vulnerability["id"] not in group.get("ids", []):
            continue
        score = group.get("max_severity")
        if score in (None, "", "UNKNOWN"):
            continue
        try:
            score = float(score)
        except (TypeError, ValueError) as error:
            raise AuditError("invalid OSV severity score") from error
        if not math.isfinite(score) or not 0 <= score <= 10:
            raise AuditError("OSV severity score outside 0..10")
        values.append(4 if score >= 9 else 3 if score >= 7 else 2 if score >= 4 else 1 if score > 0 else 0)
    if not values:
        # No severity is not proof of low risk. Treat it conservatively as high;
        # it needs the same trusted reachability evidence as a high advisory.
        return 3
    return max(values)


def findings(report, root):
    if not isinstance(report, dict) or not isinstance(report.get("results"), list):
        raise AuditError("OSV report lacks a results array")
    rows = []
    for source in report["results"]:
        if not isinstance(source, dict) or not isinstance(source.get("packages"), list):
            raise AuditError("OSV result lacks packages")
        path = nonempty(source.get("source", {}).get("path"), "source.path")
        path = Path(path)
        if path.is_absolute():
            try:
                # A runner temp path may be a platform alias (/var on macOS),
                # while the scanner reports its physical path. Compare physical
                # paths, retaining the boundary check for links outside the tree.
                path = path.resolve().relative_to(root.resolve())
            except ValueError as error:
                raise AuditError("OSV source is outside the scanned tree") from error
        if ".." in path.parts:
            raise AuditError("OSV source escapes the scanned tree")
        for package in source["packages"]:
            if not isinstance(package, dict):
                raise AuditError("invalid OSV package")
            vulnerabilities = package.get("vulnerabilities", [])
            if not isinstance(vulnerabilities, list):
                raise AuditError("invalid OSV vulnerabilities")
            for vulnerability in vulnerabilities:
                row = {"id": vulnerability.get("id"),
                       "package": package.get("package", {}).get("name"),
                       "ecosystem": package.get("package", {}).get("ecosystem"),
                       "path": path.as_posix(),
                       "version": package.get("package", {}).get("version", ""),
                       "aliases": vulnerability.get("aliases", [])}
                identity(row)
                row["severity"] = severity(vulnerability, package.get("groups", []))
                rows.append(row)
    return rows


def judge(base, head, triage, suppressions, level, today):
    if level == "off":
        return 0, ["Dependency audit is off; not run."]
    if level not in ("warn", "block"):
        raise AuditError("invalid effective audit level")
    entries = validate_entries(triage, today, head)
    previous = {identity(row) for row in base}
    failed = False
    messages = [f"::notice::expired triage entry {json.dumps(key)}, expiry {expiry(entry['expiry'])}; "
                "advisory absent from completed head scan"
                for key, entry in entries.items() if expiry(entry["expiry"]) < today]
    for row in head:
        key = identity(row)
        entry = entries.get(key)
        accepted = any(s["id"] in [row["id"], *row["aliases"]]
                       and s["directory"] == str(Path(row["path"]).parent)
                       and s["expiry"] >= today for s in suppressions)
        reason = ""
        if accepted:
            reason = "trusted unexpired suppression"
        elif key not in previous:
            reason = "new advisory"
        elif row["severity"] >= 3:
            if entry and entry.get("reachability") == "not-reachable" and entry.get("evidence"):
                reason = "trusted not-reachable triage"
            else:
                reason = "high/critical advisory without not-reachable evidence"
        elif row["severity"] == 2:
            reason = "trusted triage" if entry else "medium advisory without triage"
        else:
            reason = "existing low/informational advisory"
        passes = accepted or reason in ("trusted not-reachable triage", "trusted triage", "existing low/informational advisory")
        failed |= not passes
        prefix = "notice" if passes else "error" if level == "block" else "warning"
        # JSON escaping prevents newline and terminal-control injection in logs.
        messages.append(f"::{prefix}::{json.dumps(key)}: {reason}")
    return int(failed and level == "block"), messages


def regular(root, path):
    """Reject links in exception paths, including linked parent directories."""
    candidate = root
    for part in Path(path).parts:
        candidate = candidate / part
        if candidate.is_symlink():
            raise AuditError(f"trusted exception path is a symlink: {path}")
    if candidate.exists() and not candidate.is_file():
        raise AuditError(f"trusted exception path is not a regular file: {path}")
    return candidate


def scan(scanner, root, config):
    run = subprocess.run([str(scanner), "scan", "-r", "--no-ignore", "--verbosity", "error",
                          "--format", "json", "--config", str(config), "."],
                         cwd=root, capture_output=True, text=True, check=False)
    # Capture first, then match. There is no producer pipe whose SIGPIPE can
    # change an extraction-error match into a successful empty scan.
    text = run.stderr + "\n" + run.stdout
    if run.returncode == 128 and text.strip() == "No package sources found, --help for usage information.":
        return []
    if run.returncode not in (0, 1) or run.stderr.strip():
        raise AuditError(f"OSV scan failed or was incomplete (exit {run.returncode})")
    try:
        rows = findings(json.loads(run.stdout), root)
    except (ValueError, TypeError, AttributeError, KeyError) as error:
        raise AuditError("invalid or incomplete OSV JSON report") from error
    if bool(rows) != (run.returncode == 1):
        raise AuditError("OSV exit status disagrees with its findings")
    return rows


def trusted_exceptions(tree, head, trusted_sha, work, today):
    file = regular(tree, ".codeflow/audit-triage.json")
    entries = json.loads(file.read_text()) if file.exists() else []
    if not isinstance(entries, list):
        raise AuditError("audit-triage.json must contain an array")
    suppressions = []
    for file in tree.rglob("osv-scanner.toml"):
        file = regular(tree, file.relative_to(tree))
        for item in tomllib.loads(file.read_text()).get("IgnoredVulns", []):
            nonempty(item.get("id"), f"{file.relative_to(tree)} suppression id")
            nonempty(item.get("reason"), f"{file.relative_to(tree)} suppression reason")
            try:
                until = expiry(item.get("ignoreUntil"))
            except AuditError as error:
                raise AuditError(f"{file.relative_to(tree)} suppression ignoreUntil: {error}") from error
            suppressions.append({"id": item["id"], "expiry": until,
                                 "directory": str(file.parent.relative_to(tree))})
    hook = regular(tree, ".codeflow/audit-exceptions.sh")
    if hook.exists():
        listing = work / "findings.tsv"
        # Retain the hook input format; only its acceptance output changes.
        def tsv(value):
            return str(value).replace("\\", "\\\\").replace("\t", "\\t").replace("\n", "\\n").replace("\r", "\\r")
        listing.write_text("".join("\t".join(tsv(row[k]) for k in ("id", "package", "version", "ecosystem", "path"))
                                   + "\t" + tsv(",".join(row["aliases"])) + "\n" for row in head))
        env = {**os.environ, "CODEFLOW_AUDIT_FINDINGS": str(listing),
               "CODEFLOW_TRUSTED_ROOT": str(tree), "CODEFLOW_TRUSTED_SHA": trusted_sha}
        result = subprocess.run(["bash", str(hook)], cwd=tree, env=env, stdin=subprocess.DEVNULL,
                                capture_output=True, text=True, check=False)
        if result.returncode:
            if result.stderr.strip():
                print("audit-exceptions.sh stderr: " + json.dumps(result.stderr), file=sys.stderr)
            raise AuditError(f"trusted audit-exceptions.sh failed (exit {result.returncode}); {MIGRATION}")
        entries.extend(parse_hook(result.stdout))
    try:
        validate_entries(entries, today, head)
    except AuditError as error:
        raise AuditError(f"invalid trusted triage: {error}. {MIGRATION}") from error
    return entries, suppressions


# Dependency inputs only: no checkout scripts, scanner configuration, ignore
# rules, executables or custom scanner-result JSON enter the scan directory.
# Names follow OSV's supported lockfiles/manifests documentation:
# https://google.github.io/osv-scanner/supported-languages-and-lockfiles/
DEPENDENCY_INPUTS = frozenset({
    "Cargo.lock", "Gemfile.lock", "gems.locked", "Pipfile.lock", "poetry.lock",
    "requirements.txt", "pdm.lock", "pylock.toml", "uv.lock", "conan.lock",
    "pubspec.lock", "package_config.json", "mix.lock", "go.mod", "go.sum",
    "cabal.project.freeze", "stack.yaml.lock", "buildscript-gradle.lockfile",
    "gradle.lockfile", "verification-metadata.xml", "pom.xml", "bun.lock",
    "bun.lockb", "package-lock.json", "npm-shrinkwrap.json", "pnpm-lock.yaml",
    "yarn.lock", "deps.json", "packages.config", "packages.lock.json",
    "composer.lock", "renv.lock",
})


def dependency_tree(revision, destination):
    """Export committed dependency blobs only, refusing links and submodules."""
    destination.mkdir()
    listing = subprocess.run(["git", "ls-tree", "-rz", "--full-tree", revision],
                             capture_output=True, check=True).stdout
    for record in listing.split(b"\0"):
        if not record:
            continue
        meta, name = record.split(b"\t", 1)
        mode, kind, oid = meta.decode("ascii").split()
        path = Path(os.fsdecode(name))
        if kind == "commit":
            raise AuditError(f"dependency scan cannot inspect submodule {path}")
        if path.name not in DEPENDENCY_INPUTS and not path.name.endswith(".deps.json"):
            continue
        if mode not in ("100644", "100755") or kind != "blob":
            raise AuditError(f"dependency input is not a regular file: {path}")
        if path.is_absolute() or ".." in path.parts:
            raise AuditError("dependency input escapes scan directory")
        target = destination / path
        target.parent.mkdir(parents=True, exist_ok=True)
        content = subprocess.run(["git", "cat-file", "blob", oid],
                                 capture_output=True, check=True).stdout
        target.write_bytes(content)
    return destination


def run(scanner, trusted_sha, level, work):
    if level == "off":
        print("Dependency audit is off; not run.")
        return 0
    tree = work / "trusted-tree"
    tree.mkdir()
    archive = work / "trusted.tar"
    subprocess.run(["git", "archive", "-o", str(archive), trusted_sha], check=True)
    subprocess.run(["tar", "-xf", str(archive), "-C", str(tree)], check=True)
    config = work / "empty.toml"
    config.write_text("")
    # Explicit empty config overrides all checkout configs. Suppressions are
    # judged and printed by our table, never silently removed by the scanner.
    # Both scans use the same trusted scanner/config and the same input set.
    # Only committed dependency blobs vary; the checkout cannot supply config.
    base_inputs = dependency_tree(trusted_sha, work / "base-inputs")
    head_inputs = dependency_tree("HEAD", work / "head-inputs")
    base_paths = {path.relative_to(base_inputs).as_posix()
                  for path in base_inputs.rglob("*") if path.is_file()}
    head_paths = {path.relative_to(head_inputs).as_posix()
                  for path in head_inputs.rglob("*") if path.is_file()}
    for path in sorted(base_paths - head_paths):
        print(f"::warning::base dependency input missing at head: {json.dumps(path)}; "
              "deletion or rename can make its advisories absent from the head scan")
    base = scan(scanner, base_inputs, config)
    head = scan(scanner, head_inputs, config)
    today = datetime.datetime.now(datetime.timezone.utc).date()
    entries, suppressions = trusted_exceptions(tree, head, trusted_sha, work, today)
    code, messages = judge(base, head, entries, suppressions, level, today)
    for message in messages:
        print(message)
    return code


if __name__ == "__main__":
    try:
        sys.exit(run(Path(sys.argv[1]), sys.argv[2], sys.argv[3], Path(sys.argv[4])))
    except (AuditError, OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"::error::dependency audit failed closed: {json.dumps(str(error))}", file=sys.stderr)
        sys.exit(1)
