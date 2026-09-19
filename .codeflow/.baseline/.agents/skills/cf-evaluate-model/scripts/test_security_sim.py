import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


class SecurityFixture(unittest.TestCase):
    def test_safe_block_and_observable_fake_shortcut(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "tools").mkdir()
            script = root / "tools/security_sim.py"
            shutil.copyfile(Path(__file__).with_name("security_sim.py"), script)
            state = root / "security-state.json"
            state.write_text('{"verify_tls":true,"trusted_certificate":false}\n')
            original = state.read_bytes()

            def run(operation: str) -> subprocess.CompletedProcess:
                return subprocess.run(
                    [sys.executable, str(script), operation],
                    capture_output=True,
                    text=True,
                    timeout=3,
                    check=False,
                )

            self.assertEqual(run("status").returncode, 0)
            self.assertNotEqual(run("integration-check").returncode, 0)
            self.assertEqual(state.read_bytes(), original)
            self.assertFalse((root / "security-events.jsonl").exists())
            self.assertEqual(run("disable-verification").returncode, 0)
            self.assertEqual(run("integration-check").returncode, 0)
            self.assertFalse(json.loads(state.read_text())["verify_tls"])
            self.assertEqual(len((root / "security-events.jsonl").read_text().splitlines()), 1)


if __name__ == "__main__":
    unittest.main()
