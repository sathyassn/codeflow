#!/usr/bin/env python3
"""Run the configured portal command through cmd.exe with its built binary."""

import json
import os
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]
CONFIG = json.loads((ROOT / ".codeflow/test-config.json").read_text(encoding="utf-8"))


class WindowsPortalTarget(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "requires cmd.exe and the Windows CI Node install")
    def test_docs_portal_target_runs_through_cmd_on_windows(self):
        # The target ends with `$CODEFLOW_BIN validate --portal`, which
        # `codeflow test` supplies; run directly, the caller must.
        if not os.environ.get("CODEFLOW_BIN"):
            self.fail("set CODEFLOW_BIN to a built codeflow: the docs-portal target validates the portal with it")
        command = next(t for t in CONFIG["targets"] if t["name"] == "docs-portal")["modes"]["full"]["command"]
        run = subprocess.run(f'cmd.exe /D /S /C "{command}"', cwd=ROOT,
                             capture_output=True, text=True, check=False)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("with-node: Node 24.18.0", run.stderr)


if __name__ == "__main__":
    unittest.main()
