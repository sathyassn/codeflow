---
id: BRIEF-005
title: "PathFlow Cleanup System Fixes"
status: draft
author: cf-development
created: "2026-02-24"
---

# PathFlow Cleanup System Fixes

## Table of contents

- [Problem statement](#problem-statement)
- [Agreed solution](#agreed-solution)
- [Key design decisions](#key-design-decisions)
- [Cleanup responsibility matrix](#cleanup-responsibility-matrix)
- [Scenario table](#scenario-table)
- [Additional fixes](#additional-fixes)
- [Related](#related)
- [Hook lifecycle scenarios](#hook-lifecycle-scenarios)
  - [Scenario 1: Clean PF1→PF7 (happy path)](#scenario-1-clean-pf1pf7-happy-path)
  - [Scenario 2: Compaction mid-session](#scenario-2-compaction-mid-session)
  - [Scenario 3: Multiple compactions](#scenario-3-multiple-compactions)
  - [Scenario 4: Lead crash (no clean shutdown)](#scenario-4-lead-crash-no-clean-shutdown)
  - [Scenario 5: Teammate starts during active session](#scenario-5-teammate-starts-during-active-session)
  - [Scenario 6: Teammate starts after compaction](#scenario-6-teammate-starts-after-compaction)
  - [Scenario 7: SessionEnd without TeamDelete](#scenario-7-sessionend-without-teamdelete)
  - [Edge cases](#edge-cases)
  - [Hook interaction sequence](#hook-interaction-sequence)

## Problem statement

Four issues in the PathFlow cleanup system:

1. **Premature flag removal**: `cf-pre-tool-use-team-guard.sh` removes the `pathflow-active` flag at PreToolUse time (before TeamDelete succeeds). If TeamDelete fails, the flag is already gone, leaving the session in an inconsistent state. The flag should only be removed after TeamDelete succeeds.

2. **SessionEnd cleanup gaps**: The SessionEnd hook does not clean up all session-scoped artifacts. Missing: team config backstop, task list backstop, pf-7 diagnostic logging.

3. **Conditional tasks block phase sentinels**: Tasks with conditions (e.g., `adhoc_only`) that evaluate to false are not auto-skipped during phase completion evaluation. The lead must explicitly skip them, which is error-prone.

4. **Stage sentinel pattern matching fragile**: The STAGE-COMPLETE regex only matches exact uppercase format. Case variations (e.g., lowercase from tool normalization) silently fail to create sentinels. WS-QA ordering only checks ws-dev, missing ws-test as a valid predecessor.

## Agreed solution

Three-layer cleanup model:

### Layer 1: PreToolUse team-guard (gate only)

- Check pf-6 sentinel exists
- Allow TeamDelete (exit 0) or block (exit 2)
- **No side effects** -- does NOT remove the pathflow-active flag
- Does NOT modify any state

### Layer 2: PostToolUse on TeamDelete (flag removal)

- Fires AFTER TeamDelete succeeds
- Removes `pathflow-active` flag: `rm -f .state/session/{SID}/pathflow/is-pathflow-active`
- Single action, single responsibility
- If PostToolUse fails, flag stays -- correct behavior (SessionEnd handles as backstop)

### Layer 3: SessionEnd (comprehensive cleanup)

- Guards against teammate shutdown (flag present -> skip)
- Logs pf-7 diagnostic before sentinel cleanup
- Removes ALL session-scoped state:
  1. PathFlow sentinels
  2. Skill sentinels
  3. Session directory (covers flag, checkpoint, team file)
  4. Env file
  5. Active task (conditional: preserve if in_progress)
  6. Temp files
  7. Team config backstop
  8. Task list backstop

## Key design decisions

| Decision | Rationale |
|----------|-----------|
| Flag is the teammate guard | SessionEnd fires for ALL Claude Code processes including teammates; flag present -> skip cleanup |
| Flag removal AFTER TeamDelete | PostToolUse fires only on success; PreToolUse fires before execution (wrong timing) |
| pathflow-team.json lives until SessionEnd | Session-start stale detection reads it; removing it early breaks crash recovery |
| One session = one pathflow run | No flag re-creation after removal; PostToolUse on TeamDelete is final |
| pf-7 sentinel as diagnostic only | Not a gate; SessionEnd logs whether pf-7 exists for audit trail |
| Conditional task auto-evaluation | Checkpoint stores conditions metadata; evaluates against session context at phase completion |

## Cleanup responsibility matrix

| Artifact | PreToolUse team-guard | PostToolUse TeamDelete | SessionEnd |
|----------|----------------------|------------------------|------------|
| pathflow-active flag | (gate only) | **Removes** | Backstop (via session dir rm) |
| PathFlow sentinels | -- | -- | **Removes** |
| Skill sentinels | -- | -- | **Removes** |
| Session directory | -- | -- | **Removes** |
| Env file | -- | -- | **Removes** |
| Active task | -- | -- | Conditional preserve |
| Temp files | -- | -- | **Removes** |
| Team config | -- | -- | **Backstop** (if still exists) |
| Task list | -- | -- | **Backstop** (if still exists) |

## Scenario table

| Scenario | PreToolUse | PostToolUse | SessionEnd | Final state |
|----------|-----------|-------------|------------|-------------|
| Normal PF7-END | pf-6 check -> allow | Removes flag | Flag absent -> full cleanup | Clean |
| TeamDelete fails | pf-6 check -> allow | Does not fire | Flag present -> skip (teammate guard) | Stale (session-start PID cleanup catches) |
| PostToolUse fails | N/A | Flag stays | Flag present -> skip | Stale (session-start PID cleanup catches) |
| Teammate shutdown | N/A | N/A | Flag present -> skip | Correct (team still alive) |
| Lead session end (no PF7) | N/A | N/A | Flag absent (if never set) -> cleanup | Clean |
| Crash mid-session | N/A | N/A | May not fire | Stale (session-start PID cleanup catches) |

## Additional fixes

### Conditional task auto-evaluation

- `checkpoint_init_phase()` stores `conditions` metadata from pathflow-config.json
- New `checkpoint_set_context(key, value)` stores session context (origin, work_type)
- `checkpoint_is_phase_complete()` auto-evaluates conditions against context
- Fail-safe: if context key not set, condition is not evaluable -> task remains required

### WS-QA ordering warning

- Changed from checking only `ws-dev` to checking both `ws-dev` and `ws-test`
- WS-QA is valid after WS-TEST in the TEST pipeline

### Stage sentinel normalization

- Content is normalized to uppercase before regex matching
- Case variations in STAGE-COMPLETE messages no longer silently fail

### checkpoint_skip_task completion trigger

- After writing skip timestamp, calls `checkpoint_is_phase_complete()`
- Creates phase sentinel if all tasks now done/skipped
- Mirrors the logic already present in `checkpoint_complete_task()`

## Related

- `phase-checkpoint-enforcement.md` -- checkpoint system architecture
- `pid-based-teammate-detection.md` -- PID-based stale session cleanup
- `stale-session-cleanup-design.md` -- stale session cleanup design
- CLAUDE.md Section 7 -- enforcement and operations

## Hook lifecycle scenarios

Seven real-world scenarios showing how session-start, session-end, PostToolUse sentinel, and PreToolUse team-guard hooks interact.

Source references: `cf-session-start-init.sh` (v1.5.0), `cf-session-end-cleanup.sh` (v2.2.0), `cf-post-tool-use-pathflow-sentinel.sh`, `cf-pre-tool-use-team-guard.sh`.

### Scenario 1: Clean PF1→PF7 (happy path)

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Session start | SessionStart `cf-session-start-init.sh` | `source=startup`, no prior env file → generate SID, write env file, create pathflow flag, init checkpoint | env file, flag, checkpoint created |
| TeamCreate | PostToolUse `pathflow-sentinel` | Writes `pathflow-team.json` with `lead_pid=$PPID` | team file written |
| Task (spawn teammate) | PostToolUse `pathflow-sentinel` | Updates `pathflow-team.json`: `teammate_spawned=true` | team file updated |
| Stage completion SendMessage | PostToolUse `pathflow-sentinel` | Pattern-matches `STAGE-COMPLETE: WS-*` → creates stage sentinel | stage sentinel created |
| TeamDelete | PreToolUse `team-guard` | Checks pathflow-active flag + pf-6 sentinel → allows through (exit 0) | gate passed |
| TeamDelete | PostToolUse `pathflow-sentinel` | Fires after TeamDelete succeeds → removes pathflow-active flag | flag removed |
| Session end | SessionEnd `session-end-cleanup.sh` | Flag absent → `_PATHFLOW_ACTIVE=false` → full cleanup: PathFlow sentinels, skill sentinels, session dir, env file, team config backstop | all artifacts removed |

**Result: CLEAN**

### Scenario 2: Compaction mid-session

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Session start | SessionStart | `source=startup`, no prior env → normal init, `pathflow-team.json` has `lead_pid=PID_A` | Session established |
| COMPACTION | Claude Code | Old process killed, new process starts; `source=compact` | PID changes to PID_B |
| Session start (post-compact) | SessionStart | `source=compact`, reads `pathflow-team.json`, PID_A dead → skip stale cleanup, update `lead_pid=PID_B` atomically | team file updated |
| Teammate spawned | Teammate SessionStart | Reads updated `pathflow-team.json`, PID_B alive → TEAMMATE MODE | correct |
| PF7-END | Normal cleanup | Full cleanup proceeds | CLEAN |

**Result: CLEAN** (PID update in team file prevents false stale detection)

### Scenario 3: Multiple compactions

| Step | Action | State |
|------|--------|-------|
| Compaction 1 | `source=compact`, PID_A dead → update to PID_B | chain: PID_A → PID_B |
| Compaction 2 | `source=compact`, PID_B dead → update to PID_C | chain: PID_A → PID_B → PID_C |
| Teammate start (any point) | Reads latest `pathflow-team.json`, PID_C alive → TEAMMATE MODE | correct |
| PF7-END | Normal cleanup | CLEAN |

Each compaction atomically updates `lead_pid` via tmp+mv. The chain is linear — each write overwrites the previous.

**Result: CLEAN**

### Scenario 4: Lead crash (no clean shutdown)

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Lead crashes | (none) | No SessionEnd, no TeamDelete fires | All artifacts remain on disk |
| Next session start | SessionStart | `source=startup`, reads env file, finds `pathflow-team.json`, old lead PID dead → stale cleanup | Removes: team config (`~/.claude/teams/`), task list (`~/.claude/tasks/`), session dir (flag, checkpoint, team file), sentinels, env file, active task, session ID ref |

Stale cleanup runs at `cf-session-start-init.sh` section 1b (lines 96-133).

**Result: CLEAN** (deferred cleanup at next startup)

### Scenario 5: Teammate starts during active session

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Teammate session start | SessionStart | `source=startup`, reads env file, reads `pathflow-team.json`, `lead_pid` alive → TEAMMATE MODE | correct |
| Skipped | SessionStart | Session ID generation, flag creation, checkpoint init | all skipped (lead handles these) |

Detection logic: `kill -0 "$_old_lead_pid"` at `cf-session-start-init.sh` line 84.

**Result: CORRECT**

### Scenario 6: Teammate starts after compaction

| Step | Action | Result |
|------|--------|--------|
| Compaction updated `pathflow-team.json` | `lead_pid=PID_B` | team file current |
| Teammate SessionStart | Reads `lead_pid=PID_B`, `kill -0 PID_B` succeeds → TEAMMATE MODE | correct |

**Result: CORRECT** (PID update in Scenario 2 step prevents false stale detection here)

### Scenario 7: SessionEnd without TeamDelete

| Step | Hook | Action | Result |
|------|------|--------|--------|
| Session exits without PF7 | (none) | User closes terminal; no TeamDelete, no PostToolUse flag removal | pathflow-active flag still exists |
| SessionEnd fires | SessionEnd | `is_pathflow_active()` returns true → guard at line 124 skips cleanup (exit 0) | cleanup deferred |
| Next startup | SessionStart | `source=startup`, lead PID dead → stale cleanup (Scenario 4 pattern) | all artifacts removed |

**Result: CORRECT** (intentional deferred cleanup; SessionEnd guard prevents teammate shutdowns from wiping lead state)

### Edge cases

| Edge Case | Severity | Handled? | Notes |
|-----------|----------|----------|-------|
| PID recycling (OS reuses dead PID for unrelated process) | Very low | Partially | Theoretical: `kill -0` on unrelated process causes false TEAMMATE MODE |
| Compaction/teammate spawn race | Negligible | Yes | Claude Code executes sequentially — teammate cannot spawn during compaction |
| SessionEnd blocks lead cleanup on unclean exit | Low | Yes | Intentional design — deferred to next startup (Scenario 4) |
| No `pathflow-team.json` but flag exists | None | Yes | Pre-TeamCreate state; handled at `cf-session-start-init.sh` lines 162-170 — lead proceeds normally |
| Double env file sourcing | None | Yes | Idempotent; no functional impact |
| Teammate `source=startup` same value as lead | Informational | Yes | Claude Code limitation — PID check in `pathflow-team.json` disambiguates |

### Hook interaction sequence

```text
SessionStart → creates: env file, pathflow flag, checkpoint
PostToolUse(TeamCreate) → writes: pathflow-team.json with lead_pid
PostToolUse(compact SessionStart) → updates: lead_pid in pathflow-team.json
PostToolUse(SendMessage STAGE-COMPLETE) → creates: stage sentinels
PreToolUse(TeamDelete) → gate: checks pathflow-active + pf-6 sentinel
TeamDelete executes → removes: team config, task list
PostToolUse(TeamDelete) → removes: pathflow-active flag
SessionEnd → guard: skips if flag exists → cleanup: sentinels, session dir, env
```
