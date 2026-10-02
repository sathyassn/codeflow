#!/usr/bin/env python3
"""Compare the local instrumented Rust suite with the Windows raw referee.

Windows independently runs fmt, nextest (in partitions), doctests, clippy and
rustdoc. Unix-only journeys and the ignored read benchmark remain Linux-only.
Node versions, coordinator-owned binary paths and the parts of the split
Linux full gate are also checked.
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
    return cmd.startswith(("cargo fmt ", "cargo nextest ", "cargo clippy ", "cargo doc ", "cargo test --workspace")) or cmd.startswith('RUSTDOCFLAGS="-D warnings" cargo ')


SUITE = "cargo nextest run --workspace --no-fail-fast --profile codeflow"
DOCTESTS = "cargo test --workspace --doc"
COVERAGE = re.compile(r"^cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines \d+ --profile codeflow$")
COVERAGE_FORM = "cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines <N> --profile codeflow"


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
                    f"the complete `{SUITE}` and its doctests")
        if is_rust_verification_command(cmd):
            if cmd == DOCTESTS and not plain(target):
                notes.append(f"target '{target.get('name')}' runs `{DOCTESTS}` "
                             "with its own cwd or env; it does not stand for "
                             f"the complete `{SUITE}` and its doctests")
                cmd = f"{cmd} (with cwd or env)"
            out.add(cmd)
    if coverage and DOCTESTS in out:
        out.add(SUITE)
    return out


# TSK-203: the Windows suite runs in nextest partitions. A partitioned run
# stands for the whole suite only when its matrix runs every partition from
# 1 to N exactly once.
PARTITION = re.compile(r"^(.+) --partition count:\$\{\{ matrix\.partition \}\}/(\d+)$")


def matrix_partitions(job: str) -> list[int]:
    """The `partition: [..]` matrix values of a job, in order."""
    found = re.search(r"^\s*partition:\s*\[([^\]]*)\]\s*$", job, re.M)
    if not found:
        return []
    values = [v.strip() for v in found.group(1).split(",") if v.strip()]
    return [int(v) for v in values] if all(v.isdigit() for v in values) else []


def ci_rust_job_commands(workflow: str) -> set[str]:
    """Rust verification `run:` commands inside the Windows raw referee jobs
    (every job whose id starts with `windows`)."""
    out = set()
    for name, text in workflow_jobs(workflow).items():
        if not name.startswith("windows"):
            continue
        partitions = matrix_partitions(text)
        for run in re.finditer(r"^\s*run:\s*(.+?)\s*$", text, re.M):
            cmd = norm(run.group(1))
            if not is_rust_verification_command(cmd):
                continue
            split = PARTITION.match(cmd)
            if split:
                count = int(split.group(2))
                if partitions == list(range(1, count + 1)):
                    cmd = split.group(1)
                else:
                    cmd = f"{cmd} (partitions {partitions} do not cover 1 to {count})"
            out.add(cmd)
    return out


# TSK-203: the Linux full gate runs as matrix parts, each
# `codeflow test --mode full --strict --all --only <targets>`, behind one
# aggregate check named `codeflow gates`.
PART_RUN = "codeflow test --mode full --strict --all --only ${{ matrix.only }}"
VERDICT = "codeflow gates"


def requires_closure(cfg: dict, names: set[str]) -> set[str]:
    targets = {t["name"]: t for t in cfg.get("targets", [])}
    found: set[str] = set()
    pending = list(names)
    while pending:
        name = pending.pop()
        if name in found:
            continue
        found.add(name)
        pending.extend(targets.get(name, {}).get("requires", []))
    return found


def gate_part_problems(cfg: dict, workflow: str) -> list[str]:
    """The parts of the Linux full gate must together run every full-mode
    target with the same strictness, and one verdict must judge them all."""
    jobs = workflow_jobs(workflow)
    gate = jobs.get("gates", "")
    problems: list[str] = []
    runs = [norm(r) for r in re.findall(r"^\s*run:\s*(codeflow test .+?)\s*$", gate, re.M)]
    if runs != [PART_RUN]:
        problems.append(f"the gates job must run exactly `{PART_RUN}` once; it runs {runs}")
    parts = re.findall(r"^\s*only:\s*(\S+)\s*$", gate, re.M)
    named = [n for part in parts for n in part.split(",") if n]
    owed = {
        t["name"] for t in cfg.get("targets", [])
        if applicable(t) and t.get("modes", {}).get("full")
    }
    unknown = sorted(set(named) - owed)
    if unknown:
        problems.append(f"gate parts name targets with no enabled full mode: {unknown}")
    missing = sorted(owed - requires_closure(cfg, set(named)))
    if missing:
        problems.append(f"no gate part runs these full-mode targets: {missing}")
    twice = sorted({n for n in named if named.count(n) > 1})
    if twice:
        problems.append(f"more than one gate part names: {twice}")
    problems.extend(gate_step_problems(gate, workflow))
    verdicts = [
        (name, text) for name, text in jobs.items()
        if re.search(rf"^ {{4}}name:\s*{re.escape(VERDICT)}\s*$", text, re.M)
    ]
    if len(verdicts) != 1:
        problems.append(f"exactly one job must be named `{VERDICT}`; found {[n for n, _ in verdicts]}")
    else:
        name, text = verdicts[0]
        condition = re.search(r"^ {4}if:\s*(.+?)\s*$", gate, re.M)
        expected = verdict_job(condition.group(1) if condition else "")
        if norm_job(text) != norm_job(expected):
            problems.append(
                f"`{VERDICT}` ({name}) must be exactly the verdict job: run "
                "`if: always() && (<the gates job's condition>)`, need the gates job, "
                "and fail unless needs.gates.result is success; any other form could "
                "let a skipped, failed or cancelled part read as passed. Expected:\n"
                + expected)
    return problems


# The one accepted form of the verdict job. A substring check let a false
# condition, a forgiven exit or `continue-on-error` through (TSK-203 review),
# so the job must match this text exactly, apart from blank lines, comments
# and trailing spaces; `{condition}` is the gates job's own `if:` condition.
VERDICT_JOB = """\
    if: always() && ({condition})
    name: codeflow gates
    needs: gates
    runs-on: ubuntu-24.04
    steps:
      - name: Every part of the full gate passed
        shell: bash
        env:
          PARTS: ${{{{ needs.gates.result }}}}
        run: |
          if [ "$PARTS" != success ]; then
            echo "::error::the full gate's parts concluded ${{PARTS}}; see the codeflow gates (part) jobs"
            exit 1
          fi
          echo "every part of the full gate passed"
"""


def verdict_job(condition: str) -> str:
    return VERDICT_JOB.format(condition=condition)


def norm_job(text: str) -> list[str]:
    """A job's lines without blank lines, comment lines or trailing spaces."""
    return [line.rstrip() for line in text.splitlines()
            if line.strip() and not line.lstrip().startswith("#")]


# A step key is the first key after `- ` or a key at the step's indent, so a
# check holds whatever order the keys are written in.
STEP_KEY = re.compile(r"^(?: {6}- | {8})([A-Za-z_-]+):\s*(.*?)\s*$", re.M)
EXPRESSION = re.compile(r"\$\{\{.*?\}\}")


def gate_step_problems(gate: str, workflow: str = "") -> list[str]:
    """The parts job and its gate step must not be skippable, forgiven or
    run by an inherited shell, and nothing in the workflow may reshape them:
    no `defaults`, no `BASH_ENV`, no YAML merge keys, aliases or flow
    mappings, which a text check cannot follow."""
    problems = []
    if re.search(r"^\s*(?:- )?continue-on-error:", gate, re.M):
        problems.append("the gates job must not use `continue-on-error`; a forgiven part reads as passed")
    if re.search(r"^defaults:|^ {4}defaults:", workflow + "\n" + gate, re.M):
        problems.append("the workflow and the gates job must not set `defaults`; an inherited shell or "
                        "working directory changes what the gate and its verdict run")
    if "BASH_ENV" in workflow or "BASH_ENV" in gate:
        problems.append("the workflow must not set `BASH_ENV`; it runs code before every bash step")
    plain = EXPRESSION.sub("", gate)
    if re.search(r"<<:|:\s*[&*][A-Za-z]|^\s*-\s*[&*][A-Za-z]|(?::|^\s*-)\s*\{", plain, re.M):
        problems.append("the gates job must not use YAML anchors, aliases, merge keys or flow mappings; "
                        "write its keys out so they can be checked")
    steps = [s for s in re.split(r"\n(?= {6}- )", gate) if PART_RUN in norm(s)]
    if len(steps) != 1:
        problems.append(f"exactly one gates step must run `{PART_RUN}`; found {len(steps)}")
    for step in steps:
        keys = {key: value for key, value in STEP_KEY.findall(step)}
        if "if" in keys:
            problems.append("the gate step must not carry its own `if:`; a skipped step reads as passed")
        if keys.get("shell") != "bash":
            problems.append("the gate step must pin `shell: bash`, so no inherited default decides how it runs")
        if "working-directory" in keys:
            problems.append("the gate step must not set `working-directory`; the gate runs at the repository root")
    return problems


# TSK-203: `npm run deps:install` runs `npm ci`, which deletes and rewrites
# the workspace's node_modules. Two targets installing the same workspace can
# overlap under max_parallel and delete each other's packages, so one target
# installs each workspace and every other target on that workspace's Node pin
# requires it.
INSTALL = re.compile(r"npm run deps:install --prefix (\S+?)(?:\s|\"|$)")


def shared_install_problems(cfg: dict) -> list[str]:
    targets = [t for t in cfg.get("targets", []) if applicable(t)]
    commands = {t["name"]: norm(t.get("modes", {}).get("full", {}).get("command", "")) for t in targets}
    installers: dict[str, list[str]] = {}
    for name, cmd in commands.items():
        for prefix in INSTALL.findall(cmd):
            installers.setdefault(prefix, []).append(name)
    problems = []
    for prefix, names in sorted(installers.items()):
        if len(names) != 1:
            problems.append(f"more than one target installs {prefix} dependencies: {sorted(names)}; "
                            "concurrent installs delete each other's packages, so keep one installer")
            continue
        installer = names[0]
        for name, cmd in sorted(commands.items()):
            if name == installer or node_pin(cmd) != f"{prefix}/.node-version":
                continue
            if installer not in requires_closure(cfg, {name}) - {name}:
                problems.append(f"target '{name}' runs on {prefix}'s Node pin but does not require "
                                f"'{installer}', so it can start before or during the install")
    return problems


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


def gate_binary_problems(cfg: dict) -> list[str]:
    problems = []
    targets = {t["name"]: t for t in cfg.get("targets", [])}
    def closure(name, seen=None):
        seen = set() if seen is None else seen
        if name in seen:
            return seen
        seen.add(name)
        for required in targets.get(name, {}).get("requires", []):
            closure(required, seen)
        return seen
    producer = targets.get("codeflow-bin", {})
    for target in targets.values():
        cmd = norm(target.get("modes", {}).get("full", {}).get("command", ""))
        if not re.search(r"check:real-browser(\s|$)", cmd):
            continue
        if "codeflow-bin" not in closure(target["name"]) or producer.get("outputs") != ["target/debug/codeflow"] or not applicable(producer):
            problems.append(f"target '{target['name']}' needs codeflow-bin's declared output; the coordinator sets its absolute path following CARGO_TARGET_DIR")
        if "CF_PRESENT_CODEFLOW=" in cmd:
            problems.append(f"target '{target['name']}' must use the coordinator's absolute binary path following CARGO_TARGET_DIR")
    return problems


def main() -> int:
    cfg = json.loads(CONFIG.read_text())
    workflow = WORKFLOW.read_text()
    pins = (node_pin_problems(cfg, workflow) + gate_binary_problems(cfg)
            + gate_part_problems(cfg, workflow) + shared_install_problems(cfg))
    for problem in pins:
        print(f"GATE PARITY DRIFT: {problem}", file=sys.stderr)
    status = rust_parity()
    if pins:
        return 1
    if status == 0:
        print("gate-parity OK: Node targets run on their CI pins, the "
              "real-browser check runs the gate's binary, and the gate parts "
              "run every full-mode target")
    return status


def rust_parity() -> int:
    notes: list[str] = []
    local = local_rust_commands(json.loads(CONFIG.read_text()), notes)
    ci = ci_rust_job_commands(WORKFLOW.read_text())
    if not ci:
        print("gate-parity: could not find the Windows jobs' cargo commands; "
              "the workflow layout changed; update this guard.", file=sys.stderr)
        return 1
    if local != ci:
        print("GATE PARITY DRIFT: local test gate and Windows jobs disagree.",
              file=sys.stderr)
        print(f"  only in local (.codeflow/test-config.json, full): "
              f"{sorted(local - ci)}", file=sys.stderr)
        print(f"  only in CI (.github/workflows/codeflow-ci.yml, windows jobs): "
              f"{sorted(ci - local)}", file=sys.stderr)
        for note in notes:
            print(f"  note: {note}", file=sys.stderr)
        print("  fix: make both run the same cargo commands so local "
              "`codeflow test` matches CI.", file=sys.stderr)
        return 1
    print(f"gate-parity OK: local and the Windows jobs run the same: {sorted(local)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
