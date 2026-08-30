"""Description-trigger checks: the scent must contain the query's load-bearing words."""

from __future__ import annotations

import json
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TRIGGERS = Path(__file__).with_name("triggers.json")


def skill_description(skill_id: str) -> str:
    candidates = [
        ROOT / "assets/base/agents/skills" / skill_id / "SKILL.md",
        ROOT / "assets/base/claude/skills" / skill_id / "SKILL.md",
    ]
    path = next((item for item in candidates if item.is_file()), None)
    if path is None:
        raise FileNotFoundError(skill_id)
    text = path.read_text(encoding="utf-8")
    match = re.search(r"(?m)^description:\s*(.+)$", text)
    if match is None:
        raise AssertionError(f"{skill_id} missing description")
    return match.group(1).strip().strip('"').lower()


def tokens(prompt: str) -> set[str]:
    return {part for part in re.findall(r"[a-z0-9]+", prompt.lower()) if len(part) > 3}


class TriggerTests(unittest.TestCase):
    def test_should_trigger_shares_scent_words(self) -> None:
        data = json.loads(TRIGGERS.read_text(encoding="utf-8"))
        for skill in data["skills"]:
            description = skill_description(skill["id"])
            for prompt in skill["should_trigger"]:
                overlap = tokens(prompt) & tokens(description)
                self.assertTrue(
                    overlap,
                    f"{skill['id']} description should share trigger words with {prompt!r}",
                )

    def test_cf_herdr_and_cf_plan_name_use_when(self) -> None:
        for skill_id in ("cf-herdr", "cf-plan", "cf-consult"):
            description = skill_description(skill_id)
            self.assertTrue(
                "use when" in description or "when herdr_env" in description,
                f"{skill_id} description must carry a when-clause",
            )


if __name__ == "__main__":
    unittest.main()
