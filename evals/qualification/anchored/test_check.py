"""Controls for check.py (TSK-445): an anchored document passes; one missing
its anchor, its governing figure or a drawn part of the mechanism fails; and
Markdown written differently but read the same grades the same."""

import copy
import json
import unittest
from pathlib import Path

from check import MIGRATION, RETRY, check

CONTROLS = Path(__file__).resolve().parents[2] / "model-artifacts/method-controls"

ANCHOR = {
    "type": "narrative",
    "id": "anchor",
    "markdown": "## What you are looking at\n\n- **What:** the retry design.\n- **Why:** jobs were lost.\n"
    "- **Where it stands:** staged.\n- **You decide:** sign off or not.",
}


def section(ident, title):
    return {"type": "narrative", "id": ident, "markdown": f"## {title}\n\nOne lead line."}


def figure(ident, family):
    return {"type": "figure", "id": ident, "declaration": {"schema_version": 1, "figure": {"id": ident, "family": family, "title": "How a job retries"}}}


DECISION = {"type": "decision", "id": "ask", "title": "Sign off on the design?", "markdown": "Choose.", "options": []}
DEFINITIONS = {
    "type": "narrative",
    "id": "intro",
    "markdown": "A worker holds a lease for the visibility timeout. An attempt is one run. "
    "After three attempts the job goes to the dead-letter queue.",
}


def control(case, name):
    return json.loads((CONTROLS / name).read_text(encoding="utf-8"))


class RetryCase(unittest.TestCase):
    def test_anchored_passing_control_shows_every_signal(self):
        result = check(RETRY, control(RETRY, "retry/passing.json"))
        self.assertEqual(result["violations"], [])
        for signal in ("anchor_lines_before_any_figure", "orienting_figure_names_the_parts",
                       "retry_mechanism_carried_by_a_figure", "sections_titled_in_plain_words",
                       "terms_drawn_before_use", "one_ask", "unmeasured_outage_stated"):
            self.assertIn(signal, result["signals"])

    def test_missing_anchor_fails(self):
        result = check(RETRY, {"blocks": [DEFINITIONS, figure("life", "state"), DECISION]})
        self.assertNotIn("anchor_lines_before_any_figure", result["signals"])
        self.assertIn("definitions_paragraph_before_any_figure", result["violations"])

    def test_missing_figure_fails(self):
        table = {"type": "table", "id": "t", "title": "Retry steps", "columns": ["Step"], "rows": [["lease"]]}
        result = check(RETRY, {"blocks": [ANCHOR, section("steps", "The steps"), table, DECISION]})
        self.assertNotIn("orienting_figure_names_the_parts", result["signals"])
        self.assertNotIn("retry_mechanism_carried_by_a_figure", result["signals"])
        self.assertIn("table_as_main_carrier", result["violations"])

    def test_omitted_requeue_or_timeout_fails_the_mechanism(self):
        for name in ("retry/faulty-no-requeue.json", "retry/faulty-no-timeout.json"):
            with self.subTest(name=name):
                self.assertNotIn("retry_mechanism_carried_by_a_figure", check(RETRY, control(RETRY, name))["signals"])

    def test_a_transition_drawn_only_wide_fails_the_mechanism(self):
        document = control(RETRY, "retry/passing.json")
        life = next(b for b in document["blocks"] if b["id"] == "life")["declaration"]["figure"]
        life["narrow"]["draw"] = [item for item in life["narrow"]["draw"] if item.get("id") != "requeue"
                                  and "requeue" not in item.get("for", [])]
        self.assertNotIn("retry_mechanism_carried_by_a_figure", check(RETRY, document)["signals"])

    def test_question_title_fails(self):
        result = check(RETRY, {"blocks": [ANCHOR, section("whole", "Will it hold?"), figure("whole", "flow"), DECISION]})
        self.assertIn("question_form_section_title", result["violations"])


class Adversarial(unittest.TestCase):
    """Markdown that reads differently from its raw text, and asks hidden in
    nested blocks (Codex round 2 on TSK-445)."""

    def passing(self):
        return control(RETRY, "retry/passing.json")

    def test_bold_question_heading_is_a_question(self):
        document = self.passing()
        document["blocks"][1]["markdown"] = "## **Will it hold?**\n\nOne lead line."
        self.assertIn("question_form_section_title", check(RETRY, document)["violations"])

    def test_second_decision_inside_a_disclosure_is_a_second_ask(self):
        document = self.passing()
        hidden = copy.deepcopy(document["blocks"][-1])
        hidden["id"] = "hidden-ask"
        document["blocks"].insert(-1, {"type": "disclosure", "id": "more", "summary": "More detail", "blocks": [hidden]})
        result = check(RETRY, document)
        self.assertNotIn("one_ask", result["signals"])
        self.assertIn("more_than_one_ask", result["violations"])

    def test_definitions_paragraph_inside_the_anchor_is_not_an_anchor(self):
        document = self.passing()
        document["blocks"][0]["markdown"] = document["blocks"][0]["markdown"].replace(
            "\n\n- ", "\n\nA worker holds a lease for the visibility timeout, and an attempt is one run.\n\n- ", 1)
        result = check(RETRY, document)
        self.assertNotIn("anchor_lines_before_any_figure", result["signals"])
        self.assertIn("definitions_paragraph_before_any_figure", result["violations"])

    def test_plus_bullets_still_make_an_anchor(self):
        document = self.passing()
        document["blocks"][0]["markdown"] = document["blocks"][0]["markdown"].replace("\n- ", "\n+ ")
        self.assertIn("anchor_lines_before_any_figure", check(RETRY, document)["signals"])

    def test_a_question_heading_inside_fenced_code_is_code(self):
        document = self.passing()
        document["blocks"][3]["markdown"] += "\n\n```text\n## Will it hold?\n```"
        self.assertNotIn("question_form_section_title", check(RETRY, document)["violations"])


class MigrationCase(unittest.TestCase):
    def test_anchored_document_passes(self):
        result = check(MIGRATION, {"blocks": [ANCHOR, section("options", "The two options"), figure("options", "coverage"), DECISION]})
        self.assertEqual(result["violations"], [])
        self.assertIn("anchor_then_governing_comparison", result["signals"])

    def test_missing_anchor_fails(self):
        cards = {"type": "comparison", "id": "c", "columns": [{"title": "Dual write", "markdown": "x"}, {"title": "Snapshot", "markdown": "y"}]}
        prose = {"type": "narrative", "id": "p", "markdown": "We compared two options for the orders table and both have costs."}
        result = check(MIGRATION, {"blocks": [prose, cards, DECISION]})
        self.assertNotIn("anchor_then_governing_comparison", result["signals"])
        self.assertIn("narrative_first_text_cards_ask_last", result["violations"])

    def test_missing_governing_figure_fails(self):
        cards = {"type": "comparison", "id": "c", "columns": [{"title": "Dual write", "markdown": "x"}, {"title": "Snapshot", "markdown": "y"}]}
        result = check(MIGRATION, {"blocks": [ANCHOR, section("options", "The two options"), cards, DECISION]})
        self.assertNotIn("anchor_then_governing_comparison", result["signals"])
        self.assertIn("narrative_first_text_cards_ask_last", result["violations"])


class CommittedControls(unittest.TestCase):
    """The signals check.py reads agree with the committed control answers:
    for the retry case, every signal it records; for the migration case, the
    order and ask signals (its facts stay with the judged grade)."""

    MIGRATION_READ = {
        "anchor_then_governing_comparison", "one_ask", "narrative_first_text_cards_ask_last",
        "question_form_section_title", "more_than_one_ask",
    }

    def test_controls_agree(self):
        controls = json.loads((CONTROLS / "controls.json").read_text(encoding="utf-8"))
        for case in (MIGRATION, RETRY):
            for entry in controls["cases"][case]:
                with self.subTest(case=case, answer=entry["answer"]):
                    result = check(case, control(case, entry["answer"]))
                    found = set(result["signals"]) | set(result["violations"])
                    recorded = set(entry["signals"])
                    if case == MIGRATION:
                        found &= self.MIGRATION_READ
                        recorded &= self.MIGRATION_READ
                    self.assertEqual(recorded, found)


if __name__ == "__main__":
    unittest.main()
