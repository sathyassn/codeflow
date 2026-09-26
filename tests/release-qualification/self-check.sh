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
#   pane-text-after-enter  swapped into pane-text on any send-keys
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
  printf '    [ -f "$D/exit-on-keys" ] && cp "$D/proc-shell" "$D/proc-out"\n'
  printf '    [ -f "$D/pane-text-after-enter" ] && cp "$D/pane-text-after-enter" "$D/pane-text"\n'
  printf '    n=$(($(cat "$D/enters") + 1)); printf "%%s" "$n" >"$D/enters"\n'
  printf '    [ "$n" -lt "$(cat "$D/accept-at")" ] || : >"$D/accepted"\n'
  printf '    exit 0 ;;\n'
  printf 'esac\n'
  printf 'exit 0\n'
} >"$STUB_DIR/herdr"
chmod 0755 "$STUB_DIR/herdr"

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
    "$STUB_DIR/pane-text-directive" "$STUB_DIR/pane-text-after-enter"
  printf '%s\n' "$PROC_AGENT" >"$STUB_DIR/proc-out"
  printf '%s\n' "$PROC_SHELL" >"$STUB_DIR/proc-shell"
}

# The `pane process-info` replies for a pane running an agent and for one back
# at its shell: only the foreground process group differs.
PROC_AGENT='{"result":{"process_info":{"foreground_process_group_id":200,"shell_pid":100},"type":"pane_process_info"}}'
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

# The wait switched off: say so, ask nobody, read nothing.
stub_herdr "$BLOCKED_PANE"
run_resolve 0
ok resolve_trust_prompt.disabled "a zero budget says the wait was disabled by the flag" \
  "$([ "$RESOLVE_RC" != 0 ] && [ "$TRUST_OUTCOME" = disabled ] &&
     printf '%s' "$TRUST_REASON" | grep -qF -- '--trust-wait-seconds 0' && echo 0 || echo 1)"
ok resolve_trust_prompt.disabled "a zero budget neither asks the operator nor reads the pane" \
  "$([ ! -s "$STUB_DIR/banner" ] && [ ! -s "$STUB_DIR/calls" ] && echo 0 || echo 1)"

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

RULE='──────────────────────────────'

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

# A long paste Claude folded into an attachment in the current editor, and the
# same editor once the directive is typed after it.
PASTED_PANE="$RULE
❯ [Pasted text #1 +12 lines]
$RULE
  paste again to expand"
DIRECTIVE_PANE="$RULE
❯ [Pasted text #1 +12 lines]Carry out the pasted instructions.
$RULE
  ⏸ manual mode on"

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

# Something that looks like an editor, with a marker line after it: ambiguous.
AMBIGUOUS_PANE="$RULE
❯ Reply with exactly: ok. Do not edit any file.
$RULE
 ❯ 1. Yes"

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
stub_herdr "$UNSENT_PANE"
_text=$(current_editor p1) && _r=0 || _r=$?
ok current_editor.found "the editor between the last two rules is found, marker removed" \
  "$([ "$_r" = "$EDITOR_FOUND" ] && [ "$_text" = 'Reply with exactly: ok. Do not edit any file.' ] && echo 0 || echo 1)"
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
no_editor_case 'a screen with no rules' "$TRUSTED_PANE"
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
  "$([ "$_launch" = "workflow launch: no Workflow call recorded" ] && echo 0 || echo 1)"

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
# write_task_tx - a transcript with the pipeline Workflow launched as task
# wtask0001 and Claude Code's notice naming its output file.
write_task_tx() {
  {
    printf '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Workflow","input":{"scriptPath":"/s/.claude/workflows/pipeline.workflow.js"}}]}}\n'
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

printf '\n%s check(s), %s failed\n' "$CHECKS" "$FAILED"
[ "$FAILED" = 0 ]
