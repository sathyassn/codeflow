---
id: BRIEF-004
title: "PID-Based Teammate Detection and Stale Session Auto-Cleanup"
status: draft
author: cf-development
created: "2026-02-23"
---

# PID-Based Teammate Detection and Stale Session Auto-Cleanup

## Table of contents

- [Problem statement](#problem-statement)
- [Solution overview](#solution-overview)
- [pathflow-team.json schema](#pathflow-teamjson-schema)
- [Hook timing sequence](#hook-timing-sequence)
- [Decision tree at SessionStart](#decision-tree-at-sessionstart)
- [Cleanup scope](#cleanup-scope)
- [Worktree selective symlink](#worktree-selective-symlink)
- [Validated findings](#validated-findings)
- [Edge cases](#edge-cases)
- [Integration with checkpoint enforcement](#integration-with-checkpoint-enforcement)

## Problem statement

When a Claude Code session crashes (context overflow, terminal kill, OOM), SessionEnd hooks either don't fire or skip cleanup because the `pathflow-active` flag exists. This leaves stale artifacts:

| Artifact | Location | Impact |
|----------|----------|--------|
| Env file | `.state/runtime/codeflow-env.sh` | Next session sources stale session ID |
| Session directory | `.state/session/{SID}/` | Checkpoint data contaminates new session |
| PathFlow flag | `.state/session/{SID}/pathflow/is-pathflow-active` | SessionEnd guard skips cleanup |
| Team config | `~/.claude/teams/{team_name}/` | Stale member entries block team operations |
| Task list | `~/.claude/tasks/{team_name}/` | Stale task entries confuse task tracker |
| Sentinels | `.state/sentinels/pathflow/{SID}/` | Gate enforcement uses wrong session state |
| Active task | `.state/runtime/active-task.json` | Wrong task context loaded |

The current approach (v1.4.0) emits warnings at SessionStart but never auto-deletes. LLMs consistently ignore these warnings and proceed with stale state, leading to sentinel lookup failures, checkpoint contamination, and phantom teammates.

## Solution overview

A new bridge file `pathflow-team.json` records the team lead's OS process PID at TeamCreate time. At SessionStart, the init hook checks whether the lead PID is still alive via `kill -0`. This distinguishes three scenarios:

| Scenario | PID check result | Action |
|----------|-----------------|--------|
| Teammate starting | Lead PID alive | Skip cleanup, minimal init |
| New lead after crash | Lead PID dead | Full auto-cleanup, fresh start |
| Fresh session (no team file) | N/A | Normal init, no cleanup needed |

### Why PID-based detection works

- `$PPID` in hooks equals the Claude Code process PID (direct parent of the hook subprocess)
- Hooks run OUTSIDE Claude Code's sandbox -- `kill -0`, `ps`, `tmux` all work without bypass
- Each Claude Code process (lead vs teammate) has a distinct OS PID
- `kill -0` is signal 0 -- checks process existence only, does NOT send any signal

### Why warning-only was insufficient

The previous "warning + manual cleanup" approach relied on the LLM reading stderr warnings and acting on them. In practice:

1. LLMs do not reliably process stderr warnings during SessionStart
2. Even when processed, LLMs often proceed optimistically rather than cleaning up
3. The stale env file is sourced BEFORE warnings are emitted, contaminating the session ID
4. Manual `/cf-cleanup --sessions` requires user intervention that may not happen

## pathflow-team.json schema

**Location:** `.state/session/{SID}/pathflow/pathflow-team.json`

```json
{
  "team_name": "string",
  "lead_claude_uuid": "string",
  "lead_pid": 12345,
  "codeflow_session_id": "ses-...",
  "teammate_spawned": false,
  "created_at": "2026-02-23T10:00:00Z",
  "last_spawn_name": null
}
```

| Field | Type | Set by | Purpose |
|-------|------|--------|---------|
| `team_name` | string | PostToolUse on TeamCreate | Team identifier for config/task cleanup |
| `lead_claude_uuid` | string | PostToolUse on TeamCreate | Claude Code's internal session UUID for the lead |
| `lead_pid` | integer | PostToolUse on TeamCreate | OS PID of the lead Claude Code process (`$PPID`) |
| `codeflow_session_id` | string | PostToolUse on TeamCreate | CodeFlow session ID from env file |
| `teammate_spawned` | boolean | PostToolUse on Task | Whether any teammate has been spawned |
| `created_at` | string | PostToolUse on TeamCreate | ISO 8601 timestamp |
| `last_spawn_name` | string/null | PostToolUse on Task | Name of most recently spawned teammate |

### Why in pathflow directory

The file lives under `.state/session/{SID}/pathflow/` because:

1. It is session-scoped (cleaned with the session directory)
2. It is PathFlow-specific (only created during tracked sessions)
3. The pathflow directory already exists (created by SessionStart)
4. It is co-located with the pathflow-active flag and checkpoint file

## Hook timing sequence

```text
1. SessionStart hook fires
   ├── Section 1: Read stdin, generate session ID
   ├── NEW: PID-based stale session cleanup (before env file source)
   │   ├── Check env file exists
   │   ├── Source env file temporarily for old SID
   │   ├── Check pathflow-team.json for old SID
   │   └── kill -0 lead_pid → alive (teammate) or dead (cleanup)
   ├── Section 2: Setup (env file source, libraries)
   └── Sections 3-9: Normal init

2. TeamCreate PostToolUse fires
   └── Write pathflow-team.json with team_name, lead PID, session ID

3. Task PostToolUse fires (teammate spawn)
   └── Update pathflow-team.json: teammate_spawned=true, last_spawn_name

4. Teammate's SessionStart fires
   ├── Sources env file → gets lead's CODEFLOW_SESSION_ID
   ├── Reads pathflow-team.json → gets lead_pid
   ├── kill -0 lead_pid → ALIVE → teammate path
   └── Minimal init (skip checkpoint re-init, skip flag creation)

5. SessionEnd fires (normal shutdown)
   └── Removes pathflow-team.json (and team config/task list)
```

## Decision tree at SessionStart

```text
Does env file exist?
├── NO → Fresh start, no cleanup needed → normal init
└── YES → Source env file, get old CODEFLOW_SESSION_ID
          │
          Does pathflow-team.json exist for old SID?
          ├── NO → Check pathflow-active flag
          │        ├── FLAG MISSING → Orphan env file → clean env file only
          │        └── FLAG EXISTS → Lead's own session pre-TeamCreate → proceed normally
          └── YES → Read lead_pid from pathflow-team.json
                    │
                    kill -0 $lead_pid
                    ├── ALIVE → Teammate starting → skip cleanup, minimal init
                    └── DEAD → Stale session → FULL CLEANUP:
                              a. Read team_name from pathflow-team.json
                              b. rm -rf ~/.claude/teams/{team_name}/
                              c. rm -rf ~/.claude/tasks/{team_name}/
                              d. rm -rf .state/session/{SID}/
                              e. rm -rf .state/sentinels/pathflow/{SID}/
                              f. rm .state/runtime/codeflow-env.sh
                              g. rm .state/runtime/active-task.json
                              h. rm .state/runtime/current-session-id
                              i. Unset CODEFLOW_SESSION_ID
                              j. Log cleanup to stderr
```

## Cleanup scope

| Artifact | Path | Cleanup action | Why |
|----------|------|----------------|-----|
| Team config | `~/.claude/teams/{team_name}/` | `rm -rf` | Stale member entries with dead panes |
| Task list | `~/.claude/tasks/{team_name}/` | `rm -rf` | Stale task entries from previous session |
| Session directory | `.state/session/{SID}/` | `rm -rf` | Contains pathflow flag, checkpoint, team file |
| Sentinels | `.state/sentinels/pathflow/{SID}/` | `rm -rf` | Session-scoped, stale gate state |
| Env file | `.state/runtime/codeflow-env.sh` | `rm -f` | Contains stale session ID |
| Active task | `.state/runtime/active-task.json` | `rm -f` | References stale task |
| Session ID ref | `.state/runtime/current-session-id` | `rm -f` | References stale session |

After cleanup, the hook unsets `CODEFLOW_SESSION_ID` so a fresh one is generated by the normal Session ID generation code.

## Worktree selective symlink

The current worktree setup creates a full `.state` symlink to the main repo. This causes worktree teammates to share session-scoped state (sentinels, session directories) with the main repo, leading to cross-session contamination.

### Shared vs local state

| Directory | Shared? | Reason |
|-----------|---------|--------|
| `.state/db/` | Yes | Single SQLite database for all worktrees |
| `.state/ledger/` | Yes | Single JSONL event log for all worktrees |
| `.state/registry/` | Yes | Shared registry data |
| `.state/backups/` | Yes | Shared backup location |
| `.state/coordination/` | Yes | CRDT coordination data |
| `.state/logs/` | Yes | Shared log aggregation |
| `.state/runtime/` | No | Session-specific (env file, active task) |
| `.state/session/` | No | Session-scoped PathFlow state |
| `.state/sentinels/` | No | Session-scoped sentinel files |

### Migration from full symlink

If `.state` is currently a full symlink (old setup), the worktree setup script removes it and creates the selective structure:

```text
.state/                    (directory, not symlink)
├── db -> main/.state/db          (shared)
├── ledger -> main/.state/ledger  (shared)
├── registry -> main/.state/registry  (shared)
├── backups -> main/.state/backups    (shared)
├── coordination -> main/.state/coordination  (shared)
├── logs -> main/.state/logs          (shared)
├── runtime/               (local, per-worktree)
├── session/               (local, per-worktree)
└── sentinels/             (local, per-worktree)
```

## Validated findings

### Finding 1: $PPID in hooks equals Claude Code PID

Hooks are executed as subprocesses of the Claude Code process. `$PPID` in the hook's execution context is the PID of the Claude Code process that spawned the hook. This was validated by comparing `$PPID` in hook output with the actual Claude Code PID from `ps`.

### Finding 2: Hooks run outside sandbox

Claude Code hooks execute in the user's normal shell environment, not inside the sandbox. This means system commands like `kill -0`, `ps`, `tmux list-panes`, and file operations on paths outside the sandbox allowlist all work correctly.

### Finding 3: kill -0 checks process existence

`kill -0 $pid` sends signal 0 to the process. Signal 0 does not actually send any signal -- it only checks whether the process exists and the caller has permission to send signals to it. Exit code 0 means the process exists; non-zero means it does not.

### Finding 4: Distinct PIDs for lead vs teammate

When Claude Code spawns teammates via tmux, each teammate process has its own distinct OS PID. The lead's PID is recorded at TeamCreate time, and teammates can verify the lead is alive by checking that specific PID.

### Finding 5: In-process teammates share lead PID

For in-process teammate backends (non-tmux), teammates run in the same process as the lead. `kill -0` on the lead PID would return alive, which is the correct behavior -- the lead is alive, so no cleanup is needed.

## Edge cases

| Edge case | Likelihood | Handling |
|-----------|-----------|---------|
| PID recycling (dead lead's PID reused by new process) | Astronomically unlikely on modern systems (PID space is large, recycling is sequential) | Acceptable risk. The window between lead death and new SessionStart is typically seconds, not enough for full PID space cycling. |
| In-process teammates | Common (Claude Code default) | Correct behavior: same PID as lead, `kill -0` succeeds, treated as teammate (no cleanup) |
| Crash within seconds of TeamCreate | Rare | pathflow-team.json may not exist yet. Falls through to "no team file" path, checks pathflow-active flag instead. |
| Team config missing at cleanup time | Possible (manually deleted) | Skip team config cleanup gracefully, continue with other cleanup |
| Env file missing | Clean start | No cleanup needed, proceed normally |
| Multiple stale sessions | Possible | Only the session referenced by the env file is cleaned. Other stale sessions still get warning treatment. |
| Concurrent teammate spawns | Possible | pathflow-team.json writes may race. Use atomic write (tmp + mv) for safety. |

## Integration with checkpoint enforcement

The pathflow-team.json file is stored in the same session directory as the checkpoint file:

```text
.state/session/{SID}/pathflow/
    is-pathflow-active              <-- PathFlow flag
    pathflow-phase-tasks.json       <-- checkpoint state (phase-checkpoint hooks)
    pathflow-team.json              <-- NEW: team lead PID and metadata
```

When stale session cleanup occurs, the entire `.state/session/{SID}/` directory is removed, which cleans up all three files atomically. This ensures no partial state survives a cleanup.

The checkpoint pre-initialization (`checkpoint_init_all_phases()`) runs after the cleanup decision, so a cleaned session gets a fresh checkpoint file as part of normal init.

## Related

- `stale-session-cleanup-design.md` -- previous warning-only approach (superseded for crash scenarios)
- `phase-checkpoint-enforcement.md` -- checkpoint system that shares the session directory
- `cf-session-start-init.sh` -- SessionStart hook (modified to add PID-based cleanup)
- `cf-post-tool-use-pathflow-sentinel.sh` -- PostToolUse hook (modified to create pathflow-team.json)
- `cf-session-end-cleanup.sh` -- SessionEnd hook (modified to clean pathflow-team.json)
- `cf-worktree-setup.sh` -- worktree setup (modified for selective symlinks)
