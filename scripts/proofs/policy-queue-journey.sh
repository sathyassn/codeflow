#!/usr/bin/env bash
# TSK-1040 AC-2 and AC-6 journey: on a scratch adopter repository, a push to
# an open pull request followed within seconds by two body edits ends with
# every codeflow-policy run for that pull request completed and successful,
# and the required "commit standards" check passing on the head.
#
# Usage:
#   policy-queue-journey.sh OWNER/REPO    run the journey (changes the repository)
#   policy-queue-journey.sh --self-test   run the verdict function on fixtures
#                                         only; touches no repository
#
# Needs (nothing here creates a repository):
#   - a scratch repository that already runs codeflow with the 4.0.0 managed
#     .github/workflows/codeflow-policy.yml (cancel-in-progress: false,
#     queue: max) on its default branch, with "commit standards" required;
#   - `gh` logged in for that repository with a token that can push a
#     branch, open and edit pull requests and read Actions runs and checks
#     (classic scope `repo`; fine-grained: Contents and Pull requests
#     write, Actions read and Checks read on that one repository);
#   - a local clone of that repository as the current directory, with a
#     clean tree and the default branch checked out.
# The script never runs git credential commands. It leaves the pull request
# open and prints its run links; close it and delete the branch afterwards.
#
# The verdict fails when evidence is missing: it expects the runs the
# journey causes (the opening run, one run for the push and one for each of
# the two edits) and treats a missing, pending, cancelled or unsuccessful run,
# or a missing or unsuccessful check, as a failure.
set -euo pipefail

workflow=codeflow-policy.yml
extra_events=3 # the push and the two body edits

# verdict RUNS_JSON CHECKS_JSON WANT_RUNS WANT_CHECKS
#   RUNS_JSON: array of {id,status,conclusion,created_at,html_url}, one per run
#   CHECKS_JSON: array of conclusions of the "commit standards" check on the
#     final head (one per event that fired on it: the push and the two edits)
verdict() {
  local runs=$1 checks=$2 want=$3 want_checks=$4
  jq -e --argjson want "$want" 'length >= $want' <<<"$runs" >/dev/null \
    || { echo "FAIL: expected at least $want runs, saw $(jq length <<<"$runs"); a missing run is missing evidence"; return 1; }
  jq -e 'all(.[]; .status == "completed")' <<<"$runs" >/dev/null \
    || { echo "FAIL: a run has not completed"; return 1; }
  jq -e 'all(.[]; .conclusion != "cancelled")' <<<"$runs" >/dev/null \
    || { echo "FAIL: a run was cancelled"; return 1; }
  jq -e 'all(.[]; .conclusion == "success")' <<<"$runs" >/dev/null \
    || { echo "FAIL: a run did not succeed"; return 1; }
  jq -e --argjson want "$want_checks" 'length >= $want and all(.[]; . == "success")' <<<"$checks" >/dev/null \
    || { echo "FAIL: expected at least $want_checks passing commit standards checks on the head, saw $checks"; return 1; }
  echo "PASS: $(jq length <<<"$runs") runs completed successfully and $(jq length <<<"$checks") commit standards checks passed"
}

self_test() {
  local ok='{"status":"completed","conclusion":"success"}'
  local runs4 pend fail canc
  runs4=$(jq -n --argjson r "$ok" '[$r,$r,$r,$r]')
  pend=$(jq -n --argjson r "$ok" '[$r,$r,$r,{"status":"in_progress","conclusion":null}]')
  fail=$(jq -n --argjson r "$ok" '[$r,$r,$r,{"status":"completed","conclusion":"failure"}]')
  canc=$(jq -n --argjson r "$ok" '[$r,$r,$r,{"status":"completed","conclusion":"cancelled"}]')
  local checks4='["success","success","success"]'
  verdict "$runs4" "$checks4" 4 3 >/dev/null || { echo "self-test: a complete journey must pass"; return 1; }
  must_fail() { # must_fail LABEL REASON RUNS CHECKS: fails for that reason
    local out
    if out=$(verdict "$3" "$4" 4 3 2>&1); then
      echo "self-test: '$1' must fail but passed"; return 1
    fi
    case $out in
      *"$2"*) ;;
      *) echo "self-test: '$1' failed for another reason: $out"; return 1 ;;
    esac
  }
  must_fail empty "expected at least 4 runs" '[]' "$checks4"
  must_fail short "expected at least 4 runs" "$(jq -c '.[:3]' <<<"$runs4")" "$checks4"
  must_fail pending "has not completed" "$pend" "$checks4"
  must_fail failed "did not succeed" "$fail" "$checks4"
  must_fail cancelled "was cancelled" "$canc" "$checks4"
  must_fail nochecks "passing commit standards checks" "$runs4" '[]'
  must_fail shortchecks "passing commit standards checks" "$runs4" '["success","success"]'
  must_fail failedcheck "passing commit standards checks" "$runs4" '["success","success","failure"]'
  echo "self-test ok: a complete journey passes; empty, short, pending, failed, cancelled and no-check evidence each fail"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit
fi

repo=${1:?usage: policy-queue-journey.sh OWNER/REPO | --self-test}
branch="task/TSK-1040-queue-journey-$(date +%s)"
body_file=$(mktemp)
trap 'rm -f "$body_file"' EXIT

pr_runs() { # the runs of $workflow for pull request $1
  gh api --paginate "repos/$repo/actions/workflows/$workflow/runs?event=pull_request_target&per_page=100" \
    --jq ".workflow_runs[] | select(any(.pull_requests[]?; .number == $1)) | {id, status, conclusion, created_at, html_url}" \
    | jq -s .
}

wait_until() { # wait_until SECONDS DESCRIPTION COMMAND...
  local limit=$1 what=$2 deadline
  shift 2
  deadline=$((SECONDS + limit))
  until "$@"; do
    [ "$SECONDS" -lt "$deadline" ] || { echo "FAIL: timed out waiting for $what"; exit 1; }
    sleep 15
  done
}

# 1. Open the pull request first, so the later push is a push to an open PR.
git switch -c "$branch"
echo "journey $(date -u +%FT%TZ)" > journey.txt
git add journey.txt
git commit -m "chore(ci): exercise the policy queue" -m "- Add a journey marker file"
git push -u origin "$branch"
printf '## Summary\n\nPolicy queue journey.\n\n## Changes\n\n- Add a marker file\n' > "$body_file"
gh pr create --repo "$repo" --draft --title "chore(ci): exercise the policy queue" --body-file "$body_file" --head "$branch"
pr=$(gh pr view "$branch" --repo "$repo" --json number --jq .number)

# 2. Establish the starting state: the opening run exists and has finished.
opened_done() { local r; r=$(pr_runs "$pr"); [ "$(jq length <<<"$r")" -ge 1 ] && jq -e 'all(.[]; .status == "completed")' <<<"$r" >/dev/null; }
wait_until 900 "the opening run" opened_done
start_count=$(pr_runs "$pr" | jq length)

# 3. Push a new head, then two body edits within seconds.
echo "second $(date -u +%FT%TZ)" >> journey.txt
git commit -am "chore(ci): push a second head" -m "- Append a line to the marker file"
git push origin "$branch"
head=$(git rev-parse HEAD)
for n in 1 2; do
  printf '\nEdit %s.\n' "$n" >> "$body_file"
  gh pr edit "$pr" --repo "$repo" --body-file "$body_file"
  sleep 2
done

# 4. Wait for the runs those three events cause, then judge them. Missing
#    runs are missing evidence: the wait times out and the journey fails.
want=$((start_count + extra_events))
all_done() { local r; r=$(pr_runs "$pr"); [ "$(jq length <<<"$r")" -ge "$want" ] && jq -e 'all(.[]; .status == "completed")' <<<"$r" >/dev/null; }
wait_until 1800 "$want completed runs for pull request $pr" all_done
runs=$(pr_runs "$pr")
echo "$runs" | jq -r '.[] | "\(.html_url) \(.status) \(.conclusion)"'
checks=$(gh api --paginate "repos/$repo/commits/$head/check-runs?per_page=100" \
  --jq '.check_runs[] | select(.name == "commit standards") | .conclusion' | jq -s .)
echo "commit standards on $head: $checks"
verdict "$runs" "$checks" "$want" "$extra_events"
