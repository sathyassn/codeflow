#!/usr/bin/env python3
"""Render the qualification result matrix and refresh the release record.

Reads the TSV that qualify.sh accumulates, writes the standalone matrix
document, and rewrites the delimited qualification block inside the release
evidence record so the candidate commit, the binary digest and the verdict
there always describe the latest run.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

FIELDS = ("sample", "tier", "command", "check", "status", "expected", "observed", "owner")
STATUSES = ("passed", "failed", "unavailable")
TIERS = ("minimal", "standard", "full")

BEGIN = "<!-- cli-qualification:begin -->"
END = "<!-- cli-qualification:end -->"


def parse_rows(path: pathlib.Path) -> list[dict[str, str]]:
    rows = []
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) != len(FIELDS):
            raise SystemExit(f"{path}:{number}: expected {len(FIELDS)} fields, got {len(parts)}")
        row = dict(zip(FIELDS, parts))
        if row["status"] not in STATUSES:
            raise SystemExit(f"{path}:{number}: unknown status {row['status']!r}")
        if not row["expected"] or not row["observed"]:
            raise SystemExit(f"{path}:{number}: blank expected or observed")
        if row["status"] == "unavailable" and not row["owner"]:
            raise SystemExit(f"{path}:{number}: unavailable without an owner")
        rows.append(row)
    if not rows:
        raise SystemExit(f"{path}: no results recorded")
    return rows


def plain(value: str) -> str:
    """Replace em and en dashes with a hyphen.

    The project's writing rules forbid both characters in tracked files, and
    both appear in command output and in scaffolded files this record quotes,
    so the quotation is normalized rather than the rule broken.
    """
    return value.replace("\u2014", "-").replace("\u2013", "-")


def cell(value: str) -> str:
    return plain(value).replace("|", "/").replace("\n", " ").strip() or "-"


def matrix_table(rows: list[dict[str, str]]) -> list[str]:
    lines = [
        "| Command | Check | Result | Expected | Observed | Unavailable owner |",
        "|---|---|---|---|---|---|",
    ]
    for row in rows:
        lines.append(
            "| `{command}` | {check} | **{status}** | {expected} | {observed} | {owner} |".format(
                command=cell(row["command"]),
                check=cell(row["check"]),
                status=row["status"],
                expected=cell(row["expected"]),
                observed=cell(row["observed"]),
                owner=cell(row["owner"]) if row["owner"] else "-",
            )
        )
    return lines


DIFF_LINE_CAP = 40


def diff_section(diffs: pathlib.Path) -> list[str]:
    """Embed the recorded before/after evidence so the record stands alone.

    The run directory is disposable, so a matrix row that only names a diff
    file would cite evidence that teardown removed.
    """
    if not diffs.is_dir():
        return []
    files = sorted(path for path in diffs.iterdir() if path.is_file())
    if not files:
        return []
    lines = ["## Recorded before and after evidence", ""]
    lines.append(
        "Each block is the file the matrix row above cites, embedded here because "
        "the run directory it was written in is deleted at teardown. An empty block "
        "means the comparison found no difference."
    )
    lines.append("")
    for path in files:
        body = plain(path.read_text(encoding="utf-8", errors="replace").rstrip())
        kept = body.splitlines()[:DIFF_LINE_CAP]
        if len(body.splitlines()) > DIFF_LINE_CAP:
            kept.append(f"... truncated at {DIFF_LINE_CAP} lines")
        lines.append(f"### {path.name}")
        lines.append("")
        lines.append("```text")
        lines.append("\n".join(kept) if kept else "(no difference)")
        lines.append("```")
        lines.append("")
    return lines


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--results", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--binary-sha", required=True)
    parser.add_argument("--binary-version", required=True)
    parser.add_argument("--started", required=True)
    parser.add_argument("--finished", required=True)
    parser.add_argument("--work-dir", required=True)
    parser.add_argument("--teardown", required=True)
    parser.add_argument("--diffs", required=True)
    args = parser.parse_args()

    rows = parse_rows(pathlib.Path(args.results))
    counts = {status: sum(1 for row in rows if row["status"] == status) for status in STATUSES}
    verdict = "blocked" if counts["failed"] else "qualified"

    out = pathlib.Path(args.out)
    lines: list[str] = []
    lines.append("# CodeFlow v3.0.0 CLI qualification matrix")
    lines.append("")
    lines.append(
        "Generated by `tests/release-qualification/qualify.sh`. Every row records "
        "exactly one of passed, failed or unavailable with expected versus observed; "
        "each unavailable names the owner the gate belongs to. Quoted output has "
        "em and en dashes replaced by a hyphen, which the project's writing rules "
        "require; nothing else in it is altered. Re-running the script against a "
        "later integration head replaces this file."
    )
    lines.append("")
    lines.append("## Run identity")
    lines.append("")
    lines.append("| Field | Value |")
    lines.append("|---|---|")
    lines.append(f"| Candidate commit | `{args.commit}` |")
    lines.append(f"| Binary SHA-256 | `{args.binary_sha}` |")
    lines.append(f"| Binary version | `{args.binary_version}` |")
    lines.append(f"| Started (UTC) | {args.started} |")
    lines.append(f"| Finished (UTC) | {args.finished} |")
    lines.append(f"| Disposable root | `{args.work_dir}` |")
    lines.append("")
    lines.append(
        f"Verdict: **{verdict}** - {counts['passed']} passed, "
        f"{counts['failed']} failed, {counts['unavailable']} unavailable."
    )
    if counts["failed"]:
        lines.append("")
        lines.append(
            "A failed check blocks qualification. Every failure is listed in the "
            "Failures section below and must be judged by the responsible primary "
            "before this record can carry a qualified verdict."
        )
    lines.append("")

    if counts["failed"]:
        lines.append("## Failures")
        lines.append("")
        lines.append("| Sample | Tier | Command | Check | Expected | Observed |")
        lines.append("|---|---|---|---|---|---|")
        for row in rows:
            if row["status"] != "failed":
                continue
            lines.append(
                "| {sample} | {tier} | `{command}` | {check} | {expected} | {observed} |".format(
                    sample=cell(row["sample"]),
                    tier=cell(row["tier"]),
                    command=cell(row["command"]),
                    check=cell(row["check"]),
                    expected=cell(row["expected"]),
                    observed=cell(row["observed"]),
                )
            )
        lines.append("")

    unavailable = [row for row in rows if row["status"] == "unavailable"]
    if unavailable:
        lines.append("## Unavailable checks and their owners")
        lines.append("")
        lines.append("| Sample | Tier | Command | Check | Reason | Owner |")
        lines.append("|---|---|---|---|---|---|")
        for row in unavailable:
            lines.append(
                "| {sample} | {tier} | `{command}` | {check} | {observed} | {owner} |".format(
                    sample=cell(row["sample"]),
                    tier=cell(row["tier"]),
                    command=cell(row["command"]),
                    check=cell(row["check"]),
                    observed=cell(row["observed"]),
                    owner=cell(row["owner"]),
                )
            )
        lines.append("")

    lines.append("## Result matrix")
    lines.append("")
    seen: list[tuple[str, str]] = []
    for row in rows:
        key = (row["sample"], row["tier"])
        if key not in seen:
            seen.append(key)
    for sample, tier in seen:
        group = [row for row in rows if (row["sample"], row["tier"]) == (sample, tier)]
        group_counts = {
            status: sum(1 for row in group if row["status"] == status) for status in STATUSES
        }
        heading = f"{sample}, {tier} tier" if tier in TIERS else sample
        lines.append(f"### {heading}")
        lines.append("")
        lines.append(
            f"{group_counts['passed']} passed, {group_counts['failed']} failed, "
            f"{group_counts['unavailable']} unavailable."
        )
        lines.append("")
        lines.extend(matrix_table(group))
        lines.append("")

    lines.extend(diff_section(pathlib.Path(args.diffs)))

    lines.append("## Teardown")
    lines.append("")
    lines.append("```text")
    teardown = pathlib.Path(args.teardown)
    lines.append(
        plain(teardown.read_text(encoding="utf-8").rstrip())
        if teardown.exists()
        else "no teardown log"
    )
    lines.append("```")
    lines.append("")

    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")

    refresh_evidence(pathlib.Path(args.evidence), out, args, counts, verdict, rows)
    return 0


def unavailable_sentence(rows: list[dict[str, str]]) -> str:
    """Name the checks that did not run, so the block is not read as full cover."""
    names: list[str] = []
    for row in rows:
        if row["status"] != "unavailable":
            continue
        name = f"`{cell(row['command'])}` ({cell(row['check'])})"
        if name not in names:
            names.append(name)
    if not names:
        return "Every check in the matrix ran."
    return (
        "These checks did not run here and stay with the owners the matrix names: "
        + "; ".join(names)
        + "."
    )


def refresh_evidence(evidence, out, args, counts, verdict, rows) -> None:
    """Rewrite the delimited qualification block inside the release record."""
    relative = out.name
    block = [
        BEGIN,
        "",
        "## CLI qualification on sample projects",
        "",
        f"The candidate at `{args.commit}` was built and qualified against generated "
        "greenfield (Rust and Node) and brownfield sample repositories at the minimal, "
        "standard and full tiers. The installed binary's SHA-256 was "
        f"`{args.binary_sha}` and it reported `{args.binary_version}`.",
        "",
        f"Verdict: **{verdict}** - {counts['passed']} passed, {counts['failed']} failed, "
        f"{counts['unavailable']} unavailable. Every check carries expected versus "
        "observed, and each unavailable names its owner. The full matrix, the "
        "before/after diffs it cites, and the teardown output are in "
        f"[`{relative}`]({relative}).",
        "",
        unavailable_sentence(rows),
        "",
        "A failed check blocks qualification; this section states the verdict the "
        "matrix produced and never infers a native-platform or hosted-publication "
        "result, which stay with their own owners.",
        "",
        END,
    ]
    text = evidence.read_text(encoding="utf-8") if evidence.exists() else ""
    rendered = "\n".join(block)
    if BEGIN in text and END in text:
        head = text.split(BEGIN)[0]
        tail = text.split(END, 1)[1]
        evidence.write_text(head + rendered + tail, encoding="utf-8")
    else:
        separator = "" if text.endswith("\n\n") or not text else "\n"
        evidence.write_text(text + separator + rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    sys.exit(main())
