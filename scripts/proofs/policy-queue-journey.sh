#!/usr/bin/env bash
# TSK-1040 AC-2 and AC-6 journey: on a scratch adopter repository, a push to
# an open pull request followed within seconds by two body edits ends with
# every codeflow-policy run for that pull request completed, none cancelled,
# and the required "commit standards" check passing on the final head for
# the final body.
#
# Usage:
#   policy-queue-journey.sh OWNER/REPO TSK-NNN   run the journey (changes the repository)
#   policy-queue-journey.sh --self-test          run the verdict function on fixtures
#                                                only; touches no repository
#
# Needs (nothing here creates a repository or a record):
#   - a scratch repository that already runs codeflow with the 4.0.0 managed
#     .github/workflows/codeflow-policy.yml (cancel-in-progress: false,
#     queue: max) on its default branch, with "commit standards" required;
#   - TSK-NNN: a task record on that default branch, status todo, whose
#     integration_target is the default branch, so the journey's pull
#     request names recorded work. Where the scratch repository does not
#     track work durably, pass the tracked unit's name instead; the branch
#     and the Task: line both carry it;
#   - `gh` logged in for that repository with a token that can push a
#     branch, open and edit pull requests and read Actions runs, jobs and
#     checks (classic scope `repo`; fine-grained: Contents and Pull
#     requests write, Actions read and Checks read on that one repository);
#   - a local clone of that repository as the current directory, with a
#     clean tree and the default branch checked out.
# The script never runs git credential commands. It leaves the pull request
# open and prints its run links; close it and delete the branch afterwards.
#
# The opening run must succeed before the push and the edits: a body the
# policy rejects proves nothing about queueing, so the journey stops there.
#
# The verdict fails when evidence is missing. It expects the runs the
# journey causes (the opening run, one run for the push and one for each of
# the two edits). Every run must complete and none may be cancelled. A run
# may fail only with the workflow's own stale-input diagnostic, which AC-3
# requires when the head, base branch or body changed while it ran; any
# other failure fails the journey. The newest run must succeed on the final
# head, and the newest "commit standards" check on that head must pass.
set -euo pipefail

workflow=codeflow-policy.yml
extra_events=3 # the push and the two body edits

# The workflow's diagnostics for a run whose judged input went stale
# (assets/base/ci/codeflow-policy.yml): its head fetch, and its final read.
stale_pattern='the pull request head moved|changed while this run judged it'

# verdict RUNS_JSON CHECKS_JSON WANT_RUNS WANT_CHECKS HEAD
#   RUNS_JSON: array of {id,status,conclusion,created_at,head_sha,html_url,
#     errors}, one per run, where errors lists the run's error annotations
#   CHECKS_JSON: array of {status,conclusion,started_at} for the "commit
#     standards" check runs on the final head (one per event that fired on
#     it: the push and the two edits)
#   HEAD: the final head SHA
verdict() {
  local runs=$1 checks=$2 want=$3 want_checks=$4 head=$5
  jq -e --argjson want "$want" 'length >= $want' <<<"$runs" >/dev/null \
    || { echo "FAIL: expected at least $want runs, saw $(jq length <<<"$runs"); a missing run is missing evidence"; return 1; }
  jq -e 'all(.[]; .status == "completed")' <<<"$runs" >/dev/null \
    || { echo "FAIL: a run has not completed"; return 1; }
  jq -e 'all(.[]; .conclusion != "cancelled")' <<<"$runs" >/dev/null \
    || { echo "FAIL: a run was cancelled"; return 1; }
  jq -e --arg stale "$stale_pattern" \
    'all(.[]; .conclusion == "success" or (.conclusion == "failure" and any(.errors[]?; test($stale))))' \
    <<<"$runs" >/dev/null \
    || { echo "FAIL: a run failed without the stale-input diagnostic"; return 1; }
  jq -e --arg head "$head" 'max_by(.created_at) | .conclusion == "success" and .head_sha == $head' <<<"$runs" >/dev/null \
    || { echo "FAIL: the newest run did not succeed on the final head"; return 1; }
  jq -e --argjson want "$want_checks" \
    'length >= $want and all(.[]; .status == "completed" and .conclusion != "cancelled")' <<<"$checks" >/dev/null \
    || { echo "FAIL: expected at least $want_checks completed, uncancelled commit standards checks on the head, saw $(jq -c 'map(.conclusion)' <<<"$checks")"; return 1; }
  jq -e 'max_by(.started_at) | .conclusion == "success"' <<<"$checks" >/dev/null \
    || { echo "FAIL: the newest commit standards check on the head did not pass"; return 1; }
  echo "PASS: $(jq length <<<"$runs") runs completed ($(jq '[.[] | select(.conclusion == "failure")] | length' <<<"$runs") failed as stale), none cancelled; the newest commit standards check on the head passed"
}

self_test() {
  local h=abc
  run() { # run CREATED CONCLUSION [ERROR] [HEAD]
    jq -n --arg c "$1" --arg k "$2" --arg e "${3:-}" --arg s "${4:-$h}" \
      '{status: "completed", conclusion: $k, created_at: $c, head_sha: $s, errors: (if $e == "" then [] else [$e] end)}'
  }
  local ok1 ok2 ok3 ok4 stale other
  ok1=$(run 1 success "" old)
  ok2=$(run 2 success)
  ok3=$(run 3 success)
  ok4=$(run 4 success)
  stale=$(run 3 failure "the pull request body changed while this run judged it (judged sha256 a, now b); the run queued by that edit decides")
  other=$(run 3 failure "Process completed with exit code 1.")
  local all; all=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$ok3" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local withstale; withstale=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$stale" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local withother; withother=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$other" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local pend; pend=$(jq -c '.[3].status = "in_progress" | .[3].conclusion = null' <<<"$all")
  local canc; canc=$(jq -c '.[3].conclusion = "cancelled"' <<<"$all")
  local lastfail; lastfail=$(jq -c --argjson s "$(run 5 failure "the pull request head changed while this run judged it (judged a, now b); the run queued by the new head decides")" '. + [$s]' <<<"$all")
  local lastold; lastold=$(jq -c --argjson s "$(run 5 success "" old)" '. + [$s]' <<<"$all")
  check() { jq -n --arg t "$1" --arg k "$2" '{status: "completed", conclusion: $k, started_at: $t}'; }
  local checks; checks=$(jq -n --argjson a "$(check 2 success)" --argjson b "$(check 3 success)" --argjson c "$(check 4 success)" '[$a,$b,$c]')
  local checksstale; checksstale=$(jq -n --argjson a "$(check 2 success)" --argjson b "$(check 3 failure)" --argjson c "$(check 4 success)" '[$a,$b,$c]')
  verdict "$all" "$checks" 4 3 "$h" >/dev/null || { echo "self-test: a complete journey must pass"; return 1; }
  verdict "$withstale" "$checksstale" 4 3 "$h" >/dev/null \
    || { echo "self-test: a superseded run failing with the stale-input diagnostic must pass"; return 1; }
  must_fail() { # must_fail LABEL REASON RUNS CHECKS: fails for that reason
    local out
    if out=$(verdict "$3" "$4" 4 3 "$h" 2>&1); then
      echo "self-test: '$1' must fail but passed"; return 1
    fi
    case $out in
      *"$2"*) ;;
      *) echo "self-test: '$1' failed for another reason: $out"; return 1 ;;
    esac
  }
  must_fail empty "expected at least 4 runs" '[]' "$checks"
  must_fail short "expected at least 4 runs" "$(jq -c '.[:3]' <<<"$all")" "$checks"
  must_fail pending "has not completed" "$pend" "$checks"
  must_fail cancelled "was cancelled" "$canc" "$checks"
  must_fail otherfailure "without the stale-input diagnostic" "$withother" "$checks"
  must_fail newestfailed "newest run did not succeed" "$lastfail" "$checks"
  must_fail newestoldhead "newest run did not succeed" "$lastold" "$checks"
  must_fail nochecks "commit standards checks on the head" "$all" '[]'
  must_fail shortchecks "commit standards checks on the head" "$all" "$(jq -c '.[:2]' <<<"$checks")"
  must_fail cancelledcheck "commit standards checks on the head" "$all" "$(jq -c '.[2].conclusion = "cancelled"' <<<"$checks")"
  must_fail newestcheckfailed "newest commit standards check" "$all" "$(jq -c '.[2].conclusion = "failure"' <<<"$checks")"
  echo "self-test ok: a complete journey and a stale-input failure pass; empty, short, pending, cancelled, other-failure, failed or old-head newest run, and missing, cancelled or failing checks each fail"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit
fi

repo=${1:?usage: policy-queue-journey.sh OWNER/REPO TSK-NNN | --self-test}
task=${2:?usage: policy-queue-journey.sh OWNER/REPO TSK-NNN | --self-test}
branch="task/${task}-queue-journey-$(date +%s)"
body_file=$(mktemp)
trap 'rm -f "$body_file"' EXIT

pr_runs() { # the runs of $workflow for pull request $1, with their error annotations
  local ids id errors out='[]' run
  ids=$(gh api --paginate "repos/$repo/actions/workflows/$workflow/runs?event=pull_request_target&per_page=100" \
    --jq ".workflow_runs[] | select(any(.pull_requests[]?; .number == $1)) | {id, status, conclusion, created_at, head_sha, html_url}" \
    | jq -s .)
  for id in $(jq -r '.[].id' <<<"$ids"); do
    errors=$(gh api "repos/$repo/actions/runs/$id/jobs" --jq '.jobs[].check_run_url' \
      | while read -r url; do gh api "$url/annotations" --jq '.[] | select(.annotation_level == "failure") | .message'; done \
      | jq -R . | jq -s .)
    run=$(jq --argjson id "$id" --argjson e "$errors" '.[] | select(.id == $id) | .errors = $e' <<<"$ids")
    out=$(jq --argjson r "$run" '. + [$r]' <<<"$out")
  done
  echo "$out"
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
#    The body is policy-valid for a draft: the Task: line names recorded
#    work and every section the range needs is present. The edits change
#    one word in the Summary paragraph, so each edited body stays valid.
git switch -c "$branch"
echo "journey $(date -u +%FT%TZ)" > journey.txt
git add journey.txt
git commit -m "chore(ci): exercise the policy queue" -m "- Add a journey marker file"
git push -u origin "$branch"
cat > "$body_file" <<EOF
Task: \`$task\`

## Summary

This draft exercises the policy workflow's queue on a scratch repository: a push and two body edits within seconds must each leave a completed run (edit 0).

- Adds a journey marker file.

## Changes

- Add \`journey.txt\`.

## Testing

- Revision and command: none; this pull request is the journey itself.

\`\`\`text
none
\`\`\`

- Coverage: not measured; no code changes.
- New tests: none.
- Not tested: nothing beyond the journey's own runs.

## Reviews

| Reviewer | Scope | Verdict |
|---|---|---|
| None | scratch journey | not reviewed |

## Release impact

- Impact: \`none\`
- Breaking: \`no\`
- Rationale: a scratch journey that is never merged.
- Migration: \`none\`
EOF
gh pr create --repo "$repo" --draft --title "chore(ci): exercise the policy queue" --body-file "$body_file" --head "$branch"
pr=$(gh pr view "$branch" --repo "$repo" --json number --jq .number)

# 2. Establish the starting state: the opening run has finished and passed.
opened_done() { local r; r=$(pr_runs "$pr"); [ "$(jq length <<<"$r")" -ge 1 ] && jq -e 'all(.[]; .status == "completed")' <<<"$r" >/dev/null; }
wait_until 900 "the opening run" opened_done
opening=$(pr_runs "$pr")
jq -e 'length >= 1 and all(.[]; .conclusion == "success")' <<<"$opening" >/dev/null || {
  echo "$opening" | jq -r '.[] | "\(.html_url) \(.conclusion) \(.errors | join("; "))"'
  echo "FAIL: the opening run did not pass, so the journey cannot judge queueing; fix the scratch repository, the task record or the body first"
  exit 1
}
start_count=$(jq length <<<"$opening")

# 3. Push a new head, then two body edits within seconds.
echo "second $(date -u +%FT%TZ)" >> journey.txt
git commit -am "chore(ci): push a second head" -m "- Append a line to the marker file"
git push origin "$branch"
head=$(git rev-parse HEAD)
for n in 1 2; do
  sed -i.bak "s/(edit $((n - 1)))/(edit $n)/" "$body_file" && rm -f "$body_file.bak"
  gh pr edit "$pr" --repo "$repo" --body-file "$body_file"
  sleep 2
done

# 4. Wait for the runs those three events cause, then judge them. Missing
#    runs are missing evidence: the wait times out and the journey fails.
want=$((start_count + extra_events))
all_done() { local r; r=$(pr_runs "$pr"); [ "$(jq length <<<"$r")" -ge "$want" ] && jq -e 'all(.[]; .status == "completed")' <<<"$r" >/dev/null; }
wait_until 1800 "$want completed runs for pull request $pr" all_done
runs=$(pr_runs "$pr")
echo "$runs" | jq -r '.[] | "\(.html_url) \(.head_sha[:9]) \(.status) \(.conclusion) \(.errors | join("; "))"'
checks=$(gh api --paginate "repos/$repo/commits/$head/check-runs?per_page=100" \
  --jq '.check_runs[] | select(.name == "commit standards") | {status, conclusion, started_at}' | jq -s .)
echo "commit standards on $head: $(jq -c 'map(.conclusion)' <<<"$checks")"
verdict "$runs" "$checks" "$want" "$extra_events" "$head"
