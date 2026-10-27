"""markdown_env.py removes only a venv it created (TSK-445, Codex round 5).

Each test runs check.py in a fresh process with CODEFLOW_ANCHORED_VENV
pointed at a throwaway path, the way a grader or CI starts it. The owned
rebuild tests install the pinned wheels, so they need the package index.
Run: python3 -B evals/qualification/anchored/test_markdown_env.py
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
CHECK = HERE / "check.py"
DOCUMENT = ROOT / "evals/model-artifacts/method-controls/retry/passing.json"
CASE = "retry-review-opens-with-the-whole"
BASE = os.path.realpath(getattr(sys, "_base_executable", sys.executable))

sys.path.insert(0, str(HERE))
import markdown_env  # noqa: E402  (its functions only; ensure() is never called here)


def grade(venv: Path, python: str = BASE) -> subprocess.CompletedProcess:
    environment = {**os.environ, "CODEFLOW_ANCHORED_VENV": str(venv)}
    environment.pop("CODEFLOW_ANCHORED_ENV", None)
    return subprocess.run([python, "-B", str(CHECK), CASE, str(DOCUMENT)], env=environment,
                          capture_output=True, text=True, check=False, timeout=600)


def tree(path: Path) -> dict[str, bytes]:
    return {str(p.relative_to(path)): p.read_bytes() for p in sorted(path.rglob("*")) if p.is_file()}


def make_stale(venv: Path) -> None:
    """An owned venv whose parser no longer matches the pins."""
    python = markdown_env.interpreter(venv)
    subprocess.run([str(python), "-m", "pip", "install", "--quiet", "--disable-pip-version-check",
                    "markdown-it-py==3.0.0"], check=True)
    assert not markdown_env.current(venv)


class Ownership(unittest.TestCase):
    def setUp(self) -> None:
        self.scratch = tempfile.TemporaryDirectory()
        self.root = Path(self.scratch.name)

    def tearDown(self) -> None:
        self.scratch.cleanup()

    def test_an_unrelated_directory_and_its_contents_survive(self) -> None:
        unrelated = self.root / "notes"
        (unrelated / "inner").mkdir(parents=True)
        (unrelated / "canary.txt").write_text("keep me\n", encoding="utf-8")
        (unrelated / "inner" / "data.json").write_text("{}\n", encoding="utf-8")
        before = tree(unrelated)
        result = grade(unrelated)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("was not created by this helper", result.stderr)
        self.assertEqual(before, tree(unrelated))

    def test_a_symlinked_target_is_refused_and_its_target_survives(self) -> None:
        real = self.root / "real"
        real.mkdir()
        (real / "canary.txt").write_text("keep me\n", encoding="utf-8")
        link = self.root / "link"
        link.symlink_to(real, target_is_directory=True)
        result = grade(link)
        self.assertNotEqual(0, result.returncode)
        self.assertIn("is a symlink", result.stderr)
        self.assertTrue(link.is_symlink())
        self.assertEqual({"canary.txt": b"keep me\n"}, tree(real))

    def test_a_copied_marker_naming_another_directory_does_not_grant_ownership(self) -> None:
        other = self.root / "other"
        other.mkdir()
        (other / markdown_env.OWNER).write_text(markdown_env.owner_text(self.root / "elsewhere"), encoding="utf-8")
        (other / "canary.txt").write_text("keep me\n", encoding="utf-8")
        before = tree(other)
        self.assertNotEqual(0, grade(other).returncode)
        self.assertEqual(before, tree(other))

    def test_an_owned_stale_venv_rebuilds_from_outside_and_from_inside(self) -> None:
        venv = self.root / "managed"
        first = grade(venv)
        self.assertEqual(0, first.returncode, first.stderr)
        self.assertTrue(markdown_env.owned(venv))
        for caller in ("outside", "inside"):
            with self.subTest(caller=caller):
                make_stale(venv)
                python = BASE if caller == "outside" else str(markdown_env.interpreter(venv))
                result = grade(venv, python)
                self.assertEqual(0, result.returncode, result.stderr)
                self.assertEqual(markdown_env.pins(), json.loads(result.stdout)["parser"])
                self.assertTrue(markdown_env.owned(venv))
                self.assertTrue(markdown_env.current(venv))


if __name__ == "__main__":
    unittest.main()
