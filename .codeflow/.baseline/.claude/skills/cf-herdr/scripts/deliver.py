#!/usr/bin/env python3
"""Deliver an armed prompt file to a Herdr seat and confirm the turn started.

Usage:
  python3 deliver.py --pane <pane_id> --file <prompt> [--lifecycle]
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

Exit codes: 0 started or sent; 1 herdr error; 2 usage or oversize;
3 seat folder missing; 4 turn not confirmed; 5 seat busy or unknown.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time

LIMIT = 256 * 1024
FOLD = "[Pasted"
FOLD_SENTENCE = "Carry out the pasted instructions."
STARTED = {"working", "blocked"}
BUSY = STARTED | {"unknown"}


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
    parser.add_argument("--start-timeout", type=float, default=20.0)
    parser.add_argument("--settle", type=float, default=2.0)
    args = parser.parse_args()
    try:
        print(f"cf-herdr deliver: {deliver(args)}")
    except Stop as stop:
        print(f"cf-herdr deliver: {stop}", file=sys.stderr)
        return stop.code
    return 0


if __name__ == "__main__":
    sys.exit(main())
