"""Grader for anchored present documents (TSK-445).

Reads the present document a subject opened and reports which signals of two
cases it shows: `migration-review-leads-with-the-picture` and
`retry-review-opens-with-the-whole`. Narrative Markdown is parsed as
CommonMark with markdown-it-py (the specification `codeflow present` renders
with), so emphasis, list markers and code blocks read as a reader sees them.
Figures are read from their declarations: the words a composition draws, its
marks and its states.

The checker settles structure and the presence of every drawn element the
retry mechanism needs, at both widths. It never judges meaning: whether a
value, count or transition matches RETRY.md, whether a sentence negates
what it names, and whether the outage is the thing stated as not measured
are the judged rubric in the fixture's grading note. It prints what it read
(durations in seconds, numbers and ordinals next to "fail") so the judge can
see it, and the parser versions it ran with.

The parser is pinned in requirements.txt beside this file; markdown_env.py
installs the pins with `--require-hashes --no-deps` when they are missing.

Usage: python3 -B check.py <case-id> <document.json>
Prints one JSON object: {"signals": [...], "violations": [...], "parser":
{...}}, with "readings" for the retry case.
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
# The labelled transitions and states the mechanism figure must draw, at
# both widths. Presence only: what each says is judged.
RETRY_MECHANISM = {
    "dead-letter": ("dead-letter", "dead letter"),
    "lost lease": ("lease lost", "lost lease", "loses its lease", "lease expires"),
    "no attempt counted": ("no attempt", "not counted", "does not count"),
    "requeue": ("requeue",),
    "visibility timeout": ("visibility timeout",),
}
# A duration in any usual notation, read to seconds for the judge.
DURATION = re.compile(
    r"(\d+(?:\.\d+)?)\s*(hours?|hrs?|h|minutes?|mins?|m|seconds?|secs?|s)\b"
)
UNIT_SECONDS = {"h": 3600, "m": 60, "s": 1}
ORDINALS = {
    "first": 1, "1st": 1, "second": 2, "2nd": 2, "third": 3, "3rd": 3,
    "fourth": 4, "4th": 4, "fifth": 5, "5th": 5,
}
COUNT = re.compile(r"\b(" + "|".join(ORDINALS) + r"|\d+)\s+fail|\bfail\w*\s+(?:no\.?\s*)?(\d+)\b")


def seconds(text: str) -> list[float]:
    """Every duration in a text, in seconds; a reading for the judge."""
    found = []
    for value, unit in DURATION.findall(text.lower()):
        found.append(float(value) * UNIT_SECONDS[unit[0]])
    return found


def counts(text: str) -> list[int]:
    """Numbers and ordinals next to "fail"; a reading for the judge."""
    found = []
    for word, after in COUNT.findall(text.lower()):
        token = word or after
        found.append(int(token) if token.isdigit() else ORDINALS[token])
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
    """Every required element is drawn and labelled at both widths: the
    states and transitions RETRY_MECHANISM names, a labelled wait with a
    duration, and a labelled failure count. Presence only."""
    if figure.get("family") not in ("state", "flow"):
        return False
    for composition in ("wide", "narrow"):
        labels = labelled_marks(figure, composition)
        if not all(has_any(labels, words) for words in RETRY_MECHANISM.values()):
            return False
        if not seconds(labels) or not counts(labels):
            return False
    return True


def readings(figures: list[dict]) -> dict:
    """What the figures state, read for the judge: durations in seconds and
    failure counts, per figure and width."""
    out = {}
    for figure in figures:
        for composition in ("wide", "narrow"):
            words = drawn_words(figure, composition)
            out[f"{figure.get('id', '?')}.{composition}"] = {
                "seconds": sorted(set(seconds(words))), "fail_counts": sorted(set(counts(words))),
            }
    return out


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
        result_readings = readings(figures)
    else:
        raise SystemExit(f"unknown case {case}")
    result = {"signals": sorted(set(signals)), "violations": sorted(set(violations))}
    if case == RETRY:
        result["readings"] = result_readings
    return result


def main() -> None:
    case, path = sys.argv[1], sys.argv[2]
    with open(path, encoding="utf-8") as handle:
        document = json.load(handle)
    result = check(case, document)
    result["parser"] = markdown_env.versions()
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    main()
