#!/bin/sh
# Operator-only: apply the authority-owned secret scan workflows after review.
# Agents must not run this script or use it to bypass git.hook_integrity.
# The patch updates both CI history scanning and the trusted policy verdict.
set -eu
patch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
expected_branch=task/TSK-1176-secret-scan-from-authority
actual_branch=$(git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1176 branch --show-current)
if [ "$actual_branch" != "$expected_branch" ]; then
  echo "Refused: the assigned task branch is not checked out." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1176/scripts/ci-reuse.py | cut -d ' ' -f 1)
if [ "$actual_digest" != 5e4005a5b295d99e3d0c29c9bba133f9b3fb308efbb93d8114a56313f0c482e1 ]; then
  echo "Refused: the reuse script changed since this patch was prepared." >&2
  exit 1
fi
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1176 apply --check "$patch_dir/TSK-1176-codeflow-ci.patch"
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1176 apply "$patch_dir/TSK-1176-codeflow-ci.patch"
echo "Applied the scan workflows. Review and commit them, then rerun verification. Nothing was pushed."
