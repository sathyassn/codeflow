#!/usr/bin/env python3
"""Trial PATH launcher for seats a qualification subject opens (repository tooling).

The runner copies this file into a trial's peer-bin folder as `herdr`,
`claude`, `codex` and `grok`, next to a `context.json` it writes. It imports
nothing from the evaluator's checkout and never names it.

`herdr`: tab and pane creation get the trial environment and stay in the
trial's Herdr workspace; other workspaces are refused. Typing into another
pane waits for the watcher's ready record for that pane, then refuses; each
delivery let through is logged with its time and pane.
Everything else passes through unchanged.

`claude`, `codex`, `grok`: version, help, sign-in status and `mcp list` calls
pass through, logged as information calls. The subject's own seat start
passes through once, before readiness.
Any other start writes a request and waits for the runner's watcher, which
checks it and answers with the exact argv and environment to run, or a
refusal. Without an answer nothing runs. This arranges qualified launches;
it grants nothing. Every file here is writable by the subject, so the
watcher verifies each running peer from its live process state and flags
what it cannot verify, including seats started by another route.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import sys
import time

HERE = Path(__file__).parent
ROLE = Path(__file__).name
INFO_FLAGS = {"--version", "-V", "--help", "-h"}
STATUS = {"codex": ["login", "status"], "claude": ["auth", "status"]}
# Read-only subcommands answered as they are: listing MCP servers reads the
# configuration and starts no session. `add`, `remove` and every other
# subcommand still go to the watcher as launches.
READ_ONLY = (["mcp", "list"], ["mcp", "list", "--json"])
# Herdr commands that open a pane or a workspace.
CREATES = {("tab", "create"), ("pane", "split")}
OTHER_WORKSPACE = {("workspace", "create"), ("worktree", "create"), ("worktree", "open")}
# Herdr commands that type into a pane; a peer pane gets them only once ready.
DELIVERY = {("pane", "send-text"), ("pane", "send-keys"), ("pane", "run"),
            ("agent", "prompt"), ("agent", "send-keys")}
BOOLEAN_FLAGS = {"--current", "--wait", "--focus", "--no-focus"}
# Terminal variables the answer's environment does not carry.
PANE_VARIABLES = ("TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION")


def refuse(message: str) -> None:
    print(f"trial peer refused: {message}", file=sys.stderr)
    sys.exit(2)


def write_json(path: Path, value: dict) -> None:
    temporary = path.with_name(f".{path.name}.{secrets.token_hex(4)}.tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    os.replace(temporary, path)


def log(context: dict, kind: str, value: dict) -> None:
    # Best effort: a sandboxed subject shell may not write here.
    try:
        name = f"{time.time_ns()}-{os.getpid()}-{kind}.json"
        write_json(Path(context["launches"]) / name, {"kind": kind, "time": time.time(), **value})
    except OSError:
        pass


def option(args: list[str], index: int) -> tuple[str, str | None, int]:
    """Return (flag, value, next index) for `--flag value` or `--flag=value`."""
    flag, separator, value = args[index].partition("=")
    if separator:
        return flag, value, index + 1
    if index + 1 < len(args) and not args[index + 1].startswith("-"):
        return flag, args[index + 1], index + 2
    return flag, None, index + 1


def target(args: list[str]) -> str | None:
    """The pane or agent a Herdr command names: --pane, the first positional,
    or the caller's own pane for --current or no target."""
    positionals, index = [], 2
    while index < len(args):
        if args[index] in BOOLEAN_FLAGS or not args[index].startswith("-"):
            if not args[index].startswith("-"):
                positionals.append(args[index])
            index += 1
            continue
        flag, value, index = option(args, index)
        if flag == "--pane" and value:
            return value
    if positionals and "--current" not in args:
        return positionals[0]
    return os.environ.get("HERDR_PANE_ID") or None


def resolve(context: dict, kind: str, name: str | None) -> dict:
    """Ask the real Herdr which pane and workspace a target is."""
    if not name:
        refuse("cannot tell which pane this Herdr command targets")
    done = subprocess.run([context["herdr"], kind, "get", name], capture_output=True, text=True, timeout=30)
    try:
        info = json.loads(done.stdout)["result"][kind]
        return {"pane": info["pane_id"], "workspace": info["workspace_id"]}
    except (ValueError, KeyError, TypeError):
        refuse(f"cannot resolve the Herdr target {name}")


def ready(context: dict, pane: str) -> bool:
    gate = Path(context["launches"]) / "ready" / (re.sub(r"[^A-Za-z0-9_.-]", "_", pane) + ".json")
    deadline = time.monotonic() + float(context.get("ready_seconds", 30))
    while True:
        try:
            if json.loads(gate.read_text(encoding="utf-8")).get("ready") is True:
                return True
        except (OSError, ValueError):
            pass
        if time.monotonic() >= deadline:
            return False
        time.sleep(0.2)


def herdr(context: dict, args: list[str]) -> None:
    real = context["herdr"]
    command = tuple(args[:2])
    if command in OTHER_WORKSPACE:
        log(context, "herdr-refused", {"args": args})
        refuse(f"herdr {' '.join(command)}: peers stay in the trial's Herdr workspace")
    if command == ("pane", "split"):
        where = resolve(context, "pane", target(args))
        if where["workspace"] != context["workspace"]:
            log(context, "herdr-refused", {"args": args, "workspace": where["workspace"]})
            refuse("pane split: peers stay in the trial's Herdr workspace")
    if command in DELIVERY:
        where = resolve(context, "agent" if command[0] == "agent" else "pane", target(args))
        subject = (context.get("subject") or {}).get("pane")
        if where["pane"] != subject and (where["workspace"] != context["workspace"]
                                         or not ready(context, where["pane"])):
            log(context, "herdr-delivery-refused", {"args": args[:2], "pane": where["pane"]})
            refuse("this pane is not a verified ready peer; nothing was sent")
        log(context, "herdr-delivery", {"args": args[:2], "pane": where["pane"]})
    if command in CREATES:
        environment = context["environment"]
        given, workspace, index = set(), None, 2
        while index < len(args):
            flag, value, index = option(args, index)
            if flag == "--env" and value is not None:
                key, _, assigned = value.partition("=")
                if key in environment and assigned != environment[key]:
                    log(context, "herdr-refused", {"args": args})
                    refuse(f"--env {key} would override the trial environment")
                given.add(key)
            elif flag == "--workspace":
                workspace = value
        if command == ("tab", "create"):
            if workspace is None:
                args = [*args, "--workspace", context["workspace"]]
            elif workspace != context["workspace"]:
                log(context, "herdr-refused", {"args": args})
                refuse("tab create: peers stay in the trial's Herdr workspace")
        args = [*args, *(item for key, value in environment.items() if key not in given
                         for item in ("--env", f"{key}={value}"))]
        log(context, "herdr-create", {"args": args[:2], "workspace": context["workspace"],
                                      "environment_keys": sorted(environment)})
    os.execv(real, [real, *args])


def informational(harness: str, args: list[str]) -> bool:
    """Version, help, sign-in status and MCP listing calls start no session
    and change nothing; they run as the evaluator's real harness, so the
    answer is what the trial's seats would see."""
    if INFO_FLAGS & set(args) or args == ["help"]:
        return True
    rest = list(args)
    while rest[:1] == ["-c"] and len(rest) >= 2:
        rest = rest[2:]
    if rest in READ_ONLY:
        return True
    status = STATUS.get(harness)
    return status is not None and rest[:2] == status and len(rest) <= 3


def seat(context: dict, harness: str, args: list[str]) -> None:
    real = context["harnesses"][harness]
    pane = os.environ.get("HERDR_PANE_ID", "")
    if informational(harness, args):
        log(context, "info", {"harness": harness, "args": args, "pane": pane})
        os.execv(real, [real, *args])
    subject = context.get("subject") or {}
    if pane and pane == subject.get("pane") and not subject.get("ready"):
        # The runner's own seat start, already checked and recorded by it.
        log(context, "subject", {"harness": harness, "args": args, "pane": pane})
        os.execv(real, [real, *args])
    request_id = f"{time.time_ns()}-{os.getpid()}-{secrets.token_hex(4)}"
    launches = Path(context["launches"])
    request = {"id": request_id, "harness": harness, "args": args, "pane": pane,
               "workspace": os.environ.get("HERDR_WORKSPACE_ID", ""), "cwd": os.getcwd(),
               "pid": os.getpid(), "time": time.time(),
               "environment": {key: os.environ.get(key) for key in context["environment"]}}
    try:
        write_json(launches / f"{request_id}.request.json", request)
    except OSError as exc:
        refuse(f"cannot record this launch for the trial watcher: {exc}")
    answer = launches / f"{request_id}.decision.json"
    deadline = time.monotonic() + float(context.get("decision_seconds", 20))
    while not answer.is_file():
        if time.monotonic() >= deadline:
            refuse("no answer from the trial watcher; nothing was started")
        time.sleep(0.2)
    decision = json.loads(answer.read_text(encoding="utf-8"))
    if decision.get("id") != request_id or not decision.get("allow"):
        refuse(decision.get("error") or "the trial watcher refused this launch")
    argv = decision["argv"]
    if argv[:1] != [real]:
        refuse("the trial watcher's answer names another executable")
    environment = {key: value for key, value in os.environ.items()
                   if key.startswith("HERDR_") or key in PANE_VARIABLES}
    environment.update(decision.get("environment") or {})
    os.execve(real, argv, environment)


def main() -> None:
    context = json.loads((HERE / "context.json").read_text(encoding="utf-8"))
    if ROLE == "herdr":
        herdr(context, sys.argv[1:])
    elif ROLE in context["harnesses"]:
        seat(context, ROLE, sys.argv[1:])
    refuse(f"{ROLE} is not a trial launcher")


if __name__ == "__main__":
    main()
