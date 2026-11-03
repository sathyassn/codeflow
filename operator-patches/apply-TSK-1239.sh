#!/bin/sh
# Operator-only: apply the protected workflow pin after reviewing the patch.
# Agents must not run this script or use it to bypass git.hook_integrity.
set -eu
patch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
expected_branch=task/TSK-1239-commit-standards-admits-on-ci-completion
actual_branch=$(git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1239 branch --show-current)
if [ "$actual_branch" != "$expected_branch" ]; then
  echo "Refused: the assigned task branch is not checked out." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1239/scripts/ci-reuse.py | cut -d ' ' -f 1)
if [ "$actual_digest" != ea2604607ab5461e71bfc48d6091bc37003c19f875d33bb4106fb133180afa08 ]; then
  echo "Refused: the admission script changed since this patch was prepared." >&2
  exit 1
fi
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1239 apply --check "$patch_dir/TSK-1239-codeflow-ci.patch"
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1239 apply "$patch_dir/TSK-1239-codeflow-ci.patch"
echo "Applied the workflow pin. Review and commit it, then rerun the gate. Nothing was pushed."
