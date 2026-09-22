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
}

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
# The option the operator drives all of this with
# ---------------------------------------------------------------------------

ok qualify.sh "parses and documents --trust-wait-seconds" \
  "$(sh "$SCRIPT_DIR/qualify.sh" --help | grep -q -- '--trust-wait-seconds' && echo 0 || echo 1)"

_bad=$(sh "$SCRIPT_DIR/qualify.sh" --trust-wait-seconds soon 2>&1 || true)
ok qualify.sh "rejects a --trust-wait-seconds that is not a number" \
  "$(printf '%s' "$_bad" | grep -q 'whole number of seconds' && echo 0 || echo 1)"

ok qualify.sh "runs the trust branch through resolve_trust_prompt" \
  "$(grep -qF 'resolve_trust_prompt "$HERDR_PANE" "$HERDR_TAB"' \
     "$SCRIPT_DIR/qualify.sh" && echo 0 || echo 1)"

printf '\n%s check(s), %s failed\n' "$CHECKS" "$FAILED"
[ "$FAILED" = 0 ]
