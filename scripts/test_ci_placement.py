#!/usr/bin/env python3
"""Where each job of .github/workflows/codeflow-ci.yml and of the daily run
(.github/workflows/windows-daily-run.yml) runs (TSK-1321).

Suite placement is an operator decision (2026-11-05, "A plus Linux off line
PRs"; ADR-0097, "CI suite placement by event"; docs/releasing.md, "Where CI
suites run"). PLACEMENT below is the one table of it: one row per job, one
column per kind of run. These tests read every job's `if:` and `needs:` from
the workflows themselves, evaluate them as GitHub does for each kind of run,
with the tier step run in bash, and fail on any difference from the table in
either direction. A job added, removed or moved therefore needs a visible
edit to this table.
"""

import importlib.util
import json
import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)
WORKFLOWS = ROOT / ".github" / "workflows"
WORKFLOW = (WORKFLOWS / "codeflow-ci.yml").read_text(encoding="utf-8")
BASH = shutil.which("bash")

# The kinds of run. A ready pull request into an integration line, into main,
# or into any other base; a push to a line or to main; the weekly schedule;
# a manual run. Drafts skip every gate (TSK-449) and are tested elsewhere.
RUNS = {
    "line PR": {"event": "pull_request", "base": "integration/EPC-022-the-3-2-0-milestone-fixes",
                "ref": "refs/pull/1/merge"},
    "main PR": {"event": "pull_request", "base": "main", "ref": "refs/pull/1/merge"},
    "other PR": {"event": "pull_request", "base": "feature/x", "ref": "refs/pull/1/merge"},
    "line push": {"event": "push", "base": "", "ref": "refs/heads/integration/EPC-022-the-3-2-0-milestone-fixes"},
    "main push": {"event": "push", "base": "", "ref": "refs/heads/main"},
    "weekly": {"event": "schedule", "base": "", "ref": "refs/heads/main"},
    "manual": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/main"},
    "daily": {"event": "workflow_dispatch", "base": "", "ref": "refs/heads/integration/EPC-022-the-3-2-0-milestone-fixes",
              "workflow": "windows-daily-run.yml"},
}

# The placement table. `yes` runs, `-` does not. A pull request into an
# integration line runs only the short checks: the Linux build and gate
# parts, and the secret scan and security review (its commit standards and
# release impact run in codeflow-policy.yml and codeflow-release.yml). Linux
# coverage and every Windows job run on line pushes, pull requests into main,
# the weekly and manual runs. Both verdicts run everywhere and pass with
# their suites not selected. The daily column is the daily run, on main and
# each active line at the commit the dispatcher resolved: every Windows job
# and the Linux coverage shards with their report, which is never merge
# evidence. Each row names its jobs' workflow: ci for codeflow-ci.yml.
PLACEMENT = """\
job                       workflow  line PR  main PR  other PR  line push  main push  weekly  manual  daily
ubuntu-image              ci        -        -        -         yes        yes        -       -       -
windows-image             ci        -        -        -         yes        yes        -       -       -
reuse                     ci        -        -        -         yes        yes        -       -       -
provenance                ci        yes      yes      yes       -          -          -       -       -
gates-build               ci        yes      yes      -         yes        yes        yes     yes     -
gates                     ci        yes      yes      -         yes        yes        yes     yes     -
gates-release             ci        -        yes      -         -          yes        yes     yes     -
gates-coverage            ci        -        yes      -         yes        yes        yes     yes     -
gates-coverage-report     ci        -        yes      -         yes        yes        yes     yes     -
gates-verdict             ci        yes      yes      yes       yes        yes        yes     yes     -
windows-tier              ci        yes      yes      yes       -          -          -       -       -
windows-build             ci        -        yes      -         yes        yes        yes     yes     -
windows-tests             ci        -        yes      -         yes        yes        yes     yes     -
windows                   ci        -        yes      -         yes        yes        yes     yes     -
windows-release-journey   ci        -        yes      -         yes        yes        yes     yes     -
windows-journeys          ci        -        yes      -         yes        yes        yes     yes     -
windows-verdict           ci        yes      yes      yes       yes        yes        yes     yes     -
secret-scan               ci        yes      yes      yes       yes        yes        yes     yes     -
gitleaks-canary           ci        yes      yes      yes       yes        yes        yes     yes     -
security-review           ci        yes      yes      yes       yes        yes        yes     yes     -
gates-coverage            daily     -        -        -         -          -          -       -       yes
gates-coverage-report     daily     -        -        -         -          -          -       -       yes
windows-build             daily     -        -        -         -          -          -       -       yes
windows-tests             daily     -        -        -         -          -          -       -       yes
windows                   daily     -        -        -         -          -          -       -       yes
windows-release-journey   daily     -        -        -         -          -          -       -       yes
windows-journeys          daily     -        -        -         -          -          -       -       yes
"""
WORKFLOW_FILES = {"ci": "codeflow-ci.yml", "daily": "windows-daily-run.yml"}

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


def ran_by(kind: str) -> dict[str, set[str]]:
    """The jobs that run on a kind of run, by workflow. A kind of run starts one
    workflow: the daily run starts only the daily runner, the others only ci."""
    run = RUNS[kind]
    started = "daily" if run.get("workflow") == WORKFLOW_FILES["daily"] else "ci"
    return {name: simulate(workflow_text(name), run) if name == started else set() for name in WORKFLOW_FILES}


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


def run_tier(event: str, base: str) -> str:
    env, script = step("windows-tier", "Choose the Windows tier")
    with tempfile.TemporaryDirectory() as folder:
        out = Path(folder) / "output"
        out.write_text("", encoding="utf-8")
        values = {"PATH": os.environ["PATH"], "HOME": folder, "REDUCED_TIER": env["REDUCED_TIER"], "EVENT": event,
                  "BASE": base, "GITHUB_OUTPUT": str(out), "GITHUB_STEP_SUMMARY": os.devnull}
        done = subprocess.run([BASH, "--noprofile", "--norc", "-eo", "pipefail", "-c", script], cwd=folder,
                              env=values, capture_output=True, text=True, check=True)
        assert done.returncode == 0
        return out.read_text(encoding="utf-8").strip().removeprefix("tier=")


def step(job: str, name: str) -> tuple[dict[str, str], str]:
    lines = parity.workflow_jobs(WORKFLOW)[job].splitlines()
    start = lines.index(f"      - name: {name}")
    env, script, at = {}, [], start + 1
    while at < len(lines) and not lines[at].startswith("      - "):
        line = lines[at]
        if line == "        env:":
            at += 1
            while lines[at].startswith("          ") and ":" in lines[at]:
                key, value = lines[at].strip().split(": ", 1)
                env[key] = value[1:-1] if value[:1] in "'\"" and value[-1:] == value[:1] else value
                at += 1
            continue
        if line == "        run: |":
            at += 1
            while at < len(lines) and (not lines[at].strip() or lines[at].startswith(" " * 10)):
                script.append(lines[at][10:])
                at += 1
            continue
        at += 1
    return env, "\n".join(script) + "\n"


def simulate(workflow: str, run: dict, tier_result=None) -> set[str]:
    """The jobs that run, every job that runs passing. `tier_result` replaces
    the tier job's outcome, for the fail-closed cases."""
    jobs = parity.workflow_jobs(workflow)
    fields = {name: job_fields(text) for name, text in jobs.items()}
    context = {"github.event_name": run["event"], "github.event.pull_request.base.ref": run["base"],
               "github.event.pull_request.draft": False if run["event"] == "pull_request" else None,
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
            outputs = {}
            if runs and name == "windows-tier":
                outputs = {"tier": run_tier(run["event"], run["base"])}
            result = "success" if runs else "skipped"
            if name == "windows-tier" and runs and tier_result is not None:
                result, outputs = tier_result
            results[name] = {"result": result, "outputs": outputs}
    return {name for name, value in results.items() if value["result"] != "skipped"}


@unittest.skipIf(BASH is None or os.name == "nt", "the tier step runs in Linux bash")
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

    def test_the_daily_run_covers_linux_coverage_and_every_windows_job_the_line_pr_drops(self):
        line, push, daily = ran_by("line PR")["ci"], ran_by("line push")["ci"], ran_by("daily")["daily"]
        # The image and reuse jobs serve a push run itself and test nothing.
        self.assertEqual(push - line - {"ubuntu-image", "windows-image", "reuse"}, daily)

    def test_a_line_pull_request_runs_only_the_short_checks(self):
        ran = simulate(WORKFLOW, RUNS["line PR"])
        self.assertEqual(ran, {"provenance", "gates-build", "gates", "gates-verdict", "windows-tier",
                               "windows-verdict", "secret-scan", "gitleaks-canary", "security-review"})

    def test_a_job_moved_back_onto_line_pull_requests_is_caught(self):
        for job in ("gates-coverage", "windows", "windows-release-journey", "windows-build"):
            with self.subTest(job=job):
                text = parity.workflow_jobs(WORKFLOW)[job]
                moved = WORKFLOW.replace(text, text.replace("needs.windows-tier.outputs.tier != 'reduced' && ", "", 1))
                self.assertNotEqual(moved, WORKFLOW)
                self.assertIn(job, simulate(moved, RUNS["line PR"]))

    def test_a_short_check_dropped_from_line_pull_requests_is_caught(self):
        text = parity.workflow_jobs(WORKFLOW)["gates"]
        moved = WORKFLOW.replace(text, text.replace("!cancelled() && ", "!cancelled() && needs.windows-tier.outputs."
                                                    "tier != 'reduced' && ", 1).replace(
                                     "needs: [reuse, gates-build]", "needs: [reuse, gates-build, windows-tier]", 1))
        self.assertNotIn("gates", simulate(moved, RUNS["line PR"]))

    def test_an_unreadable_event_or_base_runs_the_full_set(self):
        # The tier step fails closed: an empty or unexpected event or base,
        # or a tier job that failed or wrote nothing, runs every moved job.
        line = RUNS["line PR"]
        full = simulate(WORKFLOW, line, tier_result=("success", {"tier": "full"}))
        moved = {"gates-coverage", "gates-coverage-report", "windows-build", "windows-tests", "windows",
                 "windows-release-journey", "windows-journeys"}
        self.assertLessEqual(moved, full)
        for case in (("failure", {}), ("success", {}), ("success", {"tier": ""}), ("success", {"tier": "full "})):
            with self.subTest(tier=case):
                self.assertEqual(simulate(WORKFLOW, line, tier_result=case), full)
        self.assertEqual(run_tier("", line["base"]), "full")
        self.assertEqual(run_tier("pull_request", ""), "full")


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
