#!/usr/bin/env python3
"""No run under main's ref executes code from another ref.

Caches are scoped by the ref a run is under, and every job can write to its
run's own scope. A job under main that runs another ref's code could therefore
write what main's runs later restore. So every workflow checks out only the
commit its run is for, and a line's code runs only in a run under that line.

This reads every workflow as text, in the repository's fixed-indentation style.
A workflow it cannot read that way fails. It sees what a workflow names, not what
a script does once called: a script that checks out another ref is outside it.
"""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github/workflows"
# Absent means the commit the run is for; this expression says so explicitly.
SAME_COMMIT = "${{ github.sha }}"
# Git commands that replace the working tree with another commit. `git fetch` is
# allowed: it brings another ref in as data, and these then find any step using it.
SWITCHES_TREE = re.compile(
    r"\bgit\b(?:[ \t]+-\S+(?:[ \t]+\S+)?)*[ \t]+(?:checkout|switch|worktree|pull|restore)\b"
    r"|\bgit\b(?:[ \t]+-\S+(?:[ \t]+\S+)?)*[ \t]+reset[ \t]+--hard\b"
    r"|\bgh[ \t]+pr[ \t]+checkout\b"
    r"|\bgit\b[^\n]*\bclone\b"
)


def checkout_steps(text):
    """Each checkout step's `with:` keys, read by indentation."""
    lines = text.splitlines()
    steps = []
    for at, line in enumerate(lines):
        found = re.match(r"^(\s*)(- )?uses: actions/checkout@", line)
        if not found:
            continue
        indent = len(found.group(1)) + (2 if found.group(2) else 0)
        keys = {}
        for following in lines[at + 1:]:
            if following.strip() and len(following) - len(following.lstrip()) < indent:
                break
            if following.startswith(" " * indent + "- "):
                break
            pair = re.match(r"^\s+([\w-]+):\s*(.*?)\s*$", following)
            if pair and following.startswith(" " * (indent + 2)) and pair.group(1) != "with":
                keys[pair.group(1)] = pair.group(2)
        steps.append(keys)
    return steps


def crossings(text):
    """Why this workflow text could run another ref's code, one reason per finding."""
    found = []
    for keys in checkout_steps(text):
        ref = keys.get("ref")
        if ref is not None and ref != SAME_COMMIT:
            found.append(f"checks out {ref}; only {SAME_COMMIT} or no ref is allowed")
        if "repository" in keys:
            found.append(f"checks out another repository, {keys['repository']}")
    for line in text.splitlines():
        if line.lstrip().startswith("#"):
            continue
        if SWITCHES_TREE.search(line):
            found.append(f"switches the working tree: {line.strip()}")
    return found


class WorkflowRefTests(unittest.TestCase):
    def workflows(self):
        found = sorted(WORKFLOWS.glob("*.y*ml"))
        self.assertTrue(found, "no workflows found")
        return found

    def test_no_workflow_runs_code_from_another_ref(self):
        for path in self.workflows():
            with self.subTest(workflow=path.name):
                self.assertEqual(crossings(path.read_text(encoding="utf-8")), [])

    def test_every_workflow_that_names_checkout_is_read_as_a_step(self):
        """A checkout written in a form the reader misses would pass unseen."""
        for path in self.workflows():
            text = path.read_text(encoding="utf-8")
            named = len(re.findall(r"^[^#\n]*actions/checkout@", text, re.M))
            with self.subTest(workflow=path.name):
                self.assertEqual(len(checkout_steps(text)), named)

    def test_the_release_and_portal_workflows_are_covered(self):
        names = {path.name for path in self.workflows()}
        self.assertLessEqual({"release.yml", "portal-pages.yml", "codeflow-policy.yml"}, names)


CHECKOUT = "      - uses: actions/checkout@abc # v6\n        with:\n{}          persist-credentials: false\n"


class CrossingTests(unittest.TestCase):
    """The rule itself, so a passing tree cannot mean a rule that finds nothing."""

    def test_the_run_s_own_commit_passes(self):
        self.assertEqual(crossings(CHECKOUT.format("")), [])
        self.assertEqual(crossings(CHECKOUT.format("          ref: ${{ github.sha }}\n")), [])

    def test_any_other_ref_is_refused(self):
        for ref in ("${{ inputs.source-sha }}", "${{ matrix.sha }}", "integration/x",
                    "${{ github.event.pull_request.head.sha }}", "main"):
            with self.subTest(ref=ref):
                self.assertEqual(len(crossings(CHECKOUT.format(f"          ref: {ref}\n"))), 1)

    def test_the_ref_is_found_after_other_keys_and_in_a_named_step(self):
        text = ("      - name: Check out\n        uses: actions/checkout@abc\n        with:\n"
                "          path: source\n          ref: ${{ inputs.source-sha }}\n"
                "      - name: Later\n        run: echo ref: main\n")
        self.assertEqual(len(crossings(text)), 1)

    def test_a_later_step_s_ref_is_not_read_as_this_checkout_s(self):
        text = ("      - uses: actions/checkout@abc\n      - uses: other/action@def\n        with:\n"
                "          ref: somewhere\n")
        self.assertEqual(crossings(text), [])

    def test_another_repository_is_refused(self):
        self.assertEqual(len(crossings(CHECKOUT.format("          repository: someone/else\n"))), 1)

    def test_commands_that_replace_the_tree_are_refused(self):
        for command in ('git checkout "$SHA"', "git -C source checkout abc", "git switch -d abc",
                        "git worktree add ../x abc", "git pull origin main", "git reset --hard abc",
                        "gh pr checkout 1", "git clone https://example.invalid/x", "git restore --source=abc ."):
            with self.subTest(command=command):
                self.assertEqual(len(crossings(f"        run: {command}\n")), 1)

    def test_fetching_as_data_and_reading_are_allowed(self):
        for command in ('git fetch --no-tags origin "+refs/pull/1/head:refs/x"', "git show abc:file",
                        "git rev-parse HEAD", "git diff --quiet HEAD --", "git ls-tree abc -- file"):
            with self.subTest(command=command):
                self.assertEqual(crossings(f"        run: {command}\n"), [])

    def test_a_comment_is_not_a_crossing(self):
        self.assertEqual(crossings("      # git checkout is never used here\n"), [])


if __name__ == "__main__":
    unittest.main()
