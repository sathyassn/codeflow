"""markdown_env.py: an isolated start, verified unpacked wheels, the
chain rule, and no deletion (TSK-445, Codex rounds 5 to 10).

Each test copies the grader into a throwaway repository, so the managed root
is that copy's `target/anchored`. The pinned wheels are fetched from the
package index once per run, checked against their pins, and reused by the
tests that need a build. Canary files record any code that should not run.
POSIX-only cases are marked; the Windows policy is tested with mocked path
resolution.
Run isolated, as the grader runs: python3 -I -S -B test_markdown_env.py
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
POSIX = unittest.skipIf(os.name == "nt", "POSIX owner and mode checks")
WHEELS: dict[tuple[str, str, str], bytes] = {}


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
        real_fetch = self.real_fetch = self.env.fetch

        def cached(name: str, version: str, sha256: str) -> bytes:
            key = (name, version, sha256)
            if key not in WHEELS:
                WHEELS[key] = real_fetch(name, version, sha256)
            return WHEELS[key]

        self.fetch = mock.patch.object(self.env, "fetch", cached)
        self.fetch.start()

    def tearDown(self) -> None:
        self.fetch.stop()
        for path in self.outside.rglob("*"):
            if not path.is_symlink():
                path.chmod(0o700)
        self.scratch.cleanup()

    def build(self) -> Path:
        target = self.env.build_path()
        self.env.build(target)
        self.assertIsNone(self.env.build_problem(target))
        return target

    def grade(self, flags=("-I", "-S", "-B"), environment: dict | None = None) -> subprocess.CompletedProcess:
        python = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
        return subprocess.run([python, *flags, str(self.check), CASE, str(DOCUMENT)],
                              env=environment if environment is not None else {},
                              capture_output=True, text=True, check=False, timeout=600)

    def refused(self, target: Path) -> str:
        reason = self.env.build_problem(target)
        self.assertIsNotNone(reason)
        return reason


class IsolatedStart(Scratch):
    """No startup hook from the caller's environment runs, and a start
    without -I -S is refused."""

    def hooks(self) -> tuple[dict[str, str], list[Path]]:
        hooks = self.outside / "hooks"
        version = f"python{sys.version_info.major}.{sys.version_info.minor}"
        userbase = self.outside / "userbase"
        user_site = userbase / "lib" / version / "site-packages"
        user_site.mkdir(parents=True)
        hooks.mkdir()
        ran = []
        for directory, name in ((hooks, "sitecustomize.py"), (hooks, "usercustomize.py"),
                                (user_site, "usercustomize.py"), (user_site, "sitecustomize.py")):
            mark = self.outside / f"ran-{directory.name}-{name}"
            (directory / name).write_text(f"open({str(mark)!r}, 'w').close()\n", encoding="utf-8")
            ran.append(mark)
        for directory in (hooks, user_site):
            mark = self.outside / f"ran-{directory.name}-pth"
            (directory / "hook.pth").write_text(f"import os; open({str(mark)!r}, 'w').close()\n", encoding="utf-8")
            ran.append(mark)
        startup = self.outside / "startup.py"
        mark = self.outside / "ran-startup"
        startup.write_text(f"open({str(mark)!r}, 'w').close()\n", encoding="utf-8")
        ran.append(mark)
        environment = {"PYTHONPATH": str(hooks), "PYTHONHOME": str(self.outside / "no-such-home"),
                       "PYTHONUSERBASE": str(userbase), "PYTHONSTARTUP": str(startup)}
        return environment, ran

    def test_the_callers_startup_hooks_never_run(self) -> None:
        environment, ran = self.hooks()
        # Control: a plain start runs the planted hooks, so the canaries work.
        python = os.path.realpath(getattr(sys, "_base_executable", sys.executable))
        plain = {key: value for key, value in environment.items() if key != "PYTHONHOME"}
        subprocess.run([python, "-c", "pass"], env=plain, check=True, timeout=60)
        self.assertTrue(any(mark.exists() for mark in ran))
        for mark in ran:
            mark.unlink(missing_ok=True)
        result = self.grade(environment=environment)
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertEqual(self.env.pins(), json.loads(result.stdout)["parser"])
        self.assertEqual([], [str(mark) for mark in ran if mark.exists()])

    def test_a_pth_file_placed_in_the_build_never_runs(self) -> None:
        target = self.build()
        mark = self.outside / "ran-build-pth"
        (target / "hook.pth").write_text(f"import os; open({str(mark)!r}, 'w').close()\n", encoding="utf-8")
        (target / "hook.pth").chmod(0o644)
        result = self.grade()
        self.assertEqual(0, result.returncode, result.stderr)
        self.assertFalse(mark.exists())

    def test_a_pth_or_sitecustomize_planted_in_lib_is_refused_and_never_runs(self) -> None:
        target = self.build()
        lib = self.env.lib_path(target)
        marks = []
        for name, text in (("hook.pth", "import os; open({!r}, 'w').close()\n"),
                           ("sitecustomize.py", "open({!r}, 'w').close()\n")):
            mark = self.outside / f"ran-lib-{name}"
            (lib / name).write_text(text.format(str(mark)), encoding="utf-8")
            (lib / name).chmod(0o644)
            marks.append(mark)
        result = self.grade()
        self.assertNotEqual(0, result.returncode)
        self.assertIn("was added or removed", result.stderr)
        self.assertEqual([], [str(mark) for mark in marks if mark.exists()])

    def test_a_start_without_isolation_is_refused(self) -> None:
        result = self.grade(flags=("-B",))
        self.assertNotEqual(0, result.returncode)
        self.assertIn("must start isolated", result.stderr)
        self.assertFalse(os.path.lexists(self.env.root()))


class Integrity(Scratch):
    """A tampered or added file under lib is refused before it is imported."""

    def test_a_tampered_package_file_is_refused_and_never_runs(self) -> None:
        target = self.build()
        init = self.env.lib_path(target) / "markdown_it" / "__init__.py"
        mark = self.outside / "ran-tampered"
        init.write_text(init.read_text(encoding="utf-8") + f"\nopen({str(mark)!r}, 'w').close()\n",
                        encoding="utf-8")
        init.chmod(0o644)
        self.assertIn("differs from its recorded SHA-256", self.refused(target))
        result = self.grade()
        self.assertNotEqual(0, result.returncode)
        self.assertFalse(mark.exists())

    def test_a_wheel_that_does_not_match_its_pin_is_refused(self) -> None:
        import io
        name, version, sha256 = self.env.requirements()[0]

        class Response(io.BytesIO):
            def __enter__(self):
                return self

            def __exit__(self, *details):
                return False

        listing = json.dumps({"urls": [{"packagetype": "bdist_wheel", "url": "https://index.invalid/w.whl",
                                        "digests": {"sha256": sha256}}]}).encode()
        responses = [Response(listing), Response(b"not the wheel")]
        with mock.patch.object(self.env.urllib.request, "urlopen", lambda *a, **k: responses.pop(0)), \
                self.assertRaises(SystemExit) as refused:
            self.real_fetch(name, version, sha256)
        self.assertIn("does not match its pinned SHA-256", str(refused.exception))

    def test_a_wheel_entry_escaping_its_directory_is_refused(self) -> None:
        import io
        import zipfile
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("../escape.py", "x = 1\n")
        lib = self.outside / "lib"
        lib.mkdir()
        with self.assertRaises(SystemExit):
            self.env.unpack(buffer.getvalue(), lib)
        self.assertFalse((self.outside / "escape.py").exists())


@POSIX
class ChainRule(Scratch):
    """Every path from the repository root to lib, and everything under lib,
    must be owned by the user, not writable by group or others, and not a
    link (Codex rounds 9 and 10)."""

    def test_a_world_writable_target_directory_is_refused(self) -> None:
        target = self.build()
        (self.repository / "target").chmod(0o777)
        self.assertIn("writable by group or others", self.refused(target))

    def test_a_world_writable_lib_directory_is_refused(self) -> None:
        target = self.build()
        self.env.lib_path(target).chmod(0o777)
        self.assertIn("writable by group or others", self.refused(target))

    def test_a_writable_package_file_is_refused(self) -> None:
        target = self.build()
        (self.env.lib_path(target) / "mdurl" / "__init__.py").chmod(0o666)
        self.assertIn("writable by group or others", self.refused(target))

    def test_a_symlinked_parent_is_refused(self) -> None:
        target = self.build()
        root = self.env.root()
        moved = self.outside / "elsewhere"
        root.rename(moved)
        root.symlink_to(moved, target_is_directory=True)
        before = snapshot(moved)
        self.assertIn("without following a link", self.refused(target))
        self.assertEqual(before, snapshot(moved))

    def test_a_link_inside_lib_is_refused(self) -> None:
        target = self.build()
        (self.env.lib_path(target) / "extra").symlink_to(self.outside)
        self.assertIn("without following a link", self.refused(target))

    def test_a_build_owned_by_another_uid_is_refused(self) -> None:
        # Creating a directory owned by another uid needs root, so the stat
        # result of the build is replaced with one naming another uid.
        target = self.build()
        real_fstat = os.fstat
        planted = os.stat(target).st_ino

        def fstat(handle):
            info = real_fstat(handle)
            if info.st_ino != planted:
                return info
            values = list(info)
            values[stat.ST_UID] = os.getuid() + 1
            return os.stat_result(values)

        with mock.patch.object(self.env.os, "fstat", fstat):
            self.assertIn("not the current user", self.refused(target))

    def test_the_retired_override_is_refused(self) -> None:
        # CODEFLOW_ANCHORED_VENV is refused, not ignored, so a caller still
        # setting it learns the root no longer moves.
        result = self.grade(environment={"CODEFLOW_ANCHORED_VENV": str(self.outside / "somewhere")})
        self.assertNotEqual(0, result.returncode)
        self.assertIn("can no longer be moved", result.stderr)
        self.assertFalse((self.outside / "somewhere").exists())


class WindowsPolicy(Scratch):
    """The Windows rule, with path resolution mocked so it runs anywhere."""

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


@POSIX
class NeverDeletes(Scratch):
    """A path at the managed name is never removed, before or during a build."""

    def test_a_directory_at_the_managed_name_is_refused_and_survives(self) -> None:
        target = self.env.build_path()
        canary(target)
        before = snapshot(self.repository)
        result = self.grade()
        self.assertNotEqual(0, result.returncode)
        self.assertEqual(before, snapshot(self.repository))

    def test_a_symlink_at_the_managed_name_is_refused_and_both_survive(self) -> None:
        real = self.outside / "real"
        canary(real)
        target = self.env.build_path()
        target.parent.mkdir(parents=True)
        target.symlink_to(real, target_is_directory=True)
        before = snapshot(self.outside)
        self.assertNotEqual(0, self.grade().returncode)
        self.assertEqual(before, snapshot(self.outside))

    def interfere(self, step: str, swap) -> Path:
        target = self.env.build_path()

        def pause(at: str) -> None:
            if at == step:
                swap(target)

        with mock.patch.object(self.env, "pause", pause), self.assertRaises(SystemExit):
            self.env.build(target)
        return target

    def test_a_directory_appearing_at_the_name_during_the_build_survives(self) -> None:
        target = self.interfere("rename", canary)
        self.assertEqual("keep me\n", (target / "canary.txt").read_text(encoding="utf-8"))
        self.assertEqual(1, len(list(self.env.root().glob(f".{target.name}.build-*"))))

    def test_a_symlink_appearing_at_the_name_during_the_build_survives(self) -> None:
        real = self.outside / "real"
        canary(real)
        target = self.interfere("rename", lambda t: t.symlink_to(real, target_is_directory=True))
        self.assertTrue(target.is_symlink())
        self.assertEqual("keep me\n", (real / "canary.txt").read_text(encoding="utf-8"))

    def swap_staging_before(self, step: str, replacement: str) -> None:
        unrelated = self.outside / f"unrelated-{step}-{replacement}"
        canary(unrelated)
        before = snapshot(unrelated)

        def swap(target: Path) -> None:
            staging = next(self.env.root().glob(f".{target.name}.build-*"))
            staging.rename(staging.with_name(staging.name + "-moved"))
            if replacement == "symlink":
                staging.symlink_to(unrelated, target_is_directory=True)
            else:
                canary(staging)

        target = self.env.build_path()

        def pause(at: str) -> None:
            if at == step:
                swap(target)

        with mock.patch.object(self.env, "pause", pause), self.assertRaises(SystemExit) as refused:
            self.env.build(target)
        self.assertIn(f"before {step}", str(refused.exception))
        self.assertEqual(before, snapshot(unrelated))
        self.assertFalse(os.path.lexists(target))

    def test_a_staging_swap_before_unpacking_is_refused(self) -> None:
        for replacement in ("directory", "symlink"):
            with self.subTest(replacement=replacement):
                self.swap_staging_before("unpack", replacement)
                shutil.rmtree(self.env.root())  # this test's own scratch only

    def test_a_staging_swap_before_stamping_is_refused(self) -> None:
        for replacement in ("directory", "symlink"):
            with self.subTest(replacement=replacement):
                self.swap_staging_before("stamp", replacement)
                shutil.rmtree(self.env.root())  # this test's own scratch only


@POSIX
class ContentNamed(Scratch):
    """A matching build is reused; a pin change gives a new one and keeps
    the old."""

    def test_a_matching_build_is_reused_and_a_pin_change_builds_a_new_one(self) -> None:
        first = self.grade()
        self.assertEqual(0, first.returncode, first.stderr)
        target = self.env.build_path()
        self.assertIsNone(self.env.build_problem(target))
        stamp = (target / self.env.STAMP).stat().st_mtime_ns
        entries = sorted(p.name for p in self.env.root().iterdir())
        second = self.grade()
        self.assertEqual(0, second.returncode, second.stderr)
        self.assertEqual(stamp, (target / self.env.STAMP).stat().st_mtime_ns)
        self.assertEqual(entries, sorted(p.name for p in self.env.root().iterdir()))
        old = snapshot(target)
        requirements = self.check.parent / "requirements.txt"
        requirements.write_text(requirements.read_text(encoding="utf-8") + "# changed\n", encoding="utf-8")
        third = self.grade()
        self.assertEqual(0, third.returncode, third.stderr)
        self.assertEqual(2, len([p for p in self.env.root().iterdir() if not p.name.startswith(".")]))
        self.assertEqual(old, snapshot(target))


if __name__ == "__main__":
    unittest.main()
