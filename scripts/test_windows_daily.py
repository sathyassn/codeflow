#!/usr/bin/env python3
"""Daily Windows source selection, exact-SHA verdict and workflow controls.

Before the operator applies the workflow patch, set WINDOWS_DAILY_PROPOSALS
to target/task-evidence to check the proposed workflow copies instead.
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
    if proposals := os.environ.get("WINDOWS_DAILY_PROPOSALS"):
        return (Path(proposals) / f"{name}.proposed.yml").read_text()
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


if __name__ == "__main__":
    unittest.main()
