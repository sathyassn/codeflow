#!/usr/bin/env python3
"""Compare guard verdicts of two codeflow binaries (TSK-1050 proof tooling).

This is proof tooling, not product code. It runs each command below through
`codeflow hook exec-guard` and `codeflow hook git-guard`, as a harness runs
them (a JSON payload on stdin), with a `before` and an `after` binary, in a
disposable repository like the `enforced_repo` fixture of
`crates/codeflow-cli/tests/hooks_cli.rs` plus a linked worktree. It writes
every case's exit code and message for both binaries to a JSON file, with
each binary's `--version` line and sha256, and exits 1 if any verdict
differs. Lines that name the binary itself are left out of the comparison.

Usage: verdicts.py <before binary> <after binary> <results.json>
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile

SETTINGS = ".claude/settings.json"
COMMANDS = [
    "echo ok",
    "ls src",
    "python3 src/a.py",
    "python3 tool.py " + SETTINGS,
    "python3 tool.py .worktrees/w",
    "python3 tool.py $PWD/.worktrees/w",
    "cd src && python3 tool.py ../" + SETTINGS,
    "cd src && python3 tool.py ..",
    "python3 -c \"open('" + SETTINGS + "','w')\"",
    "python3 -c \"import shutil; shutil.rmtree('.worktrees/w')\"",
    "node -e \"require('fs').rmSync('.git/hooks',{recursive:true})\"",
    "bash <<< 'rm -rf .worktrees/w'",
    "bash <(echo 'rm -rf .git')",
    "echo 'rm -rf .worktrees/w' | bash",
    "source <(echo 'cat " + SETTINGS + "')",
    "bash <<< 'python3 tool.py .git/config'",
    "bash <(echo 'python3 tool.py src/a.py')",
    "rm -rf .worktrees/w",
    "rm -rf build",
    "cat ~/.ssh/id_rsa",
    "git worktree add -b t .worktrees/n && printf x > .worktrees/n/" + SETTINGS,
    "git status",
    "git -C .worktrees/w status",
]
for count in (5, 20):
    for form in (
        'bash <<< "cd d{i}"',
        'bash <(echo "cd d{i}")',
        'echo "cd d{i}" | bash',
        'source <(echo "cd d{i}")',
    ):
        chain = "; ".join(form.format(i=i) for i in range(count))
        COMMANDS.append(chain + "; rm -rf $PWD/x")
        COMMANDS.append(chain + "; python3 tool.py " + SETTINGS)


def identity(binary):
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
    with open(binary, "rb") as handle:
        digest = hashlib.sha256(handle.read()).hexdigest()
    return {"path": binary, "version": version, "sha256": digest}


def fixture(env):
    # The physical path, so every spelling of a target names one owner.
    root = os.path.realpath(tempfile.mkdtemp(prefix="cf-verdicts-"))

    def git(*args):
        subprocess.run(["git", *args], cwd=root, env=env, check=True, capture_output=True)

    os.makedirs(os.path.join(root, ".claude"))
    os.makedirs(os.path.join(root, "src"))
    git("init", "-b", "main")
    git("config", "user.email", "t@example.com")
    git("config", "user.name", "t")
    with open(os.path.join(root, "base.txt"), "w") as handle:
        handle.write("base\n")
    with open(os.path.join(root, "src", "a.py"), "w") as handle:
        handle.write("print(1)\n")
    with open(os.path.join(root, SETTINGS), "w") as handle:
        handle.write("{}\n")
    git("add", "-A")
    git("commit", "-m", "chore: init")
    os.makedirs(os.path.join(root, ".codeflow"))
    with open(os.path.join(root, ".codeflow", "policy.json"), "w") as handle:
        handle.write('{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}')
    git("add", "-A")
    git("commit", "-m", "chore: fixture policy")
    git("worktree", "add", "-b", "task/w", ".worktrees/w")
    return root


def verdict(binary, guard, command, root, env):
    payload = json.dumps({"tool_name": "Bash", "tool_input": {"command": command}, "cwd": root})
    out = subprocess.run(
        [binary, "hook", guard, "--contract", "3"],
        cwd=root, env=env, input=payload.encode(), capture_output=True, timeout=900,
    )
    lines = [
        line for line in out.stderr.decode().splitlines()
        if "binary=" not in line and "rebuild the hook binary" not in line
    ]
    return {"exit": out.returncode, "message": "\n".join(lines)}


def main():
    before, after, results_path = sys.argv[1:4]
    env = dict(os.environ)
    env.update(
        CODEFLOW_HOME=tempfile.mkdtemp(prefix="cf-verdicts-home-"),
        GIT_CONFIG_GLOBAL="/dev/null",
        GIT_CONFIG_SYSTEM="/dev/null",
    )
    for key in ("CODEFLOW_INTEGRATE_TOKEN", "CODEFLOW_HUMAN_OVERRIDE", "GIT_DIR",
                "GIT_WORK_TREE", "GIT_INDEX_FILE", "CDPATH"):
        env.pop(key, None)
    root = fixture(env)
    cases = []
    for guard in ("exec-guard", "git-guard"):
        for command in COMMANDS:
            first = verdict(before, guard, command, root, env)
            second = verdict(after, guard, command, root, env)
            cases.append({"guard": guard, "command": command, "before": first,
                          "after": second, "same": first == second})
    different = [case for case in cases if not case["same"]]
    report = {
        "before": identity(before),
        "after": identity(after),
        "commands": len(COMMANDS),
        "cases": len(cases),
        "identical": len(cases) - len(different),
        "different": len(different),
        "exit_codes_before": {
            str(code): sum(1 for case in cases if case["before"]["exit"] == code)
            for code in sorted({case["before"]["exit"] for case in cases})
        },
        "results": cases,
    }
    with open(results_path, "w") as handle:
        json.dump(report, handle, indent=1)
    for side in ("before", "after"):
        print(f"{side}: {report[side]['version']} sha256={report[side]['sha256']}")
    print(f"{report['cases']} cases ({report['commands']} commands x 2 guards): "
          f"{report['identical']} identical, {report['different']} different; "
          f"exit codes before {report['exit_codes_before']}")
    for case in different:
        print(f"DIFF {case['guard']}: {case['command']}")
    return 1 if different else 0


if __name__ == "__main__":
    sys.exit(main())
