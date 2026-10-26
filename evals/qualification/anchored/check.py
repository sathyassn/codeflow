"""Grader for anchored present documents (TSK-445).

Reads the present document a subject opened and reports which signals of two
cases it shows: `migration-review-leads-with-the-picture` and
`retry-review-opens-with-the-whole`. Narrative Markdown is parsed as
CommonMark with markdown-it-py (the specification `codeflow present` renders
with), so emphasis, list markers and code blocks read as a reader sees them.
Figures are read from their declarations: the words a composition draws, its
marks and its states. Whether a fact is invented stays with the judged grade.

The parser is pinned in requirements.txt beside this file; markdown_env.py
installs the pins with `--require-hashes --no-deps` when they are missing.

Usage: python3 -B check.py <case-id> <document.json>
Prints one JSON object: {"signals": [...], "violations": [...]}.
"""

from __future__ import annotations

import json
import re
import sys

sys.path.insert(0, str(__import__("pathlib").Path(__file__).resolve().parent))
import markdown_env  # noqa: E402

markdown_env.ensure()
from markdown_it import MarkdownIt  # noqa: E402

MIGRATION = "migration-review-leads-with-the-picture"
RETRY = "retry-review-opens-with-the-whole"
ASKING = {"decision", "form", "feedback_prompt"}
FIGURES = {"figure", "html"}
PARSER = MarkdownIt("commonmark")

# The retry terms, each with the words that name it in a figure or in prose.
RETRY_TERMS = {
    "lease": ("lease",),
    "attempt": ("attempt",),
    "visibility timeout": ("visibility timeout",),
    "dead-letter queue": ("dead-letter", "dead letter"),
}
# What the retry mechanism figure must draw, at both widths: the dead-letter
# step after a third failure, the backoff, a lost lease that counts no
# attempt, and the operator requeue out of the dead-letter queue.
RETRY_MECHANISM = {
    "dead-letter": ("dead-letter", "dead letter"),
    "lost lease": ("lease lost", "lost lease", "loses its lease", "lease expires"),
    "no attempt counted": ("no attempt", "not counted", "does not count"),
    "requeue": ("requeue",),
    "visibility timeout": ("visibility timeout",),
}
# The facts RETRY.md states, read exactly. Durations are (value, unit) with
# the unit in seconds or minutes, in any of the usual notations ("30 s",
# "30s", "30 sec", "30 seconds"). The mechanism figure draws every one of
# them, and no figure draws another duration or another failure count.
DURATION = re.compile(r"(\d+)\s*(seconds?|secs?|s|minutes?|mins?|m)\b")
RETRY_DURATIONS = {(30, "s"), (60, "s"), (5, "min")}
ORDINALS = {
    "first": 1, "1st": 1, "one": 1, "second": 2, "2nd": 2, "two": 2, "third": 3, "3rd": 3,
    "three": 3, "fourth": 4, "4th": 4, "four": 4, "fifth": 5, "5th": 5, "five": 5,
}
RETRY_DEAD_LETTER_FAILURE = 3
# Words that state a thing was not measured; a sentence about the outage
# carries the uncertainty only with one of them.
UNMEASURED = re.compile(
    r"\b(not (?:been )?(?:measured|tested|run)|unmeasured|untested|no measurement|never measured)\b"
)


def durations(text: str) -> set[tuple[int, str]]:
    found = set()
    for value, unit in DURATION.findall(text.lower()):
        found.add((int(value), "min" if unit.startswith("m") else "s"))
    return found


def failure_counts(text: str) -> set[int]:
    """The failure counts a text states: an ordinal or number next to
    "fail", "failure" or "failed attempt"."""
    found = set()
    pattern = r"\b(" + "|".join(ORDINALS) + r"|\d+)\s+(?:failed|failure|fails?)\b"
    for word in re.findall(pattern, text.lower()):
        found.add(int(word) if word.isdigit() else ORDINALS[word])
    return found


class Narrative:
    """One narrative block's Markdown, parsed: its headings, its top-level
    list items and its top-level paragraphs, with code left out."""

    def __init__(self, markdown: str) -> None:
        tokens = PARSER.parse(markdown)
        self.order: list[str] = []
        self.headings: list[str] = []
        self.items = 0
        self.paragraphs: list[str] = []
        self.list_text: list[str] = []
        depth = 0
        for index, token in enumerate(tokens):
            if token.type in ("bullet_list_open", "ordered_list_open"):
                if depth == 0:
                    self.order.append("list")
                depth += 1
            elif token.type in ("bullet_list_close", "ordered_list_close"):
                depth -= 1
            elif token.type == "list_item_open" and depth == 1:
                self.items += 1
            elif token.type == "heading_open" and depth == 0:
                self.order.append("heading")
                self.headings.append(plain(tokens[index + 1]))
            elif token.type == "paragraph_open":
                text = plain(tokens[index + 1])
                if depth == 0:
                    self.order.append("paragraph")
                    self.paragraphs.append(text)
                else:
                    self.list_text.append(text)
            elif token.type in ("fence", "code_block") and depth == 0:
                self.order.append("code")

    def is_anchor(self) -> bool:
        """A heading, then one list of four or five items, and nothing else."""
        return self.order == ["heading", "list"] and 4 <= self.items <= 5

    def is_section(self) -> bool:
        """A heading, then a lead of at most two sentences."""
        if not self.order or self.order[0] != "heading" or "list" in self.order:
            return False
        return all(kind == "paragraph" for kind in self.order[1:]) and sentences(" ".join(self.paragraphs)) <= 2


def plain(inline) -> str:
    """The words of an inline token as a reader sees them: text and code
    spans, with emphasis and link markup dropped."""
    if inline is None or inline.type != "inline":
        return ""
    return "".join(child.content for child in (inline.children or []) if child.type in ("text", "code_inline")).strip()


def sentences(text: str) -> int:
    return len([part for part in re.split(r"(?<=[.!?])\s+", text.strip()) if part])


def walk(blocks: list[dict]):
    for block in blocks:
        yield block
        yield from walk(block.get("blocks", []))
        for tab in block.get("tabs", []):
            yield from walk(tab.get("blocks", []))


def narrative(block: dict) -> Narrative | None:
    return Narrative(block.get("markdown", "")) if block.get("type") == "narrative" else None


def titles(block: dict) -> list[str]:
    kind = block.get("type")
    if kind in ASKING:
        return []
    parsed = narrative(block)
    found = list(parsed.headings) if parsed else []
    for key in ("title", "summary", "label"):
        if isinstance(block.get(key), str):
            found.append(block[key])
    if kind == "figure":
        found.append(declaration(block).get("title", ""))
    if kind == "comparison":
        found += [column.get("title", "") for column in block.get("columns", [])]
    return found


def declaration(block: dict) -> dict:
    return block.get("declaration", {}).get("figure", {}) if block.get("type") == "figure" else {}


def drawn_words(figure: dict, composition: str) -> str:
    """The words a composition draws, with the figure's title and legend."""
    words = [figure.get("title", "")] + [state.get("means", "") for state in figure.get("states", [])]
    words += [item["text"] for item in figure.get(composition, {}).get("draw", []) if "text" in item]
    return " ".join(words).lower()


def labelled_marks(figure: dict, composition: str) -> str:
    """The words of the texts that label a drawn mark, at one width."""
    draw = figure.get(composition, {}).get("draw", [])
    marks = {item["id"] for item in draw if "state" in item and "id" in item}
    return " ".join(
        item["text"] for item in draw if "text" in item and set(item.get("for", [])) & marks
    ).lower()


def has_any(text: str, words: tuple[str, ...]) -> bool:
    return any(word in text for word in words)


def figure_draws_mechanism(figure: dict) -> bool:
    if figure.get("family") not in ("state", "flow"):
        return False
    for composition in ("wide", "narrow"):
        labels = labelled_marks(figure, composition)
        if not all(has_any(labels, words) for words in RETRY_MECHANISM.values()):
            return False
        if durations(labels) != RETRY_DURATIONS:
            return False
        if failure_counts(labels) != {RETRY_DEAD_LETTER_FAILURE}:
            return False
    return True


def retry_facts_wrong(figures: list[dict]) -> bool:
    """A figure draws a duration or a failure count RETRY.md does not state."""
    for figure in figures:
        for composition in ("wide", "narrow"):
            words = drawn_words(figure, composition) + " " + figure.get("caption", "").lower()
            if not durations(words) <= RETRY_DURATIONS:
                return True
            if not failure_counts(words) <= {RETRY_DEAD_LETTER_FAILURE}:
                return True
    return False


def prose_sentences(blocks: list[dict]) -> list[str]:
    """Every sentence of the document's prose: narrative paragraphs and list
    items, captions and decision prompts."""
    texts = []
    for block in walk(blocks):
        parsed = narrative(block)
        if parsed:
            texts += parsed.paragraphs + parsed.list_text
        figure = declaration(block)
        if figure:
            texts.append(figure.get("caption", ""))
        for key in ("markdown", "caption", "prompt"):
            if block.get("type") != "narrative" and isinstance(block.get(key), str):
                texts.append(block[key])
    return [s for text in texts for s in re.split(r"(?<=[.!?;])\s+", text) if s]


def check(case: str, document: dict) -> dict:
    blocks = document.get("blocks", [])
    signals: list[str] = []
    violations: list[str] = []
    first_figure = next((i for i, b in enumerate(blocks) if b.get("type") in FIGURES), None)
    first = narrative(blocks[0]) if blocks else None
    anchored = bool(first and first.is_anchor())

    # The content block after the anchor, skipping at most one section block.
    after = 1
    if len(blocks) > after and (narrative(blocks[after]) or Narrative("")).is_section():
        after += 1
    governing = blocks[after] if anchored and len(blocks) > after else None

    asks = [b for b in walk(blocks) if b.get("type") in ASKING]
    if len(asks) == 1 and asks[0] is blocks[-1]:
        signals.append("one_ask")
    elif len(asks) > 1:
        violations.append("more_than_one_ask")

    if any(t.strip().endswith("?") for b in walk(blocks) for t in titles(b)):
        violations.append("question_form_section_title")

    before = blocks[: first_figure if first_figure is not None else len(blocks)]
    if case == MIGRATION:
        if governing is not None and (
            declaration(governing).get("family") in ("extent", "coverage") or governing.get("type") == "table"
        ):
            signals.append("anchor_then_governing_comparison")
        prose_first = False
        for index, block in enumerate(before):
            parsed = narrative(block)
            if block.get("type") == "comparison":
                prose_first = True
            elif parsed and not (index == 0 and parsed.is_anchor()) and not parsed.is_section():
                prose_first = True
        if prose_first or first_figure is None:
            violations.append("narrative_first_text_cards_ask_last")
    elif case == RETRY:
        if anchored and (first_figure is None or first_figure > 0):
            signals.append("anchor_lines_before_any_figure")
        whole = declaration(governing) if governing is not None else {}
        if whole and all(has_any(drawn_words(whole, "wide"), words) for words in RETRY_TERMS.values()):
            signals.append("orienting_figure_names_the_parts")
        figures = [declaration(b) for b in walk(blocks) if b.get("type") == "figure"]
        if any(figure_draws_mechanism(figure) for figure in figures):
            signals.append("retry_mechanism_carried_by_a_figure")
        narratives = [(i, narrative(b)) for i, b in enumerate(blocks) if b.get("type") == "narrative"]
        if narratives and all(
            parsed.headings and parsed.order[0] == "heading" for _, parsed in narratives
        ):
            signals.append("sections_titled_in_plain_words")
        # A term is drawn before use when some figure at or above the first
        # block whose prose names it draws it at both widths.
        drawn_by: dict[str, int] = {}
        for index, block in enumerate(blocks):
            figure = declaration(block)
            if figure:
                for term, words in RETRY_TERMS.items():
                    if all(has_any(drawn_words(figure, c), words) for c in ("wide", "narrow")):
                        drawn_by.setdefault(term, index)
        used_at: dict[str, int] = {}
        for index, parsed in narratives:
            if index == 0 and parsed.is_anchor():
                continue
            text = " ".join(parsed.paragraphs + parsed.list_text).lower()
            for term, words in RETRY_TERMS.items():
                if has_any(text, words):
                    used_at.setdefault(term, index)
        if len(drawn_by) == len(RETRY_TERMS) and all(drawn_by[t] <= used_at.get(t, len(blocks)) for t in RETRY_TERMS):
            signals.append("terms_drawn_before_use")
        for block in before:
            parsed = narrative(block)
            if parsed:
                for paragraph in parsed.paragraphs:
                    if sum(has_any(paragraph.lower(), words) for words in RETRY_TERMS.values()) >= 2:
                        violations.append("definitions_paragraph_before_any_figure")
        if not any(b.get("type") in FIGURES for b in walk(blocks)) and any(
            b.get("type") == "table" for b in blocks
        ):
            violations.append("table_as_main_carrier")
        if any("outage" in s.lower() and UNMEASURED.search(s.lower()) for s in prose_sentences(blocks)):
            signals.append("unmeasured_outage_stated")
        if retry_facts_wrong(figures):
            violations.append("fact_invented")
    else:
        raise SystemExit(f"unknown case {case}")
    return {"signals": sorted(set(signals)), "violations": sorted(set(violations))}


def main() -> None:
    case, path = sys.argv[1], sys.argv[2]
    with open(path, encoding="utf-8") as handle:
        document = json.load(handle)
    print(json.dumps(check(case, document), sort_keys=True))


if __name__ == "__main__":
    main()
