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

It also holds two local-gate pins to CI (TSK-142). Each target that runs
Node does so through `scripts/with-node.py` and a version file, and every
`actions/setup-node` step for that directory, including one in the job that
runs the full gate, pins the same version (AC-1). The present real-browser
check is handed the `codeflow` binary the gate built, wherever
`CARGO_TARGET_DIR` points (AC-2).

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
# plain `cargo test --workspace`, so that pair stands for it here, but only in
# exactly these forms: a coverage command with a test filter, a narrower
# target selection or a feature change runs less than CI does. The line
# threshold does not change what runs, so any value is accepted.
SUITE = "cargo test --workspace"
DOCTESTS = "cargo test --workspace --doc"
COVERAGE = re.compile(
    r"^cargo llvm-cov --workspace --summary-only --fail-under-lines \d+$")
COVERAGE_FORM = ("cargo llvm-cov --workspace --summary-only "
                 "--fail-under-lines <N>")


def applicable(target: dict) -> bool:
    """A target the CI full gate runs: enabled and not skipped in CI."""
    return target.get("enabled", True) is not False and not target.get("ci_skip")


def plain(target: dict) -> bool:
    """No working directory or environment that could change what runs."""
    return not target.get("cwd") and not target.get("env")


def local_rust_commands(cfg: dict, notes: list[str] | None = None) -> set[str]:
    """Rust verification commands the CI full gate runs from this config."""
    notes = [] if notes is None else notes
    out = set()
    coverage = False
    for target in cfg.get("targets", []):
        cmd = norm(target.get("modes", {}).get("full", {}).get("command", ""))
        if not cmd or not applicable(target):
            continue
        if cmd.startswith("cargo llvm-cov "):
            if COVERAGE.match(cmd) and plain(target):
                coverage = True
            else:
                notes.append(
                    f"target '{target.get('name')}' runs coverage as {cmd!r}"
                    f"{' with its own cwd or env' if not plain(target) else ''}; "
                    f"only `{COVERAGE_FORM}` with no cwd or env stands for "
                    f"half of `{SUITE}`")
        if is_rust_verification_command(cmd):
            if cmd == DOCTESTS and not plain(target):
                notes.append(f"target '{target.get('name')}' runs `{DOCTESTS}` "
                             "with its own cwd or env; it does not stand for "
                             f"half of `{SUITE}`")
                cmd = f"{cmd} (with cwd or env)"
            out.add(cmd)
    if coverage and DOCTESTS in out:
        out.discard(DOCTESTS)
        out.add(SUITE)
    return out


def ci_rust_job_commands(workflow: str) -> set[str]:
    """Rust verification `run:` commands inside the `rust:` job block only."""
    out = set()
    in_rust = False
    for line in workflow.splitlines():
        job = re.match(r"^ {2}([A-Za-z0-9_-]+):\s*$", line)
        if job:  # a top-level job key (2-space indent)
            in_rust = job.group(1) == "rust"
            continue
        if in_rust:
            run = re.match(r"^\s*run:\s*(.+?)\s*$", line)
            if run and is_rust_verification_command(run.group(1)):
                out.add(norm(run.group(1)))
    return out


# TSK-142 AC-1: a Node target names its version file to the launcher.
WITH_NODE = re.compile(r'^python3 -B scripts/with-node\.py (\S+) "([^"]+)"$')
RUNS_NODE = re.compile(r"(^|[\s;&|(])(npm|npx|node)\s")
EXACT_VERSION = re.compile(r"^v?(\d+\.\d+\.\d+)$")
FULL_GATE = re.compile(r"^\s*run:\s*codeflow test --mode full\b", re.M)


def with_node_parts(cmd: str) -> tuple[str | None, str]:
    """(version file, inner command) of a target command; no file when the
    command does not run through the launcher."""
    match = WITH_NODE.match(norm(cmd))
    return (match.group(1), match.group(2)) if match else (None, cmd)


def node_pin(cmd: str) -> str | None:
    return with_node_parts(cmd)[0]


def cmd_segments(command: str) -> list[str]:
    """Split at cmd.exe operators outside double quotes; caret escapes one character."""
    segments = []
    quoted = False
    start = 0
    index = 0
    while index < len(command):
        char = command[index]
        if char == "^":
            index += 2
            continue
        if char == '"':
            quoted = not quoted
        elif char in "&|" and not quoted:
            segments.append(command[start:index].strip())
            if index + 1 < len(command) and command[index + 1] == char:
                index += 1
            start = index + 1
        index += 1
    segments.append(command[start:].strip())
    return segments


def workflow_jobs(workflow: str) -> dict[str, str]:
    """Each top-level job's text, keyed by job id."""
    jobs: dict[str, list[str]] = {}
    current = None
    for line in workflow.splitlines():
        job = re.match(r"^ {2}([A-Za-z0-9_-]+):\s*$", line)
        if job:
            current = job.group(1)
            jobs[current] = []
        elif re.match(r"^\S", line):
            current = None
        elif current:
            jobs[current].append(line)
    return {name: "\n".join(lines) for name, lines in jobs.items()}


def setup_node_steps(job: str) -> list[tuple[str, list[str]]]:
    """(node-version, lock files) of each setup-node step in a job."""
    steps = re.split(r"\n(?= {6}- )", job)
    found = []
    for step in steps:
        if "uses: actions/setup-node@" not in step:
            continue
        version = re.search(r"node-version:\s*['\"]?([^\s'\"]+)", step)
        locks = re.findall(r"[\w./-]*package-lock\.json", step)
        found.append((version.group(1) if version else "", locks))
    return found


def version_file_problems(root: Path, pin: str) -> tuple[str | None, list[str]]:
    path = root / pin
    if not path.is_file():
        return None, [f"{pin} does not exist; it must hold the Node version "
                      "its target runs on"]
    text = path.read_text(encoding="utf-8").strip()
    match = EXACT_VERSION.match(text)
    if not match:
        return None, [f"{pin} must hold one exact Node version such as 24.18.0; "
                      f"it holds {text!r}"]
    return match.group(1), []


def node_pin_problems(cfg: dict, workflow: str, root: Path = ROOT) -> list[str]:
    """Node targets whose local pin and CI pins disagree (TSK-142 AC-1)."""
    problems: list[str] = []
    jobs = workflow_jobs(workflow)
    gate_jobs = [name for name, text in jobs.items() if FULL_GATE.search(text)]
    for target in cfg.get("targets", []):
        name = target.get("name")
        cmd = norm(target.get("modes", {}).get("full", {}).get("command", ""))
        if not cmd or not applicable(target):
            continue
        pin, inner = with_node_parts(cmd)
        if pin is None:
            if RUNS_NODE.search(cmd):
                problems.append(
                    f"target '{name}' runs Node without a Node pin or the required "
                    "double quotes; run it as python3 -B scripts/with-node.py "
                    '<version file> "<command>"')
            continue
        if cmd_segments(cmd) != [cmd]:
            problems.append(f"target '{name}' has an inner chain that cmd.exe splits; "
                            "put the whole chain in double quotes")
        version, found = version_file_problems(root, pin)
        problems += [f"target '{name}': {p}" for p in found]
        if version is None:
            continue
        directory = str(Path(pin).parent).replace("\\", "/")
        package = root / directory / "package.json"
        if package.is_file():
            engines = json.loads(package.read_text()).get("engines", {}).get("node", "")
            exact = EXACT_VERSION.match(engines)
            if exact and exact.group(1) != version:
                problems.append(
                    f"target '{name}': {directory}/package.json engines pins Node "
                    f"{exact.group(1)} but {pin} pins {version}")
        installed_for_gate = False
        for job, text in jobs.items():
            for ci_version, locks in setup_node_steps(text):
                if not any(lock.startswith(f"{directory}/") for lock in locks):
                    continue
                if ci_version != version:
                    problems.append(
                        f"target '{name}': CI job '{job}' pins Node {ci_version} "
                        f"for {directory}, but {pin} pins {version}")
                elif job in gate_jobs:
                    installed_for_gate = True
        if not installed_for_gate:
            problems.append(
                f"target '{name}': the CI job that runs the full gate "
                f"({', '.join(gate_jobs) or 'none found'}) does not install Node "
                f"{version} for {directory} (an actions/setup-node step with "
                f"{directory}/package-lock.json in cache-dependency-path)")
    return problems


# TSK-142 AC-2: the real-browser check runs the binary this gate built. The
# directory is resolved from the repository root, where the gate runs cargo,
# so a relative or absolute CARGO_TARGET_DIR both work.
REAL_BROWSER = "npm run check:real-browser --prefix crates/codeflow-present/web"
GATE_BINARY = 'CF_PRESENT_CODEFLOW=$(python3 -B scripts/gate-binary.py)'


def gate_binary_problems(cfg: dict) -> list[str]:
    problems = []
    for target in cfg.get("targets", []):
        cmd = norm(target.get("modes", {}).get("full", {}).get("command", ""))
        if not re.search(r"check:real-browser(\s|$)", cmd):
            continue
        if f"{GATE_BINARY} {REAL_BROWSER}" not in cmd:
            problems.append(
                f"target '{target.get('name')}' runs the real-browser check without "
                "the binary the gate built; prefix it with "
                f"{GATE_BINARY} so it follows CARGO_TARGET_DIR")
    return problems


def main() -> int:
    cfg = json.loads(CONFIG.read_text())
    workflow = WORKFLOW.read_text()
    pins = node_pin_problems(cfg, workflow) + gate_binary_problems(cfg)
    for problem in pins:
        print(f"GATE PARITY DRIFT: {problem}", file=sys.stderr)
    status = rust_parity()
    if pins:
        return 1
    if status == 0:
        print("gate-parity OK: Node targets run on their CI pins, and the "
              "real-browser check runs the gate's binary")
    return status


def rust_parity() -> int:
    notes: list[str] = []
    local = local_rust_commands(json.loads(CONFIG.read_text()), notes)
    ci = ci_rust_job_commands(WORKFLOW.read_text())
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
        for note in notes:
            print(f"  note: {note}", file=sys.stderr)
        print("  fix: make both run the same cargo commands so local "
              "`codeflow test` matches CI.", file=sys.stderr)
        return 1
    print(f"gate-parity OK — local and CI `rust` job run the same: {sorted(local)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
