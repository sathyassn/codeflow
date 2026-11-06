#!/usr/bin/env python3
"""Controls for the three verdict steps (TSK-1321 AC-3, AC-8, AC-14).

The steps live in .github/workflows/codeflow-ci.yml and codeflow-line-pr.yml,
and these tests run them as the runner does: `codeflow gates` and `codeflow
gates (line)` in bash, `windows` in Python on the `needs` results GitHub would
give it. scripts/gate-parity.py pins all three jobs, so the text tested here is
the text that runs. No verdict has a "not selected" pass: a pull request into
an integration line starts neither required name's workflow.
"""

import importlib.util
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("gate_parity", ROOT / "scripts" / "gate-parity.py")
parity = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(parity)
WORKFLOW = (ROOT / ".github" / "workflows" / "codeflow-ci.yml").read_text(encoding="utf-8")
JOBS = parity.workflow_jobs(WORKFLOW)
LINE_JOBS = parity.workflow_jobs((ROOT / ".github" / "workflows" / "codeflow-line-pr.yml").read_text(encoding="utf-8"))


def step(job: str, name: str, jobs: dict[str, str] = JOBS) -> tuple[dict[str, str], str]:
    """(env, run script) of a named step, as the runner reads them."""
    lines = jobs[job].splitlines()
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


VERDICT_ENV, VERDICT_SCRIPT = step("windows-verdict", "Every Windows job passed")
GATES_ENV, GATES_SCRIPT = step("gates-verdict", "Every part of the full gate passed")
LINE_ENV, LINE_SCRIPT = step("gates-line-verdict", "Every short check passed", LINE_JOBS)
BASH = shutil.which("bash")


def needs(**results):
    """`toJSON(needs)` for the verdict: job id -> result, outputs as given."""
    out = {}
    for job, value in results.items():
        result, outputs = value if isinstance(value, tuple) else (value, {})
        out[job.replace("_", "-")] = {"result": result, "outputs": outputs}
    return out


FULL_PUSH = dict(reuse=("success", {"run": ""}), windows="success", windows_build="success",
                 windows_tests="success", windows_journeys=("success", {"missing": ""}),
                 windows_release_journey="success")
FULL_PR = {**FULL_PUSH, "reuse": "skipped"}


class VerdictTests(unittest.TestCase):
    """AC-3: the verdict passes only when every Windows job succeeded, or a
    reuse named a run and every job was skipped, and names each job that is
    not green with its result."""

    def verdict(self, results: dict):
        env = {"PATH": os.environ.get("PATH", ""), "NEEDS": json.dumps(needs(**results)),
               "GITHUB_SHA": "a" * 40, "GITHUB_SERVER_URL": "https://github.com",
               "GITHUB_REPOSITORY": "sathyassn/codeflow"}
        result = subprocess.run(["python3", "-I", "-c", VERDICT_SCRIPT], env=env, capture_output=True, text=True)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        return result.returncode, result.stdout

    def test_the_step_reads_the_results_of_every_job_it_needs(self):
        self.assertEqual(VERDICT_ENV, {"NEEDS": "${{ toJSON(needs) }}"})
        self.assertIn("shell: python3 -I {0}", JOBS["windows-verdict"])

    def test_a_full_run_passes_on_a_push_and_on_a_pull_request_into_main(self):
        self.assertEqual(self.verdict(FULL_PUSH)[0], 0)
        code, out = self.verdict(FULL_PR)
        self.assertEqual(code, 0, out)
        self.assertIn("every Windows job passed", out)

    def test_windows_not_selected_is_never_a_pass(self):
        # Every Windows job skipped with no reuse is what a line pull request
        # would look like here. It fails: there is no "not selected" pass.
        skipped = {job: "skipped" for job in FULL_PUSH}
        code, out = self.verdict(skipped)
        self.assertEqual(code, 1, out)
        self.assertIn("the Windows gates did not run", out)
        self.assertNotIn("not selected", VERDICT_SCRIPT)

    def test_one_job_left_green_among_skips_fails(self):
        for job in ("windows", "windows_build", "windows_tests", "windows_journeys", "windows_release_journey"):
            with self.subTest(job=job):
                results = {name: "skipped" for name in FULL_PUSH}
                code, out = self.verdict({**results, job: "success"})
                self.assertEqual(code, 1, out)
                self.assertIn("concluded skipped; a full run needs it", out)

    def test_a_red_checks_job_is_named_as_the_checks(self):
        code, out = self.verdict({**FULL_PUSH, "windows": "failure"})
        self.assertEqual(code, 1)
        self.assertIn("::error::windows checks concluded failure", out)

    def test_an_undeclared_skip_fails_and_is_named(self):
        code, out = self.verdict({**FULL_PUSH, "windows_tests": "skipped"})
        self.assertEqual(code, 1)
        self.assertIn("windows tests concluded skipped; a full run needs it", out)
        code, out = self.verdict({**FULL_PR, "windows_release_journey": "skipped"})
        self.assertEqual(code, 1)
        self.assertIn("windows release journey concluded skipped; a full run needs it", out)

    def test_a_cancelled_partition_is_named_with_its_missing_report(self):
        code, out = self.verdict({**FULL_PUSH, "windows_tests": "cancelled",
                                  "windows_journeys": ("failure", {"missing": "3 11"})})
        self.assertEqual(code, 1)
        self.assertIn("windows tests concluded cancelled", out)
        self.assertIn("windows journeys concluded failure", out)
        self.assertIn("Windows test partition(s) 3 11 ended without a report", out)

    def test_every_job_that_is_not_green_is_named(self):
        code, out = self.verdict({**FULL_PUSH, "windows": "failure", "windows_build": "cancelled",
                                  "windows_release_journey": "failure"})
        self.assertEqual(code, 1)
        for name in ("windows checks concluded failure", "windows test build concluded cancelled",
                     "windows release journey concluded failure"):
            self.assertIn(name, out)

    def test_a_new_job_the_verdict_needs_must_succeed(self):
        code, out = self.verdict({**FULL_PUSH, "windows_new_check": "skipped"})
        self.assertEqual(code, 1)
        self.assertIn("windows-new-check concluded skipped; a full run needs it", out)

    def test_reuse_passes_only_when_every_job_was_skipped_and_a_run_is_named(self):
        skipped = {job: "skipped" for job in FULL_PUSH if job != "reuse"}
        code, out = self.verdict({**skipped, "reuse": ("success", {"run": "123"})})
        self.assertEqual(code, 0, out)
        self.assertIn("actions/runs/123", out)
        code, out = self.verdict({**skipped, "reuse": ("success", {"run": "123"}), "windows_tests": "success"})
        self.assertEqual(code, 1)
        self.assertIn("windows tests concluded success; a reused run skips it", out)
        self.assertEqual(self.verdict({**skipped, "reuse": ("success", {"run": "x1"})})[0], 1)


class BashVerdict(unittest.TestCase):
    SCRIPT = ""

    def verdict(self, values: dict):
        with tempfile.TemporaryDirectory() as folder:
            summary = Path(folder) / "summary"
            env = {"PATH": os.environ["PATH"], "HOME": folder, "GITHUB_SHA": "a" * 40,
                   "GITHUB_STEP_SUMMARY": str(summary), "GITHUB_SERVER_URL": "https://github.com",
                   "GITHUB_REPOSITORY": "sathyassn/codeflow", **values}
            done = subprocess.run([BASH, "--noprofile", "--norc", "-eo", "pipefail", "-c", self.SCRIPT], cwd=folder,
                                  env=env, capture_output=True, text=True)
            return done.returncode, done.stdout + done.stderr


@unittest.skipIf(BASH is None or os.name == "nt", "the verdict step runs in Linux bash")
class GatesVerdictTests(BashVerdict):
    """AC-8: `codeflow gates` needs the whole full gate, and has no pass with
    coverage not selected."""

    SCRIPT = GATES_SCRIPT
    FULL = {"PARTS": "success", "RELEASE": "skipped", "SHARDS": "success", "REPORT": "success",
            "REUSE": "skipped", "REUSED": ""}

    def test_the_step_reads_no_event_base_or_tier(self):
        self.assertEqual(set(GATES_ENV), set(self.FULL))
        self.assertNotIn("not selected", GATES_SCRIPT)

    def test_a_full_run_needs_every_part(self):
        self.assertEqual(self.verdict(self.FULL)[0], 0)
        for key in ("PARTS", "SHARDS", "REPORT"):
            for state in ("skipped", "failure", "cancelled"):
                with self.subTest(key=key, state=state):
                    code, out = self.verdict({**self.FULL, key: state})
                    self.assertEqual(code, 1, out)

    def test_parts_passed_with_coverage_not_selected_fails(self):
        # What a line pull request's short checks would give this verdict.
        code, out = self.verdict({**self.FULL, "SHARDS": "skipped", "REPORT": "skipped"})
        self.assertEqual(code, 1, out)
        self.assertIn("the full gate concluded parts=success coverage shards=skipped", out)


@unittest.skipIf(BASH is None or os.name == "nt", "the verdict step runs in Linux bash")
class LineVerdictTests(BashVerdict):
    """AC-8, AC-14: `codeflow gates (line)` passes only when the build and
    every short-check part passed, and says it is not full gate evidence."""

    SCRIPT = LINE_SCRIPT
    PASSED = {"BUILD": "success", "PARTS": "success"}

    def test_the_step_reads_only_the_two_results(self):
        self.assertEqual(LINE_ENV, {"BUILD": "${{ needs.gates-build.result }}", "PARTS": "${{ needs.gates.result }}"})

    def test_the_short_checks_passing_passes_and_is_not_full_evidence(self):
        code, out = self.verdict(self.PASSED)
        self.assertEqual(code, 0, out)
        self.assertIn("This is not full gate evidence", out)

    def test_any_other_result_fails(self):
        for key in self.PASSED:
            for state in ("skipped", "failure", "cancelled", ""):
                with self.subTest(key=key, state=state):
                    code, out = self.verdict({**self.PASSED, key: state})
                    self.assertEqual(code, 1, out)
                    self.assertNotIn("This is not full gate evidence", out)


if __name__ == "__main__":
    unittest.main(verbosity=1)
