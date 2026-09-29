#!/usr/bin/env python3
"""Build the v6 Claude presets, delegate fragment and Codex rules file from actions.json.

Prototype of review finding F6: one action table, per-harness syntax in the
generator, and a parity check that fails when a family has no entry and no
stated reason for a harness. In the task this becomes a Rust module or an
asset with a `settings_presets` parity test.
"""
import copy
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).parent
table = json.loads((HERE / "actions.json").read_text())


def parity_errors():
    errors = []
    for fam in table["families"]:
        if not fam.get("claude"):
            errors.append(f"{fam['id']}: no Claude entries")
        if not fam.get("codex") and not fam.get("codex_note"):
            errors.append(f"{fam['id']}: no Codex entries and no reason")
    return errors


def claude_preset(current):
    preset = copy.deepcopy(current)
    perms = preset["permissions"]
    ask, deny = [], []
    for fam in table["families"]:
        (ask if fam["decision"] == "ask" else deny).extend(fam["claude"])
    # `!` carve-outs apply only to rules listed before them in the same file
    # (docs: permissions, "Read and Edit"), so they follow the read denies.
    deny = (table["claude_read_denies"] + table["claude_read_carveouts"]
            + table["claude_edit_denies"] + deny)
    if ask:
        perms["ask"] = ask
    else:
        perms.pop("ask", None)
    perms["deny"] = deny
    creds = preset["sandbox"]["credentials"]["envVars"]
    for name in table["sandbox_env_denies"]:
        if not any(e["name"] == name for e in creds):
            creds.append({"name": name, "mode": "deny"})
    deny_read = preset["sandbox"]["filesystem"]["denyRead"]
    for path in table["sandbox_read_denies"]:
        if path not in deny_read:
            deny_read.append(path)
    for event, groups in preset["hooks"].items():
        for group in groups:
            for hook in group["hooks"]:
                name = hook["command"].split()[2]
                if name in GUARDS:
                    hook["command"] = fail_closed(name)
                elif "--contract" not in hook["command"]:
                    # Fable R2-2: a contract bump changes every hook's hash
                    # together, so Codex re-trust covers all of them.
                    hook["command"] = f"codeflow hook {name} --contract {CONTRACT}"
    return preset


GUARDS = ("git-guard", "exec-guard", "edit-guard")
CONTRACT = 3


def fail_closed(name):
    # Departure D8. Exit 0 allows; the guard's own exit 2 blocks with its own
    # reason; anything else (missing binary 127, crash, a binary older than
    # contract 3 rejecting the flag) is turned into exit 2 with a reason on
    # stderr, which Codex requires for a block. A timeout still fails open in
    # all three harnesses.
    return (f"codeflow hook {name} --contract {CONTRACT}; s=$?; [ \"$s\" -eq 0 ] && exit 0; "
            f"[ \"$s\" -eq 2 ] || echo \"codeflow {name} did not run (exit $s); refusing until it does\" >&2; exit 2")


def starlark(value):
    return json.dumps(value)


def codex_rules():
    out = [
        "# CodeFlow command rules for Codex, generated from actions.json.",
        "# Loaded only when the project `.codex/` layer is trusted. `forbidden`",
        "# refuses without prompting under every approval policy and sandbox,",
        "# including danger-full-access (codex-rs core/src/exec_policy.rs:393-404,",
        "# unified_exec/process_manager.rs:1479-1504 at 4fd5745e).",
        "",
    ]
    for fam in table["families"]:
        for rule in fam.get("codex", []):
            out.append("prefix_rule(")
            out.append(f"    pattern = {starlark(rule['pattern'])},")
            out.append('    decision = "forbidden",')
            out.append(f"    justification = {starlark(fam.get('codex_why', fam['why']))},")
            if rule.get("match"):
                out.append(f"    match = {starlark(rule['match'])},")
            if rule.get("not_match"):
                out.append(f"    not_match = {starlark(rule['not_match'])},")
            out.append(")")
            out.append("")
    return "\n".join(out)


def main():
    errors = parity_errors()
    if errors:
        sys.exit("parity check failed:\n" + "\n".join(errors))
    current = HERE / "current"
    claude = HERE / "claude"
    default = claude_preset(json.loads((current / "default.json").read_text()))
    (claude / "default.json").write_text(json.dumps(default, indent=2) + "\n")
    for name, mode in [("acceptEdits", "acceptEdits"), ("bypass-sandboxed", "bypassPermissions")]:
        variant = copy.deepcopy(default)
        variant["permissions"] = {"defaultMode": mode, **variant["permissions"]}
        (claude / f"{name}.json").write_text(json.dumps(variant, indent=2) + "\n")
    dogfood = {"$schema": default["$schema"],
               "env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "1000000",
                       "CLAUDE_AUTOCOMPACT_PCT_OVERRIDE": "50"}}
    dogfood.update({k: v for k, v in default.items() if k != "$schema"})
    (claude / "dogfood-settings.json").write_text(json.dumps(dogfood, indent=2) + "\n")
    workspace = dict(dogfood)
    workspace["includeCoAuthoredBy"] = False
    workspace["attribution"] = {"commit": "", "pr": "", "sessionUrl": False}
    (claude / "workspace-settings.json").write_text(json.dumps(workspace, indent=2) + "\n")
    (HERE / "codex" / "rules" / "codeflow.rules").write_text(codex_rules())
    fragment = {"permissions": {"deny": table["delegate_denies"]["claude"]}}
    (claude / "delegate-settings-fragment.json").write_text(json.dumps(fragment, indent=2) + "\n")
    for path in (HERE / "codex" / "hooks.json", HERE / "grok" / "hooks" / "codeflow.json"):
        data = json.loads(path.read_text())
        for event, groups in data["hooks"].items():
            for group in groups:
                for hook in group["hooks"]:
                    name = hook["command"].split()[2]
                    if name in GUARDS:
                        hook["command"] = fail_closed(name)
                    elif "--contract" not in hook["command"]:
                        hook["command"] = f"codeflow hook {name} --contract {CONTRACT}"
        path.write_text(json.dumps(data, indent=2) + "\n")
    rules = sum(len(f.get("codex", [])) for f in table["families"])
    print("ask", len(default["permissions"].get("ask", [])), "deny", len(default["permissions"]["deny"]),
          "codex forbidden rules", rules)


def parse_all():
    # Round 2 finding SET-R2-5: every shipped sample must parse.
    import tomllib
    bad = []
    for path in sorted(HERE.rglob("*")):
        rel = path.relative_to(HERE).as_posix()
        if rel.startswith(("current/", "diffs/")) or path.suffix not in (".json", ".toml"):
            continue
        try:
            (tomllib.loads if path.suffix == ".toml" else json.loads)(path.read_text())
        except Exception as err:
            bad.append(f"{rel}: {err}")
    if bad:
        sys.exit("unparseable samples:\n" + "\n".join(bad))
    print("all samples parse")


if __name__ == "__main__":
    main()
    parse_all()
