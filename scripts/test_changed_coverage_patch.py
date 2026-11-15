#!/usr/bin/env python3
"""Verify the operator patch against explicit inputs, without walking tests.

Only two pinned workflow directories are enumerated. Other inputs are named
by the current gate configuration. No tests directory is walked and no
secret-named file is opened. The only child command is the parity checker.
"""

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import sys
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("coverage_patch", ROOT / "scripts/operator/TSK-1261/apply.py")
operator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(operator)


def load_parity(path):
    spec = importlib.util.spec_from_file_location("patched_parity", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def copy_inputs(scratch, parity):
    config = json.loads((ROOT / ".codeflow/test-config.json").read_text(encoding="utf-8"))
    inputs = {".codeflow/test-config.json", "dist-workspace.toml", "scripts/ci-reuse.py",
              "scripts/windows-test-times.json", "scripts/coverage-test-times.json"}
    for directory in (".github/workflows", "assets/base/ci"):
        inputs.update(str(p.relative_to(ROOT)) for p in (ROOT / directory).glob("*.y*ml"))
    for target in config["targets"]:
        for settings in target.get("modes", {}).values():
            command = settings.get("command", "")
            pin, _ = parity.with_node_parts(command)
            if pin:
                inputs.add(pin)
                inputs.add(str(Path(pin).parent / "package.json"))
            for name in parity.literal_script_paths(command):
                relative = Path(target.get("cwd", ".")) / name
                if relative.is_absolute() or ".." in relative.parts:
                    raise ValueError("parity input must be repository-relative")
                if (ROOT / relative).is_file():
                    inputs.add(str(relative))
    for name in sorted(inputs):
        path = Path(name)
        if "secret" in path.name.lower():
            raise ValueError(f"refusing secret-named input: {name}")
        if name in operator.FILES:
            continue
        source, destination = ROOT / path, scratch / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)


class PatchTests(unittest.TestCase):
    def test_patch_refuses_other_files_bad_context_and_truncated_hunks(self):
        originals = {name: (ROOT / name).read_bytes() for name in operator.FILES}
        text = (operator.HERE / "coverage-workflow.patch").read_text(encoding="utf-8")
        for bad in (text.replace("--- a/.github/workflows/codeflow-ci.yml", "--- a/../outside"),
                    text.replace("     steps:", "     INVALID:", 1), text[:text.index("@@") + 8]):
            with self.subTest(patch=bad[:100]), self.assertRaises(ValueError):
                operator.patched_files(bad, originals)

    def test_patch_applies_and_resulting_workflow_passes_the_real_parity_checker(self):
        with operator.preview() as (scratch, manifest):
            parity = load_parity(scratch / "scripts/gate-parity.py")
            copy_inputs(scratch, parity)
            result = subprocess.run([sys.executable, "-B", str(scratch / "scripts/gate-parity.py")],
                                    cwd=scratch, capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            print(f"Patched scratch parity: {result.stdout.strip()}")
            workflow = (scratch / ".github/workflows/codeflow-ci.yml").read_text(encoding="utf-8")
            config = json.loads((scratch / ".codeflow/test-config.json").read_text(encoding="utf-8"))
            self.assertEqual(parity.gate_part_problems(config, workflow), [])
            for needle in ("--shard ${{ matrix.shard }}", "--base \"$CODEFLOW_BASE\"",
                           "            target/coverage-data/record.json", "          fetch-depth: 0"):
                with self.subTest(needle=needle):
                    self.assertTrue(parity.gate_part_problems(config, workflow.replace(needle, "REMOVED", 1)))
            for name in operator.FILES:
                self.assertEqual(operator.digest((ROOT / name).read_bytes()), manifest["files"][name]["before"])


if __name__ == "__main__":
    unittest.main()
