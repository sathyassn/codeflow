---
title: "Multi-Session Worktree Isolation"
type: analysis
status: active
author: cf-development
created_at: "2026-03-28"
updated_at: "2026-03-28"
parent: null
---

# Multi-Session Worktree Isolation

## Table of Contents

- [1. Problem Statement](#1-problem-statement)
- [2. Root Cause Analysis](#2-root-cause-analysis)
- [3. Claude Code Platform Limitation](#3-claude-code-platform-limitation)
- [4. Solution Design](#4-solution-design)
- [5. Backend-Aware Teammate Detection](#5-backend-aware-teammate-detection)
- [6. Scenario Matrix](#6-scenario-matrix)
- [7. Race Analysis](#7-race-analysis)
- [8. Autorun Impact](#8-autorun-impact)
- [9. Relationship to PR #226](#9-relationship-to-pr-226)
- [10. Future: Claude Code Issue #6885](#10-future-claude-code-issue-6885)

---

## 1. Problem Statement

When a user opens two independent Claude Code windows pointing at the same project, the second window should receive its own isolated git worktree. Instead, the second window either:

1. **False-positive teammate detection**: The second window's `SessionStart` hook reads the first session's `pathflow-team.json`, finds the first session's `lead_pid` alive, and concludes it is a teammate joining the first session. It skips worktree creation entirely and inherits the first session's state.

2. **Stale worktree reuse**: Even if teammate detection is avoided, `detect_precreated_worktree_inner()` reads the first session's `codeflow-env.sh` file and returns the first session's worktree path as "pre-created", causing the second session to share the first session's worktree.

Both bugs result in two independent sessions operating on the same worktree, causing data corruption, conflicting git state, and broken PathFlow enforcement.

## 2. Root Cause Analysis

### Bug 1: Teammate False Positive in `handle_stale_cleanup()`

**File:** `codeflow-cli/core/src/hooks/session_start.rs`, `handle_stale_cleanup()` method.

**Code path (before fix):**

```
handle_stale_cleanup()
  1. Read codeflow-env.sh -> existing_sid
  2. Read pathflow-session-status.json -> status, team_name, lead_pid
  3. Check team config exists at ~/.claude/teams/{team_name}/config.json
  4. Check is_process_alive(lead_pid)
  5. If alive -> return (Some(existing_sid), true)  // BUG: always teammate
```

**Root cause:** Step 5 assumes that if `lead_pid` is alive, the current caller must be a teammate. This assumption fails when:
- Two independent Claude Code windows share the same project directory
- The first window's `lead_pid` is alive (it is a running Claude Code process)
- The second window reads the first window's session state from the shared `.state/runtime/codeflow-env.sh`
- The second window's `SessionStart` fires, finds `lead_pid` alive, and incorrectly classifies itself as a teammate

**Key insight:** The old code could not distinguish "new teammate being spawned by the lead" from "independent new lead whose PID check passes because the old lead is still alive."

### Bug 2: Stale Worktree Reuse in `detect_precreated_worktree_inner()`

**File:** `codeflow-cli/core/src/hooks/session_start.rs`, `detect_precreated_worktree_inner()` method.

**Code path (before fix):**

```
detect_precreated_worktree_inner()
  1. Check CODEFLOW_WORKTREE_PATH env var
  2. If not set -> FALLBACK: read codeflow-env.sh -> worktree_path
  3. If worktree_path exists on disk -> return Some(WorktreePaths)
```

**Root cause:** The fallback at step 2 reads a shared file (`codeflow-env.sh`) that belongs to the prior session. When a second Claude Code window starts, the env var `CODEFLOW_WORKTREE_PATH` is not set (it's a new process), so the fallback reads the first session's env file and returns the first session's worktree.

## 3. Claude Code Platform Limitation

**Claude Code Issue #6885:** Claude Code does not currently provide a way for hooks to distinguish between:
- A new lead session starting up
- A teammate being spawned by an existing lead

Both fire the same `SessionStart` hook with `source=startup` and identical payload structure. The `session_id` field contains a per-agent UUID, not a shared session identifier.

The CodeFlow workaround uses file-based signals (env file, status file, team config, PID liveness) to infer session membership. This works for teammate detection but fails for multi-window isolation because the signals are project-scoped, not session-scoped.

Until Claude Code provides a native session membership signal (e.g., a `parent_session_id` field in the hook payload), CodeFlow must use the pending-spawn approach described in Section 4.

## 4. Solution Design

The fix uses two complementary layers:

### Layer 1: Backend-Aware Pending Spawn Check

**Replaces:** The old "lead_pid alive -> teammate" logic in `handle_stale_cleanup()`.

**New logic:**

```
handle_stale_cleanup()
  ... (existing checks: env file, status, config, lead_pid liveness) ...

  If lead_pid alive:
    Read pathflow-team.json -> teammates array
    Count "pending" teammates:
      pending = entries where pid == 0 AND backend_type != "in-process"
    If pending > 0:
      return (Some(existing_sid), true)   // This is a teammate
    Else:
      return (None, false)                // Independent new lead
```

**Why this works:** When the lead spawns a teammate via the `Agent` or `Task` tool, the `PostToolUse` hook writes a teammate entry to `pathflow-team.json` with `pid=0`. The teammate's own `SessionStart` then fires and finds this pending entry. After claiming it (setting its PID), the pending count decreases.

An independent new lead would find zero pending entries because:
- No one spawned it as a teammate
- No `PostToolUse` hook wrote a `pid=0` entry for it

### Layer 2: Remove Env File Fallback

**Replaces:** The env file fallback in `detect_precreated_worktree_inner()`.

**New logic:** Only use the `CODEFLOW_WORKTREE_PATH` environment variable. If the env var is not set or empty, return `None` immediately. Do not fall back to reading `codeflow-env.sh`.

**Why this works:** The `CODEFLOW_WORKTREE_PATH` env var is set by the worktree setup process within the same session. An independent new window will not have this env var set, so it will correctly get `None` and create a new worktree.

### PID Update (Companion to Layer 1)

When `handle_stale_cleanup` returns `team_mode=true`, the teammate path in `SessionStartInit::run()` now updates the first `pid==0, backend_type != "in-process"` entry in `pathflow-team.json` with the current process PID. This clears the "pending" state so subsequent windows are not misclassified as teammates.

## 5. Backend-Aware Teammate Detection

### Backend Types

The `backend_type` field on `TeammateEntry` records how Claude Code spawns the teammate:

| Backend Type | Fires SessionStart? | PID Stays 0? | Pending? |
|-------------|---------------------|-------------|----------|
| `tmux` | Yes | No (updated by SessionStart) | Yes, until SessionStart fires |
| `in-process` | No (fires SubagentStart) | Yes (permanently) | No (never pending) |
| `unknown` | Unknown | Treated as tmux | Yes, if pid == 0 |

### Resolution Flow

```
handle_teammate_spawn() [PostToolUse]
  1. Read team_name from pathflow-team.json
  2. Read ~/.claude/teams/{team_name}/config.json
  3. Find member where agentId starts with "{agent_name}@"
  4. Extract backendType from member
  5. Store as backend_type on TeammateEntry
  6. Default to "unknown" if config unreadable or member not found
```

### Pending Spawn Counting

A teammate entry is "pending" if and only if:
- `pid == 0` (SessionStart hasn't fired yet)
- `backend_type != "in-process"` (in-process teammates never fire SessionStart)

In-process teammates are excluded because their `pid` permanently stays at 0 -- counting them as pending would cause every window to be classified as a teammate when in-process teammates exist.

## 6. Scenario Matrix

| # | Scenario | Pending Count | Result | Rationale |
|---|----------|-------------|--------|-----------|
| 1 | First window, no prior session | N/A (no env file) | New lead | No existing session to detect |
| 2 | First window, prior session pf-complete | N/A (status check) | New lead | Completed session ignored |
| 3 | First window, prior session dead lead | N/A (PID check) | New lead | Dead PID -> stale session |
| 4 | Teammate spawned, pid=0, tmux | 1 | Teammate | Pending tmux spawn detected |
| 5 | Teammate spawned, pid=0, in-process | 0 | New lead | In-process excluded from pending |
| 6 | Teammate spawned, pid!=0 (already started) | 0 | New lead | Already claimed, no pending |
| 7 | Second window, no teammates spawned | 0 | New lead | Empty teammates array |
| 8 | Second window, all teammates started | 0 | New lead | All PIDs set, no pending |
| 9 | Second window, one pending + one started | 1 | Teammate | Pending entry exists |
| 10 | Compact/resume/clear source | N/A (early return) | Reuse SID | Source-based routing |
| 11 | No lead_pid in status file | N/A (PID check) | New lead | Pre-PID session |
| 12 | No team config file | N/A (config check) | New lead | Dead team |
| 13 | Team file unreadable | 0 (default) | New lead | Fallback to no pending |

## 7. Race Analysis

### Race Window

There is a brief window between when the lead calls the `Agent`/`Task` tool (PostToolUse writes `pid=0`) and when the teammate's `SessionStart` fires (reads `pid=0`, claims it).

**Attack vector:** A third Claude Code window starts during this window and finds a pending entry.

**Mitigation:** This is acceptable because:
1. The window is very short (< 1 second typically)
2. If a third window does claim a pending entry, the actual teammate's SessionStart will find no pending entries and become a new lead -- it will create its own worktree
3. The third window gets classified as a teammate, which is a safe failure mode (it joins an existing session rather than corrupting it)
4. The next teammate spawn by the lead creates another `pid=0` entry for the actual teammate

### PID Update Atomicity

The `locked_rmw` function provides file-level locking on `pathflow-team.json`, preventing concurrent reads/writes from corrupting the JSON structure.

## 8. Autorun Impact

In autorun mode, sessions run in pre-created worktrees with the `CODEFLOW_WORKTREE_PATH` environment variable set by the CLI orchestrator. The Layer 2 fix (removing env file fallback) does not affect autorun because:

1. Autorun workers always have `CODEFLOW_WORKTREE_PATH` set as an env var
2. The env var path takes precedence (unchanged)
3. The removed fallback was only relevant when the env var was absent

For Layer 1, autorun workers run in isolated worktrees where each has its own `.state/session/` directory (local, not symlinked). Teammate detection within an autorun worker functions identically to interactive mode.

## 9. Relationship to PR #226

PR #226 (merged) added heartbeat-based liveness detection as a safety mechanism. It addressed the symptom (stale sessions not being cleaned up) but not the root cause (false-positive teammate detection).

This fix addresses the root cause:
- **PR #226**: "Is the lead still alive?" (heartbeat adds reliability alongside `kill(pid, 0)` for stale session detection)
- **This fix**: "Even if the lead is alive, is this caller actually a teammate?" (pending-spawn check)

Both fixes are complementary. The heartbeat from PR #226 continues to provide liveness detection for stale session cleanup. The pending-spawn check from this fix provides session membership discrimination.

## 10. Future: Claude Code Issue #6885

The ideal fix is a platform-level change in Claude Code:

1. **`parent_session_id` in hook payload**: If Claude Code included a field indicating which lead session spawned this agent, CodeFlow could definitively distinguish teammates from independent leads without file-based inference.

2. **`SubagentStart` for tmux teammates**: Currently, in-process teammates fire `SubagentStart` while tmux teammates fire `SessionStart`. If all teammates fired `SubagentStart` regardless of backend, the lead-vs-teammate distinction would be unambiguous.

Until either change is implemented, the pending-spawn approach provides a reliable workaround with minimal race conditions.


## 11. Known Limitations

### mode=disabled does not support concurrent windows

When `worktree.mode` is set to `disabled` in `parallel-work-config.json`, sessions run directly on the main repo without worktree isolation. This means:

- **No file claim enforcement**: Multiple sessions can edit the same files simultaneously, risking conflicts.
- **No sentinel isolation**: PathFlow sentinels are shared, causing cross-session interference.
- **Single window only**: Only one Claude Code window should be active at a time when mode=disabled.

If concurrent sessions are needed, use `mode=always` (interactive) or `mode=autorun` (orchestrator-managed).

### Session-to-worktree mapping for crash recovery

A `session-worktree-map.json` file in `.state/runtime/` tracks the mapping between session IDs and worktree paths. This enables crash recovery: when a new session starts and finds a stale env file, it can look up the crashed session's worktree path from the mapping. Entries are cleaned up by SessionEnd when the worktree is removed.
