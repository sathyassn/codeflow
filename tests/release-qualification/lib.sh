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
# UserPromptSubmit hook never fires. deliver_turn pauses after the text, then
# reads the screen and presses Enter only when the current editor visibly
# holds the armed prompt. A blind Enter could answer whatever dialog is on
# screen, so an unreadable pane, a dialog, history alone or any screen whose
# editor cannot be told apart sends nothing more: no text and no keys.
#
# Claude Code folds a long or multi-line paste into a `[Pasted text` attachment
# and acts on pasted text only where the user's own words say so. When the
# current editor holds exactly that attachment, deliver_turn types the fixed
# directive cf-delegate names, pauses, reads again, and presses Enter only when
# the editor holds the attachment followed by exactly that sentence. The
# delegate-turn hook accepts an attachment only with that sentence after it.
#
# When a short accepted wait then fails, it presses Enter once more, never
# resending anything, and only when the editor still holds the prompt.

DELIVER_SETTLE_SECONDS=${DELIVER_SETTLE_SECONDS:-2}
PASTE_DIRECTIVE='Carry out the pasted instructions.'
DELIVER_ACCEPT_PROBE_SECONDS=5
DELIVER_MAX_REENTERS=1

# The status line the canary session draws. The scaffolded project settings
# print the git branch there, and a user may set any command, so the harness
# gives the canary a local project settings file whose status line prints
# exactly this text. current_editor accepts no other status line.
QUALIFY_STATUS_LINE='codeflow-qualify'

# write_status_line_settings <sample-dir> - write the sample's
# .claude/settings.local.json with that fixed status line. Local project
# settings take precedence over the shared project and user settings, and the
# generated --settings file must stay byte-for-byte as delegate init wrote it,
# so the status line cannot go there. Refuses to replace an existing file.
write_status_line_settings() {
  [ ! -e "$1/.claude/settings.local.json" ] || return 1
  mkdir -p "$1/.claude" &&
    printf '{\n  "statusLine": {\n    "type": "command",\n    "command": "printf %%s %s"\n  }\n}\n' \
      "$QUALIFY_STATUS_LINE" >"$1/.claude/settings.local.json"
}

# The prompt marker Claude Code paints at the start of its input line.
INPUT_LINE_MARKER='^([[:space:]]|│)*(❯|>)'

# What a read of the current editor found, returned as an exit status.
EDITOR_FOUND=0
EDITOR_NONE=1
EDITOR_UNREADABLE=2

# current_editor <pane-id> - one bounded read of the screen, returning
# EDITOR_FOUND and printing the editor's text only when the screen has the
# layout a live Claude Code 2.1.283 pane draws around its editor:
#
#   ──────────────────────────  the next-to-last rule on screen
#   ❯ first editor line         the marker, one space, then text
#     continued editor line     zero or more, each indented two spaces
#   ──────────────────────────  the last rule on screen
#     codeflow-qualify          optional: the canary's own status line
#     ⏸ manual mode on          exactly one known footer line
#
# The footer is recognised, never guessed: after two spaces it is one of the
# lines live 2.1.283 panes showed on 2026-09-26, `⏸ manual mode on` (manual
# mode), `⏵⏵ bypass permissions on (shift+tab to cycle)` (bypassPermissions,
# the canary's mode) or `paste again to expand` (a folded paste, with or
# without the directive after it), optionally followed by the shortcut hints
# ` · ? for shortcuts` and ` · ← for agents` those panes appended. The only
# line allowed above it is QUALIFY_STATUS_LINE, indented two spaces. Any other
# footer or status line, a missing footer or a further line returns
# EDITOR_NONE, as do a second prompt marker in the frame, a body line that is
# not indented, submitted history and a dialog, and every such screen is
# logged by log_refused_screen. The printed text has the marker and the
# two-space indents removed and trailing blanks trimmed. A read that fails,
# times out or prints nothing returns EDITOR_UNREADABLE.
current_editor() {
  _ce_file=${TMPDIR:-/tmp}/cf-current-editor.$$
  pane_read_visible "$1" "$_ce_file" "$TRUST_READ_TIMEOUT" && _ce_read=0 || _ce_read=$?
  if [ "$_ce_read" != 0 ] || [ ! -s "$_ce_file" ]; then
    rm -f "$_ce_file"
    unset _ce_file _ce_read
    return "$EDITOR_UNREADABLE"
  fi
  _ce_text=$(LC_ALL=C awk -v marker="$INPUT_LINE_MARKER" -v status="$QUALIFY_STATUS_LINE" '
    { line[NR] = $0 }
    /^[[:space:]]*(─)+[[:space:]]*$/ { top = bottom; bottom = NR }
    END {
      if (top == 0 || bottom - top < 2) exit 1
      for (i = top + 1; i < bottom; i++) {
        text = line[i]
        sub(/[[:space:]]+$/, "", text)
        if (i == top + 1) {
          if (text !~ /^❯ [^ ]/) exit 1
          sub(/^❯ /, "", text)
        } else {
          if (text ~ marker) exit 1
          if (text != "" && text !~ /^  /) exit 1
          sub(/^  /, "", text)
        }
        body = body text "\n"
      }
      below = 0
      for (i = bottom + 1; i <= NR; i++) {
        text = line[i]
        sub(/[[:space:]]+$/, "", text)
        if (text != "") under[++below] = text
      }
      if (below == 2 && under[1] == "  " status) under[1] = under[2]
      else if (below != 1) exit 1
      if (under[1] !~ /^  (⏸ manual mode on|⏵⏵ bypass permissions on \(shift\+tab to cycle\)|paste again to expand)( · (\? for shortcuts|← for agents))*$/) exit 1
      printf "%s", body
    }
  ' "$_ce_file") && _ce_status=$EDITOR_FOUND || _ce_status=$EDITOR_NONE
  [ "$_ce_status" = "$EDITOR_FOUND" ] || log_refused_screen "$_ce_file"
  rm -f "$_ce_file"
  [ "$_ce_status" = "$EDITOR_FOUND" ] && printf '%s\n' "$_ce_text"
  set -- "$_ce_status"
  unset _ce_file _ce_read _ce_text _ce_status
  return "$1"
}

# log_refused_screen <screen-file> - append the screen current_editor refused
# to the transcript, cut to the frame region: from the next-to-last rule (or
# the only rule) to the bottom of the screen. History above the frame is left
# out, and a screen with no rule is logged only by its line count.
log_refused_screen() {
  {
    printf '\ncurrent_editor refused this screen (frame region only):\n'
    LC_ALL=C awk '
      { line[NR] = $0 }
      /^[[:space:]]*(─)+[[:space:]]*$/ { top = bottom; bottom = NR }
      END {
        start = top ? top : bottom
        if (!start) { printf "  (no rule on screen; %d lines not logged)\n", NR; exit }
        for (i = start; i <= NR; i++) print "  | " line[i]
      }
    ' "$1"
  } >>"${TRANSCRIPT:-/dev/null}"
}

# editor_holds <editor text> <prompt-file> - classify what the editor holds:
# `attachment` for exactly one folded-paste attachment, `directive` for that
# attachment followed by exactly the directive, `prompt` when the editor's
# text is the armed prompt apart from whitespace (the editor wraps long lines
# and indents continuations), and `other` for anything else, an empty or
# placeholder editor included.
editor_holds() {
  _eh_rest=$(printf '%s\n' "$1" |
    LC_ALL=C sed -E 's/^\[Pasted text #[0-9]+ \+[0-9]+ lines\]//')
  _eh_lines=$(printf '%s\n' "$1" | wc -l | tr -d ' ')
  if [ "$_eh_lines" = 1 ] && [ "$_eh_rest" != "$1" ] && [ -z "$_eh_rest" ]; then
    echo attachment
  elif [ "$_eh_lines" = 1 ] && [ "$_eh_rest" != "$1" ] && [ "$_eh_rest" = "$PASTE_DIRECTIVE" ]; then
    echo directive
  elif [ -n "$1" ] &&
    [ "$(printf '%s' "$1" | LC_ALL=C tr -d '[:space:]')" = "$(LC_ALL=C tr -d '[:space:]' <"$2")" ]; then
    echo prompt
  else
    echo other
  fi
  unset _eh_rest _eh_lines
}

# unsent_prompt_showing <pane-id> <prompt-file> - returns 0 only when the
# current editor still holds the armed prompt, its attachment, or the
# attachment with the directive. History, dialogs and unreadable panes are no
# evidence of unsent text.
unsent_prompt_showing() {
  _ups_text=$(current_editor "$1") || { unset _ups_text; return 1; }
  case $(editor_holds "$_ups_text" "$2") in
    prompt | attachment | directive) unset _ups_text; return 0 ;;
  esac
  unset _ups_text
  return 1
}

# deliver_stop <reason> - record why nothing more was sent, and fail.
deliver_stop() {
  printf '\n%s; no further text or keys sent\n' "$1" >>"$TRANSCRIPT"
  return 1
}

# deliver_turn <pane> <prompt-file> <run-id> <state-dir> <turn-id> - returns 0
# once the turn is accepted, non-zero when it was not.
deliver_turn() {
  herdr pane send-text "$1" "$(cat "$2")" >>"$TRANSCRIPT" 2>&1 || true
  sleep "$DELIVER_SETTLE_SECONDS"
  _dt_text=$(current_editor "$1") && _dt_read=0 || _dt_read=$?
  case $_dt_read in
    "$EDITOR_UNREADABLE") deliver_stop 'the pane could not be read after the paste'; return 1 ;;
    "$EDITOR_NONE") deliver_stop 'no current editor is visible after the paste'; return 1 ;;
  esac
  case $(editor_holds "$_dt_text" "$2") in
    prompt) ;;
    attachment)
      printf '\nthe editor holds a paste attachment; typing the directive\n' >>"$TRANSCRIPT"
      herdr pane send-text "$1" "$PASTE_DIRECTIVE" >>"$TRANSCRIPT" 2>&1 || true
      sleep "$DELIVER_SETTLE_SECONDS"
      _dt_text=$(current_editor "$1") && _dt_read=0 || _dt_read=$?
      if [ "$_dt_read" != 0 ] || [ "$(editor_holds "$_dt_text" "$2")" != directive ]; then
        deliver_stop 'the editor does not show the attachment followed by the directive'
        return 1
      fi
      ;;
    *) deliver_stop 'the current editor does not hold the armed prompt'; return 1 ;;
  esac
  herdr pane send-keys "$1" Enter >>"$TRANSCRIPT" 2>&1 || true
  _dt_reenters=0
  while :; do
    cf delegate wait --run-id "$3" --state-dir "$4" --until accepted \
      --turn-id "$5" --timeout-seconds "$DELIVER_ACCEPT_PROBE_SECONDS"
    [ "$CF_STATUS" = 0 ] && return 0
    [ "$_dt_reenters" -ge "$DELIVER_MAX_REENTERS" ] && return 1
    if ! unsent_prompt_showing "$1" "$2"; then
      printf '\nnot accepted yet, and the current editor shows no unsent prompt; no Enter sent\n' \
        >>"$TRANSCRIPT"
      return 1
    fi
    _dt_reenters=$((_dt_reenters + 1))
    printf '\nnot accepted yet, and the current editor still holds the prompt; Enter again (%s of %s)\n' \
      "$_dt_reenters" "$DELIVER_MAX_REENTERS" >>"$TRANSCRIPT"
    herdr pane send-keys "$1" Enter >>"$TRANSCRIPT" 2>&1 || true
  done
}

# ---------------------------------------------------------------------------
# Waiting for a backgrounded workflow
# ---------------------------------------------------------------------------
#
# Claude Code normally launches a Workflow in the background: the turn that
# invoked it stops at once, and a later turn, prompted by the task
# notification, writes the workflow's result. The first Stop is therefore not
# the end of the pipeline. The result file the prompt asks for is the proof,
# so the harness polls for it with the session left open, bounded by the row
# timeout. A file is taken only once it is non-empty and unchanged across one
# further poll, so a half-written file is never read.

PIPELINE_POLL_SECONDS=${PIPELINE_POLL_SECONDS:-5}

# wait_for_pipeline_result <result-file> <turn-result.json> <seconds> - returns
# 0 once the result file is present and stable, 3 when the turn itself recorded
# a terminal failure (StopFailure) and no result file exists, and 124 when the
# bound passes first.
wait_for_pipeline_result() {
  _wpr_deadline=$(($(date +%s) + $3))
  _wpr_last=""
  while :; do
    if [ -s "$1" ]; then
      _wpr_now=$(cksum <"$1")
      [ "$_wpr_now" = "$_wpr_last" ] && return 0
      _wpr_last=$_wpr_now
    elif [ -f "$2" ] &&
      grep -q '"status"[[:space:]]*:[[:space:]]*"failed"' "$2" 2>/dev/null; then
      return 3
    fi
    if [ "$(date +%s)" -ge "$_wpr_deadline" ]; then
      [ -s "$1" ] && return 0
      return 124
    fi
    sleep "$PIPELINE_POLL_SECONDS"
  done
}

# ---------------------------------------------------------------------------
# What the live session's own transcript shows
# ---------------------------------------------------------------------------

# session_transcript <state-dir> - print the Claude Code transcript of the
# session the delegate turns recorded, or nothing when it cannot be located.
# The turn's own result.json names the session, and that session's transcript
# records every tool call it made. The transcript is written by the harness,
# not by the session under test, so it is the one piece of evidence here that
# a peer cannot author.
session_transcript() {
  _sid=$(python3 -c 'import glob,json,sys
found = ""
for path in sorted(glob.glob(sys.argv[1] + "/turns/*/result.json")):
    try:
        doc = json.load(open(path, encoding="utf-8"))
    except Exception:
        continue
    if doc.get("session_id"):
        found = doc["session_id"]
print(found)' "$1" 2>/dev/null)
  [ -n "$_sid" ] || return 0
  find "$HOME/.claude/projects" -maxdepth 2 -name "$_sid.jsonl" 2>/dev/null | head -1
}

# Did the session actually invoke the native Workflow tool on the scaffolded
# pipeline? Prints yes, no, or unknown, where unknown means no transcript could
# be located and is never read as no.
workflow_invocation_evidence() {
  _tx=$(session_transcript "$1")
  if [ -z "$_tx" ]; then
    printf 'unknown'
    return 0
  fi
  if grep -q '"name"[[:space:]]*:[[:space:]]*"Workflow"' "$_tx" 2>/dev/null &&
    grep -q 'pipeline.workflow' "$_tx" 2>/dev/null; then
    printf 'yes'
  else
    printf 'no'
  fi
}

# workflow_launch_evidence <state-dir> - how the Workflow call was run: its run
# id and whether Claude Code launched it in the background, read from the tool
# result the session's transcript recorded for that call.
workflow_launch_evidence() {
  _tx=$(session_transcript "$1")
  if [ -z "$_tx" ]; then
    printf 'workflow launch unknown: no session transcript'
    return 0
  fi
  python3 -c 'import json,sys
calls, launches = set(), []
for line in open(sys.argv[1], encoding="utf-8"):
    try:
        entry = json.loads(line)
    except Exception:
        continue
    content = (entry.get("message") or {}).get("content")
    if not isinstance(content, list):
        continue
    for block in content:
        if not isinstance(block, dict):
            continue
        if block.get("type") == "tool_use" and block.get("name") == "Workflow":
            calls.add(block.get("id"))
        elif block.get("type") == "tool_result" and block.get("tool_use_id") in calls:
            result = entry.get("toolUseResult")
            result = result if isinstance(result, dict) else {}
            mode = "in the background" if result.get("status") == "async_launched" else "in the foreground"
            launches.append("workflow run %s launched %s" % (result.get("runId") or "without a run id", mode))
if launches:
    print("; ".join(launches))
elif calls:
    print("workflow launch unknown: the Workflow call has no recorded result")
else:
    print("workflow launch: no Workflow call recorded")' "$_tx" 2>/dev/null ||
    printf 'workflow launch unknown: the transcript could not be read'
}

# pipeline_result_shape - read one pipeline result object on stdin and print
# `complete` only when it is the object the driver returns for a successful
# run: status exactly `complete`, a positive attempts count, a trail with build
# and verify stages, and an approved final verify verdict. Anything else prints
# the first reason it is not.
pipeline_result_shape() {
  python3 -c 'import json,sys
raw = sys.stdin.read().strip()
if not raw:
    print("absent"); raise SystemExit
try:
    doc = json.loads(raw)
except Exception:
    print("unparsable"); raise SystemExit
if not isinstance(doc, dict):
    print("not-an-object"); raise SystemExit
status = doc.get("status")
if status == "unavailable":
    print("unavailable"); raise SystemExit
trail = doc.get("trail")
if status != "complete":
    print("status-" + str(status)); raise SystemExit
if not isinstance(trail, list) or not trail:
    print("no-trail"); raise SystemExit
if not isinstance(doc.get("attempts"), int) or doc["attempts"] < 1:
    print("no-attempts"); raise SystemExit
entries = [e for e in trail if isinstance(e, dict)]
stages = [e.get("stage") for e in entries]
if "build" not in stages or "verify" not in stages:
    print("trail-missing-stages"); raise SystemExit
verify = [e for e in entries if e.get("stage") == "verify"]
if not verify or verify[-1].get("verdict") != "approved":
    print("verify-not-approved"); raise SystemExit
print("complete")' 2>/dev/null || printf 'unparsable'
}

# workflow_task_evidence <state-dir> <result-file> - the independent proof.
# Claude Code writes each backgrounded task's own output file, named in the
# task notice it submits, and the model never authors it. This prints what that
# file says the pipeline workflow returned (status, stages, verify verdict) and
# whether the model's result file holds the same object, and returns 0 only
# when the task output is a successful run and both objects are equal.
workflow_task_evidence() {
  _tx=$(session_transcript "$1")
  if [ -z "$_tx" ]; then
    printf 'task output unknown: no session transcript'
    return 1
  fi
  python3 - "$_tx" "$2" <<'PY'
import json, re, sys

transcript, result_file = sys.argv[1], sys.argv[2]
calls, task_id, notices = set(), None, []
for line in open(transcript, encoding="utf-8"):
    try:
        entry = json.loads(line)
    except Exception:
        continue
    if entry.get("type") == "queue-operation" and isinstance(entry.get("content"), str):
        notices.append(entry["content"])
    content = (entry.get("message") or {}).get("content")
    if isinstance(content, str):
        notices.append(content)
    if not isinstance(content, list):
        continue
    for block in content:
        if not isinstance(block, dict):
            continue
        if block.get("type") == "tool_use" and block.get("name") == "Workflow" and \
                "pipeline.workflow" in json.dumps(block.get("input")):
            calls.add(block.get("id"))
        elif block.get("type") == "tool_result" and block.get("tool_use_id") in calls:
            result = entry.get("toolUseResult")
            if isinstance(result, dict) and result.get("taskId"):
                task_id = result["taskId"]
if not task_id:
    print("task output unknown: no backgrounded pipeline Workflow task in the transcript")
    raise SystemExit(1)
output_file = None
for notice in notices:
    if "<task-id>%s</task-id>" % task_id in notice:
        match = re.search(r"<output-file>([^<\n]+)</output-file>", notice)
        if match:
            output_file = match.group(1)
if not output_file:
    print("task %s output unknown: no task notice named its output file" % task_id)
    raise SystemExit(1)
try:
    task = json.load(open(output_file, encoding="utf-8"))["result"]
except Exception as error:
    print("task %s output unreadable at %s: %s" % (task_id, output_file, error))
    raise SystemExit(1)
trail = task.get("trail") if isinstance(task, dict) else None
entries = [e for e in trail if isinstance(e, dict)] if isinstance(trail, list) else []
stages = [e.get("stage") for e in entries]
verify = [e.get("verdict") for e in entries if e.get("stage") == "verify"]
verdict = verify[-1] if verify else None
successful = isinstance(task, dict) and task.get("status") == "complete" and \
    isinstance(task.get("attempts"), int) and task["attempts"] >= 1 and \
    "build" in stages and "verify" in stages and verdict == "approved"
try:
    claimed = json.loads(open(result_file, encoding="utf-8").read().strip())
except Exception:
    claimed = None
agrees = claimed == task
print("Claude Code task %s output: status %s, stages %s, verify verdict %s; %s the result file" % (
    task_id, task.get("status") if isinstance(task, dict) else None,
    "/".join(str(s) for s in stages) or "none", verdict,
    "agrees with" if agrees else "does not agree with"))
raise SystemExit(0 if successful and agrees else 1)
PY
}

# pipeline_checkout <sample-dir> <branch> <spare-dir> - print the directory
# holding the branch the pipeline built on: its existing worktree, or a
# detached checkout of the branch made at <spare-dir>. Returns 1 when the
# branch does not exist.
pipeline_checkout() {
  _pc_tree=$(git -C "$1" worktree list --porcelain 2>/dev/null | awk -v ref="branch refs/heads/$2" '
    /^worktree / { path = substr($0, 10) }
    $0 == ref { print path; exit }')
  if [ -n "$_pc_tree" ]; then
    printf '%s' "$_pc_tree"
    return 0
  fi
  git -C "$1" rev-parse --verify --quiet "refs/heads/$2" >/dev/null || return 1
  git -C "$1" worktree add --detach "$3" "$2" >/dev/null 2>&1 || return 1
  printf '%s' "$3"
}

# ---------------------------------------------------------------------------
# Ending the live session
# ---------------------------------------------------------------------------
#
# Herdr has no command that stops an agent: an agent ends when its process
# exits or its pane closes. Closing the pane would also remove the tab this
# run created, so teardown asks Claude Code itself to exit, with the double
# Ctrl-C it answers at its prompt, and confirms through `pane process-info`
# that the pane's foreground is its own shell again before the tab is closed.

STOP_AGENT_ROUNDS=3
STOP_AGENT_SETTLE_SECONDS=${STOP_AGENT_SETTLE_SECONDS:-2}

# pane_at_shell <pane> - returns 0 when the pane's foreground process group is
# its shell, meaning no agent is running in it.
pane_at_shell() {
  herdr pane process-info --pane "$1" 2>/dev/null | python3 -c 'import json,sys
info = json.load(sys.stdin)["result"]["process_info"]
sys.exit(0 if info["foreground_process_group_id"] == info["shell_pid"] else 1)' 2>/dev/null
}

# stop_pane_agent <pane> - returns 0 once the pane is back at its shell, 1
# when the agent is still running after every round.
stop_pane_agent() {
  _spa_round=0
  while [ "$_spa_round" -lt "$STOP_AGENT_ROUNDS" ]; do
    pane_at_shell "$1" && return 0
    herdr pane send-keys "$1" ctrl+c ctrl+c >/dev/null 2>&1 || true
    sleep "$STOP_AGENT_SETTLE_SECONDS"
    _spa_round=$((_spa_round + 1))
  done
  pane_at_shell "$1"
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
