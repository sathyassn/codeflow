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
import shlex
import stat
import subprocess
import sys
import time

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
                if stat.S_ISREG(info.st_mode) or stat.S_ISLNK(info.st_mode):
                    entries[key].append(info.st_mtime_ns)
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
                          "observed. Changes are not attributed to the subject."}


# Whole-frame grammar from release-qualification/lib.sh at 97f714b61,
# plus the observed branch/effort row only for the registered fixture branch.
# Empty is accepted only before pasting. It is never proof for a second Enter.
FOOTER = re.compile(r"  (?:⏸ manual mode on|⏵⏵ bypass permissions on \(shift\+tab to cycle\)|paste again to expand)(?: · (?:\? for shortcuts|← for agents))*")
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


def trust_choice(screen: str, harness: str, repository: Path) -> str | None:
    lines = [line.strip() for line in screen.splitlines()]
    if any(re.search(r"(?i)^(?:[❯›>]\s*)?(?:allow external .*imports|external imports:|import .*settings|.*hooks.*(?:review|trust))", line) for line in lines):
        raise Refused("trust or import prompt is not authorized")
    if harness == "grok" and "Do you trust the contents of this directory?" in lines:
        body = [line for line in lines[lines.index("Do you trust the contents of this directory?"):] if line]
        body = [line if i == 1 else " ".join(line.split()) for i, line in enumerate(body)]
        expected = ["Do you trust the contents of this directory?", str(repository),
                    "Grok Build may run or modify contents in this directory,", "posing security risks.",
                    "Yes, proceed y", "No, quit n"]
        if (body[:6] != expected or len(body) != 7
                or not re.fullmatch(r"Grok Build 1\.0\.44 \[stable\]", body[-1])):
            raise Refused("unrecognized Grok trust dialog or different subject path")
        return "grok-yes"
    header = {"claude": "Accessing workspace:", "codex": "Folder access"}.get(harness)
    suspicious = re.search(r"(?i)do you trust|trust (?:this|the) (?:folder|directory|project)|one you trust|folder access|accessing workspace:", screen)
    if not header or header not in lines:
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


def accept_workspace_trust(pane: str, harness: str, repository: Path, screen: str) -> dict | None:
    choice = trust_choice(screen, harness, repository)
    if choice is None:
        return None
    # Re-read before each key; never send Enter based on an earlier screen.
    current = herdr("pane", "read", pane, "--source", "visible", text=True)
    if trust_choice(current, harness, repository) != choice:
        raise Refused("trust dialog changed before key delivery")
    if choice == "no":
        herdr("pane", "send-keys", pane, "Down" if harness == "claude" else "Up")
        time.sleep(0.2)
        current = herdr("pane", "read", pane, "--source", "visible", text=True)
        if trust_choice(current, harness, repository) != "yes":
            raise Refused("trust choice not visibly selected; no Enter sent")
    herdr("pane", "send-keys", pane, "y" if harness == "grok" else "Enter")
    return {"harness": harness, "path": str(repository),
            "screen_sha256": "sha256:" + hashlib.sha256(current.encode()).hexdigest(), "time": time.time()}


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


def decline_renderer(pane: str, screen: str) -> dict | None:
    choice = renderer_choice(screen)
    if choice is None:
        return None
    current = herdr("pane", "read", pane, "--source", "visible", text=True)
    if renderer_choice(current) != choice:
        raise Refused("renderer dialog changed before key delivery")
    if choice == "yes":
        herdr("pane", "send-keys", pane, "Down")
        time.sleep(0.2)
        current = herdr("pane", "read", pane, "--source", "visible", text=True)
        if renderer_choice(current) != "no":
            raise Refused("Not now not visibly selected; no Enter sent")
    herdr("pane", "send-keys", pane, "Enter")
    return {"harness": "claude", "dialog": "fullscreen-renderer", "choice": "Not now",
            "screen_sha256": "sha256:" + hashlib.sha256(current.encode()).hexdigest(), "time": time.time()}


def handle_startup(pane: str, harness: str, repository: Path, screen: str,
                   acceptances: list, display_choices: list) -> bool:
    if trust_choice(screen, harness, repository) is not None:
        if not acceptances:
            acceptances.append(accept_workspace_trust(pane, harness, repository, screen))
        return True
    if harness == "claude" and renderer_choice(screen) is not None:
        if not display_choices:
            display_choices.append(decline_renderer(pane, screen))
        return True
    return False


def wait_ready(pane: str, seconds: float, harness: str | None = None,
               repository: Path | None = None, acceptances: list | None = None,
               display_choices: list | None = None, branch: str | None = None) -> dict:
    end = time.monotonic() + seconds
    acceptances = acceptances if acceptances is not None else []
    display_choices = display_choices if display_choices is not None else []
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
            if not screen.strip():
                trust_pending = True
        current = state(pane)
        if not trust_pending and current.get("agent_status") in {"idle", "done"}:
            return current
        if (not trust_pending and current.get("agent_status") == "blocked") or time.monotonic() >= end:
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


AUTH_REFUSAL = "evaluator home not signed in: run prepare-eval-homes"


def check_evaluator_auth(harness: str, environment: dict[str, str], cwd: Path) -> dict:
    folder = kit.evaluator_homes()[harness]
    if environment.get(kit.EVALUATOR_HOME_VARIABLES[harness]) != str(folder) or not kit.evaluator_directory(folder):
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


def grok_authenticated_editor(screen: str) -> bool:
    # Public Grok welcome renderer: these menu rows require AuthState::Done
    # with access. Pending login can paint a prompt too, so prompt alone fails.
    if re.search(r"(?im)^\s*[│┃]?[ \t\u2800-\u28ff]*(?:Login with .+|Approve in your browser to finish signing in\.|A browser window will open for authentication\.|Switch account(?:\s+.*)?)[ \t]*[│┃]?\s*$", screen):
        return False
    lines = [line.strip() for line in screen.splitlines()]
    return (bool(re.search(r"(?m)^\s*[│┃]?[ \t\u2800-\u28ff]*New worktree[ \t]+ctrl\+w[ \t]*[│┃]?\s*$", screen))
            and bool(re.search(r"(?m)^\s*[│┃]?[ \t\u2800-\u28ff]*Resume session[ \t]+ctrl\+r[ \t]*[│┃]?\s*$", screen))
            and any(re.fullmatch(r"[│┃]?\s*[❯>]\s*(?:Type a message\.\.\.)?\s*[│┃]?", line) for line in lines))


def permission_flags(harness: str, native: list[str], cwd: Path | None = None) -> dict[str, str]:
    allowed = {"claude": {"--model", "--effort", "--permission-mode"},
               "codex": {"--model", "-c", "--ask-for-approval", "--sandbox"},
               "grok": {"--model", "--reasoning-effort", "--permission-mode", "--always-approve"}}[harness]
    values = {}
    index = 0
    personal = [Path.home() / name for name in (".claude", ".claude.json", ".codex", ".grok")]
    while index < len(native):
        flag, separator, value = native[index].partition("=")
        if flag not in allowed or flag in values:
            raise Refused(f"refused native flag: {flag if flag.startswith('-') else '<positional>'}", flag if flag.startswith('-') else "<positional>")
        index += 1
        if flag == "--always-approve":
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
    for flag in required:
        if flag not in values:
            raise Refused(f"refused native flag: {flag} (required)", flag)
    return {flag: values[flag] for flag in required}


CONFIG_FILES = {
    "claude": ("settings.json", "settings.local.json", "CLAUDE.md", "CLAUDE.local.md", "rules"),
    "codex": ("config.toml", "AGENTS.md", "AGENTS.override.md", "hooks.json", "rules"),
    "grok": ("config.toml", "managed_config.toml", "requirements.toml", "GROK.md", "AGENTS.md", "rules"),
}


def config_snapshot(environment: dict[str, str]) -> dict:
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
    return result


def config_drift(before: dict, after: dict) -> list[str]:
    return ["evaluator_config_drift"] if before != after else []


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
    native = ["codex", "--ask-for-approval", "never", "--sandbox", "danger-full-access",
              *kit.trial_native_args("codex", environment)]
    command = "env -i " + " ".join(assignments) + ' TERM="${TERM:-xterm-256color}" ' + shlex.join(native)
    return ("Run this command once in an app terminal. No harness was launched.\n"
            "This fresh disposable fixture carries the trial hooks. Accept its exact-path\n"
            "folder prompt, review the hooks, then trust them yourself. Use /hooks if needed.\n"
            "Do not submit TASK.md. Exit Codex after review; keep the evaluator home.\n\n"
            f"(cd {shlex.quote(str(repository))} && {command})\n")


def launch(args) -> None:
    record, repository, ancestry = load_fixture(args.record)
    trial = repository.parent
    environment = record["subject_environment"]
    native = args.native[1:] if args.native[:1] == ["--"] else args.native
    watched = sorted({str(Path(p).resolve()) for p in
                      [environment["TMPDIR"], "/private/tmp", *args.watch_dir]})
    observation = {"shallow": ["/private/tmp"],
                   "max_entries": args.max_entries, "max_seconds": args.snapshot_seconds}
    for directory in watched:
        if not Path(directory).is_dir():
            raise Refused(f"declared directory is unavailable: {directory}")
        if args.output.resolve().is_relative_to(Path(directory)):
            raise Refused("runner evidence must live outside declared directories")
    if args.output.exists():
        raise Refused("output already exists; never reuse a trial launch")
    try:
        permissions = permission_flags(args.harness, native, repository)
    except Refused as exc:
        args.output.mkdir(parents=True)
        write(args.output / "launch.json", {"harness": args.harness, "status": "refused",
                                           "refused_flag": exc.flag, "error": str(exc)})
        raise
    authentication = check_evaluator_auth(args.harness, environment, repository)
    # Validate all config roots before a native process could follow a link.
    config_snapshot(environment)
    native = [*native, *kit.trial_native_args(args.harness, environment)]
    args.output.mkdir(parents=True)
    run = {"schema_version": 1, "fixture_record": str(args.record.resolve()),
           "repository": str(repository), "harness": args.harness,
           "native_args": native, "permission_flags": permissions,
           "environment": environment, "declared_directories": watched,
           "observation": observation, "authentication": authentication,
           "instruction_ancestors": ancestry, "trust_acceptances": [], "display_choices": [],
           "status": "prepared", "started_at": time.time()}
    write(args.output / "launch.json", run)
    try:
        argv = ["tab", "create", "--workspace", args.workspace, "--label",
                f"eval-{record['case_id'][:30]}", "--cwd", str(repository), "--no-focus"]
        for key, value in environment.items():
            argv.extend(["--env", f"{key}={value}"])
        created = herdr(*argv)["result"]
        run.update(tab=created["tab"]["tab_id"], pane=created["root_pane"]["pane_id"])
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
        try:
            initial = wait_ready(run["pane"], args.start_timeout, args.harness, repository, run["trust_acceptances"], run["display_choices"], record.get("branch"))
        except Refused as exc:
            if args.harness == "grok":
                raise Refused(AUTH_REFUSAL) from exc
            raise
        if args.harness == "grok":
            if not grok_authenticated_editor(herdr("pane", "read", run["pane"], "--source", "visible", text=True)):
                raise Refused(AUTH_REFUSAL)
            run["authentication"]["signed_in"] = True
        run["config_start"] = config_snapshot(environment)
        run["initial_agent"] = initial
        write(args.output / "launch.json", run)
        write(args.output / "before.json", snapshot(watched, **observation))
        prompt = (repository / "TASK.md").read_text(encoding="utf-8")
        if args.harness == "claude":
            run["enters"] = deliver_claude(run["pane"], prompt, initial, args.start_timeout, record.get("branch"), args.output)
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
        raise
    finally:
        write(args.output / "launch.json", run)


def finish(args) -> None:
    run = kit.load_json(args.output / "launch.json")
    after = snapshot(run["declared_directories"], **run["observation"])
    before_path = args.output / "before.json"
    if before_path.is_file():
        result = compare(kit.load_json(before_path), after)
    else:
        result = {"validity_flags": ["directory_observation_incomplete", *after["validity_flags"]],
                  "error": "no pre-trial snapshot recorded after seat readiness"}
    try:
        run["config_finish"] = config_snapshot(run["environment"])
        if "config_start" not in run:
            result["validity_flags"].append("evaluator_config_unreadable")
        else:
            result["validity_flags"].extend(config_drift(run["config_start"], run["config_finish"]))
    except (Refused, OSError, KeyError) as exc:
        run["config_finish"] = {"error": str(exc)}
        result["validity_flags"].append("evaluator_config_unreadable")
    write(args.output / "launch.json", run)
    if run["status"] != "started":
        result["validity_flags"].append("native_launch_not_confirmed")
    write(args.output / "after.json", after)
    if "pane" in run:
        try:
            result["final_agent"] = state(run["pane"])
        except (Refused, subprocess.SubprocessError, ValueError) as exc:
            result["validity_flags"].append("native_state_unavailable")
            result["native_error"] = str(exc)
    write(args.output / "observation.json", result)
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
    start.add_argument("--watch-dir", action="append", default=[])
    start.add_argument("--max-entries", type=int, default=100_000)
    start.add_argument("--snapshot-seconds", type=float, default=10)
    start.add_argument("--start-timeout", type=float, default=30)
    start.add_argument("native", nargs=argparse.REMAINDER)
    review = sub.add_parser("print-hook-review", help="print an isolated native Codex hook-review command; never launch")
    review.add_argument("--record", type=Path, required=True)
    end = sub.add_parser("finish")
    end.add_argument("--output", type=Path, required=True)
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
        else:
            finish(args)
    except (Refused, OSError, subprocess.SubprocessError, KeyError, ValueError, kit.EvalError) as exc:
        print(f"qualification runner: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
