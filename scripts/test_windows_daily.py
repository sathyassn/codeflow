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
        self.assertIn("run: python3 -I -B scripts/windows-daily.py discover", scheduler)
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
        self.assertIn("partition: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]", added["windows-tests"])
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


NEEDS = {"scripts/partition-check.py": {"check", "reports", "--count"}, "scripts/journey-gate.py": {"--part"}}
PARTITIONS = 'add_parser("check")\nadd_parser("reports")\nadd_argument("--count")\n'
COMPLETE = {"scripts/partition-check.py": PARTITIONS, "scripts/journey-gate.py": 'add_argument("--part")'}


def line_tree(needs):
    """A tree holding exactly what `needs` asks for."""
    text = {}
    for path, words in needs.items():
        text[path] = "\n".join(f'add_argument("{word}")' if word.startswith("--") else f'add_parser("{word}")'
                               for word in sorted(words)) + "\n"
    return text


class ActiveLineTests(unittest.TestCase):
    """A line is active when its own tree has what the runner calls."""

    def select(self, trees):
        listed = {"include": [{"ref": ref, "sha": sha, "key": "k"} for ref, sha in trees]}
        return daily.select_active(listed, NEEDS, lambda sha, path: trees[next(
            ref for ref, known in trees if known == sha), sha].get(path))

    def test_the_needs_are_read_from_the_runner_itself(self):
        needs = daily.runner_needs(workflow("windows-daily-run"))
        self.assertLessEqual({"reports", "check", "--count", "--prefix", "--results", "--list"},
                             needs["scripts/partition-check.py"])
        self.assertLessEqual({"--part", "--skip-benchmark", "--results"}, needs["scripts/journey-gate.py"])
        self.assertIn("scripts/test_gate_parity.py", needs)
        self.assertNotIn("scripts/windows-test-times.json", needs)
        self.assertNotIn(daily.RUNNER_OWN, needs)

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
        matrix, left_out = daily.select_active({"include": [{"ref": daily.MAIN, "sha": SHA, "key": "k"}]}, NEEDS, read)
        self.assertEqual([row["ref"] for row in matrix["include"]], [daily.MAIN])
        self.assertEqual(left_out, [])

    def test_a_failed_read_stops_discovery_and_never_reads_as_inactive(self):
        def read(sha, path):
            raise subprocess.CalledProcessError(128, "git")
        with self.assertRaises(subprocess.CalledProcessError):
            daily.select_active({"include": [{"ref": LINE, "sha": SHA, "key": "k"}]}, NEEDS, read)


class DailyCacheTests(unittest.TestCase):
    """A daily job runs a line's code under github.ref = main, so whatever it saved
    would land in main's cache scope, where main's own runs restore it."""

    def steps_using(self, action):
        pattern = rf"^      - uses: {re.escape(action)}@[^\n]*\n((?:        .*\n)*)"
        return re.findall(pattern, workflow("windows-daily-run"), re.M)

    def test_each_rust_cache_restores_and_never_saves(self):
        steps = self.steps_using("Swatinem/rust-cache")
        self.assertEqual(len(steps), 2)
        for step in steps:
            self.assertIn("save-if: false", step)
            self.assertNotIn("cache-on-failure", step)

    def test_node_uses_no_package_cache(self):
        steps = self.steps_using("actions/setup-node")
        self.assertEqual(len(steps), 1)
        self.assertIn("package-manager-cache: false", steps[0])
        self.assertNotRegex(steps[0], r"(?m)^\s*(cache|cache-dependency-path):")

    def test_a_new_action_in_a_daily_workflow_needs_a_decision_here(self):
        """Actions with no cache, or whose cache the two tests above pin. Artifacts
        belong to their own run."""
        known = {"actions/checkout", "actions/upload-artifact", "actions/download-artifact", "actions/setup-node",
                 "Swatinem/rust-cache", "taiki-e/install-action", "msys2/setup-msys2",
                 "./.github/workflows/windows-daily-run.yml"}
        for name in ("windows-daily", "windows-daily-run"):
            used = set(re.findall(r"uses: ([^@\s]+)", workflow(name)))
            self.assertLessEqual(used, known, name)

    def test_msys2_saves_before_any_line_code_runs(self):
        """setup-msys2 keeps its cache on, since it has no post step and so saves
        inside its own step. Only the identity check, from the scheduler's code, runs first."""
        job = jobs(workflow("windows-daily-run"))["windows-tests"]
        before = job[:job.index("uses: msys2/setup-msys2@")]
        commands = re.findall(r"^        run: (.*)$", before, re.M)
        self.assertEqual(len(commands), 1, commands)
        self.assertIn("../runner/scripts/windows-daily.py identity", commands[0])

    def test_the_reporting_script_runs_isolated(self):
        for name in ("windows-daily", "windows-daily-run"):
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
            matrix = json.loads(out.split("=", 1)[1])
            self.assertEqual([row["ref"] for row in matrix["include"]],
                             ["refs/heads/integration/whole", daily.MAIN])
            self.assertIn("Left out: not proven active", summary)
            self.assertRegex(summary, r"integration/behind` at `[0-9a-f]{40}`: .*partition-check.py has no reports")
            self.assertRegex(summary, r"integration/old` at `[0-9a-f]{40}`: no scripts/")
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
        with tempfile.TemporaryDirectory() as temp, mock.patch.object(
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

    def test_no_selected_line_is_skipped_for_its_age_or_an_unchanged_tip(self):
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

    def test_a_selected_line_that_fails_its_build_fails_visibly(self):
        """The active test reads files; a selected line can still fail when it runs."""
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


class TierWiringTests(unittest.TestCase):
    """Which jobs read the tier. scripts/test_windows_tier.py runs the tier and
    verdict steps themselves."""

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


if __name__ == "__main__":
    unittest.main()
