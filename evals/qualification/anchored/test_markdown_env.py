"""markdown_env.py: a fixed root, the full-chain rule, and no deletion
(TSK-445, Codex rounds 5 to 9).

Each test copies the grader into a throwaway repository, so the managed root
is that copy's `target/anchored`. Planted interpreters write a canary file
when they run; a refused case must leave it unwritten. The process tests
install the pinned wheels and need the package index. POSIX-only cases are
marked; the Windows policy is tested with mocked path resolution.
Run: python3 -B evals/qualification/anchored/test_markdown_env.py
"""

from __future__ import annotations

import importlib.util
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
REPOSITORY = HERE.parents[2]
DOCUMENT = REPOSITORY / "evals/model-artifacts/method-controls/retry/passing.json"
CASE = "retry-review-opens-with-the-whole"
BASE = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
POSIX = unittest.skipIf(os.name == "nt", "POSIX owner and mode checks")


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
    """A throwaway repository holding a copy of the grader, and that copy's
    markdown_env loaded as `self.env`."""

    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.outside = Path(self.scratch.name)
        self.repository = self.outside / "repository"
        grader = self.repository / "evals" / "qualification" / "anchored"
        grader.mkdir(parents=True)
        for name in ("check.py", "markdown_env.py", "requirements.txt"):
            shutil.copy2(HERE / name, grader / name)
        spec = importlib.util.spec_from_file_location(f"markdown_env_{id(self)}", grader / "markdown_env.py")
        self.env = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.env)
        self.check = grader / "check.py"
        self.environment = mock.patch.dict(os.environ, {}, clear=False)
        self.environment.start()
        os.environ.pop("CODEFLOW_ANCHORED_VENV", None)

    def tearDown(self) -> None:
        self.environment.stop()
        for path in self.outside.rglob("*"):
            if not path.is_symlink():
                path.chmod(0o700)
        self.scratch.cleanup()

    def grade(self) -> subprocess.CompletedProcess:
        environment = {key: value for key, value in os.environ.items() if key != "CODEFLOW_ANCHORED_VENV"}
        return subprocess.run([BASE, "-B", str(self.check), CASE, str(DOCUMENT)], env=environment,
                              capture_output=True, text=True, check=False, timeout=600)

    def fake_venv(self, venv: Path, ran: Path | None = None) -> None:
        """A venv's chain without a real Python: the interpreter is a script
        that writes `ran` when it runs and prints the pins."""
        python = self.env.interpreter(venv)
        python.parent.mkdir(parents=True, exist_ok=True)
        marker = f"touch '{ran}'\n" if ran else ""
        python.write_text(f"#!/bin/sh\n{marker}echo '{json.dumps(self.env.pins())}'\n", encoding="utf-8")
        python.chmod(0o755)
        for path in self.env.startup_paths(venv):
            if path.name == "pyvenv.cfg":
                path.write_text("home = /usr/bin\n", encoding="utf-8")
                path.chmod(0o644)
            else:
                path.mkdir(parents=True, exist_ok=True)
                path.chmod(0o755)

    def plant(self) -> tuple[Path, Path]:
        """A complete, stamped managed venv whose interpreter writes a canary."""
        venv = self.env.venv_path()
        ran = self.outside / "planted-interpreter-ran"
        self.fake_venv(venv, ran)
        (venv / self.env.STAMP).write_text(json.dumps(self.env.stamp_data(), sort_keys=True) + "\n",
                                           encoding="utf-8")
        for path in self.env.root_chain() + [venv]:
            path.chmod(0o755)
        return venv, ran

    def assert_refused_without_running(self, venv: Path, ran: Path) -> str:
        self.assertFalse(self.env.valid(venv))
        with mock.patch.object(self.env.os, "execv") as execv, self.assertRaises(SystemExit) as refused:
            self.env.ensure()
        execv.assert_not_called()
        self.assertFalse(ran.exists(), "the planted interpreter ran")
        return str(refused.exception)


@POSIX
class ChainRule(Scratch):
    """Every path from the repository root to the interpreter, and the paths
    Python reads at start, must be owned by the user, not writable by group
    or others, and not a symlink (Codex round 9)."""

    def test_the_canary_shows_a_trusted_planted_interpreter_would_run(self) -> None:
        venv, ran = self.plant()
        self.assertTrue(self.env.valid(venv))
        self.assertTrue(ran.exists())

    def test_a_world_writable_target_directory_is_refused(self) -> None:
        venv, ran = self.plant()
        (self.repository / "target").chmod(0o777)
        self.assertIn("writable by group or others", self.assert_refused_without_running(venv, ran))

    def test_a_world_writable_bin_directory_is_refused(self) -> None:
        venv, ran = self.plant()
        self.env.interpreter(venv).parent.chmod(0o777)
        self.assertIn("writable by group or others", self.assert_refused_without_running(venv, ran))

    def test_a_writable_interpreter_file_is_refused(self) -> None:
        venv, ran = self.plant()
        self.env.interpreter(venv).chmod(0o777)
        self.assertIn("writable by group or others", self.assert_refused_without_running(venv, ran))

    def test_a_writable_site_packages_is_refused(self) -> None:
        venv, ran = self.plant()
        self.env.startup_paths(venv)[-1].chmod(0o777)
        self.assert_refused_without_running(venv, ran)

    def test_a_symlinked_parent_is_refused(self) -> None:
        venv, ran = self.plant()
        root = self.env.root()
        moved = self.outside / "elsewhere"
        root.rename(moved)
        root.symlink_to(moved, target_is_directory=True)
        before = snapshot(moved)
        self.assertIn("without following a link", self.assert_refused_without_running(venv, ran))
        self.assertEqual(before, snapshot(moved))

    def test_a_venv_owned_by_another_uid_is_refused(self) -> None:
        # Creating a directory owned by another uid needs root, so the stat
        # result of the planted venv is replaced with one naming another uid.
        venv, ran = self.plant()
        real_fstat = os.fstat
        planted = os.stat(venv).st_ino

        def fstat(handle):
            info = real_fstat(handle)
            if info.st_ino != planted:
                return info
            values = list(info)
            values[stat.ST_UID] = os.getuid() + 1
            return os.stat_result(values)

        with mock.patch.object(self.env.os, "fstat", fstat):
            self.assertIn("not the current user", self.assert_refused_without_running(venv, ran))

    def test_the_retired_override_is_refused(self) -> None:
        # CODEFLOW_ANCHORED_VENV is refused, not ignored: a caller relying on
        # it learns the root no longer moves.
        venv, ran = self.plant()
        os.environ["CODEFLOW_ANCHORED_VENV"] = str(self.outside / "somewhere")
        with mock.patch.object(self.env.os, "execv") as execv, self.assertRaises(SystemExit) as refused:
            self.env.ensure()
        execv.assert_not_called()
        self.assertIn("can no longer be moved", str(refused.exception))
        self.assertFalse(ran.exists())
        self.assertFalse((self.outside / "somewhere").exists())


class WindowsPolicy(Scratch):
    """The Windows rule, with path resolution mocked so it runs anywhere: no
    reparse point, and the resolved path inside the repository."""

    def test_a_reparse_point_is_refused(self) -> None:
        problem = self.env.windows_problem(self.repository / "target", self.repository,
                                           realpath=lambda p: p, attributes=lambda p: 0x400)
        self.assertIn("reparse point", problem)

    def test_a_path_resolving_outside_the_repository_is_refused(self) -> None:
        outside = str(self.outside / "elsewhere")
        problem = self.env.windows_problem(self.repository / "target", self.repository,
                                           realpath=lambda p: outside if p.endswith("target") else p,
                                           attributes=lambda p: 0)
        self.assertIn("outside the repository", problem)

    def test_a_path_resolving_inside_the_repository_passes(self) -> None:
        self.assertIsNone(self.env.windows_problem(self.repository / "target", self.repository,
                                                   realpath=lambda p: p, attributes=lambda p: 0))

    def test_the_interpreter_is_under_scripts_on_windows(self) -> None:
        self.assertEqual(Path("v/Scripts/python.exe"), self.env.interpreter(Path("v"), windows=True))
        self.assertIn(Path("v/Lib/site-packages"), self.env.startup_paths(Path("v"), windows=True))
        self.assertEqual(Path("v/bin/python"), self.env.interpreter(Path("v"), windows=False))


@POSIX
class NeverDeletes(Scratch):
    """A path at the managed name is never removed, before or during a build."""

    def fake_build(self, during):
        """Patch the build so it needs no network, and run `during` between
        the install and the move into place."""
        return (mock.patch.object(self.env, "create", lambda staging: self.fake_venv(staging)),
                mock.patch.object(self.env, "install", lambda staging: during()),
                mock.patch.object(self.env, "installed", lambda python: self.env.pins()))

    def test_a_directory_at_the_managed_name_is_refused_and_survives(self) -> None:
        target = self.env.venv_path()
        canary(target)
        before = snapshot(self.repository)
        result = self.grade()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual(before, snapshot(self.repository))

    def test_a_symlink_at_the_managed_name_is_refused_and_both_survive(self) -> None:
        real = self.outside / "real"
        canary(real)
        target = self.env.venv_path()
        target.parent.mkdir(parents=True)
        target.symlink_to(real, target_is_directory=True)
        before = snapshot(self.outside)
        self.assertNotEqual(0, self.grade().returncode)
        self.assertEqual(before, snapshot(self.outside))

    def raced(self, swap) -> None:
        target = self.env.venv_path()
        patches = self.fake_build(lambda: swap(target))
        with patches[0], patches[1], patches[2], self.assertRaises(SystemExit):
            self.env.build(target)
        builds = list(self.env.root().glob(f".{target.name}.build-*"))
        self.assertEqual(1, len(builds), "the build stays in its own directory")

    def test_a_directory_swapped_in_during_the_build_survives(self) -> None:
        self.raced(canary)
        self.assertEqual("keep me\n", (self.env.venv_path() / "canary.txt").read_text(encoding="utf-8"))

    def test_a_symlink_swapped_in_during_the_build_survives(self) -> None:
        real = self.outside / "real"
        canary(real)
        self.raced(lambda target: target.symlink_to(real, target_is_directory=True))
        self.assertTrue(self.env.venv_path().is_symlink())
        self.assertEqual("keep me\n", (real / "canary.txt").read_text(encoding="utf-8"))

    def swap_after_the_last_check(self, swap) -> Path:
        """The name is checked free, then `swap` puts something there before
        the rename; the rename fails and removes nothing."""
        target = self.env.venv_path()
        patches = self.fake_build(lambda: None)
        real_lexists = os.path.lexists

        def lexists(path):
            if Path(path) == target and not getattr(lexists, "swapped", False):
                lexists.swapped = True
                swap(target)
                return False
            return real_lexists(path)

        with patches[0], patches[1], patches[2], mock.patch.object(self.env.os.path, "lexists", lexists), \
                self.assertRaises(SystemExit):
            self.env.build(target)
        return target

    def test_a_directory_swapped_in_after_the_last_check_survives_the_rename(self) -> None:
        target = self.swap_after_the_last_check(canary)
        self.assertEqual("keep me\n", (target / "canary.txt").read_text(encoding="utf-8"))

    def test_a_symlink_swapped_in_after_the_last_check_survives_the_rename(self) -> None:
        real = self.outside / "real"
        canary(real)
        target = self.swap_after_the_last_check(lambda t: t.symlink_to(real, target_is_directory=True))
        self.assertTrue(target.is_symlink())
        self.assertEqual("keep me\n", (real / "canary.txt").read_text(encoding="utf-8"))

    def swap_staging_before(self, step: str, replacement: str) -> None:
        unrelated = self.outside / f"unrelated-{step}-{replacement}"
        canary(unrelated)
        before = snapshot(unrelated)

        def pause(at: str) -> None:
            if at != step:
                return
            staging = next(self.env.root().glob(".anchored-venv-*.build-*")) / "venv"
            staging.rename(staging.with_name("moved-aside"))
            if replacement == "symlink":
                staging.symlink_to(unrelated, target_is_directory=True)
            else:
                canary(staging)

        target = self.env.venv_path()
        patches = self.fake_build(lambda: None)
        with mock.patch.object(self.env, "pause", pause), patches[0], patches[1], patches[2], \
                self.assertRaises(SystemExit) as refused:
            self.env.build(target)
        self.assertIn(f"before {step}", str(refused.exception))
        self.assertEqual(before, snapshot(unrelated))
        self.assertFalse(os.path.lexists(target))

    def test_a_staging_swap_before_creation_is_refused(self) -> None:
        for replacement in ("directory", "symlink"):
            with self.subTest(replacement=replacement):
                self.swap_staging_before("create", replacement)
                shutil.rmtree(self.env.root())  # this test's own scratch only

    def test_a_staging_swap_before_stamping_is_refused(self) -> None:
        for replacement in ("directory", "symlink"):
            with self.subTest(replacement=replacement):
                self.swap_staging_before("stamp", replacement)
                shutil.rmtree(self.env.root())  # this test's own scratch only


@POSIX
class ContentNamed(Scratch):
    """A pin change gives a new venv; a matching venv is reused. These build
    real venvs from the package index."""

    def test_a_matching_venv_is_reused_and_a_pin_change_builds_a_new_one(self) -> None:
        first = self.grade()
        self.assertEqual(0, first.returncode, first.stderr)
        venv = self.env.venv_path()
        self.assertTrue(self.env.valid(venv))
        self.assertFalse(self.env.interpreter(venv).is_symlink(), "a copied interpreter, not a link")
        stamp = (venv / self.env.STAMP).stat().st_mtime_ns
        entries = sorted(p.name for p in self.env.root().iterdir())
        second = self.grade()
        self.assertEqual(0, second.returncode, second.stderr)
        self.assertEqual(self.env.pins(), json.loads(second.stdout)["parser"])
        self.assertEqual(stamp, (venv / self.env.STAMP).stat().st_mtime_ns)
        self.assertEqual(entries, sorted(p.name for p in self.env.root().iterdir()))
        old_files = snapshot(venv)
        requirements = self.check.parent / "requirements.txt"
        requirements.write_text(requirements.read_text(encoding="utf-8") + "# changed\n", encoding="utf-8")
        third = self.grade()
        self.assertEqual(0, third.returncode, third.stderr)
        venvs = sorted(p.name for p in self.env.root().iterdir() if p.name.startswith("anchored-venv-"))
        self.assertEqual(2, len(venvs), venvs)
        self.assertEqual(old_files, snapshot(venv))


if __name__ == "__main__":
    unittest.main()
