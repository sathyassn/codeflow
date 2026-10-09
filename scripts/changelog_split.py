#!/usr/bin/env python3
"""Move pending changelog entries out of the written section into fragments
(ADR-0082).

    changelog_split.py split
        Write every entry of CHANGELOG.md's pending section to
        changelog.d/000-NN-<label-slug>.md under its kind heading, remove the
        section, and check that the fragments compose back to the same bytes
        below the heading.

    changelog_split.py --only-new-against <ref> --name <name> [--from <ref>]
        For an open pull request at the main merge it owes: write the
        entries of its pending section whose labels the target's pending
        entries lack into changelog.d/<name>.md, and report every entry it
        edits under a label the target already carries, naming the fragment
        that carries it, so the edit is applied there by hand.

The script is temporary: a tracked item removes it once the open pull
requests have moved their entries.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
from pathlib import Path
import re
import sys
from typing import Any

SCRIPT = Path(__file__).with_name("release.py")
SPEC = importlib.util.spec_from_file_location("codeflow_release", SCRIPT)
assert SPEC and SPEC.loader
release = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release
SPEC.loader.exec_module(release)


def baseline(root: Path) -> Any:
    config = release.load_config(root / release.CONFIG_PATH)
    return release.resolve_baseline(config, release.structural_state(config, cwd=root), cwd=root)


def slug(label: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", label.casefold())[:48].strip("-") or "entry"


def fragment_text(entries: list[Any]) -> str:
    blocks = []
    for kind in release.CHANGE_KINDS:
        items = [f"{entry.marker}\n{entry.text}" for entry in entries if entry.kind == kind]
        if items:
            blocks.append(f"### {kind}\n\n" + "\n\n".join(items))
    return "\n\n".join(blocks) + "\n"


def pending_entries(text: str, version: str) -> list[Any]:
    section = release.pending_section(text, release.Baseline(version, "", "", ""), repair=False)
    if section is None:
        return []
    return release.fragment_entries("the pending section of CHANGELOG.md", section.body)


def split(root: Path) -> dict[str, Any]:
    base = baseline(root)
    if release.read_fragments(root, cwd=root):
        release.fail(f"{release.FRAGMENT_DIR} already holds fragments; split only a written section")
    path = root / "CHANGELOG.md"
    original = path.read_text(encoding="utf-8")
    section = release.pending_section(original, base, repair=False)
    if section is None:
        return {"status": "no pending section", "fragments": 0}
    entries = release.fragment_entries("the pending section of CHANGELOG.md", section.body)
    width = max(2, len(str(len(entries))))
    names: dict[str, str] = {}
    for index, entry in enumerate(entries, start=1):
        name = f"000-{index:0{width}d}-{slug(entry.label)}.md"
        if slug(entry.label) in names:
            release.fail(f"labels '{names[slug(entry.label)]}' and '{entry.label}' give one slug")
        names[slug(entry.label)] = entry.label
        destination = root / release.FRAGMENT_DIR / name
        destination.parent.mkdir(exist_ok=True)
        destination.write_text(fragment_text([entry]), encoding="utf-8")
    path.write_text(original[: section.start] + original[section.end :], encoding="utf-8")
    composed = release.pending_text(root, base.version, cwd=root)
    heading = original[section.start : section.body_start]
    bumped = heading.replace(f"[{section.version}]", f"[{composed.version}]", 1)
    expected = original[: section.start] + bumped + original[section.body_start :]
    if composed.text != expected:
        release.fail("the fragments do not compose back to the pending section; nothing is proven")
    return {
        "status": "split",
        "fragments": len(entries),
        "version": composed.version,
        "heading": {"before": heading.strip(), "after": bumped.strip()},
        "identical_below_heading": True,
    }


def only_new(root: Path, target: str, name: str, source: str | None) -> dict[str, Any]:
    if not release.FRAGMENT_NAME.fullmatch(f"{name}.md"):
        release.fail(f"{name} is not a fragment name")
    destination = root / release.FRAGMENT_DIR / f"{name}.md"
    if destination.exists():
        release.fail(f"{destination.relative_to(root)} exists; choose another --name")
    base = baseline(root)
    text = (
        release.file_at_ref(source, "CHANGELOG.md", cwd=root).decode()
        if source
        else (root / "CHANGELOG.md").read_text(encoding="utf-8")
    )
    composed = release.pending_text(target, base.version, cwd=root)
    section = release.pending_section(composed.text, base, repair=False)
    config = release.load_config(root / release.CONFIG_PATH)
    carried = release.pending_items(section, config) if section else {}
    holder = {
        release.label_key(label): path for path, labels in composed.fragments.items() for label in labels
    }
    added, edited = [], []
    for entry in pending_entries(text, base.version):
        key = release.label_key(entry.label)
        if key not in carried:
            added.append(entry)
        elif (carried[key].impact, carried[key].text) != (entry.impact, release.entry_extent(entry.text)):
            edited.append({"label": entry.label, "carried_in": holder.get(key, "CHANGELOG.md")})
    if added:
        destination.parent.mkdir(exist_ok=True)
        destination.write_text(fragment_text(added), encoding="utf-8")
    return {
        "status": "moved",
        "written": str(destination.relative_to(root)) if added else None,
        "added": [entry.label for entry in added],
        "edited": edited,
        "note": "apply each edited entry to the fragment named in carried_in" if edited else "",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("mode", nargs="?", choices=["split"])
    parser.add_argument("--root", type=Path, default=SCRIPT.parent.parent)
    parser.add_argument("--only-new-against", metavar="REF")
    parser.add_argument("--name")
    parser.add_argument("--from", dest="source", metavar="REF", help="read CHANGELOG.md at REF, not the worktree")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        if args.mode == "split" and not args.only_new_against:
            result = split(root)
        elif args.mode is None and args.only_new_against and args.name:
            result = only_new(root, args.only_new_against, args.name, args.source)
        else:
            parser.error("give `split`, or `--only-new-against <ref> --name <name>`")
    except (release.ReleaseError, OSError) as error:
        print(f"changelog split error: {error}", file=sys.stderr)
        return 2
    print(json.dumps(result, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
