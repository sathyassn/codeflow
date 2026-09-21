#!/bin/sh
# Release qualification of the codeflow CLI against generated sample projects.
#
# Builds the binary from a given commit, records its SHA-256, generates
# greenfield (Rust and Node) and brownfield sample repositories in a disposable
# directory, exercises every subcommand at the minimal, standard and full
# tiers, and writes a result matrix in which every check carries exactly one of
# passed, failed or unavailable with expected versus observed.
#
# The candidate commit and the binary digest are parameters, never constants:
# the same script re-runs against a later integration head unchanged.
#
#   sh tests/release-qualification/qualify.sh --commit <SHA> [options]
#
# Options:
#   --commit <SHA>      Candidate commit to build and qualify (default: HEAD).
#   --repo <DIR>        Repository to build from (default: this script's repo).
#   --binary <PATH>     Use this binary instead of building (digest still recorded).
#   --out <FILE>        Matrix destination (default: the v3.0.0 sibling record).
#   --evidence <FILE>   Release record whose qualification block is refreshed.
#   --work-dir <DIR>    Disposable root (default: a mktemp under $TMPDIR).
#   --target-dir <DIR>  Cargo target directory for the build.
#   --node <DIR>        bin/ directory of the portal's pinned Node (24.18.0).
#   --herdr-workspace   Herdr workspace id for the delegate canary (default: w2).
#   --no-session-reason Why no live session could be started, recorded verbatim.
#   --no-session-owner  Who owns that gate; required with --no-session-reason.
#   --skip-canary       Record the delegate canary unavailable without running it.
#   --keep              Do not delete the work directory (teardown is still reported).

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
DEFAULT_REPO=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)

. "$SCRIPT_DIR/lib.sh"

COMMIT=""
REPO="$DEFAULT_REPO"
BINARY=""
OUT=""
EVIDENCE=""
WORK=""
TARGET_DIR=""
NODE_BIN=""
HERDR_WORKSPACE="w2"
NO_SESSION_REASON=""
NO_SESSION_OWNER=""
SKIP_CANARY=0
KEEP=0

while [ $# -gt 0 ]; do
  case $1 in
    --commit) COMMIT=$2; shift 2 ;;
    --repo) REPO=$2; shift 2 ;;
    --binary) BINARY=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --evidence) EVIDENCE=$2; shift 2 ;;
    --work-dir) WORK=$2; shift 2 ;;
    --target-dir) TARGET_DIR=$2; shift 2 ;;
    --node) NODE_BIN=$2; shift 2 ;;
    --herdr-workspace) HERDR_WORKSPACE=$2; shift 2 ;;
    --no-session-reason) NO_SESSION_REASON=$2; shift 2 ;;
    --no-session-owner) NO_SESSION_OWNER=$2; shift 2 ;;
    --skip-canary) SKIP_CANARY=1; shift ;;
    --keep) KEEP=1; shift ;;
    -h | --help) sed -n '2,30p' "$0"; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; exit 64 ;;
  esac
done

REPO=$(CDPATH= cd -- "$REPO" && pwd)
[ -n "$COMMIT" ] || COMMIT=$(git -C "$REPO" rev-parse HEAD)
COMMIT=$(git -C "$REPO" rev-parse "$COMMIT")
[ -n "$OUT" ] || OUT="$REPO/docs/verification/releases/v3.0.0-cli-qualification.md"
[ -n "$EVIDENCE" ] || EVIDENCE="$REPO/docs/verification/releases/v3.0.0.md"
[ -n "$WORK" ] || WORK=$(mktemp -d "${TMPDIR:-/tmp}/codeflow-qualification.XXXXXX")
mkdir -p "$WORK"
WORK=$(CDPATH= cd -- "$WORK" && pwd)

RESULTS="$WORK/results.tsv"
TRANSCRIPT="$WORK/transcript.log"
DIFFS="$WORK/diffs"
: >"$RESULTS"
: >"$TRANSCRIPT"
mkdir -p "$DIFFS"
FAILURES=0

# Every sample keeps its user-level codeflow state inside the run, so the
# operator's own registry, index and model bindings are never written.
CODEFLOW_HOME="$WORK/codeflow-home"
export CODEFLOW_HOME
mkdir -p "$CODEFLOW_HOME"

# The presentation surface stores session state under $HOME; PRESENT_HOME
# redirects it into the run so `present` is disposable like everything else.
PRESENT_HOME="$WORK/present-home"
mkdir -p "$PRESENT_HOME"

START_UTC=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
printf 'codeflow qualification\n  repo:    %s\n  commit:  %s\n  work:    %s\n\n' \
  "$REPO" "$COMMIT" "$WORK" >&2

# ---------------------------------------------------------------------------
# Build the candidate binary from the named commit
# ---------------------------------------------------------------------------

BUILD_TREE="$WORK/candidate"
BUILD_TREE_CREATED=0
if [ -z "$BINARY" ]; then
  [ -n "$TARGET_DIR" ] || TARGET_DIR="$WORK/cargo-target"
  mkdir -p "$TARGET_DIR"
  printf 'building %s (this takes a few minutes)\n' "$COMMIT" >&2
  git -C "$REPO" worktree add --detach --quiet "$BUILD_TREE" "$COMMIT"
  BUILD_TREE_CREATED=1
  (
    cd "$BUILD_TREE"
    DEVELOPER_DIR=${DEVELOPER_DIR:-/Library/Developer/CommandLineTools} \
      CARGO_TARGET_DIR="$TARGET_DIR" \
      cargo build -p codeflow-cli >"$WORK/build.log" 2>&1
  ) || { printf 'build failed; see %s\n' "$WORK/build.log" >&2; exit 1; }
  BINARY="$TARGET_DIR/debug/codeflow"
fi
BINARY=$(CDPATH= cd -- "$(dirname -- "$BINARY")" && pwd)/$(basename -- "$BINARY")
[ -x "$BINARY" ] || { printf 'no executable at %s\n' "$BINARY" >&2; exit 1; }

BINARY_SHA=$(shasum -a 256 "$BINARY" | awk '{print $1}')
BINARY_VERSION=$("$BINARY" --version 2>/dev/null | head -1)
printf '  binary:  %s\n  sha256:  %s\n  version: %s\n\n' \
  "$BINARY" "$BINARY_SHA" "$BINARY_VERSION" >&2

# ---------------------------------------------------------------------------
# Teardown
# ---------------------------------------------------------------------------

TEARDOWN_LOG="$WORK/teardown.log"
HERDR_TAB=""
HERDR_PANE=""
HERDR_AGENT=""

teardown() {
  : >"$TEARDOWN_LOG"
  {
    printf 'teardown at %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

    # Presentation sessions and their service processes.
    if [ -d "$PRESENT_HOME" ]; then
      for _s in $(HOME="$PRESENT_HOME" "$BINARY" present list 2>/dev/null |
        python3 -c 'import json,sys
try:
    print(" ".join(s["id"] for s in json.load(sys.stdin)))
except Exception:
    pass' 2>/dev/null); do
        HOME="$PRESENT_HOME" "$BINARY" present close "$_s" 2>&1 || true
        HOME="$PRESENT_HOME" "$BINARY" present clear --older-than 0d "$_s" 2>&1 || true
      done
      printf 'present sessions remaining: %s\n' \
        "$(HOME="$PRESENT_HOME" "$BINARY" present list 2>/dev/null | tr -d '[:space:]')"
    fi

    # The Herdr tab this run created, and only that one.
    if [ -n "$HERDR_AGENT" ]; then
      herdr agent stop "$HERDR_AGENT" 2>&1 || true
    fi
    if [ -n "$HERDR_TAB" ]; then
      herdr tab close "$HERDR_TAB" 2>&1 || true
      printf 'herdr tab closed: %s\n' "$HERDR_TAB"
    fi

    # The build worktree.
    if [ "$BUILD_TREE_CREATED" = 1 ]; then
      git -C "$REPO" worktree remove --force "$BUILD_TREE" 2>&1 || true
      printf 'build worktree removed: %s\n' "$BUILD_TREE"
    fi

    if [ "$KEEP" = 1 ]; then
      printf 'work directory kept by --keep: %s\n' "$WORK"
    else
      printf 'work directory: %s\n' "$WORK"
    fi
  } >>"$TEARDOWN_LOG" 2>&1
  TORN_DOWN=1
  cat "$TEARDOWN_LOG" >&2
}

# An abnormal exit still tears down, and says where the evidence is instead of
# writing a matrix that would describe an incomplete run.
TORN_DOWN=0
on_exit() {
  _exit=$?
  if [ "$TORN_DOWN" = 0 ]; then
    printf '\nqualification stopped early (exit %s); transcript: %s\n' "$_exit" "$TRANSCRIPT" >&2
    teardown
  fi
}
trap on_exit EXIT

# ---------------------------------------------------------------------------
# The per-sample, per-tier qualification
# ---------------------------------------------------------------------------

SAMPLE=""
TIER=""
DIR=""

# Every sample lands its scaffold through the sanctioned path, because init
# arms the protected-branch rule against the very branch it scaffolded on.
land_scaffold() {
  git -C "$DIR" checkout -q -b chore/codeflow-scaffold
  git -C "$DIR" add -A
  git -C "$DIR" commit -q -m "chore: scaffold codeflow $TIER tier"
  cf integrate chore/codeflow-scaffold --into main
}

qualify_init() {
  # Greenfield and brownfield both start from a seeded repository; the
  # brownfield one additionally carries files nothing may touch.
  if [ "$SAMPLE" = brownfield ]; then
    snapshot_files "$DIR" "$WORK/$SAMPLE-$TIER.before" \
      README.md .github/workflows/ci.yml docs-note.md AGENTS.md src/lib.rs Cargo.toml
    cp "$DIR/.git/hooks/pre-commit" "$WORK/$SAMPLE-$TIER.hook-before"
  fi

  cf init "--$TIER" --yes
  record "$SAMPLE" "$TIER" "init --$TIER" "scaffold a $SAMPLE repository" \
    "$(status_for 0)" "exit 0 and a created-file report" "$(observed_exit "$(printf '%s' "$CF_OUT" | tail -2 | tr '\n' ' ')")"

  # Idempotency: the same init again writes nothing new.
  cf init "--$TIER" --yes
  _created=$(printf '%s' "$CF_OUT" | grep -c '  created  ' || true)
  if [ "$CF_STATUS" = 0 ] && [ "$_created" = 0 ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "init --$TIER" "repeat init creates nothing" \
    "$_s" "exit 0 and zero created files" "exit $CF_STATUS and $_created created"

  if [ "$SAMPLE" = brownfield ]; then
    snapshot_files "$DIR" "$WORK/$SAMPLE-$TIER.after-init" \
      README.md .github/workflows/ci.yml docs-note.md AGENTS.md src/lib.rs Cargo.toml
    # AGENTS.md is a managed file, so init merges a block into it rather than
    # leaving it byte-identical; the owner instruction must survive.
    sh_run "diff '$WORK/$SAMPLE-$TIER.before' '$WORK/$SAMPLE-$TIER.after-init'"
    printf '%s\n' "$CF_OUT" >"$DIFFS/$SAMPLE-$TIER-init-hashes.diff"
    {
      printf 'before init:\n'
      cat "$WORK/$SAMPLE-$TIER.before"
      printf 'after init:\n'
      cat "$WORK/$SAMPLE-$TIER.after-init"
    } >"$DIFFS/$SAMPLE-$TIER-init-digests.txt"
    _unmanaged_ok=0
    for _p in README.md .github/workflows/ci.yml docs-note.md; do
      if grep -qF "$(awk -v p="$_p" '$2==p{print $1}' "$WORK/$SAMPLE-$TIER.before")  $_p" \
        "$WORK/$SAMPLE-$TIER.after-init"; then
        _unmanaged_ok=$((_unmanaged_ok + 1))
      fi
    done
    if [ "$_unmanaged_ok" = 3 ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
    record "$SAMPLE" "$TIER" "init --$TIER" "pre-existing unmanaged files are byte-identical" \
      "$_s" "README, project CI workflow and release note keep their SHA-256" \
      "$_unmanaged_ok of 3 digests unchanged (hash diff in $SAMPLE-$TIER-init-hashes.diff)"

    if grep -q 'never edit anything under' "$DIR/AGENTS.md"; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
    record "$SAMPLE" "$TIER" "init --$TIER" "owner prose survives the managed AGENTS block" \
      "$_s" "the owner instruction is still present in AGENTS.md" \
      "$(grep -c 'never edit anything under' "$DIR/AGENTS.md" || true) occurrence(s) found"

    if cmp -s "$DIR/.git/hooks/pre-commit" "$WORK/$SAMPLE-$TIER.hook-before"; then
      _s=$RESULT_PASSED
      _hook_same=yes
    else
      _s=$RESULT_FAILED
      _hook_same=no
    fi
    record "$SAMPLE" "$TIER" "init --$TIER" "the project's own .git/hooks are left on disk" \
      "$_s" ".git/hooks/pre-commit unchanged; core.hooksPath repointed, not deleted" \
      "pre-commit identical=$_hook_same; core.hooksPath=$(git -C "$DIR" config core.hooksPath || echo unset)"
  fi
}

qualify_update() {
  land_scaffold
  record "$SAMPLE" "$TIER" "integrate" "land a branch into a protected target" \
    "$(status_for_match 0 'integrated')" \
    "exit 0, fast-forward onto main through the integrate gate token" "$(observed_exit)"
  if printf '%s' "$CF_OUT" | grep -q 'protected: yes'; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "integrate" "flock path reports the protected landing" \
    "$_s" "the report names the serialized protected landing" \
    "$(printf '%s' "$CF_OUT" | grep 'protected:' | head -1 || echo 'no protected line')"

  cf update
  record "$SAMPLE" "$TIER" "update" "refresh managed files" \
    "$(status_for 0)" "exit 0 and a reconciliation report" "$(observed_exit)"

  # Idempotency proved by the working tree, not by the report.
  sh_run "git -C '$DIR' status --porcelain"
  _before_dirty=$CF_OUT
  cf update
  sh_run "git -C '$DIR' status --porcelain"
  printf 'before:\n%s\nafter:\n%s\n' "$_before_dirty" "$CF_OUT" >"$DIFFS/$SAMPLE-$TIER-update-idempotency.diff"
  if [ "$_before_dirty" = "$CF_OUT" ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "update" "a second consecutive update produces no diff" \
    "$_s" "identical git status before and after the second update" \
    "before/after status identical=$([ "$_before_dirty" = "$CF_OUT" ] && echo yes || echo no) (diff in $SAMPLE-$TIER-update-idempotency.diff)"

  # 3-way merge: a deliberate user edit inside a managed file must survive.
  _marker="Qualification edit: this line is owner-authored and must survive update."
  printf '\n%s\n' "$_marker" >>"$DIR/CLAUDE.md"
  cp "$DIR/CLAUDE.md" "$WORK/$SAMPLE-$TIER.claude-before"
  cf update
  _merge_status=$CF_STATUS
  sh_run "diff '$WORK/$SAMPLE-$TIER.claude-before' '$DIR/CLAUDE.md'"
  printf '%s\n' "$CF_OUT" >"$DIFFS/$SAMPLE-$TIER-update-3way.diff"
  if grep -qF "$_marker" "$DIR/CLAUDE.md"; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "update" "3-way merge never clobbers a user edit" \
    "$_s" "the owner-authored line is still in CLAUDE.md after update" \
    "marker present=$(grep -cF "$_marker" "$DIR/CLAUDE.md" || true), update exit $_merge_status (diff in $SAMPLE-$TIER-update-3way.diff)"
  git -C "$DIR" checkout -q -- CLAUDE.md 2>/dev/null || true
  git -C "$DIR" checkout -q main
}

qualify_read_only() {
  cf orient
  record "$SAMPLE" "$TIER" "orient" "session-start digest" \
    "$(status_for_match 0 'gates:')" "exit 0, a digest naming the wired gates" "$(observed_exit)"

  cf status
  record "$SAMPLE" "$TIER" "status" "generated status view" \
    "$(status_for_match 0 'branch:')" "exit 0, branch and work summary" "$(observed_exit)"

  cf status --capabilities
  record "$SAMPLE" "$TIER" "status --capabilities" "capability rollup" \
    "$(status_for 0)" "exit 0" "$(observed_exit)"

  cf policy explain
  record "$SAMPLE" "$TIER" "policy explain" "full key schema from the binary" \
    "$(status_for_match 0 'schema_version')" "exit 0, every policy key with type and default" "$(observed_exit)"

  cf policy show
  record "$SAMPLE" "$TIER" "policy show" "effective values and their source" \
    "$(status_for_match 0 'git.protected_branches')" "exit 0, effective values with sources" "$(observed_exit)"

  cf validate
  record "$SAMPLE" "$TIER" "validate" "record frontmatter" \
    "$(status_for 0)" "exit 0" "$(observed_exit)"

  cf validate --docs
  record "$SAMPLE" "$TIER" "validate --docs" "doc-graph referential lint" \
    "$(status_for 0)" "exit 0" "$(observed_exit)"

  cf recall scaffold
  record "$SAMPLE" "$TIER" "recall" "search project memory" \
    "$(status_for 0)" "exit 0 and an index summary" "$(observed_exit)"

  cf remote protect --dry-run
  record "$SAMPLE" "$TIER" "remote protect --dry-run" "intended rules, nothing applied" \
    "$(status_for_match 0 'dry-run')" "exit 0, the intended rules and status dry-run" "$(observed_exit)"

  # ci over a real two-commit range on this sample.
  _base=$(git -C "$DIR" rev-list --max-parents=0 HEAD | tail -1)
  cf ci --base "$_base" --head HEAD
  record "$SAMPLE" "$TIER" "ci" "range and branch check over the seeded history" \
    "$(status_for_match 0 'clean')" "exit 0, the resolved range clean" "$(observed_exit)"
}

qualify_doctor() {
  cf doctor
  record "$SAMPLE" "$TIER" "doctor" "all checks" \
    "$(status_for 0)" "exit 0; warnings allowed, failures are not" "$(observed_exit "$(printf '%s' "$CF_OUT" | awk '{print $1}' | sort | uniq -c | tr '\n' ' ')")"

  for _c in $("$BINARY" doctor --list); do
    cf doctor --check "$_c"
    record "$SAMPLE" "$TIER" "doctor --check $_c" "named health check" \
      "$(status_for 0)" "exit 0 with an ok or warn verdict" \
      "$(observed_exit "$(printf '%s' "$CF_OUT" | head -1 | cut -c1-120)")"
  done
}

qualify_test() {
  cf test setup --list-templates
  record "$SAMPLE" "$TIER" "test setup --list-templates" "embedded templates" \
    "$(status_for_match 0 'minimal.json')" "exit 0 listing the embedded test-config templates" "$(observed_exit)"

  cf test setup
  record "$SAMPLE" "$TIER" "test setup" "configure the gate by detection" \
    "$(status_for 0)" "exit 0 and a written .codeflow/test-config.json" "$(observed_exit)"

  # Auto-detection covers Cargo, vitest, jest, Go and pytest. A project on
  # another runner gets an empty config and is told to pick a template, so the
  # qualification follows that documented path rather than asserting a target
  # detection never claimed.
  _targets=$(python3 -c 'import json,sys
print(len(json.load(open(sys.argv[1]))["targets"]))' "$DIR/.codeflow/test-config.json")
  if [ "$_targets" = 0 ]; then
    cf test setup --template single-target-basic.json --replace
    record "$SAMPLE" "$TIER" "test setup --template" "write an embedded template when detection finds no stack" \
      "$(status_for_match 0 'written to')" \
      "exit 0 and the named template written over the empty config" "$(observed_exit)"
  fi

  cf test --mode quick
  record "$SAMPLE" "$TIER" "test --mode quick" "run the configured gate" \
    "$(status_for_match 0 'test gate: passed')" \
    "exit 0 with the sample's own test target green" "$(observed_exit)"

  cf test --mode full --strict
  record "$SAMPLE" "$TIER" "test --mode full --strict" "the strict gate runs a real target" \
    "$(status_for_match 0 'test gate: passed')" \
    "exit 0 with at least one target executed" "$(observed_exit)"
}

qualify_estimate() {
  # The forecast pins its evidence by repository-relative path and digest, so
  # the sources have to be present at those paths. Tiers that scaffold them
  # use their own copies; tiers that do not get the fixture tree, which is the
  # same content, so the check always runs against real pinned evidence.
  # The sources come from the repository's own managed skill rather than a
  # copy kept here: the forecast pins them by digest, and a fourth copy of a
  # managed asset is exactly the drift this project's mirror test forbids.
  _sources="$REPO/assets/base/agents/skills/cf-estimate"
  _added=0
  if [ ! -f "$DIR/.agents/skills/cf-estimate/examples/brief.md" ]; then
    mkdir -p "$DIR/.agents/skills/cf-estimate"
    (cd "$_sources" && tar cf - examples references) |
      (cd "$DIR/.agents/skills/cf-estimate" && tar xf -)
    _added=1
  fi
  _forecast="$DIR/.agents/skills/cf-estimate/examples/forecast.json"
  [ -f "$_forecast" ] || _forecast="$_sources/examples/forecast.json"

  cf estimate check "$_forecast"
  record "$SAMPLE" "$TIER" "estimate check" "explicit allocations and pinned evidence" \
    "$(status_for_match 0 'valid supplied allocation')" \
    "exit 0, the allocation accepted with every pinned source read" "$(observed_exit)"

  if [ "$_added" = 1 ]; then
    rm -rf "$DIR/.agents"
  fi
}

qualify_claude_hooks() {
  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"git status --short"}}' hook git-guard
  record "$SAMPLE" "$TIER" "hook git-guard" "positive: a read-only git command passes" \
    "$(status_for 0)" "exit 0, no output" "$(observed_exit "no violation")"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"}}' hook git-guard
  record "$SAMPLE" "$TIER" "hook git-guard" "rejecting: force-push to a protected branch" \
    "$(status_for_match 2 'git.force_push_protected')" \
    "exit 2 naming policy rule git.force_push_protected" "$(observed_exit)"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}' hook exec-guard
  record "$SAMPLE" "$TIER" "hook exec-guard" "positive: an ordinary command passes" \
    "$(status_for 0)" "exit 0, no output" "$(observed_exit "no violation")"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"sudo rm -rf /"}}' hook exec-guard
  record "$SAMPLE" "$TIER" "hook exec-guard" "rejecting: a dangerous command" \
    "$(status_for_match 2 'security.dangerous_commands')" \
    "exit 2 naming policy rule security.dangerous_commands" "$(observed_exit)"

  cf_stdin '{"hook_event_name":"SessionStart","source":"startup"}' hook session-orient
  record "$SAMPLE" "$TIER" "hook session-orient" "emit the orient digest" \
    "$(status_for 0)" "exit 0 and the digest on stdout" "$(observed_exit "$(printf '%s' "$CF_OUT" | head -1)")"

  cf_stdin '{"session_id":"qualification-run","reason":"clear","hook_event_name":"SessionEnd"}' hook session-summary
  record "$SAMPLE" "$TIER" "hook session-summary" "append the session record to the ledger" \
    "$(status_for 0)" "exit 0 and a ledger entry written" "$(observed_exit "ledger files: $(find "$DIR/.codeflow" -name '*.jsonl' 2>/dev/null | wc -l | tr -d ' ')")"
}

qualify_git_hooks() {
  git -C "$DIR" checkout -q -B feat/qualification-hooks main

  printf 'feat: add a qualification sample target\n' >"$WORK/msg-ok.txt"
  cf git-hook commit-msg "$WORK/msg-ok.txt"
  record "$SAMPLE" "$TIER" "git-hook commit-msg" "positive: a conforming subject" \
    "$(status_for 0)" "exit 0, no violation" "$(observed_exit "no violation")"

  printf 'Added some stuff\n' >"$WORK/msg-format.txt"
  cf git-hook commit-msg "$WORK/msg-format.txt"
  record "$SAMPLE" "$TIER" "git-hook commit-msg" "rejecting: commit format" \
    "$(status_for_match 1 'git.commit_format')" \
    "exit 1 naming policy rule git.commit_format" "$(observed_exit)"

  printf 'feat: add a qualification sample target\n\nCo-Authored-By: Claude <noreply@anthropic.com>\n' >"$WORK/msg-attr.txt"
  cf git-hook commit-msg "$WORK/msg-attr.txt"
  record "$SAMPLE" "$TIER" "git-hook commit-msg" "rejecting: AI attribution" \
    "$(status_for_match 1 'git.ai_attribution')" \
    "exit 1 naming policy rule git.ai_attribution" "$(observed_exit)"

  printf 'export const qualification = 1;\n' >"$DIR/qualification-clean.js"
  git -C "$DIR" add qualification-clean.js
  cf git-hook pre-commit
  record "$SAMPLE" "$TIER" "git-hook pre-commit" "positive: clean staged content" \
    "$(status_for 0)" "exit 0, no violation" "$(observed_exit "no violation")"
  git -C "$DIR" reset -q
  rm -f "$DIR/qualification-clean.js"

  # The literal is assembled so the harness itself carries no scannable key.
  printf 'const key = "%s%s";\n' 'AKIA' 'IOSFODNN7EXAMPLE' >"$DIR/qualification-leak.js"
  git -C "$DIR" add qualification-leak.js
  cf git-hook pre-commit
  record "$SAMPLE" "$TIER" "git-hook pre-commit" "rejecting: a staged secret" \
    "$(status_for_match 1 'git.secret_scan')" \
    "exit 1 naming policy rule git.secret_scan" "$(observed_exit)"
  git -C "$DIR" reset -q
  rm -f "$DIR/qualification-leak.js"

  # Protected branch, through the real client paths rather than synthetic ones.
  # A commit is refused at the pre-commit stage; a direct ref move is refused
  # by the reference-transaction stage, which no `--no-verify` can reach.
  _probe=qualification-protected-probe.txt
  git -C "$DIR" checkout -q main
  printf 'Qualification probe.\n' >"$DIR/$_probe"
  git -C "$DIR" add "$_probe"
  sh_run "git -C '$DIR' commit -m 'chore: probe the protected branch rule'"
  record "$SAMPLE" "$TIER" "git-hook pre-commit" "rejecting: a commit on protected main" \
    "$(status_for_match 1 'git.commit_to_protected')" \
    "exit 1 naming policy rule git.commit_to_protected" "$(observed_exit)"
  git -C "$DIR" restore --staged "$_probe"
  rm -f "$DIR/$_probe"

  sh_run "git -C '$DIR' update-ref refs/heads/main HEAD"
  record "$SAMPLE" "$TIER" "git-hook reference-transaction" "rejecting: a direct ref move on protected main" \
    "$(status_for_match 128 'git.local_ref_protection')" \
    "exit 128 naming policy rule git.local_ref_protection" "$(observed_exit)"

  git -C "$DIR" checkout -q -B feat/qualification-hooks main
  printf 'Qualification probe.\n' >"$DIR/$_probe"
  git -C "$DIR" add "$_probe"
  sh_run "git -C '$DIR' commit -m 'chore: probe the unprotected branch path'"
  record "$SAMPLE" "$TIER" "git-hook reference-transaction" "positive: the same commit on a feature branch" \
    "$(status_for 0)" "exit 0, the commit lands" "$(observed_exit "$(git -C "$DIR" log --oneline -1)")"

  cf git-hook pre-push origin "file://$DIR"
  record "$SAMPLE" "$TIER" "git-hook pre-push" "positive: pushing a feature branch" \
    "$(status_for 0)" "exit 0 on a feature branch with no protected target" "$(observed_exit "no violation")"

  cf git-hook pre-merge-commit
  record "$SAMPLE" "$TIER" "git-hook pre-merge-commit" "positive: a merge outside a protected branch" \
    "$(status_for 0)" "exit 0 on a feature branch" "$(observed_exit "no violation")"

  git -C "$DIR" checkout -q main
  cf git-hook pre-merge-commit
  record "$SAMPLE" "$TIER" "git-hook pre-merge-commit" "rejecting: a merge commit on protected main" \
    "$(status_for_match 1 'git.merge_to_protected')" \
    "exit 1 naming policy rule git.merge_to_protected" "$(observed_exit)"

  git -C "$DIR" branch -q -D feat/qualification-hooks 2>/dev/null || true
}

qualify_records() {
  # The record lifecycle exists only at the full tier.
  git -C "$DIR" checkout -q -B chore/qualification-records main

  cf epic new "sample epic for qualification"
  record "$SAMPLE" "$TIER" "epic new" "allocate and scaffold an epic" \
    "$(status_for_match 0 'EPC-')" "exit 0 and an allocated EPC-NNN record" "$(observed_exit)"

  cf task new --epic EPC-001 --into main "sample task for qualification"
  record "$SAMPLE" "$TIER" "task new" "allocate an epic-linked task" \
    "$(status_for_match 0 'TSK-')" "exit 0 and an allocated TSK-NNN record" "$(observed_exit)"

  cf spec new --for EPC-001 "sample spec for qualification"
  record "$SAMPLE" "$TIER" "spec new" "allocate a spec and link its consumer" \
    "$(status_for_match 0 'SPC-')" "exit 0 and an allocated SPC-NNN record" "$(observed_exit)"

  cf validate --docs
  record "$SAMPLE" "$TIER" "validate --docs" "the freshly allocated graph is clean" \
    "$(status_for_match 0 'doc graph clean')" "exit 0, doc graph clean" "$(observed_exit)"

  # The refusal: the planning record is not yet at the merge base.
  git -C "$DIR" checkout -q -B task/TSK-001-sample-task
  cf work start TSK-001 --into main
  record "$SAMPLE" "$TIER" "work start" "rejecting: planning record not anchored" \
    "$(status_for_match 1 'not present at the merge-base')" \
    "exit 1 because TSK-001 is not at the merge base" "$(observed_exit)"

  # Land the planning records, with the spec approved, then start again.
  git -C "$DIR" checkout -q chore/qualification-records
  sed -e 's/^status: draft/status: approved/' "$DIR/project-management/specs/SPC-001.md" >"$WORK/spc.tmp"
  mv "$WORK/spc.tmp" "$DIR/project-management/specs/SPC-001.md"
  git -C "$DIR" add -A
  git -C "$DIR" commit -q -m "docs: plan the qualification sample work"
  cf integrate chore/qualification-records --into main

  git -C "$DIR" checkout -q main
  git -C "$DIR" checkout -q -B task/TSK-001-sample-task
  cf work start TSK-001 --into main
  record "$SAMPLE" "$TIER" "work start" "positive: anchored task on its task branch" \
    "$(status_for_match 0 'anchored at')" \
    "exit 0 naming the anchor commit, the epic and the spec" "$(observed_exit)"

  git -C "$DIR" checkout -q main
}

qualify_portal() {
  cf portal setup --path docs-portal
  record "$SAMPLE" "$TIER" "portal setup" "adopt the documentation portal starter" \
    "$(status_for 0)" "exit 0 and a written portal workspace" "$(observed_exit "$(printf '%s' "$CF_OUT" | tail -1)")"
}

# The portal build and its evidence validation run once: they install a real
# dependency tree and render the site, which the per-tier sweep repeats nothing of.
qualify_portal_build() {
  _portal="$DIR/docs-portal"
  if [ ! -d "$_portal" ]; then
    record "$SAMPLE" "$TIER" "portal build" "render the portal from repository sources" \
      "$RESULT_UNAVAILABLE" "a built portal and a generated evidence manifest" \
      "no portal workspace at $_portal" "TSK-042 portal composition gate"
    return
  fi
  # The build reads a committed snapshot, so the workspace is landed first.
  git -C "$DIR" checkout -q -B chore/qualification-portal main
  git -C "$DIR" add -A
  if ! git -C "$DIR" diff --cached --quiet; then
    git -C "$DIR" commit -q -m "chore: adopt the documentation portal starter"
    cf integrate chore/qualification-portal --into main
  fi

  # The portal pins its own Node; --node puts that one first for these two
  # commands only, which is why PATH is exported rather than quoted inline.
  _saved_path=$PATH
  if [ -n "$NODE_BIN" ]; then
    PATH="$NODE_BIN:$PATH"
    export PATH
  fi
  sh_run "cd '$_portal' && npm run deps:install"
  _deps_status=$CF_STATUS
  if [ "$_deps_status" != 0 ]; then
    PATH=$_saved_path
    export PATH
    record "$SAMPLE" "$TIER" "portal build" "render the portal from repository sources" \
      "$RESULT_UNAVAILABLE" "exit 0 and a generated evidence manifest" \
      "dependency install failed (exit $_deps_status): $(oneline "$(printf '%s' "$CF_OUT" | tail -3)")" \
      "hosted publication and network-dependent install, TSK-010"
    record "$SAMPLE" "$TIER" "validate --portal" "verify the portal evidence manifest" \
      "$RESULT_UNAVAILABLE" "exit 0 on a manifest matching the built artifacts" \
      "no build to validate: dependency install failed" \
      "hosted publication and network-dependent install, TSK-010"
    return
  fi

  sh_run "cd '$_portal' && npm run build"
  record "$SAMPLE" "$TIER" "portal build" "render the portal from repository sources" \
    "$(status_for_match 0 'built artifact')" \
    "exit 0 and a recorded artifact count" "$(observed_exit "$(printf '%s' "$CF_OUT" | tail -2 | tr '\n' ' ')")"
  PATH=$_saved_path
  export PATH

  cf validate --portal docs-portal
  record "$SAMPLE" "$TIER" "validate --portal" "verify the portal evidence manifest" \
    "$(status_for 0)" "exit 0 with the manifest verified without executing project code" "$(observed_exit)"
}

# The presentation surface, once. `open --no-launch` starts the service without
# a browser; a real reviewer comment is the only source of a feedback envelope,
# so the positive resolve path stays with its own owner.
qualify_present() {
  _doc="$DIR/.claude/skills/cf-present/resources/present-document.example.json"
  if [ ! -f "$_doc" ]; then
    _doc="$REPO/assets/base/agents/skills/cf-present/resources/present-document.example.json"
  fi

  present_run() {
    CF_OUT=$(HOME="$PRESENT_HOME" "$BINARY" present "$@" 2>&1 </dev/null) && CF_STATUS=0 || CF_STATUS=$?
    printf '\n$ codeflow present %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
  }

  present_run open "$_doc" --no-launch
  record "$SAMPLE" "$TIER" "present open" "start a validated review session" \
    "$(status_for_match 0 'ready')" \
    "exit 0 and a session ready on an owner-private bootstrap" "$(observed_exit)"

  _sid=$(HOME="$PRESENT_HOME" "$BINARY" present list 2>/dev/null |
    python3 -c 'import json,sys
sessions = json.load(sys.stdin)
print(sessions[0]["id"] if sessions else "")')
  if [ -z "$_sid" ]; then
    record "$SAMPLE" "$TIER" "present feedback" "deliver pending review envelopes" \
      "$RESULT_FAILED" "exit 0 and a JSON-lines envelope stream" "no session to query"
    return
  fi

  present_run feedback "$_sid"
  record "$SAMPLE" "$TIER" "present feedback" "deliver pending review envelopes" \
    "$(status_for 0)" "exit 0; an empty stream when no reviewer has commented" \
    "exit $CF_STATUS, $(printf '%s' "$CF_OUT" | grep -c . || true) envelope line(s)"

  present_run history "$_sid"
  record "$SAMPLE" "$TIER" "present history" "append-only feedback history as JSON" \
    "$(status_for_match 0 'schema_version')" "exit 0 and the session's history document" "$(observed_exit "$(printf '%s' "$CF_OUT" | head -1)")"

  present_run resolve "$_sid" 00000000-0000-0000-0000-000000000000 --event-version 1 --status addressed
  record "$SAMPLE" "$TIER" "present resolve" "rejecting: an event outside the session fails closed" \
    "$(status_for_match 2 'does not belong to session')" \
    "non-zero exit refusing a cross-session transition" "$(observed_exit)"

  record "$SAMPLE" "$TIER" "present resolve" "positive: resolve a real reviewer envelope" \
    "$RESULT_UNAVAILABLE" "a delivered envelope marked addressed at its current version" \
    "no envelope exists: the Comment surface is browser-only and this run launched no browser" \
    "TSK-007 presentation qualification (isolated real-browser journey)"

  present_run close "$_sid"
  record "$SAMPLE" "$TIER" "present close" "close the session and its service" \
    "$(status_for 0)" "exit 0 and the session marked closed" "$(observed_exit)"
}

# ---------------------------------------------------------------------------
# Delegate canary: a real Claude session in a Herdr tab this run creates.
# ---------------------------------------------------------------------------

qualify_delegate() {
  _state="$WORK/delegate-state"
  _run="cfqual$(date -u '+%H%M%S')"
  _turn="turn1"

  cf delegate init --run-id "$_run" --state-dir "$_state"
  record "$SAMPLE" "$TIER" "delegate init" "owner-only run directory and task hook settings" \
    "$(status_for_match 0 'settings.json')" \
    "exit 0 printing the generated settings path" "$(observed_exit)"

  # arm takes the exact bytes the host will deliver: canonical UTF-8, internal
  # LF only, and no terminal line break, so armed and delivered can never differ.
  printf 'Reply with exactly: qualification canary acknowledged. Do not edit any file.' \
    >"$WORK/delegate-prompt.txt"
  printf 'Reply with exactly: qualification canary acknowledged.\n' \
    >"$WORK/delegate-prompt-trailing-lf.txt"

  cf delegate arm --run-id "$_run" --state-dir "$_state" --turn-id reject1 \
    --prompt-file "$WORK/delegate-prompt-trailing-lf.txt"
  record "$SAMPLE" "$TIER" "delegate arm" "rejecting: a prompt with a terminal line break" \
    "$(status_for_match 1 'no terminal line break')" \
    "exit 1 refusing prompt bytes that are not canonical" "$(observed_exit)"

  cf delegate arm --run-id "$_run" --state-dir "$_state" --turn-id "$_turn" \
    --prompt-file "$WORK/delegate-prompt.txt"
  record "$SAMPLE" "$TIER" "delegate arm" "positive: arm one prompt for a live turn" \
    "$(status_for 0)" "exit 0 with the exact prompt bytes armed" \
    "$(observed_exit "armed turn $_turn; request.json $([ -f "$_state/turns/$_turn/request.json" ] && echo written || echo missing)")"

  # Two documented wait exits need no session at all.
  cf delegate wait --run-id "$_run" --state-dir "$_state" --until ready --timeout-seconds 3
  record "$SAMPLE" "$TIER" "delegate wait" "the timeout exit with no session running" \
    "$(status_for_match 124 'timed out')" \
    "exit 124 when the awaited state is never observed" "$(observed_exit)"

  cf delegate wait --run-id "${_run}x" --state-dir "$_state" --until ready --timeout-seconds 3
  record "$SAMPLE" "$TIER" "delegate wait" "rejecting: a run id the state directory does not match" \
    "$(status_for_match 11 'do not match')" \
    "exit 11 on a run that is invalid for this state directory" "$(observed_exit)"

  # The three live transitions need a real session in a tab this run creates.
  canary_unavailable() {
    record "$SAMPLE" "$TIER" "delegate wait --until ready" "harness startup observed in a live session" \
      "$RESULT_UNAVAILABLE" "exit 0 once the session's SessionStart hook records ready" "$1" "$2"
    record "$SAMPLE" "$TIER" "delegate wait --until accepted" "the armed prompt is accepted by the live session" \
      "$RESULT_UNAVAILABLE" "exit 0 once the session accepts the armed prompt" "$1" "$2"
    record "$SAMPLE" "$TIER" "delegate wait --until terminal" "the accepted turn reaches a terminal state" \
      "$RESULT_UNAVAILABLE" "exit 0 once the turn completes or fails" "$1" "$2"
    pipeline_unavailable "$1" "$2"
  }

  if [ -n "$NO_SESSION_REASON" ]; then
    canary_unavailable "$NO_SESSION_REASON" "${NO_SESSION_OWNER:-operator environment}"
    return
  fi

  if [ "$SKIP_CANARY" = 1 ]; then
    canary_unavailable "--skip-canary was passed; no session was started" "this harness invocation"
    return
  fi

  if ! command -v herdr >/dev/null 2>&1; then
    canary_unavailable "herdr is not on PATH: 'command -v herdr' found nothing" "operator environment"
    return
  fi

  _created=$(herdr tab create --workspace "$HERDR_WORKSPACE" \
    --label "cf/codeflow/tsk044/canary" --cwd "$DIR" --no-focus 2>&1) || {
    canary_unavailable \
      "herdr tab create --workspace $HERDR_WORKSPACE --label cf/codeflow/tsk044/canary --cwd $DIR --no-focus failed: $(oneline "$_created")" \
      "operator environment"
    return
  }
  printf '%s\n' "$_created" >>"$TRANSCRIPT"
  HERDR_PANE=$(printf '%s' "$_created" | python3 -c 'import json,sys
print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
  HERDR_TAB=$(printf '%s' "$_created" | python3 -c 'import json,sys
print(json.load(sys.stdin)["result"]["root_pane"]["tab_id"])')

  # The tracked-Claude environment is set in the pane itself, then the pane has
  # to be back at its prompt: `agent start` refuses a pane that is still busy,
  # and that refusal would otherwise be recorded instead of the real blocker.
  herdr pane run "$HERDR_PANE" \
    'export CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1; echo TRACKED=$CLAUDE_CODE_DISABLE_BACKGROUND_TASKS' >>"$TRANSCRIPT" 2>&1 || true
  herdr pane wait-output "$HERDR_PANE" --match "TRACKED=1" --timeout 15000 >>"$TRANSCRIPT" 2>&1 || true
  sleep 2

  HERDR_AGENT="cf-codeflow-tsk044-cl01"
  _start_cmd="herdr agent start $HERDR_AGENT --kind claude --pane $HERDR_PANE -- --permission-mode bypassPermissions --settings $_state/settings.json"
  if ! _start_out=$(herdr agent start "$HERDR_AGENT" --kind claude --pane "$HERDR_PANE" -- \
    --permission-mode bypassPermissions --settings "$_state/settings.json" 2>&1); then
    printf '\n$ %s\n%s\n' "$_start_cmd" "$_start_out" >>"$TRANSCRIPT"
    # Say what actually blocked the session rather than that something failed.
    # A fresh scaffolded sample carries its own .claude/settings.json, and the
    # harness asks a human to trust that workspace before it takes a prompt;
    # the pane needs a moment to paint that prompt before it can be read.
    sleep 8
    _pane_text=$(herdr pane read "$HERDR_PANE" --source recent --lines 120 2>/dev/null || true)
    printf 'pane after failed agent start:\n%s\n' "$_pane_text" >>"$TRANSCRIPT"
    HERDR_AGENT=""
    if printf '%s' "$_pane_text" | grep -q 'Is this a project you created or one you trust'; then
      _why="the Claude Code workspace-trust prompt blocked startup in the tab this run created: the sample carries the scaffolded .claude/settings.json, and the session asks a human to trust the folder before it accepts any prompt. Command: $_start_cmd. Reply: $(oneline "$_start_out")"
      _who="a human operator, who alone may answer the workspace-trust prompt"
    else
      _why="$_start_cmd failed; reply: $(oneline "$_start_out"); pane text is in the transcript"
      _who="operator environment"
    fi
    canary_unavailable "$_why" "$_who"
    return
  fi
  printf '\n$ %s\n%s\n' "$_start_cmd" "$_start_out" >>"$TRANSCRIPT"

  cf delegate wait --run-id "$_run" --state-dir "$_state" --until ready --timeout-seconds 180
  _ready_status=$CF_STATUS
  record "$SAMPLE" "$TIER" "delegate wait --until ready" \
    "harness startup observed in Herdr tab $HERDR_TAB" \
    "$(status_for 0)" "exit 0 once the session's SessionStart hook records ready" "$(observed_exit)"

  if [ "$_ready_status" != 0 ]; then
    record "$SAMPLE" "$TIER" "delegate wait --until accepted" \
      "the armed prompt is accepted by the live session" \
      "$RESULT_UNAVAILABLE" "exit 0 once the session accepts the armed prompt" \
      "the session never reported ready, so no prompt was delivered" "operator environment"
    record "$SAMPLE" "$TIER" "delegate wait --until terminal" \
      "the accepted turn reaches a terminal state" \
      "$RESULT_UNAVAILABLE" "exit 0 once the turn completes or fails" \
      "no prompt was accepted" "operator environment"
    pipeline_unavailable "the canary session never reported ready" "operator environment"
    return
  fi

  deliver_turn "$HERDR_PANE" "$WORK/delegate-prompt.txt"

  cf delegate wait --run-id "$_run" --state-dir "$_state" --until accepted \
    --turn-id "$_turn" --timeout-seconds 180
  record "$SAMPLE" "$TIER" "delegate wait --until accepted" \
    "the armed prompt is accepted by the live session" \
    "$(status_for 0)" "exit 0 once the session accepts the armed prompt" "$(observed_exit)"

  cf delegate wait --run-id "$_run" --state-dir "$_state" --until terminal \
    --turn-id "$_turn" --timeout-seconds 900
  _turn1_terminal=$CF_STATUS
  record "$SAMPLE" "$TIER" "delegate wait --until terminal" \
    "the accepted turn reaches a terminal state" \
    "$(status_for 0)" "exit 0 once the turn completes or fails" "$(observed_exit)"

  printf 'delegate lifecycle files:\n' >>"$TRANSCRIPT"
  find "$_state" -type f >>"$TRANSCRIPT" 2>&1 || true

  # Second armed turn: the scaffolded pipeline, run end to end by that session.
  if [ "$_turn1_terminal" != 0 ]; then
    pipeline_unavailable "the canary's first turn never reached a terminal state" "operator environment"
    return
  fi
  if [ ! -f "$DIR/.claude/workflows/pipeline.workflow.js" ]; then
    pipeline_unavailable \
      "the $TIER tier scaffolds no .claude/workflows/pipeline.workflow.js" \
      "scaffold tier contract, TSK-041"
    return
  fi

  _turn2=turn2
  cat >"$WORK/pipeline-prompt.raw" <<PROMPT
Run the pipeline workflow that this repository already has at
.claude/workflows/pipeline.workflow.js, end to end, with the Workflow tool.
Arguments: task "add a subtract function beside add, with a unit test", criteria
["subtract(4, 1) returns 3", "the existing add test still passes"], stages
["build", "verify"]. Work on the branch feat/pipeline-canary; never commit to
main and never push. When the workflow ends, write one line to
$PIPELINE_RESULT in the repository root, exactly:
pipeline terminal state: <the workflow's terminal state>
If the workflow cannot run at all, write instead:
pipeline terminal state: unavailable - <the exact reason>
Then stop.
PROMPT
  printf '%s' "$(cat "$WORK/pipeline-prompt.raw")" >"$WORK/pipeline-prompt.txt"
  cf delegate arm --run-id "$_run" --state-dir "$_state" --turn-id "$_turn2" \
    --prompt-file "$WORK/pipeline-prompt.txt"
  _arm2=$CF_STATUS
  if [ "$_arm2" != 0 ]; then
    pipeline_unavailable "arming the pipeline turn failed: $(oneline "$CF_OUT")" "operator environment"
    return
  fi

  deliver_turn "$HERDR_PANE" "$WORK/pipeline-prompt.txt"
  cf delegate wait --run-id "$_run" --state-dir "$_state" --until accepted \
    --turn-id "$_turn2" --timeout-seconds 180
  cf delegate wait --run-id "$_run" --state-dir "$_state" --until terminal \
    --turn-id "$_turn2" --timeout-seconds 3600
  _pipeline_terminal=$CF_STATUS
  _pipeline_out=$CF_OUT

  if [ -f "$DIR/$PIPELINE_RESULT" ]; then
    _state_line=$(head -1 "$DIR/$PIPELINE_RESULT")
    cp "$DIR/$PIPELINE_RESULT" "$DIFFS/pipeline-result.txt"
  else
    _state_line=""
  fi
  if [ "$_pipeline_terminal" = 0 ] && [ -n "$_state_line" ] &&
    ! printf '%s' "$_state_line" | grep -q 'unavailable'; then
    record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
      "$RESULT_PASSED" "a recorded run output and its terminal state" \
      "turn terminal at exit $_pipeline_terminal; $(oneline "$_state_line")"
  else
    pipeline_unavailable \
      "turn exit $_pipeline_terminal; result line: $(oneline "${_state_line:-none written}"); wait output: $(oneline "$_pipeline_out")" \
      "Claude Code Workflow runtime in the canary session"
  fi
}

# Deliver the exact armed bytes to the live pane, then Enter.
deliver_turn() {
  herdr pane send-text "$1" "$(cat "$2")" >>"$TRANSCRIPT" 2>&1 || true
  herdr pane send-keys "$1" Enter >>"$TRANSCRIPT" 2>&1 || true
}

# ---------------------------------------------------------------------------
# Pipeline workflow. The scaffolded pipeline is a Claude Code Workflow script:
# only a live session can execute it, so it runs as a second armed turn inside
# the canary session rather than being simulated here.
# ---------------------------------------------------------------------------

PIPELINE_RESULT="qualification-pipeline-result.txt"

pipeline_unavailable() {
  record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
    "$RESULT_UNAVAILABLE" "a recorded run output and its terminal state" "$1" "$2"
}

# ---------------------------------------------------------------------------
# Gates this run cannot reach
#
# The run builds one binary and exercises it on one platform, and it makes no
# release. The other platform artifacts and the hosted publication path are
# recorded against the owners that already hold them, so no reader can mistake
# this record for evidence about them.
# ---------------------------------------------------------------------------

qualify_boundary() {
  SAMPLE="release boundary"
  TIER="host"
  _host="$(uname -s) $(uname -m)"
  for _target in "Linux x86-64" "Linux arm64" "Windows x86-64" "WSL2"; do
    record "$SAMPLE" "$TIER" "every subcommand" "the same matrix on $_target" \
      "$RESULT_UNAVAILABLE" \
      "the release artifact for $_target qualified by a native run" \
      "this run built and exercised one binary, on $_host only" \
      "the native and cross-target boundary in v3.0.0.md, held by TSK-013 and TSK-010"
  done
  record "$SAMPLE" "$TIER" "release artifacts" "tag, installers and their public canaries" \
    "$RESULT_UNAVAILABLE" \
    "the published tag, uploaded artifacts and both installers verified over the public network" \
    "this run publishes nothing: it builds a debug binary from a commit and touches no tag, artifact or remote" \
    "hosted publication, held by TSK-010 and the release checklist owned by TSK-040"
}

# ---------------------------------------------------------------------------
# Driver
# ---------------------------------------------------------------------------

run_sample_tier() {
  SAMPLE=$1
  TIER=$2
  DIR="$WORK/$SAMPLE-$TIER"
  rm -rf "$DIR"
  mkdir -p "$DIR"
  printf '\n=== %s / %s ===\n' "$SAMPLE" "$TIER" >&2
  printf '\n\n================ %s / %s ================\n' "$SAMPLE" "$TIER" >>"$TRANSCRIPT"

  case $SAMPLE in
    greenfield-rust) build_greenfield_rust "$DIR" ;;
    greenfield-node) build_greenfield_node "$DIR" ;;
    brownfield) build_brownfield "$DIR" ;;
  esac

  cd "$DIR"
  qualify_init
  qualify_update
  qualify_read_only
  qualify_doctor
  qualify_test
  qualify_estimate
  qualify_claude_hooks
  qualify_git_hooks
  qualify_portal
  if [ "$TIER" = full ]; then
    qualify_records
  fi
  cd "$WORK"
}

for _sample in greenfield-rust greenfield-node brownfield; do
  for _tier in minimal standard full; do
    run_sample_tier "$_sample" "$_tier"
  done
done

# The once-only lanes reuse samples the sweep already qualified.
SAMPLE=greenfield-node
TIER=full
DIR="$WORK/$SAMPLE-$TIER"
cd "$DIR"
qualify_portal_build

SAMPLE=greenfield-rust
TIER=full
DIR="$WORK/$SAMPLE-$TIER"
cd "$DIR"
qualify_present
qualify_delegate
cd "$WORK"

qualify_boundary

END_UTC=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
teardown

# ---------------------------------------------------------------------------
# Render
# ---------------------------------------------------------------------------

python3 "$SCRIPT_DIR/render.py" \
  --results "$RESULTS" \
  --out "$OUT" \
  --evidence "$EVIDENCE" \
  --commit "$COMMIT" \
  --binary-sha "$BINARY_SHA" \
  --binary-version "$BINARY_VERSION" \
  --started "$START_UTC" \
  --finished "$END_UTC" \
  --work-dir "$WORK" \
  --teardown "$TEARDOWN_LOG" \
  --diffs "$DIFFS"

if [ "$KEEP" = 0 ]; then
  rm -rf "$WORK"
fi

printf '\nmatrix: %s\n' "$OUT" >&2
if [ "$FAILURES" -gt 0 ]; then
  printf 'qualification blocked: %s failed check(s)\n' "$FAILURES" >&2
  exit 1
fi
printf 'qualification: no failed checks\n' >&2
