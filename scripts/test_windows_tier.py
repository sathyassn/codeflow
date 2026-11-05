#!/usr/bin/env python3
"""Controls for the Windows tier and verdict steps (TSK-1321 AC-3 to AC-5).

Both steps live in .github/workflows/codeflow-ci.yml, and these tests run
them as the runner does: the tier step in bash with the event and base GitHub
gives it, the verdict step in Python on the `needs` results GitHub would give
it.
scripts/gate-parity.py pins both jobs, so the text tested here is the text
that runs.
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


def step(job: str, name: str) -> tuple[dict[str, str], str]:
    """(env, run script) of a named step, as the runner reads them."""
    lines = JOBS[job].splitlines()
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


TIER_ENV, TIER_SCRIPT = step("windows-tier", "Choose the Windows tier")
VERDICT_ENV, VERDICT_SCRIPT = step("windows-verdict", "Every Windows job the tier runs passed")
BASH = shutil.which("bash")


@unittest.skipIf(BASH is None or os.name == "nt", "the tier step runs in Linux bash")
class TierTests(unittest.TestCase):
    """AC-4 and AC-5: the tier follows the event and the base only (operator
    decision 2026-11-02, card 3), and anything unexpected takes the full tier."""

    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.folder = Path(folder.name)

    def tier(self, base="integration/EPC-022-fixes", event="pull_request", switch="on"):
        folder = self.folder
        out, summary = folder / "output", folder / "summary"
        for path in (out, summary):
            path.write_text("", encoding="utf-8")
        env = {"PATH": os.environ["PATH"], "HOME": str(folder), "REDUCED_TIER": switch, "EVENT": event,
               "BASE": base, "GITHUB_OUTPUT": str(out), "GITHUB_STEP_SUMMARY": str(summary)}
        result = subprocess.run([BASH, "--noprofile", "--norc", "-eo", "pipefail", "-c", TIER_SCRIPT], cwd=folder,
                                env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(out.read_text(encoding="utf-8").count("tier="), 1)
        self.assertIn(result.stdout.strip(), summary.read_text(encoding="utf-8"))
        return out.read_text(encoding="utf-8").strip().removeprefix("tier="), result.stdout

    def test_every_pull_request_into_an_integration_line_takes_the_reduced_tier(self):
        for base in ("integration/EPC-022-the-3-2-0-milestone-fixes", "integration/workspace", "integration/x/y"):
            with self.subTest(base=base):
                tier, out = self.tier(base)
                self.assertEqual(tier, "reduced")
                self.assertIn(f"it is a pull request into the integration line {base}", out)

    def test_a_pull_request_into_any_other_base_takes_the_full_tier(self):
        for base in ("main", "master", "feature/x", "task/integration/x", "integration", "integration/", "",
                     "Integration/x", " integration/x"):
            with self.subTest(base=base):
                tier, out = self.tier(base)
                self.assertEqual(tier, "full")
                self.assertIn(f"the base '{base}' is not an integration line", out)

    def test_every_other_event_takes_the_full_tier(self):
        for event in ("push", "workflow_dispatch", "schedule", "pull_request_target", ""):
            with self.subTest(event=event):
                tier, out = self.tier(event=event)
                self.assertEqual(tier, "full")
                self.assertIn(f"the event '{event}' is not a pull request", out)

    def test_the_switch_off_or_any_other_value_keeps_the_full_tier(self):
        for switch in ("off", "", "ON", "true", "yes"):
            with self.subTest(switch=switch):
                tier, out = self.tier(switch=switch)
                self.assertEqual(tier, "full")
                self.assertIn(f"the reduced tier switch is {switch}", out)

    def test_the_step_reads_only_the_event_and_base(self):
        # No checkout, no git and no file: nothing the head changes, and no
        # label, title or body, can reach the choice.
        job = JOBS["windows-tier"]
        self.assertEqual(set(TIER_ENV), {"REDUCED_TIER", "EVENT", "BASE"})
        self.assertIn("EVENT: ${{ github.event_name }}", job)
        self.assertIn("BASE: ${{ github.event.pull_request.base.ref }}", job)
        self.assertNotIn("actions/checkout", job)
        for field in ("title", "body", "label", "head", "user", "inputs"):
            self.assertNotIn(field, job)
        self.assertNotRegex(TIER_SCRIPT, r"\bgit\b|\bcat\b|\bsource\b|python|\./|<")


def needs(**results):
    """`toJSON(needs)` for the verdict: job id -> result, outputs as given."""
    out = {}
    for job, value in results.items():
        result, outputs = value if isinstance(value, tuple) else (value, {})
        out[job.replace("_", "-")] = {"result": result, "outputs": outputs}
    return out


FULL_PUSH = dict(reuse=("success", {"run": ""}), windows_tier="skipped", windows="success",
                 windows_build="success", windows_tests="success", windows_journeys=("success", {"missing": ""}),
                 windows_release_journey="success", windows_tier_journeys="skipped")
REDUCED_PR = dict(reuse="skipped", windows_tier=("success", {"tier": "reduced"}), windows="success",
                  windows_build="skipped", windows_tests="skipped", windows_journeys="skipped",
                  windows_release_journey="success", windows_tier_journeys="success")


class VerdictTests(unittest.TestCase):
    """AC-3: the verdict passes only when every job the tier runs succeeded
    and every skipped job is one the tier declares skipped, and names each
    job that is not green with its result."""

    def verdict(self, results: dict, event="push", base=""):
        env = {"PATH": os.environ.get("PATH", ""), "NEEDS": json.dumps(needs(**results)), "EVENT": event,
               "BASE": base, "GITHUB_SHA": "a" * 40, "GITHUB_SERVER_URL": "https://github.com",
               "GITHUB_REPOSITORY": "sathyassn/codeflow"}
        result = subprocess.run(["python3", "-I", "-c", VERDICT_SCRIPT], env=env, capture_output=True, text=True)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        return result.returncode, result.stdout

    def test_the_step_reads_the_results_of_every_job_it_needs(self):
        self.assertEqual(VERDICT_ENV["NEEDS"], "${{ toJSON(needs) }}")
        self.assertIn("shell: python3 -I {0}", JOBS["windows-verdict"])

    def test_the_full_tier_passes_on_a_push_and_on_a_pull_request_into_main(self):
        self.assertEqual(self.verdict(FULL_PUSH)[0], 0)
        pull = {**FULL_PUSH, "reuse": "skipped", "windows_tier": ("success", {"tier": "full"})}
        code, out = self.verdict(pull, event="pull_request", base="main")
        self.assertEqual(code, 0, out)
        self.assertIn("every Windows job passed", out)

    def test_the_reduced_tier_passes_on_a_pull_request_into_a_line(self):
        code, out = self.verdict(REDUCED_PR, event="pull_request", base="integration/EPC-022-fixes")
        self.assertEqual(code, 0, out)
        self.assertIn("This is not full Windows evidence", out)

    def test_any_other_outcome_of_a_job_the_reduced_tier_keeps_is_refused(self):
        for job in ("windows", "windows_release_journey", "windows_tier_journeys"):
            for state in ("skipped", "failure", "cancelled"):
                with self.subTest(job=job, state=state):
                    code, out = self.verdict({**REDUCED_PR, job: state}, "pull_request", "integration/x")
                    self.assertEqual(code, 1)
                    self.assertNotIn("not full Windows evidence", out)

    def test_a_missing_tier_on_a_skipped_build_is_refused(self):
        code, out = self.verdict({**REDUCED_PR, "windows_tier": ("success", {"tier": ""}),
                                  "windows_tier_journeys": "skipped"}, "pull_request", "integration/x")
        self.assertEqual(code, 1)
        self.assertIn("windows test build concluded skipped; the full tier runs it", out)

    def test_a_red_checks_job_is_named_as_the_checks(self):
        for results, event, base in ((FULL_PUSH, "push", ""), (REDUCED_PR, "pull_request", "integration/x")):
            with self.subTest(event=event):
                code, out = self.verdict({**results, "windows": "failure"}, event, base)
                self.assertEqual(code, 1)
                self.assertIn("::error::windows checks concluded failure", out)
                self.assertNotIn("reduced tier is only", out)

    def test_an_undeclared_skip_fails_and_is_named(self):
        code, out = self.verdict({**FULL_PUSH, "windows_tests": "skipped"})
        self.assertEqual(code, 1)
        self.assertIn("windows tests concluded skipped; the full tier runs it", out)
        code, out = self.verdict({**REDUCED_PR, "windows_release_journey": "skipped"}, "pull_request",
                                 "integration/x")
        self.assertEqual(code, 1)
        self.assertIn("windows release journey concluded skipped; the reduced tier runs it", out)

    def test_a_job_the_tier_skips_that_ran_fails(self):
        code, out = self.verdict({**REDUCED_PR, "windows_build": "success"}, "pull_request", "integration/x")
        self.assertEqual(code, 1)
        self.assertIn("windows test build concluded success; the tier declares it skipped", out)
        code, out = self.verdict({**FULL_PUSH, "windows_tier_journeys": "success"})
        self.assertEqual(code, 1)
        self.assertIn("windows tier journeys concluded success; the tier declares it skipped", out)

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

    def test_the_reduced_tier_is_refused_outside_a_pull_request_into_a_line(self):
        for event, base in (("push", ""), ("pull_request", "main")):
            with self.subTest(event=event, base=base):
                code, out = self.verdict(REDUCED_PR, event, base)
                self.assertEqual(code, 1)
                self.assertIn("the reduced tier is only for a pull request into an integration line", out)

    def test_a_failed_tier_job_fails_the_pull_request_verdict(self):
        pull = {**FULL_PUSH, "reuse": "skipped", "windows_tier": "failure"}
        code, out = self.verdict(pull, event="pull_request", base="integration/x")
        self.assertEqual(code, 1)
        self.assertIn("windows tier concluded failure", out)

    def test_a_new_job_the_verdict_needs_must_succeed(self):
        code, out = self.verdict({**FULL_PUSH, "windows_new_check": "skipped"})
        self.assertEqual(code, 1)
        self.assertIn("windows-new-check concluded skipped; the full tier runs it", out)

    def test_reuse_passes_only_when_every_job_was_skipped_and_a_run_is_named(self):
        skipped = {job: "skipped" for job in FULL_PUSH if job != "reuse"}
        code, out = self.verdict({**skipped, "reuse": ("success", {"run": "123"})})
        self.assertEqual(code, 0, out)
        self.assertIn("actions/runs/123", out)
        code, out = self.verdict({**skipped, "reuse": ("success", {"run": "123"}), "windows_tests": "success"})
        self.assertEqual(code, 1)
        self.assertIn("windows tests concluded success; the tier declares it skipped", out)
        self.assertEqual(self.verdict({**skipped, "reuse": ("success", {"run": "x1"})})[0], 1)

    def test_no_run_at_all_fails_clearly(self):
        skipped = {job: "skipped" for job in FULL_PUSH}
        code, out = self.verdict({**skipped, "windows_tier": ("success", {"tier": "full"})}, "pull_request",
                                 "feature/x")
        self.assertEqual(code, 1)
        self.assertIn("the Windows gates did not run", out)


if __name__ == "__main__":
    unittest.main(verbosity=1)
