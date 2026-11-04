#!/bin/sh
# Add the manual Linux measurement job and commit it locally.
# The Director reads this script before running it. Push is a separate step
# for the operator after the Director releases the push hold.
# Pass the reviewed task head and the approved line base as full commit SHAs.
# Both pins are checked against live refs; a moved ref requires a new review.
# This prepares the comparison only. It does not switch the integration tier.
set -eu
[ "$#" -eq 2 ] || { echo "usage: sh scripts/operator/tsk-1241/apply-codeflow-ci.sh <reviewed-head> <line-base>" >&2; exit 1; }
expected_head=$1
expected_base=$2
for pin in "$expected_head" "$expected_base"; do
  [ "${#pin}" -eq 40 ] || { echo 'Use full commit SHAs.' >&2; exit 1; }
  case "$pin" in *[!0-9a-f]*) echo 'Use lowercase hexadecimal SHAs.' >&2; exit 1;; esac
done
branch=task/TSK-1241-coverage-measurement-splits-from-linux-test-execution-on-integration-pull-requests
line=integration/EPC-022-the-3-2-0-milestone-fixes
refuse() { printf '%s\n' "$1" >&2; exit 1; }
[ "$(git branch --show-current)" = "$branch" ] || refuse 'Wrong task branch.'
[ "$(git rev-parse HEAD)" = "$expected_head" ] || refuse 'Task head moved.'
[ "$(git rev-parse "origin/$line")" = "$expected_base" ] || refuse 'Local line base moved.'
remote_base=$(git ls-remote --exit-code origin "refs/heads/$line" | awk '{print $1}')
[ "$remote_base" = "$expected_base" ] || refuse 'Remote line base moved.'
cd "$(git rev-parse --show-toplevel)"
git diff --quiet || refuse 'Tracked source edits are present.'
git diff --cached --quiet || refuse 'The index is not empty.'
git merge-base --is-ancestor "$expected_base" HEAD || refuse 'Merge the approved line base first.'
mkdir -p target/task-evidence
# Build a patch from the tracked job. Only git apply below writes the workflow.
python3 -I - <<'PY'
import difflib
from pathlib import Path

workflow = Path('.github/workflows/codeflow-ci.yml')
original = workflow.read_text()
job = Path('scripts/operator/tsk-1241/linux-measurement-job.yml').read_text()
anchor = '  gates-coverage:\n'
if original.count(anchor) != 1 or '  linux-measurement:\n' in original:
    raise SystemExit('Workflow layout changed or measurement job already exists; review the patch again.')
proposed = original.replace(anchor, job + '\n' + anchor, 1)
evidence = Path('target/task-evidence')
(evidence / 'codeflow-ci.proposed.yml').write_text(proposed)
(evidence / 'codeflow-ci.yml.patch').write_text(''.join(difflib.unified_diff(
    original.splitlines(True), proposed.splitlines(True),
    fromfile='a/.github/workflows/codeflow-ci.yml', tofile='b/.github/workflows/codeflow-ci.yml')))
PY
git apply --check target/task-evidence/codeflow-ci.yml.patch
git apply target/task-evidence/codeflow-ci.yml.patch
git add .github/workflows/codeflow-ci.yml
git commit -m 'ci: add manual Linux coverage comparison'
printf 'New head: %s\n' "$(git rev-parse HEAD)"
git diff --stat "$expected_head" HEAD
printf 'Nothing was pushed. After the Director releases the push hold, run:\n'
printf 'git push origin HEAD:refs/heads/%s\n' "$branch"
