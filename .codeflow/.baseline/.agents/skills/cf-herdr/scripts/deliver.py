#!/usr/bin/env python3
"""Deliver an armed prompt file to a Herdr seat and confirm the turn started.

Usage:
  python3 deliver.py --pane <pane_id> --file <prompt> [--lifecycle]
                     [--review <base>...<head>]
                     [--start-timeout 20] [--settle 2]

Every lane first reads the pane (`herdr pane get`) and stops before sending
anything when the seat's working folder no longer exists or the prompt is
over 256 KiB.

A seat without the lifecycle hook (Codex, Grok) must not be `working`,
`blocked` or `unknown` before the send. After one Enter it must reach
`working` or `blocked`, or show a newer `state_change_seq` with `done`,
within --start-timeout seconds (`herdr agent get`). Otherwise the script
stops and reports the turn not confirmed, naming the pane. Herdr 0.9.0
exposes no input line or cursor, so a prompt waiting in the input cannot be
told from the same words already submitted: it never sends a second Enter
and never resends the prompt.

--lifecycle (tracked Claude): send, fold sentence when this paste added a
fold to the screen, Enter; the lifecycle's `accepted` wait stays the
caller's next step.

--review <base>...<head> (a unit review): before anything is sent, the
brief must name that full range, every dotted pair it names must be that
pair, and every git diff, difftool, log or show must sit in a code span or
fenced block as exactly `git <verb> [display options] <base>...<head>` (the
pair dotted or as two tokens, any case; a path, `--`, a glob, a magic
pathspec, another revision or any other option is refused). A command
outside a span is refused with the span form named; text outside a span or
fence is prose and is never read as part of a command. A backslash joins the
next line before anything is read. A review is one holistic pass over the
whole unit at one head.

Exit codes: 0 started or sent; 1 herdr error; 2 usage or oversize;
3 seat folder missing; 4 turn not confirmed; 5 seat busy or unknown;
6 review brief narrower than the unit.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time

LIMIT = 256 * 1024
FOLD = "[Pasted"
FOLD_SENTENCE = "Carry out the pasted instructions."
STARTED = {"working", "blocked"}
BUSY = STARTED | {"unknown"}
SHA = r"[0-9a-f]{7,40}"
PAIR = re.compile(rf"^({SHA})\.\.\.({SHA})$")
HEX = r"[0-9a-fA-F]{7,40}"
DOTTED = re.compile(rf"(?<![0-9A-Za-z])({HEX})(\.\.\.?)({HEX})(?![0-9A-Za-z])")
ONE_PAIR = re.compile(rf"({HEX})(\.\.\.?)({HEX})")
ONE_REV = re.compile(HEX)
# A git command that shows a diff, a log or a commit, with any global options
# between `git` and the verb.
MENTION = re.compile(
    r"\bgit(?:\s+-\S*(?:\s+[^-\s]\S*)?)*\s+(diff|difftool|log|show)\b", re.I)
# Options that change how a diff, log or show is shown and select no commits or
# paths; every other dash option can narrow the unit and is refused.
DISPLAY = re.compile(
    r"-U\d*|--unified=\d+|-w|-b|--ignore-all-space|--ignore-space-change|-p|--patch"
    r"|--stat(=\S*)?|--shortstat|--numstat|--name-only|--name-status|--summary"
    r"|--no-color|--color(=\S*)?|--oneline|--minimal|--patience|--histogram"
    r"|--no-ext-diff|--no-pager|--remerge-diff|-M\d*%?|--find-renames(=\S*)?")
FENCE = "```"
CUT = "\0"  # stands where a span or fence was cut out, so prose cannot join across it
DISCIPLINE = ".codeflow/rules/workflow-discipline.md, Review verdicts"


class Stop(Exception):
    def __init__(self, code: int, message: str) -> None:
        super().__init__(message)
        self.code = code


def herdr(*args: str) -> dict:
    done = subprocess.run(["herdr", *args], capture_output=True, text=True)
    if done.returncode != 0:
        detail = (done.stderr or done.stdout).strip()
        raise Stop(1, f"herdr {' '.join(args[:2])} failed: {detail}")
    try:
        return json.loads(done.stdout) if done.stdout.strip() else {}
    except json.JSONDecodeError as error:
        raise Stop(1, f"herdr {' '.join(args[:2])} returned no JSON: {error}")


def folds(pane: str) -> int:
    """How many folded pastes the pane's screen shows."""
    done = subprocess.run(["herdr", "pane", "read", pane, "--source", "visible"],
                          capture_output=True, text=True)
    if done.returncode != 0:
        raise Stop(1, f"herdr pane read failed: {(done.stderr or done.stdout).strip()}")
    return done.stdout.count(FOLD)


def agent(pane: str) -> tuple[str, int]:
    info = herdr("agent", "get", pane)["result"]["agent"]
    return info.get("agent_status", "unknown"), int(info.get("state_change_seq", 0))


def started(pane: str, seq: int, bound: float) -> str | None:
    deadline = time.monotonic() + bound
    while True:
        status, now = agent(pane)
        if status in STARTED or (status == "done" and now > seq):
            return status
        if time.monotonic() >= deadline:
            return None
        time.sleep(min(0.5, max(bound / 10, 0.05)))


def same(a: str, b: str) -> bool:
    """Two abbreviations of one commit: either is a prefix of the other, in
    any case."""
    a, b = a.lower(), b.lower()
    return a.startswith(b) or b.startswith(a)


def narrower(shown: str, unit: str) -> Stop:
    return Stop(6, f"review brief asks for {shown}, narrower than the unit; "
                   "findings from earlier rounds are checks within the "
                   "whole-unit pass, never its scope "
                   "(cf-model-orchestrator/resources/quality/findings.md); "
                   f"re-brief for {unit}; nothing was sent")


def carve(text: str) -> tuple[list[str], str]:
    """Split a brief into command texts and prose. A fenced block (a line
    starting with three backticks to the next such line) is cut out and each
    of its lines is a command line; a code span (backticks paired in document
    order, newlines allowed) is cut out and is a command span; what remains
    is prose."""
    commands: list[str] = []
    prose: list[str] = []
    fenced = False
    for line in text.split("\n"):
        if line.lstrip().startswith(FENCE) and (fenced or line.count(FENCE) == 1):
            fenced = not fenced
            if not fenced:
                prose.append(CUT)
        elif fenced:
            commands.append(line)
        else:
            prose.append(line)
    rest = "\n".join(prose)
    commands += re.findall(r"`([^`]*)`", rest)
    return commands, re.sub(r"`[^`]*`", CUT, rest)


def fits(tokens: list[str], base: str, head: str, verb: str, unit: str) -> bool:
    """Whether the tokens after `git VERB` are `DISPLAY* (PAIR DISPLAY*)?`,
    PAIR being `X...Y`, `X..Y` or two tokens `X Y`. A pair of other commits is
    refused as narrower than the unit."""
    def display(index: int) -> int:
        while index < len(tokens) and DISPLAY.fullmatch(tokens[index]):
            index += 1
        return index

    index = display(0)
    one = ONE_PAIR.fullmatch(tokens[index]) if index < len(tokens) else None
    two = (index + 1 < len(tokens) and ONE_REV.fullmatch(tokens[index])
           and ONE_REV.fullmatch(tokens[index + 1]))
    if one or two:
        first, second = ((one.group(1), one.group(3)) if one
                         else (tokens[index], tokens[index + 1]))
        if not (same(first, base) and same(second, head)):
            raise narrower(f"{first}..{second} in a git {verb}", unit)
        index = display(index + (1 if one else 2))
    return index == len(tokens)


def check_review(brief: str, unit: str, path: str) -> None:
    base, head = PAIR.match(unit).groups()
    text = re.sub(r"\\[ \t]*\n[ \t]*", " ", brief.replace("\r\n", "\n"))
    named = any(same(m.group(1), base) and same(m.group(3), head)
                for m in DOTTED.finditer(text) if m.group(2) == "...")
    if not named:
        raise Stop(6, f"review brief {path} does not name its full range "
                      f"{unit}; a review is one holistic pass over the whole "
                      f"unit at one head ({DISCIPLINE}); nothing was sent")
    for dotted in DOTTED.finditer(text):
        if not (same(dotted.group(1), base) and same(dotted.group(3), head)):
            raise narrower(f"{dotted.group(1)}..{dotted.group(3)}", unit)
    commands, prose = carve(text)
    outside = MENTION.search(prose)
    if outside:
        verb = outside.group(1).lower()
        shown = " ".join(prose[outside.start():outside.start() + 60]
                         .split(CUT)[0].split())
        raise Stop(6, f"review brief {path} writes a git {verb} outside a code "
                      f"span ({shown}); a command is read only in a code span "
                      f"or fenced block, as git {verb} <base>...<head> with "
                      "display options only; a review is one holistic pass "
                      f"over the whole unit at one head ({DISCIPLINE}); "
                      "nothing was sent")
    for command in commands:
        found = MENTION.search(command)
        if found is None:
            continue
        verb = found.group(1).lower()
        tokens = command[found.start():].split()
        if tokens[1].lower() != verb or not fits(tokens[2:], base, head, verb, unit):
            span = " ".join(tokens)
            span = span if len(span) <= 80 else span[:77] + "..."
            raise Stop(6, f"review brief {path} writes `{span}`; a git diff, "
                          "difftool, log or show in a code span or fence is "
                          "exactly git <verb> [display options] "
                          "<base>...<head>, so a path, --, a glob, a magic "
                          "pathspec, another revision, a global or a "
                          "non-display option is refused; a review is one "
                          "holistic pass over the whole unit at one head "
                          f"({DISCIPLINE}); re-brief for {unit}; nothing was sent")


def deliver(args: argparse.Namespace) -> str:
    try:
        with open(args.file, encoding="utf-8") as handle:
            prompt = handle.read()
    except OSError as error:
        raise Stop(2, f"cannot read {args.file}: {error}")
    if len(prompt.encode("utf-8")) > LIMIT:
        raise Stop(2, f"{args.file} is over 256 KiB; nothing was sent. Write "
                      "the brief to a file outside the repository that the "
                      "seat can read, and deliver a short prompt that names "
                      "it (cf-herdr).")
    if args.review:
        check_review(prompt, args.review, args.file)

    pane = herdr("pane", "get", args.pane)["result"]["pane"]
    folder = pane.get("foreground_cwd") or pane.get("cwd")
    if folder and not os.path.isdir(folder):
        raise Stop(3, f"pane {args.pane} works in {folder}, which no longer "
                      "exists; nothing was sent. To relaunch: close that tab, "
                      "then create a new tab with --cwd in a live worktree and "
                      "start the seat again (cf-herdr, Create or resume).")
    if args.lifecycle:
        before = folds(args.pane)
    else:
        status, seq = agent(args.pane)
        if status in BUSY:
            raise Stop(5, f"pane {args.pane} is {status}; nothing was sent. "
                          "Harvest or answer it first.")

    herdr("pane", "send-text", args.pane, prompt)
    time.sleep(args.settle)  # input settle; not completion detection
    if args.lifecycle:
        # A fold already on screen may be an earlier turn's; only one this
        # paste added gets the sentence.
        if folds(args.pane) > before:
            herdr("pane", "send-text", args.pane, FOLD_SENTENCE)
            time.sleep(0.3)
        herdr("pane", "send-keys", args.pane, "Enter")
        return (f"pane {args.pane}: prompt sent; next `codeflow delegate wait "
                "--until accepted` for this turn")

    herdr("pane", "send-keys", args.pane, "Enter")
    state = started(args.pane, seq, args.start_timeout)
    if state:
        return f"pane {args.pane} started ({state})"
    raise Stop(4, f"turn not confirmed on pane {args.pane}: the seat did not "
                  f"start within {args.start_timeout:g}s of one Enter. Herdr "
                  "cannot show whether the prompt still waits in the input or "
                  "was submitted, so no second Enter was sent. Inspect with "
                  f"`herdr agent read {args.pane}` and `herdr agent get "
                  f"{args.pane}`; never resend blindly.")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--pane", required=True)
    parser.add_argument("--file", required=True)
    parser.add_argument("--lifecycle", action="store_true",
                        help="tracked Claude seat: the lifecycle confirms")
    parser.add_argument("--review", metavar="<base>...<head>",
                        help="a unit review: refuse a brief narrower than this range")
    parser.add_argument("--start-timeout", type=float, default=20.0)
    parser.add_argument("--settle", type=float, default=2.0)
    args = parser.parse_args()
    if args.review and not PAIR.match(args.review):
        parser.error("--review takes <base>...<head>, each 7 to 40 lowercase "
                     "hex digits")
    try:
        print(f"cf-herdr deliver: {deliver(args)}")
    except Stop as stop:
        print(f"cf-herdr deliver: {stop}", file=sys.stderr)
        return stop.code
    return 0


if __name__ == "__main__":
    sys.exit(main())
