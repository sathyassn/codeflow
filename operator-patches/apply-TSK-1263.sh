#!/bin/sh
# Operator-only: prepare the line's apt cache and update the reuse script pin.
# This fixes repeated cache-missing fallbacks on integration pushes.
# Run on the assigned TSK-1263 branch after reviewing the adjacent patch.
# Expect one modified workflow to review and commit; nothing is pushed.
# Agents must not run this script or use it to bypass git.hook_integrity.
set -eu
patch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
expected_branch=task/TSK-1263-the-line-keeps-the-playwright-apt-cache-that-the-verdict-reuse-check-looks-for
actual_branch=$(git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1263 branch --show-current)
if [ "$actual_branch" != "$expected_branch" ]; then
  echo "Refused: the assigned task branch is not checked out." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1263/scripts/ci-reuse.py | cut -d ' ' -f 1)
if [ "$actual_digest" != b5a53a3f1239670b107c71f370534715ca66c6f32174c48a193e01dda38198af ]; then
  echo "Refused: the reuse script changed since this patch was prepared." >&2
  exit 1
fi
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1263 apply --check "$patch_dir/TSK-1263-codeflow-ci.patch"
git -C /Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1263 apply "$patch_dir/TSK-1263-codeflow-ci.patch"
echo "Applied the apt cache steps and reuse pin. Review and commit the workflow, then rerun the gate. Nothing was pushed."
