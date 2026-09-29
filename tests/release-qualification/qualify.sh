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
# The build always uses its own Cargo target directory inside the work
# directory. A debug binary reads its embedded assets from the checkout it was
# built in, and that checkout is removed at teardown, so building into a
# caller's target would leave the caller a binary that no longer runs.
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
#   --node <DIR>        bin/ directory of the portal's pinned Node (24.18.0).
#   --herdr-workspace   Herdr workspace id for the delegate canary (default: w2).
#   --blocked-on        What a blocked verdict is blocked on, recorded verbatim.
#   --no-session-reason Why no live session could be started, recorded verbatim.
#   --no-session-owner  Who owns that gate; required with --no-session-reason.
#   --skip-canary       Record the delegate canary unavailable without running it.
#   --trust-wait-seconds <N>
#                       How long the operator gets to answer the canary
#                       session's workspace-trust prompt (default: 240; 0
#                       waits not at all).
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
WORK_PARENT_OPT=""
NODE_BIN=""
HERDR_WORKSPACE="w2"
BLOCKED_ON=""
NO_SESSION_REASON=""
NO_SESSION_OWNER=""
SKIP_CANARY=0
TRUST_WAIT_SECONDS=240
KEEP=0

while [ $# -gt 0 ]; do
  case $1 in
    --commit) COMMIT=$2; shift 2 ;;
    --repo) REPO=$2; shift 2 ;;
    --binary) BINARY=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --evidence) EVIDENCE=$2; shift 2 ;;
    --work-dir) WORK_PARENT_OPT=$2; shift 2 ;;
    --target-dir)
      printf -- '--target-dir is not accepted: the build uses its own target under the work directory\n' >&2
      exit 64
      ;;
    --node) NODE_BIN=$2; shift 2 ;;
    --herdr-workspace) HERDR_WORKSPACE=$2; shift 2 ;;
    --blocked-on) BLOCKED_ON=$2; shift 2 ;;
    --no-session-reason) NO_SESSION_REASON=$2; shift 2 ;;
    --no-session-owner) NO_SESSION_OWNER=$2; shift 2 ;;
    --skip-canary) SKIP_CANARY=1; shift ;;
    --trust-wait-seconds) TRUST_WAIT_SECONDS=$2; shift 2 ;;
    --keep) KEEP=1; shift ;;
    -h | --help) sed -n '2,37p' "$0"; exit 0 ;;
    *) printf 'unknown option: %s\n' "$1" >&2; exit 64 ;;
  esac
done

# A wait budget that is not a whole number of seconds would fail much later, in
# the middle of the canary, so it is rejected here.
case $TRUST_WAIT_SECONDS in
  "" | *[!0-9]*)
    printf -- '--trust-wait-seconds needs a whole number of seconds: %s\n' \
      "$TRUST_WAIT_SECONDS" >&2
    exit 64
    ;;
esac

REPO=$(CDPATH= cd -- "$REPO" && pwd)
[ -n "$COMMIT" ] || COMMIT=$(git -C "$REPO" rev-parse HEAD)
COMMIT=$(git -C "$REPO" rev-parse "$COMMIT")
[ -n "$OUT" ] || OUT="$REPO/docs/verification/releases/v3.0.0-cli-qualification.md"
[ -n "$EVIDENCE" ] || EVIDENCE="$REPO/docs/verification/releases/v3.0.0.md"
# ---------------------------------------------------------------------------
# Cleanup state and handler, armed before anything is allocated
#
# Everything the run may have to release is declared and the handler installed
# before the first resource exists, so a failure at any point after this line,
# including during the build or the render, still finalizes. Nothing parsed
# from the command line is reset here.
# ---------------------------------------------------------------------------

WORK=""
WORK_PARENT=""
WORK_OWNED=0
TEARDOWN_LOG=""
HERDR_TAB=""
HERDR_PANE=""
HERDR_AGENT=""
PRESENT_HOME=""
PRESENT_REPO=""
BUILD_TREE=""
BUILD_TREE_CREATED=0
CLEANUP_FAILURES=0
TORN_DOWN=0
WORK_REMOVED=0
# The rendered record carries this line until the work directory's removal has
# actually been verified, after which it is replaced in place.
WORK_REMOVAL_MARK="work directory removal: not yet attempted"

# cleanup_failed <what> - record a resource that could not be released, so the
# record never reports a teardown that did not happen.
cleanup_failed() {
  CLEANUP_FAILURES=$((CLEANUP_FAILURES + 1))
  printf 'CLEANUP FAILURE: %s\n' "$1"
}

# The session ids this run owns, listed from inside the repository that opened
# them, one per line. The exit status is the whole signal, because a flag set
# here would be set in a subshell and never reach the caller. A listing that
# cannot be read is a cleanup failure, not an empty result: silence must never
# be reported as proof that none remain. Whatever the commands write to stderr
# flows into the teardown log, so a failure here can be explained.
present_session_ids() {
  _raw=$( (cd "$PRESENT_REPO" && HOME="$PRESENT_HOME" "$BINARY" present list) ) || return 1
  printf '%s' "$_raw" | python3 -c 'import json,sys
raw = sys.stdin.read()
try:
    sessions = json.loads(raw)
    if not isinstance(sessions, list):
        raise ValueError("not a list")
    for s in sessions:
        print(s["id"])
except Exception as error:
    sys.stderr.write("present list could not be parsed: %s\n" % error)
    sys.exit(3)'
}

teardown() {
  [ "$TORN_DOWN" = 0 ] || return 0
  TORN_DOWN=1
  if [ -n "$TEARDOWN_LOG" ]; then : >"$TEARDOWN_LOG"; fi
  {
    printf 'teardown at %s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"

    # Presentation sessions and their service processes. Session discovery
    # resolves a project from the working directory, so these run inside the
    # sample repository that opened them, not from the run root.
    if [ -n "$PRESENT_HOME" ] && [ -d "$PRESENT_HOME" ] && [ -d "$PRESENT_REPO" ] &&
      [ -n "$BINARY" ] && [ -x "$BINARY" ]; then
      if _ids=$(present_session_ids); then
        for _s in $_ids; do
          (cd "$PRESENT_REPO" && HOME="$PRESENT_HOME" "$BINARY" present close "$_s" 2>&1) || true
          (cd "$PRESENT_REPO" && HOME="$PRESENT_HOME" "$BINARY" present clear --older-than 0d "$_s" 2>&1) || true
        done
        if _second=$(present_session_ids); then
          _left=$(printf '%s' "$_second" | tr '\n' ' ' | sed 's/ *$//')
          if [ -n "$_left" ]; then
            cleanup_failed "presentation session(s) still listed: $_left"
          else
            printf 'presentation sessions: `present list` read cleanly in %s and returned none\n' "$PRESENT_REPO"
          fi
        else
          cleanup_failed "presentation sessions could not be re-listed after closing them"
        fi
      else
        cleanup_failed "presentation sessions could not be listed in $PRESENT_REPO; any still running were not released"
      fi
    else
      printf 'presentation sessions: none opened by this run\n'
    fi

    # The Herdr tab this run created, and only that one.
    if [ -n "$HERDR_AGENT" ] && [ -n "$HERDR_PANE" ]; then
      if stop_pane_agent "$HERDR_PANE"; then
        printf 'herdr agent %s exited: pane %s is back at its shell\n' "$HERDR_AGENT" "$HERDR_PANE"
      else
        printf 'herdr agent %s still running in pane %s; closing its tab ends it\n' \
          "$HERDR_AGENT" "$HERDR_PANE"
      fi
    fi
    if [ -n "$HERDR_TAB" ]; then
      if herdr tab close "$HERDR_TAB" 2>&1; then
        printf 'herdr tab closed: %s\n' "$HERDR_TAB"
      else
        cleanup_failed "herdr tab $HERDR_TAB did not close"
      fi
    else
      printf 'herdr: no tab created by this run\n'
    fi

    # The build worktree.
    if [ "$BUILD_TREE_CREATED" = 1 ]; then
      git -C "$REPO" worktree remove --force "$BUILD_TREE" >/dev/null 2>&1 || true
      if [ -e "$BUILD_TREE" ]; then
        cleanup_failed "build worktree still present: $BUILD_TREE"
      else
        printf 'build worktree removed: %s\n' "$BUILD_TREE"
      fi
    fi

    # The work directory holds the results the record is rendered from, so its
    # removal happens after rendering and its verified outcome replaces the
    # line below. Only the child this run created is ever removed; the parent
    # the caller named is never touched.
    if [ "$KEEP" = 1 ]; then
      printf 'work directory kept by --keep: %s\n' "$WORK"
    elif [ "$WORK_OWNED" = 1 ]; then
      printf '%s\n' "$WORK_REMOVAL_MARK"
    else
      printf 'work directory: never created\n'
    fi
    printf 'parent named by --work-dir, never written to except the run child: %s\n' \
      "${WORK_PARENT:-none resolved}"

    if [ "$CLEANUP_FAILURES" = 0 ]; then
      printf 'resources released so far: every one this run created\n'
    else
      printf 'teardown incomplete: %s resource(s) above were not released\n' "$CLEANUP_FAILURES"
    fi
  } >>"${TEARDOWN_LOG:-/dev/stderr}" 2>&1
  [ -n "$TEARDOWN_LOG" ] && [ -f "$TEARDOWN_LOG" ] && cat "$TEARDOWN_LOG" >&2
  return 0
}

# remove_work - delete only the child this run created, prove it is gone, and
# replace the pending line in the rendered record with the verified outcome.
# It runs after rendering, because the record is rendered out of this directory,
# and again from the exit handler for any path that never reached the render.
remove_work() {
  [ "$WORK_REMOVED" = 0 ] || return 0
  if [ "$KEEP" = 1 ]; then
    WORK_REMOVED=1
    return 0
  fi
  [ "$WORK_OWNED" = 1 ] || { WORK_REMOVED=1; return 0; }
  WORK_REMOVED=1
  rm -rf "$WORK" 2>/dev/null || true
  if [ -d "$WORK" ]; then
    cleanup_failed "work directory still present: $WORK"
    _line="work directory removal FAILED, still present: $WORK"
  else
    _line="work directory removed and verified gone: $WORK"
  fi
  if [ -f "$OUT" ]; then
    python3 -c 'import io,sys
path, mark, line = sys.argv[1], sys.argv[2], sys.argv[3]
text = io.open(path, encoding="utf-8").read()
io.open(path, "w", encoding="utf-8").write(text.replace(mark, line))' \
      "$OUT" "$WORK_REMOVAL_MARK" "$_line" 2>/dev/null || true
  fi
  printf '%s\n' "$_line" >&2
}

# Every exit path finalizes every owned resource: a build failure, a render
# failure, a signal, or an ordinary finish. Both steps are idempotent, so the
# normal path may call them itself and the handler then does nothing.
finalize() {
  teardown
  remove_work
}

on_exit() {
  _exit=$?
  if [ "$TORN_DOWN" = 0 ]; then
    printf '\nqualification stopped early (exit %s); transcript: %s\n' \
      "$_exit" "${TRANSCRIPT:-not yet created}" >&2
  fi
  finalize
}
trap on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# ---------------------------------------------------------------------------
# Allocation, after the handler above is armed
#
# --work-dir names a parent the run may write inside, never a directory the run
# may delete. The run creates one child it alone owns with exclusive mkdir, and
# writes every file it needs inside that child, so no caller-owned file or
# symlink target in the parent is ever read, written or removed.
# ---------------------------------------------------------------------------

WORK_PARENT=${WORK_PARENT_OPT:-${TMPDIR:-/tmp}}
mkdir -p "$WORK_PARENT"
WORK_PARENT=$(CDPATH= cd -- "$WORK_PARENT" && pwd)
WORK="$WORK_PARENT/cfqual-$$-$(date -u '+%Y%m%dT%H%M%SZ')"
mkdir "$WORK" || { printf 'could not create an owned work directory at %s\n' "$WORK" >&2; exit 1; }
WORK_OWNED=1
TEARDOWN_LOG="$WORK/teardown.log"

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

# With --binary the supplied executable is the one hashed and invoked, and no
# Cargo build runs; without it the candidate is built from its own checkout.
BUILD_TREE="$WORK/candidate"
if [ -n "$BINARY" ]; then
  printf 'using the supplied binary; no build will run\n' >&2
elif [ -z "$BINARY" ]; then
  TARGET_DIR="$WORK/cargo-target"
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

# Every subprocess must reach the candidate and nothing else. The installed git
# hook shims resolve `codeflow` through PATH (assets/base/git-hooks/pre-commit),
# so without this the real commit probes would be enforced by whatever codeflow
# happens to be installed on the machine, and the recorded digest would not
# describe the binary that did the enforcing.
BIN_DIR=$(dirname -- "$BINARY")
DECOY_DIR="$WORK/decoy-bin"
DECOY_SENTINEL="$WORK/decoy-was-invoked"
mkdir -p "$DECOY_DIR"
cat >"$DECOY_DIR/codeflow" <<DECOY
#!/bin/sh
# A stand-in for any other codeflow on this machine. It sits behind the
# candidate on PATH, so it runs only if the candidate was not found first.
printf 'decoy invoked: %s\n' "\$*" >>"$DECOY_SENTINEL"
exit 0
DECOY
chmod 0755 "$DECOY_DIR/codeflow"
PATH="$BIN_DIR:$DECOY_DIR:$PATH"
export PATH

# Assets a sample may need come from the candidate's own checkout when the run
# built one, so a different revision supplies its own; with --binary there is no
# checkout and the invoking repository is used, which the record states.
if [ "$BUILD_TREE_CREATED" = 1 ]; then
  ASSET_ROOT="$BUILD_TREE"
  ASSET_ORIGIN="the candidate checkout at $COMMIT"
else
  ASSET_ROOT="$REPO"
  ASSET_ORIGIN="the invoking checkout, because --binary supplied a prebuilt binary"
fi

printf '  binary:  %s\n  sha256:  %s\n  version: %s\n  assets:  %s\n\n' \
  "$BINARY" "$BINARY_SHA" "$BINARY_VERSION" "$ASSET_ORIGIN" >&2


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

# Proof that a subprocess, including an installed git hook shim, reaches the
# candidate. The decoy sits behind it on PATH, so it answers only if the
# candidate does not.
qualify_binding() {
  sh_run "command -v codeflow"
  _resolved=$CF_OUT
  if [ -x "$_resolved" ]; then
    _resolved_sha=$(shasum -a 256 "$_resolved" | awk '{print $1}')
  else
    _resolved_sha=none
  fi
  if [ "$_resolved" = "$BINARY" ] && [ "$_resolved_sha" = "$BINARY_SHA" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "candidate binding" "PATH resolves codeflow to the candidate" \
    "$_s" "command -v codeflow is the built candidate, digest $BINARY_SHA" \
    "resolved $_resolved, digest $_resolved_sha"
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

  # Idempotence is proved by the content of the tree, not by porcelain status:
  # a file that was already dirty can change its bytes without changing its
  # status letter. The update's own exit status is kept separately, so the
  # comparison cannot overwrite it.
  _before_digest=$(tree_digest "$DIR")
  cf update
  _second_status=$CF_STATUS
  _second_out=$CF_OUT
  _after_digest=$(tree_digest "$DIR")
  printf 'digest before second update: %s\ndigest after second update:  %s\nsecond update exit: %s\n' \
    "$_before_digest" "$_after_digest" "$_second_status" \
    >"$DIFFS/$SAMPLE-$TIER-update-idempotency.diff"
  if [ "$_second_status" = 0 ] && [ "$_before_digest" = "$_after_digest" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "update" "a second consecutive update changes no file" \
    "$_s" "exit 0 and an identical content digest over every file in the tree" \
    "exit $_second_status, digest before $_before_digest, after $_after_digest"

  _diff_report="$WORK/$SAMPLE-$TIER-update-report.txt"
  cf update --diff "$_diff_report"
  if [ "$CF_STATUS" = 0 ] && [ -s "$_diff_report" ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "update --diff" "write the report and applied diffs to a file" \
    "$_s" "exit 0 and a non-empty report at the named path" \
    "exit $CF_STATUS, $([ -f "$_diff_report" ] && wc -c <"$_diff_report" | tr -d ' ' || echo 0) bytes written"

  qualify_three_way

  git -C "$DIR" checkout -q main
}

# The genuine three-way merge.
#
# Within one binary the shipped text and the recorded baseline are the same, so
# a user edit alone takes the KeptUserModified path and never reaches the merge.
# To exercise the merge the baseline is aged: it is rewritten as an older
# shipped version, which makes base, local and incoming three distinct inputs.
# The target is a managed (whole-file) artifact, not a managed-region one.
qualify_three_way() {
  _dest=".codeflow/git-hooks/pre-commit"
  _base="$DIR/.codeflow/.baseline/$_dest"
  _live="$DIR/$_dest"
  if [ ! -f "$_live" ] || [ ! -f "$_base" ]; then
    record "$SAMPLE" "$TIER" "update" "3-way merge keeps a local change and takes the shipped one" \
      "$RESULT_FAILED" "a managed artifact with a recorded baseline to merge" \
      "no managed artifact at $_dest with a baseline at $_base"
    return
  fi

  # The shipped text, as this binary renders it; the live file is unmodified here.
  cp "$_live" "$WORK/$SAMPLE-$TIER.shipped"
  _shipped="$WORK/$SAMPLE-$TIER.shipped"
  _shipped_line=$(sed -n '2p' "$_shipped")

  # base: an older shipped version that lacks line 2.
  sed '2d' "$_shipped" >"$_base"
  # local: that older version plus an owner-authored line, so the user's file
  # does NOT already contain the change the new version brings. Only a real
  # merge can end with both, which is what the assertion below reads.
  _marker="# qualification: owner-authored line that must survive the merge"
  cp "$_base" "$_live"
  printf '%s\n' "$_marker" >>"$_live"
  # The evidence is captured before the update, because a clean merge rewrites
  # the baseline to the shipped text and the aged base would be gone.
  cp "$_base" "$WORK/$SAMPLE-$TIER.aged-base"
  cp "$_live" "$WORK/$SAMPLE-$TIER.local-before"

  cf update
  _merge_status=$CF_STATUS
  {
    printf 'base, an older shipped version with line 2 removed:\n'
    cat "$WORK/$SAMPLE-$TIER.aged-base"
    printf '\nlocal, that older version plus one owner-authored line:\n'
    cat "$WORK/$SAMPLE-$TIER.local-before"
    printf '\nincoming brings back line 2: %s\n' "$_shipped_line"
    printf '\nresult after update (exit %s):\n' "$_merge_status"; cat "$_live"
  } >"$DIFFS/$SAMPLE-$TIER-update-3way.diff"

  _kept=$(grep -cF "$_marker" "$_live" || true)
  _took=$(grep -cF "$_shipped_line" "$_live" || true)
  if [ "$_merge_status" = 0 ] && [ "$_kept" -ge 1 ] && [ "$_took" -ge 1 ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "update" "3-way merge keeps a local change and takes the shipped one" \
    "$_s" "exit 0, the owner-authored line kept and the line only the new version carries now present" \
    "exit $_merge_status, owner line present=$_kept, incoming-only line present=$_took"

  # The conflict: local and incoming both change the same line, so the merge
  # cannot resolve it. The documented result is exit 2, the file untouched and
  # the shipped version written beside it.
  sed '2d' "$_shipped" >"$_base"
  cp "$_base" "$WORK/$SAMPLE-$TIER.conflict-base"
  sed '2s/.*/# qualification conflicting local rewrite of this line/' "$_shipped" >"$_live"
  cp "$_live" "$WORK/$SAMPLE-$TIER.conflict-local"
  rm -f "$DIR/$_dest.new"
  cf update
  _conflict_status=$CF_STATUS
  _conflict_out=$CF_OUT
  if cmp -s "$_live" "$WORK/$SAMPLE-$TIER.conflict-local"; then _untouched=yes; else _untouched=no; fi
  if [ ! -f "$DIR/$_dest.new" ]; then
    _sidecar=absent
  elif cmp -s "$DIR/$_dest.new" "$_shipped"; then
    _sidecar="holds the shipped bytes"
  else
    _sidecar="present but its bytes are not the shipped version"
  fi
  {
    printf 'base, the same aged version:\n'; cat "$WORK/$SAMPLE-$TIER.conflict-base"
    printf '\nlocal, which rewrites the very line the new version restores:\n'
    cat "$WORK/$SAMPLE-$TIER.conflict-local"
    printf '\nupdate exit %s\n%s\n' "$_conflict_status" "$_conflict_out"
    printf '\nsidecar %s\n' "$_sidecar"
    printf '\nlocal file after update, unchanged=%s\n' "$_untouched"
  } >"$DIFFS/$SAMPLE-$TIER-update-conflict.diff"
  if [ "$_conflict_status" = 2 ] && [ "$_untouched" = yes ] &&
    [ "$_sidecar" = "holds the shipped bytes" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "update" "rejecting: a conflicting change is never clobbered" \
    "$_s" "exit 2, the local file untouched and the shipped version written to the .new sidecar" \
    "exit $_conflict_status, local untouched=$_untouched, sidecar $_sidecar"

  # Restore the managed artifact, which --force is documented to do, and record
  # that documented flag while it is being used for real.
  rm -f "$DIR/$_dest.new"
  cf update --force
  _force_status=$CF_STATUS
  if cmp -s "$_live" "$_shipped"; then _restored=yes; else _restored=no; fi
  if [ "$_force_status" = 0 ] && [ "$_restored" = yes ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "update --force" "replace a user-modified managed file" \
    "$_s" "exit 0 and the shipped bytes back on disk" \
    "exit $_force_status, restored to the shipped bytes=$_restored"
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

  cf status --delivery
  record "$SAMPLE" "$TIER" "status --delivery" "epics with open and total task counts" \
    "$(status_for 0)" "exit 0 and the delivery view" "$(observed_exit)"

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

  cf recall scaffold --limit 3
  record "$SAMPLE" "$TIER" "recall --limit" "bound the number of hits" \
    "$(status_for 0)" "exit 0 with at most the requested number of hits" "$(observed_exit)"

  cf validate project-management
  record "$SAMPLE" "$TIER" "validate <PATH>" "validate an explicitly named path" \
    "$(status_for 0)" "exit 0 over the named record directory" "$(observed_exit)"

  cf remote protect --dry-run
  record "$SAMPLE" "$TIER" "remote protect --dry-run" "intended rules, nothing applied" \
    "$(status_for_match 0 'dry-run')" "exit 0, the intended rules and status dry-run" "$(observed_exit)"

  # ci over a real two-commit range on this sample.
  _base=$(git -C "$DIR" rev-list --max-parents=0 HEAD | tail -1)
  cf ci --base "$_base" --head HEAD
  record "$SAMPLE" "$TIER" "ci" "range and branch check over the seeded history" \
    "$(status_for_match 0 'clean')" "exit 0, the resolved range clean" "$(observed_exit)"

  printf '## Summary\n\nA qualification sample.\n\n## Changes\n\n- one change\n\n## Testing\n\n- the sample gate\n' \
    >"$WORK/pr-body-ok.md"
  cf ci --base "$_base" --head HEAD --pr-body-file "$WORK/pr-body-ok.md"
  record "$SAMPLE" "$TIER" "ci --pr-body-file" "positive: a conforming PR body" \
    "$(status_for 0)" "exit 0 with the body accepted" "$(observed_exit)"

  printf '## Summary\n\nGenerated with Claude Code\n\n## Changes\n\n- one change\n\n## Testing\n\n- the sample gate\n' \
    >"$WORK/pr-body-attr.md"
  cf ci --base "$_base" --head HEAD --pr-body-file "$WORK/pr-body-attr.md"
  record "$SAMPLE" "$TIER" "ci --pr-body-file" "rejecting: AI attribution in the PR body" \
    "$(status_for_match 1 'ai_attribution')" \
    "a non-zero exit naming the AI attribution rule" "$(observed_exit)"
}

qualify_doctor() {
  cf doctor
  record "$SAMPLE" "$TIER" "doctor" "all checks" \
    "$(status_for 0)" "exit 0; warnings allowed, failures are not" "$(observed_exit "$(printf '%s' "$CF_OUT" | awk '{print $1}' | sort | uniq -c | tr '\n' ' ')")"

  cf doctor --list
  _listed=$(printf '%s' "$CF_OUT" | grep -c . || true)
  if [ "$CF_STATUS" = 0 ] && [ "${_listed:-0}" -ge 1 ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "doctor --list" "name the available checks" \
    "$_s" "exit 0 and one line per available check" \
    "exit $CF_STATUS, $_listed check name(s) listed"

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
  _sources="$ASSET_ROOT/assets/base/agents/skills/cf-estimate"
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
  if [ "$CF_STATUS" = 0 ] && [ -z "$(printf '%s' "$CF_OUT" | tr -d '[:space:]')" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "hook git-guard" "positive: a read-only git command passes" \
    "$_s" "exit 0 and nothing written to stdout or stderr" \
    "exit $CF_STATUS, output $(printf '%s' "${CF_OUT:-<empty>}" | head -1 | cut -c1-120)"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"git push --force origin main"}}' hook git-guard
  record "$SAMPLE" "$TIER" "hook git-guard" "rejecting: force-push to a protected branch" \
    "$(status_for_match 2 'git.force_push_protected')" \
    "exit 2 naming policy rule git.force_push_protected" "$(observed_exit)"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"ls -la"}}' hook exec-guard
  if [ "$CF_STATUS" = 0 ] && [ -z "$(printf '%s' "$CF_OUT" | tr -d '[:space:]')" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "hook exec-guard" "positive: an ordinary command passes" \
    "$_s" "exit 0 and nothing written to stdout or stderr" \
    "exit $CF_STATUS, output $(printf '%s' "${CF_OUT:-<empty>}" | head -1 | cut -c1-120)"

  cf_stdin '{"tool_name":"Bash","tool_input":{"command":"sudo rm -rf /"}}' hook exec-guard
  record "$SAMPLE" "$TIER" "hook exec-guard" "rejecting: a dangerous command" \
    "$(status_for_match 2 'security.dangerous_commands')" \
    "exit 2 naming policy rule security.dangerous_commands" "$(observed_exit)"

  # delegate-turn carries its own exit contract. docs/cli.md summarises it as
  # exit 1 without --run-id and exit 2 when a schema-v2 payload cannot be read;
  # the binary is narrower, returning 2 only when the failing payload is a
  # prompt submission and 1 for any other failure, so all three are recorded.
  _dt_state="$WORK/$SAMPLE-$TIER-delegate-turn"
  rm -rf "$_dt_state"
  "$BINARY" delegate init --run-id qualification --state-dir "$_dt_state" >/dev/null 2>&1
  printf 'corrupt' >"$_dt_state/settings.json"

  cf_stdin '{"hook_event_name":"SessionStart","source":"startup"}' hook delegate-turn
  record "$SAMPLE" "$TIER" "hook delegate-turn" "rejecting: no run id supplied" \
    "$(status_for 1)" "exit 1 because --run-id is required" "$(observed_exit)"

  cf_stdin '{"hook_event_name":"SessionStart","source":"startup"}' hook delegate-turn \
    --run-id qualification --state-dir "$_dt_state"
  record "$SAMPLE" "$TIER" "hook delegate-turn" "rejecting: an unreadable run, not a prompt" \
    "$(status_for 1)" \
    "exit 1, the advisory failure, because the payload is not a prompt submission" "$(observed_exit)"

  cf_stdin '{"hook_event_name":"UserPromptSubmit","prompt":"qualification"}' hook delegate-turn \
    --run-id qualification --state-dir "$_dt_state"
  record "$SAMPLE" "$TIER" "hook delegate-turn" "rejecting: an unreadable run under a prompt submission" \
    "$(status_for 2)" \
    "exit 2, which makes the harness block the turn rather than run it blind" "$(observed_exit)"
  rm -rf "$_dt_state"

  cf_stdin '{"hook_event_name":"SessionStart","source":"startup"}' hook session-orient
  record "$SAMPLE" "$TIER" "hook session-orient" "emit the orient digest" \
    "$(status_for 0)" "exit 0 and the digest on stdout" "$(observed_exit "$(printf '%s' "$CF_OUT" | head -1)")"

  # session-summary exits 0 whether it recorded or only warned, so the exit
  # status alone proves nothing. The ledger lives under the git common
  # directory, not under .codeflow, and the command names the file it wrote.
  _ledger_dir="$(git -C "$DIR" rev-parse --path-format=absolute --git-common-dir)/codeflow/ledger"
  # A ledger file count proves nothing about this submission, and the command
  # exits 0 whether it recorded or only warned. The check submits a session id
  # unique to this sample and tier, then reads that id back out of the file the
  # command says it wrote.
  _sid="qualification-$SAMPLE-$TIER-$$"
  _entries_before=$(grep -rlF "$_sid" "$_ledger_dir" 2>/dev/null | wc -l | tr -d ' ')
  cf_stdin "{\"session_id\":\"$_sid\",\"reason\":\"clear\",\"hook_event_name\":\"SessionEnd\"}" hook session-summary
  _recorded_path=$(printf '%s' "$CF_OUT" | sed -n 's/.*recorded to //p' | head -1)
  _entry_found=no
  if [ -n "$_recorded_path" ] && [ -f "$_recorded_path" ] &&
    grep -qF "$_sid" "$_recorded_path" 2>/dev/null; then
    _entry_found=yes
  fi
  if [ "$CF_STATUS" = 0 ] && [ "$_entries_before" = 0 ] && [ "$_entry_found" = yes ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "hook session-summary" "append the session record to the ledger" \
    "$_s" "exit 0 and this submission's own session id read back from the file the command names" \
    "exit $CF_STATUS, entries before $_entries_before, id found in the named file=$_entry_found, $(printf '%s' "$CF_OUT" | head -1 | cut -c1-110)"
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
  if [ "$CF_STATUS" = 1 ] && printf '%s' "$CF_OUT" | grep -q 'git.ai_attribution' &&
    printf '%s' "$CF_OUT" | grep -q 'git.commit_body'; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "git-hook commit-msg" "rejecting: AI attribution" \
    "$_s" "exit 1 naming both rules the trailer trips, git.ai_attribution and git.commit_body" \
    "$(observed_exit)"

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

  # transfer is irreversible for the adopted runtime, so it is exercised on a
  # throwaway adoption in this sample and only at the minimal tier, and the
  # refusal without --confirm is checked first.
  if [ "$TIER" = minimal ]; then
    cf portal transfer
    record "$SAMPLE" "$TIER" "portal transfer" "rejecting: no confirmation supplied" \
      "$(status_for 2)" "a non-zero exit refusing to transfer without --confirm" "$(observed_exit)"

    cf portal transfer --confirm
    record "$SAMPLE" "$TIER" "portal transfer --confirm" "hand the runtime to the project" \
      "$(status_for 0)" "exit 0 accepting future runtime reconciliation" "$(observed_exit)"

    # A transferred runtime is no longer reconciled, so the adoption is removed
    # before the sweep continues and nothing downstream inherits that state.
    rm -rf "$DIR/docs-portal"
    cf portal setup --path docs-portal
    record "$SAMPLE" "$TIER" "portal setup" "re-adopt after a transfer and removal" \
      "$(status_for 0)" "exit 0 and a freshly written portal workspace" "$(observed_exit)"
  fi
}

# The portal build and its evidence validation run once: they install a real
# dependency tree and render the site, which the per-tier sweep repeats nothing of.
qualify_portal_build() {
  _portal="$DIR/docs-portal"
  if [ ! -d "$_portal" ]; then
    record "$SAMPLE" "$TIER" "npm run build (portal)" "render the portal from repository sources" \
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
    record "$SAMPLE" "$TIER" "npm run build (portal)" "render the portal from repository sources" \
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
  record "$SAMPLE" "$TIER" "npm run build (portal)" "render the portal from repository sources" \
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
    _doc="$ASSET_ROOT/assets/base/agents/skills/cf-present/resources/present-document.example.json"
  fi

  # Session discovery resolves the project from the working directory, so the
  # teardown that closes these sessions must run from the same repository.
  PRESENT_REPO="$DIR"

  # The session's current revision, or -1 when it cannot be read.
  present_revision() {
    (cd "$PRESENT_REPO" && HOME="$PRESENT_HOME" "$BINARY" present list 2>/dev/null) |
      python3 -c 'import json,sys
try:
    for s in json.load(sys.stdin):
        if s["id"] == sys.argv[1]:
            print(s["current_revision"])
            break
    else:
        print(-1)
except Exception:
    print(-1)' "$1" 2>/dev/null || printf '%s' -1
  }

  present_run() {
    CF_OUT=$(HOME="$PRESENT_HOME" "$BINARY" present "$@" 2>&1 </dev/null) && CF_STATUS=0 || CF_STATUS=$?
    printf '\n$ codeflow present %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
  }

  present_run open "$_doc" --no-launch
  record "$SAMPLE" "$TIER" "present open" "start a validated review session" \
    "$(status_for_match 0 'ready')" \
    "exit 0 and a session ready on an owner-private bootstrap" "$(observed_exit)"

  present_run list
  _sid=$(printf '%s' "$CF_OUT" | python3 -c 'import json,sys
try:
    sessions = json.load(sys.stdin)
except Exception:
    sessions = []
print(sessions[0]["id"] if sessions else "")' 2>/dev/null)
  if [ "$CF_STATUS" = 0 ] && [ -n "$_sid" ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "present list" "list the sessions for this project" \
    "$_s" "exit 0 and JSON naming the session just opened" \
    "exit $CF_STATUS, session id $([ -n "$_sid" ] && echo "$_sid" || echo none)"
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

  present_run show "$_sid" --no-launch
  record "$SAMPLE" "$TIER" "present show" "print the endpoint and profile without launching" \
    "$(status_for_match 0 "$_sid")" \
    "exit 0, the session endpoint and profile printed and no browser launched" "$(observed_exit)"

  # A new revision, not merely exit 0: the session's current revision is read
  # before and after, and the update has to move it.
  _rev_before=$(present_revision "$_sid")
  present_run update "$_sid" "$_doc"
  _update_status=$CF_STATUS
  _rev_after=$(present_revision "$_sid")
  if [ "$_update_status" = 0 ] && [ "$_rev_before" -ge 0 ] 2>/dev/null &&
    [ "$_rev_after" -gt "$_rev_before" ] 2>/dev/null; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "present update" "append a validated immutable revision" \
    "$_s" "exit 0 and the session's current revision moved forward" \
    "exit $_update_status, revision $_rev_before then $_rev_after"

  _export="$WORK/$SAMPLE-$TIER-present.html"
  present_run export "$_sid" --out "$_export"
  if [ "$CF_STATUS" = 0 ] && [ -s "$_export" ] && grep -qi '<html' "$_export"; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "present export" "a deterministic self-contained HTML artifact" \
    "$_s" "exit 0 and a non-empty standalone HTML file at the named path" \
    "exit $CF_STATUS, $([ -f "$_export" ] && wc -c <"$_export" | tr -d ' ' || echo 0) bytes written"

  present_run resolve "$_sid" 00000000-0000-0000-0000-000000000000 --event-version 1 --status addressed
  record "$SAMPLE" "$TIER" "present resolve" "rejecting: an event outside the session fails closed" \
    "$(status_for_match 2 'does not belong to session')" \
    "non-zero exit refusing a cross-session transition" "$(observed_exit)"

  # The only supported producer of a feedback envelope is the review surface,
  # which posts to the session service from a browser. This harness drives the
  # CLI and hosts no browser, so it cannot create one. The check is covered
  # where the browser already runs: crates/codeflow-present/web/scripts/
  # real-browser-check.mjs submits a real review and resolves the delivered
  # event at its current version, under TSK-007's gate.
  record "$SAMPLE" "$TIER" "present resolve" "positive: resolve a real reviewer envelope" \
    "$RESULT_UNAVAILABLE" "a delivered envelope marked addressed at its current version" \
    "this harness drives the CLI and hosts no browser, and only the review surface produces an envelope; the same transition is exercised by real-browser-check.mjs, which resolves a delivered event at event-version 2" \
    "TSK-007 presentation qualification, whose real-browser journey already covers it"

  present_run close "$_sid"
  _close_status=$CF_STATUS
  # close returns once the session is marked closed; the documented contract
  # promises nothing about when its service process has exited, and clear
  # deliberately retains a session whose service is still alive. The wait below
  # measures that gap instead of assuming it is zero.
  _wait=0
  while [ "$_wait" -lt 30 ]; do
    present_run clear "$_sid" --older-than 0d --dry-run
    [ "$CF_STATUS" = 0 ] && break
    _wait=$((_wait + 1))
    sleep 1
  done
  record "$SAMPLE" "$TIER" "present close" "close the session and its service" \
    "$([ "$_close_status" = 0 ] && printf '%s' "$RESULT_PASSED" || printf '%s' "$RESULT_FAILED")" \
    "exit 0 and the session marked closed" \
    "exit $_close_status; its service was still running for $_wait s afterwards"

  record "$SAMPLE" "$TIER" "present clear --dry-run" "name the eligible closed session without removing it" \
    "$(status_for_match 0 "$_sid")" \
    "exit 0 naming the closed session and removing nothing" "$(observed_exit)"

  present_run clear "$_sid" --older-than 0d
  _clear_status=$CF_STATUS
  present_run list
  _left=$(printf '%s' "$CF_OUT" | python3 -c 'import json,sys
try:
    print(len(json.load(sys.stdin)))
except Exception:
    print("unreadable")' 2>/dev/null)
  if [ "$_clear_status" = 0 ] && [ "$_left" = 0 ]; then _s=$RESULT_PASSED; else _s=$RESULT_FAILED; fi
  record "$SAMPLE" "$TIER" "present clear" "remove the closed session" \
    "$_s" "exit 0 and the project listing empty afterwards" \
    "exit $_clear_status, $_left session(s) still listed"
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
  _arm_status=$CF_STATUS
  # The armed request must exist and must hold the exact bytes of the prompt
  # file, because the host delivers those bytes and the run correlates on them.
  _request="$_state/turns/$_turn/request.json"
  _armed_sha=absent
  _prompt_sha=$(shasum -a 256 "$WORK/delegate-prompt.txt" | awk '{print $1}')
  if [ -f "$_request" ]; then
    _armed_sha=$(python3 -c 'import json,sys
print(json.load(open(sys.argv[1])).get("prompt_sha256", "no-prompt-digest"))' \
      "$_request" 2>/dev/null || echo unreadable)
  fi
  if [ "$_arm_status" = 0 ] && [ "$_armed_sha" = "$_prompt_sha" ]; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "delegate arm" "positive: arm one prompt for a live turn" \
    "$_s" "exit 0 and a request holding the exact prompt bytes, digest $_prompt_sha" \
    "exit $_arm_status, request $([ -f "$_request" ] && echo written || echo missing), armed digest $_armed_sha"

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
    record "$SAMPLE" "$TIER" "candidate binding" "the live session's pane resolves codeflow to the candidate" \
      "$RESULT_UNAVAILABLE" "the live pane resolves codeflow to $BINARY" "$1" "$2"
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

  # The canary's status line must be text deliver_turn can recognise, so the
  # sample gets a local settings file that prints a fixed marker there.
  if ! write_status_line_settings "$DIR"; then
    canary_unavailable \
      "could not write $DIR/.claude/settings.local.json with the fixed status line, or it already exists" \
      "this harness"
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
  # The same line puts the candidate first on the pane's PATH, which the
  # session and its hooks inherit, and records what codeflow resolves to.
  _pane_codeflow="$WORK/pane-codeflow"
  herdr pane run "$HERDR_PANE" \
    "$(pane_env_command "$BIN_DIR:$DECOY_DIR" "$_pane_codeflow")" >>"$TRANSCRIPT" 2>&1 || true
  herdr pane wait-output "$HERDR_PANE" --match "TRACKED=1" --timeout 15000 >>"$TRANSCRIPT" 2>&1 || true
  sleep 2

  _agent_name="cf-codeflow-tsk044-cl01"
  HERDR_AGENT=$_agent_name
  _start_cmd="herdr agent start $_agent_name --kind claude --pane $HERDR_PANE -- --permission-mode bypassPermissions --settings $_state/settings.json"
  if ! _start_out=$(herdr agent start "$_agent_name" --kind claude --pane "$HERDR_PANE" -- \
    --permission-mode bypassPermissions --settings "$_state/settings.json" 2>&1); then
    printf '\n$ %s\n%s\n' "$_start_cmd" "$_start_out" >>"$TRANSCRIPT"
    # Say what actually blocked the session rather than that something failed.
    # A fresh scaffolded sample carries its own .claude/settings.json, and the
    # harness asks a human to trust that workspace before it takes a prompt;
    # the pane needs a moment to paint that prompt before it can be read.
    sleep 8
    # One-shot detection, so this read takes `recent`: the question may already
    # have scrolled off the visible screen by now. The poll that follows uses
    # the visible screen instead, because scrollback keeps an answered question
    # forever and every answer would time out.
    _pane_text=$(herdr pane read "$HERDR_PANE" --source recent --lines 120 2>/dev/null || true)
    printf 'pane after failed agent start:\n%s\n' "$_pane_text" >>"$TRANSCRIPT"
    HERDR_AGENT=""
    if ! printf '%s' "$_pane_text" | grep -qF -- "$TRUST_PROMPT_MATCH"; then
      canary_unavailable \
        "$_start_cmd failed; reply: $(oneline "$_start_out"); pane text is in the transcript" \
        "operator environment"
      return
    fi

    # The blocker is the trust prompt, and no agent may answer it. The reason
    # is assembled in two pieces so the sentence about the wait stays inside
    # the recorded cell, which `record` cuts at 400 characters.
    _trust_why="the Claude Code workspace-trust prompt blocked startup in the tab this run created: the sample carries the scaffolded .claude/settings.json, and the session asks a human to trust the folder before it accepts any prompt"
    _trust_detail="Command: $_start_cmd. Reply: $(oneline "$_start_out")"

    # resolve_trust_prompt asks the operator, waits, and then proves a live
    # session on this pane and tab. It returns non-zero for every outcome that
    # is not a proven session, so the canary is recorded unavailable with the
    # reason it actually hit and `delegate wait --until ready` never runs on a
    # session nobody has seen.
    if ! resolve_trust_prompt "$HERDR_PANE" "$HERDR_TAB" "$DIR" \
      "$TRUST_WAIT_SECONDS" "$_agent_name" "$_state/settings.json"; then
      canary_unavailable "$_trust_why; $TRUST_REASON. $_trust_detail" "$TRUST_OWNER"
      return
    fi
    HERDR_AGENT=$_agent_name
  else
    printf '\n$ %s\n%s\n' "$_start_cmd" "$_start_out" >>"$TRANSCRIPT"
  fi

  cf delegate wait --run-id "$_run" --state-dir "$_state" --until ready --timeout-seconds 180
  _ready_status=$CF_STATUS
  record "$SAMPLE" "$TIER" "delegate wait --until ready" \
    "harness startup observed in Herdr tab $HERDR_TAB" \
    "$(status_for 0)" "exit 0 once the session's SessionStart hook records ready" "$(observed_exit)"

  # The hooks that wrote ready resolved bare codeflow in the pane's
  # environment, so that resolution must be the candidate itself.
  if pane_codeflow_binding "$_pane_codeflow" "$BINARY"; then
    _s=$RESULT_PASSED
  else
    _s=$RESULT_FAILED
  fi
  record "$SAMPLE" "$TIER" "candidate binding" \
    "the live session's pane resolves codeflow to the candidate" \
    "$_s" "command -v codeflow in pane $HERDR_PANE is $BINARY, digest $BINARY_SHA" \
    "resolved $PANE_CF_PATH, digest $PANE_CF_SHA"

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

  deliver_turn "$HERDR_PANE" "$WORK/delegate-prompt.txt" "$_run" "$_state" "$_turn" || true

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
["build", "verify"]. Work on the branch $PIPELINE_BRANCH; never commit to
main and never push. When the workflow returns, write the object it returned,
verbatim, as a single line of JSON to $PIPELINE_RESULT in the repository root.
That object is the workflow's own result and carries status, attempts and trail.
If the workflow cannot run at all, write instead, on one line:
{"status": "unavailable", "reason": "<the exact reason>"}
Do not write anything else to that file, and do not hand-write a trail.
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

  _pipeline_expected="a transcript showing the native Workflow tool invoked on the scaffolded pipeline, a terminal turn, that workflow's own returned object at status complete with build and verify stages and an approved verify verdict, equal to Claude Code's own task output for the run, and on the pipeline branch this harness's own subtract(4, 1) test green and the sample gate green"
  _pipeline_began=$(date +%s)
  deliver_turn "$HERDR_PANE" "$WORK/pipeline-prompt.txt" "$_run" "$_state" "$_turn2" || true
  cf delegate wait --run-id "$_run" --state-dir "$_state" --until accepted \
    --turn-id "$_turn2" --timeout-seconds "$PIPELINE_ACCEPT_SECONDS"
  # A turn the session never accepted cannot run the pipeline, and nothing
  # later can change that, so the row fails now instead of waiting out the
  # pipeline budget on a turn that does not exist.
  if [ "$CF_STATUS" != 0 ]; then
    record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
      "$RESULT_FAILED" "$_pipeline_expected" \
      "the pipeline turn was not accepted within $PIPELINE_ACCEPT_SECONDS s: $(observed_exit)"
    return
  fi

  # The first Stop only ends the turn that launched the workflow; the budget
  # covers it and the wait for the result file together.
  cf delegate wait --run-id "$_run" --state-dir "$_state" --until terminal \
    --turn-id "$_turn2" --timeout-seconds "$PIPELINE_TIMEOUT_SECONDS"
  _pipeline_terminal=$CF_STATUS
  _pipeline_out=$CF_OUT
  _left=$((PIPELINE_TIMEOUT_SECONDS - ($(date +%s) - _pipeline_began)))
  [ "$_left" -gt 0 ] || _left=0
  wait_for_pipeline_result "$DIR/$PIPELINE_RESULT" "$_state/turns/$_turn2/result.json" "$_left" &&
    _result_wait=0 || _result_wait=$?
  case $_result_wait in
    0) _result_wait="result file written after $(($(date +%s) - _pipeline_began)) s" ;;
    3) _result_wait="the turn recorded a terminal failure and no result file" ;;
    *) _result_wait="no result file within the $PIPELINE_TIMEOUT_SECONDS s row timeout" ;;
  esac
  printf '\npipeline result wait: %s\n' "$_result_wait" >>"$TRANSCRIPT"

  if [ -f "$DIR/$PIPELINE_RESULT" ]; then
    _state_line=$(head -1 "$DIR/$PIPELINE_RESULT")
    cp "$DIR/$PIPELINE_RESULT" "$DIFFS/pipeline-result.txt"
  else
    _state_line=""
  fi

  # The session's own sentence is a claim, never the evidence. A pass needs the
  # turn to have reached a terminal state, the workflow to report a successful
  # one, and the work it was asked for to be independently verifiable here: the
  # subtract function present with its test, and the sample's own gate green.
  # A workflow that ran and failed is a failure, not an absent capability.
  # The result file must be the object the workflow itself returns, so its
  # shape is read rather than its prose. `status` must be exactly `complete`,
  # the one successful state the driver emits, and `trail` must be the per
  # stage record the driver builds; only a real run produces both. What the
  # workflow did is then checked against behaviour this harness verifies for
  # itself, never against a test the peer wrote.
  _shape=$(printf '%s' "${_state_line:-}" | pipeline_result_shape)

  # Behaviour, verified here, where the pipeline built it: its branch, in the
  # worktree it used or a spare checkout of that branch, never the sample root
  # on main. A test this harness writes is compiled and run by the sample's own
  # toolchain there, so a subtract that returns the wrong value fails even if
  # the peer shipped a test that agrees with it, and the sample gate runs there.
  _behaviour=not-checked
  _gate=not-run
  _spare="$WORK/pipeline-checkout"
  if _checkout=$(pipeline_checkout "$DIR" "$PIPELINE_BRANCH" "$_spare"); then
    _in=${_checkout#"$DIR"/}
    [ "$_checkout" != "$_spare" ] || _in="a spare checkout"
    _where="checked on $PIPELINE_BRANCH at $(git -C "$_checkout" rev-parse --short HEAD 2>/dev/null) in $_in"
    if [ -f "$_checkout/Cargo.toml" ]; then
      mkdir -p "$_checkout/tests"
      cat >"$_checkout/tests/qualification_subtract.rs" <<'PROBE'
//! Written by the release qualification, not by the session under test.
#[test]
fn subtract_four_minus_one_is_three() {
    assert_eq!(qualification_sample::subtract(4, 1), 3);
}
PROBE
      # cargo's own status, captured before anything truncates its output. A
      # pipe here would report the exit status of the last stage instead, and
      # a test binary that exits 101 would read as success.
      sh_run "cd '$_checkout' && cargo test --test qualification_subtract >'$WORK/subtract-probe.log' 2>&1"
      _sub_status=$CF_STATUS
      if [ "$_sub_status" = 0 ]; then
        _behaviour=passed
      else
        _behaviour="failed at exit $_sub_status: $(oneline "$(tail -5 "$WORK/subtract-probe.log" 2>/dev/null)")"
      fi
      rm -f "$_checkout/tests/qualification_subtract.rs"
    fi
    cd "$_checkout"
    cf test --mode full --strict
    _gate=$CF_STATUS
    cd "$DIR"
    [ "$_checkout" != "$_spare" ] || git -C "$DIR" worktree remove --force "$_spare" >/dev/null 2>&1 || true
  else
    _where="no branch $PIPELINE_BRANCH to check"
  fi

  workflow_task_evidence "$_state" "$DIR/$PIPELINE_RESULT" >"$WORK/task-evidence.txt" &&
    _task_ok=0 || _task_ok=1
  _task=$(cat "$WORK/task-evidence.txt")
  _invoked=$(workflow_invocation_evidence "$_state")
  _launch=$(workflow_launch_evidence "$_state")
  _observed="turn exit $_pipeline_terminal; native Workflow invocation $_invoked; $_launch; $_result_wait; workflow result $_shape; $_task; subtract(4, 1) test $_behaviour; sample gate exit $_gate; $_where"
  printf '\npipeline row observed: %s\n' "$_observed" >>"$TRANSCRIPT"

  # Whether the capability was exercised is decided by the transcript, not by
  # the result file. Once the Workflow tool has been invoked, every shortfall
  # is a failure: an absent or failed result then means the pipeline ran and
  # did not finish, which is a result, not a missing capability. Only a turn
  # with no invocation and nothing claimed can be unavailable, and a result
  # claiming success with no invocation behind it is a fabrication, so it
  # fails rather than passing.
  if [ "$_invoked" = yes ]; then
    if [ "$_pipeline_terminal" = 0 ] && [ "$_shape" = complete ] && [ "$_task_ok" = 0 ] &&
      [ "$_behaviour" = passed ] && [ "$_gate" = 0 ]; then
      _pipeline_verdict=$RESULT_PASSED
    else
      _pipeline_verdict=$RESULT_FAILED
    fi
  elif [ "$_shape" = complete ]; then
    _pipeline_verdict=$RESULT_FAILED
  elif [ "$_shape" = unavailable ] || [ "$_shape" = absent ]; then
    _pipeline_verdict=unavailable
  else
    _pipeline_verdict=$RESULT_FAILED
  fi

  if [ "$_pipeline_verdict" = unavailable ]; then
    pipeline_unavailable \
      "$_observed; wait output: $(oneline "$_pipeline_out")" \
      "Claude Code Workflow runtime in the canary session"
  elif [ "$_pipeline_verdict" = "$RESULT_PASSED" ]; then
    record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
      "$RESULT_PASSED" \
      "$_pipeline_expected" \
      "$_observed"
  else
    record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
      "$RESULT_FAILED" \
      "$_pipeline_expected" \
      "$_observed"
  fi
}

# ---------------------------------------------------------------------------
# Pipeline workflow. The scaffolded pipeline is a Claude Code Workflow script:
# only a live session can execute it, so it runs as a second armed turn inside
# the canary session rather than being simulated here.
# ---------------------------------------------------------------------------

PIPELINE_RESULT="qualification-pipeline-result.txt"
PIPELINE_BRANCH="feat/pipeline-canary"

# How long the pipeline turn has to be accepted, and the row timeout covering
# the launching turn and the backgrounded workflow behind it.
PIPELINE_ACCEPT_SECONDS=180
PIPELINE_TIMEOUT_SECONDS=3600

pipeline_unavailable() {
  record "$SAMPLE" "$TIER" "pipeline workflow" "run the scaffolded pipeline end to end" \
    "$RESULT_UNAVAILABLE" \
    "a transcript showing the native Workflow tool invoked on the scaffolded pipeline, and that workflow's own returned object at status complete" \
    "$1" "$2"
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

  # Every real commit, ref move and push in this run went through the installed
  # shims, which resolve codeflow through PATH. If any of them had reached a
  # different binary, the decoy behind the candidate would have logged it.
  if [ -f "$DECOY_SENTINEL" ]; then
    _s=$RESULT_FAILED
    _decoy="invoked $(grep -c . "$DECOY_SENTINEL") time(s): $(head -1 "$DECOY_SENTINEL")"
  else
    _s=$RESULT_PASSED
    _decoy="never invoked; no codeflow but the candidate answered any subprocess"
  fi
  record "$SAMPLE" "$TIER" "candidate binding" "no other codeflow on PATH is ever invoked" \
    "$_s" "the decoy behind the candidate on PATH is never reached" "$_decoy"
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
  qualify_binding
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

for _sample in ${SAMPLES:-greenfield-rust greenfield-node brownfield}; do
  for _tier in ${TIERS_TO_RUN:-minimal standard full}; do
    run_sample_tier "$_sample" "$_tier"
  done
done

# The once-only lanes reuse samples the sweep already qualified. A narrowed
# sweep may not have built them, and silently skipping a whole lane would be a
# gap, so the absence is recorded rather than ignored.
once_only_lane() {
  SAMPLE=$1
  TIER=$2
  DIR="$WORK/$SAMPLE-$TIER"
  if [ ! -d "$DIR" ]; then
    shift 2
    for _lane in "$@"; do
      record "$SAMPLE" "$TIER" "$_lane" "the once-only lane for this sample" \
        "$RESULT_UNAVAILABLE" "the lane run against the $SAMPLE $TIER sample" \
        "this run did not build that sample, so the lane had nothing to run against" \
        "this harness invocation, which narrowed the sweep"
    done
    return 1
  fi
  cd "$DIR"
  return 0
}

if once_only_lane greenfield-node full "npm run build (portal)"; then
  qualify_portal_build
fi

if once_only_lane greenfield-rust full "present open" "delegate init" "pipeline workflow"; then
  qualify_present
  qualify_delegate
fi
cd "$WORK"

qualify_boundary

END_UTC=$(date -u '+%Y-%m-%dT%H:%M:%SZ')

# ---------------------------------------------------------------------------
# Surfaces the matrix does not reach, reconciled against docs/cli.md
# ---------------------------------------------------------------------------

UNCOVERED="$WORK/uncovered.tsv"
cat >"$UNCOVERED" <<'UNCOV'
codeflow init --force	overwrites existing files; the qualification proves the non-destructive default, and a destructive flag is not exercised against a sample it would damage
codeflow test setup --add-target	appends one target interactively and needs a terminal, which this run does not host
codeflow remote protect --provider <other>	only github has an adapter; the dry-run row covers the adapter and another provider only prints a manual checklist
codeflow present export --theme, --mode	the export row covers the default editorial theme in system mode; the other five combinations are TSK-007's presentation qualification
codeflow help <command>	a listing of the same commands this matrix already exercises one by one
UNCOV

# The verdict the run exits with is computed from the results while they still
# exist, so nothing has to outlive the directory that holds them and no file is
# ever written beside the caller's own.
REQUIRED_UNAVAILABLE=$(awk -F'\t' '$5=="unavailable" && $1!="release boundary"' "$RESULTS" | wc -l | tr -d ' ')

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
  --diffs "$DIFFS" \
  --blocked-on "$BLOCKED_ON" \
  --uncovered "$UNCOVERED"

# The work directory is removed only now, after the record has been rendered
# from it, and the record's teardown line is replaced by the verified outcome.
remove_work

printf '\nmatrix: %s\n' "$OUT" >&2
if [ "$FAILURES" -gt 0 ]; then
  printf 'qualification blocked: %s failed check(s)\n' "$FAILURES" >&2
  exit 1
fi
if [ "${REQUIRED_UNAVAILABLE:-0}" -gt 0 ]; then
  printf 'qualification blocked: %s required check(s) did not run\n' "$REQUIRED_UNAVAILABLE" >&2
  exit 1
fi
if [ "$CLEANUP_FAILURES" -gt 0 ]; then
  printf 'teardown incomplete: %s resource(s) were not released\n' "$CLEANUP_FAILURES" >&2
  exit 1
fi
printf 'qualification: no failed checks and no required check unavailable\n' >&2
