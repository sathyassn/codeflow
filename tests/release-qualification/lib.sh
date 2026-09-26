#!/bin/sh
# Shared helpers for the release-qualification harness.
#
# Sourced by qualify.sh. Everything here writes to the run's disposable work
# directory; nothing touches the repository under qualification.

# ---------------------------------------------------------------------------
# Result recording
# ---------------------------------------------------------------------------
#
# One row per check. Exactly one of passed / failed / unavailable, always with
# an expected and an observed value. `unavailable` additionally names the owner
# the gate belongs to, so a gap can never read as a pass.

RESULT_PASSED=passed
RESULT_FAILED=failed
RESULT_UNAVAILABLE=unavailable

# record <sample> <tier> <command> <check> <status> <expected> <observed> [owner]
record() {
  _sample=$1
  _tier=$2
  _command=$3
  _check=$4
  _status=$5
  _expected=$6
  _observed=$7
  _owner=${8:-}

  case $_status in
    "$RESULT_PASSED" | "$RESULT_FAILED" | "$RESULT_UNAVAILABLE") ;;
    *)
      printf 'harness error: bad status "%s" for %s/%s\n' "$_status" "$_command" "$_check" >&2
      exit 70
      ;;
  esac
  if [ "$_status" = "$RESULT_UNAVAILABLE" ] && [ -z "$_owner" ]; then
    printf 'harness error: unavailable row without an owner: %s/%s\n' "$_command" "$_check" >&2
    exit 70
  fi
  if [ -z "$_expected" ] || [ -z "$_observed" ]; then
    printf 'harness error: blank expected/observed: %s/%s\n' "$_command" "$_check" >&2
    exit 70
  fi

  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$_sample" "$_tier" "$_command" "$_check" "$_status" \
    "$(oneline "$_expected")" "$(oneline "$_observed")" "$_owner" >>"$RESULTS"

  if [ "$_status" = "$RESULT_FAILED" ]; then
    FAILURES=$((FAILURES + 1))
  fi
  printf '  %-12s %s / %s\n' "$_status" "$_command" "$_check" >&2
}

# Collapse a value to a single table-safe line.
#
# Em and en dashes become a plain hyphen: the project's writing rules forbid
# both characters in tracked files, and some command output contains them, so
# the recorded cell is normalized rather than the rule being broken. Nothing
# else about the observed text changes.
oneline() {
  printf '%s' "$1" |
    tr '\n\t|' '  /' |
    sed $'s/\u2014/-/g; s/\u2013/-/g' |
    cut -c1-400
}

# ---------------------------------------------------------------------------
# Command execution
# ---------------------------------------------------------------------------
#
# cf() runs the binary under qualification, capturing stdout+stderr in CF_OUT
# and the exit status in CF_STATUS. It never aborts the run: a non-zero status
# is data, not an error, because half the matrix is about rejections.

cf() {
  CF_OUT=$("$BINARY" "$@" 2>&1 </dev/null) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ codeflow %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# cf_stdin <payload> <args...> - same, with a stdin payload (the Claude hooks).
cf_stdin() {
  _payload=$1
  shift
  CF_OUT=$(printf '%s' "$_payload" | "$BINARY" "$@" 2>&1) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ echo <payload> | codeflow %s\n[exit %s]\n%s\n' "$*" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# sh_run <command string> - an ordinary shell command, same capture contract.
sh_run() {
  CF_OUT=$(eval "$1" 2>&1) && CF_STATUS=0 || CF_STATUS=$?
  printf '\n$ %s\n[exit %s]\n%s\n' "$1" "$CF_STATUS" "$CF_OUT" >>"$TRANSCRIPT"
}

# ---------------------------------------------------------------------------
# Assertions
#
# Each returns the status string rather than branching at every call site.
# ---------------------------------------------------------------------------

# status_for <expected-exit> - passed when CF_STATUS matches.
status_for() {
  if [ "$CF_STATUS" = "$1" ]; then printf '%s' "$RESULT_PASSED"; else printf '%s' "$RESULT_FAILED"; fi
}

# status_for_match <expected-exit> <substring> - exit AND message must match.
status_for_match() {
  if [ "$CF_STATUS" = "$1" ] && printf '%s' "$CF_OUT" | grep -qF -- "$2"; then
    printf '%s' "$RESULT_PASSED"
  else
    printf '%s' "$RESULT_FAILED"
  fi
}

# observed_exit [extra] - the observed column for an exit-status check.
observed_exit() {
  if [ -n "${1:-}" ]; then
    printf 'exit %s: %s' "$CF_STATUS" "$1"
  else
    printf 'exit %s: %s' "$CF_STATUS" "$(printf '%s' "$CF_OUT" | grep -v '^[[:space:]]*$' | head -2 | tr '\n' ' ')"
  fi
}

# ---------------------------------------------------------------------------
# The workspace-trust prompt
# ---------------------------------------------------------------------------
#
# A freshly scaffolded sample carries its own .claude/settings.json, so the
# Claude session the canary starts asks a human to trust the folder before it
# takes any prompt. No agent may answer that question, so the harness waits for
# the operator instead of recording the whole delegate lane unavailable.
#
# Everything here fails closed. A pane the harness cannot read is never read as
# an answer: the run would otherwise record a failed delegate lane on evidence
# that only says herdr stopped replying.

# The line Claude Code paints while it waits for that answer.
TRUST_PROMPT_MATCH='Is this a project you created or one you trust'

# How often the pane is re-read while waiting.
TRUST_POLL_SECONDS=5

# How long one pane read may take. Never longer than the poll interval, so a
# stuck read cannot stretch the wait past the budget the operator asked for.
TRUST_READ_TIMEOUT=$TRUST_POLL_SECONDS

# What an inspection of the pane found, returned as an exit status.
TRUST_SHOWING=0
TRUST_GONE=1
TRUST_UNREADABLE=2

# pane_read_visible <pane-id> <file> <seconds> - one bounded read of the
# visible screen into <file>. Returns the read's status: 124 when the bound
# fired, 2 when perl is not on PATH, and herdr's own status otherwise.
#
# perl forks herdr as its child and kills that child when the alarm fires,
# exiting 124 itself. The shell therefore never sees a child die by signal,
# which is what makes it print "Alarm clock" on the operator's screen. A
# child that exits normally passes its status through; one that dies by a
# signal is reported as 128 plus the signal number.
pane_read_visible() {
  command -v perl >/dev/null 2>&1 || return 2
  perl -e '
    my $bound = shift;
    my $pid = fork;
    die "fork: $!" unless defined $pid;
    if ($pid == 0) { exec @ARGV or exit 127 }
    $SIG{ALRM} = sub { kill "KILL", $pid; waitpid($pid, 0); exit 124 };
    alarm $bound;
    waitpid($pid, 0);
    alarm 0;
    exit(($? & 127) ? 128 + ($? & 127) : $? >> 8);
  ' "$3" herdr pane read "$1" --source visible --lines 120 >"$2" 2>/dev/null
}

# trust_prompt_showing <pane-id> [seconds] - inspect the pane once.
#
# Returns TRUST_SHOWING while the pane still asks the question, TRUST_GONE when
# a readable pane no longer shows it, and TRUST_UNREADABLE when herdr exits
# non-zero, prints nothing, or does not answer inside the bound. The read's
# status is kept rather than piped into grep, because `grep -q` on an empty
# pipe is indistinguishable from a pane that no longer asks.
#
# The bound is enforced with perl's alarm over a forked child: macOS ships no
# timeout(1) and the harness adds no dependency, while perl is in the macOS
# base system. Output goes to a file rather than through a command
# substitution, because a pipe would keep the shell waiting on a grandchild
# that still holds the write end after the read itself was killed. A perl that
# is not on PATH is reported unreadable rather than read without a bound.
trust_prompt_showing() {
  _tps_pane=$1
  _tps_bound=${2:-$TRUST_READ_TIMEOUT}
  _tps_file=${TMPDIR:-/tmp}/cf-trust-pane.$$

  if ! : >"$_tps_file" 2>/dev/null; then
    unset _tps_pane _tps_bound _tps_file
    return 2
  fi

  # `--source visible` is the screen as it stands. `recent` carries scrollback,
  # where an answered question is still written, so polling that source would
  # keep waiting until the budget ran out on every prompt the operator did
  # answer. The prompt has to be judged gone from the current screen.
  pane_read_visible "$_tps_pane" "$_tps_file" "$_tps_bound" && _tps_read=0 || _tps_read=$?

  if [ "$_tps_read" != 0 ] || [ ! -s "$_tps_file" ]; then
    _tps_seen=$TRUST_UNREADABLE
  elif grep -qF -- "$TRUST_PROMPT_MATCH" "$_tps_file"; then
    _tps_seen=$TRUST_SHOWING
  else
    _tps_seen=$TRUST_GONE
  fi
  rm -f "$_tps_file"

  # sh has no `local`, so the answer is parked in the function's own positional
  # parameters and every temporary is dropped before the return.
  set -- "$_tps_seen"
  unset _tps_pane _tps_bound _tps_file _tps_read _tps_seen
  return "$1"
}

# What a whole wait ended in, returned as an exit status.
TRUST_WAIT_ANSWERED=0
TRUST_WAIT_TIMEOUT=1
TRUST_WAIT_UNREADABLE=2

# wait_for_trust_answer <pane-id> <seconds> - poll until the prompt is gone.
#
# Returns TRUST_WAIT_ANSWERED as soon as a readable pane no longer shows the
# question, TRUST_WAIT_TIMEOUT when the budget ends with the question still on
# screen, and TRUST_WAIT_UNREADABLE when it ends on a pane that could not be
# read. An unreadable pane is treated as "still waiting" while budget remains:
# a pane mid-repaint is common and is not evidence of an answer either way.
#
# The budget is measured against a wall-clock deadline, not against the sleeps,
# so the time a slow read spends cannot be spent twice.
wait_for_trust_answer() {
  _twa_pane=$1
  _twa_deadline=$(($(date +%s) + $2))
  _twa_last=$TRUST_UNREADABLE

  while :; do
    _twa_left=$((_twa_deadline - $(date +%s)))
    _twa_bound=$TRUST_POLL_SECONDS
    [ "$_twa_bound" -le "$_twa_left" ] || _twa_bound=$_twa_left
    [ "$_twa_bound" -ge 1 ] || _twa_bound=1
    trust_prompt_showing "$_twa_pane" "$_twa_bound" && _twa_last=0 || _twa_last=$?

    if [ "$_twa_last" = "$TRUST_GONE" ]; then
      set -- "$TRUST_WAIT_ANSWERED"
      break
    fi
    if [ "$(date +%s)" -ge "$_twa_deadline" ]; then
      if [ "$_twa_last" = "$TRUST_UNREADABLE" ]; then
        set -- "$TRUST_WAIT_UNREADABLE"
      else
        set -- "$TRUST_WAIT_TIMEOUT"
      fi
      break
    fi

    _twa_step=$TRUST_POLL_SECONDS
    _twa_left=$((_twa_deadline - $(date +%s)))
    [ "$_twa_step" -le "$_twa_left" ] || _twa_step=$_twa_left
    if [ "$_twa_step" -gt 0 ]; then sleep "$_twa_step"; fi
  done

  unset _twa_pane _twa_deadline _twa_last _twa_left _twa_bound _twa_step
  return "$1"
}

# agent_pane_and_tab - read a `herdr agent get` reply on stdin, print
# "<pane-id> <tab-id>".
#
# `herdr agent get <name>` prints one JSON object, exit 1 when the name is
# unknown, and on success carries result.agent.pane_id and result.agent.tab_id,
# so the registered session can be compared against the pane this run created.
# Any other shape prints nothing, because a reply that cannot be read proves
# nothing.
agent_pane_and_tab() {
  python3 -c 'import json, sys
try:
    agent = json.load(sys.stdin)["result"]["agent"]
    print("%s %s" % (agent["pane_id"], agent["tab_id"]))
except Exception:
    pass' 2>/dev/null
}

# What resolve_trust_prompt decided, and why.
TRUST_OUTCOME=""
TRUST_REASON=""
TRUST_OWNER=""

TRUST_OWNER_OPERATOR="a human operator, who alone may answer the workspace-trust prompt"
TRUST_OWNER_ENVIRONMENT="operator environment"

# resolve_trust_prompt <pane> <tab> <dir> <budget> <agent> <settings-file>
#
# The whole trust branch of the canary, kept here rather than inline in
# qualify.sh so the self-check can drive it against a stub herdr. Driving it
# through qualify.sh end to end is not possible: reaching this branch needs the
# built binary, the scaffolded samples and the rest of the 25-minute matrix.
#
# Returns 0 only when a live session is proven on this pane and tab. Otherwise
# it returns 1 and leaves TRUST_OUTCOME, TRUST_REASON and TRUST_OWNER for the
# caller to record. TRUST_OUTCOME is one of:
#
#   ready       a live session is proven on this pane; the run may continue
#   disabled    the wait was switched off with --trust-wait-seconds 0
#   unanswered  the budget ended with the question still on screen
#   unreadable  the budget ended without a readable pane
#   unproven    the question cleared but no session could be proven
resolve_trust_prompt() {
  _rtp_pane=$1
  _rtp_tab=$2
  _rtp_dir=$3
  _rtp_budget=$4
  _rtp_agent=$5
  _rtp_settings=$6
  TRUST_OUTCOME=""
  TRUST_REASON=""
  TRUST_OWNER=""

  if [ "$_rtp_budget" -eq 0 ]; then
    TRUST_OUTCOME=disabled
    TRUST_OWNER=$TRUST_OWNER_OPERATOR
    TRUST_REASON="the wait for an answer was disabled by --trust-wait-seconds 0"
  else
    # The operator is watching stdout, not the transcript file, so the ask goes
    # there and names the tab, the pane and the folder being trusted.
    printf '\n%s\n' '=================================================================='
    printf 'ACTION NEEDED: a human must answer the Claude Code trust prompt.\n'
    printf '  Herdr tab:  %s\n' "$_rtp_tab"
    printf '  Herdr pane: %s\n' "$_rtp_pane"
    printf '  sample:     %s\n' "$_rtp_dir"
    printf '  The session in that tab asks whether this is a project you\n'
    printf '  created or one you trust. You have %s seconds to answer it;\n' \
      "$_rtp_budget"
    printf '  the run continues on its own as soon as the prompt is gone.\n'
    printf '%s\n\n' '=================================================================='

    wait_for_trust_answer "$_rtp_pane" "$_rtp_budget" && _rtp_wait=0 || _rtp_wait=$?
    if [ "$_rtp_wait" = "$TRUST_WAIT_TIMEOUT" ]; then
      TRUST_OUTCOME=unanswered
      TRUST_OWNER=$TRUST_OWNER_OPERATOR
      TRUST_REASON="the operator did not answer the trust prompt within $_rtp_budget seconds (--trust-wait-seconds)"
    elif [ "$_rtp_wait" = "$TRUST_WAIT_UNREADABLE" ]; then
      TRUST_OUTCOME=unreadable
      TRUST_OWNER=$TRUST_OWNER_ENVIRONMENT
      TRUST_REASON="the pane could not be read: for $_rtp_budget seconds herdr pane read $_rtp_pane --source visible returned nothing usable, so whether the prompt was answered is unknown"
    else
      resolve_trust_session "$_rtp_pane" "$_rtp_tab" "$_rtp_agent" "$_rtp_settings"
    fi
  fi

  unset _rtp_pane _rtp_tab _rtp_dir _rtp_budget _rtp_agent _rtp_settings _rtp_wait
  [ "$TRUST_OUTCOME" = ready ]
}

# resolve_trust_session <pane> <tab> <agent> <settings-file> - prove a session.
#
# The question cleared, which says nothing about what the human chose: No exits
# the session. Nothing downstream may assume a session until one is proven on
# this pane and tab, either because `herdr agent start` now succeeds or because
# `herdr agent get` names this pane. Matching the agent name alone is not
# enough: the name is a constant, so a leftover agent from an earlier run would
# answer for a session that no longer exists.
resolve_trust_session() {
  _rts_pane=$1
  _rts_tab=$2
  _rts_agent=$3
  _rts_settings=$4
  _rts_cmd="herdr agent start $_rts_agent --kind claude --pane $_rts_pane -- --permission-mode bypassPermissions --settings $_rts_settings"

  if _rts_start=$(herdr agent start "$_rts_agent" --kind claude --pane "$_rts_pane" -- \
    --permission-mode bypassPermissions --settings "$_rts_settings" 2>&1); then
    _rts_status=0
  else
    _rts_status=$?
  fi
  printf '\n$ %s\n[exit %s]\n%s\n' "$_rts_cmd" "$_rts_status" "$_rts_start" >>"$TRANSCRIPT"

  if [ "$_rts_status" = 0 ]; then
    TRUST_OUTCOME=ready
    TRUST_REASON="the trust prompt was answered and the re-issued agent start registered the session"
  else
    _rts_get=$(herdr agent get "$_rts_agent" 2>&1) && _rts_getst=0 || _rts_getst=$?
    printf '\n$ herdr agent get %s\n[exit %s]\n%s\n' \
      "$_rts_agent" "$_rts_getst" "$_rts_get" >>"$TRANSCRIPT"
    _rts_where=""
    if [ "$_rts_getst" = 0 ]; then
      _rts_where=$(printf '%s' "$_rts_get" | agent_pane_and_tab)
    fi
    if [ -n "$_rts_where" ] && [ "$_rts_where" = "$_rts_pane $_rts_tab" ]; then
      TRUST_OUTCOME=ready
      TRUST_REASON="the trust prompt was answered and herdr agent get $_rts_agent reports a session on pane $_rts_pane"
    else
      TRUST_OUTCOME=unproven
      TRUST_OWNER=$TRUST_OWNER_ENVIRONMENT
      TRUST_REASON="the trust prompt cleared but no live session could be proven on pane $_rts_pane and tab $_rts_tab: agent start replied $(oneline "$_rts_start"), and herdr agent get $_rts_agent replied $(oneline "$_rts_get")"
    fi
  fi

  unset _rts_pane _rts_tab _rts_agent _rts_settings _rts_cmd _rts_start \
    _rts_status _rts_get _rts_getst _rts_where
}

# ---------------------------------------------------------------------------
# Delivering an armed prompt to the live pane
# ---------------------------------------------------------------------------
#
# Claude's input box is not settled for a moment after SessionStart records
# ready: text sent then Enter pressed at once leaves the text unsent, so the
# UserPromptSubmit hook never fires. deliver_turn pauses between the text and
# Enter. When a short accepted wait then fails, it presses Enter once more,
# never resending the text, and only when the pane's input line visibly still
# holds the armed prompt, once, as cf-delegate allows. A blind Enter
# could answer whatever dialog is on screen, so no evidence means no Enter.

DELIVER_SETTLE_SECONDS=${DELIVER_SETTLE_SECONDS:-2}
DELIVER_ACCEPT_PROBE_SECONDS=5
DELIVER_MAX_REENTERS=1

# The prompt marker Claude Code paints at the start of its input line.
INPUT_LINE_MARKER='^([[:space:]]|│)*(❯|>)'

# unsent_prompt_showing <pane-id> <prompt-file> - returns 0 only when a
# readable pane's input line, the last visible line that starts with the
# prompt marker, holds the armed prompt: its first 24 characters, or the
# paste attachment Claude shows for a long paste. Earlier marker lines are
# submitted history, so a prompt that was sent never matches. An unreadable
# pane returns non-zero: it is no evidence of unsent text.
unsent_prompt_showing() {
  _ups_file=${TMPDIR:-/tmp}/cf-unsent-pane.$$
  _ups_match=$(head -1 "$2" | cut -c1-24)
  pane_read_visible "$1" "$_ups_file" "$TRUST_READ_TIMEOUT" && _ups_read=0 || _ups_read=$?
  _ups_line=""
  if [ "$_ups_read" = 0 ]; then
    _ups_line=$(LC_ALL=C grep -E "$INPUT_LINE_MARKER" "$_ups_file" | tail -1)
  fi
  rm -f "$_ups_file"
  set -- 1
  if [ -n "$_ups_line" ] && [ -n "$_ups_match" ]; then
    case $_ups_line in
      *"$_ups_match"* | *"[Pasted text"*) set -- 0 ;;
    esac
  fi
  unset _ups_file _ups_match _ups_read _ups_line
  return "$1"
}

# deliver_turn <pane> <prompt-file> <run-id> <state-dir> <turn-id> - returns 0
# once the turn is accepted, non-zero when it was not.
deliver_turn() {
  herdr pane send-text "$1" "$(cat "$2")" >>"$TRANSCRIPT" 2>&1 || true
  sleep "$DELIVER_SETTLE_SECONDS"
  herdr pane send-keys "$1" Enter >>"$TRANSCRIPT" 2>&1 || true
  _dt_reenters=0
  while :; do
    cf delegate wait --run-id "$3" --state-dir "$4" --until accepted \
      --turn-id "$5" --timeout-seconds "$DELIVER_ACCEPT_PROBE_SECONDS"
    [ "$CF_STATUS" = 0 ] && return 0
    [ "$_dt_reenters" -ge "$DELIVER_MAX_REENTERS" ] && return 1
    if ! unsent_prompt_showing "$1" "$2"; then
      printf '\nnot accepted yet, and the input line shows no unsent prompt; no Enter sent\n' \
        >>"$TRANSCRIPT"
      return 1
    fi
    _dt_reenters=$((_dt_reenters + 1))
    printf '\nnot accepted yet, and the input line still holds the prompt; Enter again (%s of %s)\n' \
      "$_dt_reenters" "$DELIVER_MAX_REENTERS" >>"$TRANSCRIPT"
    herdr pane send-keys "$1" Enter >>"$TRANSCRIPT" 2>&1 || true
  done
}

# ---------------------------------------------------------------------------
# Binding the live session to the candidate
# ---------------------------------------------------------------------------
#
# The session's hooks run bare `codeflow`, resolved through the PATH the
# Claude process inherits from its pane shell. The harness PATH never reaches
# a Herdr pane, and that shell's own startup puts any installed codeflow
# first, so the candidate directories are exported in the pane itself after
# its startup. The same command records what `command -v codeflow` resolves
# to there, which is the environment the session and its hooks inherit.

# pane_env_command <path-prefix> <record-file> - the shell line the pane runs.
pane_env_command() {
  printf "export PATH='%s':\"\$PATH\" CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1; command -v codeflow >'%s'; echo TRACKED=\$CLAUDE_CODE_DISABLE_BACKGROUND_TASKS" \
    "$1" "$2"
}

# pane_codeflow_binding <record-file> <candidate> - sets PANE_CF_PATH and
# PANE_CF_SHA, and returns 0 only when the pane resolved codeflow to the
# candidate's own path and bytes.
pane_codeflow_binding() {
  PANE_CF_PATH=$(head -1 "$1" 2>/dev/null || true)
  [ -n "$PANE_CF_PATH" ] || PANE_CF_PATH="nothing recorded"
  if [ -f "$PANE_CF_PATH" ]; then
    PANE_CF_SHA=$(shasum -a 256 "$PANE_CF_PATH" | awk '{print $1}')
  else
    PANE_CF_SHA=none
  fi
  [ "$PANE_CF_PATH" = "$2" ] &&
    [ "$PANE_CF_SHA" = "$(shasum -a 256 "$2" | awk '{print $1}')" ]
}

# tree_digest <dir> - one digest over the content of every file in the sample,
# excluding .git. Porcelain status cannot see a change inside a file that was
# already dirty, so idempotence is judged on content instead.
tree_digest() {
  (
    cd "$1" || exit 1
    find . -name .git -prune -o -type f -print |
      LC_ALL=C sort |
      while IFS= read -r _f; do
        printf '%s  %s\n' "$(shasum -a 256 "$_f" | awk '{print $1}')" "$_f"
      done |
      shasum -a 256 |
      awk '{print $1}'
  )
}

# ---------------------------------------------------------------------------
# Sample construction
# ---------------------------------------------------------------------------

# seed_git <dir> - a repository with an identity and no codeflow state.
seed_git() {
  git init -q -b main "$1"
  git -C "$1" config user.email qualification@example.invalid
  git -C "$1" config user.name "Qualification Harness"
  git -C "$1" config commit.gpgsign false
}

# commit_in <dir> <message> - commit everything, bypassing nothing.
commit_in() {
  git -C "$1" add -A
  git -C "$1" commit -q -m "$2"
}

# build_greenfield_rust <dir>
build_greenfield_rust() {
  _d=$1
  mkdir -p "$_d/src"
  seed_git "$_d"
  cat >"$_d/Cargo.toml" <<'TOML'
[package]
name = "qualification-sample"
version = "0.1.0"
edition = "2021"

[dependencies]
TOML
  cat >"$_d/src/lib.rs" <<'RUST'
//! A minimal library so the sample has a real test target.

/// Add two numbers.
#[must_use]
pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds_two_numbers() {
        assert_eq!(add(2, 2), 4);
    }
}
RUST
  commit_in "$_d" "chore: seed the rust sample"
  printf 'target/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: ignore the build directory"
}

# build_greenfield_node <dir>
build_greenfield_node() {
  _d=$1
  mkdir -p "$_d/src"
  seed_git "$_d"
  cat >"$_d/package.json" <<'JSON'
{
  "name": "qualification-sample",
  "version": "1.0.0",
  "private": true,
  "type": "module",
  "scripts": {
    "test": "node run-tests.mjs"
  }
}
JSON
  # A runner rather than a bare `node --test` line: the shipped single-target
  # template calls `npm test -- --ci`, and the built-in runner rejects an
  # argument it does not know.
  cat >"$_d/run-tests.mjs" <<'JS'
import process from "node:process";
import { run } from "node:test";
import { spec } from "node:test/reporters";

let failed = false;
run({ files: ["src/add.test.js"] })
  .on("test:fail", () => {
    failed = true;
  })
  .compose(new spec())
  .pipe(process.stdout)
  .on("finish", () => {
    process.exitCode = failed ? 1 : 0;
  });
JS
  cat >"$_d/src/add.js" <<'JS'
export function add(left, right) {
  return left + right;
}
JS
  cat >"$_d/src/add.test.js" <<'JS'
import assert from "node:assert/strict";
import { test } from "node:test";
import { add } from "./add.js";

test("adds two numbers", () => {
  assert.equal(add(2, 2), 4);
});
JS
  commit_in "$_d" "chore: seed the node sample"
  printf 'node_modules/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: ignore installed dependencies"
}

# build_brownfield <dir> - a project that already has a README, a CI workflow,
# its own git hooks, and an owner instruction in an AGENTS.md that codeflow
# also manages. Everything here must survive init and update.
build_brownfield() {
  _d=$1
  mkdir -p "$_d/src" "$_d/.github/workflows"
  seed_git "$_d"
  cat >"$_d/README.md" <<'MD'
# Brownfield qualification sample

This project existed before codeflow. Its README, its CI workflow, its git
hooks and the owner instruction in its agent contract are the files the
qualification proves are preserved.

## Build

Run `cargo test`.
MD
  cat >"$_d/.github/workflows/ci.yml" <<'YAML'
name: project ci
on:
  push:
    branches: [main]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --locked
YAML
  cat >"$_d/AGENTS.md" <<'MD'
# Brownfield agent contract

Owner instruction: never edit anything under `vendor/`.
MD
  cat >"$_d/Cargo.toml" <<'TOML'
[package]
name = "brownfield-sample"
version = "0.2.0"
edition = "2021"

[dependencies]
TOML
  cat >"$_d/src/lib.rs" <<'RUST'
//! Pre-existing library code.

/// Multiply two numbers.
#[must_use]
pub fn multiply(left: i32, right: i32) -> i32 {
    left * right
}

#[cfg(test)]
mod tests {
    use super::multiply;

    #[test]
    fn multiplies_two_numbers() {
        assert_eq!(multiply(3, 4), 12);
    }
}
RUST
  printf 'target/\n' >"$_d/.gitignore"
  commit_in "$_d" "chore: seed the brownfield sample"

  # The project's own client hooks, installed the ordinary way. init rewires
  # core.hooksPath, so these files must still be on disk afterwards.
  mkdir -p "$_d/.git/hooks"
  cat >"$_d/.git/hooks/pre-commit" <<'SH'
#!/bin/sh
echo "brownfield project pre-commit hook"
SH
  chmod 0755 "$_d/.git/hooks/pre-commit"
  cat >"$_d/.git/hooks/commit-msg" <<'SH'
#!/bin/sh
echo "brownfield project commit-msg hook"
SH
  chmod 0755 "$_d/.git/hooks/commit-msg"

  cat >"$_d/docs-note.md" <<'MD'
Release note: the 0.2.0 line is maintained from `main`.
MD
  commit_in "$_d" "docs: record the maintained release line"
}

# snapshot_files <dir> <out> <paths...> - SHA-256 of each path, for the
# before/after preservation diff.
snapshot_files() {
  _d=$1
  _out=$2
  shift 2
  : >"$_out"
  for _p in "$@"; do
    if [ -f "$_d/$_p" ]; then
      printf '%s  %s\n' "$(shasum -a 256 "$_d/$_p" | awk '{print $1}')" "$_p" >>"$_out"
    else
      printf '%s  %s\n' "ABSENT" "$_p" >>"$_out"
    fi
  done
}
