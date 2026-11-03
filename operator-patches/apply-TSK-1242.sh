#!/bin/sh
# Operator-only: add daily Windows coverage after reviewing the patch.
# Full PR Windows coverage stays on. Agents must not run this script.
# This applies the workflow files only; it does not commit or push.
set -eu
patch_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
worktree=/Volumes/DATA/Local/software-workspace/projects/agentic-systems/codeflow/.worktrees/tsk-1242
expected_branch=task/TSK-1242-a-daily-exact-sha-windows-run-covers-each-active-integration-line
actual_branch=$(git -C "$worktree" branch --show-current)
if [ "$actual_branch" != "$expected_branch" ]; then
  echo "Refused: the assigned task branch is not checked out." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 "$worktree/scripts/windows-daily.py" | cut -d ' ' -f 1)
if [ "$actual_digest" != aea1615431250a5ba2bcc7f186b87251b9987e4c7c2f2eb6b800d99ad80d7f8c ]; then
  echo "Refused: the daily reporting script changed since this patch was prepared." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 "$worktree/.github/workflows/codeflow-ci.yml" | cut -d ' ' -f 1)
if [ "$actual_digest" != c09aee58f2cf60c271bd9cc493aa57ef1db47119611b62f93e764b21ef68f8d5 ]; then
  echo "Refused: the existing Windows jobs changed; reconcile daily coverage before applying." >&2
  exit 1
fi
actual_digest=$(shasum -a 256 "$patch_dir/TSK-1242-windows-daily.patch" | cut -d ' ' -f 1)
if [ "$actual_digest" != 5ff0d9eeead561f4dc08dfc3e3de3e35aefaacd11c56f1d6bd97124d5bded252 ]; then
  echo "Refused: the workflow patch changed since this script was prepared." >&2
  exit 1
fi
git -C "$worktree" apply --check "$patch_dir/TSK-1242-windows-daily.patch"
git -C "$worktree" apply "$patch_dir/TSK-1242-windows-daily.patch"
echo "Applied the daily Windows workflows. Review and commit them, then rerun the gate. Nothing was pushed."
