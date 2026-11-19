#!/usr/bin/env python3
"""Where each job of .github/workflows/codeflow-ci.yml, of the line workflow
(codeflow-line-pr.yml) and of the daily run (windows-daily-run.yml) runs
(TSK-1321).

Suite placement is an operator decision (2026-11-05, "A plus Linux off line
PRs"; 2026-11-06 15:34; and 2026-11-11 19:40 to 20:35, which removed every
line push run, kept Windows off line pull requests and relaxed `main` for a
time). The placement ADR (ADR-0097, "CI suite placement by event") and the
maintainer page (docs/releasing.md, "Where CI suites run") each hold it as one
table, by event and suite. An accepted ADR is never rewritten, so a placement
change is appended to it as a dated note: the ADR's table is the one in its
latest dated note that carries one, else the one in its Decision. These
tests read every workflow's triggers and every job's `if:` and `needs:` from
the workflows themselves and evaluate them as GitHub does for each kind of
run. They fail when:

- the doc table differs from that ADR table;
- a cell of the doc table differs from what the workflows run for that event
  and suite, in either direction, mapped through SUITES, the one job-to-suite
  map;
- PLACEMENT below, one row per job and one column per kind of run, differs
  from the workflows or from the doc table;
- a table, row, column or cell cannot be read, a job is in no suite, or a
  suite has no job (fail closed);
- a pull request into an integration line starts a job named for a required
  check of main, skipped or not, since GitHub counts a skipped required check
  as a success, or a push to an integration line starts a run.

So a job added, removed or moved needs a visible edit to PLACEMENT, and a
placement change needs the doc and ADR tables changed with it, which takes a
new operator decision.
"""

import importlib.util
import json
import os
import re
import subprocess
import tempfile
import unittest
from fnmatch import fnmatchcase
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)
WORKFLOWS = ROOT / ".github" / "workflows"
WORKFLOW = (WORKFLOWS / "codeflow-ci.yml").read_text(encoding="utf-8")

# The kinds of run. A ready pull request into an integration line or into
# main, a draft into either, a ready pull request into any other base; a push
# to a line or to main; the schedule of codeflow-ci.yml on main's tip; a
# manual run; the daily Windows run of windows-daily-run.yml.
RUNS = {
    "line PR": {"event": "pull_request", "base": "integration/EPC-022-the-3-2-0-milestone-fixes",
                "ref": "refs/pull/1/merge"},
    "draft line PR": {"event": "pull_request", "base": "integration/EPC-022-the-3-2-0-milestone-fixes",
                      "ref": "refs/pull/1/merge", "draft": True},
    "main PR": {"event": "pull_request", "base": "main", "ref": "refs/pull/1/merge"},
    "draft main PR": {"event": "pull_request", "base": "main", "ref": "refs/pull/1/merge", "draft": True},
    "other PR": {"event": "pull_request", "base": "feature/x", "ref": "refs/pull/1/merge"},
    "line push": {"event": "push", "base": "", "ref": "refs/heads/integration/EPC-022-the-3-2-0-milestone-fixes"},
    "main push": {"event": "push", "base": "", "ref": "refs/heads/main"},
    "scheduled": {"event": "schedule", "base": "", "ref": "refs/heads/main"},
    "manual": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/main"},
    "daily": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/integration/EPC-022-the-3-2-0-milestone-fixes",
              "workflow": "windows-daily-run.yml"},
}

# The placement table. `yes` runs, `-` does not. A push to an integration
# line starts no run (permanent). Until the line gate ships, a pull request
# into a line starts only the line workflow, which runs the short checks:
# provenance, the secret scan, its canary and the security review (its commit
# standards and release impact run in codeflow-policy.yml and
# codeflow-release.yml), and on a ready run the line verdict over them. While
# main is relaxed (operator decision, 2026-11-11), a pull request into main or
# any other base runs the same short checks in codeflow-ci.yml, a push to main
# adds the image and reuse jobs, and every Linux and Windows suite runs only
# on the schedule and a manual run. The two required verdicts never run on a
# line pull request, so they cannot pass there with their suites not
# selected. The daily column describes the disabled windows-daily-run.yml
# worker at the resolved commit: Linux and Windows without release gates.
# AC-11's active-line scope remains owed to the Director; this table enables
# no workflow. Each row names its jobs' workflow: ci for
# codeflow-ci.yml, line for the line workflow, daily for the daily runner.
PLACEMENT = """\
job                       workflow  line PR  draft line PR  main PR  draft main PR  other PR  line push  main push  scheduled  manual  daily
ubuntu-image              ci        -        -              -        -              -         -          yes        -          -       -
windows-image             ci        -        -              -        -              -         -          yes        -          -       -
reuse                     ci        -        -              -        -              -         -          yes        -          -       -
provenance                ci        -        -              yes      yes            yes       -          -          -          -       -
gates-build               ci        -        -              -        -              -         -          -          yes        yes     -
gates                     ci        -        -              -        -              -         -          -          yes        yes     -
gates-release             ci        -        -              -        -              -         -          -          yes        yes     -
gates-coverage            ci        -        -              -        -              -         -          -          yes        yes     -
gates-coverage-report     ci        -        -              -        -              -         -          -          yes        yes     -
gates-verdict             ci        -        -              -        -              -         -          -          yes        yes     -
windows-build             ci        -        -              -        -              -         -          -          yes        yes     -
windows-tests             ci        -        -              -        -              -         -          -          yes        yes     -
windows                   ci        -        -              -        -              -         -          -          yes        yes     -
windows-release-journey   ci        -        -              -        -              -         -          -          yes        yes     -
windows-journeys          ci        -        -              -        -              -         -          -          yes        yes     -
windows-verdict           ci        -        -              -        -              -         -          -          yes        yes     -
secret-scan               ci        -        -              yes      yes            yes       -          yes        yes        yes     -
gitleaks-canary           ci        -        -              yes      yes            yes       -          yes        yes        yes     -
security-review           ci        -        -              yes      yes            yes       -          yes        yes        yes     -
provenance                line      yes      yes            -        -              -         -          -          -          -       -
gates-line-verdict        line      yes      -              -        -              -         -          -          -          -       -
secret-scan               line      yes      yes            -        -              -         -          -          -          -       -
gitleaks-canary           line      yes      yes            -        -              -         -          -          -          -       -
security-review           line      yes      yes            -        -              -         -          -          -          -       -
gates-build               daily     -        -              -        -              -         -          -          -          -       yes
gates                     daily     -        -              -        -              -         -          -          -          -       yes
gates-coverage            daily     -        -              -        -              -         -          -          -          -       yes
gates-coverage-report     daily     -        -              -        -              -         -          -          -          -       yes
windows-build             daily     -        -              -        -              -         -          -          -          -       yes
windows-tests             daily     -        -              -        -              -         -          -          -          -       yes
windows                   daily     -        -              -        -              -         -          -          -          -       yes
windows-release-journey   daily     -        -              -        -              -         -          -          -          -       yes
windows-journeys          daily     -        -              -        -              -         -          -          -          -       yes
"""
WORKFLOW_FILES = {"ci": "codeflow-ci.yml", "line": "codeflow-line-pr.yml", "daily": "windows-daily-run.yml"}

# The short checks a line pull request keeps in the other workflows: commit
# standards and the policy check, and release impact. Their only job
# condition, if any, is the pull request event, so they run on every pull
# request.
OTHER_SHORT_CHECKS = {"codeflow-policy.yml": ("commit-lint", "release-impact"),
                      "codeflow-release.yml": ("release-impact",)}


def table() -> dict[tuple[str, str], set[str]]:
    """Each (workflow, job) row's kinds of run."""
    lines = PLACEMENT.splitlines()
    columns = re.split(r"\s{2,}", lines[0].strip())[2:]
    if columns != list(RUNS):
        raise AssertionError(f"the table's columns {columns} are not the kinds of run {list(RUNS)}")
    rows = {}
    for line in lines[1:]:
        cells = line.split()
        key = tuple(cells[1::-1])
        if (len(cells) != len(columns) + 2 or cells[1] not in WORKFLOW_FILES or set(cells[2:]) - {"yes", "-"}
                or key in rows):
            raise AssertionError(f"malformed placement row: {line!r}")
        rows[key] = {column for column, cell in zip(columns, cells[2:]) if cell == "yes"}
    return rows


def workflow_text(name: str) -> str:
    return WORKFLOW if name == "ci" else (WORKFLOWS / WORKFLOW_FILES[name]).read_text(encoding="utf-8")


def started_by(kind: str) -> set[str]:
    """The workflows a kind of run can start: the daily run only the daily
    runner, any other ci and the line workflow, by their triggers."""
    return {"daily"} if RUNS[kind].get("workflow") == WORKFLOW_FILES["daily"] else {"ci", "line"}


DISPATCHER = WORKFLOWS / "windows-daily.yml"


def triggers(text: str) -> dict[str, dict[str, list[str]]]:
    """Read the whole supported trigger mapping; refuse unmodelled restrictions."""
    block = re.search(r"(?ms)^on:\n(.*?)(?=^[^\s#])", text)
    if not block:
        raise ValueError("a workflow has no readable `on:` block")
    found, bodies, event = {}, {}, None
    for line in block.group(1).splitlines():
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        header = re.fullmatch(r"  ([a-z_]+):\s*", line)
        if header:
            event = header.group(1)
            if event in bodies:
                raise ValueError(f"trigger {event}: duplicate event")
            bodies[event] = []
        elif event is None or not line.startswith("    "):
            raise ValueError(f"cannot read trigger mapping: {line!r}")
        else:
            bodies[event].append(line)
    for event, lines in bodies.items():
        filters = {}
        if event in {"pull_request", "push"}:
            for line in lines:
                field = re.fullmatch(r"    ([a-z-]+): \[(.+)\]", line)
                if not field:
                    raise ValueError(f"trigger {event}: unsupported restriction {line.strip()!r}")
                key, raw = field.groups()
                if key not in {"branches", "branches-ignore", "types"} or key in filters:
                    raise ValueError(f"trigger {event}: unsupported or duplicate restriction {key}")
                values = []
                for item in raw.split(","):
                    value = re.fullmatch(r"(?:'([A-Za-z0-9_./*-]+)'|\"([A-Za-z0-9_./*-]+)\"|([A-Za-z0-9_./*-]+))", item.strip())
                    if not value:
                        raise ValueError(f"trigger {event}: unsupported {key} value {item!r}")
                    values.append(next(part for part in value.groups() if part is not None))
                if key == "types" and (event != "pull_request" or set(values) != {
                        "opened", "synchronize", "reopened", "ready_for_review"}):
                    raise ValueError(f"trigger {event}: unsupported types restriction {values}")
                filters[key] = values
            if "branches" in filters and "branches-ignore" in filters:
                raise ValueError(f"trigger {event}: branches and branches-ignore cannot be combined")
        elif event == "schedule":
            if not lines or any(not re.fullmatch(r"    - cron: [\"'][0-9*/?, -]+[\"']", line) for line in lines):
                raise ValueError(f"trigger {event}: unsupported schedule {lines!r}")
        elif event == "workflow_dispatch":
            # Inputs describe dispatch parameters and do not filter its event.
            # Accept the runner's pinned identity input, including every field.
            if lines and lines != [
                "    inputs:", "      source-sha:",
                "        description: The exact commit the dispatcher discovered; the run fails if the ref's tip has moved",
                "        required: true", "        type: string",
            ]:
                raise ValueError(f"trigger {event}: unsupported dispatch mapping {lines!r}")
        else:
            raise ValueError(f"unsupported trigger event {event}")
        found[event] = filters
    if not found:
        raise ValueError("a workflow has no supported trigger events")
    return found


def triggered(text: str, run: dict) -> bool:
    """Whether the workflow starts for this run, by its `on:` events and branch filters."""
    events = triggers(text)
    if run["event"] not in events:
        return False
    name = run["ref"].removeprefix("refs/heads/") if run["event"] == "push" else run["base"]
    def matches(patterns):
        return any(fnmatchcase(name, pattern.replace("**", "*")) for pattern in patterns)
    filters = events[run["event"]]
    if "branches" in filters and not matches(filters["branches"]):
        return False
    return not matches(filters.get("branches-ignore", []))


def ran_by(kind: str, texts: dict[str, str] | None = None, reuse_run: str = "",
           dispatcher: str | None = None) -> dict[str, set[str]]:
    """The jobs that run on a kind of run, by workflow, every job that runs
    passing. `reuse_run` is the run id the reuse job names, if any. The daily
    run starts only when the dispatcher runs on a schedule."""
    texts = texts or {name: workflow_text(name) for name in WORKFLOW_FILES}
    started = started_by(kind)
    if kind == "daily":
        dispatcher = DISPATCHER.read_text(encoding="utf-8") if dispatcher is None else dispatcher
        if "schedule" not in triggers(dispatcher):
            return {name: set() for name in WORKFLOW_FILES}
    return {name: simulate(texts[name], RUNS[kind], reuse_run=reuse_run) if name in started else set()
            for name in WORKFLOW_FILES}


def ran_by_table(kind: str) -> dict[str, set[str]]:
    """The jobs PLACEMENT says run on a kind of run, by workflow."""
    found = {name: set() for name in WORKFLOW_FILES}
    for (name, job), kinds in table().items():
        if kind in kinds:
            found[name].add(job)
    return found


# A small evaluator for the expression forms this workflow uses. Anything it
# does not know is an error, never a guess.
TOKEN = re.compile(r"\s*(?:(\$\{\{|\}\})|('(?:[^']|'')*')|(&&|\|\||==|!=|[!(),])|([A-Za-z_][A-Za-z0-9_.-]*))")
STATUS = {"always", "cancelled", "success", "failure"}


def tokens(text: str) -> list[str]:
    out, at = [], 0
    text = text.strip()
    while at < len(text):
        match = TOKEN.match(text, at)
        if not match or match.end() == at:
            raise ValueError(f"cannot read the expression at {text[at:at + 30]!r}")
        if not match.group(1):
            out.append(match.group(0).strip())
        at = match.end()
    return out


class Expression:
    def __init__(self, text: str, context: dict, needs: dict, direct: list[str]):
        self.items, self.at = tokens(text), 0
        self.context, self.needs, self.direct = context, needs, direct

    def peek(self):
        return self.items[self.at] if self.at < len(self.items) else None

    def take(self, want=None):
        item = self.peek()
        if item is None or (want is not None and item != want):
            raise ValueError(f"expected {want!r}, found {item!r} in {' '.join(self.items)}")
        self.at += 1
        return item

    def evaluate(self):
        value = self.either()
        if self.peek() is not None:
            raise ValueError(f"unread tokens {self.items[self.at:]}")
        return value

    def either(self):
        value = self.both()
        while self.peek() == "||":
            self.take()
            right = self.both()
            value = value if truthy(value) else right
        return value

    def both(self):
        value = self.unary()
        while self.peek() == "&&":
            self.take()
            right = self.unary()
            value = right if truthy(value) else value
        return value

    def unary(self):
        if self.peek() == "!":
            self.take()
            return not truthy(self.unary())
        return self.compare()

    def compare(self):
        left = self.primary()
        if self.peek() in ("==", "!="):
            op = self.take()
            right = self.primary()
            same = fold(left) == fold(right)
            return same if op == "==" else not same
        return left

    def primary(self):
        item = self.take()
        if item == "(":
            value = self.either()
            self.take(")")
            return value
        if item.startswith("'"):
            return item[1:-1].replace("''", "'")
        if self.peek() == "(":
            self.take("(")
            args = []
            while self.peek() != ")":
                args.append(self.either())
                if self.peek() == ",":
                    self.take(",")
            self.take(")")
            return self.call(item, args)
        return self.lookup(item)

    def call(self, name, args):
        if name == "always" and not args:
            return True
        if name == "cancelled" and not args:
            return False
        if name == "failure" and not args:
            return False
        if name == "success" and not args:
            return all(self.needs[job]["result"] == "success" for job in self.direct)
        if name == "fromJSON" and len(args) == 1:
            return json.loads(args[0])
        if name == "contains" and len(args) == 2:
            haystack, needle = args
            if isinstance(haystack, list):
                return fold(needle) in [fold(item) for item in haystack]
            return fold(needle) in fold(haystack)
        if name == "startsWith" and len(args) == 2:
            return fold(args[0]).startswith(fold(args[1]))
        raise ValueError(f"unknown function {name}({args})")

    def lookup(self, name):
        if name.startswith("needs."):
            parts = name.split(".")
            job = parts[1]
            if job not in self.direct:
                raise ValueError(f"{name} reads a job the job does not need")
            if parts[2:] == ["result"]:
                return self.needs[job]["result"]
            if len(parts) == 4 and parts[2] == "outputs":
                return self.needs[job]["outputs"].get(parts[3], "")
            raise ValueError(f"unknown needs value {name}")
        if name in self.context:
            return self.context[name]
        raise ValueError(f"unknown context value {name}")


def truthy(value) -> bool:
    return value not in (None, False, "", 0)


def fold(value):
    # GitHub compares strings without case.
    return value.lower() if isinstance(value, str) else value


def job_fields(text: str) -> tuple[str | None, list[str]]:
    """A job's `if:` expression and its `needs:` list, each on one line."""
    condition = re.search(r"^ {4}if:\s*(.+?)\s*$", text, re.M)
    need = re.search(r"^ {4}needs:\s*(.+?)\s*$", text, re.M)
    needs = []
    if need:
        value = need.group(1)
        needs = [item.strip() for item in value.strip("[]").split(",")] if value.startswith("[") else [value]
    return (condition.group(1) if condition else None), needs


def simulate(workflow: str, run: dict, reuse_run: str = "") -> set[str]:
    """The jobs that run, every job that runs passing; `reuse_run` is the run
    the reuse job names."""
    if not triggered(workflow, run):
        return set()
    jobs = parity.workflow_jobs(workflow)
    fields = {name: job_fields(text) for name, text in jobs.items()}
    context = {"github.event_name": run["event"], "github.event.pull_request.base.ref": run["base"],
               "github.event.pull_request.draft": run.get("draft", False) if run["event"] == "pull_request" else None,
               "github.ref": run["ref"]}
    results, pending = {}, list(jobs)
    while pending:
        ready = [name for name in pending if all(need in results for need in fields[name][1])]
        if not ready:
            raise ValueError(f"the needs of {pending} never resolve")
        for name in ready:
            pending.remove(name)
            condition, direct = fields[name]
            text = condition or "success()"
            if not set(tokens(text)) & STATUS:
                text = f"success() && ({text})"
            runs = truthy(Expression(text, context, results, direct).evaluate())
            outputs = {"run": reuse_run} if runs and name == "reuse" and reuse_run else {}
            results[name] = {"result": "success" if runs else "skipped", "outputs": outputs}
    return {name for name, value in results.items() if value["result"] != "skipped"}


class PlacementTests(unittest.TestCase):
    """Every job runs on exactly the kinds of run its row names."""

    def test_the_table_has_one_row_per_job_of_each_workflow(self):
        expected = {(name, job) for name in WORKFLOW_FILES for job in parity.workflow_jobs(workflow_text(name))}
        self.assertEqual(sorted(table()), sorted(expected))

    def test_every_job_runs_where_its_row_says_and_nowhere_else(self):
        rows = table()
        for kind in RUNS:
            ran = ran_by(kind)
            for (name, job), kinds in rows.items():
                with self.subTest(run=kind, workflow=name, job=job):
                    runs = job in ran[name]
                    self.assertEqual(runs, kind in kinds,
                                     f"{name} {job} {'runs' if runs else 'does not run'} on a {kind}; its row says "
                                     f"{'yes' if kind in kinds else '-'}")

    def test_the_daily_worker_keeps_linux_and_windows_without_release_gates(self):
        # AC-11's active-line scope is preserved; this test enables no workflow.
        scheduled, daily = ran_by("scheduled")["ci"], ran_by("daily")["daily"]
        suites = SUITES["Windows checks, journeys and partitions"] | SUITES["Linux build, lint, tests and coverage"]
        self.assertEqual(scheduled & suites, daily)

    def test_a_line_push_starts_no_run(self):
        # Permanent (operator, 2026-11-11). scripts/gate-parity.py refuses a
        # push trigger naming a line in any workflow of the repository.
        texts = {name: workflow_text(name) for name in WORKFLOW_FILES}
        self.assertEqual([name for name in sorted(started_by("line push"))
                          if triggered(texts[name], RUNS["line push"])], [])
        self.assertFalse((WORKFLOWS / "codeflow-line-push.yml").exists())

    def test_a_line_pull_request_runs_only_the_short_checks(self):
        ran = ran_by("line PR")
        self.assertEqual(ran["ci"], set())
        self.assertEqual(ran["line"], {"provenance", "gates-line-verdict", "secret-scan", "gitleaks-canary",
                                       "security-review"})

    def test_a_line_pull_request_publishes_no_required_name_of_main(self):
        # A job whose `if:` is false still leaves a skipped row, so this reads
        # every job of every workflow the run starts, not only those that run.
        texts = {name: workflow_text(name) for name in WORKFLOW_FILES}
        for kind in ("line PR", "draft line PR"):
            started = [name for name in sorted(started_by(kind)) if triggered(texts[name], RUNS[kind])]
            self.assertEqual(started, ["line"], kind)
            for name in started:
                published = {re.search(r"^ {4}name:\s*(.+?)\s*$", text, re.M).group(1)
                             for text in parity.workflow_jobs(texts[name]).values()}
                with self.subTest(run=kind, workflow=name):
                    self.assertEqual(published & MAIN_ONLY_NAMES, set())

    def test_a_workflow_widened_onto_line_pull_requests_is_caught(self):
        # The widened run would leave skipped rows named for main's required
        # checks on the line pull request.
        ci = workflow_text("ci")
        widened = ci.replace('    branches-ignore: ["integration/**"]\n', "", 1)
        self.assertNotEqual(widened, ci)
        self.assertTrue(triggered(widened, RUNS["line PR"]))
        published = {re.search(r"^ {4}name:\s*(.+?)\s*$", text, re.M).group(1)
                     for text in parity.workflow_jobs(widened).values()}
        self.assertEqual(published & MAIN_ONLY_NAMES, MAIN_ONLY_NAMES)

    def test_a_short_check_dropped_from_line_pull_requests_is_caught(self):
        line = workflow_text("line")
        moved = line.replace("\n  secret-scan:\n", "\n  secret-scan:\n    if: github.event_name == 'push'\n", 1)
        self.assertNotEqual(moved, line)
        self.assertNotIn("secret-scan", simulate(moved, RUNS["line PR"]))

    def test_any_base_that_is_not_a_line_starts_the_full_workflow(self):
        # Fail closed: only a base GitHub reads as integration/** avoids the
        # full run. Nothing else can select the line workflow.
        for base in ("main", "master", "feature/x", "task/integration/x", "integration", "Integration/x"):
            with self.subTest(base=base):
                run = {**RUNS["main PR"], "base": base}
                self.assertTrue(triggered(workflow_text("ci"), run))
                self.assertFalse(triggered(workflow_text("line"), run))
        self.assertTrue(triggered(workflow_text("ci"), {**RUNS["main PR"], "base": ""}))

    def test_every_other_event_starts_the_full_workflow(self):
        for kind in ("main push", "scheduled", "manual"):
            with self.subTest(run=kind):
                self.assertTrue(triggered(workflow_text("ci"), RUNS[kind]))
                self.assertFalse(triggered(workflow_text("line"), RUNS[kind]))


# The parity check (TSK-1321 AC-12). The placement table of docs/releasing.md,
# "Where CI suites run", and of the ADR that section links, by event and suite.
DOC = ROOT / "docs" / "releasing.md"
SECTION = "### Where CI suites run"
COLUMNS = ("Event", "Short checks", "Linux build, lint, tests and coverage",
           "Windows checks, journeys and partitions", "Release gates")
# Each event row of the table, and the kinds of run that stand for it. The
# daily Windows run of windows-daily-run.yml has no row while main is
# relaxed: the operator switches it off then (ADR-0097, note of 2026-11-11).
EVENT_ROWS = {
    "Draft pull request, any base": ("draft line PR", "draft main PR"),
    "Ready pull request into an integration line": ("line PR",),
    "Push to an integration line": ("line push",),
    "Ready pull request into `main`, while `main` is relaxed": ("main PR",),
    "Push to `main`": ("main push",),
    "Daily run, `main`'s tip": ("scheduled",),
    "Manual dispatch": ("manual",),
}
# The one job-to-suite map. The Linux tests run inside the coverage shards.
# Every job of both workflows is in a suite, a short check, or NOT_A_SUITE.
SUITES = {
    "Linux build, lint, tests and coverage": frozenset({"gates-build", "gates", "gates-coverage",
                                                         "gates-coverage-report"}),
    "Windows checks, journeys and partitions": frozenset({"windows", "windows-release-journey", "windows-journeys",
                                                           "windows-build", "windows-tests"}),
    "Release gates": frozenset({"gates-release"}),
}
# The short checks that run in codeflow-ci.yml and the line workflow. The
# daily run's secret scan reads the full history, and its dependency audit
# runs in the security review.
SHORT_JOBS = {"secret scan": frozenset({"secret-scan", "gitleaks-canary"}),
              "security review": frozenset({"security-review"})}
# Short checks of codeflow-policy.yml and codeflow-release.yml, on pull requests
# only (OtherWorkflowTests).
PR_ONLY_CHECKS = frozenset({"policy check", "commit standards", "release impact"})
# Jobs that serve a run itself: images, reuse, provenance, verdicts.
NOT_A_SUITE = frozenset({"ubuntu-image", "windows-image", "reuse", "provenance",
                         "gates-verdict", "windows-verdict", "gates-line-verdict"})
# The verdicts that run only where their suites run (ADR-0097). A line pull
# request must leave no row of these names, skipped or not.
MAIN_ONLY_NAMES = frozenset({"codeflow gates", "windows"})
# Every short-checks cell the table may hold: the checks it names, and the
# jobs that serve the run itself that must run with them. "yes" is the short
# checks the page lists. "no run" means no workflow starts. `release state`
# runs in codeflow-release.yml, which this check does not evaluate. Any other
# text fails.
PR_SHORT = frozenset({"policy check", "commit standards", "release impact", "secret scan", "security review"})
SCANS = frozenset({"secret scan", "security review"})
SHORT_CELLS = {
    "yes": (PR_SHORT, frozenset()),
    "yes; `commit standards` passes only on a ready run": (PR_SHORT, frozenset()),
    "no": (frozenset(), frozenset()),
    "no run": (frozenset(), frozenset()),
    "secret scan, security review, image and reuse jobs, and `release state`":
        (SCANS, frozenset({"ubuntu-image", "windows-image", "reuse"})),
    "secret scan of the full history and dependency audit": (SCANS, frozenset()),
    "as the daily run": (SCANS, frozenset()),
}
# Every suite cell the table may hold. A reused cell runs the suite unless the
# reuse job names a run, and then skips it; the restored push to main uses it.
# "no run" skips, and no workflow may start. Any other text fails.
SUITE_CELLS = {"yes": "runs", "no": "skips", "no run": "skips",
               "reused when the tree matches, else yes": "reused"}


def placement_section(doc: str) -> str:
    lines = doc.splitlines()
    if lines.count(SECTION) != 1:
        raise ValueError(f"docs/releasing.md has {lines.count(SECTION)} {SECTION!r} sections, not one")
    start = lines.index(SECTION) + 1
    end = next((at for at in range(start, len(lines)) if lines[at].startswith(("## ", "### "))), len(lines))
    return "\n".join(lines[start:end])


def placement_table(text: str, where: str) -> list[tuple[str, ...]]:
    """The one table whose header is COLUMNS, as rows of cells."""
    lines = text.splitlines()
    starts = [at for at, line in enumerate(lines) if line.startswith("| Event |")]
    if len(starts) != 1:
        raise ValueError(f"{where} has {len(starts)} placement tables, not one")
    def cells(line):
        return tuple(cell.strip() for cell in line.strip().strip("|").split("|"))
    header = cells(lines[starts[0]])
    if header != COLUMNS:
        raise ValueError(f"{where}'s placement table has the columns {list(header)}, not {list(COLUMNS)}")
    if set(lines[starts[0] + 1].replace("|", "")) - {"-", " "}:
        raise ValueError(f"{where}'s placement table has no separator row")
    rows = []
    for line in lines[starts[0] + 2:]:
        if not line.startswith("|"):
            break
        row = cells(line)
        if len(row) != len(COLUMNS):
            raise ValueError(f"{where}'s placement row {line!r} has {len(row)} cells, not {len(COLUMNS)}")
        rows.append(row)
    return rows


def placement_adr(section: str) -> str:
    links = set(re.findall(r"\]\((decisions/ADR-\d{4}-[a-z0-9-]+\.md)\)", section))
    if len(links) != 1:
        raise ValueError(f"the {SECTION!r} section links {len(links)} ADRs, not one: {sorted(links)}")
    return links.pop()


NOTE = re.compile(r"## Note, (\d{4}-\d{2}-\d{2})")


def adr_placement(text: str, where: str) -> list[tuple[str, ...]]:
    """The ADR's placement table: the one in its latest dated note that
    carries one, else the one in its Decision. An accepted ADR is never
    rewritten, so a placement change is a dated note with its own table."""
    sections, heading, body = [], None, []
    for line in text.splitlines():
        if line.startswith("## "):
            sections.append((heading, body))
            heading, body = line.strip(), []
        else:
            body.append(line)
    sections.append((heading, body))
    noted = [(NOTE.fullmatch(heading).group(1), at, "\n".join(lines))
             for at, (heading, lines) in enumerate(sections)
             if heading and NOTE.fullmatch(heading) and any(line.startswith("| Event |") for line in lines)]
    if noted:
        date, _, body = max(noted)
        return placement_table(body, f"{where}'s note of {date}")
    decision = [("\n".join(lines)) for heading, lines in sections if heading == "## Decision"]
    if len(decision) != 1:
        raise ValueError(f"{where} has no dated note with a placement table and {len(decision)} Decision sections")
    return placement_table(decision[0], f"{where}'s Decision")


def parity_problems(doc: str, read_adr, texts: dict[str, str], ran, reused=None, suites=SUITES) -> list[str]:
    """Each difference between the doc table, its ADR's table and what the
    workflows run. `ran(kind)` gives the jobs a kind of run runs, by workflow;
    `reused(kind)` the same when the reuse job names a run."""
    try:
        section = placement_section(doc)
        rows = placement_table(section, "docs/releasing.md")
        adr = placement_adr(section)
        adr_rows = adr_placement(read_adr(adr), adr)
    except (OSError, ValueError) as problem:
        return [str(problem)]
    problems = []
    if rows != adr_rows:
        differ = ", ".join(row[0] for row, other in zip(rows, adr_rows) if row != other)
        problems.append(f"the docs/releasing.md table differs from {adr}'s table (rows: {differ or 'count'})")
    events = [row[0] for row in rows]
    if sorted(events) != sorted(EVENT_ROWS):
        problems.append(f"the table's events {events} are not {list(EVENT_ROWS)}")
    jobs = {name: set(parity.workflow_jobs(text)) for name, text in texts.items()}
    mapped = set().union(*suites.values(), *SHORT_JOBS.values(), NOT_A_SUITE)
    for name, names in jobs.items():
        for job in sorted(names - mapped):
            problems.append(f"{WORKFLOW_FILES[name]} job {job!r} is in no suite of the job-to-suite map")
    for suite, members in suites.items():
        if not members & set().union(*jobs.values()):
            problems.append(f"no job serves the suite {suite!r}")
    short_jobs = set().union(*SHORT_JOBS.values())
    for row in rows:
        event = row[0]
        for kind in EVENT_ROWS.get(event, ()):
            started = started_by(kind)
            done = set().union(*(ran(kind)[name] for name in started))
            starts = sorted(WORKFLOW_FILES[name] for name in started if triggered(texts[name], RUNS[kind]))
            for column, cell in zip(COLUMNS[1:], row[1:]):
                where = f"{event} / {column} ({kind}): the table says {cell!r}"
                if column == "Short checks" and cell == "no run" and starts:
                    problems.append(f"{where}, but {starts} start")
                if column == "Short checks":
                    if cell not in SHORT_CELLS:
                        problems.append(f"{where}, which this check cannot read")
                        continue
                    names, serving = SHORT_CELLS[cell]
                    want = set().union(*(SHORT_JOBS[name] for name in names & SHORT_JOBS.keys()))
                    if want != done & short_jobs:
                        problems.append(f"{where}; the workflows run {sorted(done & short_jobs)}")
                    if serving - done:
                        problems.append(f"{where}; the workflows do not run {sorted(serving - done)}")
                    if names & PR_ONLY_CHECKS and RUNS[kind]["event"] != "pull_request":
                        problems.append(f"{where}, but {sorted(names & PR_ONLY_CHECKS)} run on pull requests only")
                    continue
                if cell not in SUITE_CELLS:
                    problems.append(f"{where}, which this check cannot read")
                    continue
                members = suites[column]
                if SUITE_CELLS[cell] == "skips":
                    if members & done:
                        problems.append(f"{where}; the workflows run {sorted(members & done)}")
                    continue
                if members - done:
                    problems.append(f"{where}; the workflows do not run {sorted(members - done)}")
                still_run = set().union(*(reused(kind)[name] for name in started)) if reused is not None else set()
                if SUITE_CELLS[cell] == "reused" and members & still_run:
                    problems.append(f"{where}; with a reused run the workflows still run "
                                    f"{sorted(members & still_run)}")
    return problems


def read_repo(relative: str) -> str:
    return (ROOT / "docs" / relative).read_text(encoding="utf-8")


class DocParityTests(unittest.TestCase):
    """The doc table, its ADR's table, PLACEMENT and the workflows agree."""

    @classmethod
    def setUpClass(cls):
        cls.doc = DOC.read_text(encoding="utf-8")
        if SECTION not in cls.doc.splitlines():
            raise AssertionError(f"docs/releasing.md has no {SECTION!r} section; the check fails closed "
                                 "(the section and ADR-0097 come with PR 631)")
        cls.adr_name = placement_adr(placement_section(cls.doc))
        cls.adr = read_repo(cls.adr_name)
        cls.texts = {name: workflow_text(name) for name in WORKFLOW_FILES}

    def problems(self, doc=None, adr=None, texts=None, suites=SUITES):
        texts = texts or self.texts
        adr_text = self.adr if adr is None else adr
        return parity_problems(doc or self.doc, lambda name: adr_text, texts,
                               lambda kind: ran_by(kind, texts), lambda kind: ran_by(kind, texts, "123"), suites)

    def assertFails(self, problems, *words):
        self.assertTrue(problems, "the check passed")
        self.assertTrue(any(all(word in problem for word in words) for problem in problems),
                        f"no problem names {words}: {problems}")

    def test_the_workflows_match_the_doc_table_and_its_adr(self):
        self.assertEqual(self.problems(), [])

    def test_placement_matches_the_doc_table(self):
        self.assertEqual(parity_problems(self.doc, lambda name: self.adr, self.texts, ran_by_table), [])

    def change_row(self, text, event, column, cell):
        # In an ADR, change the table of its latest note.
        if "\n## Note, 2026-11-11\n" in text:
            head, note = text.split("\n## Note, 2026-11-11\n", 1)
            return head + "\n## Note, 2026-11-11\n" + self.change_row(note, event, column, cell)
        lines = text.splitlines()
        at = next(i for i, line in enumerate(lines) if line.startswith(f"| {event} |"))
        cells = [part.strip() for part in lines[at].strip().strip("|").split("|")]
        cells[COLUMNS.index(column)] = cell
        lines[at] = "| " + " | ".join(cells) + " |"
        return "\n".join(lines) + "\n"

    def test_a_doc_cell_changed_from_no_to_yes_fails(self):
        event, column = "Ready pull request into an integration line", "Windows checks, journeys and partitions"
        doc = self.change_row(self.doc, event, column, "yes")
        adr = self.change_row(self.adr, event, column, "yes")
        self.assertFails(self.problems(doc, adr), event, column, "do not run")

    def test_a_job_condition_widened_to_line_pull_requests_fails(self):
        ci = self.texts["ci"]
        job = parity.workflow_jobs(ci)["windows-build"]
        widened = ci.replace('    branches-ignore: ["integration/**"]\n', "", 1).replace(job, job.replace(
            "(github.event_name == 'schedule'", "(github.event_name == 'pull_request' || github.event_name == 'schedule'",
            1))
        self.assertNotEqual(widened, ci)
        self.assertFails(self.problems(texts={**self.texts, "ci": widened}),
                         "Ready pull request into an integration line", "Windows checks", "windows-build")

    def test_a_job_with_no_map_entry_fails(self):
        extra = ("\n  extra-check:\n    runs-on: ubuntu-24.04\n    timeout-minutes: 5\n"
                 "    steps:\n      - run: \"true\"\n")
        self.assertFails(self.problems(texts={**self.texts, "ci": self.texts["ci"].rstrip("\n") + "\n" + extra}),
                         "extra-check", "no suite")

    def test_a_renamed_column_fails(self):
        self.assertFails(self.problems(self.doc.replace("| Release gates |", "| Release checks |", 1)),
                         "columns")

    def test_an_adr_table_that_differs_from_the_doc_fails(self):
        event = "Daily run, `main`'s tip"
        adr = self.change_row(self.adr, event, "Linux build, lint, tests and coverage", "no")
        self.assertFails(self.problems(adr=adr), "differs from", event)

    def test_the_adr_table_is_its_latest_dated_note_with_a_table(self):
        # The 2026-11-06 table in the Decision is kept as written; the note of
        # 2026-11-11 holds the placement in force.
        with self.assertRaisesRegex(ValueError, "Decision's placement table has the columns"):
            adr_placement(self.adr.split("\n## Note, 2026-11-11\n", 1)[0], "ADR")
        self.assertEqual(adr_placement(self.adr, "ADR"), placement_table(placement_section(self.doc), "doc"))
        # A later note without a table changes nothing; a later note with a
        # table is the one read, wherever it sits in the file.
        plain = self.adr.rstrip("\n") + "\n\n## Note, 2099-01-01\n\nNo placement change.\n"
        self.assertEqual(self.problems(adr=plain), [])
        newer = self.change_row(placement_section(self.doc), "Manual dispatch", "Release gates", "no")
        table = "\n".join(line for line in newer.splitlines() if line.startswith("|"))
        first = self.adr.index("\n## Note, 2026-11-11\n")
        earlier_in_file = self.adr[:first] + f"\n## Note, 2099-01-01\n\n{table}\n" + self.adr[first:]
        self.assertFails(self.problems(adr=earlier_in_file), "differs from", "Manual dispatch")

    def test_a_note_table_that_differs_from_the_doc_fails(self):
        event = "Push to `main`"
        adr = self.change_row(self.adr, event, "Release gates", "yes")
        self.assertFails(self.problems(adr=adr), "differs from", event)

    def test_a_note_table_that_cannot_be_read_fails(self):
        adr = self.adr.replace("| Release gates |", "| Release checks |")
        self.assertFails(self.problems(adr=adr), "note of 2026-11-11", "columns")

    def test_an_adr_without_a_note_falls_back_to_its_decision_table(self):
        table = "\n".join(line for line in placement_section(self.doc).splitlines() if line.startswith("|"))
        adr = f"# ADR\n\n## Context\n\nText.\n\n## Decision\n\n{table}\n\n## Consequences\n\nText.\n"
        self.assertEqual(self.problems(adr=adr), [])
        old = self.adr.split("\n## Note, 2026-11-11\n", 1)[0]
        self.assertFails(self.problems(adr=old), "Decision", "columns")
        self.assertFails(self.problems(adr="# ADR\n\nNo table.\n"), "no dated note", "Decision")

    def test_an_unreadable_cell_fails(self):
        event = "Push to an integration line"
        doc = self.change_row(self.doc, event, "Release gates", "mostly")
        adr = self.change_row(self.adr, event, "Release gates", "mostly")
        self.assertFails(self.problems(doc, adr), event, "cannot read")

    def test_a_suite_no_job_serves_fails(self):
        suites = {**SUITES, "Release gates": frozenset({"gates-release-renamed"})}
        self.assertFails(self.problems(suites=suites), "no job serves", "Release gates")

    def test_a_missing_section_or_adr_link_fails(self):
        self.assertFails(self.problems(self.doc.replace(SECTION, "### Where suites run", 1)), "sections, not one")
        no_link = self.doc.replace(f"]({self.adr_name})", "](decisions/)")
        self.assertFails(self.problems(no_link), "links 0 ADRs")

    def test_a_narrowed_trigger_fails(self):
        ci = self.texts["ci"]
        unscheduled = ci.replace("  schedule:\n", "  repository_dispatch:\n", 1)
        self.assertNotEqual(unscheduled, ci)
        with self.assertRaisesRegex(ValueError, "unsupported trigger event repository_dispatch"):
            triggers(unscheduled)

    def test_an_appended_path_filter_is_refused(self):
        ci = self.texts["ci"]
        self.assertEqual(self.problems(), [])
        for key in ("paths", "paths-ignore"):
            filtered = ci.replace('    branches-ignore: ["integration/**"]\n',
                                  f'    branches-ignore: ["integration/**"]\n    {key}: ["docs/**"]\n', 1)
            self.assertNotEqual(filtered, ci)
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, f"pull_request.*{key}"):
                self.problems(texts={**self.texts, "ci": filtered})

    def test_unmodelled_trigger_restrictions_are_refused(self):
        ci = self.texts["ci"]
        for field in ('tags: [v*]', 'unknown: true', 'branches: main',
                      'types: [closed]', 'types: [opened, synchronize, reopened]'):
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "trigger pull_request"):
                triggers(ci.replace('    branches-ignore: ["integration/**"]', f'    {field}', 1))
        duplicate = ci.replace('    branches-ignore: ["integration/**"]',
                               '    branches-ignore: ["integration/**"]\n    branches-ignore: [main]', 1)
        with self.assertRaisesRegex(ValueError, "duplicate restriction branches-ignore"):
            triggers(duplicate)

    def test_a_line_push_that_starts_a_run_fails(self):
        ci = self.texts["ci"]
        widened = ci.replace("    branches: [main, master]\n", '    branches: [main, master, "integration/**"]\n', 1)
        self.assertNotEqual(widened, ci)
        self.assertFails(self.problems(texts={**self.texts, "ci": widened}),
                         "Push to an integration line", "codeflow-ci.yml", "start")

    def test_a_reused_cell_needs_the_suite_skipped_on_reuse(self):
        # A manual run has no reuse job, so a reused cell there is false.
        event, column = "Manual dispatch", "Release gates"
        doc = self.change_row(self.doc, event, column, "reused when the tree matches, else yes")
        adr = self.change_row(self.adr, event, column, "reused when the tree matches, else yes")
        self.assertFails(self.problems(doc, adr), event, column, "reused run")

class AdmissionTests(unittest.TestCase):
    """AC-14: `commit standards` admits a line pull request on the line verdict
    and the scans when the base line has the line workflow, and only the exact
    sync of main into a line that has not. Anything else keeps the default
    checks of codeflow-ci.yml: while main is relaxed, its secret scan and
    security review. The step runs in a real repository, with a stand-in `gh`
    and `python3` and the real `git`."""

    LINE_FILE = ".github/workflows/codeflow-line-pr.yml"
    NO_GH = 'echo "gh must not be called" >&2\nexit 9\n'

    @classmethod
    def setUpClass(cls):
        text = (WORKFLOWS / "codeflow-policy.yml").read_text(encoding="utf-8")
        job = parity.workflow_jobs(text)["commit-lint"].splitlines()
        at = job.index("      - name: the exact head's ready codeflow-ci run passed")
        start = job.index("        run: |", at) + 1
        script = []
        for line in job[start:]:
            if line.strip() and not line.startswith(" " * 10):
                break
            script.append(line[10:])
        cls.script = "\n".join(script) + "\n"

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.repo = Path(folder.name) / "repo"
        self.bin = Path(folder.name) / "bin"
        self.repo.mkdir()
        self.bin.mkdir()
        self.git("init", "-q", "-b", "main")
        self.write("shared.txt", "one\ntwo\nthree\n")
        self.commit("start")
        self.git("switch", "-q", "-c", "integration/x")
        self.write("line.txt", "line work\n")
        self.line = self.commit("line work")
        self.git("switch", "-q", "main")
        self.write(self.LINE_FILE, "the line workflow\n")
        self.write("main.txt", "main work\n")
        self.main = self.commit("the placement")

    def git(self, *args):
        done = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", *args], cwd=self.repo,
                              capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stderr)
        return done.stdout.strip()

    def write(self, name, text):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)
        return self.git("rev-parse", "HEAD")

    def branch(self, name="sync", start=None):
        self.git("switch", "-q", "-c", name, start or self.line)

    def sync(self):
        """The exact sync: main merged into the line, nothing added."""
        self.branch()
        self.git("merge", "-q", "--no-edit", self.main)
        return self.git("rev-parse", "HEAD")

    def gh(self, pull=None, tip=None, compare="echo ahead"):
        """A stand-in `gh`: the live base, main's tip and their comparison."""
        return (f'case "$*" in\n  *compare/*) {compare} ;;\n  *pulls/7*) {pull or "echo " + self.line} ;;\n'
                f'  *commits/main*) {tip or "echo " + self.main} ;;\n  *) exit 9 ;;\nesac\n')

    def admit(self, head, gh=None, base="integration/x", git=None, ready=True):
        self.git("switch", "-q", "--detach", self.main)
        self.git("update-ref", "refs/codeflow/pr-base", self.line)
        self.git("update-ref", "refs/codeflow/pr-head", head)
        calls = self.bin / "calls"
        calls.unlink(missing_ok=True)
        tools = {"gh": gh or self.gh(), "python3": 'printf "%s\\n" "$@" > "$CALLS"\n'}
        real = subprocess.run(["which", "git"], capture_output=True, text=True).stdout.strip()
        if git:
            tools["git"] = git.replace("REAL", real)
        for name, body in tools.items():
            tool = self.bin / name
            tool.write_text("#!/bin/sh\n" + body, encoding="utf-8")
            tool.chmod(0o755)
        env = {"PATH": f"{self.bin}:{Path(real).parent}:/usr/bin:/bin", "CALLS": str(calls), "REPOSITORY": "o/r",
               "HEAD_SHA": head, "PR_NUMBER": "7", "DRAFT": "false" if ready else "true", "BASE_REF": base,
               "BASE_SHA": self.line, "DEFAULT_BRANCH": "main", "GITHUB_SHA": self.main}
        done = subprocess.run(["/bin/sh", "-eu", "-c", self.script], env=env, capture_output=True, text=True,
                              cwd=self.repo)
        (self.bin / "git").unlink(missing_ok=True)
        return done.returncode, done.stdout + done.stderr, (calls.read_text(encoding="utf-8").splitlines()
                                                            if calls.exists() else None)

    def assertDefault(self, result):
        code, out, argv = result
        self.assertEqual(code, 0, out)
        self.assertEqual(argv[argv.index("--workflow") + 1], ".github/workflows/codeflow-ci.yml")
        self.assertEqual([argv[at + 1] for at, word in enumerate(argv) if word == "--require"],
                         ["secret scan", "security review"])

    def assertLine(self, result):
        code, out, argv = result
        self.assertEqual(code, 0, out)
        self.assertEqual(argv[argv.index("--workflow") + 1], self.LINE_FILE)
        self.assertEqual([argv[at + 1] for at, word in enumerate(argv) if word == "--require"],
                         ["codeflow gates (line)", "secret scan", "security review"])

    def assertRefused(self, result):
        code, out, argv = result
        self.assertNotEqual(code, 0, out)
        self.assertIsNone(argv, "the step went on to read a verdict")
        self.assertIn("line workflow", out)

    def test_a_pull_request_into_main_needs_the_default_checks(self):
        head = self.sync()
        for base in ("main", "master", "feature/x", "integration/", "integration", ""):
            with self.subTest(base=base):
                self.assertDefault(self.admit(head, 'echo "gh must not be called" >&2\nexit 9\n', base=base))

    def test_a_real_sync_needs_the_line_verdict(self):
        self.assertLine(self.admit(self.sync()))

    def test_a_sync_of_an_ancestor_of_main_s_tip_needs_the_line_verdict(self):
        self.assertLine(self.admit(self.sync(), self.gh(tip=f"echo {'c' * 40}")))

    def test_a_main_commit_that_is_not_on_main_keeps_the_default_checks(self):
        for relation in ("diverged", "behind", "identical", "true"):
            with self.subTest(relation=relation):
                self.assertDefault(self.admit(self.sync() if relation == "diverged" else self.git("rev-parse", "sync"),
                                           self.gh(tip=f"echo {'c' * 40}", compare=f"echo {relation}")))

    def test_a_sync_plus_one_commit_keeps_the_default_checks(self):
        self.sync()
        self.write("extra.txt", "added\n")
        self.assertDefault(self.admit(self.commit("one more")))

    def test_a_task_branch_with_main_merged_in_keeps_the_default_checks(self):
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.commit("task work")
        self.git("merge", "-q", "--no-edit", self.main)
        self.assertDefault(self.admit(self.git("rev-parse", "HEAD")))

    def test_a_hand_resolved_sync_keeps_the_default_checks(self):
        self.git("switch", "-q", "integration/x")
        self.write("shared.txt", "one\nLINE\nthree\n")
        self.line = self.commit("line side")
        self.git("switch", "-q", "main")
        self.write("shared.txt", "one\nMAIN\nthree\n")
        self.main = self.commit("main side")
        self.branch()
        merged = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "merge", "-q", self.main],
                                cwd=self.repo, capture_output=True, text=True)
        self.assertNotEqual(merged.returncode, 0)
        self.write("shared.txt", "one\nBOTH\nthree\n")
        self.assertDefault(self.admit(self.commit("resolved")))

    def test_an_ordinary_pull_request_into_an_unsynced_line_keeps_the_default_checks(self):
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertDefault(self.admit(self.commit("task work")))

    def test_a_head_that_adds_main_s_file_without_main_keeps_the_default_checks(self):
        self.branch("task")
        self.write(self.LINE_FILE, "the line workflow\n")
        self.assertDefault(self.admit(self.commit("copy the file")))

    def test_an_adopter_with_no_line_workflow_on_main_keeps_the_default_checks(self):
        head = self.sync()
        self.git("switch", "-q", "main")
        self.git("rm", "-q", self.LINE_FILE)
        self.main = self.commit("no file")
        self.assertDefault(self.admit(head))

    def test_a_read_or_merge_that_fails_keeps_the_default_checks(self):
        head = self.sync()
        stale = f"echo {'d' * 40}"
        failed = 'echo "gh: Server Error (HTTP 502)" >&2; exit 1'
        for label, gh in (("a moved base", self.gh(pull=stale)), ("the base read", self.gh(pull=failed)),
                          ("the tip read", self.gh(tip=failed)),
                          ("the comparison", self.gh(tip=f"echo {'c' * 40}", compare=failed))):
            with self.subTest(label):
                self.assertDefault(self.admit(head, gh))
        broken = 'if [ "$1" = merge-tree ]; then exit 2; fi\nexec REAL "$@"\n'
        self.assertDefault(self.admit(head, git=broken))

    def place_line(self, text="the line workflow\n"):
        """The line has synced: it holds main's line workflow."""
        self.git("switch", "-q", "integration/x")
        self.write(self.LINE_FILE, text)
        self.line = self.commit("synced")

    def test_once_the_line_has_the_placement_the_exception_is_off(self):
        # The base commit decides: a pull request is on the line verdict with
        # no call to GitHub, whatever other files the head changes.
        self.place_line()
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertLine(self.admit(self.commit("task work"), self.NO_GH))

    def test_a_head_that_changes_a_line_workflow_is_refused(self):
        # `pull_request` runs the workflow from the head's merge, so a head that
        # edits it could post its own green verdict. It waits for no run.
        name = self.LINE_FILE
        for label, change in (("edited", lambda n: self.write(n, "steps: []\n")),
                              ("deleted", lambda n: self.git("rm", "-q", n))):
            with self.subTest(change=label):
                self.place_line()
                self.branch(f"task-{label}")
                change(name)
                self.assertRefused(self.admit(self.commit("weaken the proof"), self.NO_GH))
                self.git("switch", "-q", "main")
                self.git("branch", "-q", "-f", "integration/x", self.line + "~1")

    def main_moves(self, text="a newer line workflow\n"):
        """Main changes the line workflow after the line synced."""
        self.git("switch", "-q", "main")
        self.write(self.LINE_FILE, text)
        self.main = self.commit("main changes the line workflows")

    def test_a_line_with_an_older_main_copy_keeps_the_line_verdict(self):
        # The base holds main's earlier copies: an ancestor of main's commit
        # held them, so the line keeps its placement until it syncs again.
        self.place_line()
        self.main_moves()
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertLine(self.admit(self.commit("task work"), self.NO_GH))

    def test_a_line_s_own_line_workflow_never_chooses_the_line_verdict(self):
        # A line that landed its own copy, which main never held, is judged on
        # the default checks of codeflow-ci.yml, as if it had none. Before
        # TSK-1321's fix the base file's presence alone chose the line verdict.
        self.git("switch", "-q", "integration/x")
        self.write(self.LINE_FILE, "a line-made workflow\n")
        self.line = self.commit("install its own")
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertDefault(self.admit(self.commit("task work"), self.NO_GH))

    def test_a_history_read_that_fails_keeps_the_default_checks(self):
        self.place_line()
        self.branch("task")
        self.write("task.txt", "task work\n")
        broken = 'if [ "$1" = rev-list ]; then exit 2; fi\nexec REAL "$@"\n'
        self.assertDefault(self.admit(self.commit("task work"), self.NO_GH, git=broken))

    def test_a_head_may_bring_main_s_reviewed_line_workflows(self):
        self.place_line()
        self.main_moves()
        self.branch("task")
        self.write(self.LINE_FILE, "a newer line workflow\n")
        self.assertLine(self.admit(self.commit("take main's copy"), self.NO_GH))

    def test_a_head_that_matches_neither_base_nor_main_is_refused(self):
        self.place_line()
        self.main_moves()
        self.branch("task")
        self.write(self.LINE_FILE, "a third line workflow\n")
        self.assertRefused(self.admit(self.commit("not reviewed"), self.NO_GH))

    def resolve(self, text):
        """Finish a merge that conflicted on the line workflow."""
        self.write(self.LINE_FILE, text)
        return self.commit("resolved")

    def test_a_blob_only_a_merged_side_branch_held_keeps_the_default_checks(self):
        # Main merged a side branch whose copy it never held itself. Only
        # main's first-parent history counts, so a line that installs the side
        # branch's copy is judged on the default checks.
        self.git("switch", "-q", "-c", "side", self.main)
        self.write(self.LINE_FILE, "a side-branch workflow\n")
        side = self.commit("side copies")
        self.git("switch", "-q", "main")
        self.main_moves("main's next workflow\n")
        merged = subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.com", "merge", "-q", "side"],
                                cwd=self.repo, capture_output=True, text=True)
        self.assertNotEqual(merged.returncode, 0)
        self.main = self.resolve("the merged workflow\n")
        self.assertIn(side, self.git("rev-list", self.main, "--", self.LINE_FILE).split())
        self.assertNotIn(side, self.git("rev-list", "--first-parent", self.main, "--", self.LINE_FILE).split())
        self.place_line("a side-branch workflow\n")
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertDefault(self.admit(self.commit("task work"), self.NO_GH))

    def test_an_exact_sync_that_keeps_a_line_made_workflow_is_refused(self):
        # The line once held main's copies, then changed them. Syncing main,
        # which did not change them, merges clean and keeps the line's copies.
        # The exact sync is judged on the line verdict only when its copies are
        # main's.
        self.git("switch", "-q", "integration/x")
        self.git("merge", "-q", "--no-edit", self.main)
        self.place_line("a line-made workflow\n")
        self.git("switch", "-q", "main")
        self.write("more.txt", "more main work\n")
        self.main = self.commit("more main work")
        head = self.sync()
        self.assertEqual(self.git("rev-parse", f"{head}:{self.LINE_FILE}"),
                         self.git("rev-parse", f"{self.line}:{self.LINE_FILE}"))
        self.assertRefused(self.admit(head))

    def test_a_clean_merge_of_two_reviewed_copies_into_a_third_is_refused(self):
        # `pull_request` runs the merge of base and head. The base holds one of
        # main's copies and the head another, from an older line commit, so the
        # merge is a copy main never held.
        first, top, bottom = ("a\nb\nc\nd\ne\nf\ng\n", "A\nb\nc\nd\ne\nf\ng\n", "a\nb\nc\nd\ne\nf\nG\n")
        for text in (first, top, bottom):
            self.main_moves(text)
        self.place_line(first)
        synced = self.line
        self.place_line(top)
        self.branch("task", synced)
        self.write(self.LINE_FILE, bottom)
        head = self.commit("take main's newest copies")
        tree = self.git("merge-tree", "--write-tree", self.line, head)
        self.assertEqual(self.git("cat-file", "-p", f"{tree}:{self.LINE_FILE}"), "A\nb\nc\nd\ne\nf\nG")
        self.assertRefused(self.admit(head, self.NO_GH))

    def test_a_blob_read_that_fails_is_refused(self):
        self.place_line()
        self.branch("task")
        self.write("task.txt", "task work\n")
        broken = 'case "$*" in rev-parse*refs/codeflow/pr-head:*) exit 2 ;; esac\nexec REAL "$@"\n'
        self.assertRefused(self.admit(self.commit("task work"), self.NO_GH, git=broken))

    def test_admission_judges_once_against_the_judged_base(self):
        # TSK-1239: the step never waits; policy-admission.py rechecks the
        # pull request's base against the one judged here.
        for head, gh in ((self.sync(), None), (self.git("rev-parse", "sync"), self.NO_GH)):
            code, out, argv = self.admit(head, gh) if gh is None else self.admit(head, gh, base="main")
            self.assertEqual(code, 0, out)
            self.assertEqual(argv[:2], ["-I", "scripts/policy-admission.py"])
            self.assertEqual(argv[argv.index("--base") + 1], "integration/x" if gh is None else "main")
            self.assertNotIn("--wait", argv)


class CancelledExpression(Expression):
    """The same evaluator, in a run that has been cancelled."""

    def call(self, name, args):
        return True if name == "cancelled" and not args else super().call(name, args)


class CancelledRunTests(unittest.TestCase):
    """A cancelled PR starts no always-running job that could hold its group.
    Main's scheduled reports keep their existing always() behavior."""

    READY = {"github.event_name": "pull_request", "github.event.pull_request.draft": False}
    SCHEDULED = {"github.event_name": "schedule", "github.event.pull_request.draft": None}
    VERDICTS = (("codeflow-ci.yml", "gates-verdict"), ("codeflow-ci.yml", "windows-verdict"),
                ("codeflow-line-pr.yml", "gates-line-verdict"))
    # While main is relaxed (operator decision, 2026-11-11), the two verdicts
    # of codeflow-ci.yml run on the schedule and a manual run alone, under
    # `always()`. Those runs are never cancelled by a newer one, since only a
    # pull request run cancels in progress, so no new head waits on them. The
    # `!cancelled()` form returns with main's checks.
    RELAXED = "${{ always() && (github.event_name == 'schedule' || github.event_name == 'workflow_dispatch') }}"

    @staticmethod
    def conditions(file: str) -> dict[str, str]:
        found, job = {}, None
        for line in (WORKFLOWS / file).read_text(encoding="utf-8").split("\njobs:\n", 1)[1].splitlines():
            if name := re.fullmatch(r"  ([a-z0-9-]+):", line):
                job = name.group(1)
            elif condition := re.fullmatch(r"    if: (.+)", line):
                found[job] = condition.group(1)
        return found

    def check_cancelled_pr(self, file, conditions=None):
        jobs = parity.workflow_jobs((WORKFLOWS / file).read_text(encoding="utf-8"))
        for job, condition in (self.conditions(file) if conditions is None else conditions).items():
            if file == "codeflow-ci.yml" and "always()" in condition:
                direct = job_fields(jobs[job])[1]
                needs = {name: {"result": "success", "outputs": {}} for name in direct}
                self.assertFalse(truthy(CancelledExpression(condition, self.READY, needs, direct).evaluate()),
                                 f"{file} {job} starts on a cancelled pull request")
            else:
                self.assertNotIn("always()", condition)

    def test_scheduled_reports_are_allowed_but_widening_them_to_prs_is_refused(self):
        conditions = self.conditions("codeflow-ci.yml")
        self.check_cancelled_pr("codeflow-ci.yml", conditions)
        for job in ("gates-coverage-report", "windows-journeys"):
            condition = conditions[job]
            self.assertIn("always()", condition)
            widened = condition.replace("github.event_name == 'schedule'", "github.event_name == 'pull_request'")
            self.assertNotEqual(condition, widened)
            with self.subTest(job=job), self.assertRaises(AssertionError):
                self.check_cancelled_pr("codeflow-ci.yml", {job: widened})

    def test_no_job_of_a_workflow_that_cancels_starts_after_the_cancel(self):
        for file in ("codeflow-ci.yml", "codeflow-line-pr.yml"):
            self.check_cancelled_pr(file)
        self.assertIn("  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n",
                      (WORKFLOWS / "codeflow-ci.yml").read_text(encoding="utf-8"))

    def test_a_cancelled_run_posts_no_verdict_and_a_failed_job_still_reaches_it(self):
        for file, job in self.VERDICTS:
            condition = self.conditions(file)[job]
            with self.subTest(file=file, job=job):
                self.assertFalse(truthy(CancelledExpression(condition, self.READY, {}, []).evaluate()))
                where = self.SCHEDULED if condition == self.RELAXED else self.READY
                self.assertTrue(truthy(Expression(condition, where, {}, []).evaluate()))


class OtherWorkflowTests(unittest.TestCase):
    """The short checks a line pull request keeps outside codeflow-ci.yml."""

    def test_commit_standards_and_release_impact_run_on_every_pull_request(self):
        for name, ids in OTHER_SHORT_CHECKS.items():
            jobs = parity.workflow_jobs((WORKFLOWS / name).read_text(encoding="utf-8"))
            for job in ids:
                with self.subTest(workflow=name, job=job):
                    self.assertIn(job, jobs)
                    self.assertIn(job_fields(jobs[job])[0], (None, "github.event_name == 'pull_request'"),
                                  f"{name} {job} must run on every pull request")


# The codeflow-ci.yml jobs that scripts/gate-parity.py holds equal to the line
# workflow's: the copied jobs and provenance.
PAIRED_JOBS = ("secret-scan", "gitleaks-canary", "security-review", "provenance")
CHECKOUT_USES = re.compile(r"^\s*(?:- )?uses:\s*actions/checkout@")
# One plain `key: value` line of a checkout step: no quoted key, no merge key.
STEP_LINE = re.compile(r"([A-Za-z0-9_][A-Za-z0-9_.-]*):(?: +(.*))?")
# The first characters of a value the reader does not consume completely: a
# quoted scalar (it runs on over later lines until its closing quote), a block
# scalar, a flow mapping or sequence, an anchor, an alias, a tag, a reserved
# indicator, or another indicator that starts a plain value.
UNREAD_VALUE = ("'", '"', "|", ">", "{", "[", "&", "*", "!", "%", "@", "`", ",", "?", ":", "-")


def value_unread(value: str) -> bool:
    """True unless the value, its comment removed, is empty or one plain
    scalar the reader takes in full from this one line. A plain scalar that
    goes on over later lines needs a deeper line, which the indent rule in
    checkout_step_problem refuses."""
    return value.startswith(UNREAD_VALUE) or ": " in value or value.endswith(":")


def checkout_step_problem(lines: list[str], index: int) -> str | None:
    """The problem with the checkout step holding lines[index], or None when
    the step sets `persist-credentials: false` in its own `with:` mapping.

    It reads only the subset of YAML the workflows use for a checkout step:
    the step's `- ` line, step keys two columns in, and the `with:` inputs two
    columns further, each a plain `key: value` whose value is empty or a plain
    scalar on that line. Anything else in the step (a quoted, block or other
    scalar it cannot take in full, a flow mapping, an anchor, an alias, a tag,
    a tab, a key it cannot parse, a repeated key or another indent) is
    reported as unreadable, so a form it cannot read never passes."""
    def indent(line: str) -> int:
        return len(line) - len(line.lstrip(" "))

    start = index
    while start >= 0 and not lines[start].lstrip(" ").startswith("- "):
        start -= 1
    if start < 0:
        return "a checkout sits outside any step"
    dash = indent(lines[start])
    end = start + 1
    while end < len(lines) and (parity.blank_or_comment(lines[end]) or indent(lines[end]) > dash):
        end += 1
    if index >= end:
        return "a checkout sits outside any step"
    keys: dict[str, tuple[str, list[tuple[str, str]]]] = {}
    current = None
    for number in range(start, end):
        raw = lines[number]
        if "\t" in raw:
            return "a checkout step holds a tab; the reader cannot read it"
        if parity.blank_or_comment(raw):
            continue
        column, body = indent(raw), raw.strip()
        if number == start:
            body = body[2:]
            column = dash + 2
            if body.startswith(" "):
                return f"a checkout step line the reader cannot read ({raw.strip()!r})"
        match = STEP_LINE.fullmatch(body)
        if match is None:
            return f"a checkout step line the reader cannot read ({raw.strip()!r}); write it as a plain `key: value`"
        key = match.group(1)
        value = re.sub(r"(?:^|\s+)#.*$", "", match.group(2) or "").strip()
        if value_unread(value):
            return (f"a checkout step value the reader cannot read in full ({raw.strip()!r}): a quoted, block "
                    "or flow value, an anchor, alias or tag, or an indicator; write it as a plain one-line value")
        if column == dash + 2:
            if key in keys:
                return f"a checkout step repeats the key `{key}`"
            keys[key] = (value, [])
            current = key
        elif column == dash + 4 and current is not None and keys[current][0] == "":
            children = keys[current][1]
            if any(child == key for child, _ in children):
                return f"a checkout step repeats `{key}` under `{current}:`"
            children.append((key, value))
        else:
            return f"a checkout step line at an indent the reader does not accept ({raw.strip()!r})"
    uses = keys.get("uses")
    if uses is None or not uses[0].startswith("actions/checkout@"):
        return "the checkout's `uses:` is not a key of its own step"
    inputs = keys.get("with")
    if inputs is None or inputs[0] != "":
        return "a checkout keeps the job token: it has no `with:` mapping; set `persist-credentials: false` there"
    if ("persist-credentials", "false") not in inputs[1]:
        return "a checkout keeps the job token; set `persist-credentials: false` under its `with:`"
    return None


def checkout_problems(where: str, jobs: dict[str, str]) -> tuple[int, list[str]]:
    """(checkouts read, problems): each checkout step in these jobs must set
    the action input `persist-credentials: false` in the step's own `with:`
    mapping, so no job token stays in the checkout for the repository code a
    later step runs. The same key under `env:`, in another step or at another
    indent does not set the input and does not count."""
    count, problems = 0, []
    for name, text in jobs.items():
        lines = text.splitlines()
        for index, line in enumerate(lines):
            if not CHECKOUT_USES.match(line):
                continue
            count += 1
            problem = checkout_step_problem(lines, index)
            if problem is not None:
                problems.append(f"{where} `{name}`: {problem}")
    return count, problems


class CheckoutCredentialTests(unittest.TestCase):
    """A line pull request runs its own code (the canary runs
    scripts/check-gitleaks-allowlist.sh from the checkout), so no checkout of
    the line workflow, or of a codeflow-ci.yml job held equal to it, leaves
    the job token behind, as the adopter template's checkouts do not."""

    def test_no_line_checkout_or_paired_ci_checkout_keeps_the_token(self):
        line = workflow_text("line")
        count, problems = checkout_problems("codeflow-line-pr.yml", parity.workflow_jobs(line))
        self.assertEqual(count, line.count("actions/checkout@"), "a checkout the step reader did not see")
        self.assertGreaterEqual(count, len(PAIRED_JOBS))
        jobs = parity.workflow_jobs(WORKFLOW)
        paired = {name: jobs[name] for name in PAIRED_JOBS}
        ci_count, ci_problems = checkout_problems("codeflow-ci.yml", paired)
        self.assertEqual(ci_count, sum(text.count("actions/checkout@") for text in paired.values()))
        self.assertGreaterEqual(ci_count, len(PAIRED_JOBS))
        self.assertEqual(problems + ci_problems, [])

    def test_a_checkout_that_keeps_the_token_is_caught(self):
        job = ("    steps:\n"
               "      - uses: actions/checkout@abc # v6\n"
               "        with:\n"
               "          fetch-depth: 0\n"
               "      - name: second\n"
               "        uses: actions/checkout@abc # v6\n"
               "      - run: sh scripts/check-gitleaks-allowlist.sh\n"
               "        env:\n"
               "          persist-credentials: false\n")
        count, problems = checkout_problems("fixture", {"canary": job})
        self.assertEqual(count, 2)
        self.assertEqual(len(problems), 2, problems)
        fixed = job.replace("          fetch-depth: 0\n", "          fetch-depth: 0\n          persist-credentials: false\n")
        fixed = fixed.replace("        uses: actions/checkout@abc # v6\n",
                              "        uses: actions/checkout@abc # v6\n        with:\n"
                              "          persist-credentials: false # no token left\n")
        self.assertEqual(checkout_problems("fixture", {"canary": fixed}), (2, []))

    def test_the_key_under_the_same_steps_env_is_caught(self):
        # `env:` sets an environment variable, not the checkout input, so the
        # job token stays even though the step holds the key.
        job = ("    steps:\n"
               "      - uses: actions/checkout@abc # v6\n"
               "        with:\n"
               "          fetch-depth: 0\n"
               "        env:\n"
               "          persist-credentials: false\n"
               "      - run: sh scripts/check-gitleaks-allowlist.sh\n")
        count, problems = checkout_problems("fixture", {"canary": job})
        self.assertEqual(count, 1)
        self.assertEqual(len(problems), 1, problems)
        self.assertIn("under its `with:`", problems[0])
        moved = job.replace("        env:\n          persist-credentials: false\n",
                            "        env:\n          FOO: bar\n")
        moved = moved.replace("          fetch-depth: 0\n", "          fetch-depth: 0\n          persist-credentials: false\n")
        self.assertEqual(checkout_problems("fixture", {"canary": moved}), (1, []))

    def test_the_key_beside_or_below_with_is_caught(self):
        for wrong in ("        with:\n          fetch-depth: 0\n        persist-credentials: false\n",
                      "        with:\n          fetch-depth: 0\n            persist-credentials: false\n",
                      "        with:\n          persist-credentials: true\n",
                      "        with:\n          persist-credentials: 'false'\n",
                      "        with: persist-credentials\n"):
            job = "    steps:\n      - uses: actions/checkout@abc # v6\n" + wrong
            count, problems = checkout_problems("fixture", {"canary": job})
            self.assertEqual((count, len(problems)), (1, 1), (wrong, problems))

    def test_a_checkout_step_the_reader_cannot_read_is_refused(self):
        for form in ("        with: |\n          persist-credentials: false\n",
                     "        with: {persist-credentials: false}\n",
                     "        with: &inputs\n          persist-credentials: false\n",
                     "        with: *inputs\n",
                     "        with: !!map\n          persist-credentials: false\n",
                     "        with:\n          persist-credentials: false\n          <<: *inputs\n",
                     "        with:\n          \"persist-credentials\": false\n",
                     "        with:\n          persist-credentials: false\n          persist-credentials: true\n",
                     "        with:\n          persist-credentials: false\n        with:\n          fetch-depth: 0\n",
                     "        with:\n\t  persist-credentials: false\n"):
            job = "    steps:\n      - uses: actions/checkout@abc # v6\n" + form
            count, problems = checkout_problems("fixture", {"canary": job})
            self.assertEqual((count, len(problems)), (1, 1), (form, problems))
        good = "    steps:\n      - uses: actions/checkout@abc # v6\n        with:\n          persist-credentials: false\n"
        self.assertEqual(checkout_problems("fixture", {"canary": good}), (1, []))

    def test_a_quoted_value_over_several_lines_is_refused(self):
        # A quoted scalar runs on to its closing quote, so the lines inside it
        # are its text, not keys: YAML gives this step only `uses` and a
        # multiline `name`, and the apparent `with:` sets no input.
        for quote in ('"', "'"):
            shapes = (("      - uses: actions/checkout@abc\n"
                       f"        name: {quote}example\n"
                       "        with:\n"
                       "          persist-credentials: false\n"
                       f"        env: end{quote}\n"),
                      ("      - name: {q}example\n"
                       "        uses: actions/checkout@abc\n"
                       "        with:\n"
                       "          persist-credentials: false\n"
                       "        env: end{q}\n").format(q=quote),
                      ("      - uses: actions/checkout@abc\n"
                       "        with:\n"
                       f"          fetch-depth: {quote}0\n"
                       "          persist-credentials: false\n"
                       f"          ref: main{quote}\n"))
            for shape in shapes:
                job = "    steps:\n" + shape
                count, problems = checkout_problems("fixture", {"canary": job})
                self.assertEqual((count, len(problems)), (1, 1), (shape, problems))
                self.assertIn("cannot read", problems[0])

    def test_a_value_the_reader_cannot_take_in_full_is_refused(self):
        for form in ("        name: a: b\n",
                     "        name: @example\n",
                     "        name: `example`\n",
                     "        name: %example\n",
                     "        name: ? example\n",
                     "        name: - example\n",
                     "        name: example:\n",
                     "        name: example\n          continued\n"):
            job = ("    steps:\n      - uses: actions/checkout@abc # v6\n" + form
                   + "        with:\n          persist-credentials: false\n")
            count, problems = checkout_problems("fixture", {"canary": job})
            self.assertEqual((count, len(problems)), (1, 1), (form, problems))
        good = ("    steps:\n      - name: Check out the head # v6\n        uses: actions/checkout@abc # v6\n"
                "        with:\n          ref: ${{ github.event.pull_request.head.sha }}\n"
                "          fetch-depth: 0\n          persist-credentials: false # no token left\n")
        self.assertEqual(checkout_problems("fixture", {"canary": good}), (1, []))


class TriggerRestrictionTests(unittest.TestCase):
    def test_path_filters_are_refused_instead_of_ignored(self):
        control = "on:\n  pull_request:\n    types: [opened, synchronize, reopened, ready_for_review]\npermissions: {}\n"
        self.assertIn("pull_request", triggers(control))
        for field in ("paths", "paths-ignore"):
            narrowed = control.replace("permissions:", f"    {field}: ['docs/**']\npermissions:")
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, "unsupported"):
                triggers(narrowed)


class EvaluatorTests(unittest.TestCase):
    """The evaluator refuses what it does not know instead of guessing."""

    def evaluate(self, text, context=None):
        return Expression(text, context or {"github.event_name": "push"}, {}, []).evaluate()

    def test_known_forms(self):
        self.assertTrue(self.evaluate("github.event_name == 'PUSH'"))
        self.assertTrue(self.evaluate("contains(fromJSON('[\"main\",\"master\"]'), 'Main')"))
        self.assertFalse(self.evaluate("!always() || cancelled()"))

    def test_unknown_forms_are_errors(self):
        for text in ("github.actor == 'x'", "endsWith('a', 'a')", "needs.reuse.result == 'success'", "1 < 2"):
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    self.evaluate(text)


if __name__ == "__main__":
    unittest.main(verbosity=1)
