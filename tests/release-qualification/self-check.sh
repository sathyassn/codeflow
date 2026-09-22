#!/bin/sh
# Self-checks for the release-qualification harness itself.
#
# The matrix run takes about 25 minutes and needs a live Claude session, so the
# harness's own logic is proved here instead, against stub commands on PATH.
# Every check is a negative control paired with its positive: a helper that
# only ever succeeds proves nothing, so each case that must succeed is run
# beside a case that must fail.
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

# stub_herdr <pane text> - a herdr on PATH whose `pane read` prints that text.
# It also records every invocation, so the poll interval can be counted.
stub_herdr() {
  : >"$STUB_DIR/calls"
  {
    printf '#!/bin/sh\n'
    printf 'printf "%%s\\n" "$*" >>"%s/calls"\n' "$STUB_DIR"
    printf 'cat <<"PANE"\n%s\nPANE\n' "$1"
  } >"$STUB_DIR/herdr"
  chmod 0755 "$STUB_DIR/herdr"
}

TRUSTED_PANE='> Welcome to Claude Code
  cwd: /tmp/sample
  Ready for input.'

BLOCKED_PANE='Do you trust the files in this folder?

  Is this a project you created or one you trust?

  1. Yes, proceed
  2. No, exit'

# ---------------------------------------------------------------------------
# trust_prompt_showing reads the pane through herdr
# ---------------------------------------------------------------------------

stub_herdr "$BLOCKED_PANE"
trust_prompt_showing p1 && _r=0 || _r=1
ok trust_prompt_showing "true while the pane asks the question" "$_r"
ok trust_prompt_showing.command \
  "reads the pane with --source recent --lines 120" \
  "$(grep -qF 'pane read p1 --source recent --lines 120' "$STUB_DIR/calls" && echo 0 || echo 1)"

stub_herdr "$TRUSTED_PANE"
trust_prompt_showing p1 && _r=1 || _r=0
ok trust_prompt_showing "false once the question is gone" "$_r"

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

# ---------------------------------------------------------------------------
# The option the operator drives all of this with
# ---------------------------------------------------------------------------

ok qualify.sh "parses and documents --trust-wait-seconds" \
  "$(sh "$SCRIPT_DIR/qualify.sh" --help | grep -q -- '--trust-wait-seconds' && echo 0 || echo 1)"

_bad=$(sh "$SCRIPT_DIR/qualify.sh" --trust-wait-seconds soon 2>&1 || true)
ok qualify.sh "rejects a --trust-wait-seconds that is not a number" \
  "$(printf '%s' "$_bad" | grep -q 'whole number of seconds' && echo 0 || echo 1)"

printf '\n%s check(s), %s failed\n' "$CHECKS" "$FAILED"
[ "$FAILED" = 0 ]
