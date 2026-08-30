"""Prove the Herdr armed-prompt recipe is executable, not documentation-only."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
import unittest
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERDR_SKILL = ROOT / "assets/base/agents/skills/cf-herdr/SKILL.md"


def herdr(*args: str) -> dict:
    raw = subprocess.check_output(["herdr", *args], text=True)
    return json.loads(raw)


class HerdrDeliveryTests(unittest.TestCase):
    def test_skill_names_send_text_recipe(self) -> None:
        text = HERDR_SKILL.read_text(encoding="utf-8")
        for marker in (
            "herdr pane send-text",
            "herdr pane send-keys",
            "Enter",
            "256 KiB",
            "codeflow delegate arm",
        ):
            self.assertIn(marker, text, f"cf-herdr lost delivery marker {marker!r}")

    def test_herdr_cli_exposes_send_text_and_send_keys(self) -> None:
        if shutil.which("herdr") is None:
            self.skipTest("herdr not on PATH")
        help_text = subprocess.check_output(["herdr", "pane", "--help"], text=True)
        self.assertIn("send-text", help_text)
        self.assertIn("send-keys", help_text)

    def test_live_send_text_round_trip(self) -> None:
        if os.environ.get("HERDR_ENV") != "1":
            self.skipTest("not inside Herdr")
        if shutil.which("herdr") is None:
            self.skipTest("herdr not on PATH")
        workspace = os.environ.get("HERDR_WORKSPACE_ID")
        caller = os.environ.get("HERDR_PANE_ID")
        if not workspace or not caller:
            self.skipTest("missing HERDR_WORKSPACE_ID or HERDR_PANE_ID")

        token = f"cf-herdr-canary-{uuid.uuid4().hex[:8]}"
        created = herdr(
            "tab",
            "create",
            "--workspace",
            workspace,
            "--label",
            f"cf/codeflow/skill-cat/canary/{token[-2:]}",
            "--cwd",
            str(ROOT),
            "--no-focus",
        )
        tab_id = created["result"]["tab"]["tab_id"]
        pane_id = created["result"]["root_pane"]["pane_id"]
        try:
            self.assertNotEqual(pane_id, caller)
            current = herdr("pane", "current", "--current")
            self.assertEqual(
                current["result"]["pane"]["pane_id"],
                caller,
                "tab create stole focus from the caller pane",
            )
            deadline = time.time() + 10
            while time.time() < deadline:
                info = herdr("pane", "process-info", "--pane", pane_id)
                names = [
                    proc.get("name", "")
                    for proc in info["result"]["process_info"].get(
                        "foreground_processes", []
                    )
                ]
                if any(name in {"zsh", "bash", "sh"} for name in names):
                    break
                time.sleep(0.2)
            else:
                self.fail("new pane never reached a shell prompt")
            subprocess.check_call(["herdr", "pane", "send-text", pane_id, f"echo {token}"])
            time.sleep(0.3)
            subprocess.check_call(["herdr", "pane", "send-keys", pane_id, "Enter"])
            subprocess.check_call(
                [
                    "herdr",
                    "pane",
                    "wait-output",
                    pane_id,
                    "--match",
                    token,
                    "--timeout",
                    "15000",
                    "--source",
                    "visible",
                    "--lines",
                    "40",
                ]
            )
        finally:
            subprocess.call(["herdr", "tab", "close", tab_id])


if __name__ == "__main__":
    unittest.main()
