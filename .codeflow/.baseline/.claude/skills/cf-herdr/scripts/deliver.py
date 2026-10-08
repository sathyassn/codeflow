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
brief must name that full range, and every commit range it names (`git
diff` or `git log` with two commits, or a dotted pair) must be that pair.
A `git diff` or `git log` narrowed in any other way is refused too: a path,
a glob (`*.rs`), a magic pathspec (`:!docs`, `:(exclude)docs`), `--` or a
bare name before or after the pair (after the closing backtick, in a fenced
block, on the next line, or after a backslash continuation), git global
options before the verb, and any dash option outside the display set
(`--stat`, `--name-only`, `-U`, `-w` and the like). A blank line ends the
command, except that a `--` after a blank line is still refused. A trailing
backslash joins the next line, including one between `git` and the verb.
Outside a code span or fence, any word after the pair is read as a path
unless sentence punctuation, a bare number (a count such as "(189 files)")
or a plain prose word (`and`, `then`, `read`, `reply`) ends the command
first, so "and read <file>" still sends. A review is one holistic pass over
the whole unit at one head.

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
DOTTED = re.compile(rf"(?<![0-9A-Za-z])({SHA})(\.\.\.?)({SHA})(?![0-9A-Za-z])")
COMMAND = re.compile(
    r"\bgit(?:\s+(?:-[Cc]\s+\S+"
    r"|--(?:git-dir|work-tree|namespace|super-prefix|config-env|exec-path)\s+\S+"
    r"|--[\w-]+(?:=\S+)?|-[A-Za-z]))*\s+(?P<verb>diff|log)\b")
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
    """Two abbreviations of one commit: either is a prefix of the other."""
    return a.startswith(b) or b.startswith(a)


def is_path(token: str) -> bool:
    return "/" in token or bool(re.fullmatch(r"[\w-]+\.[\w.-]+", token))


def is_pathspec(token: str) -> bool:
    """A path, a glob (`*.rs`) or a magic pathspec (`:!docs`, `:(exclude)x`),
    with one layer of matching quotes removed."""
    if len(token) > 1 and token[0] == token[-1] and token[0] in "'\"":
        token = token[1:-1]
    return (is_path(token) or bool(re.search(r"[*?\[]", token))
            or bool(re.match(r":[!^/(]", token)))


def starts_pathspec(token: str) -> bool:
    """`is_pathspec` for the first word of a line, where a markdown bullet,
    emphasis or link is not a glob."""
    token = token.rstrip(TRAILING)
    if re.fullmatch(r"[*_]+(\w[\w:'-]*[*_]*)?|\[.*", token) and not is_path(token):
        return False
    return is_pathspec(token)


REVISION = re.compile(
    rf"{SHA}|(?:HEAD|@|FETCH_HEAD|ORIG_HEAD)(?:[~^][0-9]*)*|{SHA}(?:[~^][0-9]*)+|\S+\.\.\.?\S+")
TRAILING = ".,;:)"
# Options that change how a diff or log is shown and select no commits or
# paths; every other dash option can narrow the unit and is refused.
DISPLAY = re.compile(
    r"-U\d*|--unified=\d+|-w|-b|--ignore-all-space|--ignore-space-change|-p|--patch"
    r"|--stat(=\S*)?|--shortstat|--numstat|--name-only|--name-status|--summary"
    r"|--no-color|--color(=\S*)?|--oneline|--minimal|--patience|--histogram"
    r"|--no-ext-diff|--no-pager|-M\d*%?|--find-renames(=\S*)?")
OPTION = re.compile(r"-{1,2}[A-Za-z0-9]")
# After the revisions, a word outside this list is read as a path (`Makefile`,
# `LICENSE`): a pathspec can be any name, so only these end the command.
PROSE = frozenset("""
    a about after again against all also an and any are as at be because before
    but by can carefully check compare completely confirm consider could do
    does each either end every examine find finally first for from fully get
    give has have here how if in include including inspect into is it its just
    keep last list look make may mention must name next no nor not note now of
    on once only open or our plus post read record reply report return review
    run say see send shows show since skip so start state stop summarise
    summarize take tell than that the their then there these they this those
    thoroughly through to treat until up use using verify wait was we what when
    where whether which while whole will with without would write yet you your
""".split())
# Top-level names a bare word can mean when the brief runs from outside the
# repository; a word that exists in the working folder counts too.
DIRECTORIES = {"src", "crates", "docs", "assets", "lib", "tests", "test", "evals",
               "scripts", "app", "apps", "packages", "bin", "cmd", "internal",
               "pkg", "include", "config", "examples", "tools", "vendor"}


def is_bare_path(token: str) -> bool:
    return token in DIRECTORIES or (bool(re.fullmatch(r"[\w.-]+", token))
                                    and os.path.exists(token))


def invocation(tokens: list[str], in_span: bool,
               seen: bool = False) -> tuple[list[str], bool, bool]:
    """The revisions one `git diff` or `git log` names, whether a path, a
    glob, a magic pathspec, `--`, `--relative`, a bare name or an option
    outside the display set narrows it, and whether it ran to the end of its
    text. In a code span or fence every extra token narrows. Outside one,
    once a revision is named (`seen`, or one here), the command ends only at
    sentence punctuation or a word in PROSE."""
    revisions: list[str] = []
    narrowed = False
    for raw in tokens:
        token = raw if in_span else raw.rstrip(TRAILING)
        if token == "\\":
            continue
        if token == "":
            if in_span or not raw:
                continue
            return revisions, narrowed, False
        if token == "--" or token.startswith("--relative"):
            return revisions, True, False
        if token.startswith("-") and OPTION.match(token):
            if not DISPLAY.fullmatch(token):
                narrowed = True
        elif token.startswith("-"):
            if not in_span:
                return revisions, narrowed, False
        elif REVISION.fullmatch(token):
            revisions.append(token)
        elif in_span or is_pathspec(token) or is_bare_path(token):
            narrowed = True
        elif re.fullmatch(r"\(*\d+\)*", token):
            return revisions, narrowed, False
        elif (seen or revisions) and token.lower().lstrip("(\"'") not in PROSE:
            narrowed = True
        else:
            return revisions, narrowed, False
        if not in_span and raw != token:
            return revisions, narrowed, False
    return revisions, narrowed, True


def joined(lines: list[str]) -> list[str]:
    """The lines with a trailing backslash joined onto the next line when the
    join completes a `git diff` or `git log` that the line alone does not
    (`git \\` then `diff A...B -- path`)."""
    out: list[str] = []
    index = 0
    while index < len(lines):
        line = lines[index]
        index += 1
        while line.rstrip().endswith("\\") and index < len(lines):
            merged = line.rstrip()[:-1] + " " + lines[index]
            if len(list(COMMAND.finditer(merged))) <= len(list(COMMAND.finditer(line))):
                break
            line = merged
            index += 1
        out.append(line)
    return out


def commands(brief: str) -> list[tuple[str, list[str], bool]]:
    """Every `git diff` or `git log` the brief names, as its verb, its
    revisions and whether it is narrowed. A line in a fenced block is read
    like a code span. Text after a closing backtick, a backslash
    continuation, and a `--` pathspec on a later line (past blank lines)
    belong to the command; so does a path, glob or magic pathspec at the
    start of the next line."""
    found = []
    lines = joined(brief.splitlines())
    fenced = False
    for number, line in enumerate(lines):
        if line.lstrip().startswith("```") and line.count("```") == 1:
            fenced = not fenced
            continue
        spans = [] if fenced else [(m.start(), m.end())
                                   for m in re.finditer(r"`[^`]*`", line)]
        for command in COMMAND.finditer(line):
            close = next((end - 1 for start, end in spans
                          if start < command.start() < end), None)
            in_span = fenced or close is not None
            tokens = line[command.end():close].split()
            revisions, narrowed, ran_out = invocation(tokens, in_span)
            if close is not None:
                rest = line[close + 1:]
                tokens = []
                if rest.strip() and rest[:1].isspace():
                    tokens = rest.split()
                    more, cut, ran_out = invocation(tokens, False, bool(revisions))
                    revisions += more
                    narrowed = narrowed or cut
                elif rest.strip():
                    ran_out = False
            cursor, skipped = number, False
            while ran_out and not narrowed:
                cursor += 1
                if cursor >= len(lines):
                    break
                following = lines[cursor].split()
                if not following or following == ["\\"]:
                    skipped = True
                    tokens = following or tokens
                    continue
                if tokens and tokens[-1].endswith("\\"):
                    more, narrowed, ran_out = invocation(following, fenced,
                                                         bool(revisions))
                    revisions += more
                    tokens, skipped = following, False
                    continue
                first = following[0]
                narrowed = (first == "--" or first.startswith("--relative")
                            or (not skipped and starts_pathspec(first)))
                break
            found.append((command.group("verb"), revisions, narrowed))
    return found


def the_pair(revisions: list[str], base: str, head: str) -> bool:
    if len(revisions) == 1:
        dotted = DOTTED.fullmatch(revisions[0])
        ends = (dotted.group(1), dotted.group(3)) if dotted else None
    elif len(revisions) == 2 and all(re.fullmatch(SHA, r) for r in revisions):
        ends = (revisions[0], revisions[1])
    else:
        ends = None
    return ends is not None and same(ends[0], base) and same(ends[1], head)


def narrower(shown: str, unit: str) -> Stop:
    return Stop(6, f"review brief asks for {shown}, narrower than the unit; "
                   "findings from earlier rounds are checks within the "
                   "whole-unit pass, never its scope "
                   "(cf-model-orchestrator/resources/quality/findings.md); "
                   f"re-brief for {unit}; nothing was sent")


def check_review(brief: str, unit: str, path: str) -> None:
    base, head = PAIR.match(unit).groups()
    named = any(same(m.group(1), base) and same(m.group(3), head)
                for m in DOTTED.finditer(brief) if m.group(2) == "...")
    if not named:
        raise Stop(6, f"review brief {path} does not name its full range "
                      f"{unit}; a review is one holistic pass over the whole "
                      f"unit at one head ({DISCIPLINE}); nothing was sent")
    for dotted in DOTTED.finditer(brief):
        if not (same(dotted.group(1), base) and same(dotted.group(3), head)):
            raise narrower(f"{dotted.group(1)}..{dotted.group(3)}", unit)
    for verb, revisions, narrowed in commands(brief):
        if narrowed:
            raise narrower(f"a git {verb} restricted by a path or an option "
                           "(a word after the revisions that is not plain "
                           "prose counts as a path)", unit)
        if revisions and not the_pair(revisions, base, head):
            shown = ("..".join(revisions) if len(revisions) == 2
                     else " ".join(revisions))
            raise narrower(f"{shown} in a git {verb}", unit)


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
