#!/usr/bin/env python3
"""Launch and observe one native interactive qualification trial.

Repository tooling only. No headless model execution or credential copying.
Use --help and README.md. A clean snapshot is bounded evidence, not confinement.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import secrets
import shlex
import shutil
import stat
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[2]
KIT_PATH = ROOT / "assets/base/agents/skills/cf-evaluate-model/scripts/eval_kit.py"
spec = importlib.util.spec_from_file_location("qualification_eval_kit", KIT_PATH)
kit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(kit)


class Refused(RuntimeError):
    def __init__(self, message: str, flag: str | None = None):
        super().__init__(message)
        self.flag = flag


def herdr(*args: str, text: bool = False):
    done = subprocess.run(["herdr", *args], capture_output=True, text=True, timeout=60)
    if done.returncode:
        raise Refused(f"herdr {' '.join(args[:2])}: {done.stderr or done.stdout}")
    return done.stdout if text or args[:2] in {("pane", "send-keys"), ("pane", "send-text")} else json.loads(done.stdout)


def write(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def snapshot(directories: list[str], *, shallow: list[str] | None = None,
             max_entries: int = 100_000,
             max_seconds: float = 10) -> dict:
    """Metadata only, with a streaming walk and cooperative time/entry caps.

    /private/tmp is always direct entries only. Other roots are recursive.
    No symlink targets or file contents are read. A blocking OS metadata call
    cannot be interrupted by this cooperative deadline.
    """
    started_at = time.monotonic()
    shallow_roots = {"/private/tmp", *(shallow or [])}
    entries, errors, flags = {}, {}, []
    roots = set(directories)

    def budget() -> bool:
        if time.monotonic() - started_at >= max_seconds:
            flags.append("directory_observation_time_cap")
            return False
        if len(entries) >= max_entries:
            flags.append("directory_observation_entry_cap")
            return False
        return True

    def visit(path: Path, depth: int | None, top_level: bool) -> bool:
        if not budget():
            return False
        key = str(path)
        try:
            info = path.lstat()
            kind = stat.S_IFMT(info.st_mode)
            if key in roots:
                # Root metadata is a presence/type marker; entries are observed below.
                entries[key] = [kind]
            elif top_level:
                entries[key] = [kind]
                # The inode tells a replaced entry apart even when the
                # filesystem clock is too coarse to move the mtime, as Linux
                # timestamps are for a swap within one tick; a file's size
                # shows a rewrite that changes its length. A directory's own
                # mtime is left out: writes inside it are not observed here.
                if stat.S_ISREG(info.st_mode):
                    entries[key].extend([info.st_mtime_ns, info.st_ino, info.st_size])
                elif stat.S_ISLNK(info.st_mode):
                    entries[key].extend([info.st_mtime_ns, info.st_ino])
                elif stat.S_ISDIR(info.st_mode):
                    entries[key].append(info.st_ino)
            else:
                entries[key] = [info.st_mode, info.st_size, info.st_mtime_ns, info.st_ctime_ns]
        except OSError as exc:
            entries[key] = None
            errors[key] = str(exc)
            return True
        if stat.S_ISDIR(info.st_mode) and depth != 0:
            try:
                with os.scandir(path) as children:
                    for child in children:
                        if not visit(Path(child.path), None if depth is None else depth - 1, top_level):
                            return False
            except OSError as exc:
                errors[key] = str(exc)
        return True

    for value in directories:
        if not visit(Path(value), 1 if value in shallow_roots else None, value in shallow_roots):
            break
    elapsed = time.monotonic() - started_at
    if elapsed >= max_seconds:
        flags.append("directory_observation_time_cap")
    return {"entries": entries, "errors": errors, "roots": sorted(roots),
            "validity_flags": sorted(set(flags)), "entry_count": len(entries),
            "elapsed_seconds": elapsed}


def compare(before: dict, after: dict) -> dict:
    old, new = before["entries"], after["entries"]
    added = sorted(set(new) - set(old))
    changed = sorted(p for p in new.keys() & old.keys() if old[p] != new[p])
    removed = sorted(set(old) - set(new))
    old_errors, new_errors = before["errors"], after["errors"]
    roots = set(before.get("roots", [])) | set(after.get("roots", []))
    stable = {p for p in old_errors.keys() & new_errors.keys()
              if p not in roots and p in old and p in new and old[p] == new[p]
              and old[p] is not None and old_errors[p] == new_errors[p]}
    incomplete = (set(old_errors) | set(new_errors)) - stable
    flags = set(before.get("validity_flags", [])) | set(after.get("validity_flags", []))
    if added or changed or removed:
        flags.add("declared_directory_changed")
    if incomplete:
        flags.add("directory_observation_incomplete")
    return {"validity_flags": sorted(flags), "added": added, "changed": changed,
            "removed": removed, "errors": {"before": old_errors, "after": new_errors},
            "limitations": [{"path": p, "reason": "unreadable in both snapshots with unchanged metadata"}
                            for p in sorted(stable)],
            "limitation": "Writes inside pre-existing top-level directories of /private/tmp are unobserved "
                          "(except separately watched roots). Writes outside declared directories, startup writes "
                          "before seat readiness, and transient entries gone before the final snapshot are not "
                          "observed. An in-place rewrite that keeps the size within one filesystem clock tick "
                          "is not observed. Changes are not attributed to the subject."}


# Whole-frame grammar from release-qualification/lib.sh at 97f714b61,
# plus the observed branch/effort row only for the registered fixture branch
# and the auto-mode footer (`--permission-mode auto`, the evaluation runs' mode).
# Empty is accepted only before pasting. It is never proof for a second Enter.
FOOTER = re.compile(r"  (?:⏸ manual mode on|⏵⏵ (?:bypass permissions|auto mode) on \(shift\+tab to cycle\)|paste again to expand)(?: · (?:\? for shortcuts|← for agents))*")
FOLD = re.compile(r"\[Pasted text #\d+ \+\d+ lines\]")
DIRECTIVE = "Carry out the pasted instructions."
PLACEHOLDER = re.compile(r'Try "[^"\n]+"')


def editor(screen: str, branch: str | None = None) -> str | None:
    raw = screen.splitlines()
    lines = [line.rstrip() for line in raw]
    rules = [i for i, line in enumerate(lines) if re.fullmatch(r"\s*─+\s*", line)]
    if len(rules) < 2:
        return None
    top, bottom = rules[-2:]
    body = lines[top + 1:bottom]
    footer = [line for line in lines[bottom + 1:] if line]
    if len(footer) == 2 and (footer[0] == "  codeflow-qualify" or (
            branch is not None and re.fullmatch(r"  " + re.escape(branch) + r"(?: +(?:● (?:low|medium|high|xhigh|max) · /effort|ctrl\+g to edit in Nvim))?", footer[0]))):
        footer = footer[1:]
    if len(footer) != 1 or not FOOTER.fullmatch(footer[0]) or not body:
        return None
    # rstrip removes the blank separator in an empty editor.
    if body[0] == "❯":
        if raw[top + 1] not in {"❯", "❯ ", "❯\u00a0"}:
            return None
        first = ""
    elif body[0].startswith(("❯ ", "❯\u00a0")):
        first = body[0][2:]
        if first.startswith((" ", "\u00a0")):
            return None
    else:
        return None
    rest = []
    for line in body[1:]:
        if re.match(r"^[\s│]*[❯>]", line) or (line and not line.startswith("  ")):
            return None
        rest.append(line[2:] if line else "")
    return "\n".join([first, *rest]).strip()


def visible(pane: str, branch: str | None = None, evidence: Path | None = None) -> str | None:
    screen = herdr("pane", "read", pane, "--source", "visible", text=True)
    if evidence is not None:
        evidence.write_text(screen, encoding="utf-8")
    return editor(screen, branch)


def state(pane: str) -> dict:
    return herdr("agent", "get", pane)["result"]["agent"]


def recover_registration(pane: str, name: str, harness: str, repository: Path,
                         start_args: tuple[str, ...]) -> dict:
    try:
        registered = state(pane)
    except Refused:
        return herdr(*start_args)
    expected = {"name": name, "pane_id": pane, "agent": harness, "cwd": str(repository)}
    if any(registered.get(key) != value for key, value in expected.items()):
        raise Refused("registered seat identity differs after workspace trust")
    return {"registered_agent": registered}


# Grok Build releases whose trust and welcome screens were captured from a
# live launch. Any other version fails closed until it is captured too.
GROK_VERSIONS = ("1.0.44", "1.0.46")
GROK_FOOTER = re.compile(r"(?m)^\s*[│┃]?\s*Grok Build\s+(\d+(?:\.\d+)+\S*)\s+\[[^\]\n]+\]\s*[│┃]?\s*$")


def grok_version(screen: str) -> str | None:
    """The version in Grok Build's footer, when the screen shows exactly one."""
    found = set(GROK_FOOTER.findall(screen))
    return found.pop() if len(found) == 1 else None


def trust_choice(screen: str, harness: str, repository: Path) -> str | None:
    lines = [line.strip() for line in screen.splitlines()]
    if harness == "codex" and "Hooks need review" in lines:
        raise Refused(f"hooks for the Codex trial fixture {repository} need one "
                      "human-authorized acceptance in the dedicated evaluator home "
                      "~/.codeflow-eval/codex; no key was sent")
    if any(re.search(r"(?i)^(?:[❯›>]\s*)?(?:allow external .*imports|external imports:|import .*settings|.*hooks.*(?:review|trust))", line) for line in lines):
        raise Refused("trust or import prompt is not authorized")
    if harness == "grok" and "Do you trust the contents of this directory?" in lines:
        body = [line for line in lines[lines.index("Do you trust the contents of this directory?"):] if line]
        body = [line if i == 1 else " ".join(line.split()) for i, line in enumerate(body)]
        expected = ["Do you trust the contents of this directory?", str(repository),
                    "Grok Build may run or modify contents in this directory,", "posing security risks.",
                    "Yes, proceed y", "No, quit n"]
        version = grok_version(screen)
        if version not in GROK_VERSIONS:
            raise Refused(f"Grok Build {version or '(version not shown)'} trust dialog: not a captured "
                          f"version ({', '.join(GROK_VERSIONS)}); no key sent")
        if (body[:6] != expected or len(body) != 7
                or body[-1] != f"Grok Build {version} [stable]"):
            raise Refused(f"Grok Build {version} trust dialog: unrecognized wording or a different subject path; no key sent")
        return "grok-yes"
    header = {"claude": "Accessing workspace:", "codex": "Folder access"}.get(harness)
    suspicious = re.search(r"(?i)do you trust|trust (?:this|the) (?:folder|directory|project)|one you trust|folder access|accessing workspace:", screen)
    if not header or header not in lines:
        if suspicious and harness == "grok":
            raise Refused(f"Grok Build {grok_version(screen) or '(version not shown)'} trust dialog: "
                          "unrecognized wording; no key sent")
        if suspicious:
            raise Refused("unrecognized trust dialog")
        return None
    if lines.count(header) != 1:
        raise Refused("ambiguous trust dialog")
    index = lines.index(header) + 1
    while index < len(lines) and not lines[index]:
        index += 1
    path_lines = []
    while index < len(lines) and lines[index]:
        path_lines.append(lines[index]); index += 1
    if "".join(path_lines) != str(repository):
        raise Refused("trust dialog does not name the exact materialized subject path")
    if harness == "claude":
        yes, no, cursor = "Yes, I trust this folder", "No, exit", "❯"
        valid = "Is this a project you created or one you trust?" in screen and "Enter to confirm · Esc to cancel" in lines
    else:
        yes, no, cursor = "1. Trust and continue", "2. Quit", "›"
        valid = "Trust this folder? Codex can read, edit, and run files here," in screen and any(re.fullmatch(r"enter continue(?: and create sandbox)? · esc quit", line) for line in lines)
    if valid and f"{cursor} {yes}" in lines and no in lines:
        return "yes"
    if valid and f"{cursor} {no}" in lines and yes in lines:
        return "no"
    raise Refused("unrecognized trust choices; no acceptance sent")


def accept_workspace_trust(pane: str, harness: str, repository: Path, screen: str,
                           before_key=None) -> dict | None:
    """`before_key`, when given, runs before every key and raises to stop."""
    choice = trust_choice(screen, harness, repository)
    if choice is None:
        return None
    # Re-read before each key; never send Enter based on an earlier screen.
    current = herdr("pane", "read", pane, "--source", "visible", text=True)
    if trust_choice(current, harness, repository) != choice:
        raise Refused("trust dialog changed before key delivery")
    if choice == "no":
        if before_key:
            before_key()
        herdr("pane", "send-keys", pane, "Down" if harness == "claude" else "Up")
        time.sleep(0.2)
        current = herdr("pane", "read", pane, "--source", "visible", text=True)
        if trust_choice(current, harness, repository) != "yes":
            raise Refused("trust choice not visibly selected; no Enter sent")
    if before_key:
        before_key()
    herdr("pane", "send-keys", pane, "y" if harness == "grok" else "Enter")
    event = {"harness": harness, "path": str(repository),
             "screen_sha256": "sha256:" + hashlib.sha256(current.encode()).hexdigest(), "time": time.time()}
    if harness == "grok":
        event["version"] = grok_version(current)
    return event


def renderer_choice(screen: str) -> str | None:
    lines = [line.strip() for line in screen.splitlines() if line.strip()]
    title = "Try the new fullscreen renderer?"
    if title not in lines:
        return None
    rules = [i for i, line in enumerate(lines) if re.fullmatch("─+", line)]
    if not rules:
        raise Refused("unrecognized renderer dialog")
    body = lines[rules[-1] + 1:]
    fixed = [title, "· Flicker-free output",
             "· Mouse support — click to move your cursor or expand results",
             "· Selected text auto-copies to your clipboard"]
    for choice, options in [("yes", ["❯ 1. Yes, try it", "2. Not now"]),
                            ("no", ["1. Yes, try it", "❯ 2. Not now"])]:
        if body == [*fixed, *options, "Enter to confirm · Esc to cancel"]:
            return choice
    raise Refused("unrecognized renderer dialog; no display choice sent")


def decline_renderer(pane: str, screen: str, before_key=None) -> dict | None:
    choice = renderer_choice(screen)
    if choice is None:
        return None
    current = herdr("pane", "read", pane, "--source", "visible", text=True)
    if renderer_choice(current) != choice:
        raise Refused("renderer dialog changed before key delivery")
    if choice == "yes":
        if before_key:
            before_key()
        herdr("pane", "send-keys", pane, "Down")
        time.sleep(0.2)
        current = herdr("pane", "read", pane, "--source", "visible", text=True)
        if renderer_choice(current) != "no":
            raise Refused("Not now not visibly selected; no Enter sent")
    if before_key:
        before_key()
    herdr("pane", "send-keys", pane, "Enter")
    return {"harness": "claude", "dialog": "fullscreen-renderer", "choice": "Not now",
            "screen_sha256": "sha256:" + hashlib.sha256(current.encode()).hexdigest(), "time": time.time()}


def handle_startup(pane: str, harness: str, repository: Path, screen: str,
                   acceptances: list, display_choices: list, before_key=None) -> bool:
    if trust_choice(screen, harness, repository) is not None:
        if not acceptances:
            acceptances.append(accept_workspace_trust(pane, harness, repository, screen, before_key))
        return True
    if harness == "claude" and renderer_choice(screen) is not None:
        if not display_choices:
            display_choices.append(decline_renderer(pane, screen, before_key))
        return True
    return False


# Codex CLI idle frame, captured live: the empty composer, one blank line, the
# status line (model and effort, then cwd, then volatile title and branch) and
# the shortcuts footer, whose right side may carry a volatile warning count.
CODEX_EMPTY = "› Ask Codex to do anything"
CODEX_SHORTCUTS = re.compile(r"  \? for shortcuts(?: {2,}⚠ \d+ warnings? · f2 to view)?")
CODEX_EFFORTS = {"minimal", "low", "medium", "high", "xhigh"}
# Above the composer: no second cursor or menu row and no confirm hint.
CODEX_DIALOG = re.compile(r"(?i)^\s*[│┃]?\s*[›❯]|\b(?:enter|esc)\b.*\b(?:confirm|continue|cancel|quit|skip|select)\b")
CODEX_FOLD = re.compile(r"\[Pasted Content (\d+) chars\]")


def codex_expectation(native: list[str], repository: Path) -> dict:
    """The caller's requested model and effort, for Codex frame checks."""
    model = effort = None
    index = 0
    while index < len(native):
        flag, separator, value = native[index].partition("=")
        index += 1
        if not separator and index < len(native):
            value = native[index]; index += 1
        if flag == "--model":
            model = value
        elif flag == "-c":
            effort = value.partition("=")[2].strip("\"'")
    if not model:
        raise Refused("refused native flag: --model (required so Codex readiness can verify the model)", "--model")
    return {"model": model, "effort": effort, "repository": repository}


def codex_status(line: str, model: str, effort: str | None, repository: Path) -> bool:
    parts = line[2:].split(" · ") if line.startswith("  ") else []
    if len(parts) < 2 or parts[1] != str(repository):
        return False
    label = parts[0].split(" ")
    if len(label) > 2 or label[0].lower() != model.lower():
        return False
    shown = label[1] if len(label) == 2 else None
    if shown is not None and shown not in CODEX_EFFORTS:
        return False
    return effort is None or shown == effort


def codex_composer(screen: str, model: str, effort: str | None, repository: Path) -> str | None:
    """Composer text of an exact Codex frame: "" when idle, None when unknown.

    The idle frame is pinned whole. With input pending, the footer below the
    status line was not captured, so it is not pinned and may be absent; a
    bottom-pane dialog replaces the composer, which is checked in full.
    """
    lines = [line.rstrip() for line in screen.splitlines()]
    while lines and not lines[-1]:
        lines.pop()
    if len(lines) >= 3 and codex_status(lines[-1], model, effort, repository):
        status, footer = len(lines) - 1, None
    elif len(lines) >= 4 and codex_status(lines[-2], model, effort, repository):
        status, footer = len(lines) - 2, lines[-1]
    else:
        return None
    if lines[status - 1]:
        return None
    top = max((i for i, line in enumerate(lines[:status - 1]) if line == "›" or line.startswith("› ")), default=None)
    if top is None or any(CODEX_DIALOG.search(line) for line in lines[:top]):
        return None
    body = lines[top:status - 1]
    if body == [CODEX_EMPTY]:
        return "" if footer is not None and CODEX_SHORTCUTS.fullmatch(footer) else None
    if any(line and not line.startswith("  ") for line in body[1:]):
        return None
    text = "\n".join([body[0][2:], *(line[2:] for line in body[1:])]).strip()
    return text or None


def codex_holds(text: str | None, prompt: str) -> bool:
    if not text:
        return False
    fold = CODEX_FOLD.fullmatch(text)
    if fold:
        return int(fold.group(1)) in {len(prompt), len(prompt.rstrip("\n"))}
    return "".join(text.split()) == "".join(prompt.split())


def frame_event(harness: str, stage: str, screen: str) -> dict:
    return {"harness": harness, "stage": stage,
            "screen_sha256": "sha256:" + hashlib.sha256(screen.encode()).hexdigest(), "time": time.time()}


# Herdr can keep reporting a seat "blocked" for a moment after its trust
# dialog was accepted, while the screen already shows the ready input box
# (seen with Codex 0.160.0 on 2026-10-04). Readiness waits this long for the
# status to catch up; any other screen is never treated as ready.
STALE_BLOCKED_GRACE = 10.0


def wait_ready(pane: str, seconds: float, harness: str | None = None,
               repository: Path | None = None, acceptances: list | None = None,
               display_choices: list | None = None, branch: str | None = None,
               codex: dict | None = None, frames: list | None = None,
               waits: list | None = None) -> dict:
    end = time.monotonic() + seconds
    acceptances = acceptances if acceptances is not None else []
    display_choices = display_choices if display_choices is not None else []
    waits = waits if waits is not None else []
    blocked_since = None
    while True:
        trust_pending = False
        if harness is not None:
            screen = herdr("pane", "read", pane, "--source", "visible", text=True)
            trust_pending = handle_startup(pane, harness, repository, screen, acceptances, display_choices)
            if harness == "claude":
                body = editor(screen, branch)
                if body is None or (body != "" and not PLACEHOLDER.fullmatch(body)):
                    trust_pending = True
            if harness == "grok" and not grok_authenticated_editor(screen):
                trust_pending = True
            if harness == "codex" and (codex is None or codex_composer(screen, **codex) != ""):
                trust_pending = True
            if not screen.strip():
                trust_pending = True
        current = state(pane)
        now = time.monotonic()
        if not trust_pending and current.get("agent_status") in {"idle", "done"}:
            if blocked_since is not None:
                waits.append({"status": "blocked", "after": "trust_acceptance", "cleared": True,
                              "seconds": round(now - blocked_since, 2)})
            if harness == "codex" and frames is not None:
                frames.append(frame_event("codex", "ready", screen))
            return current
        stale = (not trust_pending and current.get("agent_status") == "blocked" and acceptances
                 and harness is not None)
        if stale and blocked_since is None:
            blocked_since = now
        if not stale:
            blocked_since = None
        if (not trust_pending and current.get("agent_status") == "blocked"
                and (not stale or now - blocked_since >= STALE_BLOCKED_GRACE)) or now >= end:
            if blocked_since is not None:
                waits.append({"status": "blocked", "after": "trust_acceptance", "cleared": False,
                              "seconds": round(now - blocked_since, 2)})
            if harness == "grok":
                raise Refused(grok_startup_refusal(screen))
            raise Refused("seat did not become ready; inspect its native UI before any delivery")
        time.sleep(0.5)


def started(pane: str, initial: dict, seconds: float) -> bool:
    end = time.monotonic() + seconds
    while True:
        current = state(pane)
        if current.get("agent_status") in {"working", "blocked"} or (
            current.get("agent_status") == "done"
            and current.get("state_change_seq", 0) > initial.get("state_change_seq", 0)
        ):
            return True
        if time.monotonic() >= end:
            return False
        time.sleep(0.5)


def holds(text: str | None, prompt: str) -> bool:
    return text is not None and bool(text) and (
        "".join(text.split()) == "".join(prompt.split())
        or bool(FOLD.fullmatch(text)) or bool(re.fullmatch(FOLD.pattern + re.escape(DIRECTIVE), text))
    )


def deliver_claude(pane: str, prompt: str, initial: dict, seconds: float,
                   branch: str | None = None, evidence: Path | None = None) -> int:
    if not prompt.strip() or len(prompt.encode("utf-8")) > 256 * 1024:
        raise Refused("prompt must be nonempty and at most 256 KiB; nothing sent")
    def capture(name: str) -> str | None:
        return visible(pane, branch, evidence / name if evidence is not None else None)

    initial_text = capture("editor-before.txt")
    if initial_text is None or (initial_text != "" and not PLACEHOLDER.fullmatch(initial_text)):
        raise Refused("no verified empty or placeholder Claude editor; nothing sent")
    herdr("pane", "send-text", pane, prompt)
    time.sleep(2)
    pending = capture("editor-pasted.txt")
    if pending is not None and FOLD.fullmatch(pending):
        herdr("pane", "send-text", pane, DIRECTIVE)
        time.sleep(0.3)
        pending = capture("editor-folded.txt")
        if pending is None or not re.fullmatch(FOLD.pattern + re.escape(DIRECTIVE), pending):
            raise Refused("fold directive not visible in the editor; no Enter sent")
    if not holds(pending, prompt):
        raise Refused("the visible editor does not hold this prompt; no Enter sent")
    herdr("pane", "send-keys", pane, "Enter")
    if started(pane, initial, seconds):
        return 1
    # Never press on history, an empty editor, a dialog or an unreadable pane.
    if not holds(capture("editor-second-enter.txt"), prompt):
        raise Refused("turn not confirmed and no verified pending prompt; no second Enter")
    herdr("pane", "send-keys", pane, "Enter")
    if not started(pane, initial, seconds):
        raise Refused("turn not confirmed after two verified Enters; inspect without resending")
    return 2


def deliver_codex(pane: str, prompt: str, initial: dict, seconds: float, codex: dict,
                  frames: list, evidence: Path | None = None) -> int:
    """Paste only into the verified empty composer; one Enter, never a second."""
    if not prompt.strip() or len(prompt.encode("utf-8")) > 256 * 1024:
        raise Refused("prompt must be nonempty and at most 256 KiB; nothing sent")
    def capture(name: str) -> str:
        screen = herdr("pane", "read", pane, "--source", "visible", text=True)
        if evidence is not None:
            (evidence / name).write_text(screen, encoding="utf-8")
        return screen

    before = capture("editor-before.txt")
    if codex_composer(before, **codex) != "":
        raise Refused("no verified empty Codex composer; nothing sent")
    frames.append(frame_event("codex", "before-paste", before))
    herdr("pane", "send-text", pane, prompt)
    time.sleep(2)
    pending = capture("editor-pasted.txt")
    if not codex_holds(codex_composer(pending, **codex), prompt):
        raise Refused("the visible Codex composer does not hold this prompt; no Enter sent")
    frames.append(frame_event("codex", "pending", pending))
    herdr("pane", "send-keys", pane, "Enter")
    if not started(pane, initial, seconds):
        raise Refused("turn not confirmed after one verified Enter; inspect without resending")
    return 1


AUTH_REFUSAL = "evaluator home not signed in: run prepare-eval-homes"


def check_evaluator_auth(harness: str, environment: dict[str, str], cwd: Path) -> dict:
    folder = kit.evaluator_homes()[harness]
    configured = environment.get(kit.EVALUATOR_HOME_VARIABLES[harness])
    matches = (bool(configured) and Path(configured).resolve() == folder.resolve()) if harness == "codex" else configured == str(folder)
    if not matches or not kit.evaluator_directory(folder):
        raise Refused(AUTH_REFUSAL)
    if harness == "grok":
        # No status command in Grok Build. Its authenticated welcome is checked
        # after native startup, before snapshot or delivery. No keys are sent.
        return {"method": "native-welcome", "signed_in": False}
    argv = (["claude", "auth", "status"] if harness == "claude" else
            ["codex", "-c", 'cli_auth_credentials_store="file"', "login", "status"])
    try:
        done = subprocess.run(argv, env=environment, cwd=cwd, capture_output=True, text=True, timeout=30)
        if harness == "claude":
            status = json.loads(done.stdout)
            confirmed = status.get("loggedIn") is True and status.get("configDirectory") == str(folder)
        else:
            confirmed = (done.stdout + done.stderr).strip() == "Logged in using ChatGPT"
        if done.returncode or not confirmed:
            raise Refused(AUTH_REFUSAL)
    except (OSError, subprocess.SubprocessError, ValueError, AttributeError) as exc:
        raise Refused(AUTH_REFUSAL) from exc
    # Do not retain account details or raw status output.
    return {"method": "native-status", "command": argv, "signed_in": True}


GROK_SIGN_IN = re.compile(r"(?im)^\s*[│┃]?[ \t\u2800-\u28ff]*(?:Login with .+|Approve in your browser to finish signing in\.|A browser window will open for authentication\.|Switch account(?:\s+.*)?)[ \t]*[│┃]?\s*$")


def grok_startup_refusal(screen: str) -> str:
    """Why a Grok seat is not ready, naming its version and the screen shown.
    Only a visible sign-in screen is reported as a missing sign-in."""
    if GROK_SIGN_IN.search(screen):
        return AUTH_REFUSAL
    # For naming only: the welcome shows its version without the channel.
    shown = set(re.findall(r"Grok Build\s+(\d+(?:\.\d+)+)", screen))
    version = grok_version(screen) or (shown.pop() if len(shown) == 1 else "(version not shown)")
    if "Do you trust the contents of this directory?" in screen:
        kind = "trust dialog"
    elif re.search(r"\b(?:New worktree|Resume session)\b", screen):
        kind = "welcome screen"
    elif not screen.strip():
        kind = "blank screen"
    else:
        kind = "unknown screen"
    return (f"Grok Build {version}: {kind} not recognized (captured versions: "
            f"{', '.join(GROK_VERSIONS)}); no key or prompt sent")


def grok_authenticated_editor(screen: str) -> bool:
    # Public Grok welcome renderer: these menu rows require AuthState::Done
    # with access. Pending login can paint a prompt too, so prompt alone fails.
    if GROK_SIGN_IN.search(screen):
        return False
    lines = [line.strip() for line in screen.splitlines()]
    return (bool(re.search(r"(?m)^\s*[│┃]?[ \t\u2800-\u28ff]*New worktree[ \t]+ctrl\+w[ \t]*[│┃]?\s*$", screen))
            and bool(re.search(r"(?m)^\s*[│┃]?[ \t\u2800-\u28ff]*Resume session[ \t]+ctrl\+r[ \t]*[│┃]?\s*$", screen))
            and any(re.fullmatch(r"[│┃]?\s*[❯>]\s*(?:Type a message\.\.\.)?\s*[│┃]?", line) for line in lines))


def permission_flags(harness: str, native: list[str], cwd: Path | None = None,
                     *, environment: dict[str, str] | None = None,
                     codex_hook_trust: str = "review", peer: bool = False) -> dict[str, str]:
    """Allowlisted native flags. A peer's start (`peer=True`) uses the same
    allowlist, but its approval flags are the subject's choice, not required."""
    allowed = {"claude": {"--model", "--effort", "--permission-mode"},
               "codex": {"--model", "-c", "--ask-for-approval", "--sandbox"},
               "grok": {"--model", "--reasoning-effort", "--permission-mode", "--always-approve"}}[harness]
    values = {}
    index = 0
    personal = [Path.home() / name for name in (".claude", ".claude.json", ".codex", ".grok")]
    while index < len(native):
        flag, separator, value = native[index].partition("=")
        if flag == CODEX_HOOK_TRUST_FLAG:
            if harness != "codex":
                raise Refused(f"{flag} is only authorized for Codex trials", flag)
            if codex_hook_trust != "bypass":
                raise Refused(f"{flag} requires explicit --codex-hook-trust=bypass", flag)
            dedicated_codex_home(environment or {})
            allowed.add(flag)
        if flag not in allowed or flag in values:
            raise Refused(f"refused native flag: {flag if flag.startswith('-') else '<positional>'}", flag if flag.startswith('-') else "<positional>")
        index += 1
        if flag in {"--always-approve", CODEX_HOOK_TRUST_FLAG}:
            if separator or "--permission-mode" in values:
                raise Refused(f"refused native flag: {flag}", flag)
            value = "true"
        elif not separator:
            if index == len(native) or native[index].startswith("-"):
                raise Refused(f"refused native flag: {flag} (missing value)", flag)
            value = native[index]; index += 1
        if not value or (harness == "grok" and flag == "--permission-mode" and "--always-approve" in values):
            raise Refused(f"refused native flag: {flag}", flag)
        path_value = value.strip("\"'")
        if path_value.startswith("~/"):
            path_value = str(Path.home() / path_value[2:])
        candidate = Path(path_value).expanduser()
        resolved = (candidate if candidate.is_absolute() else (cwd or Path.cwd()) / candidate).resolve()
        if any(resolved == folder.resolve() or folder.resolve() in resolved.parents for folder in personal):
            raise Refused(f"refused native flag: {flag} (personal config path)", flag)
        if flag == "-c" and not re.fullmatch(r'model_reasoning_effort=(?:"(?:minimal|low|medium|high|xhigh)"|minimal|low|medium|high|xhigh)', value):
            raise Refused("refused native flag: -c (only model_reasoning_effort is caller-owned)", flag)
        values[flag] = value
    required = {"codex": ("--ask-for-approval", "--sandbox"),
                "claude": ("--permission-mode",), "grok": ("--permission-mode",)}[harness]
    if harness == "grok" and "--always-approve" in values:
        required = ("--always-approve",)
    if peer:
        return values
    for flag in required:
        if flag not in values:
            raise Refused(f"refused native flag: {flag} (required)", flag)
    if CODEX_HOOK_TRUST_FLAG in values:
        required = (*required, CODEX_HOOK_TRUST_FLAG)
    return {flag: values[flag] for flag in required}


CODEX_HOOK_TRUST_FLAG = "--dangerously-bypass-hook-trust"


def dedicated_codex_home(environment: dict[str, str]) -> Path:
    expected = kit.evaluator_homes()["codex"]
    value = environment.get("CODEX_HOME")
    try:
        candidate = Path(value).expanduser().absolute() if value else None
        personal = Path.home() / ".codex"
        matches = (candidate is not None and candidate != personal and personal not in candidate.parents
                   and candidate.resolve() == expected.resolve()
                   and not expected.is_symlink() and not expected.parent.is_symlink())
    except (OSError, RuntimeError) as exc:
        raise Refused("cannot resolve the dedicated evaluator home", CODEX_HOOK_TRUST_FLAG) from exc
    if not matches:
        raise Refused(f"{CODEX_HOOK_TRUST_FLAG} requires the dedicated evaluator home "
                      "~/.codeflow-eval/codex; other or personal CODEX_HOME paths are refused", CODEX_HOOK_TRUST_FLAG)
    return expected.resolve()


def hook_settings_bytes(path: Path) -> bytes:
    if path.is_symlink():
        raise Refused(f"hook source contains a symlink: {path}", CODEX_HOOK_TRUST_FLAG)
    if not path.is_file():
        raise Refused(f"hook source is not a regular file: {path}", CODEX_HOOK_TRUST_FLAG)
    with path.open("rb") as stream:
        content = stream.read(kit.MAX_SETTINGS_BYTES + 1)
    if len(content) > kit.MAX_SETTINGS_BYTES:
        raise Refused(f"hook source exceeds size cap: {path}", CODEX_HOOK_TRUST_FLAG)
    return content


PLUGIN_MANIFEST_DIRS = (".codex-plugin", ".claude-plugin", ".cursor-plugin")


def manifest_has_key(value, key: str) -> bool:
    if isinstance(value, dict):
        return key in value or any(manifest_has_key(child, key) for child in value.values())
    if isinstance(value, list):
        return any(manifest_has_key(child, key) for child in value)
    return False


def inspect_plugins(root: Path, *, marketplace: str | None = None) -> list[dict]:
    """Inspect all native manifest layouts without loading plugins or following links."""
    for parent in (root, *root.parents):
        if parent.is_symlink():
            raise Refused(f"plugin path contains a symlink: {parent}", CODEX_HOOK_TRUST_FLAG)
    if not root.is_dir():
        raise Refused(f"plugin source is not a directory on disk: {root}", CODEX_HOOK_TRUST_FLAG)
    pending, manifests, entries, cached = [root], {}, {}, []
    count, deadline = 0, time.monotonic() + 10
    while pending:
        path = pending.pop()
        count += 1
        if count > 100_000 or time.monotonic() > deadline:
            raise Refused(f"plugin inspection cap reached: {root}", CODEX_HOOK_TRUST_FLAG)
        info = path.lstat()
        mode = info.st_mode
        if stat.S_ISLNK(mode):
            raise Refused(f"plugin path contains a symlink: {path}", CODEX_HOOK_TRUST_FLAG)
        directory = stat.S_ISDIR(mode)
        if not (directory or stat.S_ISREG(mode)) or not os.access(path, os.R_OK | (os.X_OK if directory else 0)):
            raise Refused(f"plugin path is unreadable or not regular: {path}", CODEX_HOOK_TRUST_FLAG)
        if path.name == "hooks.json" or (directory and path.name == "hooks"):
            raise Refused(f"plugin contains hooks: {path}", CODEX_HOOK_TRUST_FLAG)
        entries[path] = [mode, info.st_size, info.st_mtime_ns]
        if directory:
            parts = path.relative_to(root).parts
            if len(parts) in {3, 4} and parts[0] == "cache":
                cached.append(path)
            if path.name in PLUGIN_MANIFEST_DIRS and not (path / "plugin.json").is_file():
                raise Refused(f"plugin manifest missing: {path}", CODEX_HOOK_TRUST_FLAG)
            pending.extend(sorted(path.iterdir(), reverse=True))
        elif path.name == "plugin.json":
            plugin = path.parent.parent if path.parent.name in PLUGIN_MANIFEST_DIRS else path.parent
            manifests.setdefault(plugin, []).append(path)

    for folder in cached:
        version = len(folder.relative_to(root).parts) == 4
        if not any(plugin == folder or (not version and plugin.parent == folder) for plugin in manifests):
            raise Refused(f"cached plugin has no parseable manifest: {folder}", CODEX_HOOK_TRUST_FLAG)
    plugins = []
    for plugin, paths in sorted(manifests.items()):
        documents, hashes = [], {}
        # Root Agent Plugins metadata takes precedence; inspect every fallback too.
        paths.sort(key=lambda path: (0 if path.parent == plugin else
                                    1 + PLUGIN_MANIFEST_DIRS.index(path.parent.name)))
        for manifest in paths:
            if time.monotonic() > deadline:
                raise Refused(f"plugin inspection time cap reached: {root}", CODEX_HOOK_TRUST_FLAG)
            content = hook_settings_bytes(manifest)
            document = json.loads(content)
            if not isinstance(document, dict) or not isinstance(document.get("name"), str) or not document["name"]:
                raise Refused(f"invalid plugin manifest: {manifest}", CODEX_HOOK_TRUST_FLAG)
            if manifest_has_key(document, "hooks"):
                raise Refused(f"plugin manifest declares hooks: {manifest}", CODEX_HOOK_TRUST_FLAG)
            documents.append(document)
            hashes[manifest.relative_to(plugin).as_posix()] = "sha256:" + hashlib.sha256(content).hexdigest()
        parts = plugin.relative_to(root).parts
        market = marketplace or (parts[1] if len(parts) >= 3 and parts[0] == "cache" else None)
        source = ("remote curated" if market == "openai-curated-remote" else
                  "local" if marketplace or not market or parts[-1] == "local" else "cached marketplace")
        metadata = {path.relative_to(plugin).as_posix(): value for path, value in entries.items()
                    if path.is_relative_to(plugin)}
        plugins.append({"name": documents[0]["name"], "version": documents[0].get("version"),
                        "source": source, "marketplace": market, "path": str(plugin),
                        "skills": any(manifest_has_key(doc, "skills") for doc in documents) or (plugin / "skills").is_dir(),
                        "apps": any(manifest_has_key(doc, "apps") for doc in documents) or (plugin / ".app.json").is_file(),
                        "manifest_sha256": next(iter(hashes.values())), "manifests": hashes,
                        "tree_metadata_sha256": "sha256:" + hashlib.sha256(
                            json.dumps(metadata, sort_keys=True).encode()).hexdigest()})
    return plugins


def reject_extra_hook_sources(root: Path, plugins: list[dict]) -> None:
    config = root / "config.toml"
    declarations = {"plugins": [], "marketplaces": []}
    if config.exists() or config.is_symlink():
        document = tomllib.loads(hook_settings_bytes(config).decode("utf-8"))
        def visit(table: dict, features: bool = False) -> None:
            for key, value in table.items():
                # Feature booleans enable mechanisms, not handler definitions.
                if features and isinstance(value, bool):
                    continue
                if key == "hooks":
                    raise Refused(f"hooks are not allowed in {config}", CODEX_HOOK_TRUST_FLAG)
                if key in declarations:
                    if not isinstance(value, dict):
                        raise Refused(f"cannot inspect {key} configuration in {config}", CODEX_HOOK_TRUST_FLAG)
                    declarations[key].extend(value.items())
                if isinstance(value, dict):
                    visit(value, key == "features")
        visit(document)
    for name in ("plugins", ".plugins"):
        path = root / name
        if path.exists() or path.is_symlink():
            plugins.extend(inspect_plugins(path))
    for name, settings in declarations["marketplaces"]:
        if not isinstance(settings, dict):
            raise Refused(f"cannot inspect marketplace: {name}", CODEX_HOOK_TRUST_FLAG)
        if settings.get("enabled") is False:
            continue
        if settings.get("source_type") == "local":
            if not isinstance(settings.get("source"), str) or not settings["source"]:
                raise Refused(f"cannot inspect local marketplace: {name}", CODEX_HOOK_TRUST_FLAG)
            source = Path(settings["source"]).expanduser()
            if not source.is_absolute():
                source = root / source
            personal = [Path.home() / entry for entry in (".claude", ".claude.json", ".codex", ".grok")]
            if any(source.resolve().is_relative_to(path.resolve()) or path.resolve().is_relative_to(source.resolve())
                   for path in personal):
                raise Refused("personal harness plugin sources are refused", CODEX_HOOK_TRUST_FLAG)
            plugins.extend(inspect_plugins(source, marketplace=name))
        elif not any(plugin["marketplace"] == name for plugin in plugins):
            raise Refused(f"marketplace is not on disk to inspect: {name}", CODEX_HOOK_TRUST_FLAG)
    for name, settings in declarations["plugins"]:
        if not isinstance(settings, dict):
            raise Refused(f"cannot inspect plugin configuration: {name}", CODEX_HOOK_TRUST_FLAG)
        if settings.get("enabled") is False:
            continue
        if not any(name == f"{plugin['name']}@{plugin['marketplace']}" for plugin in plugins):
            raise Refused(f"enabled plugin is not on disk to inspect: {name}", CODEX_HOOK_TRUST_FLAG)
    # One disk manifest can be both cached and explicitly configured.
    plugins[:] = list({plugin["path"]: plugin for plugin in plugins}.values())


# A contract-3 hook wrapper: the hook call, then a fallback that exits 2 on
# any failure. No `$` anywhere: Grok reads `$name` as its own template and
# skips the hook (TSK-215).
CONTRACT_3_WRAPPER = re.compile(r"codeflow hook [a-z][a-z-]* --contract 3 \|\| \{ [^$]* exit 2; \}")


def contract_3_wrapper(command: str) -> bool:
    """True when `command` is a contract-3 wrapper as CodeFlow ships them."""
    return CONTRACT_3_WRAPPER.fullmatch(command) is not None


def codex_hook_preflight(environment: dict[str, str], repository: Path,
                         evidence: dict | None = None) -> dict:
    """Inspect only non-secret settings; never grant trust or change config."""
    result = evidence if evidence is not None else {}
    result.update(flag_used=False, evaluator_home=None, hooks_sha256=None, plugins=[],
                  checks={"evaluator_home": "not_checked", "fixture_hooks": "not_checked"})
    stage = "evaluator_home"
    try:
        home = dedicated_codex_home(environment)
        result["evaluator_home"] = str(home)
        if not kit.evaluator_directory(home):
            raise Refused("dedicated evaluator home is missing, unreadable or contains symlinks", CODEX_HOOK_TRUST_FLAG)
        if (home / "hooks.json").exists() or (home / "hooks.json").is_symlink():
            raise Refused("user-level hooks.json is not allowed in the dedicated evaluator home", CODEX_HOOK_TRUST_FLAG)
        reject_extra_hook_sources(home, result["plugins"])
        result["checks"][stage] = "passed"
        stage = "fixture_hooks"
        kit.refuse_symlink_components(repository / ".codex", repository)
        reject_extra_hook_sources(repository / ".codex", result["plugins"])
        content = hook_settings_bytes(repository / ".codex/hooks.json")
        result["hooks_sha256"] = "sha256:" + hashlib.sha256(content).hexdigest()
        expected = json.loads(hook_settings_bytes(ROOT / "assets/base/codex/hooks.json"))
        if json.loads(content) != expected:
            raise Refused("trial hooks must exactly match the shipped contract-3 hook definitions", CODEX_HOOK_TRUST_FLAG)
        for groups in expected["hooks"].values():
            for group in groups:
                for hook in group["hooks"]:
                    if not contract_3_wrapper(hook["command"]):
                        raise Refused("shipped hooks must use contract-3 wrappers", CODEX_HOOK_TRUST_FLAG)
        result["checks"][stage] = "passed"
        return result
    except (Refused, OSError, ValueError, KeyError, TypeError, kit.EvalError) as exc:
        result["checks"][stage] = "refused"
        if isinstance(exc, Refused):
            raise
        raise Refused(f"cannot verify Codex {stage}: {exc}", CODEX_HOOK_TRUST_FLAG) from exc


def codex_remote_plugin_state(home: Path) -> dict:
    """The dedicated Codex home's plugin feature settings and any
    account-managed remote plugin cache left in it."""
    try:
        features = kit.codex_plugin_features(home / "config.toml")
        cache = [path.relative_to(home).as_posix() for path in kit.codex_remote_plugin_cache(home)]
    except (kit.EvalError, OSError, ValueError) as exc:
        raise Refused(f"cannot read the plugin state of the dedicated Codex home: {exc}") from exc
    return {"features": features, "remote_cache": cache}


def require_codex_remote_plugins_off(home: Path) -> dict:
    """No plugin, account-managed ones included, loads in the dedicated Codex home."""
    found = codex_remote_plugin_state(home)
    unset = [name for name, value in found["features"].items() if value is not False]
    if unset:
        raise Refused(f"the dedicated Codex home {home} does not set {' and '.join(f'{name} = false' for name in unset)} "
                      "under [features]: run prepare-eval-homes")
    if found["remote_cache"]:
        raise Refused(f"the dedicated Codex home {home} holds an account-managed remote plugin cache "
                      f"({', '.join(found['remote_cache'])}): run prepare-eval-homes, which moves it out")
    return found


def claude_account_state(home: Path) -> dict:
    """The dedicated Claude home's account-content settings and anything in it
    that would add plugins, skills, commands, agents or MCP servers."""
    try:
        return {"settings": kit.claude_account_settings(home / "settings.json"),
                **kit.claude_account_content(home)}
    except (kit.EvalError, OSError, ValueError) as exc:
        raise Refused(f"cannot read the account-content state of the dedicated Claude home: {exc}") from exc


def require_claude_account_content_off(home: Path) -> dict:
    """No account plugin, skill or connector, and no user extension, loads in
    the dedicated Claude home."""
    found = claude_account_state(home)
    wrong = [name for name, value in kit.CLAUDE_ACCOUNT_SETTINGS.items() if found["settings"].get(name) is not value]
    if wrong:
        raise Refused(f"the dedicated Claude home {home} does not set "
                      + " and ".join(f"{name}: {json.dumps(kit.CLAUDE_ACCOUNT_SETTINGS[name])}" for name in wrong)
                      + " in settings.json: run prepare-eval-homes")
    if found["synced"]:
        raise Refused(f"the dedicated Claude home {home} holds synced account content "
                      f"({', '.join(found['synced'])}): run prepare-eval-homes, which moves it out")
    extra = kit.claude_extra_extensions(found)
    if extra:
        raise Refused(f"the dedicated Claude home {home} holds skills, commands, agents or plugins "
                      f"that would reach the trial ({', '.join(extra)}): remove them, then run prepare-eval-homes")
    return found


# What a plugin, a synced claude.ai skill or an MCP server adds to a Claude
# session, as Claude Code records it in the structured fields of its native
# transcript: plugin and synced skills and agents carry a `<source>:<name>`
# namespace, and every MCP tool is `mcp__<server>__<tool>`. The fixture's
# project skills and agents and the harness's bundled skills carry no
# namespace; they are recorded, not flagged. The only MCP servers a trial may
# load are those its fixture repository declared in `.mcp.json` when the trial
# launched; claude.ai connectors, plugin servers and user-scope servers all
# flag. Only inventory and invocation fields count: a description, an
# instruction or an example that mentions a name never makes it loaded.
CLAUDE_SKILL_LINE = re.compile(r"^- (\S+?)(?:: |:$|$)")
# Attachment fields that list MCP tool names, and those that name MCP servers.
CLAUDE_TOOL_NAME_FIELDS = {"deferred_tools_delta": ("addedNames", "surfacedNames", "readdedNames")}
CLAUDE_TOOL_ENTRY_FIELDS = {"deferred_tools_record": ("entries",), "prompt_snapshot": ("tools", "inlineTools")}
CLAUDE_SERVER_FIELDS = {"mcp_instructions_delta": ("addedNames",),
                        "deferred_tools_delta": ("pendingMcpServers", "failedMcpServers")}


def mcp_server_key(name: str) -> str:
    """Claude Code's tool-name form of an MCP server name."""
    return re.sub(r"[^A-Za-z0-9_-]", "_", name)


def mcp_tool_declared(name: str, keys: set[str], servers: set[str] | frozenset[str] = frozenset()) -> bool:
    """Whether an `mcp__<server>__<tool>` name belongs to a declared server.
    `servers` holds the servers the transcript itself attributes the tool to;
    when there are any, each must be declared and own the name. Without
    attribution the wire name is ambiguous, since server names may hold
    `__`: it counts as declared only when every way of splitting it names a
    declared server. Any other reading flags, failing closed."""
    if not name.startswith("mcp__"):
        return True
    rest = name[len("mcp__"):]
    if servers:
        for server in servers:
            key = mcp_server_key(server)
            if key not in keys or not rest.startswith(key + "__") or len(rest) == len(key) + 2:
                return False
        return True
    splits = [(rest[:at], rest[at + 2:]) for at in range(len(rest)) if rest.startswith("__", at)]
    splits = [server for server, tool in splits if server and tool]
    return bool(splits) and all(server in keys for server in splits)


def declared_mcp_servers(repository: Path) -> dict:
    """The MCP servers the fixture repository declares in `.mcp.json`, read
    once at launch before any session starts. A link or an unreadable file
    declares nothing and is recorded as an error."""
    path = repository / ".mcp.json"
    result = {"servers": [], "error": None}
    if path.is_symlink():
        result["error"] = f"{path} is a link"
        return result
    if not path.exists():
        return result
    try:
        servers = json.loads(path.read_text(encoding="utf-8")).get("mcpServers", {})
    except (OSError, UnicodeDecodeError, ValueError, AttributeError) as exc:
        result["error"] = f"cannot read {path}: {exc}"
        return result
    if not isinstance(servers, dict):
        result["error"] = f"{path} mcpServers is not an object"
        return result
    result["servers"] = sorted(str(name) for name in servers)
    return result


def _names(value) -> list[str]:
    """Names from a list of strings or of objects with a `name`."""
    names = []
    for item in value if isinstance(value, list) else []:
        if isinstance(item, str):
            names.append(item)
        elif isinstance(item, dict) and isinstance(item.get("name"), str):
            names.append(item["name"])
    return names


def _open_folder(name, dir_fd: int | None, errors: list[str], shown: Path) -> int | None:
    """A folder opened without following a link, checked to be the entry
    that was looked at. Missing is None with no error; anything else that
    is not a plain folder is an error."""
    try:
        before = os.lstat(name, dir_fd=dir_fd)
    except FileNotFoundError:
        return None
    except OSError as exc:
        errors.append(f"cannot read {shown}: {exc.strerror}")
        return None
    if stat.S_ISLNK(before.st_mode):
        errors.append(f"transcript path holds a link: {shown}")
        return None
    if not stat.S_ISDIR(before.st_mode):
        errors.append(f"transcript path is not a folder: {shown}")
        return None
    try:
        handle = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | getattr(os, "O_DIRECTORY", 0),
                         dir_fd=dir_fd)
    except OSError as exc:
        errors.append(f"cannot open {shown}: {exc.strerror}")
        return None
    if not os.path.samestat(before, os.fstat(handle)):
        os.close(handle)
        errors.append(f"transcript folder changed while it was read: {shown}")
        return None
    return handle


def claude_transcripts(folder_fd: int, folder: Path, errors: list[str]) -> list[tuple[str, bytes]]:
    """Every regular `.jsonl` file under the trial's project folder, read
    through folder descriptors so no component is ever a followed link, even
    one swapped in during the walk. A link, dangling or not, a special file
    named `.jsonl` (a pipe would block the read), a folder that cannot be
    listed and anything that changed between look and open are errors."""
    found: list[tuple[str, bytes]] = []

    def walk(handle: int, relative: str) -> None:
        try:
            names = sorted(entry.name for entry in os.scandir(handle))
        except OSError as exc:
            errors.append(f"cannot list {folder / relative}: {exc.strerror}")
            return
        for name in names:
            shown = folder / relative / name
            inner = f"{relative}/{name}" if relative else name
            try:
                before = os.lstat(name, dir_fd=handle)
            except OSError as exc:
                errors.append(f"cannot read {shown}: {exc.strerror}")
                continue
            if stat.S_ISLNK(before.st_mode):
                errors.append(f"transcript folder holds a link: {shown}")
            elif stat.S_ISDIR(before.st_mode):
                child = _open_folder(name, handle, errors, shown)
                if child is not None:
                    try:
                        walk(child, inner)
                    finally:
                        os.close(child)
            elif name.endswith(".jsonl"):
                if not stat.S_ISREG(before.st_mode):
                    errors.append(f"transcript is not a regular file: {shown}")
                    continue
                try:
                    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=handle)
                except OSError as exc:
                    errors.append(f"cannot open {shown}: {exc.strerror}")
                    continue
                with os.fdopen(descriptor, "rb") as stream:
                    now = os.fstat(stream.fileno())
                    if not stat.S_ISREG(now.st_mode) or not os.path.samestat(before, now):
                        errors.append(f"transcript changed while it was read: {shown}")
                        continue
                    try:
                        found.append((inner, stream.read()))
                    except OSError as exc:
                        errors.append(f"cannot read {shown}: {exc.strerror}")

    walk(folder_fd, "")
    return found


def claude_loaded_extensions(environment: dict[str, str], declared: list[str] | set[str] = ()) -> dict:
    """Plugins, synced skills and MCP servers that reached any Claude session
    of this trial, subject or peer, read from the native transcripts in the
    trial's project folder. `declared` holds the MCP servers the fixture
    declared at launch. `loaded` lists each kind that flags the trial; an
    unreadable transcript or a link on the way is an error, never a clean read."""
    config = Path(environment["CLAUDE_CONFIG_DIR"])
    folder = config / "projects" / environment["CLAUDE_CODE_PROJECT_DIR_NAME"]
    allowed = {mcp_server_key(name) for name in declared}
    found = {"folder": str(folder), "transcripts": [], "skills": [], "agents": [], "mcp_servers": [],
             "tools": [], "declared_mcp_servers": sorted(declared),
             "loaded": {"skills": [], "agents": [], "tools": [], "mcp_servers": []}, "errors": []}
    errors = found["errors"]
    opened: list[int] = []
    try:
        handle = _open_folder(str(config), None, errors, config)
        for name in ("projects", environment["CLAUDE_CODE_PROJECT_DIR_NAME"]):
            if handle is None:
                break
            opened.append(handle)
            handle = _open_folder(name, handle, errors, config / "projects" if name == "projects" else folder)
        if handle is None:
            return found
        opened.append(handle)
        transcripts = claude_transcripts(handle, folder, errors)
    finally:
        for descriptor in opened:
            os.close(descriptor)
    skills, agents, servers, seen_tools, undeclared_tools = set(), set(), set(), set(), set()
    for relative, data in transcripts:
        found["transcripts"].append(relative)
        # Each transcript is one session: a tool's server attribution in one
        # session never vouches for the same name in another.
        tools: dict[str, set[str]] = {}
        try:
            lines = data.decode("utf-8").splitlines()
        except UnicodeDecodeError as exc:
            errors.append(f"cannot read {folder / relative}: {exc}")
            continue
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if not isinstance(record, dict):
                continue
            attachment = record.get("attachment")
            if isinstance(attachment, dict):
                kind = attachment.get("type")
                if kind == "skill_listing":
                    listed = attachment.get("names")
                    if isinstance(listed, list):
                        skills.update(_names(listed))
                    else:
                        for row in str(attachment.get("content", "")).splitlines():
                            match = CLAUDE_SKILL_LINE.match(row)
                            if match:
                                skills.add(match.group(1))
                elif kind == "invoked_skills":
                    skills.update(_names(attachment.get("skills")))
                elif kind == "agent_listing_delta":
                    agents.update(_names(attachment.get("addedTypes")))
                for field in CLAUDE_TOOL_NAME_FIELDS.get(kind, ()):
                    for name in _names(attachment.get(field)):
                        if name.startswith("mcp__"):
                            tools.setdefault(name, set())
                for field in CLAUDE_TOOL_ENTRY_FIELDS.get(kind, ()):
                    entries = attachment.get(field)
                    for entry in entries if isinstance(entries, list) else []:
                        name = entry if isinstance(entry, str) else entry.get("name") if isinstance(entry, dict) else None
                        if not isinstance(name, str) or not name.startswith("mcp__"):
                            continue
                        attributed = tools.setdefault(name, set())
                        server = entry.get("server") if isinstance(entry, dict) else None
                        if isinstance(server, str) and server:
                            attributed.add(server)
                            servers.add(server)
                for field in CLAUDE_SERVER_FIELDS.get(kind, ()):
                    servers.update(_names(attachment.get(field)))
            message = record.get("message")
            if record.get("type") == "assistant" and isinstance(message, dict):
                for block in message.get("content") or []:
                    if isinstance(block, dict) and block.get("type") == "tool_use":
                        name = block.get("name")
                        if isinstance(name, str) and name.startswith("mcp__"):
                            tools.setdefault(name, set())
        seen_tools.update(tools)
        undeclared_tools.update(name for name, by in tools.items() if not mcp_tool_declared(name, allowed, by))
    found["skills"], found["agents"] = sorted(skills), sorted(agents)
    found["mcp_servers"], found["tools"] = sorted(servers), sorted(seen_tools)
    found["loaded"] = {
        "skills": sorted(name for name in skills if ":" in name),
        "agents": sorted(name for name in agents if ":" in name),
        "tools": sorted(undeclared_tools),
        "mcp_servers": sorted(name for name in servers if mcp_server_key(name) not in allowed),
    }
    return found


CONFIG_FILES = {
    "claude": ("settings.json", "settings.local.json", "CLAUDE.md", "CLAUDE.local.md", "rules"),
    "codex": ("config.toml", "AGENTS.md", "AGENTS.override.md", "hooks.json", "rules"),
    "grok": ("config.toml", "managed_config.toml", "requirements.toml", "GROK.md", "AGENTS.md", "rules"),
}


def codex_trust_entry(path: Path, repository: Path) -> dict:
    """The trial repository's Codex project entry, and a digest of the rest.
    Accepting Codex's folder-trust dialog writes exactly that entry."""
    try:
        document = tomllib.loads(hook_settings_bytes(path).decode("utf-8")) if path.is_file() else {}
    except (Refused, OSError, ValueError):
        return {"entry": None, "rest_sha256": None}
    projects = document.get("projects")
    entry = projects.pop(str(repository), None) if isinstance(projects, dict) else None
    if projects == {}:
        del document["projects"]  # the entry may be the file's first project
    return {"entry": entry, "rest_sha256": "sha256:" + hashlib.sha256(
        json.dumps(document, sort_keys=True, default=str).encode()).hexdigest()}


def config_snapshot(environment: dict[str, str], *, plugin_repository: Path | None = None,
                    repository: Path | None = None) -> dict:
    result = {}
    for harness, variable in kit.EVALUATOR_HOME_VARIABLES.items():
        root = Path(environment[variable])
        if not kit.evaluator_directory(root):
            raise Refused(f"evaluator config unavailable or contains symlinks: {harness}")
        hashes = {}
        for name in CONFIG_FILES[harness]:
            path = root / name
            candidates = sorted(path.rglob("*")) if path.is_dir() else [path]
            for candidate in candidates:
                # No auth/account stores, session transcripts, or credential files.
                if candidate.name in {"auth.json", ".credentials.json", "mcp_credentials.json"}:
                    continue
                if candidate.is_symlink():
                    raise Refused(f"evaluator config contains symlink: {harness}")
                if candidate.is_file():
                    if candidate.stat().st_size > kit.MAX_SETTINGS_BYTES:
                        raise Refused(f"evaluator config exceeds size cap: {harness}")
                    hashes[str(candidate.relative_to(root))] = "sha256:" + hashlib.sha256(candidate.read_bytes()).hexdigest()
        result[harness] = hashes
        if harness == "codex":
            result["codex_remote_plugins"] = codex_remote_plugin_state(root)
        if harness == "claude":
            result["claude_account_content"] = claude_account_state(root)
    if repository is not None:
        result["codex_trust"] = codex_trust_entry(Path(environment["CODEX_HOME"]) / "config.toml", repository)
    if plugin_repository is not None:
        plugins = []
        reject_extra_hook_sources(Path(environment["CODEX_HOME"]), plugins)
        reject_extra_hook_sources(plugin_repository / ".codex", plugins)
        result["codex"]["plugins"] = plugins
    return result


def config_drift(before: dict, after: dict, peer_trust_accepted: bool = False) -> list[str]:
    """Any change drifts, except the one Codex project entry for the trial
    repository written after a recorded peer folder-trust acceptance."""
    if before == after:
        return []
    if peer_trust_accepted:
        strip = lambda snapshot: {**snapshot, "codex_trust": None,  # noqa: E731
                                  "codex": {k: v for k, v in snapshot.get("codex", {}).items() if k != "config.toml"}}
        old, new = before.get("codex_trust") or {}, after.get("codex_trust") or {}
        if (strip(before) == strip(after) and old.get("rest_sha256") is not None
                and old.get("rest_sha256") == new.get("rest_sha256")
                and new.get("entry") == {"trust_level": "trusted"} and old.get("entry") in (None, new["entry"])):
            return []
    return ["evaluator_config_drift"]


# Every trial can start Codex and Grok, as its seat or as a peer, and both
# write their dedicated home's config during a trial (folder trust). Two
# trials at once would change each other's config and both drift, so a
# launch takes an exclusive lock on each of these homes and `finish` frees it.
LOCKED_HOMES = ("CODEX_HOME", "GROK_HOME")


def evaluator_lock_path(home: Path) -> Path:
    home = Path(os.path.realpath(home))
    return home.parent / "trial-locks" / f"{home.name}.lock.json"


def acquire_evaluator_locks(environment: dict[str, str], output: Path) -> list[dict]:
    """Lock the Codex and Grok evaluator homes for this trial, or refuse
    naming the trial that holds one. Nothing is left locked on refusal."""
    held: list[dict] = []
    try:
        for variable in LOCKED_HOMES:
            home = Path(os.path.realpath(environment[variable]))
            path = evaluator_lock_path(home)
            path.parent.mkdir(mode=0o700, exist_ok=True)
            if path.parent.is_symlink():
                raise Refused(f"evaluator lock folder is a link: {path.parent}")
            record = {"home": str(home), "path": str(path), "output": str(output.resolve()),
                      "pid": os.getpid(), "time": time.time(), "token": secrets.token_hex(16)}
            try:
                descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
            except FileExistsError:
                try:
                    holder = json.loads(path.read_text(encoding="utf-8")).get("output", "unknown")
                except (OSError, ValueError, AttributeError):
                    holder = "unreadable lock record"
                raise Refused(f"another trial holds the evaluator home {home} ({holder}): run one trial at a "
                              f"time; when that trial is over, `runner.py finish --output <its output>` frees "
                              f"it; remove {path} yourself only if no trial is running") from None
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                json.dump(record, handle)
            held.append(record)
    except BaseException:
        release_evaluator_locks(held)
        raise
    return held


def release_evaluator_locks(locks: list[dict]) -> list[str]:
    """Free the locks this trial took; a lock another trial holds is kept."""
    released = []
    for lock in locks or []:
        path = Path(lock["path"])
        try:
            if path.is_symlink():
                continue
            if json.loads(path.read_text(encoding="utf-8")).get("token") == lock.get("token"):
                path.unlink()
                released.append(str(path))
        except (OSError, ValueError, AttributeError):
            continue
    return released


def load_fixture(record_path: Path) -> tuple[dict, Path, dict]:
    record = kit.load_json(record_path)
    if not kit.registration_verifies(record):
        raise Refused("fixture record signature is missing or changed; use its evaluator key")
    repository = Path(record["path"]).resolve()
    if not repository.is_dir() or not repository.is_relative_to(Path(record["subjects_root"]).resolve()):
        raise Refused("fixture must be inside its registered subjects root")
    if kit.tree_digest(repository) != record["fixture_digest"]:
        raise Refused("fixture changed since materialization; use a fresh trial")
    binary = Path(record["subject_codeflow"])
    if kit.executable_digest(binary) != record["codeflow_executable"]["sha256"]:
        raise Refused("subject binary changed since materialization")
    trial = repository.parent
    ancestry = {"run": kit.check_instruction_ancestors(record_path.resolve().parent.parent),
                "subject": kit.check_instruction_ancestors(trial)}
    environment = record["subject_environment"]
    for key, path in {"HOME": trial / "home", "TMPDIR": trial / "tmp",
                      "CODEFLOW_HOME": trial / "home/.codeflow", "XDG_CONFIG_HOME": trial / "home/.config"}.items():
        if environment.get(key) != str(path):
            raise Refused(f"materialize with the current kit: {key} must be {path}")
    return record, repository, ancestry


def print_hook_review(record_path: Path) -> str:
    record, repository, _ancestry = load_fixture(record_path)
    environment = record["subject_environment"]
    config_snapshot(environment)
    assignments = [shlex.quote(f"{key}={value}") for key, value in environment.items() if key != "TERM"]
    # Review only: read-only sandbox and Codex's default approval policy.
    native = ["codex", "--sandbox", "read-only", *kit.trial_native_args("codex", environment)]
    command = "env -i " + " ".join(assignments) + ' TERM="${TERM:-xterm-256color}" ' + shlex.join(native)
    return ("Run this command once in an app terminal. No harness was launched.\n"
            "This fresh disposable fixture carries the trial hooks. Accept its exact-path\n"
            "folder prompt, review the hooks, then trust them yourself. Use /hooks if needed.\n"
            "Do not submit TASK.md. Exit Codex after review; keep the evaluator home.\n\n"
            f"(cd {shlex.quote(str(repository))} && {command})\n")


def watch_directories(environment: dict[str, str], declared: list[str],
                      reserved: list[Path] | None = None) -> list[str]:
    roots = sorted({str(Path(value).resolve()) for value in declared})
    if not roots:
        raise Refused("declare at least one --watch-dir outside harness TMPDIR")
    scratch = Path(environment["TMPDIR"]).resolve()
    for value in roots:
        root = Path(value)
        if root == scratch or root.is_relative_to(scratch) or scratch.is_relative_to(root):
            raise Refused("watch and planted-control roots must not overlap harness TMPDIR")
        for kit_owned in reserved or []:
            kit_owned = kit_owned.resolve()
            if root == kit_owned or root.is_relative_to(kit_owned) or kit_owned.is_relative_to(root):
                raise Refused("watch roots must not overlap the trial's peer launcher folders")
    return roots


# Seats a subject opens (peers). The subject runs as the operator's user, so
# nothing it can write (requests, answers, context.json, ready files) is
# authority, and it can always start a native seat by another route. The
# launchers on the trial PATH only arrange a correct start. The watcher
# verifies each peer process from its live state (executable, argv, cwd and
# environment) and flags anything it cannot verify; such a peer gets no key.
PEER_HARNESSES = ("claude", "codex", "grok")
PEER_SHIM = Path(__file__).resolve().with_name("peer_shim.py")
# The longest interval between two completed watcher polls that still counts
# as covered.
WATCH_GAP_SECONDS = 10.0
PEER_FLAGS = {
    "refused": "peer_launch_refused", "unanswered": "peer_launch_unanswered",
    "unrecorded": "peer_launch_unrecorded", "unverified": "peer_launch_unverified",
    "unobserved": "peer_launch_unobserved", "outside_workspace": "peer_outside_trial_workspace",
    "startup_refused": "peer_startup_refused", "pending": "peer_not_ready",
    "plugin_drift": "peer_plugin_drift", "delivered_early": "peer_delivered_before_ready",
    "delivery_refused": "peer_delivery_refused", "launcher_changed": "peer_launcher_changed",
    "watch_incomplete": "peer_watch_incomplete",
}
LAUNCH_FLAGS = {"unrecorded", "unverified", "startup_refused", "plugin_drift", "pending"}


def peer_folders(repository: Path) -> tuple[Path, Path]:
    trial = repository.parent
    return trial / "peer-bin", trial / "peer-launches"


def digest_file(path: Path) -> str:
    return "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()


def gate_name(pane: str) -> str:
    """File name of a pane's ready record; peer_shim.py uses the same rule."""
    return re.sub(r"[^A-Za-z0-9_.-]", "_", pane) + ".json"


def write_peer_context(peers: dict, **changes) -> None:
    peers["context"].update(changes)
    path = Path(peers["bin"]) / "context.json"
    write(path, peers["context"])
    peers["context_sha256"] = digest_file(path)


def prepare_peers(repository: Path, environment: dict[str, str], workspace: str, option: str) -> dict:
    """Write the trial's peer launchers and put them first on the trial PATH."""
    shims, launches_root = peer_folders(repository)
    for path in (shims, launches_root):
        kit.refuse_symlink_components(path, repository.parent)
    original = environment["PATH"]
    executables = {name: shutil.which(name, path=original) for name in (*PEER_HARNESSES, "herdr")}
    if shims.exists():
        shutil.rmtree(shims)
    shims.mkdir()
    launches = launches_root / secrets.token_hex(8)
    (launches / "ready").mkdir(parents=True)
    environment["PATH"] = f"{shims}{os.pathsep}{original}"
    source = PEER_SHIM.read_text(encoding="utf-8").replace("#!/usr/bin/env python3", f"#!{sys.executable} -B", 1)
    digests = {}
    for name, real in executables.items():
        if real is None:
            continue
        target = shims / name
        target.write_text(source, encoding="utf-8")
        target.chmod(0o555)
        digests[name] = digest_file(target)
    peers = {"bin": str(shims), "launches": str(launches), "executables": executables,
             "shim_sha256": digests, "interpreters": launcher_interpreters(), "context": {
                 "schema_version": 1, "workspace": workspace, "option": option,
                 "herdr": executables["herdr"], "launches": str(launches),
                 "decision_seconds": 20, "ready_seconds": 30,
                 "harnesses": {name: executables[name] for name in PEER_HARNESSES if executables[name]},
                 "environment": dict(environment), "subject": None}}
    write_peer_context(peers)
    return peers


def decode(value: bytes) -> str:
    return value.decode("utf-8", "surrogateescape")


def parse_procargs(data: bytes) -> tuple[str, list[str], dict[str, str]]:
    """Parse macOS KERN_PROCARGS2: argc, exec path, padding, argv, environment."""
    argc = int.from_bytes(data[:4], sys.byteorder)
    executable, _, rest = data[4:].partition(b"\0")
    fields = rest.lstrip(b"\0").split(b"\0")
    environment = {}
    for item in fields[argc:]:
        if not item:
            break
        key, separator, value = item.partition(b"=")
        if separator:
            environment[decode(key)] = decode(value)
    return decode(executable), [decode(item) for item in fields[:argc]], environment


def darwin_procargs(pid: int) -> tuple[str, list[str], dict[str, str]]:
    import ctypes
    import ctypes.util
    libc = ctypes.CDLL(ctypes.util.find_library("c"), use_errno=True)
    maximum, size = ctypes.c_int(0), ctypes.c_size_t(ctypes.sizeof(ctypes.c_int))
    if libc.sysctl((ctypes.c_int * 2)(1, 8), 2, ctypes.byref(maximum), ctypes.byref(size), None, 0):
        raise OSError(ctypes.get_errno(), "cannot read KERN_ARGMAX")
    buffer, size = ctypes.create_string_buffer(maximum.value), ctypes.c_size_t(maximum.value)
    if libc.sysctl((ctypes.c_int * 3)(1, 49, pid), 3, buffer, ctypes.byref(size), None, 0):
        raise OSError(ctypes.get_errno(), f"cannot read the arguments of process {pid}")
    return parse_procargs(buffer.raw[:size.value])


# libproc's PROC_PIDVNODEPATHINFO: struct proc_vnodepathinfo holds the current
# and root directories, each a struct vnode_info (152 bytes) followed by its
# MAXPATHLEN (1024) path. Reading it is one kernel call; `lsof` walks every
# process first and took 9 to 16 seconds for one pid on a loaded host, longer
# than the watcher's 10-second poll gap.
PROC_PIDVNODEPATHINFO = 9
VNODE_INFO_SIZE = 152
MAXPATHLEN = 1024


def darwin_cwd(pid: int) -> str:
    import ctypes
    import ctypes.util
    libproc = ctypes.CDLL(ctypes.util.find_library("proc") or "/usr/lib/libproc.dylib", use_errno=True)
    size = 2 * (VNODE_INFO_SIZE + MAXPATHLEN)
    buffer = ctypes.create_string_buffer(size)
    read = libproc.proc_pidinfo(pid, PROC_PIDVNODEPATHINFO, ctypes.c_uint64(0), buffer, size)
    if read != size:
        error = ctypes.get_errno()
        if error == 3:  # ESRCH
            raise ProcessLookupError(f"process {pid} is gone")
        raise OSError(error, f"cannot read the working directory of process {pid}")
    path = buffer.raw[VNODE_INFO_SIZE:VNODE_INFO_SIZE + MAXPATHLEN].partition(b"\0")[0]
    if not path.startswith(b"/"):
        raise OSError(f"cannot read the working directory of process {pid}")
    return decode(path)


def process_start(pid: int) -> str:
    done = subprocess.run(["ps", "-o", "lstart=", "-p", str(pid)], capture_output=True, text=True, timeout=10)
    if done.stderr.strip():
        raise OSError(f"cannot read the start time of process {pid}: {done.stderr.strip()}")
    if done.returncode or not done.stdout.strip():
        raise ProcessLookupError(f"process {pid} is gone")
    return done.stdout.strip()


def live_process(pid: int) -> dict:
    """A same-user process as the OS reports it, never as files describe it."""
    start = process_start(pid)
    if sys.platform == "darwin":
        executable, argv, environment = darwin_procargs(pid)
        cwd = darwin_cwd(pid)
    elif sys.platform.startswith("linux"):
        proc = Path(f"/proc/{pid}")
        executable, cwd = os.readlink(proc / "exe"), os.readlink(proc / "cwd")
        argv = [decode(item) for item in (proc / "cmdline").read_bytes().split(b"\0")[:-1]]
        environment = dict(decode(item).partition("=")[::2] for item in
                           (proc / "environ").read_bytes().split(b"\0") if b"=" in item)
    else:
        raise OSError("live process inspection is not supported on this platform")
    if cwd is None:
        raise OSError(f"cannot read the working directory of process {pid}")
    if process_start(pid) != start:
        raise ProcessLookupError(f"process {pid} was replaced while it was read")
    return {"pid": pid, "start": start, "executable": executable, "argv": argv,
            "environment": environment, "cwd": cwd}


def peer_argument_tail(harness: str, environment: dict[str, str], option: str) -> list[str]:
    """What every peer start ends with: the kit's isolation arguments, then the
    Codex hook-trust flag only for a Codex peer under the bypass option."""
    tail = kit.trial_native_args(harness, environment)
    return [*tail, CODEX_HOOK_TRUST_FLAG] if harness == "codex" and option == "bypass" else tail


def check_peer_state(run: dict, harness: str, args: list[str], cwd: str, environment: dict[str, str]) -> dict:
    """The checks a peer start must pass, for a request or a live process.
    Raises Refused; returns the recorded flags and hook checks."""
    repository = Path(run["repository"])
    option = run["hook_trust"]["option"]
    if harness not in PEER_HARNESSES:
        raise Refused("not a qualified peer harness")
    if os.path.realpath(cwd) != os.path.realpath(repository):
        raise Refused("peers start in the fixture repository")
    # The login shell may reorder PATH; it does not decide which seat runs.
    for key, value in run["environment"].items():
        if key not in {"TERM", "PATH"} and environment.get(key) != value:
            raise Refused(f"{key} differs from the trial environment", key)
    result = {"permission_flags": permission_flags(harness, args, repository, environment=environment,
                                                   codex_hook_trust=option, peer=True)}
    if harness == "codex":
        require_codex_remote_plugins_off(Path(environment["CODEX_HOME"]))
    if harness == "claude":
        require_claude_account_content_off(Path(environment["CLAUDE_CONFIG_DIR"]))
    if harness == "codex" and option == "bypass":
        checks = codex_hook_preflight(environment, repository)
        result["hook_trust"] = {key: checks[key] for key in ("evaluator_home", "hooks_sha256", "checks")}
        if checks["plugins"] != run["hook_trust"]["plugins"]:
            raise Refused("plugin inventory changed since launch", CODEX_HOOK_TRUST_FLAG)
    return result


def verify_peer_process(run: dict, harness: str, live: dict) -> dict:
    """Verify a running peer from its live state, rerunning hook and plugin checks."""
    result = {"verified": False, "time": time.time(),
              "flag_used": CODEX_HOOK_TRUST_FLAG in live["argv"]}
    try:
        expected = run["peers"]["executables"].get(harness)
        if not expected or os.path.realpath(live["executable"]) != os.path.realpath(expected):
            raise Refused(f"the executable is not the trial's {harness}")
        tail = peer_argument_tail(harness, run["environment"], run["hook_trust"]["option"])
        args = live["argv"][1:]
        head = args[:len(args) - len(tail)]
        if args[len(head):] != tail or CODEX_HOOK_TRUST_FLAG in head:
            raise Refused("the trial's isolation arguments or hook-trust flag are missing or changed")
        result.update(check_peer_state(run, harness, head, live["cwd"], live["environment"]))
        result["verified"] = True
    except Refused as exc:
        result["error"] = str(exc)
    except (OSError, ValueError, KeyError, TypeError, kit.EvalError) as exc:
        result["error"] = f"cannot verify this peer: {exc}"
    return result


def peer_decision(run: dict, request: dict) -> dict:
    """Check a start the subject asked for and answer with the exact argv and
    environment. The answer is a convenience; only live verification counts."""
    harness, args, pane = request.get("harness"), request.get("args"), request.get("pane")
    result = {"id": request.get("id"), "allow": False, "harness": harness, "pane": pane, "time": time.time()}
    try:
        if harness not in PEER_HARNESSES or not isinstance(args, list) or not all(isinstance(a, str) for a in args):
            raise Refused("unrecognized peer launch request")
        if not pane:
            raise Refused("peers start in their own tab of the trial's Herdr workspace")
        if pane == run.get("pane"):
            raise Refused("peers start in their own tab, not in the subject's pane")
        if state(pane).get("workspace_id") != run["workspace"]:
            raise Refused("peers stay in the trial's Herdr workspace")
        environment = request.get("environment") or {}
        result.update(check_peer_state(run, harness, args, str(request.get("cwd")), environment))
        # Started with the runner's environment, whatever the request claimed.
        environment = dict(run["environment"])
        argv = [run["peers"]["executables"][harness], *(a for a in args if a != CODEX_HOOK_TRUST_FLAG),
                *peer_argument_tail(harness, environment, run["hook_trust"]["option"])]
        result.update(allow=True, argv=argv, environment=environment)
    except Refused as exc:
        result.update(error=str(exc), refused_flag=exc.flag)
    except (OSError, ValueError, KeyError, TypeError, kit.EvalError) as exc:
        result.update(error=f"cannot check this peer launch: {exc}")
    return result


def answer_requests(run: dict, record: dict) -> None:
    answered = {entry["request"].get("id") for entry in record["requests"]}
    for path in sorted(Path(run["peers"]["launches"]).glob("*.request.json")):
        try:
            request = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            continue
        if not isinstance(request, dict) or request.get("id") in answered or path.name != f"{request.get('id')}.request.json":
            continue
        decision = peer_decision(run, request)
        reply = {key: decision.get(key) for key in ("id", "allow", "argv", "environment", "error")}
        temporary = path.with_name(f".{request['id']}.decision.tmp")
        write(temporary, reply)
        os.replace(temporary, path.with_name(f"{request['id']}.decision.json"))
        record["requests"].append({"request": request, "decision": decision, "bound": None})
        answered.add(request["id"])


def harness_process(processes: list[dict], executables: dict) -> dict | None:
    """A foreground Claude, Codex or Grok process, by name or by its executable."""
    reals = {os.path.realpath(path) for path in executables.values() if path}
    for process in processes:
        argv0 = process.get("argv0") or (process.get("argv") or [""])[0] or ""
        if (Path(argv0).name in PEER_HARNESSES or process.get("name") in PEER_HARNESSES
                or (os.path.isabs(argv0) and os.path.realpath(argv0) in reals)):
            return process
    return None


def set_gate(run: dict, pane: str, launch: dict | None) -> None:
    """The ready record the herdr launcher waits for before delivering to a peer."""
    path = Path(run["peers"]["launches"]) / "ready" / gate_name(pane)
    if launch is None:
        path.unlink(missing_ok=True)
    else:
        write(path, {"pane": pane, "identity": launch["identity"], "ready": True})


def claim_decision(record: dict, pane: str, live: dict) -> dict | None:
    """Bind one unbound answer for this pane to the process the launcher
    became: it execs in place, so the pid and argv match the request."""
    for entry in record["requests"]:
        decision = entry["decision"]
        if (entry["bound"] is None and decision.get("allow") and decision["pane"] == pane
                and entry["request"].get("pid") == live["pid"] and decision["argv"] == live["argv"]):
            return entry
    return None


def identity_of(run: dict, live: dict) -> list:
    """A launch is one program in one state in one process: pid and start
    time, plus a digest of the executable, argv, working directory and every
    trial environment value except TERM and PATH. An exec keeps the pid and
    start time, so any change to the rest is a new launch, never verified by
    inheritance."""
    state = {key: live["environment"].get(key) for key in run["environment"] if key not in {"TERM", "PATH"}}
    program = json.dumps([live["executable"], live["argv"], os.path.realpath(live["cwd"]), state],
                         sort_keys=True).encode()
    return [live["pid"], live["start"], "sha256:" + hashlib.sha256(program).hexdigest()[:16]]


def launcher_interpreters() -> list[str]:
    """The interpreter the launchers' first line names, and the app binary a
    macOS framework Python re-executes into, as the OS reports them."""
    paths = {os.path.realpath(sys.executable)}
    app = Path(sys.prefix) / "Resources/Python.app/Contents/MacOS/Python"
    if app.is_file():
        paths.add(os.path.realpath(app))
    return sorted(paths)


def trial_launcher(run: dict, live: dict) -> bool:
    """The trial's own launcher, still waiting for its answer before it execs
    in place: the pinned interpreter running `-B <launcher file>`, exactly
    as the launcher's first line starts it. Anything else is a launch."""
    peers, argv = run["peers"], live["argv"]
    if os.path.realpath(live["executable"]) not in peers.get("interpreters", []) or argv[1:2] != ["-B"]:
        return False
    if len(argv) < 3:
        return False
    script = os.path.realpath(os.path.join(live["cwd"], argv[2]))
    return any(script == os.path.realpath(Path(peers["bin"]) / name) for name in peers["shim_sha256"])


def peer_harness(run: dict, process: dict, live: dict) -> str | None:
    for name in PEER_HARNESSES:
        path = run["peers"]["executables"].get(name)
        if path and os.path.realpath(live["executable"]) == os.path.realpath(path):
            return name
    name = Path(process.get("argv0") or process.get("name") or "").name
    return name if name in PEER_HARNESSES else None


def observe_peer(run: dict, record: dict, peer: dict, agent: dict) -> None:
    """One poll of one peer pane. Each process in it is a separate launch,
    identified by pid and start time and verified from its live state.
    Keys go only to the current verified launch, only for the dialogs
    handle_startup accepts, and only after its identity is rechecked."""
    pane = peer["pane"]
    if agent.get("workspace_id") != run["workspace"]:
        peer["outside_workspace"] = True
        return
    executables = run["peers"]["executables"]
    info = herdr("pane", "process-info", "--pane", pane)["result"]["process_info"]
    process = harness_process(info.get("foreground_processes") or [], executables)
    launch = peer["launches"][-1] if peer["launches"] else None
    if process is None:
        if launch is not None and "ended" not in launch:
            launch["ended"] = time.time()
            set_gate(run, pane, None)
        return
    try:
        live = live_process(int(process["pid"]))
    except ProcessLookupError:
        return  # exited between the two reads; the next poll decides
    if trial_launcher(run, live):
        if launch is not None and "ended" not in launch:
            launch["ended"] = time.time()
            set_gate(run, pane, None)
        return  # it gets no key; what it execs is a new launch
    identity = identity_of(run, live)
    if launch is None or launch["identity"] != identity:
        set_gate(run, pane, None)
        if launch is not None and "ended" not in launch:
            launch["ended"] = time.time()  # superseded; its readiness ends here
        if (launch is not None and launch["identity"][:2] == identity[:2]
                and launch["status"] in {"pending", "ready"}):
            launch.update(status="unverified",
                          error="the process's argv, working directory or environment changed")
        entry = claim_decision(record, pane, live)
        harness = entry["decision"]["harness"] if entry else peer_harness(run, process, live)
        launch = {"identity": identity, "harness": harness, "executable": live["executable"],
                  "argv": live["argv"], "cwd": live["cwd"], "first_seen": time.time(), "status": "pending",
                  "request_id": entry["decision"]["id"] if entry else None,
                  "trust_acceptances": [], "display_choices": []}
        peer["launches"].append(launch)
        if entry is None:
            launch["status"] = "unrecorded"
            return
        entry["bound"] = identity
        launch["verification"] = verify_peer_process(run, harness, live)
        if not launch["verification"]["verified"]:
            launch["status"] = "unverified"
            return
    if launch["status"] != "pending":
        return
    # A prompt needs a ready editor: Herdr's working status means delivery
    # only once this launch was seen idle at its editor, never in startup.
    if agent.get("agent_status") == "working" and launch.get("idle_seen"):
        launch["delivered_before_ready"] = True

    def before_key() -> None:
        current = herdr("pane", "process-info", "--pane", pane)["result"]["process_info"]
        now = harness_process(current.get("foreground_processes") or [], executables)
        if now is None or int(now["pid"]) != live["pid"] or identity_of(run, live_process(live["pid"])) != identity:
            raise Refused("the peer process changed before a key; no key sent")

    screen = herdr("pane", "read", pane, "--source", "visible", text=True)
    try:
        if handle_startup(pane, launch["harness"], Path(run["repository"]), screen,
                          launch["trust_acceptances"], launch["display_choices"], before_key):
            return
    except (Refused, ProcessLookupError) as exc:
        launch.update(status="startup_refused", error=str(exc),
                      screen_sha256="sha256:" + hashlib.sha256(screen.encode()).hexdigest())
        return
    if agent.get("agent_status") == "idle":
        launch["idle_seen"] = True
    if agent.get("agent_status") not in {"idle", "working", "done"}:
        return
    # Readiness: verify the live process again, hook and plugin checks included.
    try:
        again = live_process(live["pid"])
    except ProcessLookupError:
        return
    if identity_of(run, again) != identity:
        return  # replaced since this poll began; the next poll sees a new launch
    launch["ready_check"] = verify_peer_process(run, launch["harness"], again)
    if launch["ready_check"]["verified"]:
        launch.update(status="ready", ready_at=time.time())
        set_gate(run, pane, launch)
    else:
        error = launch["ready_check"].get("error", "")
        launch.update(status="plugin_drift" if "plugin" in error else "unverified", error=error)


def watch_once(run: dict, record: dict) -> None:
    answer_requests(run, record)
    answered = {entry["decision"]["pane"] for entry in record["requests"] if entry["decision"].get("allow")}
    trial = Path(os.path.realpath(Path(run["repository"]).parent))
    for agent in herdr("agent", "list")["result"]["agents"]:
        pane = agent.get("pane_id")
        if not pane or pane == run.get("pane"):
            continue
        cwd = agent.get("foreground_cwd") or agent.get("cwd")
        inside = bool(cwd) and Path(os.path.realpath(cwd)).is_relative_to(trial)
        # A pane is a peer once a start was answered there, or once Herdr sees
        # an agent working in the trial's folders; it stays one.
        if pane not in record["peers"] and pane not in answered and not (inside and agent.get("agent")):
            continue
        peer = record["peers"].setdefault(pane, {
            "pane": pane, "tab": agent.get("tab_id"), "workspace": agent.get("workspace_id"),
            "cwd": cwd, "first_seen": time.time(), "launches": []})
        observe_peer(run, record, peer, agent)


def watch(args) -> None:
    """Answer peer launches and verify peers until finish stops it. Any failed
    poll, gap or other ending is recorded and makes the trial invalid."""
    run = kit.load_json(args.output / "launch.json")
    state_path, stop = args.output / "peers.json", args.output / "peers.stop"
    watcher = {"pid": os.getpid(), "started_at": time.time(), "heartbeat": None, "polls": 0,
               "last_poll": None, "max_gap": 0.0, "stopped_at": None, "stop_reason": None}
    record = {"watcher": watcher, "requests": [], "peers": {}, "errors": []}
    deadline = time.monotonic() + args.max_hours * 3600
    reason = "time limit"
    try:
        while True:
            try:
                watch_once(run, record)
                now = time.time()
                if watcher["last_poll"] is not None:
                    watcher["max_gap"] = max(watcher["max_gap"], now - watcher["last_poll"])
                watcher.update(last_poll=now, polls=watcher["polls"] + 1)
            except Exception as exc:  # recorded, and any error invalidates the trial
                record["errors"] = [*record["errors"], {"time": time.time(), "error": repr(exc)}][-50:]
            watcher["heartbeat"] = time.time()
            write(state_path, record)
            if stop.exists():
                reason = "finish"
                break
            if time.monotonic() >= deadline:
                break
            time.sleep(args.interval)
    except BaseException as exc:
        reason = f"error: {exc!r}"
        raise
    finally:
        for gate in (Path(run["peers"]["launches"]) / "ready").glob("*.json"):
            gate.unlink(missing_ok=True)
        watcher.update(stopped_at=time.time(), stop_reason=reason)
        write(state_path, record)


def start_peer_watch(output: Path) -> dict:
    log = (output / "peers.log").open("ab")
    process = subprocess.Popen([sys.executable, "-B", str(Path(__file__).resolve()), "watch", "--output", str(output)],
                               stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                               cwd=str(output), start_new_session=True)
    log.close()
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        try:
            watcher = kit.load_json(output / "peers.json")["watcher"]
            if watcher["pid"] == process.pid and watcher["last_poll"]:
                return {"pid": process.pid, "log": str(output / "peers.log")}
        except (OSError, ValueError, KeyError, kit.EvalError):
            pass
        if process.poll() is not None:
            raise Refused("the peer watcher exited; see peers.log")
        time.sleep(0.2)
    raise Refused("the peer watcher did not start; no prompt delivered")


def stop_peer_watch(output: Path, seconds: float = 20) -> dict | None:
    (output / "peers.stop").write_text("", encoding="utf-8")
    deadline = time.monotonic() + seconds
    while True:
        try:
            record = kit.load_json(output / "peers.json")
        except (OSError, ValueError, kit.EvalError):
            record = None
        if (record and record["watcher"].get("stopped_at")) or time.monotonic() >= deadline:
            return record
        time.sleep(0.5)


def watch_gaps(watcher: dict | None, errors: list) -> list[str]:
    """Why the watcher's record does not cover the trial through finish."""
    if not watcher or not watcher.get("stopped_at"):
        return ["the watcher did not stop"]
    reasons = []
    if watcher.get("stop_reason") != "finish":
        reasons.append(f"the watcher stopped: {watcher.get('stop_reason')}")
    if errors:
        reasons.append(f"{len(errors)} failed polls")
    if not watcher.get("last_poll"):
        reasons.append("no completed poll")
    elif (watcher.get("max_gap", 0) > WATCH_GAP_SECONDS
          or watcher["stopped_at"] - watcher["last_poll"] > WATCH_GAP_SECONDS):
        reasons.append("an interval longer than the coverage limit")
    return reasons


def peer_findings(run: dict, record: dict | None, watched: bool = True) -> tuple[list[str], dict]:
    """Validity flags and a summary for every peer launch the trial saw.
    `watched` is false when launch refused before the watcher started."""
    peers = run["peers"]
    flags, summary = set(), {"requests": [], "peers": [], "launcher_logs": [], "watcher": None,
                             "errors": [], "coverage_gaps": [], "early_deliveries": []}
    for name, expected in peers["shim_sha256"].items():
        path = Path(peers["bin"]) / name
        if not path.is_file() or digest_file(path) != expected:
            flags.add(PEER_FLAGS["launcher_changed"])
    context = Path(peers["bin"]) / "context.json"
    if not context.is_file() or digest_file(context) != peers.get("context_sha256"):
        flags.add(PEER_FLAGS["launcher_changed"])
    launches = Path(peers["launches"])
    for path in sorted(launches.glob("*.json")) if launches.is_dir() else []:
        if not path.name.endswith((".request.json", ".decision.json")):
            try:
                summary["launcher_logs"].append(json.loads(path.read_text(encoding="utf-8")))
            except (OSError, ValueError):
                flags.add(PEER_FLAGS["launcher_changed"])
    kinds = {entry.get("kind") for entry in summary["launcher_logs"]}
    if "herdr-refused" in kinds:
        flags.add(PEER_FLAGS["refused"])
    if "herdr-delivery-refused" in kinds:
        flags.add(PEER_FLAGS["delivery_refused"])
    record = record or {"requests": [], "peers": {}, "errors": []}
    summary.update(watcher=record.get("watcher"), errors=record.get("errors", []))
    if watched:
        summary["coverage_gaps"] = watch_gaps(record.get("watcher"), record.get("errors", []))
        if summary["coverage_gaps"]:
            flags.add(PEER_FLAGS["watch_incomplete"])
    answered = {entry["request"].get("id") for entry in record["requests"]}
    for path in sorted(launches.glob("*.request.json")) if launches.is_dir() else []:
        if path.name.removesuffix(".request.json") not in answered:
            flags.add(PEER_FLAGS["unanswered"])
    for entry in record["requests"]:
        decision = entry["decision"]
        summary["requests"].append({**{key: decision.get(key) for key in
                                       ("id", "harness", "pane", "allow", "argv", "error", "permission_flags", "hook_trust")},
                                    "bound": entry.get("bound")})
        if not decision.get("allow"):
            flags.add(PEER_FLAGS["refused"])
        elif entry.get("bound") is None:
            flags.add(PEER_FLAGS["unobserved"])
    # The trial's herdr logs each delivery it lets through. Each counts against
    # the launch in that pane at that time, which must already be ready and
    # not yet ended or superseded; another launch's readiness never counts.
    for entry in summary["launcher_logs"]:
        pane, when = entry.get("pane"), entry.get("time", 0)
        if entry.get("kind") != "herdr-delivery" or pane == run.get("pane"):
            continue
        started = [launch for launch in record["peers"].get(pane, {}).get("launches", [])
                   if launch["first_seen"] <= when]
        occupant = max(started, key=lambda launch: launch["first_seen"]) if started else None
        if (occupant is None or occupant.get("ready_at") is None or occupant["ready_at"] > when
                or occupant.get("ended", float("inf")) <= when):
            flags.add(PEER_FLAGS["delivered_early"])
            summary["early_deliveries"].append(entry)
    for peer in record["peers"].values():
        summary["peers"].append(peer)
        if peer.get("outside_workspace"):
            flags.add(PEER_FLAGS["outside_workspace"])
        for launch in peer.get("launches", []):
            if launch["status"] in LAUNCH_FLAGS:
                flags.add(PEER_FLAGS[launch["status"]])
            if launch.get("delivered_before_ready"):
                flags.add(PEER_FLAGS["delivered_early"])
    return sorted(flags), summary


def launch(args) -> None:
    record, repository, ancestry = load_fixture(args.record)
    trial = repository.parent
    environment = dict(record["subject_environment"])
    native = args.native[1:] if args.native[:1] == ["--"] else args.native
    watched = watch_directories(environment, args.watch_dir, list(peer_folders(repository)))
    observation = {"shallow": ["/private/tmp"],
                   "max_entries": args.max_entries, "max_seconds": args.snapshot_seconds}
    for directory in watched:
        if not Path(directory).is_dir():
            raise Refused(f"declared directory is unavailable: {directory}")
        if args.output.resolve().is_relative_to(Path(directory)):
            raise Refused("runner evidence must live outside declared directories")
    if args.output.exists():
        raise Refused("output already exists; never reuse a trial launch")
    hook_trust = {"option": args.codex_hook_trust, "flag_used": False, "evaluator_home": None, "hooks_sha256": None, "plugins": [],
                  "checks": {"evaluator_home": "not_checked", "fixture_hooks": "not_checked"}}
    try:
        if args.codex_hook_trust not in {"review", "bypass"}:
            raise Refused("unknown Codex hook-trust option", "--codex-hook-trust")
        permissions = permission_flags(args.harness, native, repository, environment=environment,
                                       codex_hook_trust=args.codex_hook_trust)
        codex = codex_expectation(native, repository) if args.harness == "codex" else None
        # On any harness the option also covers Codex peers the subject opens;
        # only a Codex seat itself gets the native flag at launch.
        if args.codex_hook_trust == "bypass":
            dedicated_codex_home(environment)
        # Every trial can start Codex, as its seat or as a peer, in this home.
        remote_plugins = require_codex_remote_plugins_off(Path(environment["CODEX_HOME"]))
        # Every trial can start Claude too, as its seat or as a peer.
        account_content = require_claude_account_content_off(Path(environment["CLAUDE_CONFIG_DIR"]))
        # One trial at a time per Codex and Grok home; `finish` frees them.
        locks = acquire_evaluator_locks(environment, args.output)
        if args.codex_hook_trust == "bypass":
            codex_hook_preflight(environment, repository, hook_trust)
            environment["CODEX_HOME"] = hook_trust["evaluator_home"]
    except Refused as exc:
        release_evaluator_locks(locals().get("locks") or [])
        args.output.mkdir(parents=True)
        write(args.output / "launch.json", {"harness": args.harness, "status": "refused",
                                           "refused_flag": exc.flag, "error": str(exc),
                                           "hook_trust": hook_trust})
        raise
    authentication = check_evaluator_auth(args.harness, environment, repository)
    # Validate all config roots before a native process could follow a link.
    config_preflight = config_snapshot(environment)
    if args.codex_hook_trust == "bypass":
        config_preflight["codex"]["plugins"] = hook_trust["plugins"]
    native = [*native, *kit.trial_native_args(args.harness, environment)]
    if args.harness == "codex" and args.codex_hook_trust == "bypass":
        if CODEX_HOOK_TRUST_FLAG not in native:
            native.append(CODEX_HOOK_TRUST_FLAG)
        permissions[CODEX_HOOK_TRUST_FLAG] = "true"
        hook_trust["flag_used"] = True
    elif args.harness == "codex":
        print(print_hook_review(args.record), end="")
    # After the native status checks, which must not meet the launchers.
    peers = prepare_peers(repository, environment, args.workspace, args.codex_hook_trust)
    args.output.mkdir(parents=True)
    run = {"schema_version": 1, "fixture_record": str(args.record.resolve()),
           "repository": str(repository), "harness": args.harness, "workspace": args.workspace, "peers": peers,
           "native_args": native, "permission_flags": permissions,
           "hook_trust": hook_trust, "codex_remote_plugins": remote_plugins,
           "claude_account_content": account_content,
           "evaluator_locks": locks,
           "declared_mcp_servers": declared_mcp_servers(repository),
           "config_preflight": config_preflight,
           "environment": environment, "declared_directories": watched,
           "observation": observation,
           "observation_limitations": ["Only explicitly declared watch directories are observed. Harness TMPDIR is unobserved; planted controls must be outside it."],
           "authentication": authentication,
           "instruction_ancestors": ancestry, "trust_acceptances": [], "display_choices": [], "verified_frames": [],
           "status": "prepared", "started_at": time.time()}
    write(args.output / "launch.json", run)
    try:
        argv = ["tab", "create", "--workspace", args.workspace, "--label",
                f"eval-{record['case_id'][:30]}", "--cwd", str(repository), "--no-focus"]
        for key, value in environment.items():
            argv.extend(["--env", f"{key}={value}"])
        created = herdr(*argv)["result"]
        run.update(tab=created["tab"]["tab_id"], pane=created["root_pane"]["pane_id"])
        write_peer_context(peers, subject={"pane": run["pane"], "ready": False})
        write(args.output / "launch.json", run)
        end, idle = time.monotonic() + args.start_timeout, 0
        while idle < 2:
            info = herdr("pane", "process-info", "--pane", run["pane"])["result"]["process_info"]
            processes = info.get("foreground_processes", [])
            idle = idle + 1 if processes and all(p.get("name") in {"sh", "zsh", "bash"} for p in processes) else 0
            if time.monotonic() >= end:
                raise Refused("new pane did not reach an idle shell; no seat started")
            time.sleep(0.5)
        start_args = ("agent", "start", f"eval-{trial.name}", "--kind",
                      args.harness, "--pane", run["pane"], "--", *native)
        try:
            run["launch_response"] = herdr(*start_args)
        except Refused as exc:
            # Herdr may report not-ready with or without a registered native seat.
            # Reuse only the exact seat; otherwise register this same pane.
            screen = herdr("pane", "read", run["pane"], "--source", "visible", text=True)
            if not handle_startup(run["pane"], args.harness, repository, screen,
                                  run["trust_acceptances"], run["display_choices"]):
                raise exc
            run["initial_start_error"] = str(exc)
            write(args.output / "launch.json", run)
            time.sleep(0.2)
            screen = herdr("pane", "read", run["pane"], "--source", "visible", text=True)
            if trust_choice(screen, args.harness, repository) is not None:
                raise Refused("accepted trust dialog has not cleared; no repeated key")
            run["launch_response"] = recover_registration(
                run["pane"], f"eval-{trial.name}", args.harness, repository, start_args)
        initial = wait_ready(run["pane"], args.start_timeout, args.harness, repository, run["trust_acceptances"],
                             run["display_choices"], record.get("branch"), codex=codex, frames=run["verified_frames"],
                             waits=run.setdefault("readiness_waits", []))
        if args.harness == "grok":
            screen = herdr("pane", "read", run["pane"], "--source", "visible", text=True)
            if not grok_authenticated_editor(screen):
                raise Refused(grok_startup_refusal(screen))
            # The welcome shows no version; the fresh fixture's trust dialog does.
            versions = {event.get("version") for event in run["trust_acceptances"] if event}
            if len(versions) != 1 or not versions <= set(GROK_VERSIONS):
                raise Refused("Grok Build version not verified: no trust dialog of a captured version "
                              "was accepted for this fresh fixture; no prompt sent")
            run["authentication"]["signed_in"] = True
            run["authentication"]["grok_version"] = versions.pop()
            run["verified_frames"].append(frame_event("grok", "ready", screen))
            (args.output / "grok-ready.txt").write_text(screen, encoding="utf-8")
        # From here a start in the subject's pane is a peer request too.
        write_peer_context(peers, subject={"pane": run["pane"], "ready": True})
        peers["subject_start_via_launcher"] = any(
            path.name.endswith("-subject.json") for path in Path(peers["launches"]).glob("*.json"))
        bypass = hook_trust["option"] == "bypass"
        try:
            require_codex_remote_plugins_off(Path(environment["CODEX_HOME"]))
        except Refused as exc:
            raise Refused(f"remote plugin recheck after readiness refused: {exc}") from exc
        try:
            require_claude_account_content_off(Path(environment["CLAUDE_CONFIG_DIR"]))
        except Refused as exc:
            raise Refused(f"Claude account-content recheck after readiness refused: {exc}") from exc
        try:
            run["config_start"] = config_snapshot(
                environment, plugin_repository=repository if bypass else None, repository=repository)
        except (Refused, OSError, ValueError) as exc:
            run["config_start"] = {"error": str(exc)}
            raise Refused(f"plugin/config recheck after readiness refused: {exc}") from exc
        if bypass and run["config_start"]["codex"]["plugins"] != hook_trust["plugins"]:
            raise Refused("plugin inventory changed after readiness; no prompt delivered")
        run["initial_agent"] = initial
        write(args.output / "launch.json", run)
        write(args.output / "before.json", snapshot(watched, **observation))
        # The watcher answers peer starts and their dialogs from now to finish.
        run["peer_watch"] = start_peer_watch(args.output)
        write(args.output / "launch.json", run)
        prompt = (repository / "TASK.md").read_text(encoding="utf-8")
        if args.harness == "claude":
            run["enters"] = deliver_claude(run["pane"], prompt, initial, args.start_timeout, record.get("branch"), args.output)
        elif args.harness == "codex":
            run["enters"] = deliver_codex(run["pane"], prompt, initial, args.start_timeout, codex,
                                          run["verified_frames"], args.output)
        else:
            helper = ROOT / "assets/base/agents/skills/cf-herdr/scripts/deliver.py"
            done = subprocess.run([sys.executable, "-B", str(helper), "--pane", run["pane"],
                                   "--file", str(repository / "TASK.md"), "--start-timeout",
                                   str(args.start_timeout)], capture_output=True, text=True, timeout=120)
            run["delivery"] = {"returncode": done.returncode, "stdout": done.stdout, "stderr": done.stderr}
            if done.returncode:
                raise Refused("delivery refused; inspect the recorded result, never resend blindly")
        run["status"] = "started"
    except (Refused, OSError, subprocess.SubprocessError, KeyError, ValueError) as exc:
        run.update(status="refused", error=str(exc))
        if "pane" in run and "initial_agent" not in run:
            # Keep the screen a startup refusal met, for inspection.
            try:
                (args.output / "startup-screen.txt").write_text(
                    herdr("pane", "read", run["pane"], "--source", "visible", text=True), encoding="utf-8")
            except Exception:  # noqa: BLE001 - evidence only; never masks the refusal
                pass
        if "peer_watch" in run:
            (args.output / "peers.stop").write_text("", encoding="utf-8")
        raise
    finally:
        write(args.output / "launch.json", run)


def finish(args) -> None:
    run = kit.load_json(args.output / "launch.json")
    # Stop answering peer dialogs before the final observation.
    peer_record = stop_peer_watch(args.output) if "peer_watch" in run else None
    after = snapshot(run["declared_directories"], **run["observation"])
    before_path = args.output / "before.json"
    if before_path.is_file():
        result = compare(kit.load_json(before_path), after)
    else:
        result = {"validity_flags": ["directory_observation_incomplete", *after["validity_flags"]],
                  "error": "no pre-trial snapshot recorded after seat readiness"}
    result["observation_limitations"] = run.get("observation_limitations", [])
    try:
        run["config_finish"] = config_snapshot(
            run["environment"], plugin_repository=Path(run["repository"])
            if run.get("hook_trust", {}).get("flag_used") or run.get("hook_trust", {}).get("option") == "bypass"
            else None, repository=Path(run["repository"]) if "codex_trust" in run.get("config_start", {}) else None)
        accepted = any(event.get("harness") == "codex" and event.get("path") == run["repository"]
                       for peer in (peer_record or {}).get("peers", {}).values()
                       for launch in peer.get("launches", []) for event in launch.get("trust_acceptances", []))
        if "config_start" not in run:
            result["validity_flags"].append("evaluator_config_unreadable")
        else:
            drift = config_drift(run["config_start"], run["config_finish"], accepted)
            result["validity_flags"].extend(drift)
            if accepted and not drift and run["config_start"] != run["config_finish"]:
                result["config_allowances"] = ["Codex project trust entry for the trial repository, "
                                               "written after a recorded peer folder-trust acceptance"]
    except (Refused, OSError, KeyError, ValueError) as exc:
        run["config_finish"] = {"error": str(exc)}
        result["validity_flags"].append("evaluator_config_unreadable")
    write(args.output / "launch.json", run)
    if run["status"] != "started":
        result["validity_flags"].append("native_launch_not_confirmed")
    if "peers" in run:
        flags, result["peers"] = peer_findings(run, peer_record, watched="peer_watch" in run)
        result["validity_flags"].extend(flags)
    # Any plugin, synced skill or claude.ai connector that reached a Claude
    # session, the subject's or a peer's, invalidates the trial. A launch
    # refused before its environment was recorded started no session.
    environment = run.get("environment") or {}
    if {"CLAUDE_CONFIG_DIR", "CLAUDE_CODE_PROJECT_DIR_NAME"} <= environment.keys():
        # The fixture's declaration as launch read it: an edit during the
        # trial never authorizes a server after the fact.
        declaration = run.get("declared_mcp_servers")
        declared = declaration.get("servers", []) if isinstance(declaration, dict) else []
        extensions = claude_loaded_extensions(environment, declared)
        if isinstance(declaration, dict) and declaration.get("error"):
            extensions["errors"].append(declaration["error"])
        result["claude_extensions"] = extensions
        if any(extensions["loaded"].values()):
            result["validity_flags"].append("extra_extension_loaded")
        if extensions["errors"]:
            result["validity_flags"].append("evaluator_config_unreadable")
    write(args.output / "after.json", after)
    if "pane" in run:
        try:
            result["final_agent"] = state(run["pane"])
        except (Refused, subprocess.SubprocessError, ValueError) as exc:
            result["validity_flags"].append("native_state_unavailable")
            result["native_error"] = str(exc)
    write(args.output / "observation.json", result)
    # The final config snapshot is taken: the next trial may start.
    if run.get("evaluator_locks"):
        run["evaluator_locks_released"] = release_evaluator_locks(run["evaluator_locks"])
        write(args.output / "launch.json", run)
    print(json.dumps(result, indent=2))
    # The primary retains native transcripts and closes the recorded tab.


def main() -> int:
    cli = argparse.ArgumentParser(description=__doc__)
    sub = cli.add_subparsers(dest="command", required=True)
    start = sub.add_parser("launch")
    start.add_argument("--record", type=Path, required=True)
    start.add_argument("--output", type=Path, required=True)
    start.add_argument("--workspace", required=True)
    start.add_argument("--harness", choices=["claude", "codex", "grok"], required=True)
    start.add_argument("--codex-hook-trust", choices=["review", "bypass"], default="review",
                       help="Codex evaluation hook review: review (default), or explicitly bypass after verifying hook sources")
    start.add_argument("--watch-dir", action="append", default=[])
    start.add_argument("--max-entries", type=int, default=100_000)
    start.add_argument("--snapshot-seconds", type=float, default=10)
    start.add_argument("--start-timeout", type=float, default=30)
    start.add_argument("native", nargs=argparse.REMAINDER)
    review = sub.add_parser("print-hook-review", help="print an isolated native Codex hook-review command; never launch")
    review.add_argument("--record", type=Path, required=True)
    end = sub.add_parser("finish")
    end.add_argument("--output", type=Path, required=True)
    peers = sub.add_parser("watch", help="answer peer launches and startup dialogs; launch starts it, finish stops it")
    peers.add_argument("--output", type=Path, required=True)
    peers.add_argument("--interval", type=float, default=0.5)
    peers.add_argument("--max-hours", type=float, default=6)
    args = cli.parse_args()
    try:
        if args.command == "launch":
            if not 0 < args.start_timeout <= 120:
                raise Refused("start timeout must be in (0, 120]")
            if args.max_entries <= 0 or not 0 < args.snapshot_seconds <= 60:
                raise Refused("snapshot limits require positive entries and seconds in (0, 60]")
            launch(args)
        elif args.command == "print-hook-review":
            print(print_hook_review(args.record), end="")
        elif args.command == "watch":
            watch(args)
        else:
            finish(args)
    except (Refused, OSError, subprocess.SubprocessError, KeyError, ValueError, kit.EvalError) as exc:
        print(f"qualification runner: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
