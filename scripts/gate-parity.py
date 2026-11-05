#!/usr/bin/env python3
"""Compare the local instrumented Rust suite with the Windows raw referee.

Windows independently runs nextest (in partitions), doctests, clippy and
rustdoc; formatting is platform independent, so only Linux runs fmt
(TSK-449). Unix-only journeys and the ignored read benchmark remain Linux-only.
Node versions, coordinator-owned binary paths and the parts of the split
Linux full gate, its coverage shards and their report are also checked.
"""

import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CONFIG = ROOT / ".codeflow" / "test-config.json"
WORKFLOW = ROOT / ".github" / "workflows" / "codeflow-ci.yml"
DIST_CONFIG = ROOT / "dist-workspace.toml"


def repository_text(path: Path) -> str:
    """Repository files are strict UTF-8, independent of the runner locale."""
    try:
        return path.read_text(encoding="utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{path}: repository text must be UTF-8: {error}") from error


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


# TSK-203: the Windows suite runs in nextest partitions. TSK-265: one job
# builds the suite into a nextest archive and each partition runs that
# archive. TSK-1321: each runs `--partition hash:<n>/<count>`, which gives
# every test exactly one partition by the hash of its name, under the
# windows-hosted profile, which adds the per-test bound to the codeflow
# profile's settings. The archived run stands for the whole suite only when the
# archive is built from the whole workspace with the codeflow profile, and the
# matrix runs every partition from 1 to the run's count exactly once. The
# Windows journeys job checks each run's results against the archive's test
# list (scripts/partition-check.py).
# TSK-1242: the reduced tier's journey run is a named subset, never the suite,
# so it is pinned by TIER_JOURNEYS_JOB and owes no local parity.
ARCHIVE = re.compile(r"^cargo nextest archive --workspace --profile codeflow --archive-file (\S+)$")
ARCHIVED_RUN = re.compile(
    r"^cargo nextest run --archive-file (\S+) --workspace-remap \. --extract-to \. --extract-overwrite "
    r"--no-fail-fast --profile windows-hosted --partition hash:\$\{\{ matrix\.partition \}\}/([1-9][0-9]*)$")
TIER_RUN = ('cargo nextest run --workspace --no-fail-fast --profile codeflow '
            '-E "$(python -B scripts/journey-gate.py --nextest-filter --skip-benchmark)"')


def matrix_partitions(job: str) -> list[int]:
    """The `partition: [..]` matrix values of a job, in order."""
    found = re.search(r"^\s*partition:\s*\[([^\]]*)\]\s*$", job, re.M)
    if not found:
        return []
    values = [v.strip() for v in found.group(1).split(",") if v.strip()]
    return [int(v) for v in values] if all(v.isdigit() for v in values) else []


def ci_rust_job_commands(workflow: str) -> set[str]:
    """Rust verification `run:` commands inside the Windows raw referee jobs
    (every job whose id starts with `windows`). An archive build runs no
    test; the archived partitions stand for the suite they cover."""
    out = set()
    jobs = {name: text for name, text in workflow_jobs(workflow).items() if name.startswith("windows")}
    archives = {match.group(1) for text in jobs.values()
                for run in re.finditer(r"^\s*run:\s*(.+?)\s*$", text, re.M)
                if (match := ARCHIVE.match(norm(run.group(1))))}
    for name, text in jobs.items():
        partitions = matrix_partitions(text)
        for run in re.finditer(r"^\s*run:\s*(.+?)\s*$", text, re.M):
            cmd = norm(run.group(1))
            if not is_rust_verification_command(cmd) or ARCHIVE.match(cmd):
                continue
            if name == "windows-tier-journeys" and cmd == TIER_RUN:
                continue
            archived = ARCHIVED_RUN.match(cmd)
            if archived:
                count = int(archived.group(2))
                if archived.group(1) not in archives:
                    cmd = f"{cmd} (no windows job builds the archive {archived.group(1)})"
                elif partitions != list(range(1, count + 1)):
                    cmd = f"{cmd} (partitions {partitions} do not cover 1 to {count})"
                else:
                    cmd = SUITE
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
    release = jobs.get("gates-release", "")
    parts = re.findall(r"^\s*only:\s*(\S+)\s*$", gate, re.M)
    release_parts = re.findall(r"^\s*only:\s*(\S+)\s*$", release, re.M)
    landing = [n for part in parts for n in part.split(",") if n]
    released = [n for part in release_parts for n in part.split(",") if n]
    named = landing + released
    problems.extend(release_job_problems(release, gate, workflow, released))
    owed = {
        t["name"] for t in cfg.get("targets", [])
        if applicable(t) and t.get("modes", {}).get("full")
    }
    unknown = sorted(set(named) - owed)
    if unknown:
        problems.append(f"gate parts name targets with no enabled full mode: {unknown}")
    by_coverage = requires_closure(cfg, set(COVERAGE_TARGETS))
    missing = sorted(owed - requires_closure(cfg, set(named)) - by_coverage)
    if missing:
        problems.append(f"no gate part, release part or coverage job runs these full-mode targets: {missing}; "
                        "the hosted landing gate and the schedule and release path both leave them out")
    twice = sorted({n for n in named if named.count(n) > 1})
    if twice:
        problems.append(f"more than one gate part names: {twice}")
    shared = sorted(set(named) & set(COVERAGE_TARGETS))
    if shared:
        problems.append(f"gate parts name {shared}, which the coverage jobs run; a second run would "
                        "repeat the instrumented suite")
    problems.extend(gate_step_problems(gate, workflow))
    verdicts = [
        (name, text) for name, text in jobs.items()
        if re.search(rf"^ {{4}}name:\s*{re.escape(VERDICT)}\s*$", text, re.M)
    ]
    if len(verdicts) != 1:
        problems.append(f"exactly one job must be named `{VERDICT}`; found {[n for n, _ in verdicts]}")
    else:
        name, text = verdicts[0]
        line = re.search(r"^ {4}if:\s*(.+?)\s*$", gate, re.M)
        guarded = GATES_IF.match(line.group(1)) if line else None
        if guarded is None:
            problems.append("the gates job's condition must be exactly `if: ${{ !cancelled() && "
                            "needs.reuse.outputs.run == '' && (<event condition>) }}`, so it runs on "
                            "every protected-base pull request and push the reuse decision does not "
                            f"cover; found {line.group(1) if line else 'none'}")
        expected = verdict_job()
        if norm_job(text) != norm_job(expected):
            problems.append(
                f"`{VERDICT}` ({name}) must be exactly the verdict job: run "
                "`if: always()` except on a draft, need the reuse, gates and "
                "coverage jobs, and fail unless each of them succeeded or the reuse decision named a "
                "run and all of them were skipped; any other form could let a skipped, failed or "
                "cancelled part read as passed. Expected:\n" + expected)
    if "    needs: [reuse, gates-build]" not in [l.rstrip() for l in gate.splitlines()]:
        problems.append("the gates job must need exactly the reuse job, whose output its condition reads, "
                        "and the build job (`needs: [reuse, gates-build]`)")
    problems.extend(build_job_problems(jobs, gate))
    problems.extend(reuse_job_problems(jobs))
    line = re.search(r"^ {4}if:\s*(.+?)\s*$", gate, re.M)
    guarded = GATES_IF.match(line.group(1)) if line else None
    problems.extend(coverage_job_problems(cfg, jobs, guarded.group(1) if guarded else ""))
    return problems


# TSK-270: the instrumented suite runs as shards beside the gate parts, and a
# report job adds up their coverage, applies the floor and reads the
# journeys. Both jobs are pinned exactly, like the verdict, and are built
# from the config: the shards run the rust-coverage command with
# --no-report and their hash partition after its present-web-build
# prerequisite (TSK-1321), the report reads the floor from the config and
# runs the journey-gate command on the shards' results, and the matrix, the
# merge and the partition check cover every one of the COVERAGE_SHARDS
# shards, once. The merge's own tests (scripts/test_coverage_merge.py) and
# the partition check's (scripts/test_partition_check.py) cover the rest.
COVERAGE_TARGETS = ("rust-coverage", "journey-gate")
COVERAGE_SHARDS = 2
SHARD_FILTER = "--partition hash:${{ matrix.shard }}/@COUNT@"
COVERAGE_CANONICAL = "cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 90 --profile codeflow"
COVERAGE_JOB = """\
    if: ${{ !cancelled() && needs.reuse.outputs.run == '' && (@CONDITION@) }}
    name: codeflow gates (coverage ${{ matrix.shard }} of @COUNT@)
    needs: reuse
    runs-on: ubuntu-24.04
    timeout-minutes: 30
    strategy:
      fail-fast: false
      matrix:
        shard: [@SHARDS@]
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0
      - name: Install the differential shells
        run: sudo apt-get update && sudo apt-get install -y bash zsh dash
      - name: Install Rust (rust-toolchain.toml)
        run: rustup toolchain install
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2
        with:
          shared-key: coverage-gates
          save-if: ${{ matrix.shard == 1 }}
      - uses: actions/setup-node@249970729cb0ef3589644e2896645e5dc5ba9c38 # v6.5.0
        with:
          node-version: 26.4.0
          cache: npm
          cache-dependency-path: crates/codeflow-present/web/package-lock.json
      - name: Install exact npm for the owned presentation renderer
        run: npm install --global npm@11.17.0
      - name: Install nextest and llvm-cov
        uses: taiki-e/install-action@4cef1412cce204788f482e778a0b9187f9626a29 # v2
        with:
          tool: cargo-nextest@0.9.129,cargo-llvm-cov@0.8.4
      - name: present-web-build
        shell: bash
        run: @WEB_BUILD@
      - name: Remove old coverage profiles
        run: cargo llvm-cov clean --workspace
      - name: cargo llvm-cov nextest (one shard)
        shell: bash
        env:
          LLVM_PROFILE_FILE_NAME: codeflow-%4m.profraw
        run: @SHARD_RUN@
      - name: Export this shard's coverage and list its tests
        shell: bash
        run: |
          mkdir -p target/coverage-data
          cargo llvm-cov report --json --output-path target/coverage-data/coverage.json
          eval "$(cargo llvm-cov show-env --sh)"
          CARGO_TARGET_DIR=target/llvm-cov-target cargo nextest list --workspace --profile codeflow --message-format json > target/nextest/codeflow/tests.json
      - name: Keep this shard's coverage for the report
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: coverage-data-${{ matrix.shard }}
          path: target/coverage-data/coverage.json
          if-no-files-found: error
          retention-days: 1
      - name: Keep this shard's results for the report
        if: always()
        uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a # v7.0.1
        with:
          name: coverage-shard-${{ matrix.shard }}
          path: |
            target/nextest/codeflow/junit.xml
            target/nextest/codeflow/tests.json
          if-no-files-found: ignore
          retention-days: 7
"""
COVERAGE_REPORT_JOB = """\
    if: always() && needs.gates-coverage.result != 'skipped' && (@CONDITION@)
    name: codeflow gates (coverage report)
    needs: gates-coverage
    runs-on: ubuntu-24.04
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0
      - name: Begin journey check
        shell: bash
        run: |
          rm -rf target/coverage
          echo "CODEFLOW_GATE_STARTED_AT=$(python3 -c 'import time; print(time.time())')" >> "$GITHUB_ENV"
      - name: Fetch every shard's coverage
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c # v8.0.1
        with:
          pattern: coverage-data-*
          path: target/coverage/data
      - name: Fetch every shard's results
        uses: actions/download-artifact@3e5f45b2cfb9172054b4087a40e8e0b5a5461e7c # v8.0.1
        with:
          pattern: coverage-shard-*
          path: target/coverage/results
      - name: Line coverage of the whole suite
        shell: bash
        run: python3 -B scripts/test_coverage_merge.py && python3 -B scripts/coverage-merge.py --floor-from .codeflow/test-config.json --shards @COUNT@ target/coverage/data
      - name: journey gate
        if: ${{ !cancelled() }}
        shell: bash
        env:
          CODEFLOW_GATE_RESULTS: target/coverage/results
        run: @JOURNEY@
      - name: Every test ran in exactly one shard
        if: ${{ !cancelled() }}
        shell: bash
        run: python3 -B scripts/test_partition_check.py && python3 -B scripts/partition-check.py check --count @COUNT@ --prefix coverage-shard --margin-minutes 24 --setup-minutes 6 --list @LISTS@ --results target/coverage/results
"""


def coverage_jobs(cfg: dict, condition: str, shards: int) -> tuple[str, str, list[str]]:
    """(the shard job, the report job, problems) this config and plan pin."""
    problems = []
    targets = {t.get("name"): t for t in cfg.get("targets", [])}
    full = {name: norm(t.get("modes", {}).get("full", {}).get("command", "")) for name, t in targets.items()}
    coverage = full.get("rust-coverage", "")
    if not (COVERAGE.match(coverage) and plain(targets.get("rust-coverage", {}))
            and applicable(targets.get("rust-coverage", {}))):
        problems.append(f"the rust-coverage target must run `{COVERAGE_FORM}` with no cwd or env; the coverage "
                        f"shards run it with --no-report; found {coverage!r}")
        coverage = COVERAGE_CANONICAL
    if targets.get("rust-coverage", {}).get("requires") != ["present-web-build"]:
        problems.append("the rust-coverage target must require exactly present-web-build, the one prerequisite "
                        "the coverage shards run")
    if targets.get("journey-gate", {}).get("requires") != ["rust-coverage"] or not full.get("journey-gate"):
        problems.append("the journey-gate target must require exactly rust-coverage, whose results the coverage "
                        "report reads")
    shard_run = (re.sub(r" --fail-under-lines \S+", "", coverage).replace(" --profile ", " --no-report --profile ")
                 + " " + SHARD_FILTER)
    numbers = range(1, shards + 1)
    lists = " ".join(f"target/coverage/results/coverage-shard-{n}/tests.json" for n in numbers)
    fill = {"@CONDITION@": condition, "@SHARDS@": ", ".join(map(str, numbers)),
            "@WEB_BUILD@": full.get("present-web-build", ""), "@SHARD_RUN@": shard_run, "@COUNT@": str(shards),
            "@JOURNEY@": full.get("journey-gate", ""), "@LISTS@": lists}
    jobs = []
    for template in (COVERAGE_JOB, COVERAGE_REPORT_JOB):
        for marker, value in fill.items():
            template = template.replace(marker, value)
        jobs.append(template)
    return jobs[0], jobs[1], problems


def coverage_job_problems(cfg: dict, jobs: dict[str, str], condition: str,
                          count: int = COVERAGE_SHARDS) -> list[str]:
    """The shard and report jobs are exactly the pinned ones for this config
    and shard count, and nothing else in the workflow claims their names."""
    shards, report, problems = coverage_jobs(cfg, condition, count)
    for job, expected in (("gates-coverage", shards), ("gates-coverage-report", report)):
        if norm_job(jobs.get(job, "")) != norm_job(expected):
            problems.append(f"the `{job}` job must be exactly this, built from .codeflow/test-config.json and "
                            f"{count} hash shards: any other form could run part of the suite, skip a shard or "
                            "judge the floor on another number. Expected:\n" + expected)
    return problems


# TSK-265: a push reuses a pull request run's verdict only through this job,
# pinned exactly like the verdict: it runs on a push only, holds read-only
# permissions and decides through scripts/ci-reuse.py, whose unit tests
# (scripts/test_ci_reuse.py) cover each condition.
GATES_IF = re.compile(r"^\$\{\{ !cancelled\(\) && needs\.reuse\.outputs\.run == '' && \((.+)\) \}\}$")
REUSE_JOB = """\
    if: github.event_name == 'push'
    name: reuse a pull request verdict
    needs: [ubuntu-image, windows-image]
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    permissions:
      contents: read
      actions: read
      pull-requests: read
    outputs:
      run: ${{ steps.decide.outputs.run }}
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0
        with:
          fetch-depth: 0
          filter: blob:none
      - name: Decide from the tree ids and the pull request run
        id: decide
        env:
          GH_TOKEN: ${{ github.token }}
          REPOSITORY: ${{ github.repository }}
          PUSHED: ${{ github.sha }}
          PUSHED_REF: ${{ github.ref }}
          EVENT: ${{ github.event_name }}
          WORKFLOW_REF: ${{ github.workflow_ref }}
          WINDOWS_IMAGE_OS: ${{ needs.windows-image.outputs.os }}
          WINDOWS_IMAGE_VERSION: ${{ needs.windows-image.outputs.version }}
          DECIDER_SHA256: @DECIDER_SHA256@
        run: |
          actual=$(sha256sum scripts/ci-reuse.py | cut -d ' ' -f 1)
          if [ "$actual" != "$DECIDER_SHA256" ]; then
            echo "ci-reuse: every job runs: scripts/ci-reuse.py ($actual) is not the script this workflow pins ($DECIDER_SHA256); changing it needs the same reviewed edit to this workflow"
            exit 0
          fi
          python3 -I scripts/ci-reuse.py --repo "$REPOSITORY" --sha "$PUSHED" --event "$EVENT" \\
            --workflow-ref "$WORKFLOW_REF" --ref "$PUSHED_REF" \\
            --image-os "${ImageOS:-}" --image-version "${ImageVersion:-}" \\
            --windows-image-os "$WINDOWS_IMAGE_OS" --windows-image-version "$WINDOWS_IMAGE_VERSION" \\
            --push-only "reuse a pull request verdict" --push-only "read the Windows runner image" \\
            --push-only "read the Ubuntu runner image" \\
            --require "codeflow gates" --require windows
"""


# TSK-265: the `windows` verdict, pinned exactly like `codeflow gates`: a
# substring check would let an inserted second success path through.
# TSK-1321: it is computed from the result of every job it needs; each tier
# declares the jobs it skips (the full tier only the reduced tier's journeys,
# the reduced tier on a pull request into an integration line the build, the
# partitions and their journeys, a reuse that names a run every job), every
# other job must succeed, and a failure names each job that is not green.
# scripts/test_windows_tier.py runs this step. Verdicts run on every event
# but a draft pull request, which ends them neutral (TSK-449), including
# when gate selection skips the work.
WINDOWS_VERDICT = "windows"
WINDOWS_VERDICT_JOB = """\
    if: ${{ always() && !(github.event_name == 'pull_request' && github.event.pull_request.draft) }}
    name: windows
    needs: [reuse, windows-tier, windows, windows-build, windows-tests, windows-journeys, windows-release-journey, windows-tier-journeys]
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    steps:
      - name: Every Windows job the tier runs passed
        shell: python3 -I {0}
        env:
          NEEDS: ${{ toJSON(needs) }}
          EVENT: ${{ github.event_name }}
          BASE: ${{ github.event.pull_request.base.ref }}
        run: |
          import json, os, re, sys
          needs = json.loads(os.environ["NEEDS"])
          event, base, sha = os.environ["EVENT"], os.environ.get("BASE", ""), os.environ["GITHUB_SHA"]
          names = {"windows": "windows checks", "windows-build": "windows test build",
                   "windows-tests": "windows tests", "windows-journeys": "windows journeys",
                   "windows-release-journey": "windows release journey",
                   "windows-tier-journeys": "windows tier journeys", "windows-tier": "windows tier"}
          def result(job): return needs.get(job, {}).get("result", "missing")
          def output(job, key): return (needs.get(job, {}).get("outputs") or {}).get(key, "")
          tier, reused = output("windows-tier", "tier"), output("reuse", "run")
          jobs = sorted(job for job in needs if job != "reuse")
          gates = [job for job in jobs if job != "windows-tier"]
          errors = []
          if result("reuse") == "success" and re.fullmatch(r"[0-9]+", reused):
              mode, skips = "reuse", set(jobs)
          elif tier == "reduced":
              mode, skips = "reduced", {"windows-build", "windows-tests", "windows-journeys"}
              if event != "pull_request" or not base.startswith("integration/"):
                  errors.append(f"the reduced tier is only for a pull request into an integration line; "
                                f"event {event}, base {base or 'none'}")
          else:
              mode, skips = "full", {"windows-tier-journeys"}
          if event != "pull_request":
              skips.add("windows-tier")
          if mode != "reuse" and all(result(job) == "skipped" for job in gates):
              errors.append("the Windows gates did not run; bases outside main, master or integration/** "
                            "cannot supply a passing verdict")
          else:
              for job in jobs:
                  want = "skipped" if job in skips else "success"
                  if result(job) != want:
                      declared = "the tier declares it skipped" if want == "skipped" else f"the {mode} tier runs it"
                      errors.append(f"{names.get(job, job)} concluded {result(job)}; {declared}")
          missing = output("windows-journeys", "missing")
          if missing:
              errors.append(f"Windows test partition(s) {missing} ended without a report (a timeout or a "
                            "cancellation); their tests did not finish, none is known to have failed")
          if errors:
              for line in errors:
                  print(f"::error::{line}")
              sys.exit(1)
          url = f"{os.environ.get('GITHUB_SERVER_URL', '')}/{os.environ.get('GITHUB_REPOSITORY', '')}/actions/runs/{reused}"
          line = {"reuse": f"Windows passed on pull request run {url}, which tested this exact tree",
                  "reduced": f"Windows reduced tier passed at {sha}: the test partitions were skipped by tier. "
                             "This is not full Windows evidence; the push to the line runs them.",
                  "full": f"every Windows job passed at {sha}"}[mode]
          print(line)
          with open(os.environ.get("GITHUB_STEP_SUMMARY", os.devnull), "a", encoding="utf-8") as summary:
              print(line, file=summary)
"""


# TSK-1242: the switch is the one value of this job that may differ, off or on.
# A pull request into an integration line takes the reduced tier only when it
# is on and every changed file is on the REDUCED_TIER_PATHS list, none is
# COMPILED_IN, and each is a regular file added or changed in place (TSK-1321).
# Everything else takes the full tier: any other value, base or event, an
# empty, quoted or unreadable change list, a link, submodule, mode change or
# deletion, and a grep error.
TIER_SWITCH = re.compile(r'^( {10}REDUCED_TIER: )"(?:on|off)"$', re.M)
WINDOWS_TIER_JOB = """\
    if: github.event_name == 'pull_request'
    name: windows tier
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    outputs:
      tier: ${{ steps.tier.outputs.tier }}
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0
        with:
          fetch-depth: 2
          persist-credentials: false
      - name: Choose the Windows tier
        id: tier
        shell: bash
        # Only the REDUCED_TIER_PATHS take the reduced tier, and only as
        # regular files added or changed in place (REGULAR); everything else
        # takes the full tier. The partitions read this Markdown only as text
        # with line endings, separators or whitespace normalised:
        # cli/tests/doc_fact_pins.rs:100-123 (README.md, docs/),
        # cli/tests/release_contract.rs:455-473 (every tracked .md, records
        # and changelog.d/ included), cli/src/command_reference.rs:309-315
        # (docs/cli.md), cli/tests/orchestration_contract.rs:943-944 (README.md,
        # docs/) and core/tests/delegate_doctrine_contract.rs:33-41
        # (project-management/specs/SPC-002.md); checkout keeps regular files
        # LF (.gitattributes eol=lf). Rust compiles in the COMPILED_IN files,
        # so they take the full tier: core/tests/coord_store.rs:1140,
        # core/tests/path_policy.rs:508-510 and
        # core/src/security/launchers.rs:95-98 (include_str!(concat!(env!(...)))).
        env:
          REDUCED_TIER: "off"
          BASE: ${{ github.event.pull_request.base.ref }}
          BASE_SHA: ${{ github.event.pull_request.base.sha }}
          HEAD_SHA: ${{ github.event.pull_request.head.sha }}
          REDUCED_TIER_PATHS: '^(project-management/.+\\.md|docs/.+\\.md|changelog\\.d/[^/]+\\.md|README\\.md)$'
          COMPILED_IN: '^(docs/coordination-ledger\\.md|docs/policy-reference\\.md|changelog\\.d/TSK-(322|1079)\\.md)$'
          REGULAR: '^:(000000 100644|000000 100755|100644 100644|100755 100755) [0-9a-f]+ [0-9a-f]+ [AM][[:space:]]'
        run: |
          tier=full
          if [ "$REDUCED_TIER" != on ]; then
            reason="the reduced tier switch is $REDUCED_TIER"
          else
            case "$BASE" in
              integration/*)
                if ! parents=$(git rev-list --parents -n 1 HEAD); then
                  reason="the tested commit's parents cannot be read"
                elif [ "$parents" != "$(git rev-parse HEAD) $BASE_SHA $HEAD_SHA" ]; then
                  reason="the tested commit's parents ($parents) are not the pull request's base $BASE_SHA and head $HEAD_SHA"
                elif ! raw=$(git -c core.quotePath=true diff --no-renames --raw --no-abbrev "$BASE_SHA" HEAD); then
                  reason="the changed files cannot be listed"
                elif [ -z "$raw" ]; then
                  reason="the diff lists no changed file"
                elif ! files=$(printf '%s\\n' "$raw" | cut -f 2-) || [ -z "$files" ]; then
                  reason="the changed paths cannot be read from the diff"
                elif printf '%s\\n' "$files" | grep '^"' > /dev/null; then
                  reason="a changed path holds characters git quotes, so it cannot be matched by name"
                else
                  if odd=$(printf '%s\\n' "$raw" | grep -vE "$REGULAR"); then kind=0; else kind=$?; fi
                  if outside=$(printf '%s\\n' "$files" | grep -vE "$REDUCED_TIER_PATHS"); then off=0; else off=$?; fi
                  if compiled=$(printf '%s\\n' "$files" | grep -E "$COMPILED_IN"); then built=0; else built=$?; fi
                  count=$(printf '%s\\n' "$files" | wc -l | tr -d ' ')
                  if [ "$kind" -gt 1 ] || [ "$off" -gt 1 ] || [ "$built" -gt 1 ]; then
                    reason="the path patterns could not be matched (grep exit $kind, $off and $built)"
                  elif [ "$kind" -eq 0 ]; then
                    odd=${odd%%$'\\n'*}
                    reason="${odd#*$'\\t'} is not a regular file added or changed in place (a link, submodule, mode change or deletion: ${odd%%$'\\t'*})"
                  elif [ "$off" -eq 0 ]; then
                    reason="it changes ${outside%%$'\\n'*} and $(($(printf '%s\\n' "$outside" | wc -l) - 1)) other path(s) outside the reduced-tier list"
                  elif [ "$built" -eq 0 ]; then
                    reason="it changes ${compiled%%$'\\n'*}, which Rust compiles in"
                  else
                    tier=reduced
                    reason="all of its $count changed files are on the reduced-tier list"
                  fi
                fi
                ;;
              *) reason="the base $BASE is not an integration line" ;;
            esac
          fi
          echo "tier=$tier" >> "$GITHUB_OUTPUT"
          echo "Windows tier: $tier, because $reason"
          echo "Windows tier: $tier, because $reason" >> "$GITHUB_STEP_SUMMARY"
"""
TIER_JOURNEYS_JOB = f"""\
    if: ${{{{ !cancelled() && needs.windows-tier.outputs.tier == 'reduced' && !github.event.pull_request.draft }}}}
    name: windows tier journeys
    needs: windows-tier
    runs-on: windows-latest
    timeout-minutes: 45
    steps:
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0
      - name: Install the differential shells
        id: shells
        uses: msys2/setup-msys2@ec48f7c5447b3140e2b088413ae3a55687bccb6e # v2
        with:
          msystem: MSYS
          install: bash zsh dash
      - name: Select the differential shell directory
        shell: pwsh
        env:
          MSYS2_LOCATION: ${{{{ steps.shells.outputs.msys2-location }}}}
        run: Add-Content -Path $env:GITHUB_ENV -Value "CODEFLOW_DIFFERENTIAL_SHELL_DIR=$env:MSYS2_LOCATION\\usr\\bin"
      - name: Install Rust (rust-toolchain.toml)
        run: rustup toolchain install
      - uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2
        with:
          shared-key: windows-tests
          cache-on-failure: true
      - name: Install nextest
        uses: taiki-e/install-action@4cef1412cce204788f482e778a0b9187f9626a29 # v2
        with:
          tool: cargo-nextest@0.9.129
      - name: Begin journey check
        shell: bash
        run: |
          rm -rf target/nextest/codeflow
          echo "CODEFLOW_GATE_STARTED_AT=$(python -c 'import time; print(time.time())')" >> "$GITHUB_ENV"
      - name: Run every supported Windows journey test
        shell: bash
        run: {TIER_RUN}
      - name: journey gate
        shell: bash
        run: python scripts/test_journey_gate.py && python scripts/journey-gate.py --results target/nextest/codeflow/junit.xml --skip-benchmark --part results
"""
BUILD_TIER_CLAUSE = "!cancelled() && needs.windows-tier.outputs.tier != 'reduced' && needs.reuse.outputs.run == ''"


def windows_tier_problems(workflow: str) -> list[str]:
    """The tier job, its journey job and the one job that declares the skip are
    exactly the pinned forms, so no other job can leave the full tier."""
    jobs = workflow_jobs(workflow)
    problems = []
    tier = TIER_SWITCH.sub(r'\1"off"', jobs.get("windows-tier", ""))
    if norm_job(tier) != norm_job(WINDOWS_TIER_JOB):
        problems.append("`windows-tier` must be exactly this, with its switch off or on:\n" + WINDOWS_TIER_JOB)
    if norm_job(jobs.get("windows-tier-journeys", "")) != norm_job(TIER_JOURNEYS_JOB):
        problems.append("`windows-tier-journeys` must be exactly this:\n" + TIER_JOURNEYS_JOB)
    build = jobs.get("windows-build", "")
    if BUILD_TIER_CLAUSE not in build or "    needs: [reuse, windows-tier]\n" not in build:
        problems.append(f"`windows-build` must need [reuse, windows-tier] and run under `{BUILD_TIER_CLAUSE}`")
    others = [name for name, text in jobs.items() if "needs.windows-tier.outputs.tier" in text
              and name not in ("windows-build", "windows-tier-journeys", "windows-verdict")]
    if others:
        problems.append(f"only windows-build may leave the full tier; these jobs also read the tier: {others}")
    return problems


def windows_verdict_problems(workflow: str) -> list[str]:
    """Exactly one job is named `windows`, and it is exactly the pinned verdict."""
    jobs = workflow_jobs(workflow)
    verdicts = [(name, text) for name, text in jobs.items()
                if re.search(rf"^ {{4}}name:\s*{re.escape(WINDOWS_VERDICT)}\s*$", text, re.M)]
    if len(verdicts) != 1:
        return [f"exactly one job must be named `{WINDOWS_VERDICT}`; found {[n for n, _ in verdicts]}"]
    expected = WINDOWS_VERDICT_JOB
    name, text = verdicts[0]
    if norm_job(text) != norm_job(expected):
        return [f"`{WINDOWS_VERDICT}` ({name}) must be exactly the Windows verdict job: it passes only when every "
                "Windows job the tier runs succeeded and every skipped job is one the tier declares skipped; any "
                "other form could let a skipped, failed or cancelled job read as passed. Expected:\n" + expected]
    return []


# TSK-449 AC-5: the release binary the gate parts run is built once, by the
# `gates-build` job under the gates job's own condition, and each part
# fetches it; no other job of the workflow builds the release binary.
BUILD_STEP = "cargo build --release -p codeflow-cli"


def build_job_problems(jobs: dict[str, str], gate: str) -> list[str]:
    problems = []
    build = jobs.get("gates-build", "")
    builds = [name for name, text in jobs.items() if "cargo build --release" in text]
    if builds != ["gates-build"]:
        problems.append(f"only the gates-build job may build the release binary; found {builds}")
    if f"run: {BUILD_STEP}" not in build or "name: codeflow-release" not in build:
        problems.append(f"the gates-build job must run `{BUILD_STEP}` and keep it as the codeflow-release artifact")
    condition = re.search(r"^ {4}if:\s*(.+?)\s*$", gate, re.M)
    if not condition or f"    if: {condition.group(1)}" not in build.splitlines():
        problems.append("the gates-build job must run under exactly the gates job's condition")
    if "name: codeflow-release" not in gate or 'echo "$RUNNER_TEMP/codeflow-bin" >> "$GITHUB_PATH"' not in gate:
        problems.append("each gate part must fetch the codeflow-release build and put it on PATH")
    return problems


def reuse_job_problems(jobs: dict[str, str]) -> list[str]:
    # The job pins the decider by hash (TSK-359), so editing scripts/ci-reuse.py
    # needs the same reviewed edit to the workflow.
    pin = hashlib.sha256((ROOT / "scripts" / "ci-reuse.py").read_bytes()).hexdigest()
    expected = REUSE_JOB.replace("@DECIDER_SHA256@", pin)
    if norm_job(jobs.get("reuse", "")) != norm_job(expected):
        return ["the `reuse` job must be exactly this, so only a push can reuse a verdict, its token stays "
                "read-only and the pinned scripts/ci-reuse.py decides:\n" + expected]
    return []


# The one accepted form of the verdict job. A substring check let a false
# condition, a forgiven exit or `continue-on-error` through (TSK-203 review),
# so the job must match this text exactly, apart from blank lines, comments
# and trailing spaces. Every event but a draft pull request must receive a
# verdict; a draft's verdicts are skipped, which is neutral (TSK-449).
VERDICT_JOB = """\
    if: ${{{{ always() && !(github.event_name == 'pull_request' && github.event.pull_request.draft) }}}}
    name: codeflow gates
    needs: [reuse, gates, gates-release, gates-coverage, gates-coverage-report]
    runs-on: ubuntu-24.04
    timeout-minutes: 5
    steps:
      - name: Every part of the full gate passed
        shell: bash
        env:
          PARTS: ${{{{ needs.gates.result }}}}
          RELEASE: ${{{{ needs.gates-release.result }}}}
          SHARDS: ${{{{ needs.gates-coverage.result }}}}
          REPORT: ${{{{ needs.gates-coverage-report.result }}}}
          REUSE: ${{{{ needs.reuse.result }}}}
          REUSED: ${{{{ needs.reuse.outputs.run }}}}
        run: |
          if [ "$REUSE" = success ] && [ "$PARTS $RELEASE $SHARDS $REPORT" = "skipped skipped skipped skipped" ] && printf '%s' "$REUSED" | grep -Eqx '[0-9]+'; then
            echo "the full gate passed on pull request run ${{GITHUB_SERVER_URL}}/${{GITHUB_REPOSITORY}}/actions/runs/${{REUSED}}, which tested this exact tree"
            exit 0
          fi
          if [ "$PARTS $SHARDS $REPORT" = "skipped skipped skipped" ]; then
            echo "::error::the Linux gates did not run; bases outside main, master or integration/** cannot supply a passing verdict"
            exit 1
          fi
          if [ "$PARTS" != success ] || [ "$SHARDS" != success ] || [ "$REPORT" != success ]; then
            echo "::error::the full gate concluded parts=${{PARTS}} coverage shards=${{SHARDS}} coverage report=${{REPORT}}; see the codeflow gates jobs"
            exit 1
          fi
          if [ "$RELEASE" != success ] && [ "$RELEASE" != skipped ]; then
            echo "::error::the release parts concluded ${{RELEASE}}; see the codeflow gates (release) jobs"
            exit 1
          fi
          echo "every part of the full gate passed"
"""


def verdict_job() -> str:
    return VERDICT_JOB.format()


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
GATES_KEYS = ("if", "name", "needs", "runs-on", "timeout-minutes", "strategy", "steps")
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


# TSK-449 AC-4: the release-tier targets run in gates-release, never in the
# hosted landing gate. That job takes the gates job's form, runs on the
# release candidate (a ready pull request into main or master), pushes to
# main or master, the schedule and manual runs, and never on a pull request
# into integration/**.
RELEASE_TARGETS = ("prose-exhaustive", "read-benchmark", "present-browser", "present-binary-delta")
RELEASE_IF = ("${{ !cancelled() && needs.reuse.outputs.run == '' && (github.event_name == 'pull_request' && "
              "!github.event.pull_request.draft && contains(fromJSON('[\"main\",\"master\"]'), "
              "github.event.pull_request.base.ref) || github.event_name == 'push' && "
              "contains(fromJSON('[\"refs/heads/main\",\"refs/heads/master\"]'), github.ref) || "
              "github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') }}")


def release_job_problems(release: str, gate: str, workflow: str, released: list[str]) -> list[str]:
    if not release:
        return ["the workflow must have the gates-release job that runs the release-tier targets"]
    problems = []
    condition = re.search(r"^ {4}if:\s*(.+?)\s*$", release, re.M)
    if not condition or condition.group(1) != RELEASE_IF:
        problems.append(f"the gates-release job's condition must be exactly `if: {RELEASE_IF}`, so it runs on "
                        "the release path and never on a pull request into integration/**")
    if "    needs: [reuse, gates-build]" not in [line.rstrip() for line in release.splitlines()]:
        problems.append("the gates-release job must need exactly `[reuse, gates-build]`")
    if sorted(released) != sorted(RELEASE_TARGETS):
        problems.append(f"the gates-release parts must name exactly {sorted(RELEASE_TARGETS)}; found {sorted(released)}")
    landing = re.findall(r"^\s*only:\s*(\S+)\s*$", gate, re.M)
    early = sorted(set(RELEASE_TARGETS) & {n for part in landing for n in part.split(",")})
    if early:
        problems.append(f"the landing gate parts name release-tier targets {early}; they run in gates-release")
    problems.extend(problem.replace("the gates job", "the gates-release job")
                    for problem in gate_step_problems(release, workflow))
    return problems


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


# A job header is a mapping key at the job indent (two spaces): a plain or
# quoted job id, then a colon, then at most a comment. TSK-254 review: a
# header with a trailing comment (`  nightly: # weekly`) or a quoted id is
# still a job GitHub runs, and a reader that skipped it never checked it, so
# every non-blank, non-comment line at the job indent must parse as a header
# or be reported.
JOB_ID = r"[A-Za-z0-9_-]+"
JOB_HEADER = re.compile(rf"^ {{2}}(?:\"({JOB_ID})\"|'({JOB_ID})'|({JOB_ID})):(?:[ \t]+#.*|[ \t]*)$")
JOB_KEY = re.compile(r"^ {4}(?:\"([A-Za-z0-9_.-]+)\"|'([A-Za-z0-9_.-]+)'|([A-Za-z0-9_.-]+)):(?:[ \t]+(.*))?$")
JOBS_KEY = re.compile(r"^jobs:[ \t]*(?:#.*)?$")
QUOTED_STEP_KEY = re.compile(r"^(?: {6}- | {8})[\"'][A-Za-z0-9_-]+[\"']\s*:")


def blank_or_comment(line: str) -> bool:
    return not line.strip() or line.lstrip().startswith("#")


def parse_jobs(workflow: str, legacy: bool = False) -> tuple[dict[str, str], list[str]]:
    """(jobs keyed by id, problems) for the top-level `jobs:` section. A line
    the parser cannot place is reported, never skipped. With `legacy`, a text
    with no `jobs:` key is read from its first line (a fragment)."""
    lines = workflow.splitlines()
    starts = [i for i, line in enumerate(lines) if JOBS_KEY.match(line)]
    problems = ["more than one top-level `jobs:` key"] if len(starts) > 1 else []
    if starts:
        offset = starts[0] + 1
    elif legacy:
        offset = 0
    else:
        return {}, problems
    jobs: dict[str, list[str]] = {}
    current = None
    unplaced = False
    for number, line in enumerate(lines[offset:], offset + 1):
        if blank_or_comment(line):
            if current is not None:
                jobs[current].append(line)
            continue
        indent = len(line) - len(line.lstrip(" "))
        if line[indent] == "\t":
            problems.append(f"line {number} is indented with a tab ({line.strip()!r}); YAML allows spaces only")
            current, unplaced = None, True
        elif indent == 0:
            if starts:
                break
            current = None
        elif indent < 2:
            problems.append(f"line {number} ({line.strip()!r}) sits between the top level and the job ids; "
                            "job ids are indented two spaces")
            current, unplaced = None, True
        elif indent == 2:
            header = JOB_HEADER.match(line)
            if header is None:
                problems.append(f"line {number} ({line.strip()!r}) is at the job indent but is not a job id "
                                "(a plain or quoted id, a colon and at most a comment); the checks cannot read "
                                "that job, so it is refused rather than skipped")
                current, unplaced = None, True
                continue
            current, unplaced = next(g for g in header.groups() if g is not None), False
            if current in jobs:
                problems.append(f"job '{current}' is declared twice (line {number})")
            jobs[current] = []
        elif current is not None:
            jobs[current].append(line)
        elif not unplaced:
            problems.append(f"line {number} ({line.strip()!r}) sits under no job id")
            unplaced = True
    return {name: "\n".join(body) for name, body in jobs.items()}, problems


def workflow_jobs(workflow: str) -> dict[str, str]:
    """Each top-level job's text, keyed by job id."""
    return parse_jobs(workflow, legacy=True)[0]


def job_structure_problems(name: str, text: str, where: str) -> list[str]:
    """The job's own key lines (four spaces) and step keys, read as the
    other checks read them. A key line they cannot parse, a repeated key (a
    commented-out job header folds its body into the job above and repeats
    that job's keys), a flow-style `steps:` or a quoted step key would be
    read past, so each is reported."""
    problems: list[str] = []
    seen: set[str] = set()
    for line in text.splitlines():
        if blank_or_comment(line):
            continue
        indent = len(line) - len(line.lstrip(" "))
        if indent == 4:
            key = JOB_KEY.match(line)
            if key is None:
                problems.append(f"job '{name}'{where} has a line at the job-key indent that is not a plain "
                                f"`key: value` ({line.strip()!r}); the checks cannot read it")
                continue
            word = next(g for g in key.groups()[:3] if g is not None)
            if word in seen:
                problems.append(f"job '{name}'{where} repeats the key `{word}` at the job-key indent; a "
                                "commented-out job header folds that job's keys into the job above it")
            seen.add(word)
            rest = (key.group(4) or "").strip()
            if word == "steps" and rest and not rest.startswith("#"):
                problems.append(f"job '{name}'{where} writes `steps:` inline ({rest!r}); write the steps as a "
                                "block sequence so every step is checked")
        elif indent < 4:
            problems.append(f"job '{name}'{where} has a line indented {indent} spaces ({line.strip()!r}); "
                            "job keys are indented four")
        elif QUOTED_STEP_KEY.match(line):
            problems.append(f"job '{name}'{where} has a quoted step key ({line.strip()!r}); write step keys "
                            "plain so the checks that read `uses:` and `run:` see them")
    return problems


SETUP_NODE = re.compile(r"^[ \t]*(?:-[ \t]+)?uses:[ \t]*[\"']?actions/setup-node@", re.M)


def setup_node_steps(job: str) -> list[tuple[str, list[str]]]:
    """(node-version, lock files) of each setup-node step in a job."""
    steps = re.split(r"\n(?= {6}- )", job)
    found = []
    for step in steps:
        if not SETUP_NODE.search(step):
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
    text = repository_text(path).strip()
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
            engines = json.loads(repository_text(package)).get("engines", {}).get("node", "")
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


def literal_script_paths(command: str) -> list[str]:
    """Candidate script operands, including those inside launcher strings."""
    extension = r'\.(?:py|mjs|cjs|js|sh|bash|ps1)'
    # Keep quoted paths with spaces whole. Scan unquoted operands separately
    # too: a quoted launcher string may contain several script invocations.
    quoted = re.findall(r'"([^"\r\n]+' + extension + r')"', command)
    quoted += re.findall(r"'([^'\r\n]+" + extension + r")'", command)
    unquoted = re.findall(
        r'''(?<![\w./\\?:-])((?:\\\\\?\\)?(?:[A-Za-z]:)?[/\\\w.-]+'''
        + extension + r''')(?=$|[\s"';&|()<>])''', command)
    # Keep the original scanner too: colons and question marks can separate
    # an embedded script operand, even when the whole token looks like a drive.
    embedded = re.findall(
        r'''(?<![\w./-])([/\w.-]+''' + extension + r''')(?=$|[\s"';&|()<>])''', command)
    return list(dict.fromkeys(quoted + unquoted + embedded))


def script_uses_codeflow_bin(command: str, cwd: Path) -> bool:
    """Inspect literal script operands, including those inside launcher strings."""
    # These two files inspect references and construct synthetic controls.
    # The runtime Windows portal test has its own binary-dependent target.
    structural = {ROOT / "scripts/gate-parity.py", ROOT / "scripts/test_gate_parity.py"}
    for name in literal_script_paths(command):
        path = (cwd / name).resolve()
        if path not in structural and path.is_file() and re.search(r"\bCODEFLOW_BIN\b", repository_text(path)):
            return True
    return False


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
    for target in targets.values():
        for mode, settings in target.get("modes", {}).items():
            cmd = norm(settings.get("command", ""))
            # The browser uses the coordinator's CF_PRESENT_CODEFLOW alias.
            # CODEFLOW_BIN consumers are discovered from named scripts.
            consumer = (re.search(r"check:real-browser(\s|$)", cmd)
                        or re.search(r"\bCODEFLOW_BIN\b", cmd)
                        or script_uses_codeflow_bin(cmd, ROOT / target.get("cwd", ".")))
            inputs = set(re.findall(r'''target/(?:debug|release)/codeflow(?:\.exe)?(?=$|[\s"';&|()<>])''', cmd))
            if consumer:
                inputs.add("target/debug/codeflow")
            dependencies = closure(target["name"]) - {target["name"]}
            for artifact in sorted(inputs):
                producers = [targets[name] for name in dependencies if name in targets
                             and artifact in targets[name].get("outputs", [])]
                if consumer:
                    # Only this named producer makes the coordinator export CODEFLOW_BIN.
                    producers = [p for p in producers if p["name"] == "codeflow-bin"
                                 and p.get("outputs") == ["target/debug/codeflow"]]
                if not any(applicable(p) and mode in p.get("modes", {}) for p in producers):
                    problems.append(
                        f"target '{target['name']}' ({mode}) needs a declared prerequisite "
                        f"producing {artifact}; codeflow-bin produces target/debug/codeflow "
                        "and the coordinator sets CODEFLOW_BIN following CARGO_TARGET_DIR")
            if consumer and "CF_PRESENT_CODEFLOW=" in cmd:
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
TIMEOUT_KEY = re.compile(r"^ {4}[\"']?timeout-minutes[\"']?:")
TIMEOUT_LINE = re.compile(r"^ {4}timeout-minutes: ([1-9][0-9]*)(?:[ \t]+#.*)?$")
# TSK-254 AC-1: a hung job frees its runner inside 45 minutes. GitHub's own
# default is 360, the six-hour hold this task exists to stop, so a value
# above this bound is refused even though it is a plain whole number.
MAX_TIMEOUT_MINUTES = 45
CALLER_LINE = re.compile(r"^ {4}[\"']?uses[\"']?:", re.M)
CALLER_TARGET = re.compile(r"^ {4}[\"']?uses[\"']?:[ \t]*(?:\"([^\"]*)\"|'([^']*)'|([^\s#]*))[ \t]*(?:#.*)?$", re.M)
WORKFLOW_DIRS = (Path(".github/workflows"), Path("assets/base/ci"))


def jobs_section(workflow: str) -> dict[str, str]:
    """Each job under the top-level `jobs:` key, keyed by job id."""
    return parse_jobs(workflow)[0]


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
    callee = timeout_problems(repository_text(ROOT / relative), relative.as_posix(),
                              seen=seen | {relative.as_posix()})
    return [f"job '{name}'{where} calls `{target}`, whose own jobs are not all bounded: {callee}"] if callee else []


def timeout_problems(workflow: str, label: str = "",
                     seen: frozenset[str] = frozenset()) -> list[str]:
    jobs, unparsed = parse_jobs(workflow)
    where = f" in {label}" if label else ""
    problems = [] if jobs else [f"the workflow{where} has no `jobs:` section to check for timeouts"]
    problems += [f"the workflow{where}: {p}" for p in unparsed]
    for name, text in jobs.items():
        problems += job_structure_problems(name, text, where)
        if CALLER_LINE.search(text):
            problems += caller_problems(name, text, where, seen)
            continue
        own = [line.rstrip() for line in text.splitlines() if TIMEOUT_KEY.match(line)]
        value = TIMEOUT_LINE.match(own[0]) if len(own) == 1 else None
        if value is None:
            problems.append(f"job '{name}'{where} must carry exactly one `timeout-minutes: <whole minutes>` "
                            f"of its own, so a stalled step cannot hold its runner for six hours; found {own}")
        elif int(value.group(1)) > MAX_TIMEOUT_MINUTES:
            problems.append(f"job '{name}'{where} has `timeout-minutes: {value.group(1)}`, above {MAX_TIMEOUT_MINUTES} "
                            f"minutes (TSK-254 AC-1: a hung job must free its runner inside {MAX_TIMEOUT_MINUTES} "
                            "minutes; GitHub's default is 360, six hours); size it from the observed runs")
    return problems


def workflow_files() -> list[Path]:
    """Every GitHub workflow this repository runs or ships, relative to ROOT."""
    found = []
    for directory in WORKFLOW_DIRS:
        for path in sorted((ROOT / directory).glob("*.y*ml")):
            text = repository_text(path)
            # assets/base/ci also holds other CI systems' files; a workflow
            # names its triggers and jobs at the top level.
            if directory.parts[0] == ".github" or re.search(r"^(on|\"on\"|'on'|jobs|\"jobs\"|'jobs'):", text, re.M):
                found.append(path.relative_to(ROOT))
    return found


def all_timeout_problems() -> list[str]:
    files = workflow_files()
    problems = [] if files else ["no workflow files were found to check for timeouts"]
    for relative in files:
        problems += timeout_problems(repository_text(ROOT / relative), relative.as_posix())
    return problems


# Issue 148: a tag can be moved, so a workflow that holds repository write or
# pull request permissions would run whatever code the tag points at that day.
# Every action a workflow or a shipped template references is pinned to a full
# 40-character commit. A shipped template also names the release the commit
# is as a trailing `# vX.Y.Z` comment, so an adopter can read and move the pin;
# the cargo-dist generated release.yml is pinned by commit without comments.
USES_LINE = re.compile(r"^[ \t]*(?:-[ \t]+)?[\"']?uses[\"']?[ \t]*:[ \t]*(?P<value>.*)$")
ACTION_PIN = re.compile(r"^(?P<action>[^@\s]+)@(?P<ref>[0-9a-f]{40})$")
DOCKER_PIN = re.compile(r"^docker://[^@\s]+@sha256:[0-9a-f]{64}$")
RELEASE_COMMENT = re.compile(r"^v\d+\.\d+\.\d+(?:[ \t].*)?$")


def pin_problems(workflow: str, label: str = "", shipped: bool = False) -> list[str]:
    """Each `uses:` reference in `workflow` that is not a full commit (or, for
    a docker image, digest); in a shipped template also one with no release
    tag comment. A local `./` workflow or action has no ref to pin."""
    where = f"{label}:" if label else "line "
    problems = []
    for number, line in enumerate(workflow.splitlines(), 1):
        found = USES_LINE.match(line)
        if found is None:
            continue
        value, _, comment = found.group("value").partition(" #")
        value = value.strip().strip("\"'")
        if value.startswith("./"):
            continue
        if not (ACTION_PIN.match(value) or DOCKER_PIN.match(value)):
            problems.append(
                f"{where}{number} references `{value}` by a tag, branch or short ref; pin it to a full "
                "40-character commit with its release as a comment (`owner/repo@<commit> # vX.Y.Z`), "
                "so a moved tag cannot change the code a workflow runs (issue 148)")
        elif shipped and not RELEASE_COMMENT.match(comment.strip()):
            problems.append(
                f"{where}{number} pins `{value}` with no release comment; a shipped template names the "
                "release of each pin as a trailing `# vX.Y.Z` so an adopter can read and move it")
    return problems


def all_pin_problems() -> list[str]:
    files = workflow_files()
    problems = [] if files else ["no workflow files were found to check for action pins"]
    for relative in files:
        problems += pin_problems(repository_text(ROOT / relative), relative.as_posix(),
                                 shipped=relative.parts[0] != ".github")
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
    allowed = tomllib.loads(repository_text(config_path)).get("dist", {}).get("allow-dirty", [])
    # cargo-dist reads a list; `in` on a string is a substring test, so
    # "ci" or "preci" as a string must not pass.
    if isinstance(allowed, list) and "ci" in allowed:
        return []
    return ["dist-workspace.toml must set `allow-dirty = [\"ci\"]` while release.yml carries hand-added job "
            "timeouts; otherwise `dist plan` rejects the file and `dist generate` rewrites it without them"]


# TSK-254: only the release present part launches a browser. Its Playwright
# install is bounded and retried: the apt download of the
# browsers' system libraries stalled for up to three hours with no timeout
# (AC-2: each attempt under a 10 minute `timeout`, two attempts, a step
# timeout, no `--with-deps`). Text checks for "a bounded install line" were
# beaten by new spellings each review round (a line-broken command, a
# `timeout` that wraps another command, an install through a variable), so
# the steps are pinned instead: every step of codeflow-ci.yml whose text
# names Playwright, in any case, must be one of these, in this order, apart
# from comment lines, blank lines and trailing spaces. A deliberate change
# edits the workflow and this constant together, and
# scripts/test_gate_parity.py checks that the pin itself keeps the AC-2 bound.
# A step that installs without naming Playwright (a script file, an `eval`)
# is outside a text check; the 45 minute job timeout still ends it.
PLAYWRIGHT_STEPS = (
    r"""      - name: Read the locked Playwright version
        if: matrix.part == 'present'
        id: playwright
        run: |
          present=$(node -p "require('./crates/codeflow-present/web/package-lock.json').packages['node_modules/playwright-core'].version")
          portal=$(node -p "require('./docs-portal/package-lock.json').packages['node_modules/playwright-core'].version")
          test "$present" = "$portal" || { echo "playwright-core differs: present $present, portal $portal"; exit 1; }
          echo "version=$present" >> "$GITHUB_OUTPUT"
          echo "image=${ImageOS:-unknown}-${ImageVersion:-unknown}" >> "$GITHUB_OUTPUT"
""",
    r"""      - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        if: matrix.part == 'present'
        with:
          path: ~/.cache/ms-playwright
          key: playwright-${{ runner.os }}-${{ steps.playwright.outputs.version }}
""",
    r"""      - uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        if: matrix.part == 'present'
        with:
          path: ~/.cache/playwright-apt
          key: playwright-apt-${{ steps.playwright.outputs.image }}-${{ steps.playwright.outputs.version }}
""",
    r"""      - name: Install Playwright browsers from the lockfile
        if: matrix.part == 'present'
        timeout-minutes: 25
        run: |
          npm ci --ignore-scripts --prefix crates/codeflow-present/web
          node=$(command -v node)
          cli=crates/codeflow-present/web/node_modules/playwright-core/cli.js
          archives="$HOME/.cache/playwright-apt"
          mkdir -p "$archives/partial"
          printf 'Dir::Cache::Archives "%s/";\nAPT::Keep-Downloaded-Packages "true";\n' "$archives" |
            sudo tee /etc/apt/apt.conf.d/90codeflow-playwright-archives >/dev/null
          echo "apt archives restored: $(find "$archives" -maxdepth 1 -name '*.deb' | wc -l)"
          for attempt in 1 2; do
            if timeout --kill-after=30s 10m "$node" "$cli" install chromium firefox webkit &&
              sudo timeout --kill-after=30s 10m "$node" "$cli" install-deps chromium firefox webkit; then
              sudo apt-get autoclean || echo "::warning::apt-get autoclean failed; the cache keeps stale archives"
              sudo chown -R "$(id -u):$(id -g)" "$archives" || echo "::warning::the apt archives may not be saved"
              echo "apt archives kept: $(find "$archives" -maxdepth 1 -name '*.deb' | wc -l)"
              exit 0
            fi
            echo "::warning::Playwright install attempt ${attempt} failed or ran past 10 minutes"
            sudo dpkg --configure -a || true
          done
          echo "::error::the Playwright install failed twice; see the attempts above"
          exit 1
""",
)
PIN_FIX = ("to change a Playwright step on purpose, make the same edit in .github/workflows/codeflow-ci.yml "
           "and in PLAYWRIGHT_STEPS or LINE_APT_STEPS in scripts/gate-parity.py (one entry per step, in workflow order), keep "
           "each install attempt under a `timeout` of at most 10 minutes, two attempts, the step's "
           "`timeout-minutes` and `if: matrix.part == 'present'` (TSK-254 AC-2), then run "
           "`python3 -B -m unittest scripts.test_gate_parity`")


# TSK-1263: integration pushes prepare only apt archives, with the same
# key and bounded install as the release job. Pin each step and its trigger.
LINE_APT_STEPS = (
    r"""      - name: Read the locked Playwright version
        if: matrix.part == 'present' && github.event_name == 'push' && startsWith(github.ref, 'refs/heads/integration/')
        id: playwright
        run: |
          present=$(node -p "require('./crates/codeflow-present/web/package-lock.json').packages['node_modules/playwright-core'].version")
          portal=$(node -p "require('./docs-portal/package-lock.json').packages['node_modules/playwright-core'].version")
          test "$present" = "$portal" || { echo "playwright-core differs: present $present, portal $portal"; exit 1; }
          echo "version=$present" >> "$GITHUB_OUTPUT"
          echo "image=${ImageOS:-unknown}-${ImageVersion:-unknown}" >> "$GITHUB_OUTPUT"
""",
    r"""      - name: Cache Playwright apt archives on the integration line
        uses: actions/cache@55cc8345863c7cc4c66a329aec7e433d2d1c52a9 # v6.1.0
        if: matrix.part == 'present' && github.event_name == 'push' && startsWith(github.ref, 'refs/heads/integration/')
        id: line-apt
        with:
          path: ~/.cache/playwright-apt
          key: playwright-apt-${{ steps.playwright.outputs.image }}-${{ steps.playwright.outputs.version }}
""",
    r"""      - name: Prepare Playwright apt archives for line verdict reuse
        if: matrix.part == 'present' && github.event_name == 'push' && startsWith(github.ref, 'refs/heads/integration/') && steps.line-apt.outputs.cache-hit != 'true'
        timeout-minutes: 25
        run: |
          npm ci --ignore-scripts --prefix crates/codeflow-present/web
          node=$(command -v node)
          cli=crates/codeflow-present/web/node_modules/playwright-core/cli.js
          archives="$HOME/.cache/playwright-apt"
          mkdir -p "$archives/partial"
          printf 'Dir::Cache::Archives "%s/";\nAPT::Keep-Downloaded-Packages "true";\n' "$archives" |
            sudo tee /etc/apt/apt.conf.d/90codeflow-playwright-archives >/dev/null
          echo "apt archives restored: $(find "$archives" -maxdepth 1 -name '*.deb' | wc -l)"
          for attempt in 1 2; do
            if sudo timeout --kill-after=30s 10m "$node" "$cli" install-deps chromium firefox webkit; then
              sudo apt-get autoclean || echo "::warning::apt-get autoclean failed; the cache keeps stale archives"
              sudo chown -R "$(id -u):$(id -g)" "$archives" || echo "::warning::the apt archives may not be saved"
              echo "apt archives kept: $(find "$archives" -maxdepth 1 -name '*.deb' | wc -l)"
              exit 0
            fi
            echo "::warning::Playwright install attempt ${attempt} failed or ran past 10 minutes"
            sudo dpkg --configure -a || true
          done
          echo "::error::the Playwright install failed twice; see the attempts above"
          exit 1
""",
)


def playwright_problems(workflow: str) -> list[str]:
    """Pin browser installation in release and apt preparation on line pushes."""
    problems = []
    expected = {"gates-release": PLAYWRIGHT_STEPS, "gates": LINE_APT_STEPS}
    found = {name: [] for name in expected}
    for name, text in jobs_section(workflow).items():
        for step in re.split(r"\n(?= {6}- )", text):
            lines = norm_job(step)
            if "playwright" not in "\n".join(lines).lower():
                continue
            if name not in expected:
                problems.append(f"job '{name}' has a Playwright step ({lines[0].strip()}); "
                                "only gates-release installs browsers and gates prepares apt archives")
                continue
            found[name].append(lines)
    for name, steps in expected.items():
        pinned = [norm_job(step) for step in steps]
        for lines in found[name]:
            if lines not in pinned:
                problems.append(f"the {name} step `{lines[0].strip()}` names Playwright but matches no pinned "
                                f"step in PLAYWRIGHT_STEPS or LINE_APT_STEPS; {PIN_FIX}")
        for step, lines in zip(steps, pinned):
            if lines not in found[name]:
                problems.append(f"the {name} job lacks this pinned Playwright step, or changed it:\n{step}{PIN_FIX}")
        if (found[name] != pinned and all(lines in pinned for lines in found[name])
                and all(lines in found[name] for lines in pinned)):
            problems.append(f"the {name} job's Playwright steps repeat or reorder the pinned steps; {PIN_FIX}")
    return problems


def main() -> int:
    cfg = json.loads(repository_text(CONFIG))
    workflow = repository_text(WORKFLOW)
    pins = (node_pin_problems(cfg, workflow) + gate_binary_problems(cfg)
            + gate_part_problems(cfg, workflow) + shared_install_problems(cfg)
            + all_timeout_problems() + all_pin_problems() + dist_problems() + playwright_problems(workflow)
            + windows_verdict_problems(workflow) + windows_tier_problems(workflow))
    for problem in pins:
        print(f"GATE PARITY DRIFT: {problem}", file=sys.stderr)
    status = rust_parity()
    if pins:
        return 1
    if status == 0:
        print("gate-parity OK: Node targets run on their CI pins, the "
              "real-browser check runs the gate's binary, the gate parts and coverage shards "
              "run every full-mode target, every job of every workflow has a timeout, every action "
              "is pinned to a commit, and only "
              "the present part touches Playwright, through the pinned steps")
    return status


def windows_owed(local: set[str]) -> set[str]:
    """The local Rust commands Windows must also run: all but fmt, whose
    result does not depend on the platform (TSK-449)."""
    return {cmd for cmd in local if not cmd.startswith("cargo fmt ")}


def rust_parity() -> int:
    notes: list[str] = []
    local = windows_owed(local_rust_commands(json.loads(repository_text(CONFIG)), notes))
    ci = ci_rust_job_commands(repository_text(WORKFLOW))
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
