"""Structural grader for anchored present documents (TSK-445).

Reads the present document a subject opened and reports which structural
signals of two cases it shows: `migration-review-leads-with-the-picture`
and `retry-review-opens-with-the-whole`. It reads block order, block types,
headings and anchor lines only. Whether a figure is the right one, whether a
fact is invented and whether terms are drawn before use stay with the
judged grade; this script only decides what structure can settle.

Usage: python3 -B check.py <case-id> <document.json>
Prints one JSON object: {"signals": [...], "violations": [...]}.
"""

from __future__ import annotations

import json
import re
import sys

MIGRATION = "migration-review-leads-with-the-picture"
RETRY = "retry-review-opens-with-the-whole"
ASKING = {"decision", "form", "feedback_prompt"}
RETRY_TERMS = ("lease", "attempt", "visibility timeout", "dead-letter")


def headings(markdown: str) -> list[str]:
    return [line.lstrip("#").strip() for line in markdown.splitlines() if line.startswith("#")]


def list_items(markdown: str) -> list[str]:
    return [line for line in markdown.splitlines() if re.match(r"\s*[-*] ", line)]


def prose(markdown: str) -> str:
    kept = [line for line in markdown.splitlines() if not line.startswith("#")]
    return re.sub(r"`[^`]*`", "", " ".join(kept))


def sentences(text: str) -> int:
    return len([part for part in re.split(r"(?<=[.!?])\s+", text.strip()) if part])


def titles(block: dict) -> list[str]:
    kind = block.get("type")
    if kind in ASKING:
        return []
    found = []
    if kind == "narrative":
        found += headings(block.get("markdown", ""))
    for key in ("title", "summary", "label"):
        if isinstance(block.get(key), str):
            found.append(block[key])
    if kind == "figure":
        found.append(block.get("declaration", {}).get("figure", {}).get("title", ""))
    for column in block.get("columns", []) if kind == "comparison" else []:
        found.append(column.get("title", ""))
    return found


def walk(blocks: list[dict]):
    for block in blocks:
        yield block
        yield from walk(block.get("blocks", []))
        for tab in block.get("tabs", []):
            yield from walk(tab.get("blocks", []))


def is_anchor(block: dict) -> bool:
    if block.get("type") != "narrative":
        return False
    markdown = block.get("markdown", "")
    return markdown.lstrip().startswith("## ") and 4 <= len(list_items(markdown)) <= 5


def is_section(block: dict) -> bool:
    """A section block: a heading and a lead of at most two sentences."""
    if block.get("type") != "narrative":
        return False
    markdown = block.get("markdown", "")
    return markdown.lstrip().startswith("#") and not list_items(markdown) and sentences(prose(markdown)) <= 2


def family(block: dict) -> str | None:
    if block.get("type") != "figure":
        return None
    return block.get("declaration", {}).get("figure", {}).get("family")


def check(case: str, document: dict) -> dict:
    blocks = document.get("blocks", [])
    signals: list[str] = []
    violations: list[str] = []
    first_figure = next((i for i, b in enumerate(blocks) if b.get("type") in ("figure", "html")), None)
    anchored = bool(blocks) and is_anchor(blocks[0])

    # The content block after the anchor, skipping at most one section block.
    after = 1
    if len(blocks) > after and is_section(blocks[after]):
        after += 1
    governing = blocks[after] if anchored and len(blocks) > after else None

    asks = [b for b in blocks if b.get("type") in ("decision", "feedback_prompt")]
    if len(asks) == 1 and asks[-1] is blocks[-1]:
        signals.append("one_ask")
    elif len(asks) > 1:
        violations.append("more_than_one_ask")

    if any(t.strip().endswith("?") for b in walk(blocks) for t in titles(b)):
        violations.append("question_form_section_title")

    before = blocks[: first_figure if first_figure is not None else len(blocks)]
    if case == MIGRATION:
        if governing is not None and (
            family(governing) in ("extent", "coverage") or governing.get("type") == "table"
        ):
            signals.append("anchor_then_governing_comparison")
        prose_first = any(
            b.get("type") == "comparison"
            or (b.get("type") == "narrative" and not is_anchor(b) and not is_section(b))
            for b in before
        )
        if prose_first or first_figure is None:
            violations.append("narrative_first_text_cards_ask_last")
    elif case == RETRY:
        if anchored and (first_figure is None or first_figure > 0):
            signals.append("anchor_lines_before_any_figure")
        if governing is not None and governing.get("type") in ("figure", "html"):
            signals.append("orienting_figure_names_the_parts")
        if any(family(b) in ("state", "flow") for b in walk(blocks)):
            signals.append("retry_mechanism_carried_by_a_figure")
        narratives = [b for b in blocks if b.get("type") == "narrative" and not is_anchor(b)]
        if narratives and all(b.get("markdown", "").lstrip().startswith("#") for b in narratives):
            signals.append("sections_titled_in_plain_words")
        for b in before:
            if b.get("type") == "narrative" and not is_anchor(b):
                text = prose(b.get("markdown", "")).lower()
                if sum(term in text for term in RETRY_TERMS) >= 2:
                    violations.append("definitions_paragraph_before_any_figure")
                    break
        if not any(b.get("type") in ("figure", "html") for b in walk(blocks)) and any(
            b.get("type") == "table" for b in blocks
        ):
            violations.append("table_as_main_carrier")
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
