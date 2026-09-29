"""Static description lint: the scent must contain the query's load-bearing words.

These checks read skill descriptions and names only. They run no model and
prove no runtime routing; a behavioural claim needs a native evaluation run.
"""

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
        ROOT / "assets/base/claude/agents" / f"{skill_id}.md",
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


def trigger_scent(description: str) -> str:
    """Positive trigger text; 'do not use' is a collision guard, not scent."""
    return re.split(r"do not use", description, maxsplit=1, flags=re.I)[0]


def all_skill_descriptions() -> dict[str, str]:
    found: dict[str, str] = {}
    for base in (
        ROOT / "assets/base/agents/skills",
        ROOT / "assets/base/claude/skills",
    ):
        if not base.is_dir():
            continue
        for skill_md in sorted(base.glob("*/SKILL.md")):
            skill_id = skill_md.parent.name
            if skill_id in found:
                continue
            text = skill_md.read_text(encoding="utf-8")
            match = re.search(r"(?m)^description:\s*(.+)$", text)
            if match:
                found[skill_id] = match.group(1).strip().strip('"').lower()
    agents = ROOT / "assets/base/claude/agents"
    if agents.is_dir():
        for agent_md in sorted(agents.glob("*.md")):
            skill_id = agent_md.stem
            if skill_id in found:
                continue
            text = agent_md.read_text(encoding="utf-8")
            match = re.search(r"(?m)^description:\s*(.+)$", text)
            if match:
                found[skill_id] = match.group(1).strip().strip('"').lower()
    return found


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

    def test_should_trigger_wins_description_overlap(self) -> None:
        catalog = all_skill_descriptions()
        data = json.loads(TRIGGERS.read_text(encoding="utf-8"))
        for skill in data["skills"]:
            self.assertIn(skill["id"], catalog)
            for prompt in skill["should_trigger"]:
                prompt_tokens = tokens(prompt)
                scores = {
                    skill_id: len(prompt_tokens & tokens(trigger_scent(description)))
                    for skill_id, description in catalog.items()
                }
                target = scores[skill["id"]]
                rivals = {
                    skill_id: score
                    for skill_id, score in scores.items()
                    if skill_id != skill["id"]
                }
                best_rival = max(rivals.values()) if rivals else 0
                self.assertGreater(
                    target,
                    best_rival,
                    f"{prompt!r} should select {skill['id']} "
                    f"(score {target}) over {sorted(rivals.items(), key=lambda item: -item[1])[:5]}",
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
            "cf-estimate",
            "cf-plan",
            "cf-consult",
            "cf-customize",
            "cf-delegate",
            "cf-develop",
            "cf-ship",
            "cf-reviewer",
        ):
            description = skill_description(skill_id)
            self.assertTrue(
                "use when" in description
                or "use for" in description
                or "use after" in description
                or "when herdr_env" in description,
                f"{skill_id} description must carry a when-clause",
            )


if __name__ == "__main__":
    unittest.main()
