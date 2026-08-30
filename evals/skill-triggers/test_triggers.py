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
            scent = {word.lower() for word in skill["scent"]}
            self.assertTrue(
                scent & tokens(description),
                f"{skill['id']} description must contain its scent words {sorted(scent)}",
            )
            for prompt in skill["should_trigger"]:
                overlap = tokens(prompt) & tokens(description) & scent
                self.assertTrue(
                    overlap,
                    f"{skill['id']} description should share scent {sorted(scent)} with {prompt!r}",
                )

    def test_should_not_trigger_avoids_scent_words(self) -> None:
        data = json.loads(TRIGGERS.read_text(encoding="utf-8"))
        for skill in data["skills"]:
            scent = {word.lower() for word in skill["scent"]}
            self.assertTrue(
                skill["should_not_trigger"],
                f"{skill['id']} needs should_not_trigger prompts",
            )
            description = skill_description(skill["id"])
            desc_tokens = tokens(description)
            for prompt in skill["should_not_trigger"]:
                prompt_tokens = tokens(prompt)
                overlap = prompt_tokens & scent
                self.assertFalse(
                    overlap,
                    f"{skill['id']} should not trigger on {prompt!r}; shared scent {sorted(overlap)}",
                )
                shared = (prompt_tokens & desc_tokens) - scent
                self.assertTrue(
                    shared,
                    f"{skill['id']} negative {prompt!r} must share non-scent description words "
                    f"so it is a hard negative, not a disjoint string",
                )

    def test_non_herdr_descriptions_do_not_name_the_tty_host(self) -> None:
        for skill_id in ("cf-consult", "cf-delegate", "cf-plan", "cf-customize"):
            description = skill_description(skill_id)
            self.assertNotIn(
                "herdr",
                description,
                f"{skill_id} description is a load trigger, not a TTY-host dump",
            )

    def test_changed_skills_name_use_when(self) -> None:
        for skill_id in (
            "cf-herdr",
            "cf-plan",
            "cf-consult",
            "cf-customize",
            "cf-delegate",
        ):
            description = skill_description(skill_id)
            self.assertTrue(
                "use when" in description
                or "use for" in description
                or "when herdr_env" in description,
                f"{skill_id} description must carry a when-clause",
            )


if __name__ == "__main__":
    unittest.main()
