#!/usr/bin/env python3
"""Launch and observe one native interactive qualification trial.

Repository tooling only. No headless model execution or credential copying.
Use --help and README.md. A clean snapshot is bounded evidence, not confinement.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
from pathlib import Path
import re
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
    pass


def herdr(*args: str, text: bool = False):
    done = subprocess.run(["herdr", *args], capture_output=True, text=True, timeout=60)
    if done.returncode:
        raise Refused(f"herdr {' '.join(args[:2])}: {done.stderr or done.stdout}")
    return done.stdout if text else json.loads(done.stdout)


def write(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def snapshot(directories: list[str], *, shallow: list[str] | None = None,
             exclusions: list[str] | None = None, max_entries: int = 100_000,
             max_seconds: float = 10) -> dict:
    """Metadata only, with a streaming walk and cooperative time/entry caps.

    /private/tmp is always direct entries only. Other roots are recursive.
    No symlink targets or file contents are read. A blocking OS metadata call
    cannot be interrupted by this cooperative deadline.
    """
    started_at = time.monotonic()
    shallow_roots = {"/private/tmp", *(shallow or [])}
    excluded = [Path(p) for p in exclusions or []]
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
        if any(path == skip or path.is_relative_to(skip) for skip in excluded):
            return True
        if not budget():
            return False
        key = str(path)
        try:
            info = path.lstat()
            # Root mtimes duplicate direct-entry names and would make a
            # recorded exclusion's creation/removal invalidate the trial.
            entries[key] = ([stat.S_IFMT(info.st_mode)] if key in roots else
                            [stat.S_IFMT(info.st_mode), info.st_mtime_ns] if top_level else
                            [info.st_mode, info.st_size, info.st_mtime_ns, info.st_ctime_ns])
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
            "elapsed_seconds": elapsed,
            "exclusions": [str(p) for p in excluded]}


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
            "limitation": "Writes outside declared directories, below /private/tmp direct entries "
                          "(except separately watched roots), in recorded exclusions, and transient entries "
                          "gone before the final snapshot are not observed. Changes are not attributed to the subject."}


def scratch_exclusions(values: list[str], watched: list[str]) -> list[str]:
    """Accept only specifically declared Claude session scratch roots, not their shared parent."""
    result = []
    for value in values:
        path = Path(value).resolve()
        parts = path.parts
        if (not path.is_dir() or len(parts) < 6 or parts[:3] != ("/", "private", "tmp")
                or not parts[3].startswith("claude-")):
            raise Refused("evaluator scratch exclusion must be an existing session root below /private/tmp/claude-*/<project>")
        if any(Path(p).resolve().is_relative_to(path) for p in watched if p != "/private/tmp"):
            raise Refused("an exclusion must not contain a subject or additional watched root")
        result.append(str(path))
    return sorted(set(result))


# The same whole-frame grammar as release-qualification/lib.sh at 97f714b61.
# Empty is accepted only before pasting. It is never proof for a second Enter.
FOOTER = re.compile(r"  (?:⏸ manual mode on|⏵⏵ bypass permissions on \(shift\+tab to cycle\)|paste again to expand)(?: · (?:\? for shortcuts|← for agents))*")
FOLD = re.compile(r"\[Pasted text #\d+ \+\d+ lines\]")
DIRECTIVE = "Carry out the pasted instructions."
PLACEHOLDER = re.compile(r'Try "[^"\n]+"')


def editor(screen: str) -> str | None:
    raw = screen.splitlines()
    lines = [line.rstrip() for line in raw]
    rules = [i for i, line in enumerate(lines) if re.fullmatch(r"\s*─+\s*", line)]
    if len(rules) < 2:
        return None
    top, bottom = rules[-2:]
    body = lines[top + 1:bottom]
    footer = [line for line in lines[bottom + 1:] if line]
    if len(footer) == 2 and footer[0] == "  codeflow-qualify":
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


def visible(pane: str) -> str | None:
    return editor(herdr("pane", "read", pane, "--source", "visible", text=True))


def state(pane: str) -> dict:
    return herdr("agent", "get", pane)["result"]["agent"]


def wait_ready(pane: str, seconds: float) -> dict:
    end = time.monotonic() + seconds
    while True:
        current = state(pane)
        if current.get("agent_status") in {"idle", "done"}:
            return current
        if current.get("agent_status") == "blocked" or time.monotonic() >= end:
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


def deliver_claude(pane: str, prompt: str, initial: dict, seconds: float) -> int:
    if not prompt.strip() or len(prompt.encode("utf-8")) > 256 * 1024:
        raise Refused("prompt must be nonempty and at most 256 KiB; nothing sent")
    initial_text = visible(pane)
    if initial_text is None or (initial_text != "" and not PLACEHOLDER.fullmatch(initial_text)):
        raise Refused("no verified empty or placeholder Claude editor; nothing sent")
    herdr("pane", "send-text", pane, prompt)
    time.sleep(2)
    pending = visible(pane)
    if pending is not None and FOLD.fullmatch(pending):
        herdr("pane", "send-text", pane, DIRECTIVE)
        time.sleep(0.3)
        pending = visible(pane)
        if pending is None or not re.fullmatch(FOLD.pattern + re.escape(DIRECTIVE), pending):
            raise Refused("fold directive not visible in the editor; no Enter sent")
    if not holds(pending, prompt):
        raise Refused("the visible editor does not hold this prompt; no Enter sent")
    herdr("pane", "send-keys", pane, "Enter")
    if started(pane, initial, seconds):
        return 1
    # Never press on history, an empty editor, a dialog or an unreadable pane.
    if not holds(visible(pane), prompt):
        raise Refused("turn not confirmed and no verified pending prompt; no second Enter")
    herdr("pane", "send-keys", pane, "Enter")
    if not started(pane, initial, seconds):
        raise Refused("turn not confirmed after two verified Enters; inspect without resending")
    return 2


def permission_flags(harness: str, native: list[str]) -> dict[str, str]:
    required = {"codex": ("--ask-for-approval", "--sandbox"),
                "claude": ("--permission-mode",), "grok": ("--permission-mode",)}[harness]
    if any(arg in {"exec", "-p", "--print", "--single"} for arg in native):
        raise Refused("only native interactive arguments are allowed")
    if harness == "grok" and "--always-approve" in native:
        required = ("--always-approve",)
    result = {}
    for flag in required:
        if native.count(flag) != 1:
            raise Refused(f"supply one explicit {flag} in native arguments")
        index = native.index(flag)
        if flag == "--always-approve":
            result[flag] = "true"
        elif index + 1 >= len(native) or native[index + 1].startswith("-"):
            raise Refused(f"missing value for {flag}")
        else:
            result[flag] = native[index + 1]
    return result


def launch(args) -> None:
    record = kit.load_json(args.record)
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
    environment = record["subject_environment"]
    for key, path in {"HOME": trial / "home", "TMPDIR": trial / "tmp",
                      "CODEFLOW_HOME": trial / "home/.codeflow"}.items():
        if environment.get(key) != str(path):
            raise Refused(f"materialize with the current kit: {key} must be {path}")
    native = args.native[1:] if args.native[:1] == ["--"] else args.native
    permissions = permission_flags(args.harness, native)
    watched = sorted({str(Path(p).resolve()) for p in
                      [environment["TMPDIR"], "/private/tmp", *args.watch_dir]})
    exclusions = scratch_exclusions(args.evaluator_scratch_root, watched)
    observation = {"shallow": ["/private/tmp"], "exclusions": exclusions,
                   "max_entries": args.max_entries, "max_seconds": args.snapshot_seconds}
    for directory in watched:
        if not Path(directory).is_dir():
            raise Refused(f"declared directory is unavailable: {directory}")
        if args.output.resolve().is_relative_to(Path(directory)):
            raise Refused("runner evidence must live outside declared directories")
    if args.output.exists():
        raise Refused("output already exists; never reuse a trial launch")
    args.output.mkdir(parents=True)
    run = {"schema_version": 1, "fixture_record": str(args.record.resolve()),
           "repository": str(repository), "harness": args.harness,
           "native_args": native, "permission_flags": permissions,
           "environment": environment, "declared_directories": watched,
           "observation": observation,
           "recorded_exclusions": [{"path": p, "reason": "evaluator primary Claude Code session scratch root as launched"}
                                   for p in exclusions],
           "status": "prepared", "started_at": time.time()}
    write(args.output / "launch.json", run)
    write(args.output / "before.json", snapshot(watched, **observation))
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
        run["launch_response"] = herdr("agent", "start", f"eval-{trial.name}", "--kind",
                                      args.harness, "--pane", run["pane"], "--", *native)
        initial = wait_ready(run["pane"], args.start_timeout)
        run["initial_agent"] = initial
        prompt = (repository / "TASK.md").read_text(encoding="utf-8")
        if args.harness == "claude":
            run["enters"] = deliver_claude(run["pane"], prompt, initial, args.start_timeout)
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
    result = compare(kit.load_json(args.output / "before.json"), after)
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
    start.add_argument("--evaluator-scratch-root", action="append", default=[])
    start.add_argument("--max-entries", type=int, default=100_000)
    start.add_argument("--snapshot-seconds", type=float, default=10)
    start.add_argument("--start-timeout", type=float, default=30)
    start.add_argument("native", nargs=argparse.REMAINDER)
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
        else:
            finish(args)
    except (Refused, OSError, subprocess.SubprocessError, KeyError, ValueError, kit.EvalError) as exc:
        print(f"qualification runner: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
