#!/usr/bin/env python3
"""Daily Windows and Linux coverage source selection, the reader's verdict,
workflow controls, and which jobs read the tier, from the workflows' own text.
"""

import contextlib
from datetime import datetime, timedelta, timezone
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("windows_daily", ROOT / "scripts/windows-daily.py")
daily = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(daily)
SHA = "a" * 40
LINE = "refs/heads/integration/example"


def total(run_id, sha=SHA, covered=950):
    """A run's recorded Linux coverage total: 95 percent of 1000 lines at `sha`."""
    return {"lines": 1000, "covered": covered, "floor": 90.0, "sha": sha}


# No test reaches GitHub for a coverage total: every run recorded `total` unless a
# test passes its own. CoverageReaderTests covers the reader's checks of it.
DOWNLOAD = daily.download_coverage
PATCHES =[mock.patch.object(daily, "download_coverage", total)]


def setUpModule():
    for patch in PATCHES:
        patch.start()


def tearDownModule():
    for patch in PATCHES:
        patch.stop()


class SourceTests(unittest.TestCase):
    def test_main_and_every_integration_ref_are_resolved_once(self):
        refs = f"{SHA}\trefs/heads/main\n{'b' * 40}\t{LINE}\n{SHA}\trefs/heads/task/example\n"
        selected = daily.sources(refs)["include"]
        self.assertEqual([(s["ref"], s["sha"]) for s in selected], [(LINE, "b" * 40), ("refs/heads/main", SHA)])

    def test_new_lines_are_discovered_without_a_name_allowlist(self):
        refs = f"{SHA}\trefs/heads/main\n{SHA}\trefs/heads/integration/new-line\n"
        self.assertEqual(len(daily.sources(refs)["include"]), 2)

    def test_empty_missing_main_and_duplicate_refs_are_refused(self):
        for refs in ("", f"{SHA}\t{LINE}\n", f"{SHA}\trefs/heads/main\n" * 2):
            with self.subTest(refs=refs), self.assertRaises(ValueError):
                daily.sources(refs)

    def test_malformed_sha_or_ref_cannot_become_a_matrix_row(self):
        for sha, ref in (("a" * 7, LINE), ("0" * 40, LINE), (SHA, LINE + "/../other"),
                         (SHA, LINE + ";echo"), (SHA, LINE + "/.hidden")):
            with self.subTest(sha=sha, ref=ref), self.assertRaises(ValueError):
                daily.sources(f"{SHA}\trefs/heads/main\n{sha}\t{ref}\n")

    def test_more_than_the_github_matrix_limit_fails_without_truncation(self):
        refs = f"{SHA}\trefs/heads/main\n" + "".join(
            f"{SHA}\trefs/heads/integration/line-{n}\n" for n in range(256))
        with self.assertRaisesRegex(ValueError, "256-job"):
            daily.sources(refs)


class IdentityTests(unittest.TestCase):
    def test_checkout_must_match_the_discovered_commit(self):
        self.assertEqual(daily.checked_out(LINE, SHA, SHA), SHA)
        with self.assertRaisesRegex(ValueError, "no tests were credited"):
            daily.checked_out(LINE, SHA, "b" * 40)

    def test_identity_refuses_a_ref_that_is_neither_main_nor_a_line(self):
        for ref in ("refs/heads/task/x", "refs/tags/integration/x", "refs/pull/1/merge"):
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                daily.identity(ref, SHA)


def workflow(name):
    return (ROOT / ".github/workflows" / f"{name}.yml").read_text()


def jobs(text):
    text = text.split("\njobs:\n", 1)[1]
    return {m.group(1): m.group(2) for m in re.finditer(
        r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)", text, re.M | re.S)}


# The one daily-only command: the report job records the total and the commit.
RECORD_TOTAL = ("python3 -B scripts/coverage-merge.py --floor-from .codeflow/test-config.json --shards 2 "
                "--total-file target/coverage/daily-coverage-total.json target/coverage/data")


def commands(job):
    """Read run scalars from the repository's fixed-indentation workflow style."""
    found = []
    for m in re.finditer(r"^        run: (.*)(?:\n|$)", job, re.M):
        command = m.group(1)
        if command == "|":
            lines = []
            for line in job[m.end():].splitlines():
                if not line.startswith("          "):
                    break
                lines.append(line[10:])
            command = "\n".join(lines)
        if "scripts/windows-daily.py identity" not in command:
            found.append(command)
    return found


class WorkflowTests(unittest.TestCase):
    def test_the_schedule_discovers_lines_and_dispatches_the_runner_at_their_refs(self):
        scheduler = workflow("windows-daily")
        self.assertIn('cron: "23 6 * * *"', scheduler)
        self.assertIn("run: python3 -I -B scripts/windows-daily.py discover", scheduler)
        self.assertIn("matrix: ${{ fromJSON(needs.sources.outputs.matrix) }}", scheduler)
        self.assertIn("fail-fast: false", scheduler)
        self.assertNotIn("uses: ./.github/workflows/windows-daily-run.yml", scheduler)

    def test_the_runner_can_only_be_dispatched(self):
        runner = workflow("windows-daily-run")
        triggers = runner.split("\non:\n", 1)[1].split("\npermissions:", 1)[0]
        self.assertEqual(re.findall(r"^  (\w+):", triggers, re.M), ["workflow_dispatch"])
        self.assertEqual(re.findall(r"^      ([\w-]+):", triggers, re.M), ["source-sha"])
        permissions = runner.split("\npermissions:\n", 1)[1].split("\n\n", 1)[0]
        self.assertEqual(permissions.split(), ["contents:", "read"])

    def test_daily_jobs_execute_the_full_existing_windows_and_coverage_commands(self):
        existing = jobs((ROOT / ".github/workflows/codeflow-ci.yml").read_text())
        added = jobs(workflow("windows-daily-run"))
        self.assertEqual(set(added), daily.WINDOWS_JOBS | daily.COVERAGE_JOBS)
        for name in daily.WINDOWS_JOBS | daily.COVERAGE_JOBS:
            with self.subTest(job=name):
                daily_only = [RECORD_TOTAL] if name == "gates-coverage-report" else []
                self.assertEqual(commands(added[name]), commands(existing[name]) + daily_only)
                runner = "windows-latest" if name in daily.WINDOWS_JOBS else "ubuntu-24.04"
                self.assertIn(f"runs-on: {runner}", added[name])
                self.assertEqual(added[name].count("persist-credentials: false"), 1)
                self.assertEqual(added[name].count("uses: actions/checkout@"), 1)
        self.assertIn("shard: [1, 2]", added["gates-coverage"])
        self.assertIn("--shards 2 ", RECORD_TOTAL)
        self.assertEqual(len(re.findall(r"^\s+shard: \[1, 2\]$", existing["gates-coverage"], re.M)), 1)
        self.assertIn("partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]", added["windows-tests"])
        self.assertIn("fail-fast: false", added["windows-tests"])

    def test_the_total_is_recorded_for_this_commit_and_kept_under_the_floor_too(self):
        report = jobs(workflow("windows-daily-run"))["gates-coverage-report"]
        steps = re.findall(r"^      - name: (.*)$", report, re.M)
        self.assertEqual(steps[-2:], ["Record the coverage total for the daily verdict",
                                      "Keep the coverage total for the daily verdict"])
        kept = report.split("      - name: Keep the coverage total for the daily verdict\n", 1)[1]
        self.assertIn("if: ${{ !cancelled() }}", kept)
        self.assertIn(f"name: {daily.COVERAGE_ARTIFACT}", kept)
        self.assertIn(f"path: target/coverage/{daily.COVERAGE_FILE}", kept)
        self.assertIn(f"--total-file target/coverage/{daily.COVERAGE_FILE}", RECORD_TOTAL)

    def test_every_job_stops_when_its_commit_is_not_the_requested_one(self):
        """The exact-SHA check: dispatching at a ref runs its tip, which can have moved."""
        added = jobs(workflow("windows-daily-run"))
        self.assertIn("TESTED_SHA: ${{ inputs.source-sha }}", workflow("windows-daily-run"))
        for name in daily.WINDOWS_JOBS | daily.COVERAGE_JOBS:
            with self.subTest(job=name):
                steps = re.findall(r"^      - name: (.*)$", added[name], re.M)
                self.assertEqual(steps[:2], ["Check out this run's commit", "Report and verify the tested commit"])
                self.assertIn('windows-daily.py identity --ref "$GITHUB_REF" --sha "$TESTED_SHA"', added[name])

    def test_the_identity_command_fails_on_another_commit(self):
        with tempfile.TemporaryDirectory() as temp:
            subprocess.run(["git", "init", "-q", temp], check=True, env=CLEAN_ENV)
            subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "core.hooksPath=/dev/null",
                            "commit", "-q", "--allow-empty", "-m", "x"], cwd=temp, check=True, env=CLEAN_ENV)
            head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=temp, text=True, env=CLEAN_ENV).strip()
            for sha, status in ((head, 0), ("b" * 40, 1)):
                with self.subTest(sha=sha):
                    done = subprocess.run([sys.executable, "-B", str(ROOT / "scripts/windows-daily.py"), "identity",
                                           "--ref", LINE, "--sha", sha], cwd=temp, text=True, capture_output=True,
                                          env=CLEAN_ENV)
                    self.assertEqual(done.returncode, status, done.stderr)

    def test_the_job_names_the_reader_expects_are_the_runner_s(self):
        names = set()
        for job in jobs(workflow("windows-daily-run")).values():
            name = re.search(r"^    name: (.*)$", job, re.M).group(1)
            axis = re.search(r"\$\{\{ matrix\.(\w+) \}\}", name)
            if not axis:
                names.add(name)
                continue
            values = re.search(rf"^\s+{axis.group(1)}: \[(.*)\]$", job, re.M).group(1).split(", ")
            names.update(name.replace(axis.group(0), value) for value in values)
        self.assertEqual(names, daily.RUN_JOB_NAMES)


class DispatcherTests(unittest.TestCase):
    """Review test 1: only the dispatch job can start runs, and it runs no line code."""

    def scheduler(self):
        return jobs(workflow("windows-daily"))

    def test_only_the_dispatch_job_holds_actions_write(self):
        self.assertIn("permissions: {}", workflow("windows-daily").split("jobs:")[0])
        for name, job in self.scheduler().items():
            with self.subTest(job=name):
                self.assertEqual("actions: write" in job, name == "dispatch")
        for other in ("windows-daily-run", "windows-daily-verdict"):
            self.assertNotIn("actions: write", workflow(other))

    def test_the_dispatch_job_holds_nothing_else(self):
        block = re.search(r"^    permissions:\n((?:      .*\n)+)", self.scheduler()["dispatch"], re.M).group(1)
        self.assertEqual(block.split(), ["actions:", "write"])

    def test_the_dispatch_job_checks_out_nothing_and_only_starts_a_run(self):
        job = self.scheduler()["dispatch"]
        self.assertNotIn("actions/checkout", job)
        self.assertEqual(re.findall(r"^\s+- uses: |^\s+uses: ", job, re.M), [])
        self.assertEqual(job.count("      - name: "), 1)
        self.assertEqual(re.findall(r"^        run: (.*)$", job, re.M), [
            'gh workflow run windows-daily-run.yml --repo "$GITHUB_REPOSITORY" '
            '--ref "${SOURCE_REF#refs/heads/}" -f source-sha="$SOURCE_SHA"'])

    def test_the_dispatch_job_passes_the_line_through_the_environment(self):
        """Never interpolated into the command, though discovery validates both."""
        command = re.search(r"^        run: (.*)$", self.scheduler()["dispatch"], re.M).group(1)
        self.assertNotIn("${{", command)

    def test_the_dispatch_step_starts_the_run_at_the_line_s_own_ref(self):
        """The real command line, against a stand-in gh that records its arguments."""
        command = re.search(r"^        run: (.*)$", self.scheduler()["dispatch"], re.M).group(1)
        with tempfile.TemporaryDirectory() as temp:
            gh = Path(temp) / "gh"
            gh.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$GH_ARGUMENTS"\n', encoding="utf-8")
            gh.chmod(0o755)
            arguments = Path(temp) / "arguments"
            done = subprocess.run(["bash", "-e", "-c", command], text=True, capture_output=True, env={
                "PATH": f"{temp}{os.pathsep}{os.environ['PATH']}", "GH_ARGUMENTS": str(arguments),
                "GITHUB_REPOSITORY": "o/n", "SOURCE_REF": "refs/heads/integration/EPC-1-x", "SOURCE_SHA": SHA})
            self.assertEqual(done.returncode, 0, done.stderr)
            self.assertEqual(arguments.read_text().split(), [
                "workflow", "run", "windows-daily-run.yml", "--repo", "o/n", "--ref", "integration/EPC-1-x",
                "-f", f"source-sha={SHA}"])

    def test_the_source_job_reads_lines_as_data_and_cannot_write(self):
        job = self.scheduler()["sources"]
        self.assertEqual(re.search(r"^    permissions:\n((?:      .*\n)+)", job, re.M).group(1).split(),
                         ["actions:", "read", "contents:", "read"])
        self.assertNotRegex(job, r"(?m)^\s+ref:")
        self.assertEqual(re.findall(r"^        run: (.*)$", job, re.M),
                         ["python3 -I -B scripts/windows-daily.py discover"])

    def test_the_dispatcher_and_reader_act_only_under_main(self):
        self.assertIn("if: ${{ github.ref == 'refs/heads/main' }}", self.scheduler()["sources"])
        self.assertIn("if: ${{ github.ref == 'refs/heads/main' }}", jobs(workflow("windows-daily-verdict"))["verdict"])

    def test_the_reader_can_read_and_write_nothing(self):
        job = jobs(workflow("windows-daily-verdict"))["verdict"]
        self.assertEqual(re.search(r"^    permissions:\n((?:      .*\n)+)", job, re.M).group(1).split(),
                         ["actions:", "read", "contents:", "read"])
        self.assertIn("run: python3 -I -B scripts/windows-daily.py read", job)
        self.assertIn("permissions: {}", workflow("windows-daily-verdict").split("jobs:")[0])
        self.assertNotRegex(job, r"(?m)^\s+ref:")


REQUESTED_AT = datetime(2026, 11, 4, 6, 23, tzinfo=timezone.utc)
STARTED = "2026-11-04T06:24:00Z"
RUNNER_TEXT = "the runner file\n"
RUNNER_HASH = daily.runner_hash(RUNNER_TEXT)


def recorded_run(**changes):
    return {"id": 7, "path": daily.RUNNER_PATH, "event": "workflow_dispatch", "head_branch": "integration/example",
            "head_sha": SHA, "status": "completed", "conclusion": "success", "created_at": STARTED, **changes}


def recorded_jobs():
    return [{"name": name, "conclusion": "success"} for name in daily.RUN_JOB_NAMES]


class ReaderTests(unittest.TestCase):
    """Review test 2: a line's workflow can change what its run reports, so the reader
    judges only from what GitHub records and fails closed on any mismatch."""

    def judge(self, runs=None, job_rows=None, runner=RUNNER_TEXT, ref=LINE):
        return daily.judge({"ref": ref, "sha": SHA}, REQUESTED_AT, [recorded_run()] if runs is None else runs,
                           lambda run: recorded_jobs() if job_rows is None else job_rows,
                           lambda sha: runner, RUNNER_HASH, total, 90.0)

    def test_a_run_matching_every_record_is_credited(self):
        result = self.judge()
        self.assertEqual(result["result"], "credited")
        self.assertEqual(result["tested_sha"], SHA)
        self.assertEqual(result["run_id"], 7)

    def test_each_recorded_field_that_does_not_match_gets_no_credit(self):
        for field, value in (("path", ".github/workflows/other.yml"), ("event", "push"), ("event", "pull_request"),
                             ("head_branch", "integration/other"), ("head_sha", "b" * 40), ("path", None),
                             ("event", None), ("head_branch", None), ("head_sha", None)):
            with self.subTest(field=field, value=value):
                result = self.judge([recorded_run(**{field: value})])
                self.assertEqual(result["result"], "no-credit")
                self.assertIsNone(result["tested_sha"])
                self.assertIn(field, result["reasons"][0])

    def test_any_conclusion_but_success_gets_no_credit(self):
        for conclusion in ("failure", "cancelled", "skipped", "timed_out", "neutral", "action_required", None):
            with self.subTest(conclusion=conclusion):
                result = self.judge([recorded_run(conclusion=conclusion)])
                self.assertEqual(result["result"], "failure")
                self.assertIsNone(result["tested_sha"])

    def test_a_line_s_changed_workflow_is_not_credited_even_with_all_green(self):
        """What stops a line's own workflow being credited as full tier."""
        for runner in ("a line removed the partitions\n", RUNNER_TEXT + "\n", "", None):
            with self.subTest(runner=runner):
                result = self.judge(runner=runner)
                self.assertEqual(result["result"], "no-credit")
                self.assertIsNone(result["tested_sha"])
                self.assertIn("differs from main's", result["reasons"][0])

    def test_a_missing_failed_skipped_or_extra_job_gets_no_credit(self):
        names = sorted(daily.RUN_JOB_NAMES)
        for name in names:
            for state in ("failure", "skipped", "cancelled", None):
                rows = [{"name": n, "conclusion": state if n == name else "success"} for n in names]
                with self.subTest(name=name, state=state):
                    self.assertEqual(self.judge(job_rows=rows)["result"], "no-credit")
        without_partition = [row for row in recorded_jobs() if row["name"] != "windows tests (7 of 16)"]
        self.assertEqual(self.judge(job_rows=without_partition)["result"], "no-credit")
        self.assertEqual(self.judge(job_rows=recorded_jobs() + [{"name": "extra", "conclusion": "success"}])["result"],
                         "no-credit")
        self.assertEqual(self.judge(job_rows=[])["result"], "no-credit")

    def test_the_newest_run_decides_and_an_older_green_one_cannot_cover_it(self):
        newer = recorded_run(id=8, created_at="2026-11-04T06:30:00Z", conclusion="failure")
        self.assertEqual(self.judge([recorded_run(), newer])["result"], "failure")

    def test_main_s_row_is_judged_like_any_line(self):
        run = recorded_run(head_branch="main")
        self.assertEqual(self.judge([run], ref=daily.MAIN)["result"], "credited")
        self.assertEqual(self.judge([recorded_run()], ref=daily.MAIN)["result"], "no-credit")


class MissingRunTests(unittest.TestCase):
    """Review test 3: results no longer come through `needs`, so a missing or late
    run is no evidence, never green."""

    NOW = REQUESTED_AT + timedelta(hours=6)

    def judge(self, runs):
        return daily.judge({"ref": LINE, "sha": SHA}, REQUESTED_AT, runs, lambda run: recorded_jobs(),
                           lambda sha: RUNNER_TEXT, RUNNER_HASH, total, 90.0)

    def test_no_run_at_all_is_no_evidence(self):
        result = self.judge([])
        self.assertEqual(result["result"], "no-evidence")
        self.assertIsNone(result["tested_sha"])

    def test_a_green_run_from_before_the_request_is_no_evidence(self):
        result = self.judge([recorded_run(created_at="2026-11-03T06:24:00Z")])
        self.assertEqual(result["result"], "no-evidence")

    def test_an_unfinished_run_is_no_evidence(self):
        for status in ("queued", "in_progress", "waiting", "pending", "requested", None):
            with self.subTest(status=status):
                result = self.judge([recorded_run(status=status, conclusion=None)])
                self.assertEqual(result["result"], "no-evidence")
                self.assertIsNone(result["tested_sha"])

    def evidence(self, request, runs_by_ref, requested_run=None):
        run = {"id": 5, "path": daily.DISPATCHER_PATH, "head_branch": "main", "event": "schedule",
               "status": "completed", "created_at": "2026-11-04T06:23:00Z", **(requested_run or {})}

        def api(path):
            if "windows-daily.yml/runs" in path:
                return {"workflow_runs": [run]}
            if "/jobs?" in path:
                return {"jobs": recorded_jobs()}
            return {"workflow_runs": next(runs for ref, runs in runs_by_ref.items()
                                          if f"branch={daily.quote(ref, safe='')}&" in path)}

        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            return daily.read_evidence(api, lambda run_id: {"runner_sha256": RUNNER_HASH, **request},
                                       lambda sha, path: RUNNER_TEXT, self.NOW)

    REQUEST = {"include": [{"ref": daily.MAIN, "sha": SHA}, {"ref": LINE, "sha": SHA}], "skipped": [], "left_out": []}
    MAIN_RUN = recorded_run(head_branch="main")

    def test_every_requested_line_credited_passes(self):
        result = self.evidence(self.REQUEST, {"main": [self.MAIN_RUN], "integration/example": [recorded_run()]})
        self.assertEqual(result["result"], "success")

    def test_one_missing_line_fails_the_whole_verdict(self):
        result = self.evidence(self.REQUEST, {"main": [self.MAIN_RUN], "integration/example": []})
        self.assertEqual(result["result"], "failure")
        self.assertEqual([line["result"] for line in result["lines"]], ["credited", "no-evidence"])

    def test_one_late_line_fails_the_whole_verdict(self):
        late = recorded_run(status="in_progress", conclusion=None)
        result = self.evidence(self.REQUEST, {"main": [self.MAIN_RUN], "integration/example": [late]})
        self.assertEqual(result["result"], "failure")

    def test_a_request_without_main_s_runner_hash_is_refused(self):
        for recorded in (None, "", "abc", "A" * 64, 7):
            request = {**self.REQUEST, "runner_sha256": recorded}
            with self.subTest(recorded=recorded), self.assertRaisesRegex(ValueError, "does not record main's runner"):
                self.evidence(request, {"main": [self.MAIN_RUN], "integration/example": [recorded_run()]})

    def test_a_runner_that_main_changes_after_the_request_costs_no_red_day(self):
        """The lines ran main's runner of 06:23, and the request recorded it; the
        file main holds at 12:23 has no say."""
        with mock.patch.object(daily, "RUNNER", Path("/nonexistent/runner.yml")):
            result = self.evidence(self.REQUEST, {"main": [self.MAIN_RUN], "integration/example": [recorded_run()]})
        self.assertEqual(result["result"], "success")

    def test_a_line_s_runner_is_held_to_the_hash_the_request_recorded(self):
        request = {**self.REQUEST, "runner_sha256": daily.runner_hash("main's runner at the request\n")}
        result = self.evidence(request, {"main": [self.MAIN_RUN], "integration/example": [recorded_run()]})
        self.assertEqual(result["result"], "failure")
        self.assertEqual([line["result"] for line in result["lines"]], ["no-credit", "no-credit"])

    def test_a_request_without_main_or_malformed_is_refused(self):
        for request in ({"include": [{"ref": LINE, "sha": SHA}], "skipped": [], "left_out": []},
                        {"include": [], "skipped": [], "left_out": []}, {},
                        {"include": [{"ref": daily.MAIN, "sha": SHA}], "left_out": []},
                        {"include": [{"ref": daily.MAIN, "sha": "bad"}], "skipped": [], "left_out": []}):
            with self.subTest(request=request), self.assertRaises((ValueError, KeyError)):
                self.evidence(request, {"main": [self.MAIN_RUN]})

    def test_a_dispatcher_run_that_is_missing_stale_or_not_main_s_is_refused(self):
        good = {"id": 5, "path": daily.DISPATCHER_PATH, "head_branch": "main", "event": "schedule",
                "status": "completed", "created_at": "2026-11-04T06:23:00Z"}
        self.assertEqual(daily.latest_request([good], self.NOW)["id"], 5)
        for changes in ({"created_at": "2026-11-02T06:23:00Z"}, {"head_branch": "integration/example"},
                        {"path": ".github/workflows/other.yml"}, {"event": "push"}, {"status": "in_progress"}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                daily.latest_request([{**good, **changes}], self.NOW)
        with self.assertRaises(ValueError):
            daily.latest_request([], self.NOW)

    def test_the_read_command_fails_and_keeps_a_result_when_github_cannot_be_read(self):
        with tempfile.TemporaryDirectory() as temp, contextlib.chdir(temp), mock.patch.object(
                daily, "gh_api", side_effect=subprocess.CalledProcessError(1, "gh")), mock.patch.dict(
                os.environ, {"GITHUB_REPOSITORY": "o/n", "GITHUB_STEP_SUMMARY": str(Path(temp) / "summary")}), \
                mock.patch.object(sys, "argv", ["windows-daily.py", "read"]), mock.patch("builtins.print"):
            self.assertEqual(daily.main(), 1)
            result = json.loads((Path(temp) / daily.RESULT_FILE).read_text())
            self.assertEqual(result["result"], "failure")
            self.assertIn("no line has evidence", (Path(temp) / "summary").read_text())

    def test_the_read_command_writes_the_exact_source_and_the_limit(self):
        request = {"include": [{"ref": daily.MAIN, "sha": SHA}], "skipped": [], "left_out": [
            {"ref": LINE, "sha": "b" * 40, "missing": [f"{daily.RUNNER_PATH} differs from main's"]}],
            "runner_sha256": daily.runner_hash(daily.RUNNER.read_text(encoding="utf-8"))}
        run = {"id": 5, "path": daily.DISPATCHER_PATH, "head_branch": "main", "event": "schedule",
               "status": "completed", "created_at": datetime.now(timezone.utc).isoformat()}
        started = (datetime.now(timezone.utc) + timedelta(seconds=1)).isoformat()

        def api(path):
            if "windows-daily.yml/runs" in path:
                return {"workflow_runs": [run]}
            if "/jobs?" in path:
                return {"jobs": recorded_jobs()}
            return {"workflow_runs": [recorded_run(head_branch="main", created_at=started)]}

        with tempfile.TemporaryDirectory() as temp, contextlib.chdir(temp), mock.patch.object(
                daily, "gh_api", api), mock.patch.object(daily, "download_request", lambda run_id: request), \
                mock.patch.object(daily, "tree_reader", lambda: lambda sha, path: daily.RUNNER.read_text(encoding="utf-8")), \
                mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n", "GITHUB_STEP_SUMMARY": str(Path(temp) / "summary")}), \
                mock.patch.object(sys, "argv", ["windows-daily.py", "read"]), mock.patch("builtins.print"):
            self.assertEqual(daily.main(), 0)
            summary = (Path(temp) / "summary").read_text()
            self.assertIn(SHA, summary)
            self.assertIn("no evidence for a newer branch tip", summary)
            self.assertIn(f"{LINE}` at `{'b' * 40}`: left out, no daily evidence and no daily coverage", summary)
            self.assertIn(f"Linux line coverage 95.00 percent (950 of 1000 lines) measured at `{SHA}`", summary)
            self.assertIn("differs from main's", summary)


NEEDS = {"scripts/partition-check.py": {"check", "reports", "--count"}, "scripts/journey-gate.py": {"--part"}}
PARTITIONS = 'add_parser("check")\nadd_parser("reports")\nadd_argument("--count")\n'
COMPLETE = {"scripts/partition-check.py": PARTITIONS, "scripts/journey-gate.py": 'add_argument("--part")',
            daily.RUNNER_PATH: RUNNER_TEXT}


def line_tree(needs):
    """A tree holding exactly what `needs` asks for."""
    text = {}
    for path, words in needs.items():
        text[path] = "\n".join(f'add_argument("{word}")' if word.startswith("--") else f'add_parser("{word}")'
                               for word in sorted(words)) + "\n"
    text[daily.RUNNER_PATH] = workflow("windows-daily-run")
    return text


class ActiveLineTests(unittest.TestCase):
    """A line is active when its own tree has what the runner calls."""

    def select(self, trees):
        listed = {"include": [{"ref": ref, "sha": sha} for ref, sha in trees]}
        return daily.select_active(listed, NEEDS, lambda sha, path: trees[next(
            ref for ref, known in trees if known == sha), sha].get(path), RUNNER_TEXT)

    def test_the_needs_are_read_from_the_runner_itself(self):
        needs = daily.runner_needs(workflow("windows-daily-run"))
        self.assertLessEqual({"reports", "check", "--count", "--prefix", "--results", "--list"},
                             needs["scripts/partition-check.py"])
        self.assertLessEqual({"--part", "--skip-benchmark", "--results"}, needs["scripts/journey-gate.py"])
        self.assertIn("scripts/test_gate_parity.py", needs)
        self.assertNotIn("scripts/windows-test-times.json", needs)
        self.assertLessEqual({"identity", "--ref"}, needs["scripts/windows-daily.py"])

    def test_a_runner_naming_no_script_proves_no_line(self):
        with self.assertRaisesRegex(ValueError, "names no script"):
            daily.runner_needs("jobs:\n  build:\n    steps:\n      - run: cargo build\n")

    def test_this_tree_has_everything_the_runner_needs(self):
        needs = daily.runner_needs(workflow("windows-daily-run"))
        read = lambda path: (ROOT / path).read_text(encoding="utf-8") if (ROOT / path).exists() else None
        self.assertEqual(daily.missing_from_line(needs, read), [])

    def test_a_complete_line_is_selected_and_nothing_is_left_out(self):
        matrix, left_out = self.select({(LINE, SHA): COMPLETE})
        self.assertEqual([row["ref"] for row in matrix["include"]], [LINE])
        self.assertEqual(left_out, [])

    def test_a_line_without_the_partition_script_is_left_out_and_named(self):
        tree = {k: v for k, v in COMPLETE.items() if k != "scripts/partition-check.py"}
        matrix, left_out = self.select({(LINE, SHA): tree})
        self.assertEqual(matrix["include"], [])
        self.assertEqual(left_out, [{"ref": LINE, "sha": SHA, "missing": ["no scripts/partition-check.py"]}])

    def test_a_line_with_another_runner_file_is_left_out_and_named(self):
        """Its run could not be credited, so it gets no run."""
        for runner in ("something else\n", None):
            with self.subTest(runner=runner):
                matrix, left_out = self.select({(LINE, SHA): {**COMPLETE, daily.RUNNER_PATH: runner}})
                self.assertEqual(matrix["include"], [])
                self.assertEqual(left_out[0]["missing"], [f"{daily.RUNNER_PATH} differs from main's"])

    def test_a_line_whose_script_lacks_a_runner_command_is_left_out_and_named(self):
        tree = {**COMPLETE, "scripts/partition-check.py": 'add_parser("check")\nadd_argument("--list")\n'}
        _, left_out = self.select({(LINE, SHA): tree})
        self.assertEqual(left_out[0]["missing"], ["scripts/partition-check.py has no --count",
                                                  "scripts/partition-check.py has no reports"])

    def test_a_line_whose_script_lacks_a_runner_flag_is_left_out_and_named(self):
        _, left_out = self.select({(LINE, SHA): {**COMPLETE, "scripts/journey-gate.py": "pass"}})
        self.assertEqual(left_out[0]["missing"], ["scripts/journey-gate.py has no --part"])

    def test_a_command_named_only_in_a_comment_does_not_count(self):
        tree = {**COMPLETE, "scripts/partition-check.py": '# check reports\nadd_argument("--count")\n'}
        _, left_out = self.select({(LINE, SHA): tree})
        self.assertIn("scripts/partition-check.py has no reports", left_out[0]["missing"])

    def test_main_is_always_selected_and_never_read(self):
        def read(sha, path):
            self.fail("main's tree was read")
        matrix, left_out = daily.select_active({"include": [{"ref": daily.MAIN, "sha": SHA}]}, NEEDS, read, RUNNER_TEXT)
        self.assertEqual([row["ref"] for row in matrix["include"]], [daily.MAIN])
        self.assertEqual(left_out, [])

    def test_a_failed_read_stops_discovery_and_never_reads_as_inactive(self):
        def read(sha, path):
            raise subprocess.CalledProcessError(128, "git")
        with self.assertRaises(subprocess.CalledProcessError):
            daily.select_active({"include": [{"ref": LINE, "sha": SHA}]}, NEEDS, read, RUNNER_TEXT)


class DailyCacheTests(unittest.TestCase):
    """A run's caches belong to its own ref. The daily run still saves none: the
    line's push runs keep them current, and it adds nothing worth saving."""

    def steps_using(self, action):
        pattern = rf"^      - uses: {re.escape(action)}@[^\n]*\n((?:        .*\n)*)"
        return re.findall(pattern, workflow("windows-daily-run"), re.M)

    def test_each_rust_cache_restores_and_never_saves(self):
        steps = self.steps_using("Swatinem/rust-cache")
        self.assertEqual(len(steps), 3)
        for step in steps:
            self.assertIn("save-if: false", step)
            self.assertNotIn("cache-on-failure", step)

    def test_node_uses_no_package_cache(self):
        steps = self.steps_using("actions/setup-node")
        self.assertEqual(len(steps), 2)
        for step in steps:
            self.assertIn("package-manager-cache: false", step)
            self.assertNotRegex(step, r"(?m)^\s*(cache|cache-dependency-path):")

    def test_a_new_action_in_a_daily_workflow_needs_a_decision_here(self):
        """Actions with no cache, or whose cache the two tests above pin. Artifacts
        belong to their own run."""
        known = {"actions/checkout", "actions/upload-artifact", "actions/download-artifact", "actions/setup-node",
                 "Swatinem/rust-cache", "taiki-e/install-action", "msys2/setup-msys2"}
        for name in ("windows-daily", "windows-daily-run", "windows-daily-verdict"):
            used = set(re.findall(r"uses: ([^@\s]+)", workflow(name)))
            self.assertLessEqual(used, known, name)

    def test_the_reporting_script_runs_isolated(self):
        for name in ("windows-daily", "windows-daily-run", "windows-daily-verdict"):
            for command in re.findall(r"^\s*run: (.*windows-daily\.py.*)$", workflow(name), re.M):
                self.assertRegex(command, r"^python3? -I -B ", command)


# A hook or a gate may set these; each journey names its own repository.
CLEAN_ENV = {name: value for name, value in os.environ.items() if not name.startswith("GIT_")}


class DiscoverJourneyTests(unittest.TestCase):
    """The real command against a real remote: listing, shallow fetch and tree reads."""

    def git(self, cwd, *args):
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "core.hooksPath=/dev/null",
                        *args], cwd=cwd, env=CLEAN_ENV, check=True, capture_output=True, text=True)

    def commit(self, origin, branch, files):
        self.git(origin, "checkout", "-q", "--orphan", branch)
        self.git(origin, "rm", "-rfq", "--ignore-unmatch", ".")
        for path, text in files.items():
            target = origin / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")
        self.git(origin, "add", "-A")
        self.git(origin, "commit", "-qm", "x", "--allow-empty")

    def discover(self, checkout, temp):
        out, summary = temp / "out", temp / "summary"
        result = subprocess.run([sys.executable, "-B", str(ROOT / "scripts/windows-daily.py"), "discover"], cwd=checkout,
                                text=True, capture_output=True,
                                env={**CLEAN_ENV, "GITHUB_OUTPUT": str(out), "GITHUB_STEP_SUMMARY": str(summary)})
        return result, out.read_text() if out.exists() else "", summary.read_text() if summary.exists() else ""

    def test_active_lines_run_and_the_others_are_named(self):
        with tempfile.TemporaryDirectory() as name:
            temp = Path(name).resolve()
            origin = temp / "origin"
            origin.mkdir()
            self.git(origin, "init", "-q", "-b", "start")
            whole = line_tree(daily.runner_needs(workflow("windows-daily-run")))
            behind = {**whole, "scripts/partition-check.py": 'add_parser("check")\n'}
            self.commit(origin, "main", {"README": "x"})
            self.commit(origin, "integration/whole", whole)
            self.commit(origin, "integration/behind", behind)
            self.commit(origin, "integration/old", {"README": "x"})
            folder = {key: text for key, text in whole.items() if key != "scripts/partition-check.py"}
            self.commit(origin, "integration/folder", {**folder, "scripts/partition-check.py/x": "{}"})
            self.git(origin, "checkout", "-q", "--orphan", "other")
            checkout = temp / "checkout"
            checkout.mkdir()
            self.git(checkout, "init", "-q")
            self.git(checkout, "remote", "add", "origin", f"file://{origin.as_posix()}")

            result, out, summary = self.discover(checkout, temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            matrix = json.loads(out.splitlines()[0].split("=", 1)[1])
            self.assertEqual([row["ref"] for row in matrix["include"]],
                             ["refs/heads/integration/whole", daily.MAIN])
            self.assertIn("Left out, no daily evidence and no daily coverage: not proven active", summary)
            self.assertRegex(summary, r"integration/behind` at `[0-9a-f]{40}`: .*partition-check.py has no reports")
            self.assertRegex(summary, r"integration/old` at `[0-9a-f]{40}`: .*differs from main's; no scripts/")
            requested = json.loads((checkout / daily.REQUESTED_FILE).read_text())
            self.assertEqual(requested["include"], matrix["include"])
            self.assertEqual(requested["runner_sha256"], daily.runner_hash(workflow("windows-daily-run")))
            self.assertEqual({row["ref"] for row in requested["left_out"]},
                             {f"refs/heads/integration/{name}" for name in ("behind", "old", "folder")})
            self.assertRegex(summary, r"integration/folder` at `[0-9a-f]{40}`: no scripts/partition-check.py")

    def test_an_unreadable_remote_fails_the_run_and_offers_no_lines(self):
        with tempfile.TemporaryDirectory() as name:
            temp = Path(name).resolve()
            checkout = temp / "checkout"
            checkout.mkdir()
            self.git(checkout, "init", "-q")
            self.git(checkout, "remote", "add", "origin", f"file://{(temp / 'gone').as_posix()}")
            result, out, _ = self.discover(checkout, temp)
            self.assertEqual(result.returncode, 1)
            self.assertEqual(out, "")


class DailyCoverageTests(unittest.TestCase):
    """What the daily run covers and skips, and how its list stays current. The
    refs are read from the remote on every run and nothing stores them."""

    def discover(self, **listing):
        with tempfile.TemporaryDirectory() as temp, contextlib.chdir(temp), mock.patch.object(
                daily, "gh_api", side_effect=OSError("no GitHub here")), mock.patch.object(
                daily.subprocess, "check_output", **listing) as remote, mock.patch.object(
                daily, "tree_reader", lambda: lambda sha, path: line_tree(daily.runner_needs(
                    workflow("windows-daily-run"))).get(path)), mock.patch.dict(
                os.environ, {"GITHUB_OUTPUT": str(Path(temp) / "out"),
                             "GITHUB_STEP_SUMMARY": str(Path(temp) / "summary")}), mock.patch.object(
                sys, "argv", ["windows-daily.py", "discover"]), mock.patch("builtins.print"):
            status = daily.main()
            written = Path(temp) / "out"
            return status, remote, written.read_text() if written.exists() else ""

    def test_the_list_comes_from_the_remote_on_every_run(self):
        status, remote, written = self.discover(return_value=f"{SHA}\trefs/heads/main\n{SHA}\t{LINE}\n")
        self.assertEqual(status, 0)
        self.assertEqual(remote.call_args.args[0], ["git", "ls-remote", "--heads", "origin", "main", "integration/*"])
        self.assertIn(LINE, written)
        discovery = jobs(workflow("windows-daily"))["sources"]
        for stored in ("actions/cache", "download-artifact", "vars.", "inputs."):
            self.assertNotIn(stored, discovery)

    def test_a_failed_listing_fails_the_run_and_offers_no_lines(self):
        status, _, written = self.discover(side_effect=subprocess.CalledProcessError(128, "git"))
        self.assertEqual(status, 1)
        self.assertEqual(written, "")

    def test_a_new_line_joins_and_a_deleted_line_leaves_on_the_next_run(self):
        main = f"{SHA}\trefs/heads/main\n"
        added = "refs/heads/integration/new-line"
        rows = lambda listing: [row["ref"] for row in daily.sources(listing)["include"]]
        self.assertEqual(rows(main + f"{SHA}\t{LINE}\n"), [LINE, "refs/heads/main"])
        self.assertEqual(rows(main + f"{SHA}\t{added}\n"), [added, "refs/heads/main"])

    def test_only_main_and_integration_refs_are_covered(self):
        others = ("refs/heads/task/TSK-1-x", "refs/heads/master", "refs/heads/integrationish",
                  "refs/heads/release/main", "refs/tags/integration/x", "refs/pull/1/head")
        listing = f"{SHA}\trefs/heads/main\n" + "".join(f"{SHA}\t{ref}\n" for ref in others)
        self.assertEqual([row["ref"] for row in daily.sources(listing)["include"]], ["refs/heads/main"])

    def test_the_dispatcher_has_these_jobs_and_conditions(self):
        scheduler = jobs(workflow("windows-daily"))
        self.assertEqual(set(scheduler), {"sources", "dispatch"})
        self.assertEqual(re.findall(r"^    if: (.*)$", scheduler["sources"], re.M),
                         ["${{ github.ref == 'refs/heads/main' }}"])
        self.assertEqual(re.findall(r"^    if: (.*)$", scheduler["dispatch"], re.M),
                         ["${{ needs.sources.outputs.runs == 'true' }}"])
        self.assertEqual(set(daily.sources(f"{SHA}\trefs/heads/main\n")["include"][0]), {"ref", "sha"})

    def test_a_run_skips_no_windows_or_coverage_job_and_has_no_tier(self):
        """Only the two readers of other jobs' files have a condition, and the verdict
        refuses their skip."""
        runner = jobs(workflow("windows-daily-run"))
        conditions = {name: re.findall(r"^    if: (.*)$", job, re.M) for name, job in runner.items()}
        self.assertEqual(conditions, {
            "windows-build": [], "windows-tests": [], "windows": [], "windows-release-journey": [],
            "windows-journeys": ["${{ always() && needs.windows-tests.result != 'skipped' }}"],
            "gates-coverage": [],
            "gates-coverage-report": ["${{ always() && needs.gates-coverage.result != 'skipped' }}"],
        })

    def test_a_selected_line_that_fails_its_build_fails_visibly(self):
        """The active test reads files; a selected line can still fail when it runs."""
        result = daily.judge({"ref": LINE, "sha": SHA}, REQUESTED_AT, [recorded_run(conclusion="failure")],
                             lambda run: recorded_jobs(), lambda sha: RUNNER_TEXT, RUNNER_HASH, total, 90.0)
        self.assertEqual(result["result"], "failure")
        self.assertIsNone(result["tested_sha"])

    def test_the_daily_run_is_windows_and_linux_coverage_only(self):
        runner = jobs(workflow("windows-daily-run"))
        for name in daily.WINDOWS_JOBS:
            self.assertIn("runs-on: windows-latest", runner[name])
        for name in daily.COVERAGE_JOBS:
            self.assertIn("runs-on: ubuntu-24.04", runner[name])
        self.assertEqual(set(runner), daily.WINDOWS_JOBS | daily.COVERAGE_JOBS)


class CoverageReaderTests(unittest.TestCase):
    """The reader credits a line only with a coverage total for the requested commit
    at or above main's floor, and the verdict reports it. The total comes from the
    line's run, so the reader checks it and holds it to main's floor."""

    def judge(self, coverage_of=total, floor=90.0, conclusion="success"):
        return daily.judge({"ref": LINE, "sha": SHA}, REQUESTED_AT, [recorded_run(conclusion=conclusion)],
                           lambda run: recorded_jobs(), lambda sha: RUNNER_TEXT, RUNNER_HASH, coverage_of, floor)

    def test_a_total_at_or_over_the_floor_is_credited_and_reported_with_its_commit(self):
        for covered in (900, 950):
            with self.subTest(covered=covered):
                result = self.judge(lambda run: total(run, covered=covered))
                self.assertEqual(result["result"], "credited")
                self.assertEqual(result["coverage"], {"lines": 1000, "covered": covered,
                                                      "percent": covered / 10, "sha": SHA})
                report = daily.verdict_report({"result": "success", "lines": [result]})
                self.assertIn(f"Linux line coverage {covered / 10:.2f} percent ({covered} of 1000 lines) "
                              f"measured at `{SHA}`; main's floor is 90 percent", report)

    def test_a_total_under_main_s_floor_fails_even_when_the_run_passed(self):
        """A line that lowered its own floor still passes its own report job."""
        result = self.judge(lambda run: {**total(run, covered=899), "floor": 50.0})
        self.assertEqual(result["result"], "failure")
        self.assertIn("89.90 percent is under main's 90 percent floor", result["reasons"][0])
        self.assertIsNone(result["tested_sha"])

    def test_the_floor_is_main_s_rust_coverage_floor(self):
        self.assertEqual(daily.main_floor(), 90.0)
        self.assertEqual(self.judge(lambda run: total(run, covered=899), floor=None)["result"], "failure")

    def test_a_missing_unreadable_malformed_or_other_commit_total_gets_no_credit(self):
        def fails(run):
            raise subprocess.CalledProcessError(1, "gh")

        for coverage_of, reason in (
                (None, "recorded no Linux coverage total"),
                (lambda run: None, "recorded no Linux coverage total"),
                (lambda run: [], "recorded no Linux coverage total"),
                (fails, "could be read"),
                (lambda run: {**total(run), "lines": 0}, "malformed"),
                (lambda run: {**total(run), "covered": 1001}, "malformed"),
                (lambda run: {**total(run), "covered": 950.0}, "malformed"),
                (lambda run: {**total(run), "lines": True}, "malformed"),
                (lambda run: {**total(run), "covered": -1}, "malformed"),
                (lambda run: total(run, sha="b" * 40), "not " + SHA),
                (lambda run: {**total(run), "sha": None}, "not " + SHA)):
            with self.subTest(reason=reason, coverage_of=coverage_of):
                result = self.judge(coverage_of)
                self.assertEqual(result["result"], "no-credit")
                self.assertIn(reason, result["reasons"][0])
                self.assertIsNone(result["tested_sha"])
                report = daily.verdict_report({"result": "failure", "lines": [result]})
                self.assertIn("no Linux coverage total recorded", report)

    def test_a_failed_run_still_reports_its_total(self):
        result = self.judge(lambda run: total(run, covered=850), conclusion="failure")
        self.assertEqual(result["result"], "failure")
        self.assertEqual(result["coverage"]["percent"], 85.0)

    def test_the_reader_downloads_the_total_from_the_run_by_its_artifact_name(self):
        def fake(command, **kwargs):
            folder = Path(command[command.index("--dir") + 1])
            (folder / daily.COVERAGE_FILE).write_text(json.dumps(total(7)), encoding="utf-8")
            return ""

        with mock.patch.object(daily.subprocess, "check_output", side_effect=fake) as called, \
                mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            self.assertEqual(DOWNLOAD(7), total(7))
        self.assertEqual(called.call_args.args[0], ["gh", "run", "download", "7", "--repo", "o/n", "--name",
                                                    daily.COVERAGE_ARTIFACT, "--dir", mock.ANY])

    def test_a_run_without_a_total_is_not_a_proof_for_a_skip(self):
        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            to_run, skipped = daily.skip_proven(
                {"include": [{"ref": LINE, "sha": SHA}]}, 7,
                lambda path: {"jobs": recorded_jobs()} if "/jobs?" in path else {"workflow_runs": [recorded_run()]},
                lambda sha, path: RUNNER_TEXT, RUNNER_HASH, REQUESTED_AT + timedelta(hours=1), lambda run: None)
        self.assertEqual((len(to_run["include"]), skipped), (1, []))

    def test_a_daily_coverage_result_is_never_merge_evidence(self):
        self.assertIn("never merge evidence", daily.verdict_report({"result": "success", "lines": []}))
        for name in ("ci-reuse.py", "gate-parity.py"):
            self.assertNotIn(daily.COVERAGE_ARTIFACT, (ROOT / "scripts" / name).read_text(encoding="utf-8"))
        self.assertNotIn(daily.COVERAGE_ARTIFACT, workflow("codeflow-ci"))


class LeftOutLineTests(unittest.TestCase):
    """A line left out is not green. The overall verdict can pass without it, but
    nothing may read that pass as a daily verdict for that line."""

    SHA_OUT = "b" * 40
    REQUEST = {"include": [{"ref": daily.MAIN, "sha": SHA}], "skipped": [],
               "left_out": [{"ref": LINE, "sha": "b" * 40, "missing": ["no scripts/partition-check.py"]}]}
    NOW = REQUESTED_AT + timedelta(hours=6)

    def evidence(self, request=None):
        dispatcher = {"id": 5, "path": daily.DISPATCHER_PATH, "head_branch": "main", "event": "schedule",
                      "status": "completed", "created_at": "2026-11-04T06:23:00Z"}

        def api(path):
            if "windows-daily.yml/runs" in path:
                return {"workflow_runs": [dispatcher]}
            if "/jobs?" in path:
                return {"jobs": recorded_jobs()}
            return {"workflow_runs": [recorded_run(head_branch="main")]}

        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            return daily.read_evidence(api, lambda run_id: {"runner_sha256": RUNNER_HASH, **(request or self.REQUEST)},
                                       lambda sha, path: RUNNER_TEXT, self.NOW)

    def test_the_overall_run_can_pass_with_a_line_left_out(self):
        self.assertEqual(self.evidence()["result"], "success")

    def test_a_left_out_line_has_its_own_row_saying_no_evidence_while_the_run_passes(self):
        result = self.evidence()
        self.assertEqual(result["result"], "success")
        self.assertEqual(result["lines"][1:], [{
            "ref": LINE, "requested_sha": self.SHA_OUT, "tested_sha": None, "run_id": None,
            "result": "no-evidence", "left_out": True, "reasons": ["no scripts/partition-check.py"]}])
        self.assertEqual(result["lines"][0]["result"], "credited")
        self.assertNotIn("left_out", result["lines"][0])

    def test_a_left_out_line_cannot_rescue_a_line_that_failed(self):
        no_run = {"include": [{"ref": daily.MAIN, "sha": "c" * 40}], "skipped": [], "left_out": self.REQUEST["left_out"]}
        result = self.evidence(no_run)
        self.assertEqual(result["result"], "failure")
        self.assertEqual([line["result"] for line in result["lines"]], ["no-credit", "no-evidence"])

    def test_nothing_reads_the_result_file_as_a_per_line_pass(self):
        """Only the reader writes the result and only its workflow keeps it, as an artifact."""
        found = set()
        for folder in (ROOT / "scripts", ROOT / ".github/workflows"):
            for path in folder.rglob("*"):
                if path.is_file() and "__pycache__" not in path.parts and b"daily-windows-result" in path.read_bytes():
                    found.add(path.relative_to(ROOT).as_posix())
        self.assertEqual(found, {"scripts/windows-daily.py", "scripts/test_windows_daily.py",
                                 ".github/workflows/windows-daily-verdict.yml"})
        keeping = [line.strip() for line in workflow("windows-daily-verdict").splitlines() if "daily-windows-result" in line]
        self.assertEqual(keeping, ["name: daily-windows-result", "path: daily-windows-result.json"])
        self.assertIn("actions/upload-artifact", workflow("windows-daily-verdict"))
        self.assertNotIn("actions/download-artifact", workflow("windows-daily-verdict"))

    def test_the_report_says_no_daily_evidence_for_that_line_and_limits_the_pass(self):
        report = daily.verdict_report(self.evidence())
        self.assertIn(f"`{LINE}` at `{self.SHA_OUT}`: left out, no daily evidence and no daily coverage\n"
                      "  - no scripts/", report)
        self.assertNotIn(f"measured at `{self.SHA_OUT}`", report)
        self.assertIn("It covers only the lines credited below.", report)
        self.assertNotIn(f"`{LINE}` at `{self.SHA_OUT}`: credited", report)

    def test_a_line_both_judged_and_left_out_is_refused(self):
        request = {"include": [{"ref": daily.MAIN, "sha": SHA}, {"ref": LINE, "sha": SHA}], "skipped": [],
                   "left_out": [{"ref": LINE, "sha": SHA, "missing": ["x"]}]}
        with self.assertRaisesRegex(ValueError, "both judges a line and leaves it out"):
            self.evidence(request)

    def test_a_malformed_left_out_row_is_refused(self):
        for row in ({"ref": LINE}, {"ref": "refs/heads/task/x", "sha": SHA}, {"ref": LINE, "sha": "bad"},
                    {"ref": LINE, "sha": SHA}):
            request = {"include": [{"ref": daily.MAIN, "sha": SHA}], "skipped": [], "left_out": [row]}
            with self.subTest(row=row), self.assertRaises((ValueError, KeyError)):
                self.evidence(request)

    def test_a_left_out_line_can_earn_no_reduced_tier_credit(self):
        """The tier and the proof that a run was full tier read nothing the daily run
        produces, so a daily pass can credit no line with them. The tier verdict
        still calls its own pass not full evidence, for any line."""
        readers = {
            "codeflow-ci.yml": workflow("codeflow-ci"),
            "ci-reuse.py": (ROOT / "scripts/ci-reuse.py").read_text(encoding="utf-8"),
            "gate-parity.py": (ROOT / "scripts/gate-parity.py").read_text(encoding="utf-8"),
        }
        for name, text in readers.items():
            for daily_output in ("daily-windows-result", "daily-windows-requested", "windows-daily.py",
                                 "windows-daily-verdict", "windows-daily-run", "workflows/windows-daily"):
                with self.subTest(reader=name, output=daily_output):
                    self.assertNotIn(daily_output, text)
        tier = jobs(workflow("codeflow-ci"))["windows-tier"]
        self.assertNotRegex(tier, r"(?i)\bdaily\b")
        self.assertNotIn("needs:", tier)
        self.assertNotIn("actions/", tier)
        self.assertNotRegex(tier, r"\bgh\b")
        verdict = jobs(workflow("codeflow-ci"))["windows-verdict"]
        self.assertIn("This is not Windows evidence", step_script(verdict, "Every Windows job the tier runs passed"))


class SkipTests(unittest.TestCase):
    """A line's exact tip with a passing run within the age gets no new run. Anything
    else runs it: a failed, missing, unfinished or unreadable run never skips."""

    NOW = datetime(2026, 11, 4, 6, 23, tzinfo=timezone.utc)
    MATRIX = {"include": [{"ref": LINE, "sha": SHA}]}

    def api(self, runs=None, job_rows=None):
        def call(path):
            if "/jobs?" in path:
                return {"jobs": recorded_jobs() if job_rows is None else job_rows}
            return {"workflow_runs": [self.run_at(days=1)] if runs is None else runs}
        return call

    def run_at(self, days, **changes):
        return recorded_run(created_at=(self.NOW - timedelta(days=days)).isoformat(), **changes)

    def skip(self, api=None, age=7, read=lambda sha, path: RUNNER_TEXT, matrix=None):
        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            return daily.skip_proven(matrix or self.MATRIX, age, api or self.api(), read, RUNNER_HASH, self.NOW)

    def assert_runs(self, result):
        self.assertEqual(result, (self.MATRIX, []))

    def test_a_recent_passing_run_at_the_exact_tip_skips(self):
        self.assertEqual(self.skip(), ({"include": []}, [{"ref": LINE, "sha": SHA, "run_id": 7}]))

    def test_a_run_inside_the_age_skips_and_one_past_it_reruns(self):
        self.assertEqual(len(self.skip(self.api([self.run_at(days=6)]))[1]), 1)
        self.assertEqual(len(self.skip(self.api([self.run_at(days=7)]))[1]), 1)
        self.assert_runs(self.skip(self.api([self.run_at(days=7, id=9) | {
            "created_at": (self.NOW - timedelta(days=7, seconds=1)).isoformat()}])))
        self.assert_runs(self.skip(self.api([self.run_at(days=30)])))

    def test_a_project_s_own_age_is_honoured(self):
        api = self.api([self.run_at(days=4)])
        self.assert_runs(self.skip(api, age=3))
        self.assertEqual(len(self.skip(api, age=5)[1]), 1)

    def test_a_failed_or_unfinished_run_never_skips(self):
        for changes in ({"conclusion": "failure"}, {"conclusion": "cancelled"}, {"conclusion": "timed_out"},
                        {"conclusion": None, "status": "in_progress"}, {"conclusion": None, "status": "queued"}):
            with self.subTest(changes=changes):
                self.assert_runs(self.skip(self.api([self.run_at(days=1, **changes)])))

    def test_a_newer_failure_beats_an_older_pass_at_the_same_tip(self):
        runs = [self.run_at(days=3), self.run_at(days=1, id=8, conclusion="failure")]
        self.assert_runs(self.skip(self.api(runs)))

    def test_no_run_at_all_never_skips(self):
        self.assert_runs(self.skip(self.api([])))

    def test_a_pass_at_another_commit_never_skips(self):
        self.assert_runs(self.skip(self.api([self.run_at(days=1, head_sha="b" * 40)])))

    def test_a_tampered_or_incomplete_record_never_skips(self):
        for changes in ({"path": ".github/workflows/other.yml"}, {"event": "push"}, {"head_branch": "integration/x"}):
            with self.subTest(changes=changes):
                self.assert_runs(self.skip(self.api([self.run_at(days=1, **changes)])))
        self.assert_runs(self.skip(self.api(job_rows=recorded_jobs()[1:])))
        self.assert_runs(self.skip(read=lambda sha, path: "another runner\n"))

    def test_an_unreadable_record_never_skips(self):
        def fails(error):
            def call(path):
                raise error
            return call

        for error in (subprocess.CalledProcessError(1, "gh"), OSError("gone"), subprocess.TimeoutExpired("gh", 1),
                      ValueError("not JSON")):
            with self.subTest(error=error):
                self.assert_runs(self.skip(fails(error)))
        self.assert_runs(self.skip(lambda path: {"unexpected": "shape"}))
        self.assert_runs(self.skip(self.api([{"id": 7}])))
        self.assert_runs(self.skip(self.api([self.run_at(days=1) | {"created_at": "not a date"}])))

        def jobs_fail(path):
            if "/jobs?" in path:
                raise OSError("gone")
            return {"workflow_runs": [self.run_at(days=1)]}
        self.assert_runs(self.skip(jobs_fail))

        def read_fails(sha, path):
            raise subprocess.CalledProcessError(128, "git")
        self.assert_runs(self.skip(read=read_fails))

    def test_a_missing_repository_name_never_skips(self):
        with mock.patch.dict(os.environ, clear=True):
            self.assert_runs(daily.skip_proven(self.MATRIX, 7, self.api(), lambda sha, path: RUNNER_TEXT,
                                               RUNNER_HASH, self.NOW))

    def test_an_age_that_is_absent_or_not_a_positive_whole_number_skips_nothing(self):
        for value in (None, "", "0", "-1", "7.5", "seven", "07", " 7", "99999"):
            with self.subTest(value=value):
                self.assertEqual(daily.max_age_days(value), 0)
                self.assert_runs(self.skip(age=daily.max_age_days(value)))
        self.assertEqual(daily.max_age_days("7"), 7)

    def test_each_line_is_decided_alone(self):
        other = "refs/heads/integration/other"
        matrix = {"include": [{"ref": LINE, "sha": SHA}, {"ref": other, "sha": SHA}]}

        def api(path):
            if "/jobs?" in path:
                return {"jobs": recorded_jobs()}
            if "integration%2Fexample" in path:
                return {"workflow_runs": [self.run_at(days=1)]}
            return {"workflow_runs": []}
        to_run, skipped = self.skip(api, matrix=matrix)
        self.assertEqual([row["ref"] for row in to_run["include"]], [other])
        self.assertEqual([row["ref"] for row in skipped], [LINE])

    def test_the_default_is_declared_once_and_the_script_holds_none(self):
        found = {name: workflow(name).count("DAILY_PROOF_MAX_AGE_DAYS: ")
                 for name in ("windows-daily", "windows-daily-run", "windows-daily-verdict")}
        self.assertEqual(found, {"windows-daily": 1, "windows-daily-run": 0, "windows-daily-verdict": 0})
        self.assertIn('DAILY_PROOF_MAX_AGE_DAYS: "7"', jobs(workflow("windows-daily"))["sources"])
        self.assertEqual(daily.MAX_AGE_VARIABLE, "DAILY_PROOF_MAX_AGE_DAYS")
        self.assertEqual(re.findall(r"timedelta\(days=\d+", (ROOT / "scripts/windows-daily.py").read_text()), [])

    def test_every_line_skipped_starts_no_run_and_says_so(self):
        job = jobs(workflow("windows-daily"))["dispatch"]
        self.assertIn("if: ${{ needs.sources.outputs.runs == 'true' }}", job)

    def discover(self, runs):
        with tempfile.TemporaryDirectory() as temp, contextlib.chdir(temp), mock.patch.object(
                daily.subprocess, "check_output", return_value=f"{SHA}\trefs/heads/main\n"), mock.patch.object(
                daily, "tree_reader", lambda: lambda sha, path: daily.RUNNER.read_text(encoding="utf-8")), mock.patch.object(
                daily, "gh_api", lambda path: {"jobs": recorded_jobs()} if "/jobs?" in path else {"workflow_runs": runs}), \
                mock.patch.dict(os.environ, {"GITHUB_OUTPUT": str(Path(temp) / "out"), "GITHUB_REPOSITORY": "o/n",
                                             "GITHUB_STEP_SUMMARY": str(Path(temp) / "summary"),
                                             "DAILY_PROOF_MAX_AGE_DAYS": "7"}), \
                mock.patch.object(sys, "argv", ["windows-daily.py", "discover"]), mock.patch("builtins.print"):
            self.assertEqual(daily.main(), 0)
            return ((Path(temp) / "out").read_text(), (Path(temp) / "summary").read_text(),
                    json.loads((Path(temp) / daily.REQUESTED_FILE).read_text()))

    def test_the_discover_command_records_and_names_a_skip(self):
        stamp = (datetime.now(timezone.utc) - timedelta(days=1)).isoformat()
        out, summary, request = self.discover([recorded_run(head_branch="main", created_at=stamp)])
        self.assertIn("runs=false", out)
        self.assertIn('matrix={"include":[]}', out)
        self.assertIn("Skipped: a passing run within 7 days", summary)
        self.assertEqual(request["include"], [])
        self.assertEqual(request["skipped"], [{"ref": daily.MAIN, "sha": SHA, "run_id": 7}])
        self.assertEqual(request["max_age_days"], 7)

    def test_the_discover_command_runs_a_line_with_no_record(self):
        out, summary, request = self.discover([])
        self.assertIn("runs=true", out)
        self.assertNotIn("Skipped", summary)
        self.assertEqual(request["include"], [{"ref": daily.MAIN, "sha": SHA}])


class SkippedLineVerdictTests(unittest.TestCase):
    """A skipped line stands on its recorded run, and the reader checks that run again."""

    NOW = REQUESTED_AT + timedelta(hours=6)

    def evidence(self, runs, request=None):
        dispatcher = {"id": 5, "path": daily.DISPATCHER_PATH, "head_branch": "main", "event": "schedule",
                      "status": "completed", "created_at": "2026-11-04T06:23:00Z"}

        def api(path):
            if "windows-daily.yml/runs" in path:
                return {"workflow_runs": [dispatcher]}
            if "/jobs?" in path:
                return {"jobs": recorded_jobs()}
            return {"workflow_runs": runs}

        request = request or {"include": [], "skipped": [{"ref": daily.MAIN, "sha": SHA, "run_id": 7}],
                              "left_out": [], "max_age_days": 7}
        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "o/n"}):
            return daily.read_evidence(api, lambda run_id: {"runner_sha256": RUNNER_HASH, **request},
                                       lambda sha, path: RUNNER_TEXT, self.NOW)

    def run_days_ago(self, days, **changes):
        return recorded_run(head_branch="main", created_at=(REQUESTED_AT - timedelta(days=days)).isoformat(), **changes)

    def test_a_skipped_line_with_its_run_inside_the_age_is_credited(self):
        result = self.evidence([self.run_days_ago(3)])
        self.assertEqual(result["result"], "success")
        self.assertTrue(result["lines"][0]["skipped"])
        self.assertIn("stands on run 7", daily.verdict_report(result))

    def test_a_skipped_line_whose_run_is_gone_fails_the_verdict(self):
        self.assertEqual(self.evidence([])["result"], "failure")

    def test_a_skipped_line_whose_run_is_past_the_age_fails_the_verdict(self):
        self.assertEqual(self.evidence([self.run_days_ago(8)])["result"], "failure")

    def test_a_skipped_line_with_a_newer_failure_fails_the_verdict(self):
        self.assertEqual(self.evidence([self.run_days_ago(3), self.run_days_ago(1, id=8, conclusion="failure")])["result"],
                         "failure")

    def test_a_skip_with_no_valid_age_recorded_is_not_credited(self):
        request = {"include": [], "skipped": [{"ref": daily.MAIN, "sha": SHA, "run_id": 7}], "left_out": []}
        self.assertEqual(self.evidence([self.run_days_ago(3)], request)["result"], "failure")


def step_script(job, step):
    """The bash script of one named `run: |` step, as the runner sees it."""
    lines = job.splitlines()
    at = lines.index(f"      - name: {step}")
    start = lines.index("        run: |", at) + 1
    script = []
    for line in lines[start:]:
        if line.strip() and not line.startswith(" " * 10):
            break
        script.append(line[10:])
    return "\n".join(script)


class TierWiringTests(unittest.TestCase):
    """Which jobs read the tier. scripts/test_windows_tier.py runs the tier and
    verdict steps themselves."""

    def test_only_a_pull_request_chooses_a_tier(self):
        job = jobs(workflow("codeflow-ci"))["windows-tier"]
        self.assertIn("if: github.event_name == 'pull_request'", job)

    def test_the_tier_moves_coverage_and_every_windows_job_and_a_push_keeps_them(self):
        ci = jobs(workflow("codeflow-ci"))
        for moved in ("gates-coverage", "windows-build", "windows", "windows-release-journey"):
            with self.subTest(job=moved):
                self.assertIn("!cancelled() && needs.windows-tier.outputs.tier != 'reduced'", ci[moved])
                self.assertIn("needs: [reuse, windows-tier]", ci[moved])
        self.assertIn("needs.gates-coverage.result != 'skipped'", ci["gates-coverage-report"])
        self.assertIn("needs.windows-build.result == 'success'", ci["windows-tests"])
        self.assertIn("needs.windows-tests.result != 'skipped'", ci["windows-journeys"])
        self.assertNotIn("windows-tier-journeys", ci)

    def test_the_daily_runner_has_no_tier(self):
        self.assertNotIn("tier", workflow("windows-daily-run").lower())


if __name__ == "__main__":
    unittest.main()
