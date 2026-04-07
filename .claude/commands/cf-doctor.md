---
description: "Health check and diagnostics"
argument-hint: "[--repair] [--verbose]"
---

# /cf-doctor Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Assess infrastructure state
- 🔧 think-and-act: Before any repair operations (PAC-5)
- 🔧 decide: Repair decisions are Tier 2 (recommend + confirm)
- 🔧 respond-organized: Clear diagnostic output with actionable remediation

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Run health checks on CodeFlow infrastructure, diagnose issues, and optionally repair problems.

**Usage:**

```text
/cf-doctor [--repair] [--verbose]
```

**Use When:**

- Something seems broken or inconsistent in the session
- Hooks are failing or producing unexpected output
- PathFlow state seems stale or corrupted
- Teammates are unresponsive or behaving unexpectedly
- After an abnormal session termination
- Periodic infrastructure validation

**Do Not Use When:**

- Just checking current session state (use `/cf-stack`)
- Want general help or command list (use `/cf-help`)
- Normal development work -- diagnostics are for troubleshooting

### Pipeline Position

```text
Phase: Any (always available) | Type: Information/Diagnostics
No pipeline dependencies — can be invoked at any point during a session.
```

---

## 2. Arguments & Flags

**Arguments:** None.

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--repair` | `-r` | Attempt to fix detected issues automatically | false |
| `--verbose` | `-v` | Show detailed check output including passing checks | false |

**Examples:**

```bash
# Standard health check (show issues only)
/cf-doctor

# Verbose output (show all checks including passing)
/cf-doctor --verbose

# Diagnose and repair
/cf-doctor --repair

# Full verbose diagnostic with repair
/cf-doctor --repair --verbose
```

---

## 3. Prerequisites

**Required State:**

- [ ] None -- designed to work even when infrastructure is broken

This command is always available. It diagnoses problems rather than requiring prerequisites.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: Any | Type: Information/Diagnostics

/cf-doctor invoked
    |
    v
Parse flags (--repair, --verbose)
    |
    v
Run diagnostic checks (sequential):
    1. Directory structure
    2. Database health
    3. Hook subcommands (21 entries)
    4. Settings files
    5. PathFlow state consistency
    6. Sentinel integrity
    7. Teammate responsiveness
    8. Flag file state
    9. JSONL integrity
    10. Agent definitions (8 files)
    |
    v
Summarize results
    |
    v
Issues found? ---NO---> All checks passed
    |
    YES
    |
    v
--repair flag? ---NO---> Report issues with remediation steps
    |
    YES
    |
    v
Attempt automatic repair
    |
    v
Report repair results
    |
    v
Next: No pipeline progression — standalone diagnostics command.
```

### 4.2 Execution Steps

**Step 1: Parse Flags**

- Check for `--repair` and `--verbose` flags
- Set diagnostic depth and repair mode

**Step 2: Run Diagnostic Checks**

Execute all checks sequentially. Each check returns: `pass`, `warn`, or `fail`.

**Check 1: Directory Structure**

- Verify required directories exist:
  - `.claude/agents/` (8 agent definitions)
  - `.claude/skills/` (active skills)
  - `.claude/settings.json` (hook configuration — 21 `codeflow hooks` entries)
  - `.claude/settings-templates/` (4 mode templates)
  - `.codeflow/config/` (enforcement and pathflow configs)
  - `.codeflow/scripts/security/protection/` (resource protection scripts)
  - `.codeflow/testing/` (test suite)
  - `.state/db/` (SQLite database)
  - `.state/ledger/` (JSONL event logs)
  - `.state/runtime/` (runtime state)
  - `.state/sentinels/` (PathFlow sentinels)

**Check 2: Database Health**

- Verify `.state/db/codeflow.db` exists and is readable
- Check expected tables exist (sessions, tasks, pathflow_events)
- Verify database is not locked or corrupted

**Check 3: Hook Subcommands**

- Verify all 21 hook entries in `.claude/settings.json` reference valid `codeflow hooks` subcommands
- Check that the `codeflow` binary is on PATH and executable
- Verify each hook subcommand responds without error (e.g., `codeflow hooks session-start init --help`)

**Check 4: Settings Files**

- Verify `.claude/settings.json` exists and is valid JSON
- Check hook configurations reference existing scripts
- Verify settings template files are well-formed

**Check 5: PathFlow State Consistency**

- If `pathflow-session-status.json` exists with status not `"pf-complete"`:
  - Verify `current-session-id` exists and matches
  - Verify `active-task.json` exists and is well-formed
  - Check that JSONL events are consistent with session status
- If no active session: verify no orphaned runtime state exists

**Check 6: Sentinel Integrity**

- List all sentinel files in `.state/sentinels/`
- Verify each sentinel corresponds to a valid phase/stage marker
- Check for orphaned sentinels (from aborted sessions)
- Verify sentinel progression is consistent (no `ws-rev-done` without `ws-dev-done`)

**Check 6b: Worktree Health**

- Run `codeflow worktree list` to show all worktrees and their state
- Run `codeflow worktree prune --dry-run` to detect registry/filesystem inconsistencies
- With `--repair`: run `codeflow worktree prune` to fix stale entries, `codeflow worktree cleanup` to remove stale worktrees
- Check `codeflow parallel status` for active claims and merge queue state

**Check 6c: Sync Daemon Health**

- Check if the PID file exists at `.state/coordination/sync-daemon.pid`
- If PID file exists: verify the daemon process is alive using `is_pid_alive(pid)` (equivalent to `/bin/kill -0 <pid>`)
- Run `codeflow sync status` to get the full daemon status: running/stopped state, PID, peer ID, session count, and last sync completion flag
- Check `.state/coordination/sync-state.json` for `last_sync_vv` — if `null`, the daemon has not completed a sync cycle yet
- Check `.state/runtime/peer-id` exists — if missing, the daemon has not initialized yet
- If active worktrees > 1 and daemon is stopped: report as WARNING (daemon should auto-start)
- If active worktrees <= 1 and daemon is running: report as INFO (daemon may auto-stop on next worktree deregistration)
- With `--repair`: if daemon should be running but is stopped, run `codeflow sync start`; if daemon should be stopped but is running (no active worktrees), run `codeflow sync stop`

**Check 6d: Interactive Session Health**

- Verify `codeflow interactive` subcommand is available: run `codeflow interactive --help` and check it exits 0
- Run `codeflow interactive status` to list active sessions with PID liveness
- For each active session returned:
  - Verify the `CODEFLOW_SESSION_ID` matches a valid session record in the DB
  - Verify the heartbeat file `.state/interactive/heartbeat-{SID}` exists
  - Verify the session's worktree path exists (if mode=always was used)
- Check for stale sessions: sessions with PIDs that are no longer alive
- Report total session counts: active, complete, stale
- With `--repair`: run `codeflow interactive cleanup` to mark dead-PID sessions as stale and remove orphaned heartbeat files
- Verify `CODEFLOW_MANAGED` env var: if set but `CODEFLOW_SESSION_ID` is missing, report as WARNING (env is in inconsistent state)
- Check env var propagation: if `CODEFLOW_WORKTREE_PATH` is set but the path doesn't exist, report as FAIL

**Check 7: Teammate Responsiveness**

- If a team is active, check team config for registered members
- Verify each registered teammate is reachable (SendMessage ping)
- Report unresponsive or shut-down teammates

**Check 8: Session Status State**

- Check `.state/session/{SID}/pathflow/pathflow-session-status.json`
- Verify status field is one of: `created`, `pf-started`, `pf-in-progress`, `pf-complete`
- Verify status is consistent with active session state
- Detect stale status from previous sessions

**Check 9: JSONL Integrity**

- Verify `.state/logs/pathflow-events.jsonl` exists
- Check each line is valid JSON
- Verify events are chronologically ordered
- Check for duplicate or missing event IDs

**Check 10: Agent Definitions**

- Verify all 8 agent definition files exist in `.claude/agents/`
- Check each has required YAML frontmatter (name, description)
- Verify the 5-section structure (Identity, Constraints, SOPs, Communication, Quality Checklist)

**Step 3: Summarize Results**

- Count: passed, warnings, failures
- Group issues by severity

**Step 4: Report or Repair**

- **Without `--repair`:** Display issues with manual remediation steps
- **With `--repair`:** Attempt automatic fixes for known issues

### Repair Operations

| Issue | Automatic Repair | Manual Fallback |
|-------|-----------------|-----------------|
| Missing directory | Create directory | `mkdir -p {path}` |
| Stale session status | Set status to `"pf-complete"` in pathflow-session-status.json | Trigger PF7-END flow |
| Orphaned sentinels | Remove orphaned files | `rm .state/sentinels/{orphan}` |
| Missing runtime files | Create empty defaults | Manually initialize |
| Database locked | Copy to backup, recreate | Rebuild from JSONL |
| Missing `codeflow` binary | Verify Go CLI is built and on PATH | `cd codeflow-cli && go build ./cmd/codeflow/...` |
| Stale active-task.json | Remove file | Manual cleanup |

**Repair safety:** All repairs are logged. Original files are backed up before modification. Destructive repairs (database rebuild) require explicit user confirmation even with `--repair`.

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Reasoning before repair operations |
| cf-security | validate-config | Verify settings and hook integrity |
| cf-knowledge-layer | query-events | Verify JSONL and database consistency |

**Note:** For Check 7 (teammate responsiveness), the lead sends direct messages to teammates. All other checks are performed by the lead using Read and Bash tools.

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-doctor` invoked | Validate invocation |
| PreToolUse | Before Bash commands (checks) | Standard Bash validation |
| Stop | Command completes | Standard session logging |

**Note:** Diagnostic checks use Read and Bash tools. The `--repair` flag may trigger Edit/Write operations, which are subject to PreToolUse hooks.

---

## 7. Memory Integration

**Memory usage:** Read-only for diagnostics; write-only for repair log.

### Files Accessed (Read)

| File | Purpose |
|------|---------|
| `.state/db/codeflow.db` | Database health check |
| `.state/logs/pathflow-events.jsonl` | JSONL integrity check |
| `.state/runtime/active-task.json` | Runtime state check |
| `.state/runtime/current-session-id` | Session identity check |
| `.state/sentinels/*` | Sentinel integrity check |
| `.state/session/{SID}/pathflow/pathflow-session-status.json` | Session status check |
| `.claude/settings.json` | Settings validation |
| `.claude/agents/cf-*.md` | Agent definition validation |
| `.claude/settings.json` (hooks section) | Hook subcommand validation |

### Files Modified (repair mode only)

| File | Repair Action |
|------|---------------|
| `.state/session/{SID}/pathflow/pathflow-session-status.json` | Set status to `"pf-complete"` for stale sessions |
| `.state/sentinels/*` | Remove orphaned sentinels |
| `.state/runtime/active-task.json` | Remove stale runtime state |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Database corruption | SQLite file damaged | Rebuild from JSONL (Tier 0 is authoritative) |
| Hook subcommand error | Go CLI hook returned non-zero exit | Check `codeflow hooks <event> <subcommand>` output; verify `codeflow` binary is up to date |
| Stale session state | Abnormal termination of previous session | Use `--repair` to clean up stale flags and sentinels |
| Missing agent definitions | Files deleted or renamed | Restore from git: `git checkout main -- .claude/agents/` |
| JSONL corruption | Truncated or malformed entries | Identify last valid entry; truncate at corruption point |
| Permission errors | File ownership issues | Report specific files; fix with `chmod`/`chown` |

**Recovery Procedures:**

```text
ON "Database corruption":
  1. Back up current database: cp .state/db/codeflow.db .state/db/codeflow.db.bak
  2. Rebuild from JSONL: cf-knowledge-layer rebuild-from-events
  3. Verify rebuilt database
  4. If rebuild fails: start with empty database

ON "Stale session state" (with --repair):
  1. Set status to "pf-complete" in .state/session/{SID}/pathflow/pathflow-session-status.json
  2. Remove .state/runtime/active-task.json
  3. Clear orphaned sentinels from .state/sentinels/
  4. Log cleanup to pathflow-events.jsonl

ON "JSONL corruption":
  1. Identify last valid JSON line
  2. Back up corrupted file
  3. Truncate to last valid entry
  4. Log recovery event
```

---

## 9. Examples

**Example 1: Standard health check**

```bash
/cf-doctor
```

Output:

```text
CodeFlow Health Check

  Directory structure     PASS
  Database health         PASS
  Hook subcommands        PASS  (21/21 valid)
  Settings files          PASS
  PathFlow state          PASS
  Sentinel integrity      PASS
  Teammate status         WARN  cf-review shut down unexpectedly
  Flag file state         PASS
  JSONL integrity         PASS  (47 events)
  Agent definitions       PASS  (8/8 valid)

Result: 9 passed, 1 warning, 0 failures

Warning: cf-review was registered but is no longer responsive.
  Fix: Will be respawned automatically at next WS-REV stage.
```

**Example 2: Verbose output**

```bash
/cf-doctor --verbose
```

Shows all individual checks including passing ones with details.

**Example 3: Repair stale state after crash**

```bash
/cf-doctor --repair
```

Output:

```text
CodeFlow Health Check (repair mode)

  Directory structure     PASS
  Database health         PASS
  Hook subcommands        PASS
  Settings files          PASS
  PathFlow state          FAIL  Stale session status (no matching active session)
  Sentinel integrity      FAIL  2 orphaned sentinels found
  Teammate status         N/A   No active team
  Session status state    FAIL  Stale session status detected
  JSONL integrity         PASS
  Agent definitions       PASS

Repairing...
  Set stale session status to "pf-complete"
  Removed orphaned sentinel: pathflow:ws-dev-done
  Removed orphaned sentinel: pathflow:pf-3
  Cleaned stale runtime files

Result: Repaired 3 issues. Infrastructure is now clean.
```

**Example 4: All clear**

```bash
/cf-doctor
```

Output:

```text
CodeFlow Health Check

All 10 checks passed. Infrastructure is healthy.
```

---

## 10. References

- [CLAUDE.md](../CLAUDE.md) -- Infrastructure overview and recovery procedures
- [PathFlow config](../../.codeflow/config/pathflow/pathflow-config.json) -- Expected phase/stage definitions
- [Protection scripts](../../.codeflow/scripts/security/protection/) -- Resource protection shell scripts
- [cf-security agent](../agents/cf-security.md) -- Security validation
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md) -- Database and JSONL operations
- [cf-stack command](./cf-stack.md) -- Session state view
- [cf-help command](./cf-help.md) -- Help and navigation
