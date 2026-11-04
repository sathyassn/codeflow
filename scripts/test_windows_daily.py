#!/usr/bin/env python3
"""Daily Windows source selection, exact-SHA verdict, workflow controls, and
the reduced Windows tier's switch and verdict, run from the workflow's own text.
"""

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


def successful(sha=SHA):
    return {name: {"result": "success", "outputs": {"sha": sha}} for name in daily.WINDOWS_JOBS}


class SourceTests(unittest.TestCase):
    def test_main_and_every_integration_ref_are_resolved_once(self):
        refs = f"{SHA}\trefs/heads/main\n{'b' * 40}\t{LINE}\n{SHA}\trefs/heads/task/example\n"
        selected = daily.sources(refs)["include"]
        self.assertEqual([(s["ref"], s["sha"]) for s in selected], [(LINE, "b" * 40), ("refs/heads/main", SHA)])

    def test_new_lines_are_discovered_without_a_name_allowlist(self):
        refs = f"{SHA}\trefs/heads/main\n{SHA}\trefs/heads/integration/new-line\n"
        self.assertEqual(len(daily.sources(refs)["include"]), 2)

    def test_same_sha_on_two_lines_has_separate_artifacts(self):
        selected = daily.sources(f"{SHA}\trefs/heads/main\n{SHA}\t{LINE}\n")["include"]
        self.assertNotEqual(selected[0]["key"], selected[1]["key"])
        self.assertTrue(all(re.fullmatch(r"[0-9a-f]{64}", s["key"]) for s in selected))

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


class VerdictTests(unittest.TestCase):
    def test_all_windows_jobs_at_the_requested_sha_pass(self):
        result = daily.verdict(LINE, SHA, successful())
        self.assertEqual(result["result"], "success")
        self.assertEqual(result["tested_sha"], SHA)
        self.assertEqual(result["ref"], LINE)

    def test_every_skipped_failed_cancelled_or_missing_job_refuses(self):
        for name in daily.WINDOWS_JOBS:
            for state in ("skipped", "failure", "cancelled", None):
                needs = successful()
                needs[name]["result"] = state
                with self.subTest(name=name, state=state):
                    self.assertEqual(daily.verdict(LINE, SHA, needs)["result"], "failure")

    def test_older_sha_cannot_pass_for_a_newer_requested_tip(self):
        result = daily.verdict(LINE, "b" * 40, successful())
        self.assertEqual(result["result"], "failure")
        self.assertIsNone(result["tested_sha"])
        self.assertEqual(result["requested_sha"], "b" * 40)

    def test_missing_identity_and_changed_dependency_set_refuse(self):
        for mutation in (lambda n: n.pop("windows"),
                         lambda n: n.update(extra={"result": "success", "outputs": {"sha": SHA}}),
                         lambda n: n["windows"]["outputs"].clear()):
            needs = successful()
            mutation(needs)
            self.assertEqual(daily.verdict(LINE, SHA, needs)["result"], "failure")

    def test_checkout_must_match_the_discovered_commit(self):
        self.assertEqual(daily.checked_out(LINE, SHA, SHA), SHA)
        with self.assertRaisesRegex(ValueError, "no tests were credited"):
            daily.checked_out(LINE, SHA, "b" * 40)

    def test_malformed_job_records_refuse(self):
        for needs in ([], {"windows": None}, {"windows": {"outputs": None}}):
            with self.subTest(needs=needs), self.assertRaises(ValueError):
                daily.verdict(LINE, SHA, needs)

    def test_verdict_command_writes_the_exact_source_to_summary_and_artifact(self):
        for sha, status in ((SHA, 0), ("b" * 40, 1)):
            with tempfile.TemporaryDirectory() as temp, self.subTest(sha=sha):
                summary = Path(temp) / "summary.md"
                result = subprocess.run(
                    [sys.executable, "-B", str(ROOT / "scripts/windows-daily.py"),
                     "verdict", "--ref", LINE, "--sha", sha], cwd=temp, text=True,
                    env={**os.environ, "DAILY_WINDOWS_NEEDS": json.dumps(successful()),
                         "GITHUB_STEP_SUMMARY": str(summary)}, capture_output=True,
                )
                self.assertEqual(result.returncode, status, result.stderr)
                report = json.loads((Path(temp) / "daily-windows-result.json").read_text())
                self.assertEqual(report["requested_sha"], sha)
                self.assertEqual(report["tested_sha"], SHA if status == 0 else None)
                self.assertIn(LINE, summary.read_text())
                self.assertIn(sha, summary.read_text())
                self.assertIn("no evidence for a newer branch tip", summary.read_text())


def workflow(name):
    return (ROOT / ".github/workflows" / f"{name}.yml").read_text()


def jobs(text):
    text = text.split("\njobs:\n", 1)[1]
    return {m.group(1): m.group(2) for m in re.finditer(
        r"^  ([\w-]+):\n(.*?)(?=^  [\w-]+:\n|\Z)", text, re.M | re.S)}


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
        if "../runner/scripts/windows-daily.py identity" not in command:
            found.append(command.replace("$GITHUB_WORKSPACE/source", "$GITHUB_WORKSPACE")
                         .replace("${GITHUB_WORKSPACE}/source", "${GITHUB_WORKSPACE}"))
    return found


class WorkflowTests(unittest.TestCase):
    def test_schedule_discovers_lines_and_calls_the_runner_without_write_permissions(self):
        scheduler = workflow("windows-daily")
        self.assertIn('cron: "23 6 * * *"', scheduler)
        self.assertIn("run: python3 -B scripts/windows-daily.py discover", scheduler)
        self.assertIn("matrix: ${{ fromJSON(needs.sources.outputs.matrix) }}", scheduler)
        self.assertIn("uses: ./.github/workflows/windows-daily-run.yml", scheduler)
        self.assertIn("source-sha: ${{ matrix.sha }}", scheduler)
        self.assertIn("source-ref: ${{ matrix.ref }}", scheduler)
        self.assertNotIn("write", scheduler)
        self.assertIn("fail-fast: false", scheduler)

    def test_daily_jobs_execute_the_full_existing_windows_commands(self):
        existing = jobs((ROOT / ".github/workflows/codeflow-ci.yml").read_text())
        added = jobs(workflow("windows-daily-run"))
        self.assertEqual(set(added), daily.WINDOWS_JOBS | {"verdict"})
        for name in daily.WINDOWS_JOBS:
            with self.subTest(job=name):
                self.assertEqual(commands(added[name]), commands(existing[name]))
                self.assertIn("runs-on: windows-latest", added[name])
                self.assertIn("working-directory: source", added[name])
                self.assertIn("ref: ${{ inputs.source-sha }}\n          path: source", added[name])
                self.assertIn("id: identity", added[name])
                self.assertIn("sha: ${{ steps.identity.outputs.sha }}", added[name])
                self.assertEqual(added[name].count("persist-credentials: false"), 2)
        self.assertIn("partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]", added["windows-tests"])
        self.assertIn("fail-fast: false", added["windows-tests"])

    def test_daily_verdict_runs_after_failures_and_reads_all_job_identities(self):
        job = jobs(workflow("windows-daily-run"))["verdict"]
        self.assertIn("if: ${{ always() }}", job)
        self.assertIn("DAILY_WINDOWS_NEEDS: ${{ toJSON(needs) }}", job)
        self.assertIn('verdict --ref "$TESTED_REF" --sha "$TESTED_SHA"', job)
        self.assertIn("at ${{ inputs.source-sha }}", job)
        dependencies = re.search(r"needs: \[([^\]]+)\]", job).group(1)
        self.assertEqual(set(dependencies.split(", ")), daily.WINDOWS_JOBS)

    def test_artifacts_are_namespaced_by_the_discovered_ref_and_sha(self):
        runner = workflow("windows-daily-run")
        for line in runner.splitlines():
            if re.match(r"          (?:name|pattern):", line):
                self.assertIn("daily-${{ inputs.artifact-key }}-", line)
        self.assertIn("pattern: daily-${{ inputs.artifact-key }}-windows-junit-*", runner)
        self.assertIn("path: source/target/nextest/partitions", runner)
        self.assertIn("path: source/target/windows-tests.tar.zst", runner)
        self.assertNotIn("actions: write", runner)


class DailyCoverageTests(unittest.TestCase):
    """What the daily run covers and skips, and how its list stays current. A
    line is active while its ref exists: the list is read from the remote on
    every run and nothing stores it."""

    def discover(self, **listing):
        with tempfile.TemporaryDirectory() as temp, mock.patch.object(
                daily.subprocess, "check_output", **listing) as remote, mock.patch.dict(
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

    def test_an_old_or_planning_line_stays_while_its_ref_exists(self):
        refs = ["refs/heads/integration/release-3-0-0-r3", "refs/heads/integration/planning-only"]
        listing = f"{SHA}\trefs/heads/main\n" + "".join(f"{'b' * 40}\t{ref}\n" for ref in refs)
        self.assertEqual({row["ref"] for row in daily.sources(listing)["include"]} - {"refs/heads/main"}, set(refs))

    def test_only_main_and_integration_refs_are_covered(self):
        others = ("refs/heads/task/TSK-1-x", "refs/heads/master", "refs/heads/integrationish",
                  "refs/heads/release/main", "refs/tags/integration/x", "refs/pull/1/head")
        listing = f"{SHA}\trefs/heads/main\n" + "".join(f"{SHA}\t{ref}\n" for ref in others)
        self.assertEqual([row["ref"] for row in daily.sources(listing)["include"]], ["refs/heads/main"])

    def test_no_line_is_skipped_for_its_tip_age_or_name(self):
        """An unchanged tip runs again: nothing compares it with an earlier run."""
        scheduler = jobs(workflow("windows-daily"))
        self.assertEqual(set(scheduler), {"sources", "lines"})
        for name, job in scheduler.items():
            self.assertNotIn("if:", job, name)
        script = (ROOT / "scripts/windows-daily.py").read_text()
        for memory in ("actions/runs", "api.github.com", '"gh"'):
            self.assertNotIn(memory, script)

    def test_a_run_skips_no_windows_job_and_has_no_tier(self):
        """Only the journey reader has a condition, and the verdict refuses its skip."""
        runner = jobs(workflow("windows-daily-run"))
        conditions = {name: re.findall(r"^    if: (.*)$", job, re.M) for name, job in runner.items()}
        self.assertEqual(conditions, {
            "windows-build": [], "windows-tests": [], "windows": [], "windows-release-journey": [],
            "windows-journeys": ["${{ always() && needs.windows-tests.result != 'skipped' }}"],
            "verdict": ["${{ always() }}"],
        })

    def test_a_line_too_old_for_the_runner_fails_and_never_passes(self):
        """The commands come from the scheduler and the scripts from the line, so a
        line without a command's script fails its build and skips what follows."""
        needs = successful()
        needs["windows-build"] = {"result": "failure", "outputs": {"sha": SHA}}
        for name in ("windows-tests", "windows-journeys"):
            needs[name] = {"result": "skipped", "outputs": {}}
        result = daily.verdict(LINE, SHA, needs)
        self.assertEqual(result["result"], "failure")
        self.assertIsNone(result["tested_sha"])

    def test_the_daily_run_is_windows_only(self):
        runner = jobs(workflow("windows-daily-run"))
        for name in daily.WINDOWS_JOBS:
            self.assertIn("runs-on: windows-latest", runner[name])
        self.assertEqual(set(runner) - daily.WINDOWS_JOBS, {"verdict"})


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


SWITCH = re.compile(r'REDUCED_TIER: "(on|off)"')


class TierSwitchTests(unittest.TestCase):
    """AC-5: with the switch on a pull request into a line takes the reduced
    tier; with it off, and for any other base, every partition stays."""

    def tier(self, switch, base):
        job = jobs(workflow("codeflow-ci"))["windows-tier"]
        self.assertRegex(job, SWITCH)
        script = step_script(job, "Choose the Windows tier")
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "output"
            done = subprocess.run(["bash", "-e", "-c", script], text=True, capture_output=True, env={
                "PATH": os.environ["PATH"], "REDUCED_TIER": switch, "BASE": base, "GITHUB_OUTPUT": str(output)})
            self.assertEqual(done.returncode, 0, done.stderr)
            return output.read_text().strip()

    def test_switch_on_reduces_only_a_pull_request_into_an_integration_line(self):
        self.assertEqual(self.tier("on", "integration/EPC-022-x"), "tier=reduced")
        for base in ("main", "master", "task/integration/x", "integration", ""):
            with self.subTest(base=base):
                self.assertEqual(self.tier("on", base), "tier=full")

    def test_switch_off_keeps_every_partition_on_every_base(self):
        for base in ("integration/EPC-022-x", "main", "master"):
            with self.subTest(base=base):
                self.assertEqual(self.tier("off", base), "tier=full")

    def test_any_other_switch_value_keeps_every_partition(self):
        for switch in ("", "ON", "true", "yes"):
            with self.subTest(switch=switch):
                self.assertEqual(self.tier(switch, "integration/EPC-022-x"), "tier=full")

    def test_only_a_pull_request_chooses_a_tier(self):
        job = jobs(workflow("codeflow-ci"))["windows-tier"]
        self.assertIn("if: github.event_name == 'pull_request'", job)

    def test_the_skip_comes_from_the_build_job_and_a_push_keeps_it_running(self):
        ci = jobs(workflow("codeflow-ci"))
        self.assertIn("needs.windows-tier.outputs.tier != 'reduced'", ci["windows-build"])
        self.assertIn("!cancelled()", ci["windows-build"])
        self.assertIn("needs: [reuse, windows-tier]", ci["windows-build"])
        self.assertIn("needs.windows-build.result == 'success'", ci["windows-tests"])
        self.assertIn("needs.windows-tests.result != 'skipped'", ci["windows-journeys"])
        for kept in ("windows", "windows-release-journey"):
            self.assertNotIn("windows-tier", ci[kept])
        self.assertIn("needs.windows-tier.outputs.tier == 'reduced'", ci["windows-tier-journeys"])

    def test_the_daily_runner_has_no_tier(self):
        self.assertNotIn("tier", workflow("windows-daily-run").lower())


class TierVerdictTests(unittest.TestCase):
    """AC-4 and AC-5: the `windows` verdict accepts exactly the reduced tier's
    declared skips, refuses any other skip, and its summary says so."""

    SHA = "c" * 40

    def verdict(self, **changes):
        job = jobs(workflow("codeflow-ci"))["windows-verdict"]
        script = step_script(job, "Every Windows job passed")
        env = {"REUSE": "skipped", "REUSED": "", "TIER": "reduced", "EVENT": "pull_request",
               "BASE": "integration/EPC-022-x", "CHECKS": "success", "BUILD": "skipped", "TESTS": "skipped",
               "JOURNEYS": "skipped", "RELEASE": "success", "TIER_JOURNEYS": "success", "MISSING": ""}
        env.update(changes)
        with tempfile.TemporaryDirectory() as temp:
            summary = Path(temp) / "summary.md"
            summary.touch()
            done = subprocess.run(["bash", "-e", "-c", script], text=True, capture_output=True, env={
                **env, "PATH": os.environ["PATH"], "GITHUB_SHA": self.SHA, "GITHUB_STEP_SUMMARY": str(summary),
                "GITHUB_SERVER_URL": "https://example.invalid", "GITHUB_REPOSITORY": "o/n"})
            return done.returncode, done.stdout, summary.read_text()

    def test_the_declared_skips_pass_and_the_summary_names_the_commit_and_the_limit(self):
        code, _, summary = self.verdict()
        self.assertEqual(code, 0)
        self.assertIn(self.SHA, summary)
        self.assertIn("twelve test partitions were skipped by tier", summary)
        self.assertIn("not full Windows evidence", summary)

    def test_any_other_skip_or_outcome_is_refused(self):
        for name in ("CHECKS", "RELEASE", "TIER_JOURNEYS"):
            for state in ("skipped", "failure", "cancelled", ""):
                with self.subTest(name=name, state=state):
                    code, out, summary = self.verdict(**{name: state})
                    self.assertEqual(code, 1)
                    self.assertIn("reduced Windows tier accepts only", out)
                    self.assertEqual(summary, "")

    def test_a_partition_path_job_that_ran_is_not_a_declared_skip(self):
        for name in ("BUILD", "TESTS", "JOURNEYS"):
            for state in ("success", "failure", "cancelled"):
                with self.subTest(name=name, state=state):
                    self.assertEqual(self.verdict(**{name: state})[0], 1)

    def test_the_reduced_tier_is_refused_on_main_and_on_a_push(self):
        for changes in ({"BASE": "main"}, {"BASE": "master"}, {"BASE": ""}, {"EVENT": "push"},
                        {"EVENT": "schedule"}, {"EVENT": "workflow_dispatch"}):
            with self.subTest(changes=changes):
                self.assertEqual(self.verdict(**changes)[0], 1)

    def test_the_full_tier_still_needs_every_job(self):
        full = {"TIER": "full", "BUILD": "success", "TESTS": "success", "JOURNEYS": "success",
                "TIER_JOURNEYS": "skipped"}
        self.assertEqual(self.verdict(**full)[0], 0)
        for name in ("CHECKS", "BUILD", "TESTS", "JOURNEYS", "RELEASE"):
            for state in ("skipped", "failure", "cancelled"):
                with self.subTest(name=name, state=state):
                    self.assertEqual(self.verdict(**{**full, name: state})[0], 1)

    def test_the_tier_journey_job_is_refused_outside_the_reduced_tier(self):
        full = {"TIER": "full", "BUILD": "success", "TESTS": "success", "JOURNEYS": "success"}
        for state in ("success", "failure", "cancelled", ""):
            with self.subTest(state=state):
                self.assertEqual(self.verdict(**{**full, "TIER_JOURNEYS": state})[0], 1)

    def test_a_missing_tier_on_a_skipped_build_is_refused(self):
        self.assertEqual(self.verdict(TIER="", TIER_JOURNEYS="skipped")[0], 1)

    def test_reuse_still_needs_every_windows_job_skipped(self):
        reused = {"TIER": "", "REUSE": "success", "REUSED": "123", "CHECKS": "skipped", "RELEASE": "skipped",
                  "TIER_JOURNEYS": "skipped", "EVENT": "push", "BASE": ""}
        self.assertEqual(self.verdict(**reused)[0], 0)
        self.assertEqual(self.verdict(**{**reused, "TIER_JOURNEYS": "success"})[0], 1)


if __name__ == "__main__":
    unittest.main()
