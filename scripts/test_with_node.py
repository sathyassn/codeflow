#!/usr/bin/env python3
"""Controls for scripts/with-node.py (TSK-142 AC-1).

Each control builds fake `node` executables that report a version, points
the launcher's search at them through the environment, and checks which one
the target command sees first on PATH, or that a missing version fails the
target naming that version.
"""

import os
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
LAUNCHER = ROOT / "scripts" / "with-node.py"
WHICH = 'command -v node; node --version'
SPEC = importlib.util.spec_from_file_location("with_node", LAUNCHER)
launcher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(launcher)


class WindowsLaunchControls(unittest.TestCase):
    def test_windows_uses_bash_for_the_posix_chain(self):
        command = "printf '%s' \"$(pwd)\" && test -n \"${PATH}\""
        with patch.object(launcher.shutil, "which", return_value="C:/Git/bin/bash.exe"), \
             patch.object(launcher.subprocess, "run") as run:
            run.return_value.returncode = 0
            self.assertEqual(launcher.run_inner(command, {"PATH": "pinned"}, windows=True), 0)
        run.assert_called_once_with(
            ["C:/Git/bin/bash.exe", "-c", command], env={"PATH": "pinned"}, check=False)

    def test_windows_without_bash_fails_before_the_chain(self):
        with patch.object(launcher.shutil, "which", return_value=None), \
             patch.object(launcher.subprocess, "run") as run:
            with self.assertRaises(SystemExit):
                launcher.run_inner("echo marker", {"PATH": "pinned"}, windows=True)
        run.assert_not_called()


def fake_node(directory: Path, reports: str) -> Path:
    directory.mkdir(parents=True, exist_ok=True)
    node = directory / "node"
    node.write_text(f"#!/bin/sh\necho v{reports}\n")
    node.chmod(0o755)
    return node


@unittest.skipIf(os.name == "nt", "the controls use POSIX shell fakes")
class WithNodeControls(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.pin = self.root / ".node-version"
        self.pin.write_text("24.18.0\n")
        self.path = [str(self.root / "empty")]

    def tearDown(self):
        self.tmp.cleanup()

    def run_launcher(self, command: str = WHICH, pin: Path | None = None, **extra):
        env = {
            "HOME": str(self.root / "home"),
            "PATH": os.pathsep.join(self.path + ["/usr/bin", "/bin"]),
        }
        env.update(extra)
        return subprocess.run(
            [sys.executable, "-B", str(LAUNCHER), str(pin or self.pin), command],
            capture_output=True, text=True, env=env, check=False)

    def test_a_version_manager_install_wins_over_the_same_version_on_path(self):
        on_path = fake_node(self.root / "homebrew/bin", "24.18.0")
        self.path.insert(0, str(on_path.parent))
        managed = fake_node(self.root / "asdf/installs/nodejs/24.18.0/bin", "24.18.0")
        run = self.run_launcher(ASDF_DATA_DIR=str(self.root / "asdf"))
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(run.stdout.splitlines(), [str(managed), "v24.18.0"])

    def test_the_actions_tool_cache_and_nvm_are_searched(self):
        cache = fake_node(self.root / "cache/node/24.18.0/x64/bin", "24.18.0")
        run = self.run_launcher(RUNNER_TOOL_CACHE=str(self.root / "cache"))
        self.assertEqual(run.stdout.splitlines()[0], str(cache), run.stderr)
        nvm = fake_node(self.root / "nvm/versions/node/v24.18.0/bin", "24.18.0")
        run = self.run_launcher(NVM_DIR=str(self.root / "nvm"))
        self.assertEqual(run.stdout.splitlines()[0], str(nvm), run.stderr)

    def test_a_path_node_of_another_version_is_passed_over(self):
        shim = fake_node(self.root / "shims", "23.10.0")
        wanted = fake_node(self.root / "later", "24.18.0")
        self.path[:0] = [str(shim.parent), str(wanted.parent)]
        run = self.run_launcher()
        self.assertEqual(run.stdout.splitlines(), [str(wanted), "v24.18.0"], run.stderr)

    def test_a_directory_is_never_trusted_by_its_name(self):
        fake_node(self.root / "asdf/installs/nodejs/24.18.0/bin", "24.17.0")
        wanted = fake_node(self.root / "later", "24.18.0")
        self.path.insert(0, str(wanted.parent))
        run = self.run_launcher(ASDF_DATA_DIR=str(self.root / "asdf"))
        self.assertEqual(run.stdout.splitlines()[0], str(wanted), run.stderr)

    def test_a_missing_version_fails_the_target_naming_it(self):
        fake_node(self.root / "other", "26.4.0")
        self.path.insert(0, str(self.root / "other"))
        marker = self.root / "ran"
        run = self.run_launcher(f"touch {marker}")
        self.assertEqual(run.returncode, 1)
        self.assertIn("Node 24.18.0 is required by", run.stderr)
        self.assertIn(str(self.pin), run.stderr)
        self.assertFalse(marker.exists(), "the command must not run")

    def test_a_missing_or_malformed_version_file_fails_naming_it(self):
        fake_node(self.root / "bin", "24.18.0")
        self.path.insert(0, str(self.root / "bin"))
        missing = self.root / "absent/.node-version"
        run = self.run_launcher(pin=missing)
        self.assertEqual(run.returncode, 1)
        self.assertIn(f"cannot read the Node version file {missing}", run.stderr)
        self.pin.write_text("lts/*\n")
        run = self.run_launcher()
        self.assertEqual(run.returncode, 1)
        self.assertIn("must hold one exact Node version", run.stderr)

    def test_the_command_runs_through_the_shell_with_its_exit_status(self):
        fake_node(self.root / "bin", "24.18.0")
        self.path.insert(0, str(self.root / "bin"))
        run = self.run_launcher("node --version && exit 7")
        self.assertEqual(run.returncode, 7, run.stderr)
        self.assertEqual(run.stdout.strip(), "v24.18.0")


if __name__ == "__main__":
    unittest.main()
