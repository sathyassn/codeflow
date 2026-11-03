#!/bin/sh
# Operator only: apply the reviewed main-only policy cache proposal on its task branch.
# This restores no integration dispatcher. It neither commits nor pushes.
# Main receives the identical reviewed workflow through the normal landing path.
set -eu
patch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo=/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1240
expected_branch=task/TSK-1240-commit-standards-caches-the-target-build-by-the
actual_branch=$(git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1240 branch --show-current)
if [ "$actual_branch" != "$expected_branch" ]; then
  echo "Refused: the assigned task branch is not checked out." >&2
  exit 1
fi
check_digest() {
  actual_digest=$(shasum -a 256 "$1" | cut -d ' ' -f 1)
  if [ "$actual_digest" != "$2" ]; then
    echo "Refused: pinned file changed: $1" >&2
    exit 1
  fi
}
# Baseline policy bytes from line 59c9ae96547aaf8241e0c66029c809b80d748f51.
check_digest "$repo/.github/workflows/codeflow-policy.yml" 8b3b7b2ca94ac31b1bfe021b1ebf64e0b0cb559f7aa5ac331c3543094325014a
check_digest "$patch_dir/TSK-1240-codeflow-policy.proposed.yml" 97b8d65e39038077d57fb00e09c870fc790440b256273825e0328040fc341066
check_digest "$patch_dir/TSK-1240-codeflow-policy.patch" fc2e0c50a71214f8382d9f40346f4fbeadf8794b503f3a825690b65dc6ce08d3
check_digest "$repo/scripts/test_policy_cache.py" 816e5190be78e315777e610698a27cfefe72b0f2facdb257e7f0877e7c3bf40d
check_digest "$repo/scripts/test_gate_contracts.py" 2484f535de50c5c443e60c07d8532e62b1cc4a50afde04fa12b3d6e6e8cc3fb6
check_digest "$repo/scripts/ci-reuse.py" 5e4005a5b295d99e3d0c29c9bba133f9b3fb308efbb93d8114a56313f0c482e1
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1240 apply --check "$patch_dir/TSK-1240-codeflow-policy.patch"
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1240 apply "$patch_dir/TSK-1240-codeflow-policy.patch"
cmp "$repo/.github/workflows/codeflow-policy.yml" "$patch_dir/TSK-1240-codeflow-policy.proposed.yml"
echo "Applied the exact pinned policy proposal. Review and commit it, then verify. Nothing was pushed."
