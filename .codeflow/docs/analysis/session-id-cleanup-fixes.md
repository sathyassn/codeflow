# Session ID Cleanup Fixes

## Problem

10 hook scripts use the Claude per-agent UUID from stdin `.session_id` instead of the
canonical `CODEFLOW_SESSION_ID` (written to `.state/runtime/codeflow-env.sh` by the
session-start-init hook). This causes state path mismatches: hooks create directories,
look up sentinels, and write logs under the wrong session ID.

Claude Code provides a different UUID per agent process. In a team session, the lead
and each teammate receive distinct UUIDs. Only `CODEFLOW_SESSION_ID` (format:
`ses-{13-digit-timestamp}{12-hex-chars}`) is stable across all agents in a session.

## Root Cause

Hooks read stdin JSON and assign `.session_id` to their working variable before
sourcing `codeflow-env.sh`. The stdin value overwrites the correct
`CODEFLOW_SESSION_ID` from the environment or env file.

## Correct Pattern

Used by: `cf-session-start-init.sh`, `cf-session-end-cleanup.sh`,
`cf-pre-tool-use-pathflow-gate.sh`, `cf-post-tool-use-pathflow-sentinel.sh`,
`cf-post-tool-use-phase-checkpoint.sh`.

```bash
# 1. Read stdin (capture UUID as labeled fallback only)
_stdin_sid=""
if [[ ! -t 0 ]]; then
    _HOOK_STDIN=$(cat)
    if [[ -n "$_HOOK_STDIN" ]] && command -v jq &>/dev/null; then
        _sid=$(echo "$_HOOK_STDIN" | jq -r '.session_id // empty' 2>/dev/null)
        [[ -n "$_sid" ]] && _stdin_sid="$_sid"
    fi
fi

# 2. Source env file (after REPO_ROOT is set)
_env_file="${REPO_ROOT}/.state/runtime/codeflow-env.sh"
if [[ -f "$_env_file" ]]; then
    # shellcheck source=/dev/null
    source "$_env_file"
fi

# 3. Priority: env file > stdin > fallback
SESSION_ID="${CODEFLOW_SESSION_ID:-${_stdin_sid:-unknown}}"
```

## Affected Hooks

| # | Hook Script | Severity | Impact | Fix |
|---|------------|----------|--------|-----|
| A1 | settings.json PostToolUse matcher | CRITICAL | pathflow-active flag never removed after TeamDelete | Add TeamDelete to matcher |
| A2 | cf-session-end-cleanup.sh | CRITICAL | Lead's SessionEnd indistinguishable from teammate's | Add $PPID==lead_pid check |
| A3 | cf-pre-tool-use-team-guard.sh | CRITICAL | pf-6 sentinel check uses wrong path, TeamDelete blocked at PF7 | Source env file, fix session ID |
| B1 | cf-session-start-logging.sh | HIGH | Creates orphan .state/session/{UUID}/ dirs | Source env file, fix state dir |
| B2 | cf-session-start-instructions.sh | MEDIUM | PathFlow active check fails, recovery instructions skipped | Source env file, fix session ID |
| B3 | cf-session-end-logging.sh | MEDIUM | Meta file lookup wrong, duration shows 0 | Source env file, fix priority |
| B4 | cf-stop-logging.sh | LOW | verify-work-retry lookup wrong path (DEAD CODE) | Source env file, fix priority |
| B5 | cf-user-prompt-submit-logging.sh | MEDIUM | Prompt counter resets per agent, audit counts wrong | Source env file, fix session ID |
| C1 | cf-pre-tool-use-protected-resource.sh | LOW | Wrong session_id in security logs only | Source env file |

## Additional Fixes

### PostToolUse Matcher (A1)

The `cf-post-tool-use-pathflow-sentinel.sh` hook handles flag removal after
TeamDelete, but the settings.json PostToolUse matcher was
`TeamCreate|Task|SendMessage|Bash` -- missing `TeamDelete`. The hook never fires
on TeamDelete, so the pathflow-active flag is never removed, causing SessionEnd
to skip cleanup (treating it as a teammate shutdown).

Fix: Change matcher to `TeamCreate|TeamDelete|Task|SendMessage|Bash`.

### PID Self-Reference (A2)

When in-process teammates share the lead's PID, `kill -0 $lead_pid` always
succeeds for the lead's own SessionEnd (checking self). The hook cannot
distinguish "lead ending" from "teammate ending".

Fix: Check `$PPID == $lead_pid` first. If true, this IS the lead's SessionEnd --
proceed with cleanup. Only fall through to `kill -0` for the teammate case.

### Test Cleanup Leak (D1)

`test-cf-sentinel.sh` EXIT trap only cleans `/tmp` sentinel dirs. PathFlow tests
create `.state/session/test-session-$$/` dirs that leak on test failure.

Fix: Add `.state/session/${CODEFLOW_SESSION_ID}` cleanup to the EXIT trap.

### Dead Code Note (B4)

The `verify-work-retry` lookup in `cf-stop-logging.sh` (line ~230) references
state files written by `cf-stop-verify-work.sh`, which was deleted during the V4
revamp. The UUID bug has no functional impact because no hook writes these files
anymore. The fix normalizes the session ID anyway for consistency.

### Settings Template Sync (A1)

`.claude/settings-templates/autonomous.json` requires the same PostToolUse matcher fix
as settings.json.

### Dead Code Removal (B4)

Remove the entire verify-work-retry block from `cf-stop-logging.sh` (~lines 228-240).
This is V3 orphan code -- the writer hook `cf-stop-verify-work.sh` was deleted during
V4 revamp. WS-REV stage now handles review. Check `enforcement-policy.json` for any
related PCV config to remove.

### Documentation Updates (E1)

`.codeflow/docs/analysis/pathflow-lifecycle-reference.md` needs updates in 5 sections:
SessionEnd lifecycle guard flow, Layer 2 cleanup, edge cases table, Scenario 1 clean
PF7, hook interaction sequence.

### SessionStart Hook Matcher Split (NEW)

**Problem:** All SessionStart hooks run in PARALLEL within a single matcher entry in
`settings.json`. The init hook writes `codeflow-env.sh` (with the canonical session ID),
but the logging hook runs simultaneously, finds no env file, falls back to the stdin UUID,
and creates orphan `.state/session/{UUID}/` directories.

**Evidence:** During a session, BOTH `.state/session/ses-177204701170500ef6fd85361/` (correct,
from init) AND `.state/session/e557fb86-742a-4b27-b5bc-ebb33b4277e8/` (orphan UUID, from
logging hook) were created. The logging hook ran before init finished writing the env file.

**Fix:** Split the SessionStart hooks in `settings.json` and all 4 settings templates into
TWO separate matcher entries:

- First matcher: working-protocol skill (pure read) + init hook (writes env file)
- Second matcher: instructions hook + logging hook (both read env file)

Different matcher entries within the same event run SEQUENTIALLY, so init completes
before instructions/logging start. This ensures the env file exists when downstream hooks
read it.

**Files changed:** `.claude/settings.json`, `.claude/settings-templates/autonomous.json`,
`.claude/settings-templates/permissive.json`, `.claude/settings-templates/standard.json`,
`.claude/settings-templates/strict.json`.

**Test:** New tests 66-67 in `test-cf-session-start-logging.sh` verify the matcher split
structure and that the env file SID takes priority when available.

## Test Changes

| Test File | Tests Affected | Change |
|-----------|---------------|--------|
| test-cf-session-start-logging.sh | Tests 15, 43 | Assert env file priority over stdin UUID |
| test-cf-session-start-logging.sh | Tests 66, 67 | Verify matcher split structure and env file availability |
| test-cf-session-start-init.sh | Test 3b | Source guard: source=clear + dead PID preserves sentinels |
| test-cf-session-end-logging.sh | Test 49 | Assert env file priority over stdin UUID |
| test-cf-stop-logging.sh | Test 57 | Assert env file priority over stdin UUID |
| test-cf-pathflow-enforcement.sh | test_init_hook_creates_flag | Fix isolated test env lacking checkpoint config |
| test-cf-sentinel.sh | EXIT trap | Add .state/session/ cleanup |

## Fix Status

All fixes implemented and committed on branch `fix/session-end-cleanup`.

- Commits 1-4: Core session ID priority fixes (9 hooks), test updates (5 test files), docs
- Commit 5: SessionStart matcher split (race condition fix), source=clear test, doc updates
