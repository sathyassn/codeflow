#!/usr/bin/env bash
# TSK-1040 queue journey, not AC-6 evidence (see the task record): on a
# scratch adopter repository, a push to
# an open pull request followed within seconds by two body edits ends with
# every codeflow-policy run for that pull request completed, none cancelled,
# and the required "commit standards" check passing on the final head
# for the final body.
#
# Usage:
#   policy-queue-journey.sh OWNER/REPO TSK-NNN   run the journey (changes the repository)
#   policy-queue-journey.sh --self-test          run the verdict function on fixtures
#                                                only; touches no repository
#
# Needs (nothing here creates a repository or a record):
#   - a scratch repository that already runs codeflow with the 4.0.0 managed
#     .github/workflows/codeflow-policy.yml (cancel-in-progress: false,
#     queue: max) on its default branch, with "commit standards"
#     required;
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
# may fail only in the step that detects stale input (the head fetch, or the
# final read), with that step's whole diagnostic and the runner's exit-code
# annotation and nothing else; AC-3 requires that failure when the head,
# base branch or body changed while the run judged it. Any other failure
# fails the journey. The newest run must succeed on the final head, and the
# newest "commit standards" check on that head must pass.
set -euo pipefail

workflow=codeflow-policy.yml
extra_events=3 # the push and the two body edits

# A run whose judged input went stale fails in exactly one of two steps of
# assets/base/ci/codeflow-policy.yml, each with its own whole diagnostic,
# plus the runner's exit-code annotation for that step. Anything else in a
# failed run, another failed step or any other failure annotation, fails
# the journey.
fetch_step="Fetch the pull request's base and head as data"
recheck_step='the pull request is unchanged since it was judged'
check_name='commit standards'
# shellcheck disable=SC2016 # jq source, expanded by jq, not the shell
stale_jq='
  def sha: "[0-9a-f]{40}";
  def moved: "^the pull request head moved \\(" + sha + ", event " + sha + "\\); this run belongs to an older event, so it fails; re-run the check for the current head$";
  def changed: "^the pull request (head changed while this run judged it \\(judged " + sha + ", now " + sha + "\\); the run queued by the new head decides|base branch changed while this run judged it \\(judged .+, now .+\\); the run queued by that change decides|body changed while this run judged it \\(judged sha256 [0-9a-f]{64}, now [0-9a-f]{64}\\); the run queued by that edit decides)$";
  def exited: "^Process completed with exit code 1\\.$";
  def stale_failure($fetch; $recheck):
    .conclusion == "failure"
    and (.errors | length) == 2
    and (.errors | map(select(test(exited))) | length) == 1
    and (
      (.failed_steps == [$fetch] and (.errors | map(select(test(moved))) | length) == 1)
      or (.failed_steps == [$recheck] and (.errors | map(select(test(changed))) | length) == 1)
    );
'

# verdict RUNS_JSON CHECKS_JSON WANT_RUNS WANT_CHECKS HEAD
#   RUNS_JSON: array of {id,status,conclusion,created_at,head_sha,html_url,
#     failed_steps,errors}, one per run: failed_steps names the steps whose
#     conclusion is failure, errors lists the run's failure annotations
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
  jq -e --arg fetch "$fetch_step" --arg recheck "$recheck_step" \
    "$stale_jq"' all(.[]; .conclusion == "success" or stale_failure($fetch; $recheck))' \
    <<<"$runs" >/dev/null \
    || { echo "FAIL: a run failed other than with only a stale-input step and its diagnostic"; return 1; }
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
  # The names the verdict matches must be the workflow's own: a fixture that
  # reuses a wrong name would pass here and fail the real journey.
  local root wf name
  root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
  for wf in "$root/assets/base/ci/codeflow-policy.yml" "$root/.github/workflows/codeflow-policy.yml"; do
    for name in "$fetch_step" "$recheck_step"; do
      grep -Fxq "      - name: $name" "$wf" \
        || { echo "self-test: $wf has no step named '$name'"; return 1; }
    done
    grep -Fxq "    name: \"$check_name\"" "$wf" \
      || { echo "self-test: $wf has no job named '$check_name'"; return 1; }
  done
  local h=abc
  h=$(printf 'a%.0s' {1..40})
  local o; o=$(printf 'b%.0s' {1..40})
  local d1 d2; d1=$(printf 'c%.0s' {1..64}); d2=$(printf 'd%.0s' {1..64})
  local exit1='Process completed with exit code 1.'
  local body_stale="the pull request body changed while this run judged it (judged sha256 $d1, now $d2); the run queued by that edit decides"
  local head_stale="the pull request head changed while this run judged it (judged $o, now $h); the run queued by the new head decides"
  local moved="the pull request head moved ($h, event $o); this run belongs to an older event, so it fails; re-run the check for the current head"
  run() { # run CREATED CONCLUSION HEAD [FAILED_STEPS_JSON] [ERRORS_JSON]
    jq -n --arg c "$1" --arg k "$2" --arg s "$3" --argjson f "${4:-[]}" --argjson e "${5:-[]}" \
      '{status: "completed", conclusion: $k, created_at: $c, head_sha: $s, failed_steps: $f, errors: $e}'
  }
  steps() { jq -nc '$ARGS.positional' --args "$@"; }
  local ok1 ok2 ok3 ok4 stale other
  ok1=$(run 1 success "$o")
  ok2=$(run 2 success "$h")
  ok3=$(run 3 success "$h")
  ok4=$(run 4 success "$h")
  stale=$(run 3 failure "$o" "$(steps "$recheck_step")" "$(steps "$body_stale" "$exit1")")
  local stalefetch; stalefetch=$(run 3 failure "$o" "$(steps "$fetch_step")" "$(steps "$moved" "$exit1")")
  other=$(run 3 failure "$h" "$(steps "codeflow ci (commit + branch + PR-body standards)")" "$(steps "$exit1")")
  # Round 07's negative cases: a valid stale diagnostic beside an unrelated
  # failed cleanup step, and an unrelated annotation quoting the diagnostic.
  local plusclean; plusclean=$(run 3 failure "$o" "$(steps "$recheck_step" "Post job cleanup.")" "$(steps "$body_stale" "$exit1" "cleanup failed" "$exit1")")
  local quoted; quoted=$(run 3 failure "$o" "$(steps "codeflow ci (commit + branch + PR-body standards)")" "$(steps "codeflow ci found: $body_stale" "$exit1")")
  local wrongstep; wrongstep=$(run 3 failure "$o" "$(steps "codeflow ci (commit + branch + PR-body standards)")" "$(steps "$body_stale" "$exit1")")
  local crossed; crossed=$(run 3 failure "$o" "$(steps "$fetch_step")" "$(steps "$body_stale" "$exit1")")
  local all; all=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$ok3" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local withstale; withstale=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$stale" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local withother; withother=$(jq -n --argjson a "$ok1" --argjson b "$ok2" --argjson c "$other" --argjson d "$ok4" '[$a,$b,$c,$d]')
  local pend; pend=$(jq -c '.[3].status = "in_progress" | .[3].conclusion = null' <<<"$all")
  local canc; canc=$(jq -c '.[3].conclusion = "cancelled"' <<<"$all")
  local lastfail; lastfail=$(jq -c --argjson s "$(run 5 failure "$h" "$(steps "$recheck_step")" "$(steps "$head_stale" "$exit1")")" '. + [$s]' <<<"$all")
  local lastold; lastold=$(jq -c --argjson s "$(run 5 success "$o")" '. + [$s]' <<<"$all")
  replace3() { jq -c --argjson r "$1" '.[2] = $r' <<<"$all"; }
  check() { jq -n --arg t "$1" --arg k "$2" '{status: "completed", conclusion: $k, started_at: $t}'; }
  local checks; checks=$(jq -n --argjson a "$(check 2 success)" --argjson b "$(check 3 success)" --argjson c "$(check 4 success)" '[$a,$b,$c]')
  local checksstale; checksstale=$(jq -n --argjson a "$(check 2 success)" --argjson b "$(check 3 failure)" --argjson c "$(check 4 success)" '[$a,$b,$c]')
  verdict "$all" "$checks" 4 3 "$h" >/dev/null || { echo "self-test: a complete journey must pass"; return 1; }
  verdict "$withstale" "$checksstale" 4 3 "$h" >/dev/null \
    || { echo "self-test: a superseded run failing with the stale-input diagnostic must pass"; return 1; }
  verdict "$(replace3 "$stalefetch")" "$checksstale" 4 3 "$h" >/dev/null \
    || { echo "self-test: a superseded run failing its head fetch must pass"; return 1; }
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
  must_fail otherfailure "other than with only a stale-input step" "$withother" "$checks"
  must_fail staleplusclean "other than with only a stale-input step" "$(replace3 "$plusclean")" "$checks"
  must_fail quoteddiagnostic "other than with only a stale-input step" "$(replace3 "$quoted")" "$checks"
  must_fail stalewrongstep "other than with only a stale-input step" "$(replace3 "$wrongstep")" "$checks"
  must_fail stalecrossedstep "other than with only a stale-input step" "$(replace3 "$crossed")" "$checks"
  must_fail newestfailed "newest run did not succeed" "$lastfail" "$checks"
  must_fail newestoldhead "newest run did not succeed" "$lastold" "$checks"
  must_fail nochecks "commit standards checks on the head" "$all" '[]'
  must_fail shortchecks "commit standards checks on the head" "$all" "$(jq -c '.[:2]' <<<"$checks")"
  must_fail cancelledcheck "commit standards checks on the head" "$all" "$(jq -c '.[2].conclusion = "cancelled"' <<<"$checks")"
  must_fail newestcheckfailed "newest commit standards check" "$all" "$(jq -c '.[2].conclusion = "failure"' <<<"$checks")"
  echo "self-test ok: a complete journey and a stale-input failure in either step pass; empty, short, pending, cancelled, other-failure, stale-plus-other-failure, quoted-diagnostic, wrong-step and crossed-step failures, a failed or old-head newest run, and missing, cancelled or failing checks each fail"
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

pr_runs() { # the runs of $workflow for pull request $1, with failed steps and failure annotations
  local ids id jobs failed errors out='[]' run
  ids=$(gh api --paginate "repos/$repo/actions/workflows/$workflow/runs?event=pull_request_target&per_page=100" \
    --jq ".workflow_runs[] | select(any(.pull_requests[]?; .number == $1)) | {id, status, conclusion, created_at, head_sha, html_url}" \
    | jq -s .)
  for id in $(jq -r '.[].id' <<<"$ids"); do
    jobs=$(gh api "repos/$repo/actions/runs/$id/jobs")
    failed=$(jq -c '[.jobs[].steps[] | select(.conclusion == "failure") | .name]' <<<"$jobs")
    errors=$(jq -r '.jobs[].check_run_url' <<<"$jobs" \
      | while read -r url; do gh api "$url/annotations" --jq '.[] | select(.annotation_level == "failure") | .message'; done \
      | jq -R . | jq -s .)
    run=$(jq --argjson id "$id" --argjson f "$failed" --argjson e "$errors" \
      '.[] | select(.id == $id) | .failed_steps = $f | .errors = $e' <<<"$ids")
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
  --jq ".check_runs[] | select(.name == \"$check_name\") | {status, conclusion, started_at}" | jq -s .)
echo "$check_name on $head: $(jq -c 'map(.conclusion)' <<<"$checks")"
verdict "$runs" "$checks" "$want" "$extra_events" "$head"
