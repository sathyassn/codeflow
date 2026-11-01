#!/usr/bin/env bash
# TSK-1040 AC-2 and AC-6 journey: on a scratch adopter repository, a push
# followed within seconds by two body edits ends with every codeflow-policy
# run completed and the required "commit standards" check passing on the head.
#
# Usage: policy-queue-journey.sh OWNER/REPO
#
# Needs (nothing here creates a repository):
#   - a scratch repository that already runs codeflow with the 4.0.0 managed
#     .github/workflows/codeflow-policy.yml (cancel-in-progress: false,
#     queue: max) on its default branch, with "commit standards" required;
#   - `gh` logged in for that repository with a token that can push a
#     branch, open and edit pull requests and read Actions runs
#     (classic scope `repo`; fine-grained: Contents, Pull requests and
#     Actions read/write on that one repository);
#   - a local clone of that repository as the current directory, with a
#     clean tree and the default branch checked out.
# The script never runs git credential commands. It leaves the pull request
# open and prints its run links; close it and delete the branch afterwards.
set -euo pipefail

repo=${1:?usage: policy-queue-journey.sh OWNER/REPO}
branch="task/TSK-1040-queue-journey-$(date +%s)"
body_file=$(mktemp)
trap 'rm -f "$body_file"' EXIT

git switch -c "$branch"
echo "journey $(date -u +%FT%TZ)" > journey.txt
git add journey.txt
git commit -m "chore(ci): exercise the policy queue" -m "- Add a journey marker file"
git push -u origin "$branch"
head=$(git rev-parse HEAD)

printf '## Summary\n\nPolicy queue journey.\n\n## Changes\n\n- Add a marker file\n' > "$body_file"
gh pr create --repo "$repo" --draft --title "chore(ci): exercise the policy queue" --body-file "$body_file" --head "$branch"
pr=$(gh pr view "$branch" --repo "$repo" --json number --jq .number)

# Two body edits within seconds of the push.
for n in 1 2; do
  printf '\nEdit %s.\n' "$n" >> "$body_file"
  gh pr edit "$pr" --repo "$repo" --body-file "$body_file"
  sleep 2
done

echo "waiting for every codeflow-policy run on $head"
deadline=$((SECONDS + 1800))
while :; do
  runs=$(gh run list --repo "$repo" --workflow codeflow-policy.yml --json databaseId,headSha,status,conclusion,event,url \
    --jq "[.[] | select(.event == \"pull_request_target\")]")
  pending=$(jq '[.[] | select(.status != "completed")] | length' <<<"$runs")
  [ "$pending" = 0 ] && break
  [ "$SECONDS" -lt "$deadline" ] || { echo "FAIL: runs still pending"; exit 1; }
  sleep 20
done

echo "$runs" | jq -r '.[] | "\(.url) \(.status) \(.conclusion)"'
bad=$(jq '[.[] | select(.conclusion == "cancelled")] | length' <<<"$runs")
[ "$bad" = 0 ] || { echo "FAIL: $bad cancelled run(s)"; exit 1; }

check=$(gh api "repos/$repo/commits/$head/check-runs" --jq '[.check_runs[] | select(.name == "commit standards")] | map(.conclusion)')
echo "commit standards on $head: $check"
jq -e 'length > 0 and all(. == "success" or . == "failure")' <<<"$check" >/dev/null \
  || { echo "FAIL: commit standards not completed on the head"; exit 1; }
jq -e 'index("success") != null' <<<"$check" >/dev/null \
  || { echo "FAIL: no passing commit standards check on the head"; exit 1; }
echo "PASS: no cancelled run; commit standards completed on $head (run links above)"
