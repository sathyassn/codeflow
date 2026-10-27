"""markdown_env.py never deletes anything (TSK-445, Codex round 6).

The managed venv is named by its content, built in a private directory and
moved into place with one rename. These tests check that a directory, file
or symlink put at the managed name, before or during a build, is refused and
survives; that a pin change gives a new venv and keeps the old one; and that
a matching venv is reused. The process tests run check.py with
CODEFLOW_ANCHORED_VENV as the root to build under and install the pinned
wheels, so they need the package index.
Run: python3 -B evals/qualification/anchored/test_markdown_env.py
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
DOCUMENT = ROOT / "evals/model-artifacts/method-controls/retry/passing.json"
CASE = "retry-review-opens-with-the-whole"
BASE = os.path.realpath(getattr(sys, "_base_executable", sys.executable))

sys.path.insert(0, str(HERE))
import markdown_env  # noqa: E402  (its functions only; ensure() is never called in this process)


def grade(root: Path, check: Path = HERE / "check.py", python: str = BASE) -> subprocess.CompletedProcess:
    environment = {**os.environ, "CODEFLOW_ANCHORED_VENV": str(root)}
    return subprocess.run([python, "-B", str(check), CASE, str(DOCUMENT)], env=environment,
                          capture_output=True, text=True, check=False, timeout=600)


def snapshot(path: Path) -> dict[str, object]:
    """Every entry under path: file bytes, and symlink targets."""
    out: dict[str, object] = {}
    for entry in sorted(path.rglob("*")):
        key = str(entry.relative_to(path))
        if entry.is_symlink():
            out[key] = ("link", os.readlink(entry))
        elif entry.is_file():
            out[key] = entry.read_bytes()
    return out


def canary(directory: Path) -> None:
    (directory / "inner").mkdir(parents=True)
    (directory / "canary.txt").write_text("keep me\n", encoding="utf-8")
    (directory / "inner" / "data.json").write_text("{}\n", encoding="utf-8")


class Scratch(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)
        self.environment = mock.patch.dict(os.environ, {"CODEFLOW_ANCHORED_VENV": str(self.root)})
        self.environment.start()

    def tearDown(self) -> None:
        self.environment.stop()
        self.scratch.cleanup()


class NeverDeletes(Scratch):
    """A path at the managed name is never removed, before or during a build."""

    def test_a_directory_at_the_managed_name_is_refused_and_survives(self) -> None:
        target = markdown_env.venv_path()
        canary(target)
        before = snapshot(self.root)
        result = grade(self.root)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("is not a valid managed venv", result.stderr)
        self.assertEqual(before, snapshot(self.root))

    def test_a_symlink_at_the_managed_name_is_refused_and_both_survive(self) -> None:
        real = self.root / "real"
        canary(real)
        markdown_env.venv_path().symlink_to(real, target_is_directory=True)
        before = snapshot(self.root)
        result = grade(self.root)
        self.assertNotEqual(0, result.returncode)
        self.assertEqual(before, snapshot(self.root))

    def fake_build(self, during):
        """Patch the build so it needs no network, and run `during` between
        the install and the move into place."""
        def create(staging: Path) -> None:
            (staging / "bin").mkdir()
            (staging / "bin" / "python").write_text("", encoding="utf-8")

        def install(staging: Path) -> None:
            during()

        return (mock.patch.object(markdown_env, "create", create), mock.patch.object(markdown_env, "install", install),
                mock.patch.object(markdown_env, "installed", lambda python: markdown_env.pins()))

    def assert_raced_build_keeps(self, swap) -> None:
        target = markdown_env.venv_path()
        patches = self.fake_build(lambda: swap(target))
        with patches[0], patches[1], patches[2], self.assertRaises(SystemExit):
            markdown_env.build(target)
        builds = [p for p in self.root.iterdir() if p.name.startswith(f".{target.name}.build-")]
        self.assertEqual(1, len(builds), "the build stays in its own directory")
        self.assertTrue((builds[0] / markdown_env.STAMP).is_file())

    def test_a_directory_swapped_in_during_the_build_survives(self) -> None:
        self.assert_raced_build_keeps(canary)
        self.assertEqual("keep me\n", (markdown_env.venv_path() / "canary.txt").read_text(encoding="utf-8"))

    def test_a_symlink_swapped_in_during_the_build_survives(self) -> None:
        real = self.root / "real"
        canary(real)
        self.assert_raced_build_keeps(lambda target: target.symlink_to(real, target_is_directory=True))
        self.assertTrue(markdown_env.venv_path().is_symlink())
        self.assertEqual("keep me\n", (real / "canary.txt").read_text(encoding="utf-8"))

    def swap_after_the_last_check(self, swap) -> Path:
        """The name is checked free, then `swap` puts something there before
        the rename; the rename fails and removes nothing."""
        target = markdown_env.venv_path()
        patches = self.fake_build(lambda: None)
        real_lexists = os.path.lexists

        def lexists(path):
            if Path(path) == target and not getattr(lexists, "swapped", False):
                lexists.swapped = True
                swap(target)
                return False
            return real_lexists(path)

        with patches[0], patches[1], patches[2], mock.patch.object(markdown_env.os.path, "lexists", lexists), \
                self.assertRaises(SystemExit):
            markdown_env.build(target)
        return target

    def test_a_directory_swapped_in_after_the_last_check_survives_the_rename(self) -> None:
        target = self.swap_after_the_last_check(canary)
        self.assertEqual("keep me\n", (target / "canary.txt").read_text(encoding="utf-8"))
        self.assertEqual("{}\n", (target / "inner" / "data.json").read_text(encoding="utf-8"))

    def test_a_symlink_swapped_in_after_the_last_check_survives_the_rename(self) -> None:
        real = self.root / "real"
        canary(real)
        target = self.swap_after_the_last_check(lambda t: t.symlink_to(real, target_is_directory=True))
        self.assertTrue(target.is_symlink())
        self.assertEqual("keep me\n", (real / "canary.txt").read_text(encoding="utf-8"))


class ContentNamed(Scratch):
    """A pin change gives a new venv; a matching venv is reused."""

    def test_a_matching_venv_is_reused(self) -> None:
        first = grade(self.root)
        self.assertEqual(0, first.returncode, first.stderr)
        venv = markdown_env.venv_path()
        self.assertTrue(markdown_env.valid(venv))
        stamp = (venv / markdown_env.STAMP).stat().st_mtime_ns
        entries = sorted(p.name for p in self.root.iterdir())
        second = grade(self.root)
        self.assertEqual(0, second.returncode, second.stderr)
        self.assertEqual(markdown_env.pins(), json.loads(second.stdout)["parser"])
        self.assertEqual(stamp, (venv / markdown_env.STAMP).stat().st_mtime_ns)
        self.assertEqual(entries, sorted(p.name for p in self.root.iterdir()))

    def test_a_pin_change_builds_a_new_venv_and_keeps_the_old(self) -> None:
        old = grade(self.root)
        self.assertEqual(0, old.returncode, old.stderr)
        old_venv = markdown_env.venv_path()
        old_files = snapshot(old_venv)
        # A copy of the grader whose requirements file differs, as a pin change does.
        copy = self.root / "grader"
        shutil.copytree(HERE, copy, ignore=shutil.ignore_patterns("__pycache__"))
        requirements = copy / "requirements.txt"
        requirements.write_text(requirements.read_text(encoding="utf-8") + "# changed\n", encoding="utf-8")
        new = grade(self.root, copy / "check.py")
        self.assertEqual(0, new.returncode, new.stderr)
        venvs = sorted(p.name for p in self.root.iterdir() if p.name.startswith("anchored-venv-"))
        self.assertEqual(2, len(venvs), venvs)
        self.assertEqual(old_files, snapshot(old_venv))


if __name__ == "__main__":
    unittest.main()
