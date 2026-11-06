#!/usr/bin/env python3
"""Where each job of .github/workflows/codeflow-ci.yml, of the line workflows
(codeflow-line-pr.yml, codeflow-line-push.yml) and of the daily run
(windows-daily-run.yml) runs (TSK-1321).

Suite placement is an operator decision (2026-11-05, "A plus Linux off line
PRs", and 2026-11-06 15:34 for line pushes). The placement ADR (ADR-0097, "CI suite placement by event") and the
maintainer page (docs/releasing.md, "Where CI suites run") each hold it as one
table, by event and suite. These tests read every workflow's triggers and
every job's `if:` and `needs:` from the workflows themselves and evaluate them
as GitHub does for each kind of run. They fail when:

- the doc table differs from the table of the ADR the section links;
- a cell of the doc table differs from what the workflows run for that event
  and suite, in either direction, mapped through SUITES, the one job-to-suite
  map;
- PLACEMENT below, one row per job and one column per kind of run, differs
  from the workflows or from the doc table;
- a table, row, column or cell cannot be read, a job is in no suite, or a
  suite has no job (fail closed);
- a pull request into an integration line starts a job named for a required
  check of main, skipped or not, or a line push leaves a `windows` row, since
  GitHub counts a skipped required check as a success.

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
# to a line or to main; the weekly schedule; a manual run; the daily run.
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
    "weekly": {"event": "schedule", "base": "", "ref": "refs/heads/main"},
    "manual": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/main"},
    "daily": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/integration/EPC-022-the-3-2-0-milestone-fixes",
              "workflow": "windows-daily-run.yml"},
}

# The placement table. `yes` runs, `-` does not. A pull request into an
# integration line starts only the line workflow, which runs the short
# checks: the Linux build and gate parts, the secret scan and the security
# review (its commit standards and release impact run in codeflow-policy.yml
# and codeflow-release.yml). A draft runs only the secret scan and security
# review. A line push starts only the line push workflow: full Linux and no
# Windows. Every Windows job runs on pull requests into main, pushes to main,
# the weekly and manual runs and the daily run. The two required verdicts never
# run on a line pull request, so they cannot pass there with their suites not
# selected. The daily column is the daily run, on main and each
# active line at the commit the dispatcher resolved: the Linux build and gate
# parts, the Linux coverage shards with their report and every Windows job,
# which is never merge evidence. Each row names its jobs' workflow: ci for
# codeflow-ci.yml, line and push for the line workflows, daily for the daily
# runner.
PLACEMENT = """\
job                       workflow  line PR  draft line PR  main PR  draft main PR  other PR  line push  main push  weekly  manual  daily
ubuntu-image              ci        -        -              -        -              -         -          yes        -       -       -
windows-image             ci        -        -              -        -              -         -          yes        -       -       -
reuse                     ci        -        -              -        -              -         -          yes        -       -       -
provenance                ci        -        -              yes      yes            yes       -          -          -       -       -
gates-build               ci        -        -              yes      -              -         -          yes        yes     yes     -
gates                     ci        -        -              yes      -              -         -          yes        yes     yes     -
gates-release             ci        -        -              yes      -              -         -          yes        yes     yes     -
gates-coverage            ci        -        -              yes      -              -         -          yes        yes     yes     -
gates-coverage-report     ci        -        -              yes      -              -         -          yes        yes     yes     -
gates-verdict             ci        -        -              yes      -              yes       -          yes        yes     yes     -
windows-build             ci        -        -              yes      -              -         -          yes        yes     yes     -
windows-tests             ci        -        -              yes      -              -         -          yes        yes     yes     -
windows                   ci        -        -              yes      -              -         -          yes        yes     yes     -
windows-release-journey   ci        -        -              yes      -              -         -          yes        yes     yes     -
windows-journeys          ci        -        -              yes      -              -         -          yes        yes     yes     -
windows-verdict           ci        -        -              yes      -              yes       -          yes        yes     yes     -
secret-scan               ci        -        -              yes      yes            yes       -          yes        yes     yes     -
gitleaks-canary           ci        -        -              yes      yes            yes       -          yes        yes     yes     -
security-review           ci        -        -              yes      yes            yes       -          yes        yes     yes     -
gates-build               line      yes      -              -        -              -         -          -          -       -       -
gates                     line      yes      -              -        -              -         -          -          -       -       -
gates-line-verdict        line      yes      -              -        -              -         -          -          -       -       -
secret-scan               line      yes      yes            -        -              -         -          -          -       -       -
gitleaks-canary           line      yes      yes            -        -              -         -          -          -       -       -
security-review           line      yes      yes            -        -              -         -          -          -       -       -
gates-build               push      -        -              -        -              -         yes        -          -       -       -
gates                     push      -        -              -        -              -         yes        -          -       -       -
gates-coverage            push      -        -              -        -              -         yes        -          -       -       -
gates-coverage-report     push      -        -              -        -              -         yes        -          -       -       -
gates-verdict             push      -        -              -        -              -         yes        -          -       -       -
secret-scan               push      -        -              -        -              -         yes        -          -       -       -
gitleaks-canary           push      -        -              -        -              -         yes        -          -       -       -
security-review           push      -        -              -        -              -         yes        -          -       -       -
gates-build               daily     -        -              -        -              -         -          -          -       -       yes
gates                     daily     -        -              -        -              -         -          -          -       -       yes
gates-coverage            daily     -        -              -        -              -         -          -          -       -       yes
gates-coverage-report     daily     -        -              -        -              -         -          -          -       -       yes
windows-build             daily     -        -              -        -              -         -          -          -       -       yes
windows-tests             daily     -        -              -        -              -         -          -          -       -       yes
windows                   daily     -        -              -        -              -         -          -          -       -       yes
windows-release-journey   daily     -        -              -        -              -         -          -          -       -       yes
windows-journeys          daily     -        -              -        -              -         -          -          -       -       yes
"""
WORKFLOW_FILES = {"ci": "codeflow-ci.yml", "line": "codeflow-line-pr.yml", "push": "codeflow-line-push.yml",
                  "daily": "windows-daily-run.yml"}

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
    runner, any other ci and the line workflows, by their triggers."""
    return {"daily"} if RUNS[kind].get("workflow") == WORKFLOW_FILES["daily"] else {"ci", "line", "push"}


DISPATCHER = WORKFLOWS / "windows-daily.yml"


def triggers(text: str) -> dict[str, dict[str, list[str]]]:
    """A workflow's events, each with its `branches` and `branches-ignore`."""
    block = re.search(r"(?ms)^on:\n(.*?)(?=^[^\s#])", text)
    if not block:
        raise ValueError("a workflow has no readable `on:` block")
    found = {}
    for event in re.finditer(r"(?m)^  ([a-z_]+):[^\n]*\n((?:(?:    .*)?\n)*)", block.group(1)):
        filters = {}
        for key in ("branches", "branches-ignore"):
            value = re.search(rf"(?m)^    {key}: \[(.*)\]$", event.group(2))
            if value:
                filters[key] = [item.strip().strip("\"'") for item in value.group(1).split(",")]
        found[event.group(1)] = filters
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

    def test_the_daily_run_runs_the_full_suites_without_the_short_checks_or_release_gates(self):
        weekly, daily = ran_by("weekly")["ci"], ran_by("daily")["daily"]
        # The image, reuse and verdict jobs serve a run itself and test nothing.
        self.assertEqual(weekly - NOT_A_SUITE - set().union(*SHORT_JOBS.values()) - {"gates-release"}, daily)

    def test_a_line_push_runs_full_linux_and_leaves_no_windows_row(self):
        # A job whose `if:` is false still leaves a skipped row at the tip, which
        # is the head of the pull request into main, so this reads every job of
        # every workflow the push starts.
        texts = {name: workflow_text(name) for name in WORKFLOW_FILES}
        started = [name for name in sorted(started_by("line push")) if triggered(texts[name], RUNS["line push"])]
        self.assertEqual(started, ["push"])
        published = {re.search(r"^ {4}name:\s*(.+?)\s*$", text, re.M).group(1)
                     for text in parity.workflow_jobs(texts["push"]).values()}
        self.assertEqual({name for name in published if name.startswith("windows")}, set())
        self.assertEqual(ran_by("line push")["push"], set(parity.workflow_jobs(texts["push"])), "no job can skip")

    def test_a_line_pull_request_runs_only_the_short_checks(self):
        ran = ran_by("line PR")
        self.assertEqual(ran["ci"], set())
        self.assertEqual(ran["line"], {"gates-build", "gates", "gates-line-verdict", "secret-scan",
                                       "gitleaks-canary", "security-review"})

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

    def test_a_job_widened_onto_line_pull_requests_is_caught(self):
        ci = workflow_text("ci")
        widened = ci.replace('    branches-ignore: ["integration/**"]\n', "", 1)
        self.assertNotEqual(widened, ci)
        ran = simulate(widened, RUNS["line PR"])
        self.assertLessEqual({"gates-verdict", "windows-verdict"}, ran)

    def test_a_short_check_dropped_from_line_pull_requests_is_caught(self):
        line = workflow_text("line")
        text = parity.workflow_jobs(line)["gates"]
        moved = line.replace(text, text.replace("!cancelled() && ", "!cancelled() && !always() && ", 1))
        self.assertNotIn("gates", simulate(moved, RUNS["line PR"]))

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
        for kind in ("main push", "weekly", "manual"):
            with self.subTest(run=kind):
                self.assertTrue(triggered(workflow_text("ci"), RUNS[kind]))
                self.assertFalse(triggered(workflow_text("line"), RUNS[kind]))


# The parity check (TSK-1321 AC-12). The placement table of docs/releasing.md,
# "Where CI suites run", and of the ADR that section links, by event and suite.
DOC = ROOT / "docs" / "releasing.md"
SECTION = "### Where CI suites run"
COLUMNS = ("Event", "Short checks", "Linux build and lint", "Linux tests", "Linux coverage",
           "Windows checks and journeys", "Windows partitions", "Release gates")
# Each event row of the table, and the kinds of run that stand for it.
EVENT_ROWS = {
    "Draft pull request, any base": ("draft line PR", "draft main PR"),
    "Pull request into an integration line": ("line PR",),
    "Push to an integration line": ("line push",),
    "Pull request into `main`": ("main PR",),
    "Push to `main`": ("main push",),
    "Daily run, `main` and each active line": ("daily",),
    "Weekly run, `main`": ("weekly",),
}
# The one job-to-suite map. The Linux tests run inside the coverage shards, so
# the two Linux test columns share their jobs. Every job of both workflows is
# in a suite, a short check, or NOT_A_SUITE.
SUITES = {
    "Linux build and lint": frozenset({"gates-build", "gates"}),
    "Linux tests": frozenset({"gates-coverage", "gates-coverage-report"}),
    "Linux coverage": frozenset({"gates-coverage", "gates-coverage-report"}),
    "Windows checks and journeys": frozenset({"windows", "windows-release-journey", "windows-journeys"}),
    "Windows partitions": frozenset({"windows-build", "windows-tests"}),
    "Release gates": frozenset({"gates-release"}),
}
# The short checks that run in codeflow-ci.yml. The weekly run's secret scan
# reads the full history, and its dependency audit runs in the security review.
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
# Every short-checks cell the table may hold, and the checks it names. "yes"
# is the five the operator named plus the security review, which the page
# says runs on every event. Any other text fails.
SHORT_CELLS = {
    "yes": frozenset({"policy check", "commit standards", "release impact", "secret scan", "security review"}),
    "no": frozenset(),
    "secret scan and security review": frozenset({"secret scan", "security review"}),
    "secret scan and security review only; commit standards passes only on a ready run":
        frozenset({"secret scan", "security review"}),
    "secret scan of the full history and dependency audit": frozenset({"secret scan", "security review"}),
}
# Every suite cell the table may hold. A reused cell runs the suite unless the
# reuse job names a run, and then skips it. Any other text fails.
SUITE_CELLS = {"yes": "runs", "no": "skips", "yes, or reused": "reused",
               "yes, or the merged pull request's reused verdict": "reused"}


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


def parity_problems(doc: str, read_adr, texts: dict[str, str], ran, reused=None, suites=SUITES) -> list[str]:
    """Each difference between the doc table, its ADR's table and what the
    workflows run. `ran(kind)` gives the jobs a kind of run runs, by workflow;
    `reused(kind)` the same when the reuse job names a run."""
    try:
        section = placement_section(doc)
        rows = placement_table(section, "docs/releasing.md")
        adr = placement_adr(section)
        adr_rows = placement_table(read_adr(adr), adr)
    except (OSError, ValueError) as problem:
        return [str(problem)]
    problems = []
    if rows != adr_rows:
        differ = [f"{row[0]!r}" for row, other in zip(rows, adr_rows) if row != other]
        problems.append(f"the docs/releasing.md table differs from {adr}'s table (rows {differ or 'count'})")
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
            for column, cell in zip(COLUMNS[1:], row[1:]):
                where = f"{event} / {column} ({kind}): the table says {cell!r}"
                if column == "Short checks":
                    if cell not in SHORT_CELLS:
                        problems.append(f"{where}, which this check cannot read")
                        continue
                    names = SHORT_CELLS[cell]
                    want = set().union(*(SHORT_JOBS[name] for name in names & SHORT_JOBS.keys()))
                    if want != done & short_jobs:
                        problems.append(f"{where}; the workflows run {sorted(done & short_jobs)}")
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

    def problems(self, doc=None, adr=None, texts=None, suites=SUITES, dispatcher=None):
        texts = texts or self.texts
        adr_text = self.adr if adr is None else adr
        return parity_problems(doc or self.doc, lambda name: adr_text, texts,
                               lambda kind: ran_by(kind, texts, dispatcher=dispatcher),
                               lambda kind: ran_by(kind, texts, "123", dispatcher), suites)

    def assertFails(self, problems, *words):
        self.assertTrue(problems, "the check passed")
        self.assertTrue(any(all(word in problem for word in words) for problem in problems),
                        f"no problem names {words}: {problems}")

    def test_the_workflows_match_the_doc_table_and_its_adr(self):
        self.assertEqual(self.problems(), [])

    def test_placement_matches_the_doc_table(self):
        self.assertEqual(parity_problems(self.doc, lambda name: self.adr, self.texts, ran_by_table), [])

    def change_row(self, text, event, column, cell):
        lines = text.splitlines()
        at = next(i for i, line in enumerate(lines) if line.startswith(f"| {event} |"))
        cells = [part.strip() for part in lines[at].strip().strip("|").split("|")]
        cells[COLUMNS.index(column)] = cell
        lines[at] = "| " + " | ".join(cells) + " |"
        return "\n".join(lines) + "\n"

    def test_a_doc_cell_changed_from_no_to_yes_fails(self):
        event, column = "Pull request into an integration line", "Windows partitions"
        doc = self.change_row(self.doc, event, column, "yes")
        adr = self.change_row(self.adr, event, column, "yes")
        self.assertFails(self.problems(doc, adr), event, column, "do not run")

    def test_a_job_condition_widened_to_line_pull_requests_fails(self):
        ci = self.texts["ci"]
        widened = ci.replace('    branches-ignore: ["integration/**"]\n', "", 1).replace(
            """(github.event_name == 'pull_request' && !github.event.pull_request.draft && contains(fromJSON('["main","master"]'), github.event.pull_request.base.ref) || github.event_name == 'push'""",
            """(github.event_name == 'pull_request' && !github.event.pull_request.draft || github.event_name == 'push'""")
        self.assertNotEqual(widened, ci)
        self.assertFails(self.problems(texts={**self.texts, "ci": widened}),
                         "Pull request into an integration line", "Windows partitions", "windows-build")

    def test_a_job_with_no_map_entry_fails(self):
        extra = ("\n  extra-check:\n    runs-on: ubuntu-24.04\n    timeout-minutes: 5\n"
                 "    steps:\n      - run: \"true\"\n")
        self.assertFails(self.problems(texts={**self.texts, "ci": self.texts["ci"].rstrip("\n") + "\n" + extra}),
                         "extra-check", "no suite")

    def test_a_renamed_column_fails(self):
        self.assertFails(self.problems(self.doc.replace("| Windows partitions |", "| Windows shards |", 1)),
                         "columns")

    def test_an_adr_table_that_differs_from_the_doc_fails(self):
        adr = self.change_row(self.adr, "Daily run, `main` and each active line", "Linux build and lint", "no")
        self.assertFails(self.problems(adr=adr), "differs from", "Daily run")

    def test_an_unreadable_cell_fails(self):
        event = "Push to an integration line"
        doc = self.change_row(self.doc, event, "Linux coverage", "mostly")
        adr = self.change_row(self.adr, event, "Linux coverage", "mostly")
        self.assertFails(self.problems(doc, adr), event, "cannot read")

    def test_a_suite_no_job_serves_fails(self):
        suites = {**SUITES, "Release gates": frozenset({"gates-release-renamed"})}
        self.assertFails(self.problems(suites=suites), "no job serves", "Release gates")

    def test_a_missing_section_or_adr_link_fails(self):
        self.assertFails(self.problems(self.doc.replace(SECTION, "### Where suites run", 1)), "sections, not one")
        no_link = self.doc.replace(f"]({self.adr_name})", "](decisions/)")
        self.assertFails(self.problems(no_link), "links 0 ADRs")

    def test_a_narrowed_trigger_fails(self):
        push = self.texts["push"]
        narrowed = push.replace('    branches: ["integration/**"]', '    branches: ["integration/x"]', 1)
        self.assertNotEqual(narrowed, push)
        self.assertFails(self.problems(texts={**self.texts, "push": narrowed}),
                         "Push to an integration line", "Linux coverage", "do not run")
        dispatcher = DISPATCHER.read_text(encoding="utf-8")
        unscheduled = dispatcher.replace("  schedule:\n", "  repository_dispatch:\n", 1)
        self.assertNotEqual(unscheduled, dispatcher)
        self.assertFails(self.problems(dispatcher=unscheduled), "Daily run", "Linux coverage", "do not run")

    def test_a_reused_cell_needs_the_suite_skipped_on_reuse(self):
        ci = self.texts["ci"]
        job = parity.workflow_jobs(ci)["gates-release"]
        ignores_reuse = ci.replace(job, job.replace("needs.reuse.outputs.run == '' && ", "", 1))
        self.assertNotEqual(ignores_reuse, ci)
        self.assertFails(self.problems(texts={**self.texts, "ci": ignores_reuse}),
                         "Push to `main`", "Release gates", "reused run")


class AdmissionTests(unittest.TestCase):
    """AC-14: `commit standards` admits a line pull request on the line verdict
    when the base line has the line workflow, and only the exact sync of main
    into a line that has not. Anything else keeps both verdicts. The step runs in
    a real repository, with a stand-in `gh` and `python3` and the real `git`."""

    LINE_FILE = ".github/workflows/codeflow-line-pr.yml"

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

    def assertBoth(self, result):
        code, out, argv = result
        self.assertEqual(code, 0, out)
        self.assertEqual(argv[argv.index("--workflow") + 1], ".github/workflows/codeflow-ci.yml")
        self.assertEqual([argv[at + 1] for at, word in enumerate(argv) if word == "--require"],
                         ["codeflow gates", "windows"])

    def assertLine(self, result):
        code, out, argv = result
        self.assertEqual(code, 0, out)
        self.assertEqual(argv[argv.index("--workflow") + 1], self.LINE_FILE)
        self.assertEqual([argv[at + 1] for at, word in enumerate(argv) if word == "--require"],
                         ["codeflow gates (line)"])

    def test_a_pull_request_into_main_needs_both_required_verdicts(self):
        head = self.sync()
        for base in ("main", "master", "feature/x", "integration/", "integration", ""):
            with self.subTest(base=base):
                self.assertBoth(self.admit(head, 'echo "gh must not be called" >&2\nexit 9\n', base=base))

    def test_a_real_sync_needs_the_line_verdict(self):
        self.assertLine(self.admit(self.sync()))

    def test_a_sync_of_an_ancestor_of_main_s_tip_needs_the_line_verdict(self):
        self.assertLine(self.admit(self.sync(), self.gh(tip=f"echo {'c' * 40}")))

    def test_a_main_commit_that_is_not_on_main_keeps_both_verdicts(self):
        for relation in ("diverged", "behind", "identical", "true"):
            with self.subTest(relation=relation):
                self.assertBoth(self.admit(self.sync() if relation == "diverged" else self.git("rev-parse", "sync"),
                                           self.gh(tip=f"echo {'c' * 40}", compare=f"echo {relation}")))

    def test_a_sync_plus_one_commit_keeps_both_verdicts(self):
        self.sync()
        self.write("extra.txt", "added\n")
        self.assertBoth(self.admit(self.commit("one more")))

    def test_a_task_branch_with_main_merged_in_keeps_both_verdicts(self):
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.commit("task work")
        self.git("merge", "-q", "--no-edit", self.main)
        self.assertBoth(self.admit(self.git("rev-parse", "HEAD")))

    def test_a_hand_resolved_sync_keeps_both_verdicts(self):
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
        self.assertBoth(self.admit(self.commit("resolved")))

    def test_an_ordinary_pull_request_into_an_unsynced_line_keeps_both_verdicts(self):
        self.branch("task")
        self.write("task.txt", "task work\n")
        self.assertBoth(self.admit(self.commit("task work")))

    def test_a_head_that_adds_main_s_file_without_main_keeps_both_verdicts(self):
        self.branch("task")
        self.write(self.LINE_FILE, "the line workflow\n")
        self.assertBoth(self.admit(self.commit("copy the file")))

    def test_an_adopter_with_no_line_workflow_on_main_keeps_both_verdicts(self):
        head = self.sync()
        self.git("switch", "-q", "main")
        self.git("rm", "-q", self.LINE_FILE)
        self.main = self.commit("no file")
        self.assertBoth(self.admit(head))

    def test_a_read_or_merge_that_fails_keeps_both_verdicts(self):
        head = self.sync()
        stale = f"echo {'d' * 40}"
        failed = 'echo "gh: Server Error (HTTP 502)" >&2; exit 1'
        for label, gh in (("a moved base", self.gh(pull=stale)), ("the base read", self.gh(pull=failed)),
                          ("the tip read", self.gh(tip=failed)),
                          ("the comparison", self.gh(tip=f"echo {'c' * 40}", compare=failed))):
            with self.subTest(label):
                self.assertBoth(self.admit(head, gh))
        broken = 'if [ "$1" = merge-tree ]; then exit 2; fi\nexec REAL "$@"\n'
        self.assertBoth(self.admit(head, git=broken))

    def test_once_the_line_has_the_placement_the_exception_is_off(self):
        # The base commit decides: every pull request is on the line verdict,
        # with no call to GitHub and whatever the head holds.
        self.git("switch", "-q", "integration/x")
        self.write(self.LINE_FILE, "the line workflow\n")
        self.line = self.commit("synced")
        self.branch("task")
        self.git("rm", "-q", self.LINE_FILE)
        self.assertLine(self.admit(self.commit("drop it"), 'echo "gh must not be called" >&2\nexit 9\n'))

    def test_a_draft_does_not_wait(self):
        code, out, argv = self.admit(self.sync(), ready=False)
        self.assertEqual(code, 0, out)
        self.assertEqual(argv[argv.index("--wait") + 1], "0")


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
