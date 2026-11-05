#!/usr/bin/env python3
"""Controls for the Windows tier and verdict steps (TSK-1321 AC-3 to AC-5).

Both steps live in .github/workflows/codeflow-ci.yml, and these tests run
them as the runner does: the tier step in bash in a real merge commit, the
verdict step in Python on the `needs` results GitHub would give it.
scripts/gate-parity.py pins both jobs, so the text tested here is the text
that runs.
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
REDUCED_TIER_PATHS = re.compile(TIER_ENV["REDUCED_TIER_PATHS"])
COMPILED_IN = re.compile(TIER_ENV["COMPILED_IN"])


def takes_reduced(path: str) -> bool:
    """Whether a pull request changing only this path may take the reduced tier."""
    return bool(REDUCED_TIER_PATHS.search(path)) and not COMPILED_IN.search(path)
BASH = shutil.which("bash")


def git(cwd: Path, *args: str) -> str:
    env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
           "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com", "GIT_CONFIG_NOSYSTEM": "1"}
    return subprocess.run(["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", *args], cwd=cwd,
                          env=env, check=True, capture_output=True, text=True).stdout.strip()


@unittest.skipIf(BASH is None or os.name == "nt", "the tier step runs in Linux bash")
class TierTests(unittest.TestCase):
    """AC-4 and AC-5: the tier comes from the merge commit's changes, read
    against the base and head commits GitHub gives, and fails closed."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.repo = Path(self.dir.name) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q")
        self.write({"crates/core/src/lib.rs": "fn a() {}\n", "project-management/tasks/TSK-1.md": "one\n",
                    "docs/guide.md": "guide\n"})
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "-m", "base")
        self.base = git(self.repo, "rev-parse", "HEAD")

    def write(self, files: dict[str, str | None]) -> None:
        for path, text in files.items():
            target = self.repo / path
            if text is None:
                target.unlink()
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")

    def merge(self, files: dict[str, str | None], renames: dict[str, str] | None = None) -> tuple[str, str]:
        """A head branch with these changes, merged into the base as GitHub's
        refs/pull/<n>/merge is: (head sha, merge sha), merge checked out."""
        git(self.repo, "checkout", "-q", "-b", "head", self.base)
        self.write(files)
        for old, new in (renames or {}).items():
            (self.repo / new).parent.mkdir(parents=True, exist_ok=True)
            git(self.repo, "mv", old, new)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "head")
        head = git(self.repo, "rev-parse", "HEAD")
        git(self.repo, "checkout", "-q", "--detach", self.base)
        git(self.repo, "merge", "-q", "--no-ff", "-m", "merge", head)
        return head, git(self.repo, "rev-parse", "HEAD")

    def tier(self, head: str, base_ref="integration/EPC-022-fixes", base_sha=None, switch="on", cwd=None,
             patterns=None):
        out = Path(self.dir.name) / "output"
        summary = Path(self.dir.name) / "summary"
        for path in (out, summary):
            path.write_text("", encoding="utf-8")
        env = {"PATH": os.environ["PATH"], "HOME": self.dir.name, "GIT_CONFIG_NOSYSTEM": "1",
               "REDUCED_TIER": switch, "BASE": base_ref, "BASE_SHA": base_sha or self.base, "HEAD_SHA": head,
               "REDUCED_TIER_PATHS": TIER_ENV["REDUCED_TIER_PATHS"], "COMPILED_IN": TIER_ENV["COMPILED_IN"],
               "GITHUB_OUTPUT": str(out), "GITHUB_STEP_SUMMARY": str(summary), **(patterns or {})}
        result = subprocess.run([BASH, "--noprofile", "--norc", "-eo", "pipefail", "-c", TIER_SCRIPT],
                                cwd=cwd or self.repo, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(out.read_text(encoding="utf-8").count("tier="), 1)
        return out.read_text(encoding="utf-8").strip().removeprefix("tier="), result.stdout

    def test_a_records_only_pull_request_takes_the_reduced_tier(self):
        head, _ = self.merge({"project-management/tasks/TSK-1.md": "two\n", "docs/guide.md": "more\n",
                              "changelog.d/TSK-9.md": "entry\n", "README.md": "read me\n"})
        tier, out = self.tier(head)
        self.assertEqual(tier, "reduced")
        self.assertIn("all of its 4 changed files are on the reduced-tier list", out)

    def test_every_path_off_the_list_takes_the_full_tier(self):
        # The list is short on purpose: a path nobody thought about, a new
        # top-level file or a workflow, takes the full tier like any code.
        for path in ("crates/core/src/new.rs", "Cargo.toml", "Cargo.lock", ".config/nextest.toml",
                     ".github/workflows/codeflow-ci.yml", ".github/workflows/release.yml", ".gitattributes",
                     "dist-workspace.toml", ".codex/config.toml", ".claude/settings.json", ".grok/hooks/x",
                     ".agents/skills/x/SKILL.md", "AGENTS.md", "CHANGELOG.md", "docs-portal/figures/x.json",
                     "docs/verification/evidence/x.json", "docs/guide.txt", "scripts/partition-check.py",
                     "scripts/release-notes.py", "assets/base/policy.json", "rust-toolchain.toml",
                     "crates/core/README.md", "changelog.d/sub/x.md", "brand-new-file"):
            with self.subTest(path=path):
                self.setUp()
                head, _ = self.merge({path: "changed\n", "project-management/tasks/TSK-1.md": "two\n"})
                tier, out = self.tier(head)
                self.assertEqual(tier, "full")
                self.assertIn(f"it changes {path} and 0 other path(s) outside the reduced-tier list", out)

    def test_a_listed_file_rust_compiles_in_takes_the_full_tier(self):
        for path in ("docs/coordination-ledger.md", "changelog.d/TSK-322.md", "changelog.d/TSK-1079.md"):
            with self.subTest(path=path):
                self.setUp()
                head, _ = self.merge({path: "changed\n", "docs/guide.md": "more\n"})
                tier, out = self.tier(head)
                self.assertEqual(tier, "full")
                self.assertIn(f"it changes {path}, which Rust compiles in", out)

    def test_a_grep_error_takes_the_full_tier(self):
        head, _ = self.merge({"docs/guide.md": "more\n"})
        for name in ("REDUCED_TIER_PATHS", "COMPILED_IN"):
            with self.subTest(pattern=name):
                tier, out = self.tier(head, patterns={name: "(["})
                self.assertEqual(tier, "full")
                self.assertIn("the path patterns could not be matched", out)

    def test_a_deleted_or_renamed_crate_file_takes_the_full_tier(self):
        head, _ = self.merge({"crates/core/src/lib.rs": None})
        self.assertEqual(self.tier(head)[0], "full")
        self.setUp()
        head, _ = self.merge({}, renames={"crates/core/src/lib.rs": "docs/lib.rs"})
        tier, out = self.tier(head)
        self.assertEqual(tier, "full")
        self.assertIn("crates/core/src/lib.rs", out)

    def test_other_bases_and_the_switch_off_keep_the_full_tier(self):
        head, _ = self.merge({"docs/guide.md": "more\n"})
        for base in ("main", "master", "feature/x"):
            with self.subTest(base=base):
                tier, out = self.tier(head, base_ref=base)
                self.assertEqual(tier, "full")
                self.assertIn(f"the base {base} is not an integration line", out)
        for base in ("task/integration/x", "integration", ""):
            with self.subTest(base=base):
                self.assertEqual(self.tier(head, base_ref=base)[0], "full")
        tier, out = self.tier(head, switch="off")
        self.assertEqual((tier, "the reduced tier switch is off" in out), ("full", True))
        for switch in ("", "ON", "true", "yes"):
            with self.subTest(switch=switch):
                self.assertEqual(self.tier(head, switch=switch)[0], "full")

    def test_parents_other_than_the_events_base_and_head_take_the_full_tier(self):
        head, _ = self.merge({"docs/guide.md": "more\n"})
        other = "0" * 40
        for kwargs in ({"base_sha": other}, {"head": other}):
            with self.subTest(**kwargs):
                tier, out = self.tier(kwargs.get("head", head), base_sha=kwargs.get("base_sha"))
                self.assertEqual(tier, "full")
                self.assertIn("are not the pull request's base", out)

    def test_a_tested_commit_that_is_not_the_merge_takes_the_full_tier(self):
        git(self.repo, "checkout", "-q", "-b", "head", self.base)
        self.write({"docs/guide.md": "more\n"})
        git(self.repo, "commit", "-q", "-am", "head")
        head = git(self.repo, "rev-parse", "HEAD")
        tier, out = self.tier(head)
        self.assertEqual(tier, "full")
        self.assertIn("are not the pull request's base", out)

    def test_an_unreadable_history_takes_the_full_tier(self):
        head, _ = self.merge({"docs/guide.md": "more\n"})
        outside = Path(self.dir.name) / "not-a-repo"
        outside.mkdir()
        tier, out = self.tier(head, cwd=outside)
        self.assertEqual(tier, "full")
        self.assertIn("the tested commit's parents cannot be read", out)

    def test_a_path_git_quotes_takes_the_full_tier(self):
        head, _ = self.merge({"docs/naïve.md": "x\n"})
        tier, out = self.tier(head)
        self.assertEqual(tier, "full")
        self.assertIn("a changed path holds characters git quotes", out)

    def test_an_empty_change_list_takes_the_full_tier(self):
        head, _ = self.merge({})
        tier, out = self.tier(head)
        self.assertEqual(tier, "full")
        self.assertIn("the diff lists no changed file", out)

    def test_a_head_side_file_cannot_select_the_reduced_tier(self):
        # The step reads only git objects and the event's base and head; a
        # file the head adds, whatever it says, is just one more changed path.
        files = {"crates/core/src/lib.rs": "fn b() {}\n", ".windows-tier": "reduced\n",
                 ".github/windows-tier.json": '{"tier": "reduced"}\n', "TIER": "tier=reduced\n"}
        head, _ = self.merge(files)
        self.assertEqual(self.tier(head)[0], "full")

    def test_the_step_reads_no_author_controlled_event_field(self):
        job = JOBS["windows-tier"]
        self.assertEqual(set(TIER_ENV), {"REDUCED_TIER", "BASE", "BASE_SHA", "HEAD_SHA", "REDUCED_TIER_PATHS",
                                         "COMPILED_IN"})
        for field in ("title", "body", "label", "head.ref", "head_ref", "user", "inputs"):
            self.assertNotIn(field, job.replace("head.sha", ""))
        self.assertIn("BASE_SHA: ${{ github.event.pull_request.base.sha }}", job)
        self.assertIn("HEAD_SHA: ${{ github.event.pull_request.head.sha }}", job)
        self.assertNotRegex(TIER_SCRIPT, r"\bcat\b|\bsource\b|python|\./")


class ReducedTierPathTests(unittest.TestCase):
    """AC-4: only the listed paths take the reduced tier, and nothing the
    skipped jobs run or the partitions compile is on the list."""

    def test_no_script_the_skipped_jobs_run_takes_the_reduced_tier(self):
        for job in ("windows-build", "windows-tests", "windows-journeys"):
            for script in set(re.findall(r"\bscripts/[\w.-]+", JOBS[job])):
                with self.subTest(job=job, script=script):
                    self.assertFalse(takes_reduced(script))

    def test_no_repository_file_the_rust_code_includes_takes_the_reduced_tier(self):
        found = set()
        for source in (ROOT / "crates").rglob("*.rs"):
            if "target" in source.parts:
                continue
            try:
                text = source.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError):
                continue
            for included in re.findall(r'include_(?:str|bytes)!\(\s*"([^"]+)"', text):
                resolved = Path(os.path.normpath(source.parent / included))
                found.add(resolved.relative_to(ROOT).as_posix())
        self.assertTrue({"docs/coordination-ledger.md", "changelog.d/TSK-322.md"} <= found, found)
        for path in sorted(found):
            with self.subTest(path=path):
                self.assertFalse(takes_reduced(path))

    def test_records_changelog_entries_and_docs_markdown_take_the_reduced_tier(self):
        for path in ("project-management/tasks/TSK-1321.md", "docs/architecture.md", "docs/decisions/0001-x.md",
                     "changelog.d/TSK-1321.md", "README.md"):
            with self.subTest(path=path):
                self.assertTrue(takes_reduced(path))


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
