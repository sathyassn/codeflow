#!/usr/bin/env python3
"""Check or apply the pinned coverage workflow and its parity template."""

import argparse
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FILES = {".github/workflows/codeflow-ci.yml", "scripts/gate-parity.py"}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def patched_files(text, originals):
    """Apply only exact text hunks to the two pinned, already-hashed inputs."""
    rows = text.splitlines(keepends=True)
    index = 0
    results = {}
    while index < len(rows):
        before = rows[index].removeprefix("--- a/").rstrip("\n")
        if (rows[index] != f"--- a/{before}\n" or before not in FILES
                or before in results or index + 1 >= len(rows)
                or rows[index + 1] != f"+++ b/{before}\n"):
            raise ValueError("patch must name each pinned file exactly once")
        index += 2
        source = originals[before].decode("utf-8").splitlines(keepends=True)
        output, cursor = [], 0
        hunks = 0
        while index < len(rows) and rows[index].startswith("@@ "):
            match = re.fullmatch(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@\n", rows[index])
            if match is None:
                raise ValueError("invalid patch hunk header")
            old_start, old_count, new_start, new_count = match.groups()
            old_count = int(old_count) if old_count is not None else 1
            new_count = int(new_count) if new_count is not None else 1
            old_start = int(old_start) - (1 if old_count else 0)
            new_start = int(new_start) - (1 if new_count else 0)
            if not cursor <= old_start <= len(source):
                raise ValueError("patch hunk overlaps or exceeds its input")
            output.extend(source[cursor:old_start])
            cursor = old_start
            if len(output) != new_start:
                raise ValueError("patch output position differs from its hunk")
            index += 1
            removed = added = 0
            while removed < old_count or added < new_count:
                if index >= len(rows) or rows[index][:1] not in (" ", "+", "-"):
                    raise ValueError("truncated patch hunk")
                row = rows[index]
                if row[0] in (" ", "-"):
                    if cursor >= len(source) or source[cursor] != row[1:]:
                        raise ValueError("patch context differs from the pinned input")
                    cursor += 1
                    removed += 1
                if row[0] in (" ", "+"):
                    output.append(row[1:])
                    added += 1
                index += 1
            if (removed, added) != (old_count, new_count):
                raise ValueError("patch hunk counts differ from its header")
            hunks += 1
        if not hunks:
            raise ValueError("patch file has no hunks")
        output.extend(source[cursor:])
        results[before] = "".join(output).encode("utf-8")
    if set(results) != FILES:
        raise ValueError("patch does not contain both pinned files")
    return results


@contextmanager
def preview(root=ROOT):
    manifest = json.loads((HERE / "patch-manifest.json").read_text(encoding="utf-8"))
    patch = HERE / "coverage-workflow.patch"
    if (manifest.get("version") != 1 or set(manifest.get("files", {})) != FILES
            or digest(patch.read_bytes()) != manifest.get("patch_sha256")):
        raise ValueError("patch or manifest differs from the reviewed bytes")
    with tempfile.TemporaryDirectory(prefix="tsk1261-patch-") as directory:
        scratch = Path(directory)
        originals = {}
        for name in sorted(FILES):
            source = root / name
            if any(p.is_symlink() for p in (source, *source.parents)):
                raise ValueError(f"{name}: source must not use a symbolic link")
            data = source.read_bytes()
            if digest(data) != manifest["files"][name]["before"]:
                raise ValueError(f"{name}: head copy differs from the pinned preimage; reconcile the patch")
            originals[name] = data
        for name, data in patched_files(patch.read_text(encoding="utf-8"), originals).items():
            destination = scratch / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        for name in sorted(FILES):
            if digest((scratch / name).read_bytes()) != manifest["files"][name]["after"]:
                raise ValueError(f"{name}: patched digest differs from the expected result")
        yield scratch, manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="patch temporary copies; print resulting SHA-256; leave the worktree unchanged")
    mode.add_argument("--apply", action="store_true", help="operator only: write both checked files")
    args = parser.parse_args()
    try:
        if args.apply and os.environ.get("CLAUDECODE"):
            raise ValueError("apply is operator-only; run --check in the agent session")
        with preview() as (scratch, manifest):
            for name in sorted(FILES):
                print(f"sha256 {manifest['files'][name]['after']}  {name}")
            if args.apply:
                # Recheck all inputs immediately before either write.
                for name in FILES:
                    if digest((ROOT / name).read_bytes()) != manifest["files"][name]["before"]:
                        raise ValueError(f"{name}: changed during the check; nothing applied")
                for name in sorted(FILES):
                    (ROOT / name).write_bytes((scratch / name).read_bytes())
                print("APPLIED coverage workflow and parity template; not committed")
            else:
                print("CHECK OK: patch applies to the pinned head copies; worktree unchanged")
    except (OSError, ValueError, KeyError) as error:
        print(f"REFUSED: {error}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
