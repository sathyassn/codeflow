#!/bin/sh
# Self-checks for the release-qualification harness itself.
#
# The matrix run takes about 25 minutes and needs a live Claude session, so the
# harness's own logic is proved here instead, against stub commands on PATH.
# Every check is a negative control paired with its positive: a helper that
# only ever succeeds proves nothing, so each case that must succeed is run
# beside a case that must fail.
#
# The trust branch is exercised through the real helpers in lib.sh, not a copy
# of them. It cannot be driven through qualify.sh end to end: reaching that
# branch needs the built binary, the scaffolded samples and the rest of the
# matrix, so the branch lives in lib.sh as resolve_trust_prompt and qualify.sh
# calls it. The last check pins that call site, so moving the branch back
# inline breaks this script.
#
#   sh tests/release-qualification/self-check.sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$SCRIPT_DIR/lib.sh"

CHECKS=0
FAILED=0

# ok <name> <condition-description> <0|1 outcome>
ok() {
  CHECKS=$((CHECKS + 1))
  if [ "$3" = 0 ]; then
    printf 'pass  %s: %s\n' "$1" "$2"
  else
    printf 'FAIL  %s: %s\n' "$1" "$2"
    FAILED=$((FAILED + 1))
  fi
}

STUB_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cf-qualify-self-check.XXXXXX")
trap 'rm -rf "$STUB_DIR"' EXIT HUP INT TERM
PATH="$STUB_DIR:$PATH"
export PATH

# resolve_trust_session appends to the transcript the run keeps.
TRANSCRIPT="$STUB_DIR/transcript"
: >"$TRANSCRIPT"

# A single herdr stub on PATH, driven by files rather than rewritten per case,
# so every case runs the same script the earlier ones did:
#   calls       every invocation, one line each
#   pane-text   what `pane read` prints
#   pane-rc     the status `pane read` exits with
#   pane-delay  seconds `pane read` sleeps first, to model a stuck read
#   start-out / start-rc   the reply and status of `agent start`
#   get-out / get-rc       the reply and status of `agent get`
#   enters / accept-at     Enters sent so far, and the one that is accepted
#   proc-out               what `pane process-info` prints; with exit-on-keys
#                          present, any send-keys swaps in proc-shell
#   pane-text-directive    swapped into pane-text when the directive is typed
#   pane-text-after-down   swapped into pane-text when the key is `down`
#   pane-text-after-enter  swapped into pane-text on any other send-keys
#   proc-cwd               the session's working directory, as `lsof` prints
#                          it; proc-cwd-after-enter is swapped in on Enter
{
  printf '#!/bin/sh\n'
  printf 'D=%s\n' "$STUB_DIR"
  printf 'printf "%%s\\n" "$*" >>"$D/calls"\n'
  printf 'case "$1 $2" in\n'
  printf '  "pane read")\n'
  printf '    sleep "$(cat "$D/pane-delay")"\n'
  printf '    cat "$D/pane-text"\n'
  printf '    exit "$(cat "$D/pane-rc")" ;;\n'
  printf '  "agent start") cat "$D/start-out"; exit "$(cat "$D/start-rc")" ;;\n'
  printf '  "agent get") cat "$D/get-out"; exit "$(cat "$D/get-rc")" ;;\n'
  printf '  "pane process-info") cat "$D/proc-out"; exit 0 ;;\n'
  printf '  "pane send-text")\n'
  printf '    case "$*" in *"Carry out the pasted instructions."*)\n'
  printf '      [ -f "$D/pane-text-directive" ] && cp "$D/pane-text-directive" "$D/pane-text" ;;\n'
  printf '    esac\n'
  printf '    exit 0 ;;\n'
  printf '  "pane send-keys")\n'
  printf '    case "$*" in *" down")\n'
  printf '      [ -f "$D/pane-text-after-down" ] && cp "$D/pane-text-after-down" "$D/pane-text"\n'
  printf '      exit 0 ;;\n'
  printf '    esac\n'
  printf '    [ -f "$D/exit-on-keys" ] && cp "$D/proc-shell" "$D/proc-out"\n'
  printf '    [ -f "$D/proc-cwd-after-enter" ] && cp "$D/proc-cwd-after-enter" "$D/proc-cwd"\n'
  printf '    [ -f "$D/pane-text-after-enter" ] && cp "$D/pane-text-after-enter" "$D/pane-text"\n'
  printf '    n=$(($(cat "$D/enters") + 1)); printf "%%s" "$n" >"$D/enters"\n'
  printf '    [ "$n" -lt "$(cat "$D/accept-at")" ] || : >"$D/accepted"\n'
  printf '    exit 0 ;;\n'
  printf 'esac\n'
  printf 'exit 0\n'
} >"$STUB_DIR/herdr"
chmod 0755 "$STUB_DIR/herdr"

# An lsof stub for the session's working directory, printed in the field
# format lsof -Fn uses. The session's process id is beyond any pid_max, so
# process_cwd never finds it under /proc and always asks this stub.
{
  printf '#!/bin/sh\n'
  printf 'D=%s\n' "$STUB_DIR"
  printf 'printf "p%%s\\nfcwd\\nn%%s\\n" 99999999 "$(cat "$D/proc-cwd")"\n'
} >"$STUB_DIR/lsof"
chmod 0755 "$STUB_DIR/lsof"

# stub_herdr <pane text> - reset the stub: that pane text, a readable pane, an
# agent start that refuses and an agent get that finds nothing.
stub_herdr() {
  : >"$STUB_DIR/calls"
  printf '%s\n' "$1" >"$STUB_DIR/pane-text"
  printf '0' >"$STUB_DIR/pane-rc"
  printf '0' >"$STUB_DIR/pane-delay"
  printf '1' >"$STUB_DIR/start-rc"
  printf 'pane is busy\n' >"$STUB_DIR/start-out"
  printf '1' >"$STUB_DIR/get-rc"
  printf 'agent target not found\n' >"$STUB_DIR/get-out"
  printf '0' >"$STUB_DIR/enters"
  printf '99' >"$STUB_DIR/accept-at"
  rm -f "$STUB_DIR/accepted" "$STUB_DIR/exit-on-keys" \
    "$STUB_DIR/pane-text-directive" "$STUB_DIR/pane-text-after-enter" \
    "$STUB_DIR/pane-text-after-down" "$STUB_DIR/proc-cwd-after-enter"
  printf '%s' /nonexistent >"$STUB_DIR/proc-cwd"
  printf '%s\n' "$PROC_AGENT" >"$STUB_DIR/proc-out"
  printf '%s\n' "$PROC_SHELL" >"$STUB_DIR/proc-shell"
}

# The `pane process-info` replies for a pane running an agent and for one back
# at its shell: only the foreground process group differs.
PROC_AGENT='{"result":{"process_info":{"foreground_process_group_id":99999999,"shell_pid":100},"type":"pane_process_info"}}'
PROC_SHELL='{"result":{"process_info":{"foreground_process_group_id":100,"shell_pid":100},"type":"pane_process_info"}}'

# agent_get_json <pane> <tab> - the reply shape `herdr agent get` prints.
agent_get_json() {
  printf '{"id":"cli:agent:get","result":{"agent":{"name":"cf-selfcheck-cl01",'
  printf '"pane_id":"%s","tab_id":"%s"},"type":"agent_info"}}\n' "$1" "$2"
}

# run_resolve <budget> - drive the real trust branch, capturing the operator
# banner so it can be inspected instead of printed over the check output.
run_resolve() {
  resolve_trust_prompt p1 t1 /tmp/sample "$1" cf-selfcheck-cl01 \
    /tmp/state/settings.json >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
}

TRUSTED_PANE='> Welcome to Claude Code
  cwd: /tmp/sample
  Ready for input.'

BLOCKED_PANE='Do you trust the files in this folder?

  Is this a project you created or one you trust?

  1. Yes, proceed
  2. No, exit'

# ---------------------------------------------------------------------------
# trust_prompt_showing reads the pane through herdr, and fails closed
# ---------------------------------------------------------------------------

stub_herdr "$BLOCKED_PANE"
trust_prompt_showing p1 && _r=0 || _r=1
ok trust_prompt_showing "true while the pane asks the question" "$_r"
ok trust_prompt_showing.command \
  "reads the visible screen, not scrollback" \
  "$(grep -qF 'pane read p1 --source visible --lines 120' "$STUB_DIR/calls" && echo 0 || echo 1)"

stub_herdr "$TRUSTED_PANE"
trust_prompt_showing p1 && _r=1 || _r=0
ok trust_prompt_showing "false once the question is gone" "$_r"

stub_herdr "$TRUSTED_PANE"
trust_prompt_showing p1 && _s=0 || _s=$?
ok trust_prompt_showing.gone \
  "a readable pane without the question is TRUST_GONE, not unreadable" \
  "$([ "$_s" = "$TRUST_GONE" ] && echo 0 || echo 1)"

# Negative control for the fail-closed rule: herdr refusing must never read as
# an answered prompt.
stub_herdr "$BLOCKED_PANE"
printf '1' >"$STUB_DIR/pane-rc"
trust_prompt_showing p1 && _s=0 || _s=$?
ok trust_prompt_showing.unreadable \
  "a non-zero herdr exit is TRUST_UNREADABLE" \
  "$([ "$_s" = "$TRUST_UNREADABLE" ] && echo 0 || echo 1)"

stub_herdr "$TRUSTED_PANE"
: >"$STUB_DIR/pane-text"
trust_prompt_showing p1 && _s=0 || _s=$?
ok trust_prompt_showing.empty \
  "an empty pane read is TRUST_UNREADABLE" \
  "$([ "$_s" = "$TRUST_UNREADABLE" ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# wait_for_trust_answer, the negative control and its positive
# ---------------------------------------------------------------------------

# Positive: an already-answered pane returns at once, well inside the budget.
stub_herdr "$TRUSTED_PANE"
_began=$(date +%s)
wait_for_trust_answer p1 3 && _r=0 || _r=1
_took=$(($(date +%s) - _began))
ok wait_for_trust_answer "returns 0 when the prompt is not showing" "$_r"
ok wait_for_trust_answer \
  "returns promptly, in $_took s of a 3 s budget" \
  "$([ "$_took" -lt 2 ] && echo 0 || echo 1)"

# Negative control: a pane that keeps asking must time out, and must not
# overrun the budget it was given.
stub_herdr "$BLOCKED_PANE"
_began=$(date +%s)
wait_for_trust_answer p1 3 && _r=1 || _r=0
_took=$(($(date +%s) - _began))
ok wait_for_trust_answer "returns 1 when the prompt keeps showing" "$_r"
ok wait_for_trust_answer \
  "times out after the budget, in $_took s of a 3 s budget" \
  "$([ "$_took" -ge 3 ] && [ "$_took" -lt 8 ] && echo 0 || echo 1)"

# A pane that never reads must end in its own code, not in the answered one.
stub_herdr "$BLOCKED_PANE"
printf '1' >"$STUB_DIR/pane-rc"
wait_for_trust_answer p1 3 && _s=0 || _s=$?
ok wait_for_trust_answer.unreadable \
  "ends in TRUST_WAIT_UNREADABLE when the pane never reads" \
  "$([ "$_s" = "$TRUST_WAIT_UNREADABLE" ] && echo 0 || echo 1)"

# A read that hangs must be cut short, so the wall-clock budget still holds.
stub_herdr "$BLOCKED_PANE"
printf '30' >"$STUB_DIR/pane-delay"
_began=$(date +%s)
wait_for_trust_answer p1 3 && _s=0 || _s=$?
_took=$(($(date +%s) - _began))
ok wait_for_trust_answer.bounded \
  "a hanging pane read is bounded, ending in $_took s of a 3 s budget" \
  "$([ "$_took" -lt 10 ] && echo 0 || echo 1)"
ok wait_for_trust_answer.bounded \
  "a timed-out read counts as unreadable, not as a clear" \
  "$([ "$_s" = "$TRUST_WAIT_UNREADABLE" ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# resolve_trust_prompt: the branch qualify.sh runs, against the stub herdr
# ---------------------------------------------------------------------------

# Answered, and the re-issued agent start registers the session.
stub_herdr "$TRUSTED_PANE"
printf '0' >"$STUB_DIR/start-rc"
run_resolve 3
ok resolve_trust_prompt.ready "answered and registered by agent start" \
  "$([ "$RESOLVE_RC" = 0 ] && [ "$TRUST_OUTCOME" = ready ] && echo 0 || echo 1)"
ok resolve_trust_prompt.banner \
  "the operator banner names the tab, the pane and the folder" \
  "$(grep -qF 'ACTION NEEDED' "$STUB_DIR/banner" &&
     grep -qF 'Herdr tab:  t1' "$STUB_DIR/banner" &&
     grep -qF 'Herdr pane: p1' "$STUB_DIR/banner" &&
     grep -qF '/tmp/sample' "$STUB_DIR/banner" && echo 0 || echo 1)"

# Answered, agent start refuses, but agent get names this pane and tab.
stub_herdr "$TRUSTED_PANE"
printf '0' >"$STUB_DIR/get-rc"
agent_get_json p1 t1 >"$STUB_DIR/get-out"
run_resolve 3
ok resolve_trust_prompt.ready "answered and proven by agent get on this pane" \
  "$([ "$RESOLVE_RC" = 0 ] && [ "$TRUST_OUTCOME" = ready ] && echo 0 || echo 1)"

# Negative control for that proof: the same agent name on a different pane is
# a leftover from an earlier run, and proves nothing about this one.
stub_herdr "$TRUSTED_PANE"
printf '0' >"$STUB_DIR/get-rc"
agent_get_json p9 t9 >"$STUB_DIR/get-out"
run_resolve 3
ok resolve_trust_prompt.unproven "agent get on another pane does not prove a session" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = unproven ] && echo 0 || echo 1)"

# Answered, and nothing registered at all.
stub_herdr "$TRUSTED_PANE"
run_resolve 3
ok resolve_trust_prompt.unproven "answered but no session provable" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = unproven ] && echo 0 || echo 1)"
ok resolve_trust_prompt.unproven "the reason says no session could be proven" \
  "$(printf '%s' "$TRUST_REASON" | grep -qF 'no live session could be proven' && echo 0 || echo 1)"
ok resolve_trust_prompt.unproven "the row has an owner, so it can be recorded unavailable" \
  "$([ -n "$TRUST_OWNER" ] && echo 0 || echo 1)"

# The pane never reads: unavailable for the pane's reason, and no session is
# claimed, so agent start is never even attempted.
stub_herdr "$BLOCKED_PANE"
printf '1' >"$STUB_DIR/pane-rc"
run_resolve 3
ok resolve_trust_prompt.unreadable "an unreadable pane is not an answer" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = unreadable ] && echo 0 || echo 1)"
ok resolve_trust_prompt.unreadable "the reason says the pane could not be read" \
  "$(printf '%s' "$TRUST_REASON" | grep -qF 'the pane could not be read' && echo 0 || echo 1)"
ok resolve_trust_prompt.unreadable "no session is started on an unreadable pane" \
  "$(grep -qF 'agent start' "$STUB_DIR/calls" && echo 1 || echo 0)"

# The question stays on screen for the whole budget.
stub_herdr "$BLOCKED_PANE"
run_resolve 3
ok resolve_trust_prompt.unanswered "an unanswered prompt names the operator" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = unanswered ] &&
     [ "$TRUST_OWNER" = "$TRUST_OWNER_OPERATOR" ] && echo 0 || echo 1)"

# The operator wait switched off: say so, ask nobody, send nothing. The one
# read that remains is the check for this run's own sample.
stub_herdr "$BLOCKED_PANE"
run_resolve 0
ok resolve_trust_prompt.disabled "a zero budget says the wait was disabled by the flag" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = disabled ] &&
     printf '%s' "$TRUST_REASON" | grep -qF -- '--trust-wait-seconds 0' && echo 0 || echo 1)"
ok resolve_trust_prompt.disabled "a zero budget neither asks the operator nor sends a key" \
  "$([ ! -s "$STUB_DIR/banner" ] && ! grep -qF 'send-keys' "$STUB_DIR/calls" && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# The trust prompt for this run's own sample is the harness's to answer
# ---------------------------------------------------------------------------
#
# The screens follow a live Claude Code 2.1.283 capture of the dialog: the
# folder under "Accessing workspace:", wrapped at the pane width, the cursor on
# "No, exit". Every folder below exists, so only identity decides. The sample
# is named the way qualify.sh names one: lowercase words and a hex nonce.

STUB_REAL=$(CDPATH= cd -P -- "$STUB_DIR" && pwd -P)
NONCE=0123456789abcdef
OWN="$STUB_REAL/greenfield-rust-full-$NONCE"
SIBLING="$STUB_REAL/greenfield-rust-full-0123456789abcdee"
ELSEWHERE="$STUB_REAL/other-folder"
NL='
'
TAB=$(printf '\t')
# Codex's three foreign folders (TSK-083 review round 1): a trailing blank,
# a newline and a tab in the name, each once read as the sample.
TRAILING="$OWN "
NEWLINE="$STUB_REAL/greenfield-rust-full-01234567${NL}89abcdef"
TABBED="$OWN$TAB-other"
mkdir -p "$OWN" "$SIBLING" "$ELSEWHERE" "$TRAILING" "$NEWLINE" "$TABBED"
ln -s "$ELSEWHERE" "$STUB_REAL/link-elsewhere"
ln -s "$OWN" "$STUB_REAL/link-own"

# trust_screen <folder> <no|yes> [width] - the dialog as the pane shows it.
trust_screen() {
  _ts_path=$1
  printf '%s\n' '────────────────────────────────────────────────────────────'
  printf ' Accessing workspace:\n\n'
  while [ -n "$_ts_path" ]; do
    _ts_line=$(printf '%s' "$_ts_path" | cut -c1-"${3:-400}")
    printf ' %s\n' "$_ts_line"
    _ts_path=${_ts_path#"$_ts_line"}
  done
  printf '\n Quick safety check: Is this a project you created or one\n'
  printf ' you trust? (Like your own code, a well-known open source\n'
  printf ' project, or work from your team). If not, take a moment to\n'
  printf " review what's in this folder first.\n\n"
  printf " Claude Code'll be able to read, edit, and execute files\n here.\n\n"
  printf ' Security guide\n\n'
  if [ "$2" = no ]; then
    printf ' ❯ No, exit\n   Yes, I trust this folder\n'
  else
    printf '   No, exit\n ❯ Yes, I trust this folder\n'
  fi
  printf '\n Enter to confirm · Esc to cancel\n'
  unset _ts_path _ts_line
}

# A pane narrower than the question wraps it; it is still the question.
stub_herdr "$(trust_screen "$OWN" no 60)"
trust_prompt_showing p1 && _r=0 || _r=$?
ok trust_prompt_showing.wrapped "a question wrapped by a narrow pane is still showing" \
  "$([ "$_r" = "$TRUST_SHOWING" ] && echo 0 || echo 1)"

# own_stub <folder shown> <cursor after down> [width] [session folder] [sample]
# - set the stub up: the dialog names <folder shown>, the session runs in
# <session folder> (the shown folder by default), this run's sample is
# <sample> ($OWN by default).
own_stub() {
  stub_herdr "$(trust_screen "$1" no "${3:-}")"
  trust_screen "$1" "$2" "${3:-}" >"$STUB_DIR/pane-text-after-down"
  printf '%s\n' "$TRUSTED_PANE" >"$STUB_DIR/pane-text-after-enter"
  printf '0' >"$STUB_DIR/start-rc"
  printf '%s' "${4:-$1}" >"$STUB_DIR/proc-cwd"
  OWN_SAMPLE=${5:-$OWN}
}

# run_own <budget> <own_stub arguments> - drive the real trust branch.
run_own() {
  _ro_budget=$1
  shift
  own_stub "$@"
  resolve_trust_prompt p1 t1 "$OWN_SAMPLE" "$_ro_budget" cf-selfcheck-cl01 \
    /tmp/state/settings.json >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
  unset _ro_budget
}

# keys_sent - the keys the harness pressed, in order, on one line.
keys_sent() {
  sed -n 's/^pane send-keys p1 //p' "$STUB_DIR/calls" | tr '\n' ' '
}

run_own 0 "$OWN" yes
ok trust_own.answered "the prompt for this run's own sample is answered with no operator wait" \
  "$([ "$RESOLVE_RC" = 0 ] && [ "$TRUST_OUTCOME" = ready ] &&
     [ "$(keys_sent)" = "down Enter " ] && echo 0 || echo 1)"
ok trust_own.answered "the harness says it answered, and why; no operator banner" \
  "$(printf '%s' "$TRUST_ANSWERED_BY" | grep -qF "own sample $OWN" &&
     grep -qF "trust prompt answered by the harness" "$STUB_DIR/banner" &&
     ! grep -qF 'ACTION NEEDED' "$STUB_DIR/banner" && echo 0 || echo 1)"

run_own 0 "$OWN" yes 60
ok trust_own.wrapped "a path wrapped at a 60-column pane is still read as the sample" \
  "$([ "$RESOLVE_RC" = 0 ] && [ "$(keys_sent)" = "down Enter " ] && echo 0 || echo 1)"

# Negative controls: each must leave the pane without a single key and hand
# the prompt to the operator with a reason that says so. The session runs in
# the folder the dialog names, except where the case says otherwise.
#   symlink-own    the dialog shows a link to the sample, not its real path
#   trailing-blank a sibling whose name ends in a blank
#   newline        a folder whose name holds a newline; wrapped, its screen
#                  reads exactly as the sample, and only the session's
#                  working directory tells them apart
#   tab            a folder whose name holds a tab
#   session-elsewhere  the screen names the sample, the session runs elsewhere
#   no-nonce, short-nonce, upper-case  a sample path the harness would not
#                  have named, answered by nobody but the operator
mkdir -p "$STUB_REAL/greenfield-rust-full" "$STUB_REAL/greenfield-rust-full-0123abcd" \
  "$STUB_REAL/Greenfield-rust-full-$NONCE"
while IFS='|' read -r _name _shown _width _session _sample; do
  run_own 3 "$_shown" yes "$_width" "$_session" "$_sample"
  ok "trust_other.$_name" "a prompt that is not provably this run's own sample gets no key and waits for the operator" \
    "$([ "$RESOLVE_RC" != 0 ] && [ "$(keys_sent)" = "" ] &&
       [ "$TRUST_OUTCOME" = unanswered ] && [ "$TRUST_OWNER" = "$TRUST_OWNER_OPERATOR" ] &&
       [ -z "$TRUST_ANSWERED_BY" ] && grep -qF 'ACTION NEEDED' "$STUB_DIR/banner" &&
       printf '%s' "$TRUST_REASON" | grep -qF "did not name this run's own sample" &&
       echo 0 || echo 1)"
done <<CASES
sibling|$SIBLING||$SIBLING|
parent|$STUB_REAL||$STUB_REAL|
symlink-elsewhere|$STUB_REAL/link-elsewhere||$ELSEWHERE|
symlink-own|$STUB_REAL/link-own||$OWN|
missing|$STUB_REAL/not-there||$STUB_REAL/not-there|
prefix|$OWN-b||$OWN-b|
trailing-blank|$TRAILING||$TRAILING|
tab|$TABBED||$TABBED|
session-elsewhere|$OWN||$SIBLING|
no-nonce|$STUB_REAL/greenfield-rust-full||$STUB_REAL/greenfield-rust-full|$STUB_REAL/greenfield-rust-full
short-nonce|$STUB_REAL/greenfield-rust-full-0123abcd||$STUB_REAL/greenfield-rust-full-0123abcd|$STUB_REAL/greenfield-rust-full-0123abcd
upper-case|$STUB_REAL/Greenfield-rust-full-$NONCE||$STUB_REAL/Greenfield-rust-full-$NONCE|$STUB_REAL/Greenfield-rust-full-$NONCE
CASES
unset _name _shown _width _session _sample

# The newline folder needs its own case: its name cannot sit on one line of
# the table above. Its screen is the sample's own, wrapped where the name
# breaks, so the screen alone would pass.
run_own 3 "$OWN" yes "" "$NEWLINE"
ok trust_other.newline "a folder whose name holds a newline, shown exactly as the sample, gets no key" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$(keys_sent)" = "" ] &&
     [ "$TRUST_OUTCOME" = unanswered ] && [ -z "$TRUST_ANSWERED_BY" ] && echo 0 || echo 1)"

# The dialog parser: an exact path or nothing.
own_stub "$OWN" yes
trust_dialog "$STUB_DIR/pane-text-after-down" >"$STUB_DIR/dialog" && _r=0 || _r=1
ok trust_dialog.exact "the parsed path is the sample's real path and the cursor is on yes" \
  "$([ "$_r" = 0 ] && [ "$(sed -n 1p "$STUB_DIR/dialog")" = "$OWN" ] &&
     [ "$(sed -n 2p "$STUB_DIR/dialog")" = yes ] && echo 0 || echo 1)"
for _bad in "$TRAILING" "$TABBED" "$STUB_REAL/sample with space-$NONCE"; do
  trust_screen "$_bad" yes >"$STUB_DIR/screen"
  trust_dialog "$STUB_DIR/screen" >/dev/null && _r=1 || _r=0
  ok trust_dialog.refused "a path line with a blank, tab or other character outside the set is refused" "$_r"
done
unset _bad

# The cursor did not reach "Yes" after the move: no Enter is pressed.
run_own 3 "$OWN" no
ok trust_own.no_enter "Enter is pressed only with the cursor on Yes" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$(keys_sent)" = "down " ] && echo 0 || echo 1)"

# The screen after the move names another folder: no Enter either.
own_stub "$OWN" yes
trust_screen "$SIBLING" yes >"$STUB_DIR/pane-text-after-down"
resolve_trust_prompt p1 t1 "$OWN" 3 cf-selfcheck-cl01 /tmp/state/settings.json \
  >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
ok trust_own.changed "a dialog that changes folder between reads gets no Enter" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$(keys_sent)" = "down " ] && echo 0 || echo 1)"

# The accepted read-to-key gap: the screen and the session change after the
# final read, so Enter goes out. The after-answer check must then stop the run.
own_stub "$OWN" yes
printf '%s' "$SIBLING" >"$STUB_DIR/proc-cwd-after-enter"
resolve_trust_prompt p1 t1 "$OWN" 3 cf-selfcheck-cl01 /tmp/state/settings.json \
  >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
ok trust_own.race "a change after the final read is caught after the answer and stops the run" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$(keys_sent)" = "down Enter " ] &&
     [ "$TRUST_OUTCOME" = stopped ] && [ "$TRUST_OWNER" = "$TRUST_OWNER_HARNESS" ] &&
     printf '%s' "$TRUST_REASON" | grep -qF 'not running in that sample' &&
     ! grep -qF 'agent start' "$STUB_DIR/calls" && echo 0 || echo 1)"

# The session is gone after the answer (a No, or a crash): stop the run.
own_stub "$OWN" yes
: >"$STUB_DIR/exit-on-keys"
resolve_trust_prompt p1 t1 "$OWN" 3 cf-selfcheck-cl01 /tmp/state/settings.json \
  >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
ok trust_own.exited "a session gone after the answer stops the run" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = stopped ] &&
     printf '%s' "$TRUST_REASON" | grep -qF 'no session in the foreground' && echo 0 || echo 1)"

# Answered, but the dialog never clears: the run stops.
own_stub "$OWN" yes
rm -f "$STUB_DIR/pane-text-after-enter"
_saved=$TRUST_SELF_ANSWER_SECONDS
TRUST_SELF_ANSWER_SECONDS=2
resolve_trust_prompt p1 t1 "$OWN" 3 cf-selfcheck-cl01 /tmp/state/settings.json \
  >"$STUB_DIR/banner" 2>&1 && RESOLVE_RC=0 || RESOLVE_RC=$?
TRUST_SELF_ANSWER_SECONDS=$_saved
ok trust_own.stuck "an answered prompt that stays on screen stops the run, owned by the harness" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = stopped ] &&
     [ "$TRUST_OWNER" = "$TRUST_OWNER_HARNESS" ] && echo 0 || echo 1)"
unset _saved

# Mutation probe: with the session check removed, the newline folder whose
# screen reads as the sample is answered, so that check is what stops it.
MUTANT_ID="$STUB_DIR/lib-identity.sh"
sed 's#^    pane_session_in "\$1" "\$_tdn_real" >/dev/null; then$#    true; then#' \
  "$SCRIPT_DIR/lib.sh" >"$MUTANT_ID"
run_own 3 "$OWN" yes "" "$NEWLINE"
_real_keys=$(keys_sent)
_mutant_keys=$(sh -c '. "$1"; TRANSCRIPT=$2/transcript; resolve_trust_prompt p1 t1 "$3" 3 cf-selfcheck-cl01 /tmp/s.json >/dev/null 2>&1
  sed -n "s/^pane send-keys p1 //p" "$2/calls" | tr "\n" " "' sh "$MUTANT_ID" "$STUB_DIR" "$OWN" 2>/dev/null)
ok mutation.identity "without the session check a folder that only looks like the sample would be answered" \
  "$([ "$(grep -c '^    true; then$' "$MUTANT_ID")" = 1 ] &&
     [ "$_real_keys" = "" ] && [ -n "$_mutant_keys" ] && echo 0 || echo 1)"
unset _real_keys _mutant_keys

# ---------------------------------------------------------------------------
# Mutation probe
# ---------------------------------------------------------------------------
#
# Remove the fail-closed guard from a copy of lib.sh. The unreadable pane above
# must then read as an answered one, which is the bug this branch exists to
# stop. If this probe stops reporting the mutant as broken, the checks above
# have stopped testing anything.
#
# The equivalent one-liner, run from the repository root:
#   sed 's#^  if \[ "\$_tps_read" != 0 \].*#  if false; then#' \
#     tests/release-qualification/lib.sh >/tmp/mutant.sh &&
#     sh tests/release-qualification/self-check.sh   # with lib.sh replaced

MUTANT="$STUB_DIR/lib-mutated.sh"
sed 's#^  if \[ "\$_tps_read" != 0 \].*#  if false; then#' \
  "$SCRIPT_DIR/lib.sh" >"$MUTANT"
ok mutation.applied "the probe actually removes the guard" \
  "$([ "$(grep -c '^  if false; then$' "$MUTANT")" = 1 ] && echo 0 || echo 1)"

# A read that fails while printing a screen with no question on it is the
# case the guard exists for: herdr said nothing usable, the text alone says
# "no prompt". The real guard returns unreadable; the mutant reads it as gone.
stub_herdr "$TRUSTED_PANE"
printf '1' >"$STUB_DIR/pane-rc"
_real_rc=$(sh -c '. "$1"; trust_prompt_showing p1 2 >/dev/null 2>&1; echo $?' sh "$SCRIPT_DIR/lib.sh")
_mutant_rc=$(sh -c '. "$1"; trust_prompt_showing p1 2 >/dev/null 2>&1; echo $?' sh "$MUTANT")
ok mutation.control "with the guard the same failed read is unreadable" \
  "$([ "$_real_rc" = "$TRUST_UNREADABLE" ] && echo 0 || echo 1)"
ok mutation.probe "without the guard an unreadable pane reads as answered" \
  "$([ "$_mutant_rc" = "$TRUST_GONE" ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# deliver_turn: an Enter only for a positively identified editor
# ---------------------------------------------------------------------------
#
# The stub herdr counts Enters and writes `accepted` on the one named in
# accept-at; the stub candidate answers `delegate wait` with 0 once that file
# exists and with the timeout exit 124 until then. `pane read` prints
# pane-text. Typing the directive swaps in pane-text-directive and an Enter
# swaps in pane-text-after-enter, when those files exist, so a case can model
# the screen changing. The screens follow a live Claude Code 2.1.283 pane: the
# editor sits between two rules with its footer below.

{
  printf '#!/bin/sh\n'
  printf 'D=%s\n' "$STUB_DIR"
  printf 'printf "codeflow %%s\\n" "$*" >>"$D/calls"\n'
  printf '[ -f "$D/accepted" ] && exit 0\n'
  printf 'echo "timed out"; exit 124\n'
} >"$STUB_DIR/cf-stub"
chmod 0755 "$STUB_DIR/cf-stub"
# shellcheck disable=SC2034 # read by cf() in lib.sh
BINARY="$STUB_DIR/cf-stub"
printf 'Reply with exactly: ok. Do not edit any file.' >"$STUB_DIR/prompt.txt"

# The positive screens are the layouts live Claude Code 2.1.283 panes showed
# through `herdr pane read --source visible` on 2026-09-26, in manual and in
# bypassPermissions mode: a full-width rule, the editor, a rule, then one
# footer line, with any notice right-aligned above the frame. Only the rule
# width is shortened here, and the prompt text is this check's own.
RULE='────────────────────────────────────────────────────────────'

# The idle editor: its placeholder, which holds no prompt.
IDLE_PANE="
$RULE
❯ Try \"fix lint errors\"
$RULE
  ⏸ manual mode on · ? for shortcuts · ← for agents"

# A short multi-line paste Claude did not fold: continuations indented.
MULTILINE_PANE="$RULE
❯ line one of a probe
  line two of a probe
  line three
$RULE
  ⏸ manual mode on"

# The prompt in the current editor, below an earlier turn.
UNSENT_PANE="> earlier question
● earlier answer
$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
  ⏸ manual mode on"

# The same prompt submitted: history now, and the editor shows its placeholder.
SENT_PANE="❯ Reply with exactly: ok. Do not edit any file.
● Working
$RULE
❯ Try \"fix lint errors\"
$RULE
  ⏸ manual mode on · ? for shortcuts"

# A long paste Claude folded into an attachment, under the usage notice the
# live pane showed, and the same editor once the directive is typed after it.
PASTED_PANE="                                                  You've used 94% of your limit
$RULE
❯ [Pasted text #1 +12 lines]
$RULE
  paste again to expand"
DIRECTIVE_PANE="$RULE
❯ [Pasted text #1 +12 lines]Carry out the pasted instructions.
$RULE
  paste again to expand"

# The idle editor in bypassPermissions mode, the mode the canary runs in.
BYPASS_IDLE_PANE="                                                  ◐ medium · /effort
$RULE
❯ Try \"create a util logging.py that...\"
$RULE
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents"

# An unfolded multi-line paste in bypassPermissions mode.
BYPASS_MULTILINE_PANE="$RULE
❯ probe line 1 with some more words
  probe line 2 with some more words
$RULE
  ⏵⏵ bypass permissions on (shift+tab to cycle)"

# The canary's layouts with the harness's fixed status line, captured live on
# 2026-09-26 in bypassPermissions mode from a folder whose shared project
# settings print the git branch as the status line, as the scaffolded sample
# does, and whose local settings came from write_status_line_settings: the
# marker replaced the branch. These panes drew U+00A0 after the marker, and
# NBSP keeps that byte sequence here.
NBSP=$(printf '\302\240')
STATUS_IDLE_PANE="⚠ 3 MCP servers need authentication · run /mcp
$RULE
❯${NBSP}Try \"refactor <filepath>\"
$RULE
  codeflow-qualify
  ⏵⏵ bypass permissions on (shift+tab to cycle) · ← for agents"
STATUS_UNSENT_PANE="⚠ 3 MCP servers need authentication · run /mcp
$RULE
❯${NBSP}Reply with exactly: ok. Do not edit any file.
$RULE
  codeflow-qualify
  ⏵⏵ bypass permissions on (shift+tab to cycle)"
STATUS_PASTED_PANE="$RULE
❯${NBSP}[Pasted text #1 +13 lines]
$RULE
  codeflow-qualify
  paste again to expand"
STATUS_DIRECTIVE_PANE="$RULE
❯${NBSP}[Pasted text #1 +13 lines]Carry out the pasted instructions.
$RULE
  codeflow-qualify
  paste again to expand"

# frame_under <lines below the frame> - the prompt in a well-formed frame over
# the given lines, for the status line near misses below.
frame_under() {
  printf '%s\n%s\n%s\n%s' "$RULE" '❯ Reply with exactly: ok. Do not edit any file.' "$RULE" "$1"
}
BYPASS_FOOTER='  ⏵⏵ bypass permissions on (shift+tab to cycle)'

# frame_with_footer <footer line> - the prompt in a well-formed frame over the
# given footer, for the footer near misses below.
frame_with_footer() {
  printf '%s\n%s\n%s\n%s' "$RULE" '❯ Reply with exactly: ok. Do not edit any file.' "$RULE" "$1"
}

# Two attachments: herdr delivered one long text as two pastes.
TWO_ATTACHMENTS_PANE="$RULE
❯ [Pasted text #1 +15 lines][Pasted text #2 +14 lines]
$RULE
  paste again to expand"

# Negative controls. None of these is an editor, whatever its first line says.

# An attachment that is only history: no editor is drawn below it.
HISTORY_PASTE_PANE='❯ [Pasted text #1 +12 lines]
● Working on earlier request'

# A dialog: one rule above it, and its own marker on an option.
DIALOG_PANE="$RULE
 Accessing workspace:

 Quick safety check: Is this a project you created or one you trust?

 ❯ No, exit
   Yes, I trust this folder

 Enter to confirm · Esc to cancel"

# A marker line below the frame.
AMBIGUOUS_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
 ❯ 1. Yes"

# A frame with a trust dialog below it instead of a footer.
DIALOG_BELOW_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
$BLOCKED_PANE"

# Answer text inside the frame, and no footer.
HISTORY_IN_FRAME_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
● Working on earlier request
$RULE"

# A second marker inside the frame, and dialog text below it.
MARKER_IN_FRAME_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
 ❯ 1. Yes
$RULE
  Enter to confirm · Esc to cancel"

# A well-formed frame with nothing below it.
NO_FOOTER_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE"

# Three lines below the frame: more than a footer.
LONG_FOOTER_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
  ⏸ manual mode on
  ⎿ earlier output
  ⎿ more output"

# A footer that is not indented.
FLUSH_FOOTER_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
Continue? (y/n)"

# The prompt with more text after it in the editor.
EXTRA_TEXT_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
  Then push the branch.
$RULE
  ⏸ manual mode on"

# run_deliver <accept-at> <pane text> [directive pane] [pane after Enter]
run_deliver() {
  stub_herdr "$2"
  printf '%s' "$1" >"$STUB_DIR/accept-at"
  rm -f "$STUB_DIR/pane-text-directive" "$STUB_DIR/pane-text-after-enter"
  [ -z "${3:-}" ] || printf '%s\n' "$3" >"$STUB_DIR/pane-text-directive"
  [ -z "${4:-}" ] || printf '%s\n' "$4" >"$STUB_DIR/pane-text-after-enter"
  deliver_turn p1 "$STUB_DIR/prompt.txt" run1 /tmp/state t1 && DELIVER_RC=0 || DELIVER_RC=$?
  DELIVER_ENTERS=$(cat "$STUB_DIR/enters")
  DELIVER_WAITS=$(grep -c '^codeflow delegate wait' "$STUB_DIR/calls" || true)
  DELIVER_READS=$(grep -c '^pane read' "$STUB_DIR/calls" || true)
  DELIVER_TEXTS=$(grep -c '^pane send-text' "$STUB_DIR/calls" || true)
  DELIVER_DIRECTIVES=$(grep -c '^pane send-text p1 Carry out the pasted instructions\.$' \
    "$STUB_DIR/calls" || true)
}

# current_editor: the three results, each against its opposite.
# found_case <description> <pane text> <expected editor text>
found_case() {
  stub_herdr "$2"
  _text=$(current_editor p1) && _r=0 || _r=$?
  ok current_editor.found "$1" \
    "$([ "$_r" = "$EDITOR_FOUND" ] && [ "$_text" = "$3" ] && echo 0 || echo 1)"
}
found_case 'the live idle layout is an editor holding its placeholder' \
  "$IDLE_PANE" 'Try "fix lint errors"'
found_case 'the live unfolded multi-line layout is an editor, indents removed' \
  "$MULTILINE_PANE" "$(printf 'line one of a probe\nline two of a probe\nline three')"
found_case 'the prompt in the editor below history, marker removed' \
  "$UNSENT_PANE" 'Reply with exactly: ok. Do not edit any file.'
found_case 'the live folded layout under a notice is an editor holding the attachment' \
  "$PASTED_PANE" '[Pasted text #1 +12 lines]'
found_case 'the live directive layout is an editor holding the attachment and sentence' \
  "$DIRECTIVE_PANE" '[Pasted text #1 +12 lines]Carry out the pasted instructions.'
found_case 'the live bypassPermissions idle layout is an editor' \
  "$BYPASS_IDLE_PANE" 'Try "create a util logging.py that..."'
found_case 'the live bypassPermissions multi-line layout is an editor' \
  "$BYPASS_MULTILINE_PANE" "$(printf 'probe line 1 with some more words\nprobe line 2 with some more words')"
found_case 'the live idle layout with the fixed status line is an editor' \
  "$STATUS_IDLE_PANE" 'Try "refactor <filepath>"'
found_case 'the live prompt layout with the fixed status line is an editor' \
  "$STATUS_UNSENT_PANE" 'Reply with exactly: ok. Do not edit any file.'
found_case 'the live folded layout with the fixed status line is an editor' \
  "$STATUS_PASTED_PANE" '[Pasted text #1 +13 lines]'
found_case 'the live directive layout with the fixed status line is an editor' \
  "$STATUS_DIRECTIVE_PANE" '[Pasted text #1 +13 lines]Carry out the pasted instructions.'
found_case 'the manual mode footer alone is recognised' \
  "$(frame_with_footer '  ⏸ manual mode on')" 'Reply with exactly: ok. Do not edit any file.'

# no_editor_case <description> <pane text>
no_editor_case() {
  stub_herdr "$2"
  current_editor p1 >/dev/null && _r=0 || _r=$?
  ok current_editor.none "$1 has no current editor" \
    "$([ "$_r" = "$EDITOR_NONE" ] && echo 0 || echo 1)"
}
no_editor_case 'an attachment only in history' "$HISTORY_PASTE_PANE"
no_editor_case 'a dialog' "$DIALOG_PANE"
no_editor_case 'a marker line after the editor' "$AMBIGUOUS_PANE"
no_editor_case 'a frame with a trust dialog below it' "$DIALOG_BELOW_PANE"
no_editor_case 'a frame holding answer text and no footer' "$HISTORY_IN_FRAME_PANE"
no_editor_case 'a frame holding a second marker' "$MARKER_IN_FRAME_PANE"
no_editor_case 'a frame with no footer' "$NO_FOOTER_PANE"
no_editor_case 'a frame with three lines below it' "$LONG_FOOTER_PANE"
no_editor_case 'a frame with an unindented line below it' "$FLUSH_FOOTER_PANE"
no_editor_case 'a screen with no rules' "$TRUSTED_PANE"
# Footer near misses: indented like a footer, but not one Claude Code draws.
no_editor_case 'a frame over "Continue? (y/n)"' "$(frame_with_footer '  Continue? (y/n)')"
no_editor_case 'a frame over "Press Enter to continue"' "$(frame_with_footer '  Press Enter to continue')"
no_editor_case 'a frame over "Working on earlier request"' \
  "$(frame_with_footer '  Working on earlier request')"
no_editor_case 'a frame over a mode line with extra text' \
  "$(frame_with_footer '  ⏸ manual mode on · Press Enter to continue')"
no_editor_case 'a frame over the paste hint indented three spaces' \
  "$(frame_with_footer '   paste again to expand')"
no_editor_case 'a frame over two known footer lines' \
  "$(frame_with_footer "$(printf '  ⏸ manual mode on\n  paste again to expand')")"
# Status line near misses: only the harness's own marker, above the footer.
no_editor_case 'a frame over the branch status line of rerun C' \
  "$(frame_under "$(printf '  main\n%s' "$BYPASS_FOOTER")")"
no_editor_case 'a frame over the marker with extra text' \
  "$(frame_under "$(printf '  codeflow-qualify main\n%s' "$BYPASS_FOOTER")")"
no_editor_case 'a frame over the marker not indented' \
  "$(frame_under "$(printf 'codeflow-qualify\n%s' "$BYPASS_FOOTER")")"
no_editor_case 'a frame over the marker below the footer' \
  "$(frame_under "$(printf '%s\n  codeflow-qualify' "$BYPASS_FOOTER")")"
no_editor_case 'a frame over the marker and no footer' "$(frame_under '  codeflow-qualify')"
no_editor_case 'a frame over the marker twice' \
  "$(frame_under "$(printf '  codeflow-qualify\n  codeflow-qualify\n%s' "$BYPASS_FOOTER")")"
no_editor_case 'a frame over the marker and an unknown footer' \
  "$(frame_under "$(printf '  codeflow-qualify\n  Continue? (y/n)')")"
# Separator near misses: the marker takes one ASCII space or one U+00A0.
# frame_with_marker <marker and separator> - the prompt after them in a frame.
frame_with_marker() {
  printf '%s\n%sReply with exactly: ok.\n%s\n%s' "$RULE" "$1" "$RULE" "$BYPASS_FOOTER"
}
no_editor_case 'a marker followed by a tab' "$(frame_with_marker "$(printf '❯\t')")"
no_editor_case 'a marker followed by U+202F' "$(frame_with_marker "$(printf '❯\342\200\257')")"
no_editor_case 'a marker followed by two U+00A0' "$(frame_with_marker "❯$NBSP$NBSP")"
no_editor_case 'a marker followed by U+00A0 and a space' "$(frame_with_marker "❯$NBSP ")"
no_editor_case 'a marker followed by a space and U+00A0' "$(frame_with_marker "❯ $NBSP")"
no_editor_case 'a marker with no separator' "$(frame_with_marker '❯')"

# log_refused_screen: a refusal leaves the frame region in the transcript, and
# only that; an accepted screen leaves nothing.
: >"$TRANSCRIPT"
stub_herdr "$(printf 'secret history line\n%s' "$(frame_under "$(printf '  main\n%s' "$BYPASS_FOOTER")")")"
current_editor p1 >/dev/null || true
ok log_refused_screen "a refused screen is logged from its frame down" \
  "$(grep -qF 'current_editor refused this screen (frame region only):' "$TRANSCRIPT" &&
     grep -qF '  | ❯ Reply with exactly: ok. Do not edit any file.' "$TRANSCRIPT" &&
     grep -qF '  |   main' "$TRANSCRIPT" && echo 0 || echo 1)"
ok log_refused_screen "the history above the frame is not logged" \
  "$(grep -qF 'secret history line' "$TRANSCRIPT" && echo 1 || echo 0)"
: >"$TRANSCRIPT"
stub_herdr "$(printf 'secret history line\nno frame here')"
current_editor p1 >/dev/null || true
ok log_refused_screen "a screen with no rule is logged only by its line count" \
  "$(grep -qF '(no rule on screen; 2 lines not logged)' "$TRANSCRIPT" &&
     ! grep -qF 'secret history line' "$TRANSCRIPT" && echo 0 || echo 1)"
: >"$TRANSCRIPT"
stub_herdr "$STATUS_UNSENT_PANE"
current_editor p1 >/dev/null
ok log_refused_screen "an accepted screen is not logged" \
  "$([ ! -s "$TRANSCRIPT" ] && echo 0 || echo 1)"

# write_status_line_settings: the file the canary's status line comes from.
mkdir -p "$STUB_DIR/sample"
write_status_line_settings "$STUB_DIR/sample" && _r=0 || _r=$?
ok write_status_line_settings "writes a local settings file whose status line prints the marker" \
  "$([ "$_r" = 0 ] && python3 -c 'import json,sys
d = json.load(open(sys.argv[1]))
assert d == {"statusLine": {"type": "command", "command": "printf %s codeflow-qualify"}}
' "$STUB_DIR/sample/.claude/settings.local.json" && echo 0 || echo 1)"
ok write_status_line_settings "the marker command prints exactly the marker" \
  "$([ "$(sh -c 'printf %s codeflow-qualify')" = "$QUALIFY_STATUS_LINE" ] && echo 0 || echo 1)"
printf 'owner file\n' >"$STUB_DIR/sample/.claude/settings.local.json"
write_status_line_settings "$STUB_DIR/sample" && _r=0 || _r=$?
ok write_status_line_settings "refuses to replace an existing local settings file" \
  "$([ "$_r" != 0 ] && [ "$(cat "$STUB_DIR/sample/.claude/settings.local.json")" = 'owner file' ] && echo 0 || echo 1)"

stub_herdr "$UNSENT_PANE"
printf '1' >"$STUB_DIR/pane-rc"
current_editor p1 >/dev/null && _r=0 || _r=$?
ok current_editor.unreadable "a failed read is unreadable, never an editor or its absence" \
  "$([ "$_r" = "$EDITOR_UNREADABLE" ] && echo 0 || echo 1)"
stub_herdr ''
: >"$STUB_DIR/pane-text"
current_editor p1 >/dev/null && _r=0 || _r=$?
ok current_editor.unreadable "an empty read is unreadable" \
  "$([ "$_r" = "$EDITOR_UNREADABLE" ] && echo 0 || echo 1)"

# editor_holds: what an identified editor holds, each against a near miss.
printf 'line one of a probe\nline two of a probe\nline three' >"$STUB_DIR/multi.txt"
# holds_case <description> <editor text> <prompt file> <expected>
holds_case() {
  ok editor_holds "$1" "$([ "$(editor_holds "$2" "$3")" = "$4" ] && echo 0 || echo 1)"
}
holds_case 'the whole multi-line prompt is the prompt' \
  "$(printf 'line one of a probe\nline two of a probe\nline three')" "$STUB_DIR/multi.txt" prompt
holds_case 'a prompt wrapped at another place is still the prompt' \
  "$(printf 'line one of a\nprobe line two of a probe\nline three')" "$STUB_DIR/multi.txt" prompt
holds_case 'only its first line is not the prompt' \
  'line one of a probe' "$STUB_DIR/multi.txt" other
holds_case 'the prompt with more text is not the prompt' \
  "$(printf 'Reply with exactly: ok. Do not edit any file.\nThen push the branch.')" \
  "$STUB_DIR/prompt.txt" other
holds_case 'the placeholder is not the prompt' 'Try "fix lint errors"' "$STUB_DIR/prompt.txt" other
holds_case 'one attachment is the attachment' '[Pasted text #1 +12 lines]' "$STUB_DIR/prompt.txt" attachment
holds_case 'the attachment and the directive are the directive' \
  '[Pasted text #1 +12 lines]Carry out the pasted instructions.' "$STUB_DIR/prompt.txt" directive
holds_case 'the directive with more text is neither' \
  '[Pasted text #1 +12 lines]Carry out the pasted instructions. Now.' "$STUB_DIR/prompt.txt" other
holds_case 'two attachments are neither' \
  '[Pasted text #1 +15 lines][Pasted text #2 +14 lines]' "$STUB_DIR/prompt.txt" other
holds_case 'two attachments and the directive are neither' \
  '[Pasted text #1 +15 lines][Pasted text #2 +14 lines]Carry out the pasted instructions.' \
  "$STUB_DIR/prompt.txt" other
holds_case 'an attachment with a second line is neither' \
  "$(printf '[Pasted text #1 +12 lines]\nmore')" "$STUB_DIR/prompt.txt" other

# Accepted on the first Enter, after the default settle pause.
_began=$(date +%s)
run_deliver 1 "$UNSENT_PANE"
_took=$(($(date +%s) - _began))
ok deliver_turn.first "the prompt in the editor: accepted on one Enter after one read" \
  "$([ "$DELIVER_RC" = 0 ] && [ "$DELIVER_ENTERS" = 1 ] && [ "$DELIVER_READS" = 1 ] && echo 0 || echo 1)"
ok deliver_turn.settle "waits the 2 s settle between the text and the read, took $_took s" \
  "$([ "$_took" -ge 2 ] && echo 0 || echo 1)"
ok deliver_turn.order "sends the text, reads the editor, then the first Enter" \
  "$(sed -n '1p' "$STUB_DIR/calls" | grep -qF 'pane send-text p1 Reply with exactly: ok.' &&
     sed -n '2p' "$STUB_DIR/calls" | grep -qF 'pane read p1 --source visible' &&
     sed -n '3p' "$STUB_DIR/calls" | grep -qF 'pane send-keys p1 Enter' && echo 0 || echo 1)"
ok deliver_turn.no_directive "a prompt shown as text gets no directive" \
  "$([ "$DELIVER_TEXTS" = 1 ] && echo 0 || echo 1)"
ok deliver_turn.probe "probes acceptance of this turn for 5 s" \
  "$(grep -qF 'delegate wait --run-id run1 --state-dir /tmp/state --until accepted --turn-id t1 --timeout-seconds 5' \
     "$STUB_DIR/calls" && echo 0 || echo 1)"

# shellcheck disable=SC2034 # read by deliver_turn in lib.sh
DELIVER_SETTLE_SECONDS=0

# The prompt still in the editor after the first Enter: one re-Enter lands it.
run_deliver 2 "$UNSENT_PANE"
ok deliver_turn.reenter "the prompt still in the editor gets one re-Enter, then acceptance" \
  "$([ "$DELIVER_RC" = 0 ] && [ "$DELIVER_ENTERS" = 2 ] && [ "$DELIVER_WAITS" = 2 ] && echo 0 || echo 1)"
ok deliver_turn.reenter "the re-Enter follows a second read and never resends the text" \
  "$([ "$DELIVER_READS" = 2 ] && [ "$DELIVER_TEXTS" = 1 ] && echo 0 || echo 1)"

# The prompt went to history after the first Enter: no re-Enter.
run_deliver 99 "$UNSENT_PANE" '' "$SENT_PANE"
ok deliver_turn.no_evidence "a prompt that went to history gets no re-Enter" \
  "$([ "$DELIVER_RC" != 0 ] && [ "$DELIVER_ENTERS" = 1 ] && echo 0 || echo 1)"

# A folded paste: the directive is typed, seen, and then submitted.
run_deliver 1 "$PASTED_PANE" "$DIRECTIVE_PANE"
ok deliver_turn.directive "an attachment gets the directive, a second read, then one Enter" \
  "$([ "$DELIVER_RC" = 0 ] && [ "$DELIVER_ENTERS" = 1 ] &&
     sed -n '2p' "$STUB_DIR/calls" | grep -qF 'pane read p1 --source visible' &&
     [ "$(sed -n '3p' "$STUB_DIR/calls")" = 'pane send-text p1 Carry out the pasted instructions.' ] &&
     sed -n '4p' "$STUB_DIR/calls" | grep -qF 'pane read p1 --source visible' &&
     sed -n '5p' "$STUB_DIR/calls" | grep -qF 'pane send-keys p1 Enter' && echo 0 || echo 1)"

run_deliver 1 "$STATUS_PASTED_PANE" "$STATUS_DIRECTIVE_PANE"
ok deliver_turn.directive "the live status line layouts: directive, a second read, then one Enter" \
  "$([ "$DELIVER_RC" = 0 ] && [ "$DELIVER_ENTERS" = 1 ] && [ "$DELIVER_DIRECTIVES" = 1 ] && echo 0 || echo 1)"

run_deliver 2 "$PASTED_PANE" "$DIRECTIVE_PANE"
ok deliver_turn.pasted "the attachment and directive still in the editor get one re-Enter" \
  "$([ "$DELIVER_RC" = 0 ] && [ "$DELIVER_ENTERS" = 2 ] && [ "$DELIVER_DIRECTIVES" = 1 ] && echo 0 || echo 1)"

# The directive typed but not seen after the attachment: nothing is submitted.
run_deliver 1 "$PASTED_PANE"
ok deliver_turn.directive "a directive the editor does not show gets no Enter" \
  "$([ "$DELIVER_RC" != 0 ] && [ "$DELIVER_ENTERS" = 0 ] && [ "$DELIVER_DIRECTIVES" = 1 ] && echo 0 || echo 1)"

# Negative controls: after the paste, no current editor holding the prompt
# means no further text and no keys at all.
# unavailable_case <description> <pane text>
unavailable_case() {
  run_deliver 1 "$2"
  ok deliver_turn.unavailable "$1 after the paste: no directive and no Enter" \
    "$([ "$DELIVER_RC" != 0 ] && [ "$DELIVER_ENTERS" = 0 ] && [ "$DELIVER_TEXTS" = 1 ] && echo 0 || echo 1)"
}
unavailable_case 'an attachment only in history' "$HISTORY_PASTE_PANE"
unavailable_case 'a dialog' "$DIALOG_PANE"
unavailable_case 'a marker line after the editor' "$AMBIGUOUS_PANE"
unavailable_case 'an editor showing only its placeholder' "$SENT_PANE"
unavailable_case 'a screen with no rules' "$TRUSTED_PANE"
unavailable_case 'a frame with a trust dialog below it' "$DIALOG_BELOW_PANE"
unavailable_case 'a frame holding answer text and no footer' "$HISTORY_IN_FRAME_PANE"
unavailable_case 'a frame holding a second marker' "$MARKER_IN_FRAME_PANE"
unavailable_case 'a frame with no footer' "$NO_FOOTER_PANE"
unavailable_case 'a frame with three lines below it' "$LONG_FOOTER_PANE"
unavailable_case 'a frame with an unindented line below it' "$FLUSH_FOOTER_PANE"
unavailable_case 'the prompt with more text in the editor' "$EXTRA_TEXT_PANE"
unavailable_case 'two attachments in the editor' "$TWO_ATTACHMENTS_PANE"
unavailable_case 'a frame over the branch status line of rerun C' \
  "$(frame_under "$(printf '  main\n%s' "$BYPASS_FOOTER")")"
unavailable_case 'a frame over "Continue? (y/n)"' "$(frame_with_footer '  Continue? (y/n)')"
unavailable_case 'a frame over "Press Enter to continue"' \
  "$(frame_with_footer '  Press Enter to continue')"
unavailable_case 'a frame over "Working on earlier request"' \
  "$(frame_with_footer '  Working on earlier request')"

stub_herdr "$PASTED_PANE"
printf '1' >"$STUB_DIR/pane-rc"
printf '1' >"$STUB_DIR/accept-at"
deliver_turn p1 "$STUB_DIR/prompt.txt" run1 /tmp/state t1 && _r=0 || _r=$?
ok deliver_turn.unreadable "an unreadable pane after the paste: no directive and zero Enters" \
  "$([ "$_r" != 0 ] && [ "$(cat "$STUB_DIR/enters")" = 0 ] &&
     [ "$(grep -c '^pane send-text' "$STUB_DIR/calls")" = 1 ] && echo 0 || echo 1)"

# Still in the editor after the one re-Enter: stop and fail.
run_deliver 99 "$UNSENT_PANE"
ok deliver_turn.bounded "still unsent after one re-Enter ends non-zero with two Enters" \
  "$([ "$DELIVER_RC" != 0 ] && [ "$DELIVER_ENTERS" = 2 ] && [ "$DELIVER_WAITS" = 2 ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# The live session's binding to the candidate
# ---------------------------------------------------------------------------
#
# A pane whose startup put an installed codeflow first on PATH. The line the
# harness runs in the pane must still resolve the candidate, and the binding
# check must refuse anything else.

mkdir -p "$STUB_DIR/candidate" "$STUB_DIR/installed"
printf '#!/bin/sh\necho candidate\n' >"$STUB_DIR/candidate/codeflow"
printf '#!/bin/sh\necho installed\n' >"$STUB_DIR/installed/codeflow"
chmod 0755 "$STUB_DIR/candidate/codeflow" "$STUB_DIR/installed/codeflow"
CANDIDATE="$STUB_DIR/candidate/codeflow"
PANE_RECORD="$STUB_DIR/pane-codeflow"

rm -f "$PANE_RECORD"
_pane_out=$(PATH="$STUB_DIR/installed:$PATH" \
  sh -c "$(pane_env_command "$STUB_DIR/candidate:$STUB_DIR/decoy" "$PANE_RECORD")")
pane_codeflow_binding "$PANE_RECORD" "$CANDIDATE" && _r=0 || _r=1
ok binding.pane "the pane line puts the candidate ahead of an installed codeflow" "$_r"
ok binding.pane "the pane line still reports the tracked environment" \
  "$([ "$_pane_out" = TRACKED=1 ] && echo 0 || echo 1)"

# Negative control: the pane as it was before, with no export.
rm -f "$PANE_RECORD"
PATH="$STUB_DIR/installed:$PATH" sh -c 'command -v codeflow >"$1"' sh "$PANE_RECORD"
pane_codeflow_binding "$PANE_RECORD" "$CANDIDATE" && _r=1 || _r=0
ok binding.installed "an installed codeflow in the pane fails the binding" "$_r"
ok binding.installed "the observed path names the installed binary" \
  "$([ "$PANE_CF_PATH" = "$STUB_DIR/installed/codeflow" ] && echo 0 || echo 1)"

rm -f "$PANE_RECORD"
pane_codeflow_binding "$PANE_RECORD" "$CANDIDATE" && _r=1 || _r=0
ok binding.missing "no recorded resolution fails the binding" \
  "$([ "$_r" = 0 ] && [ "$PANE_CF_PATH" = "nothing recorded" ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# The pipeline row waits for the backgrounded workflow's result file
# ---------------------------------------------------------------------------

# shellcheck disable=SC2034 # read by wait_for_pipeline_result in lib.sh
PIPELINE_POLL_SECONDS=1
RESULT_FILE="$STUB_DIR/pipeline-result.txt"
TURN_RESULT="$STUB_DIR/turn-result.json"

rm -f "$RESULT_FILE" "$TURN_RESULT"
printf '{"status": "completed"}\n' >"$TURN_RESULT"
(sleep 2; printf '{"status":"complete"}\n' >"$RESULT_FILE") &
wait_for_pipeline_result "$RESULT_FILE" "$TURN_RESULT" 20 && _r=0 || _r=$?
wait
ok pipeline.wait "a result file written after the first Stop ends the wait with 0" \
  "$([ "$_r" = 0 ] && echo 0 || echo 1)"

rm -f "$RESULT_FILE"
_began=$(date +%s)
wait_for_pipeline_result "$RESULT_FILE" "$TURN_RESULT" 2 && _r=0 || _r=$?
_took=$(($(date +%s) - _began))
ok pipeline.wait "no result file ends at the bound with 124, took $_took s" \
  "$([ "$_r" = 124 ] && [ "$_took" -ge 2 ] && [ "$_took" -le 5 ] && echo 0 || echo 1)"

printf '{\n  "status": "failed"\n}\n' >"$TURN_RESULT"
wait_for_pipeline_result "$RESULT_FILE" "$TURN_RESULT" 20 && _r=0 || _r=$?
ok pipeline.wait "a turn that recorded a terminal failure ends the wait with 3" \
  "$([ "$_r" = 3 ] && echo 0 || echo 1)"

# A file still growing is not taken until it holds still.
printf '{"status": "completed"}\n' >"$TURN_RESULT"
: >"$RESULT_FILE"
(for _n in 1 2 3; do sleep 1; printf 'x' >>"$RESULT_FILE"; done) &
wait_for_pipeline_result "$RESULT_FILE" "$TURN_RESULT" 20 && _r=0 || _r=$?
wait
ok pipeline.wait "a result file is read only once it stops changing" \
  "$([ "$_r" = 0 ] && [ "$(cat "$RESULT_FILE")" = xxx ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# The Workflow launch, read from the session's own transcript
# ---------------------------------------------------------------------------

FAKE_HOME="$STUB_DIR/home"
mkdir -p "$FAKE_HOME/.claude/projects/sample" "$STUB_DIR/state/turns/turn2"
printf '{"session_id": "s-launch"}\n' >"$STUB_DIR/state/turns/turn2/result.json"
LAUNCH_TX="$FAKE_HOME/.claude/projects/sample/s-launch.jsonl"

# write_launch_tx <tool-use-result json> - a transcript with one Workflow call
# on the scaffolded pipeline and the given recorded result for it.
write_launch_tx() {
  {
    printf '{"message":{"content":[{"type":"tool_use","id":"t1","name":"Workflow","input":{"scriptPath":"/s/.claude/workflows/pipeline.workflow.js"}}]}}\n'
    printf '{"message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]},"toolUseResult":%s}\n' "$1"
  } >"$LAUNCH_TX"
}

write_launch_tx '{"status":"async_launched","runId":"wf_self-check","taskType":"local_workflow"}'
_launch=$(HOME="$FAKE_HOME" workflow_launch_evidence "$STUB_DIR/state")
ok pipeline.launch "an async launch reports its run id and the background" \
  "$([ "$_launch" = "workflow run wf_self-check launched in the background" ] && echo 0 || echo 1)"
ok pipeline.launch "the same transcript still proves the Workflow invocation" \
  "$([ "$(HOME="$FAKE_HOME" workflow_invocation_evidence "$STUB_DIR/state")" = yes ] && echo 0 || echo 1)"

write_launch_tx '{"status":"completed","runId":"wf_self-check"}'
_launch=$(HOME="$FAKE_HOME" workflow_launch_evidence "$STUB_DIR/state")
ok pipeline.launch "any other recorded result reports the foreground" \
  "$([ "$_launch" = "workflow run wf_self-check launched in the foreground" ] && echo 0 || echo 1)"

printf '{"message":{"content":[{"type":"text","text":"no tools"}]}}\n' >"$LAUNCH_TX"
_launch=$(HOME="$FAKE_HOME" workflow_launch_evidence "$STUB_DIR/state")
ok pipeline.launch "a transcript without a Workflow call says so" \
  "$([ "$_launch" = "workflow launch: no pipeline Workflow call recorded" ] && echo 0 || echo 1)"

rm -f "$LAUNCH_TX"
_launch=$(HOME="$FAKE_HOME" workflow_launch_evidence "$STUB_DIR/state")
ok pipeline.launch "a missing transcript is unknown, never a background claim" \
  "$([ "$_launch" = "workflow launch unknown: no session transcript" ] && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# Claude Code's own task output is the independent proof of the pipeline
# ---------------------------------------------------------------------------

GOOD_RESULT='{"status":"complete","attempts":1,"trail":[{"stage":"build","verdict":null},{"stage":"verify","verdict":"approved"}]}'
ok pipeline.shape "a complete run with an approved verify reads as complete" \
  "$([ "$(printf '%s' "$GOOD_RESULT" | pipeline_result_shape)" = complete ] && echo 0 || echo 1)"
ok pipeline.shape "a rejected verify does not" \
  "$([ "$(printf '%s' "$GOOD_RESULT" | sed 's/approved/rejected/' | pipeline_result_shape)" = verify-not-approved ] &&
     echo 0 || echo 1)"

TASK_OUT="$STUB_DIR/task.output"
MODEL_RESULT="$STUB_DIR/model-result.txt"
# write_task_tx [input-json] - a transcript with a Workflow call launched as
# task wtask0001 and Claude Code's notice naming its output file. The call's
# input defaults to the pipeline's script path.
write_task_tx() {
  _wtt_input=${1:-'{"scriptPath":"/s/.claude/workflows/pipeline.workflow.js"}'}
  {
    printf '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Workflow","input":%s}]}}\n' "$_wtt_input"
    printf '{"type":"user","promptId":"p1","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]},"toolUseResult":{"status":"async_launched","taskId":"wtask0001","runId":"wf_x"}}\n'
    printf '{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\\n<task-id>wtask0001</task-id>\\n<tool-use-id>toolu_1</tool-use-id>\\n<output-file>%s</output-file>\\n</task-notification>"}\n' "$TASK_OUT"
  } >"$LAUNCH_TX"
}
write_task_tx
printf '{"summary":"s","result":%s}\n' "$GOOD_RESULT" >"$TASK_OUT"
printf '%s\n' "$GOOD_RESULT" >"$MODEL_RESULT"
_task=$(HOME="$FAKE_HOME" workflow_task_evidence "$STUB_DIR/state" "$MODEL_RESULT") && _r=0 || _r=$?
ok pipeline.task "a successful task output equal to the result file is the proof" \
  "$([ "$_r" = 0 ] && [ "$_task" = "Claude Code task wtask0001 output: status complete, stages build/verify, verify verdict approved; agrees with the result file" ] &&
     echo 0 || echo 1)"

printf '%s\n' "$GOOD_RESULT" | sed 's/"attempts":1/"attempts":2/' >"$MODEL_RESULT"
_task=$(HOME="$FAKE_HOME" workflow_task_evidence "$STUB_DIR/state" "$MODEL_RESULT") && _r=0 || _r=$?
ok pipeline.task "a result file that differs from the task output fails" \
  "$([ "$_r" != 0 ] && printf '%s' "$_task" | grep -q 'does not agree' && echo 0 || echo 1)"

printf '{"summary":"s","result":%s}\n' "$(printf '%s' "$GOOD_RESULT" | sed 's/approved/rejected/')" >"$TASK_OUT"
printf '%s\n' "$GOOD_RESULT" | sed 's/approved/rejected/' >"$MODEL_RESULT"
_task=$(HOME="$FAKE_HOME" workflow_task_evidence "$STUB_DIR/state" "$MODEL_RESULT") && _r=0 || _r=$?
ok pipeline.task "agreeing on an unsuccessful run still fails" \
  "$([ "$_r" != 0 ] && printf '%s' "$_task" | grep -q 'verify verdict rejected; agrees' && echo 0 || echo 1)"

rm -f "$TASK_OUT"
_task=$(HOME="$FAKE_HOME" workflow_task_evidence "$STUB_DIR/state" "$MODEL_RESULT") && _r=0 || _r=$?
ok pipeline.task "a missing task output is no proof" \
  "$([ "$_r" != 0 ] && printf '%s' "$_task" | grep -q 'output unreadable' && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# One rule recognises the pipeline's Workflow call in every reading
# ---------------------------------------------------------------------------

printf '{"summary":"s","result":%s}\n' "$GOOD_RESULT" >"$TASK_OUT"
printf '%s\n' "$GOOD_RESULT" >"$MODEL_RESULT"
TASK_PROOF="Claude Code task wtask0001 output: status complete, stages build/verify, verify verdict approved; agrees with the result file"

# check_pipeline_match <label> <yes|no> - read the current transcript with the
# invocation, launch and task readings and check that each one does, or does
# not, find the pipeline's Workflow call in it.
check_pipeline_match() {
  _cpm_inv=$(HOME="$FAKE_HOME" workflow_invocation_evidence "$STUB_DIR/state")
  _cpm_launch=$(HOME="$FAKE_HOME" workflow_launch_evidence "$STUB_DIR/state")
  _cpm_task=$(HOME="$FAKE_HOME" workflow_task_evidence "$STUB_DIR/state" "$MODEL_RESULT") &&
    _cpm_r=0 || _cpm_r=$?
  if [ "$2" = yes ]; then
    ok pipeline.match "$1: the invocation reads yes" \
      "$([ "$_cpm_inv" = yes ] && echo 0 || echo 1)"
    ok pipeline.match "$1: the launch reads its run in the background" \
      "$([ "$_cpm_launch" = "workflow run wf_x launched in the background" ] && echo 0 || echo 1)"
    ok pipeline.match "$1: the task output is the proof" \
      "$([ "$_cpm_r" = 0 ] && [ "$_cpm_task" = "$TASK_PROOF" ] && echo 0 || echo 1)"
  else
    ok pipeline.match "$1: the invocation reads no" \
      "$([ "$_cpm_inv" = no ] && echo 0 || echo 1)"
    ok pipeline.match "$1: no pipeline launch is claimed" \
      "$([ "$_cpm_launch" = "workflow launch: no pipeline Workflow call recorded" ] && echo 0 || echo 1)"
    ok pipeline.match "$1: no pipeline task is matched" \
      "$([ "$_cpm_r" != 0 ] && printf '%s' "$_cpm_task" | grep -q 'no backgrounded pipeline Workflow task' &&
         echo 0 || echo 1)"
  fi
}

# The script path form, and the registered name that live Claude Code
# sessions also use, are both the pipeline.
write_task_tx
check_pipeline_match "a Workflow call on the script path" yes
write_task_tx '{"name":"pipeline","args":{"stages":["build","verify"]}}'
check_pipeline_match "a Workflow call naming the pipeline" yes

# Negative controls: another workflow's name, or a name that only contains
# the word, is not the pipeline.
for _other in deploy pipeline-canary; do
  write_task_tx "{\"name\":\"$_other\",\"args\":{}}"
  check_pipeline_match "a Workflow call naming $_other" no
done

# The script path mentioned only in prose, or read by another tool, next to a
# Workflow call on another workflow, is not the pipeline. The launched task is
# recorded against the Read call, so only the match can reject it.
{
  printf '{"type":"assistant","message":{"content":[{"type":"text","text":"I will run .claude/workflows/pipeline.workflow.js next."}]}}\n'
  printf '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/s/.claude/workflows/pipeline.workflow.js"}},{"type":"tool_use","id":"t2","name":"Workflow","input":{"name":"deploy"}}]}}\n'
  printf '{"type":"user","promptId":"p1","message":{"content":[{"type":"tool_result","tool_use_id":"t1"}]},"toolUseResult":{"status":"async_launched","taskId":"wtask0001","runId":"wf_x"}}\n'
  printf '{"type":"queue-operation","operation":"enqueue","content":"<task-notification>\\n<task-id>wtask0001</task-id>\\n<tool-use-id>toolu_1</tool-use-id>\\n<output-file>%s</output-file>\\n</task-notification>"}\n' "$TASK_OUT"
} >"$LAUNCH_TX"
check_pipeline_match "pipeline.workflow outside a Workflow call" no
write_task_tx

# ---------------------------------------------------------------------------
# The pipeline's work is checked on its own branch
# ---------------------------------------------------------------------------

SAMPLE_REPO="$STUB_DIR/sample-repo"
git init -q -b main "$SAMPLE_REPO"
git -C "$SAMPLE_REPO" -c user.name=q -c user.email=q@example.invalid commit -q --allow-empty -m init
git -C "$SAMPLE_REPO" branch feat/built
_where=$(pipeline_checkout "$SAMPLE_REPO" feat/built "$STUB_DIR/spare") && _r=0 || _r=$?
ok pipeline.checkout "a branch with no worktree gets a spare checkout" \
  "$([ "$_r" = 0 ] && [ "$_where" = "$STUB_DIR/spare" ] && [ -e "$STUB_DIR/spare/.git" ] && echo 0 || echo 1)"
git -C "$SAMPLE_REPO" worktree remove --force "$STUB_DIR/spare"
git -C "$SAMPLE_REPO" worktree add -q "$STUB_DIR/built-tree" feat/built
_where=$(pipeline_checkout "$SAMPLE_REPO" feat/built "$STUB_DIR/spare") && _r=0 || _r=$?
ok pipeline.checkout "the worktree the pipeline used is checked in place" \
  "$([ "$_r" = 0 ] && [ "$(cd "$_where" && pwd -P)" = "$(cd "$STUB_DIR/built-tree" && pwd -P)" ] &&
     [ ! -e "$STUB_DIR/spare" ] && echo 0 || echo 1)"
pipeline_checkout "$SAMPLE_REPO" feat/missing "$STUB_DIR/spare" >/dev/null && _r=0 || _r=$?
ok pipeline.checkout "a branch that does not exist is reported, never the sample root" \
  "$([ "$_r" != 0 ] && [ ! -e "$STUB_DIR/spare" ] && echo 0 || echo 1)"

ok qualify.sh "the pipeline row checks its branch and needs the task output to agree" \
  "$(grep -qF '_checkout=$(pipeline_checkout "$DIR" "$PIPELINE_BRANCH" "$_spare")' "$SCRIPT_DIR/qualify.sh" &&
     grep -qF 'workflow_task_evidence "$_state" "$DIR/$PIPELINE_RESULT"' "$SCRIPT_DIR/qualify.sh" &&
     grep -qF '[ "$_task_ok" = 0 ]' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# Teardown ends the agent through its own exit keys, then closes the tab
# ---------------------------------------------------------------------------

# shellcheck disable=SC2034 # read by stop_pane_agent in lib.sh
STOP_AGENT_SETTLE_SECONDS=0

stub_herdr "$TRUSTED_PANE"
: >"$STUB_DIR/exit-on-keys"
stop_pane_agent p1 && _r=0 || _r=$?
ok teardown.stop "an agent that answers the exit keys leaves the pane at its shell" \
  "$([ "$_r" = 0 ] && grep -qF 'pane send-keys p1 ctrl+c ctrl+c' "$STUB_DIR/calls" &&
     [ "$(grep -c 'pane send-keys' "$STUB_DIR/calls")" = 1 ] && echo 0 || echo 1)"

stub_herdr "$TRUSTED_PANE"
stop_pane_agent p1 && _r=0 || _r=$?
ok teardown.stop "an agent that never exits is reported after a bounded number of rounds" \
  "$([ "$_r" != 0 ] && [ "$(grep -c 'pane send-keys' "$STUB_DIR/calls")" = "$STOP_AGENT_ROUNDS" ] &&
     echo 0 || echo 1)"

stub_herdr "$TRUSTED_PANE"
cp "$STUB_DIR/proc-shell" "$STUB_DIR/proc-out"
stop_pane_agent p1 && _r=0 || _r=$?
ok teardown.stop "a pane already at its shell gets no keys" \
  "$([ "$_r" = 0 ] && ! grep -q 'pane send-keys' "$STUB_DIR/calls" && echo 0 || echo 1)"

ok qualify.sh "teardown stops the agent through stop_pane_agent, never a missing herdr command" \
  "$(grep -qF 'stop_pane_agent "$HERDR_PANE"' "$SCRIPT_DIR/qualify.sh" &&
     ! grep -q 'herdr agent stop' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "a pipeline turn that is not accepted fails the row before the terminal wait" \
  "$(awk '/--timeout-seconds "\$PIPELINE_ACCEPT_SECONDS"/ {a=NR} /the pipeline turn was not accepted/ {r=NR}
          /--until terminal/ && a && !t {t=NR} END {exit !(a && r && t && a < r && r < t)}' \
     "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "the pipeline row waits for the result file and records the launch" \
  "$(grep -qF 'wait_for_pipeline_result "$DIR/$PIPELINE_RESULT"' "$SCRIPT_DIR/qualify.sh" &&
     grep -qF '_launch=$(workflow_launch_evidence "$_state")' "$SCRIPT_DIR/qualify.sh" &&
     echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# The option the operator drives all of this with
# ---------------------------------------------------------------------------

ok qualify.sh "parses and documents --trust-wait-seconds" \
  "$(sh "$SCRIPT_DIR/qualify.sh" --help | grep -q -- '--trust-wait-seconds' && echo 0 || echo 1)"

_bad=$(sh "$SCRIPT_DIR/qualify.sh" --trust-wait-seconds soon 2>&1 || true)
ok qualify.sh "rejects a --trust-wait-seconds that is not a number" \
  "$(printf '%s' "$_bad" | grep -q 'whole number of seconds' && echo 0 || echo 1)"

_bad=$(sh "$SCRIPT_DIR/qualify.sh" --target-dir "$STUB_DIR/elsewhere" 2>&1) && _bad_rc=0 || _bad_rc=$?
ok qualify.sh "refuses --target-dir and builds only into its own work directory" \
  "$([ "$_bad_rc" = 64 ] && printf '%s' "$_bad" | grep -q 'its own target' &&
     [ ! -e "$STUB_DIR/elsewhere" ] &&
     grep -qF 'TARGET_DIR="$WORK/cargo-target"' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "writes the fixed status line before it creates the canary tab" \
  "$(awk '/write_status_line_settings "\$DIR"/ { w = NR } /herdr tab create --workspace "\$HERDR_WORKSPACE"/ { t = NR }
     END { exit !(w && t && w < t) }' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "runs the trust branch through resolve_trust_prompt" \
  "$(grep -qF 'resolve_trust_prompt "$HERDR_PANE" "$HERDR_TAB"' \
     "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "both armed turns go through deliver_turn with their turn id" \
  "$(grep -qF 'deliver_turn "$HERDR_PANE" "$WORK/delegate-prompt.txt" "$_run" "$_state" "$_turn"' \
     "$SCRIPT_DIR/qualify.sh" &&
     grep -qF 'deliver_turn "$HERDR_PANE" "$WORK/pipeline-prompt.txt" "$_run" "$_state" "$_turn2"' \
     "$SCRIPT_DIR/qualify.sh" && ! grep -q '^deliver_turn()' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

ok qualify.sh "exports the candidate into the live pane and records its binding" \
  "$(grep -qF 'pane_env_command "$BIN_DIR:$DECOY_DIR" "$_pane_codeflow"' "$SCRIPT_DIR/qualify.sh" &&
     grep -qF 'pane_codeflow_binding "$_pane_codeflow" "$BINARY"' "$SCRIPT_DIR/qualify.sh" &&
     echo 0 || echo 1)"

# ---------------------------------------------------------------------------
# present-review.py: the positive present resolve row's review client
# ---------------------------------------------------------------------------
#
# A stub session service on loopback checks each request the way the real
# service does: the bootstrap post carries the capability and no Origin, and
# the review post carries the cookie the bootstrap set, the session Origin,
# the request marker and JSON. Mode "refuse" rejects the capability and mode
# "nocookie" sets no cookie; the client must then print no event id.

review_stub() { # <mode> <bootstrap-file> -> "<client exit> <event id or empty> <stub verdict>"
  python3 - "$1" "$2" "$SCRIPT_DIR/present-review.py" <<'PY'
import http.server, json, subprocess, sys, threading
mode, page, client = sys.argv[1], sys.argv[2], sys.argv[3]
seen = {"bootstrap": "none", "review": "none"}
class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0))).decode()
        auth = "127.0.0.1:%d" % self.server.server_port
        if self.path == "/bootstrap":
            ok = body == "capability=cap-123" and self.headers.get("Origin") is None
            seen["bootstrap"] = "ok" if ok else "bad"
            if mode == "refuse" or not ok:
                self.send_response(401); self.end_headers(); self.wfile.write(b"invalid"); return
            self.send_response(200)
            if mode != "nocookie":
                self.send_header("Set-Cookie", "cfp=sess-9; Path=/app; HttpOnly; SameSite=Strict")
            self.end_headers(); return
        if self.path == "/app/api/reviews":
            doc = json.loads(body)
            ok = (self.headers.get("Cookie") == "cfp=sess-9"
                  and self.headers.get("Origin") == "http://" + auth
                  and self.headers.get("X-CF-Present") == "1"
                  and self.headers.get("Content-Type") == "application/json"
                  and doc["session_id"] == "s-1" and doc["revision"] == 2
                  and doc["verdict"] == "request_changes" and doc["instruction"])
            seen["review"] = "ok" if ok else "bad"
            self.send_response(200 if ok else 403); self.end_headers()
            self.wfile.write(json.dumps({"event_id": doc["event_id"], "state": "received"}).encode())
server = http.server.HTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=server.serve_forever, daemon=True).start()
open(page, "w").write('<form id="bootstrap" method="post" action="http://127.0.0.1:%d/bootstrap">'
                      '<input type="hidden" name="capability" value="cap-123"></form>' % server.server_port)
run = subprocess.run([sys.executable, client, page, "s-1", "2"], capture_output=True, text=True)
server.shutdown()
print(run.returncode, run.stdout.strip() or "-", seen["bootstrap"], seen["review"])
PY
}

set -- $(review_stub ok "$STUB_DIR/bootstrap.html")
ok present-review.py "posts the bootstrap, then the review with cookie, Origin and marker" \
  "$([ "$1" = 0 ] && printf '%s' "$2" | grep -Eq '^[0-9a-f-]{36}$' &&
     [ "$3" = ok ] && [ "$4" = ok ] && echo 0 || echo 1)"
set -- $(review_stub refuse "$STUB_DIR/bootstrap.html")
ok present-review.py "a refused bootstrap gives no event id and sends no review" \
  "$([ "$1" = 1 ] && [ "$2" = - ] && [ "$4" = none ] && echo 0 || echo 1)"
set -- $(review_stub nocookie "$STUB_DIR/bootstrap.html")
ok present-review.py "a bootstrap that sets no cookie gives no event id and sends no review" \
  "$([ "$1" = 1 ] && [ "$2" = - ] && [ "$4" = none ] && echo 0 || echo 1)"
set --

ok qualify.sh "the positive present resolve row runs through present-review.py" \
  "$(grep -qF 'python3 "$SCRIPT_DIR/present-review.py" "$_bootstrap" "$_sid"' "$SCRIPT_DIR/qualify.sh" &&
     ! grep -q 'hosts no browser' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

# grade_present_resolve: the positive resolve row passes only on the saved
# effect. Each control changes one input from the passing case: history read
# after resolve must show this event addressed after the resolved version.
EV=11111111-2222-4333-8444-555555555555
ACK="resolved $EV as addressed"
history_json() { # <extra events JSON, comma-led or empty>
  printf '{"feedback_events":[{"event":"received","sequence":1,"envelope":{"event_id":"%s"}},' "$EV"
  printf '{"event":"delivered","sequence":2,"event_id":"%s","at_unix":1}%s]}' "$EV" "$1"
}
ADDRESSED=$(printf ',{"event":"addressed","sequence":3,"event_id":"%s","at_unix":2}' "$EV")
grade_case() { # <name> <expected> <event> <delivered> <version> <status> <output> <history>
  _gc_name=$1 _gc_want=$2
  shift 2
  _gc_got=$(grade_present_resolve "$@")
  ok "grade_present_resolve.$_gc_name" "the row is $_gc_want" \
    "$([ "$_gc_got" = "$_gc_want" ] && echo 0 || echo 1)"
  unset _gc_name _gc_want _gc_got
}
grade_case addressed "$RESULT_PASSED" "$EV" yes 2 0 "$ACK" "$(history_json "$ADDRESSED")"
grade_case ack-only "$RESULT_FAILED" "$EV" yes 2 0 "$ACK" "$(history_json "")"
grade_case stale "$RESULT_FAILED" "$EV" yes 3 0 "$ACK" "$(history_json "$ADDRESSED")"
grade_case other-event "$RESULT_FAILED" "$EV" yes 2 0 "$ACK" \
  "$(history_json ',{"event":"addressed","sequence":3,"event_id":"99999999-2222-4333-8444-555555555555","at_unix":2}')"
grade_case also-dismissed "$RESULT_FAILED" "$EV" yes 2 0 "$ACK" \
  "$(history_json "$ADDRESSED$(printf ',{"event":"dismissed","sequence":4,"event_id":"%s","at_unix":3}' "$EV")")"
grade_case malformed "$RESULT_FAILED" "$EV" yes 2 0 "$ACK" '{"feedback_events":'
grade_case resolve-failed "$RESULT_FAILED" "$EV" yes 2 1 "$ACK" "$(history_json "$ADDRESSED")"
grade_case not-delivered "$RESULT_FAILED" "$EV" no 2 0 "$ACK" "$(history_json "$ADDRESSED")"
grade_case no-ack "$RESULT_FAILED" "$EV" yes 2 0 "" "$(history_json "$ADDRESSED")"
grade_case no-event "$RESULT_FAILED" "" yes 2 0 "$ACK" "$(history_json "$ADDRESSED")"
ok qualify.sh "the positive resolve row is graded on history read after resolve" \
  "$(grep -qF '_s=$(grade_present_resolve "$_event" "$_delivered" "$_version"' "$SCRIPT_DIR/qualify.sh" &&
     ! grep -qF 'grep -qF "resolved $_event as addressed"; then' "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

printf '\n%s check(s), %s failed\n' "$CHECKS" "$FAILED"
[ "$FAILED" = 0 ]
