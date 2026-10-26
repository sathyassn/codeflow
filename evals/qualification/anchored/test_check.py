"""Controls for check.py: an anchored document passes the structural signals;
one missing its anchor or its governing figure fails them (TSK-445)."""

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


class RetryCase(unittest.TestCase):
    def test_anchored_document_passes(self):
        result = check(RETRY, {"blocks": [ANCHOR, section("whole", "The whole retry path"), figure("whole", "flow"),
                                         section("life", "How one job lives"), figure("life", "state"), DECISION]})
        self.assertEqual(result["violations"], [])
        for signal in ("anchor_lines_before_any_figure", "orienting_figure_names_the_parts",
                       "retry_mechanism_carried_by_a_figure", "sections_titled_in_plain_words", "one_ask"):
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

    def test_question_title_fails(self):
        result = check(RETRY, {"blocks": [ANCHOR, section("whole", "Will it hold?"), figure("whole", "flow"), DECISION]})
        self.assertIn("question_form_section_title", result["violations"])


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
    """The structural signals check.py reads agree with the committed control
    answers of both cases: every structural signal a control records is
    found, and every structural violation it records is found."""

    STRUCTURAL = {
        "anchor_then_governing_comparison", "anchor_lines_before_any_figure",
        "orienting_figure_names_the_parts", "retry_mechanism_carried_by_a_figure",
        "sections_titled_in_plain_words", "one_ask", "narrative_first_text_cards_ask_last",
        "definitions_paragraph_before_any_figure", "table_as_main_carrier",
        "question_form_section_title", "more_than_one_ask",
    }

    def test_controls_agree(self):
        controls = json.loads((CONTROLS / "controls.json").read_text(encoding="utf-8"))
        for case in (MIGRATION, RETRY):
            for entry in controls["cases"][case]:
                with self.subTest(case=case, answer=entry["answer"]):
                    document = json.loads((CONTROLS / entry["answer"]).read_text(encoding="utf-8"))
                    result = check(case, document)
                    found = set(result["signals"]) | set(result["violations"])
                    self.assertEqual(self.STRUCTURAL & set(entry["signals"]), self.STRUCTURAL & found)


if __name__ == "__main__":
    unittest.main()
