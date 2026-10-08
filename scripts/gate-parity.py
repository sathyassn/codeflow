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
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".codeflow" / "test-config.json"
WORKFLOW = ROOT / ".github" / "workflows" / "codeflow-ci.yml"
DIST_CONFIG = ROOT / "dist-workspace.toml"


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
    timeout-minutes: 5
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


# A text check can only judge the forms it knows, so the workflow's top
# level, the gates job's own keys, its strategy and its gate step are
# allowlisted. Any other spelling (a quoted or escaped key, a continued
# value, a second `codeflow test` command, defaults or env that a bash step
# inherits) is refused rather than read past. This catches drift in a
# reviewed edit; it is not a boundary against someone who also edits this
# script in the same change.
TOP_KEYS = ("name", "on", "concurrency", "permissions", "jobs")
GATES_KEYS = ("if", "name", "runs-on", "timeout-minutes", "strategy", "steps")
STRATEGY_HEAD = ["    strategy:", "      fail-fast: false", "      matrix:", "        include:"]
PART_LINE = re.compile(r"^          - part: [a-z0-9-]+$")
ONLY_LINE = re.compile(r"^            only: [a-z0-9-]+(?:,[a-z0-9-]+)*$")
GATE_STEP = """\
      - name: codeflow test --mode full --strict
        shell: bash
        env:
          LLVM_PROFILE_FILE_NAME: codeflow-%4m.profraw
        run: codeflow test --mode full --strict --all --only ${{ matrix.only }}
"""


def gate_step_problems(gate: str, workflow: str = "") -> list[str]:
    """The workflow top level, the gates job's keys, its strategy and its
    gate step must each take their one accepted form."""
    problems = []
    # The root keys must sit at column zero: an indented root still parses,
    # and a column-zero check would then examine nothing (TSK-203 review).
    top = [line for line in workflow.splitlines() if re.match(r"^[^\s#]", line)]
    names = [m.group(1) if (m := re.match(r"^([a-z-]+):(?:\s|$)", line)) else line for line in top]
    if sorted(names) != sorted(TOP_KEYS):
        problems.append(f"the workflow's top level must be exactly {list(TOP_KEYS)}, each once at column "
                        f"zero; a top-level env or defaults reaches every bash step; found {names}")
    lines = gate.splitlines()
    own = [line.rstrip() for line in lines if re.match(r"^ {4}[^\s#]", line)]
    keys = [re.match(r"^ {4}([a-z-]+):(?:\s|$)", line) for line in own]
    if any(key is None for key in keys) or [key.group(1) for key in keys] != list(GATES_KEYS):
        problems.append(f"the gates job's own keys must be exactly {list(GATES_KEYS)} in that order, "
                        f"so it cannot be forgiven, defaulted or given env; found {own}")
    if "    strategy:" in lines and "    steps:" in lines:
        block = [line.rstrip() for line in lines[lines.index("    strategy:"):lines.index("    steps:")]
                 if line.strip() and not line.lstrip().startswith("#")]
        entries = block[len(STRATEGY_HEAD):]
        pairs_ok = (len(entries) % 2 == 0 and entries
                    and all(PART_LINE.match(a) and ONLY_LINE.match(b) for a, b in zip(entries[::2], entries[1::2])))
        if block[:len(STRATEGY_HEAD)] != STRATEGY_HEAD or not pairs_ok:
            problems.append("the gates strategy must be `fail-fast: false` and a matrix `include` of "
                            "`- part: <name>` / `only: <target>,...` pairs, one line each; refused: "
                            f"{[line for line in block if line not in STRATEGY_HEAD and not PART_LINE.match(line) and not ONLY_LINE.match(line)]}")
    else:
        problems.append("the gates job must have a `strategy:` block before `steps:`")
    steps = [s for s in re.split(r"\n(?= {6}- )", gate) if "codeflow test" in "\n".join(norm_job(s))]
    if len(steps) != 1 or norm_job(steps[0]) != norm_job(GATE_STEP):
        problems.append("the gate step must be exactly this, and the only step that names `codeflow "
                        "test`; any other form could skip, forgive or replace the command:\n" + GATE_STEP)
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


# TSK-254: a job with no timeout holds a runner for GitHub's default six
# hours when a step stalls, and its verdict waits as long. Every job in every
# workflow this repository runs or ships names its own `timeout-minutes`, a
# plain whole number, so a stall frees the slot and fails the verdict. A job
# that only calls a reusable workflow (`uses:`) cannot carry the key (GitHub
# rejects it), so the called workflow's own jobs must carry it and are checked
# here. Only a local workflow under `.github/workflows/` can be opened and
# checked; a remote or out-of-tree callee cannot, so its caller is refused.
TIMEOUT_KEY = re.compile(r"^ {4}timeout-minutes:")
TIMEOUT_LINE = re.compile(r"^ {4}timeout-minutes: [1-9][0-9]*$")
CALLER_LINE = re.compile(r"^ {4}uses:", re.M)
CALLER_TARGET = re.compile(r"^ {4}uses:[ \t]*(?:\"([^\"]*)\"|'([^']*)'|([^\s#]*))[ \t]*(?:#.*)?$", re.M)
WORKFLOW_DIRS = (Path(".github/workflows"), Path("assets/base/ci"))
# A shipped template cannot size a job that runs the adopter's own commands.
# Each exemption names its reason; a stale one (the job is gone) is drift.
UNBOUNDED_BY_DESIGN = {
    "assets/base/ci/codeflow-ci.yml": {
        "gates": "runs the adopter's own suite, whose length only the adopter knows",
        "candidate": "tracked in TSK-254 follow-up notes; bound it when this job is next edited",
    },
}


def jobs_section(workflow: str) -> dict[str, str]:
    """Each job under the top-level `jobs:` key, keyed by job id."""
    parts = re.split(r"^jobs:[ \t]*$", workflow, maxsplit=1, flags=re.M)
    return workflow_jobs("jobs:" + parts[1]) if len(parts) == 2 else {}


def caller_problems(name: str, text: str, where: str, seen: frozenset[str]) -> list[str]:
    """A reusable-workflow caller is exempt only when its callee is a local
    workflow under `.github/workflows/` that this check can open and that has
    no timeout problem of its own."""
    targets = [next(g for g in m.groups() if g is not None) for m in CALLER_TARGET.finditer(text)]
    if len(targets) != 1:
        return [f"job '{name}'{where} must have exactly one `uses:` value; found {targets}"]
    target = targets[0]
    relative = Path(target[2:] if target.startswith("./") else target)
    local = (target.startswith("./") and "@" not in target and ".." not in relative.parts
             and relative.parent == Path(".github/workflows") and relative in workflow_files())
    if not local:
        return [f"job '{name}'{where} calls `{target}`, which this check cannot open and bound (a caller job cannot "
                "carry `timeout-minutes`); call a workflow under .github/workflows/ as `./.github/workflows/<file>`"]
    if relative.as_posix() in seen:
        return [f"job '{name}'{where} calls `{target}`, which calls itself again"]
    callee = timeout_problems((ROOT / relative).read_text(), relative.as_posix(),
                              seen=seen | {relative.as_posix()})
    return [f"job '{name}'{where} calls `{target}`, whose own jobs are not all bounded: {callee}"] if callee else []


def timeout_problems(workflow: str, label: str = "", exempt: dict[str, str] | None = None,
                     seen: frozenset[str] = frozenset()) -> list[str]:
    jobs = jobs_section(workflow)
    where = f" in {label}" if label else ""
    exempt = exempt or {}
    problems = [] if jobs else [f"the workflow{where} has no `jobs:` section to check for timeouts"]
    for name in exempt:
        if name not in jobs:
            problems.append(f"{label or 'the workflow'} exempts job '{name}' from a timeout but has no such job; "
                            "drop the stale exemption")
    for name, text in jobs.items():
        if name in exempt:
            continue
        if CALLER_LINE.search(text):
            problems += caller_problems(name, text, where, seen)
            continue
        own = [line.rstrip() for line in text.splitlines() if TIMEOUT_KEY.match(line)]
        if len(own) != 1 or not TIMEOUT_LINE.match(own[0]):
            problems.append(f"job '{name}'{where} must carry exactly one `timeout-minutes: <whole minutes>` "
                            f"of its own, so a stalled step cannot hold its runner for six hours; found {own}")
    return problems


def workflow_files() -> list[Path]:
    """Every GitHub workflow this repository runs or ships, relative to ROOT."""
    found = []
    for directory in WORKFLOW_DIRS:
        for path in sorted((ROOT / directory).glob("*.y*ml")):
            text = path.read_text()
            # assets/base/ci also holds other CI systems' files; a workflow
            # names its triggers and jobs at the top level.
            if directory.parts[0] == ".github" or re.search(r"^(on|\"on\"|'on'|jobs):", text, re.M):
                found.append(path.relative_to(ROOT))
    return found


def all_timeout_problems() -> list[str]:
    files = workflow_files()
    problems = [] if files else ["no workflow files were found to check for timeouts"]
    for relative in files:
        problems += timeout_problems((ROOT / relative).read_text(), relative.as_posix(),
                                     UNBOUNDED_BY_DESIGN.get(relative.as_posix()))
    return problems


# dist 0.32 writes release.yml and cannot set a job timeout, so the timeouts
# are added by hand. dist's integrity check fails `dist plan` on a hand edit
# unless the config allows it, and with the key removed a `dist generate`
# rewrites the file without the timeouts (which `all_timeout_problems` then
# refuses).
def dist_problems(config_path: Path | None = None, release: Path | None = None) -> list[str]:
    config_path = config_path or DIST_CONFIG
    release = release or ROOT / ".github" / "workflows" / "release.yml"
    if not release.exists() or not config_path.exists():
        return []
    allowed = tomllib.loads(config_path.read_text()).get("dist", {}).get("allow-dirty", [])
    if "ci" in allowed:
        return []
    return ["dist-workspace.toml must set `allow-dirty = [\"ci\"]` while release.yml carries hand-added job "
            "timeouts; otherwise `dist plan` rejects the file and `dist generate` rewrites it without them"]


# TSK-254: only the present part launches a browser, so only it touches
# Playwright, and its install is bounded and retried: the apt download of
# the browsers' system libraries stalled for up to three hours with no
# timeout. `--with-deps` is refused because it hides that download inside
# the browser install, where the root apt-get cannot be stopped.
PRESENT_ONLY = "        if: matrix.part == 'present'"
STEP_TIMEOUT = re.compile(r"^ {8}timeout-minutes: [1-9][0-9]*$", re.M)
BOUNDED_DEPS = re.compile(r"sudo timeout --kill-after=\S+ \S+ .*install-deps")


def playwright_problems(workflow: str) -> list[str]:
    problems = []
    for name, text in jobs_section(workflow).items():
        for step in re.split(r"\n(?= {6}- )", text):
            body = "\n".join(norm_job(step))
            if "playwright" not in body.lower():
                continue
            first = body.splitlines()[0].strip() if body else ""
            if name != "gates":
                problems.append(f"job '{name}' has a Playwright step ({first}); only the gates job's "
                                "present part launches a browser")
                continue
            if PRESENT_ONLY not in body.splitlines():
                problems.append(f"the gates step `{first}` touches Playwright on every part; give it "
                                f"`{PRESENT_ONLY.strip()}`")
            if "--with-deps" in body:
                problems.append(f"the gates step `{first}` installs with --with-deps; install the "
                                "browsers and run `install-deps` as root under its own timeout")
            if "install-deps" in body and not (STEP_TIMEOUT.search(step) and BOUNDED_DEPS.search(body)
                                               and "for attempt in 1 2; do" in body):
                problems.append(f"the gates step `{first}` must carry its own `timeout-minutes`, run "
                                "`install-deps` under `sudo timeout --kill-after=<s> <limit>` and try it "
                                "twice (`for attempt in 1 2; do`)")
    return problems


def main() -> int:
    cfg = json.loads(CONFIG.read_text())
    workflow = WORKFLOW.read_text()
    pins = (node_pin_problems(cfg, workflow) + gate_binary_problems(cfg)
            + gate_part_problems(cfg, workflow) + shared_install_problems(cfg)
            + all_timeout_problems() + dist_problems() + playwright_problems(workflow))
    for problem in pins:
        print(f"GATE PARITY DRIFT: {problem}", file=sys.stderr)
    status = rust_parity()
    if pins:
        return 1
    if status == 0:
        print("gate-parity OK: Node targets run on their CI pins, the "
              "real-browser check runs the gate's binary, the gate parts "
              "run every full-mode target, every job of every workflow has a timeout, and only "
              "the present part installs Playwright, bounded and retried")
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
