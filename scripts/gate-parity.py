#!/usr/bin/env python3
"""Gate parity guard (Option B sync-guard).

codeflow's own CI keeps a raw `rust (format + test + clippy)` job that runs cargo
directly — an independent referee that catches a bug in codeflow's own test
runner. That independence is worth a duplicate command list, but a duplicate can
drift: the exact failure where CI ran clippy and the local gate did not.

This guard makes drift fail LOUDLY instead of silently. It asserts that the
Rust verification commands in the CI `rust` job exactly equal the `full`-mode
commands in `.codeflow/test-config.json` (what local `codeflow test` and the CI
`codeflow gates` job run). It runs as a `codeflow test` target, so it fires both
locally and in CI.

Scope note: only the `rust` job is compared. Other jobs (coverage llvm-cov and
doc-graph validation) run different tooling by design and are out of scope here.
The local full gate runs the suite once under coverage plus the doctests; that
pair is compared as the `rust` job's plain `cargo test --workspace`.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".codeflow" / "test-config.json"
WORKFLOW = ROOT / ".github" / "workflows" / "codeflow-ci.yml"


def norm(cmd: str) -> str:
    return " ".join(cmd.split())


def is_rust_verification_command(cmd: str) -> bool:
    return (
        cmd.startswith("cargo ")
        and not cmd.startswith("cargo llvm-cov ")
    ) or cmd.startswith('RUSTDOCFLAGS="-D warnings" cargo ')


# The full gate runs the test suite once (TSK-134): under coverage, which
# skips doctests, plus the doctests alone. Together they are the CI referee's
# plain `cargo test --workspace`, so that pair stands for it here.
SUITE = "cargo test --workspace"
DOCTESTS = "cargo test --workspace --doc"
COVERAGE_PREFIX = "cargo llvm-cov --workspace "


def local_rust_commands() -> set[str]:
    cfg = json.loads(CONFIG.read_text())
    out = set()
    coverage = False
    for target in cfg.get("targets", []):
        cmd = target.get("modes", {}).get("full", {}).get("command", "")
        coverage = coverage or norm(cmd).startswith(COVERAGE_PREFIX)
        if is_rust_verification_command(cmd):
            out.add(norm(cmd))
    if coverage and DOCTESTS in out:
        out.discard(DOCTESTS)
        out.add(SUITE)
    return out


def ci_rust_job_commands() -> set[str]:
    """Rust verification `run:` commands inside the `rust:` job block only."""
    out = set()
    in_rust = False
    for line in WORKFLOW.read_text().splitlines():
        job = re.match(r"^ {2}([A-Za-z0-9_-]+):\s*$", line)
        if job:  # a top-level job key (2-space indent)
            in_rust = job.group(1) == "rust"
            continue
        if in_rust:
            run = re.match(r"^\s*run:\s*(.+?)\s*$", line)
            if run and is_rust_verification_command(run.group(1)):
                out.add(norm(run.group(1)))
    return out


def main() -> int:
    local = local_rust_commands()
    ci = ci_rust_job_commands()
    if not ci:
        print("gate-parity: could not find the CI `rust` job cargo commands — "
              "the workflow layout changed; update this guard.", file=sys.stderr)
        return 1
    if local != ci:
        print("GATE PARITY DRIFT — local test gate and CI `rust` job disagree.",
              file=sys.stderr)
        print(f"  only in local (.codeflow/test-config.json, full): "
              f"{sorted(local - ci)}", file=sys.stderr)
        print(f"  only in CI (.github/workflows/codeflow-ci.yml, rust job): "
              f"{sorted(ci - local)}", file=sys.stderr)
        print("  fix: make both run the same cargo commands so local "
              "`codeflow test` matches CI.", file=sys.stderr)
        return 1
    print(f"gate-parity OK — local and CI `rust` job run the same: {sorted(local)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
