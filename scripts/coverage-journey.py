#!/usr/bin/env python3
"""Drive changed-line report fixtures through a supplied worktree binary.

The caller initializes a scratch repository and supplies a fixture diff and
changed-path inventory. Base and head are synthetic event inputs; this proves
report execution through the binary, not caller-side Git resolution. This
script never invokes git or reaches a network. It preserves journey output
in that scratch repository. All coverage counters are hand-made fixture data.
"""

import argparse
import json
from pathlib import Path
import shlex
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--base", required=True)
    parser.add_argument("--head", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    if not (root / ".git").is_dir() or not (root / "range.diff").is_file() or not (root / "changed-paths").is_file():
        parser.error("scratch repository requires .git, range.diff and changed-paths")
    report_script = Path(__file__).resolve().with_name("changed-coverage.py")
    config = root / ".codeflow/test-config.json"
    config.parent.mkdir(exist_ok=True)
    (root / "coverage-config.json").write_text(json.dumps({"targets": [
        {"name": "rust-coverage", "modes": {"full": {"command": "fixture"}}}]}))
    (root / "coverage-plan.json").write_text('{"partitions":2}')
    (root / "coverage-exemptions.json").write_text('{"version":1,"exemptions":[]}')
    (root / "empty.diff").write_text("")
    common = [sys.executable, "-B", str(report_script)]
    for shard in (1, 2):
        directory = root / f"data/coverage-data-{shard}"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "coverage.info").write_text("SF:src/lib.rs\nDA:1,1\nDA:2,0\nend_of_record\n")
        bind = subprocess.run(common + ["bind", "--format", "lcov", "--directory", str(directory),
            "--source-root", str(root), "--head", args.head, "--run", "42", "--attempt", "1",
            "--shard", str(shard), "--shards", "2"], cwd=root, capture_output=True, text=True, check=False)
        if bind.returncode:
            raise RuntimeError(bind.stdout + bind.stderr)
    cases = (("pr-under-floor", "pull_request", args.base, "range.diff", 1, "50.00%"),
             ("pr-no-base", "pull_request", "unknown", "range.diff", 1, "no resolvable comparison revision"),
             ("pr-empty-diff", "pull_request", args.base, "empty.diff", 1, "empty changed set"),
             ("local-no-base", "local", None, "range.diff", 0, "unavailable: no comparison base; nothing enforced"))
    for name, event, base, diff, expected, needle in cases:
        output = root / f"{name}-report.txt"
        command = common + ["report", "--event", event, "--head", args.head, "--run", "42", "--attempt", "1",
            "--directory", str(root / "data"), "--config", str(root / "coverage-config.json"),
            "--plan", str(root / "coverage-plan.json"), "--exemptions", str(root / "coverage-exemptions.json"),
            "--diff", str(root / diff), "--paths", str(root / "changed-paths"), "--output", str(output)]
        if base is not None:
            command += ["--base", base]
        config.write_text(json.dumps({"schema_version": "1.0", "targets": [
            {"name": "changed-lines", "runner": "custom", "heavy": False,
             "modes": {"essential": {"command": shlex.join(command)}}}]}))
        result = subprocess.run([str(args.binary.resolve()), "test", "--mode", "essential", "--only", "changed-lines"],
                                cwd=root, capture_output=True, text=True, check=False)
        (root / f"{name}-gate.txt").write_text(result.stdout + result.stderr)
        if result.returncode != expected or not output.is_file() or needle not in output.read_text():
            raise RuntimeError(f"{name}: expected exit {expected}; got {result.returncode}\n{result.stdout}{result.stderr}")
        if name == "pr-under-floor" and "'src/lib.rs':2: uncovered" not in output.read_text():
            raise RuntimeError("fixture did not name its untested added line")
        print(f"{name}: codeflow exit {result.returncode}; {needle}; {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
