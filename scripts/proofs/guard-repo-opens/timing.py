#!/usr/bin/env python3
"""Time exec-guard on nested-shell chains for two binaries (TSK-1050 proof tooling).

This is proof tooling, not product code. It builds the chains of
`shell_expansion_chains_have_bounded_guard_time` in
`crates/codeflow-cli/tests/hooks_cli.rs` (here-string, process
substitution, pipe and source routes, each followed by `rm -rf $PWD/x`),
runs `codeflow hook exec-guard` on each with the `before` and the `after`
binary in turn, and prints the best of `reps` runs as a Markdown table,
headed by each binary's `--version` line and sha256 and the host load.

The fixture lives at the physical path of a new temporary directory. On
macOS the temporary directory is reached through the `/var` symlink, and a
session spelled that way gives each target two spellings of one owner,
which the guard reads afresh for every target.

Usage: timing.py <before binary> <after binary> <reps> [counts, default 20,40,80,160]
"""

import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time

ROUTES = {
    "here": 'bash <<< "cd d{i}"',
    "process": 'bash <(echo "cd d{i}")',
    "pipe": 'echo "cd d{i}" | bash',
    "source": 'source <(echo "cd d{i}")',
}


def identity(binary):
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()
    with open(binary, "rb") as handle:
        return version, hashlib.sha256(handle.read()).hexdigest()


def main():
    before, after, reps = sys.argv[1], sys.argv[2], int(sys.argv[3])
    counts = [int(c) for c in (sys.argv[4] if len(sys.argv) > 4 else "20,40,80,160").split(",")]
    env = dict(os.environ)
    env.update(
        CODEFLOW_HOME=tempfile.mkdtemp(prefix="cf-timing-home-"),
        GIT_CONFIG_GLOBAL="/dev/null",
        GIT_CONFIG_SYSTEM="/dev/null",
    )
    for key in ("CODEFLOW_INTEGRATE_TOKEN", "CODEFLOW_HUMAN_OVERRIDE", "GIT_DIR",
                "GIT_WORK_TREE", "GIT_INDEX_FILE", "CDPATH"):
        env.pop(key, None)
    root = os.path.realpath(tempfile.mkdtemp(prefix="cf-timing-"))

    def git(*args):
        subprocess.run(["git", *args], cwd=root, env=env, check=True, capture_output=True)

    os.makedirs(os.path.join(root, ".claude"))
    git("init", "-b", "main")
    git("config", "user.email", "t@example.com")
    git("config", "user.name", "t")
    with open(os.path.join(root, "base.txt"), "w") as handle:
        handle.write("base\n")
    with open(os.path.join(root, ".claude", "settings.json"), "w") as handle:
        handle.write("{}\n")
    git("add", "-A")
    git("commit", "-m", "chore: init")
    os.makedirs(os.path.join(root, ".codeflow"))
    with open(os.path.join(root, ".codeflow", "policy.json"), "w") as handle:
        handle.write('{"git":{"protected_branches":["main","master"],"root_checkout_commits":"off"}}')
    git("add", "-A")
    git("commit", "-m", "chore: fixture policy")

    def once(binary, command):
        payload = json.dumps({"tool_name": "Bash", "tool_input": {"command": command}, "cwd": root})
        start = time.monotonic()
        out = subprocess.run([binary, "hook", "exec-guard", "--contract", "3"], cwd=root,
                             env=env, input=payload.encode(), capture_output=True, timeout=900)
        return time.monotonic() - start, out.returncode

    for side, binary in (("before", before), ("after", after)):
        version, digest = identity(binary)
        print(f"{side}: {version} sha256={digest}")
    print(f"load at start: {os.getloadavg()[0]:.1f}; best of {reps}")
    print("| Route | N | before (s) | after (s) | exit before/after |")
    print("|---|---|---|---|---|")
    for route, form in ROUTES.items():
        for count in counts:
            command = "; ".join(form.format(i=i) for i in range(count)) + "; rm -rf $PWD/x"
            best = {before: (float("inf"), None), after: (float("inf"), None)}
            for _ in range(reps):
                for binary in (before, after):
                    took, code = once(binary, command)
                    if took < best[binary][0]:
                        best[binary] = (took, code)
            print(f"| {route} | {count} | {best[before][0]:.2f} | {best[after][0]:.2f} "
                  f"| {best[before][1]}/{best[after][1]} |", flush=True)
    print(f"load at end: {os.getloadavg()[0]:.1f}")


if __name__ == "__main__":
    main()
