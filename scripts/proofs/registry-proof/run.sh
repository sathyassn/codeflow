#!/usr/bin/env bash
# TSK-100: prove the shared id registry on a real host (SPC-013 R-6 to R-22,
# R-109). Proof tooling only; the product builds this in TSK-101.
#
# Usage: run.sh <step>
#   setup     record TSK-001 on main, seed the orphan registry, install the
#             code-branch check, add the baseline main ruleset
#   protect   AC-1: apply the data-branch ruleset through the API, refusals
#   issue     AC-2: issue by plain push, stale push rejected, retry
#   race      AC-2 addition: concurrent pushes of one number
#   lost-ack  AC-3: acknowledgement dropped, read back by uid, fault control
#   fork      AC-5: fork-style record red until admitted, then green
#   delete    AC-4: top reservation deleted, not reissued, CI reports it
#   pushrule  AC-6: probe the host's file-path push rule
#   pr-edit   probe: a pull request that edits the check workflow
#   evidence  AC-8: branch tree, workflow, one run of each job kind
#
# Environment: REPO (owner/name of the disposable repository), WORK (scratch
# directory for clones), OUT (transcript directory). Needs git, gh (logged
# in with repo and workflow scope), jq, python3 3.11 or later and uuidgen.
# Each step writes OUT/<step>.txt and exits non-zero when an assertion fails.
# Refused pushes are expected outcomes, so the script does not use set -e.

set -uo pipefail

STEP=${1:?usage: run.sh <step>}
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=${REPO:?set REPO to owner/name}
WORK=${WORK:?set WORK to a scratch directory}
OUT=${OUT:?set OUT to the transcript directory}
URL="https://github.com/$REPO.git"
REG=refs/heads/codeflow/registry
TRACK=refs/remotes/origin/codeflow/registry
IDS="$HERE/ids.py"
WORKFLOW=registry-check.yml
MAIN_RULESET=proof-main-code-profile
DATA_RULESET=codeflow-registry-data-profile
TAB=$'\t'
FAILS=0

mkdir -p "$WORK" "$OUT"
exec > >(tee "$OUT/$STEP.txt") 2>&1

say() { printf '\n## %s\n' "$*"; }
run() { printf '$ %s\n' "$*"; "$@"; local rc=$?; printf '[exit %d]\n' "$rc"; return "$rc"; }
die() { printf 'PROOF ERROR: %s\n' "$*"; exit 2; }
assert() {
  local desc=$1; shift
  if "$@"; then echo "PASS: $desc"; else echo "FAIL: $desc"; FAILS=$((FAILS + 1)); fi
}
uuid() { uuidgen | tr '[:upper:]' '[:lower:]'; }
now() { date -u +%Y-%m-%dT%H:%M:%SZ; }
since() { # 30 seconds ago, to absorb clock skew with the host
  python3 -c 'import datetime as d; print((d.datetime.now(d.UTC) - d.timedelta(seconds=30)).strftime("%Y-%m-%dT%H:%M:%SZ"))'
}

clone() { # name email -> path
  local dir="$WORK/$1"
  [ -d "$dir" ] || git clone -q "$URL" "$dir" || die "clone $1"
  git -C "$dir" config user.name "proof $1"
  git -C "$dir" config user.email "$2"
  echo "$dir"
}

fetch_registry() { # dir; never forced (R-11)
  run git -C "$1" fetch --no-tags -q origin "$REG:$TRACK"
}

ids() { # dir, ids.py arguments
  local d=$1; shift
  (cd "$d" && python3 "$IDS" "$@")
}

entry() { # kind number uid issuer [introduced]
  cat <<EOF
id = "$1-$2"
uid = "$3"
kind = "$1"
title = "Registry proof record $1-$2"
issuer = "$4"
created = "$(now)"
target = "main"
introduced = "${5:-none}"
landed = "none"
mapped = []
mapped_by = "none"
EOF
}

tree_commit() { # dir parent-or-empty subject, then index edits as extra args
  local d=$1 parent=$2 subject=$3 idx="$WORK/.index.$$" tree
  shift 3
  rm -f "$idx"
  if [ -n "$parent" ]; then GIT_INDEX_FILE=$idx git -C "$d" read-tree "$parent"; fi
  GIT_INDEX_FILE=$idx git -C "$d" update-index "$@" || die "update-index $*"
  tree=$(GIT_INDEX_FILE=$idx git -C "$d" write-tree) || die "write-tree"
  rm -f "$idx"
  if [ -n "$parent" ]; then
    git -C "$d" commit-tree "$tree" -p "$parent" -m "$subject"
  else
    git -C "$d" commit-tree "$tree" -m "$subject"
  fi
}

reserve_commit() { # dir kind number uid issuer subject [base]; commit on the fetched tip
  local d=$1 kind=$2 n=$3 uid=$4 issuer=$5 subject=$6 base=${7:-$TRACK} file blob
  file="$WORK/.entry.$$"
  entry "$kind" "$n" "$uid" "$issuer" > "$file"
  blob=$(git -C "$d" hash-object -w "$file") || die "hash-object"
  rm -f "$file"
  tree_commit "$d" "$(git -C "$d" rev-parse "$base")" "$subject" \
    --add --cacheinfo "100644,$blob,ids/$kind/$n.toml"
}

push_classify() { # dir commit; sets CLASS (R-13 outcome classes)
  local d=$1 c=$2 out rc
  printf '$ git push --porcelain origin %s:%s\n' "${c:0:12}" "$REG"
  out=$(git -C "$d" push --porcelain origin "$c:$REG" 2>&1); rc=$?
  printf '%s\n[exit %d]\n' "$out" "$rc"
  if [ "$rc" -eq 0 ] && grep -qE "^[ *+]${TAB}[^${TAB}]*:${REG}${TAB}" <<<"$out"; then
    CLASS=accepted
  elif grep -qE "^!${TAB}.*\[rejected\] \((fetch first|non-fast-forward)\)" <<<"$out"; then
    CLASS=moved # the client saw the moved tip in the host's ref advertisement
  elif grep -qE "^!${TAB}.*\[remote rejected\] \(cannot lock ref .* but expected " <<<"$out"; then
    CLASS=moved # a concurrent push won the host's compare-and-swap
  elif grep -qE "^!${TAB}.*\[remote rejected\]" <<<"$out"; then
    CLASS=refused
  elif grep -qiE "permission|denied|403" <<<"$out"; then
    CLASS=permission
  else
    CLASS=unclear
  fi
  echo "classified: $CLASS"
}

readback() { # dir id uid; 0 when the fetched registry binds id to uid
  local d=$1 id=$2 uid=$3 bound adder tip
  fetch_registry "$d" || return 2
  bound=$(ids "$d" bound --registry "$TRACK" --id "$id")
  tip=$(git -C "$d" rev-parse "$TRACK")
  echo "read back: $id bound to $bound on tip ${tip:0:12}"
  if [ "$bound" = "$uid" ]; then
    adder=$(git -C "$d" log --format=%H --diff-filter=A "$TRACK" -- "ids/${id%%-*}/${id#*-}.toml" | tail -1)
    echo "reserved: the tip reaches ${adder:0:12}, which binds $id to our uid"
    return 0
  fi
  echo "not reserved for uid $uid"
  return 1
}

issue() { # dir kind uid issuer; sets ISSUED (R-12, R-13)
  local d=$1 kind=$2 uid=$3 issuer=$4 attempt n c
  ISSUED=
  for attempt in 1 2 3 4 5; do
    echo "issue attempt $attempt for uid $uid"
    fetch_registry "$d" || { echo "stop: transport error on fetch"; return 1; }
    if ! ids "$d" check --registry "$TRACK"; then
      echo "issue refused: the registry check fails (R-9)"
      CLASS=damaged
      return 1
    fi
    n=$(ids "$d" next --registry "$TRACK" --kind "$kind")
    c=$(reserve_commit "$d" "$kind" "$n" "$uid" "$issuer" "issue: $kind-$n")
    push_classify "$d" "$c"
    case $CLASS in
      accepted) ISSUED="$kind-$n"; echo "reserved $ISSUED for uid $uid"; return 0 ;;
      moved) echo "the tip moved: fetch, recompute and retry (R-13)" ;;
      unclear) readback "$d" "$kind-$n" "$uid" && { ISSUED="$kind-$n"; return 0; } ;;
      *) echo "stop: $CLASS"; return 1 ;;
    esac
  done
  echo "stop: five attempts and the tip kept moving (R-13)"
  return 1
}

gh_json() { gh api "$@" 2>&1; }

snapshot() { # label; host rule listing
  local label=$1 ids_list
  say "host rule listing: $label"
  ids_list=$(gh api "repos/$REPO/rulesets" --jq '.[].id') || die "list rulesets"
  { for id in $ids_list; do gh api "repos/$REPO/rulesets/$id"; done; } \
    | jq -s 'sort_by(.name)' > "$OUT/rules-$label-rulesets.json"
  jq -n --argjson main "$(gh api "repos/$REPO/rules/branches/main")" \
        --argjson registry "$(gh api "repos/$REPO/rules/branches/codeflow%2Fregistry")" \
        '{effective_rules: {main: $main, "codeflow/registry": $registry}}' \
    > "$OUT/rules-$label-effective.json"
  echo "rulesets:"
  jq -c '.[] | {id, name, target, enforcement, include: .conditions.ref_name.include, rules: [.rules[].type], bypass_actors}' \
    "$OUT/rules-$label-rulesets.json"
  echo "effective rules:"
  jq -c '.effective_rules | to_entries[] | {branch: .key, rules: [.value[].type]}' "$OUT/rules-$label-effective.json"
  printf '$ gh api repos/%s/branches/main/protection\n' "$REPO"
  gh api "repos/$REPO/branches/main/protection" 2>&1 | head -3
}

ruleset_id() { gh api "repos/$REPO/rulesets" --jq ".[] | select(.name == \"$1\") | .id"; }

put_file() { # local-path repo-path message; commit to main through the contents API
  local src=$1 path=$2 msg=$3 sha args
  sha=$(gh api "repos/$REPO/contents/$path?ref=main" --jq .sha 2>/dev/null || true)
  args=(-X PUT "repos/$REPO/contents/$path" -f "message=$msg" -f "branch=main"
        -f "content=$(base64 < "$src" | tr -d '\n')")
  [ -n "$sha" ] && args+=(-f "sha=$sha")
  printf '$ gh api -X PUT repos/%s/contents/%s (message: %s)\n' "$REPO" "$path" "$msg"
  gh api "${args[@]}" --jq '"commit " + .commit.sha' || die "put $path"
}

wait_run() { # event branch since; sets RUN_ID and waits for completion
  local ev=$1 br=$2 since=$3 i id=
  for i in $(seq 1 90); do
    id=$(gh run list -R "$REPO" --workflow "$WORKFLOW" --event "$ev" --branch "$br" \
      --json databaseId,createdAt \
      --jq "[.[] | select(.createdAt >= \"$since\")] | sort_by(.createdAt) | last | .databaseId // empty")
    [ -n "$id" ] && break
    sleep 10
  done
  [ -n "$id" ] || { echo "no $ev run on $br since $since"; RUN_ID=; return 1; }
  RUN_ID=$id
  gh run watch -R "$REPO" "$id" --interval 5 > /dev/null 2>&1
  show_run "$id"
}

wait_attempt() { # run attempt
  local id=$1 want=$2 i state
  for i in $(seq 1 90); do
    state=$(gh run view -R "$REPO" "$id" --json attempt,status --jq '"\(.attempt) \(.status)"')
    [ "$state" = "$want completed" ] && break
    sleep 10
  done
  show_run "$id"
}

show_run() { # run id; prints the summary and saves the log of its latest attempt
  local id=$1 attempt event
  gh run view -R "$REPO" "$id" --json databaseId,event,headBranch,headSha,attempt,conclusion,url,createdAt
  attempt=$(gh run view -R "$REPO" "$id" --json attempt --jq .attempt)
  event=$(gh run view -R "$REPO" "$id" --json event --jq .event)
  gh run view -R "$REPO" "$id" --attempt "$attempt" --log > "$OUT/run-$id-$event-attempt$attempt.log" 2>&1
  echo "log: run-$id-$event-attempt$attempt.log"
  grep -E "FINDING|registry check:|bound:|event:" "$OUT/run-$id-$event-attempt$attempt.log" | sed $'s/^[^\t]*\t[^\t]*\t//'
}

conclusion() { gh run view -R "$REPO" "$1" --json conclusion --jq .conclusion; }

record_md() { # id uid
  cat <<EOF
---
id: $1
uid: $2
title: "registry proof record $1"
status: todo
---

# $1: registry proof record
EOF
}

step_setup() {
  say "run at $(now) against $REPO"
  snapshot initial
  local M U1 f intro seed tip since
  M=$(clone maint maintainer@proof.invalid)

  say "record TSK-001 on main (the code branch)"
  if gh api "repos/$REPO/contents/project-management/tasks/TSK-001.md?ref=main" > /dev/null 2>&1; then
    echo "TSK-001 already on main"
  else
    U1=$(uuid); f="$WORK/TSK-001.md"
    record_md TSK-001 "$U1" > "$f"
    put_file "$f" project-management/tasks/TSK-001.md "docs: add proof record TSK-001"
  fi
  run git -C "$M" fetch -q origin main
  U1=$(git -C "$M" show origin/main:project-management/tasks/TSK-001.md | sed -n 's/^uid: //p')
  intro=$(git -C "$M" log -1 --format=%H origin/main -- project-management/tasks/TSK-001.md)

  say "seed: orphan registry whose root commit adds only ids/ files (R-109)"
  if git -C "$M" ls-remote --exit-code origin "$REG" > /dev/null; then
    echo "codeflow/registry exists; a seed rerun adds nothing"
  else
    f="$WORK/seed.toml"
    entry TSK 001 "$U1" seed "$intro@main" > "$f"
    seed=$(tree_commit "$M" "" "seed: TSK-001" \
      --add --cacheinfo "100644,$(git -C "$M" hash-object -w "$f"),ids/TSK/001.toml")
    push_classify "$M" "$seed"
    assert "seed push accepted" [ "$CLASS" = accepted ]
  fi

  say "code-branch check on main: ci/ids.py, then the workflow"
  put_file "$IDS" ci/ids.py "ci: add registry proof check"
  since=$(since)
  put_file "$HERE/registry-check.yml" ".github/workflows/$WORKFLOW" "ci: run the registry check"
  wait_run push main "$since"
  assert "push job on main is green on the healthy registry" [ "$(conclusion "$RUN_ID")" = success ]

  say "baseline code-profile ruleset on main (fixture for the unchanged check)"
  if [ -n "$(ruleset_id "$MAIN_RULESET")" ]; then
    echo "$MAIN_RULESET exists"
  else
    run gh api -X POST "repos/$REPO/rulesets" --input - --jq '{id, name}' <<EOF
{"name": "$MAIN_RULESET", "target": "branch", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/heads/main"], "exclude": []}},
 "bypass_actors": [],
 "rules": [{"type": "deletion"}, {"type": "non_fast_forward"},
           {"type": "pull_request", "parameters": {"required_approving_review_count": 0,
            "dismiss_stale_reviews_on_push": false, "require_code_owner_review": false,
            "require_last_push_approval": false, "required_review_thread_resolution": false}}]}
EOF
  fi
}

step_protect() {
  say "run at $(now) against $REPO"
  snapshot before
  local M tip rewrite tip_after
  M=$(clone maint maintainer@proof.invalid)

  say "apply the data profile to codeflow/registry through the host API (R-6, R-22)"
  if [ -n "$(ruleset_id "$DATA_RULESET")" ]; then
    echo "$DATA_RULESET exists"
  else
    run gh api -X POST "repos/$REPO/rulesets" --input - --jq '{id, name}' <<EOF
{"name": "$DATA_RULESET", "target": "branch", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/heads/codeflow/registry"], "exclude": []}},
 "bypass_actors": [],
 "rules": [{"type": "deletion"}, {"type": "non_fast_forward"}]}
EOF
  fi
  snapshot after

  say "compare"
  local reg_rules
  reg_rules=$(jq -r '.effective_rules["codeflow/registry"] | map(.type) | sort | join(",")' "$OUT/rules-after-effective.json")
  assert "codeflow/registry refuses deletion and force push" [ "$reg_rules" = "deletion,non_fast_forward" ]
  assert "codeflow/registry has no pull-request requirement" \
    test "${reg_rules/pull_request/}" = "$reg_rules"
  run diff <(jq -S ".[] | select(.name == \"$MAIN_RULESET\")" "$OUT/rules-before-rulesets.json") \
           <(jq -S ".[] | select(.name == \"$MAIN_RULESET\")" "$OUT/rules-after-rulesets.json")
  assert "main's ruleset is byte-identical before and after" [ $? -eq 0 ]
  run diff <(jq -S '.effective_rules.main' "$OUT/rules-before-effective.json") \
           <(jq -S '.effective_rules.main' "$OUT/rules-after-effective.json")
  assert "main's effective rules are unchanged" [ $? -eq 0 ]

  say "refusals through git"
  fetch_registry "$M"
  tip=$(git -C "$M" rev-parse "$TRACK")
  run git -C "$M" push origin --delete codeflow/registry
  assert "git push --delete codeflow/registry is refused" [ $? -ne 0 ]
  rewrite=$(git -C "$M" commit-tree "$(git -C "$M" rev-parse "$TRACK^{tree}")" -m "rewrite: proof force push")
  echo "rewritten history: ${rewrite:0:12} (same tree, not a descendant of ${tip:0:12})"
  run git -C "$M" push --force origin "$rewrite:$REG"
  assert "git push --force onto codeflow/registry is refused" [ $? -ne 0 ]

  say "refusals through the REST API"
  run git -C "$M" push -q origin "$rewrite:refs/heads/proof/rewrite-candidate"
  run gh api -X PATCH "repos/$REPO/git/refs/heads/codeflow/registry" -f "sha=$rewrite" -F force=true
  assert "forced ref update through the API is refused" [ $? -ne 0 ]
  run gh api -X DELETE "repos/$REPO/git/refs/heads/codeflow/registry"
  assert "ref deletion through the API is refused" [ $? -ne 0 ]

  fetch_registry "$M"
  tip_after=$(git -C "$M" rev-parse "$TRACK")
  echo "tip before ${tip:0:12}, after ${tip_after:0:12}"
  assert "the registry tip is unchanged" [ "$tip" = "$tip_after" ]
}

step_issue() {
  say "run at $(now) against $REPO"
  local A B UA UB n nb c
  A=$(clone A issuer-a@proof.invalid); B=$(clone B issuer-b@proof.invalid)
  UA=$(uuid); UB=$(uuid)
  say "both issuers fetch the same tip"
  fetch_registry "$A"; fetch_registry "$B"
  echo "A sees ${TAB}$(git -C "$A" rev-parse --short=12 "$TRACK")"
  echo "B sees ${TAB}$(git -C "$B" rev-parse --short=12 "$TRACK")"

  say "A issues: plain push of the reservation (R-12)"
  run ids "$A" check --registry "$TRACK"
  n=$(ids "$A" next --registry "$TRACK" --kind TSK)
  c=$(reserve_commit "$A" TSK "$n" "$UA" issuer-a@proof.invalid "issue: TSK-$n")
  push_classify "$A" "$c"
  assert "A's plain push of TSK-$n is accepted" [ "$CLASS" = accepted ]

  say "B, still on the old tip, pushes onto the moved tip"
  nb=$(ids "$B" next --registry "$TRACK" --kind TSK)
  echo "B computed TSK-$nb from its stale view"
  c=$(reserve_commit "$B" TSK "$nb" "$UB" issuer-b@proof.invalid "issue: TSK-$nb")
  push_classify "$B" "$c"
  assert "B's plain push onto the moved tip is rejected as moved" [ "$CLASS" = moved ]

  say "B retries by the protocol (R-13)"
  issue "$B" TSK "$UB" issuer-b@proof.invalid
  assert "B's retry reserves the next number" [ "$ISSUED" = "TSK-$(printf '%03d' $((10#$n + 1)))" ]
  assert "TSK-$n is bound to A's uid" readback "$A" "TSK-$n" "$UA"
}

step_race() { # AC-2 addition: two issuers push the same number at the same moment
  say "run at $(now) against $REPO"
  local A B trial n ca cb ua ub oa ob wins moved
  A=$(clone A issuer-a@proof.invalid); B=$(clone B issuer-b@proof.invalid)
  for trial in 1 2 3; do
    say "trial $trial"
    fetch_registry "$A"; fetch_registry "$B"
    n=$(ids "$A" next --registry "$TRACK" --kind TSK)
    ua=$(uuid); ub=$(uuid)
    ca=$(reserve_commit "$A" TSK "$n" "$ua" issuer-a@proof.invalid "issue: TSK-$n")
    cb=$(reserve_commit "$B" TSK "$n" "$ub" issuer-b@proof.invalid "issue: TSK-$n")
    oa="$WORK/.race-a.$$"; ob="$WORK/.race-b.$$"
    echo "both push TSK-$n onto $(git -C "$A" rev-parse --short=12 "$TRACK") concurrently"
    push_classify "$A" "$ca" > "$oa" 2>&1 &
    push_classify "$B" "$cb" > "$ob" 2>&1 &
    wait
    echo "--- A"; cat "$oa"; echo "--- B"; cat "$ob"
    wins=$(grep -h '^classified: accepted' "$oa" "$ob" | wc -l | tr -d ' ')
    moved=$(grep -h '^classified: moved' "$oa" "$ob" | wc -l | tr -d ' ')
    rm -f "$oa" "$ob"
    assert "trial $trial: exactly one push of TSK-$n is accepted" [ "$wins" -eq 1 ]
    assert "trial $trial: the other is classified as a moved tip, to retry" [ "$moved" -eq 1 ]
    fetch_registry "$A"
    echo "TSK-$n is bound to $(ids "$A" bound --registry "$TRACK" --id "TSK-$n") (A $ua, B $ub)"
  done
}

step_lost_ack() {
  say "run at $(now) against $REPO"
  local A B UA UA2 n m c
  A=$(clone A issuer-a@proof.invalid); B=$(clone B issuer-b@proof.invalid)
  UA=$(uuid); UA2=$(uuid)

  say "A issues, and the acknowledgement is lost"
  fetch_registry "$A"
  run ids "$A" check --registry "$TRACK"
  n=$(ids "$A" next --registry "$TRACK" --kind TSK)
  c=$(reserve_commit "$A" TSK "$n" "$UA" issuer-a@proof.invalid "issue: TSK-$n")
  echo "\$ git push $URL ${c:0:12}:$REG  # output and exit status dropped"
  git -C "$A" push -q "$URL" "$c:$REG" > /dev/null 2>&1
  echo "A's outcome: unclear (no acknowledgement; A's remote-tracking ref not updated)"
  echo "A's view of the tip: $(git -C "$A" rev-parse --short=12 "$TRACK")"

  say "B issues on top, so A's commit is an ancestor, not the tip"
  issue "$B" TSK "$(uuid)" issuer-b@proof.invalid

  say "A resolves the unclear result by fetch and read back (R-13)"
  assert "A's TSK-$n reads back as reserved by uid" readback "$A" "TSK-$n" "$UA"
  assert "A's commit is an ancestor of the fetched tip" git -C "$A" merge-base --is-ancestor "$c" "$TRACK"
  assert "A's commit is not the tip" [ "$c" != "$(git -C "$A" rev-parse "$TRACK")" ]

  say "fault control: A's push never reaches the host, and B takes the number"
  fetch_registry "$A"
  m=$(ids "$A" next --registry "$TRACK" --kind TSK)
  c=$(reserve_commit "$A" TSK "$m" "$UA2" issuer-a@proof.invalid "issue: TSK-$m")
  echo "A prepared ${c:0:12} for TSK-$m; the push is dropped before it is sent"
  issue "$B" TSK "$(uuid)" issuer-b@proof.invalid
  readback "$A" "TSK-$m" "$UA2"
  assert "A does not treat TSK-$m as reserved: bound to another uid" [ $? -ne 0 ]
  issue "$A" TSK "$UA2" issuer-a@proof.invalid
  assert "A then reserves a fresh number" test -n "$ISSUED" -a "$ISSUED" != "TSK-$m"
}

step_fork() {
  say "run at $(now) against $REPO"
  local C M UF n branch since c
  C=$(clone contrib contributor@proof.invalid); M=$(clone maint maintainer@proof.invalid)
  UF=$(uuid)
  run git -C "$C" fetch -q origin main
  fetch_registry "$C"
  n=$(ids "$C" next --registry "$TRACK" --kind TSK)
  branch="task/TSK-$n-fork-record"

  say "a fork-style contributor writes TSK-$n by hand; it has no reservation"
  run git -C "$C" switch -q -C "$branch" origin/main
  mkdir -p "$C/project-management/tasks"
  record_md "TSK-$n" "$UF" > "$C/project-management/tasks/TSK-$n.md"
  run git -C "$C" add "project-management/tasks/TSK-$n.md"
  run git -C "$C" commit -q -m "docs: add TSK-$n by hand"
  since=$(since)
  run git -C "$C" push -q origin "$branch"
  run gh pr create -R "$REPO" --base main --head "$branch" \
    --title "Add TSK-$n from a fork-style contributor" \
    --body "Proof fixture for TSK-100: a hand-written record with no reservation."
  wait_run pull_request "$branch" "$since"
  local run_id=$RUN_ID
  assert "the PR check is red before admission" [ "$(conclusion "$run_id")" = failure ]
  assert "the PR check names the missing reservation" \
    grep -q "TSK-$n: not reserved" "$OUT/run-$run_id-pull_request-attempt1.log"

  say "a maintainer admits the record's uid (R-17)"
  fetch_registry "$M"
  echo "TSK-$n is currently bound to: $(ids "$M" bound --registry "$TRACK" --id "TSK-$n")"
  c=$(reserve_commit "$M" TSK "$n" "$UF" admit:maintainer@proof.invalid "admit: TSK-$n")
  push_classify "$M" "$c"
  assert "the admission push is accepted" [ "$CLASS" = accepted ]

  say "the same PR check runs again"
  run gh run rerun -R "$REPO" "$run_id"
  sleep 5
  wait_attempt "$run_id" 2
  assert "the PR check is green after admission" [ "$(conclusion "$run_id")" = success ]
  assert "the PR check reports the binding" \
    grep -q "bound: TSK-$n -> $UF" "$OUT/run-$run_id-pull_request-attempt2.log"
  echo "$branch" > "$WORK/fork-branch"
}

step_delete() {
  say "run at $(now) against $REPO"
  local M A C top c hist tip_next branch since pr_run
  M=$(clone maint maintainer@proof.invalid); A=$(clone A issuer-a@proof.invalid)
  fetch_registry "$M"
  top=$(git -C "$M" ls-tree -r --name-only "$TRACK" -- ids/TSK/ | sed 's#ids/TSK/##; s#\.toml##' | sort -n | tail -1)

  say "a plain commit deletes the top reservation ids/TSK/$top.toml"
  c=$(tree_commit "$M" "$(git -C "$M" rev-parse "$TRACK")" "remove TSK-$top" \
    --force-remove "ids/TSK/$top.toml")
  push_classify "$M" "$c"
  assert "the host accepts the deleting commit (rules do not cover contents; AC-6)" \
    test "$CLASS" = accepted

  say "allocation after the deletion (R-7)"
  fetch_registry "$A"
  tip_next=$(ids "$A" next --registry "$TRACK" --kind TSK --tip-only)
  hist=$(ids "$A" next --registry "$TRACK" --kind TSK)
  echo "tip-only allocation would issue TSK-$tip_next; history allocation gives TSK-$hist"
  assert "tip-only allocation would reissue the deleted number (the fault)" [ "$tip_next" = "$top" ]
  assert "history allocation does not reissue TSK-$top" [ "$((10#$hist))" -gt "$((10#$top))" ]

  say "issue is refused while the registry is damaged (R-9)"
  issue "$A" TSK "$(uuid)" issuer-a@proof.invalid
  assert "issue is refused" test "$CLASS" = damaged -a -z "$ISSUED"

  say "the registry check from the code branch's CI reports the deletion"
  branch=$(cat "$WORK/fork-branch" 2>/dev/null) || die "run the fork step first"
  C=$(clone contrib contributor@proof.invalid)
  run git -C "$C" switch -q "$branch"
  run git -C "$C" commit -q --allow-empty -m "chore: rerun the registry check"
  since=$(since)
  run git -C "$C" push -q origin "$branch"
  wait_run pull_request "$branch" "$since"
  pr_run=$RUN_ID
  assert "the PR job is red" [ "$(conclusion "$pr_run")" = failure ]
  assert "the PR job reports the deleting commit" \
    grep -q "deletes ids/TSK/$top.toml" "$OUT/run-$pr_run-pull_request-attempt1.log"
  assert "the PR job reports the damaged tip" \
    grep -q "damaged: ids/TSK/$top.toml" "$OUT/run-$pr_run-pull_request-attempt1.log"
  since=$(since)
  run gh workflow run -R "$REPO" "$WORKFLOW" --ref main
  wait_run workflow_dispatch main "$since"
  assert "a run on main is red and reports the deletion" \
    grep -q "deletes ids/TSK/$top.toml" "$OUT/run-$RUN_ID-workflow_dispatch-attempt1.log"
}

step_pushrule() {
  say "run at $(now) against $REPO"
  local out id
  say "probe: push ruleset with a file-path restriction on ids/"
  printf '$ gh api -X POST repos/%s/rulesets  # target push, file_path_restriction ids/**\n' "$REPO"
  out=$(gh api -X POST "repos/$REPO/rulesets" --input - 2>&1 <<EOF
{"name": "proof-ids-file-path", "target": "push", "enforcement": "active",
 "rules": [{"type": "file_path_restriction",
            "parameters": {"restricted_file_paths": ["ids/**"]}}]}
EOF
); echo "$out"
  id=$(jq -r '.id // empty' <<<"$out" 2>/dev/null)
  [ -n "$id" ] && { echo "created push ruleset $id; removing it"; run gh api -X DELETE "repos/$REPO/rulesets/$id"; }

  say "probe: branch ruleset with a file-path restriction on codeflow/registry"
  printf '$ gh api -X POST repos/%s/rulesets  # target branch, file_path_restriction ids/**\n' "$REPO"
  out=$(gh api -X POST "repos/$REPO/rulesets" --input - 2>&1 <<EOF
{"name": "proof-ids-file-path-branch", "target": "branch", "enforcement": "active",
 "conditions": {"ref_name": {"include": ["refs/heads/codeflow/registry"], "exclude": []}},
 "rules": [{"type": "file_path_restriction",
            "parameters": {"restricted_file_paths": ["ids/**"]}}]}
EOF
); echo "$out"
  id=$(jq -r '.id // empty' <<<"$out" 2>/dev/null)
  if [ -n "$id" ]; then
    echo "created branch ruleset $id; testing whether it also refuses an addition"
    local M c
    M=$(clone maint maintainer@proof.invalid)
    fetch_registry "$M"
    c=$(reserve_commit "$M" TSK 900 "$(uuid)" maintainer@proof.invalid "issue: TSK-900")
    push_classify "$M" "$c"
    echo "an addition under ids/ with the rule active: $CLASS"
    run gh api -X DELETE "repos/$REPO/rulesets/$id"
  fi
}

step_pr_edit() { # probe of R-109's "a PR cannot select the code that checks it"
  say "run at $(now) against $REPO"
  local C branch=task/TSK-010-workflow-edit since wf=".github/workflows/$WORKFLOW"
  C=$(clone contrib contributor@proof.invalid)
  run git -C "$C" fetch -q origin main
  if git -C "$C" ls-remote --exit-code origin "refs/heads/$branch" > /dev/null; then
    run git -C "$C" fetch -q origin "$branch"
    run git -C "$C" switch -q -C "$branch" "origin/$branch"
    run git -C "$C" commit -q --allow-empty -m "chore: rerun the edited workflow"
  else
    run git -C "$C" switch -q -C "$branch" origin/main
    python3 - "$C/$wf" <<'PY'
import sys
path = sys.argv[1]
text = open(path).read()
cut = text.index('          python3 "${RUNNER_TEMP}/ids.py" check')
open(path, "w").write(text[:cut] + '          echo "registry check: skipped by the pull request"\n')
PY
    run git -C "$C" diff
    run git -C "$C" commit -q -am "ci: skip the registry check"
  fi
  since=$(since)
  run git -C "$C" push -q origin "$branch"
  gh pr view -R "$REPO" "$branch" --json number,url 2>/dev/null \
    || run gh pr create -R "$REPO" --base main --head "$branch" \
      --title "Probe: a pull request edits the check workflow" \
      --body "Proof probe for TSK-100: does a same-repository pull request choose the workflow that checks it?"
  wait_run pull_request "$branch" "$since"
  echo "the registry is damaged (see the delete step), so an honest check is red"
  assert "a PR that edits the workflow is green: the PR selects its own check (finding)" \
    [ "$(conclusion "$RUN_ID")" = success ]
}

step_evidence() {
  say "run at $(now) against $REPO"
  local M root paths ev id block
  M=$(clone maint maintainer@proof.invalid)
  fetch_registry "$M"
  say "registry branch tree"
  run git -C "$M" ls-tree -r --name-only "$TRACK"
  paths=$(git -C "$M" ls-tree -r --name-only "$TRACK" | grep -vE '^ids/(EPC|SPC|TSK|ADR)/[0-9-]+\.toml$')
  assert "every path on the registry tip is an ids/ file" test -z "$paths"
  say "registry history"
  run git -C "$M" log --format='%h %s' "$TRACK"
  root=$(git -C "$M" rev-list --max-parents=0 "$TRACK")
  say "root commit"
  run git -C "$M" diff-tree -r --root --name-status --no-commit-id "$root"
  assert "the registry has one root commit" [ "$(wc -w <<<"$root")" -eq 1 ]
  paths=$(git -C "$M" diff-tree -r --root --name-status --no-commit-id "$root" \
    | awk -F'\t' '$1 != "A" || $2 !~ /^ids\//')
  assert "the root commit adds only ids/ files" test -z "$paths"

  say "workflow on the code branch (main)"
  gh api "repos/$REPO/contents/.github/workflows/$WORKFLOW?ref=main" --jq .content | base64 -d \
    > "$OUT/workflow-main.yml"
  run grep -nE "^(on|permissions|  pull_request|  push|  schedule|    - cron|      contents)" "$OUT/workflow-main.yml"

  say "runs of each job kind"
  gh run list -R "$REPO" --workflow "$WORKFLOW" --limit 50 \
    --json databaseId,event,headBranch,conclusion,createdAt,url > "$OUT/runs.json"
  jq -r '.[] | [.databaseId, .event, .headBranch, .conclusion, .createdAt] | @tsv' "$OUT/runs.json"
  for ev in push pull_request schedule; do
    # The earliest run of each kind comes from the unedited workflow.
    id=$(jq -r "[.[] | select(.event == \"$ev\" and .conclusion != \"\")] | last | .databaseId // empty" "$OUT/runs.json")
    if [ -z "$id" ]; then
      echo "no completed $ev run yet"
      assert "one completed $ev run" false
      continue
    fi
    gh run view -R "$REPO" "$id" --log > "$OUT/run-$id-$ev-latest.log" 2>&1
    if [ ! -s "$OUT/run-$id-$ev-latest.log" ]; then
      # gh run view --log can come back empty (seen for the schedule run);
      # read each job log from the API, shaped as job, step, line.
      for job in $(gh run view -R "$REPO" "$id" --json jobs --jq '.jobs[].databaseId'); do
        gh api "repos/$REPO/actions/jobs/$job/logs" | sed "s/^/$job$TAB$TAB/"
      done > "$OUT/run-$id-$ev-latest.log" 2>&1
    fi
    block=$(awk -F'\t' '/GITHUB_TOKEN Permissions/ {on = 1; next} on && /##\[endgroup\]/ {exit} on {sub(/^[^ ]* /, "", $3); print $3}' \
      "$OUT/run-$id-$ev-latest.log")
    echo "$ev run $id token permissions:"
    echo "$block"
    assert "$ev run $id has read-only token permissions" \
      test -n "$block" -a -z "$(grep -v ': read$' <<<"$block")"
  done
}

case $STEP in
  setup) step_setup ;;
  protect) step_protect ;;
  issue) step_issue ;;
  race) step_race ;;
  lost-ack) step_lost_ack ;;
  fork) step_fork ;;
  delete) step_delete ;;
  pushrule) step_pushrule ;;
  pr-edit) step_pr_edit ;;
  evidence) step_evidence ;;
  *) die "unknown step $STEP" ;;
esac
say "result: $FAILS failed assertions"
[ "$FAILS" -eq 0 ]
