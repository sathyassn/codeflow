# Stale Session Cleanup Design

## Discovery: SessionStart `source` Field

Claude Code's SessionStart hook receives a JSON payload on stdin that includes a `source` field:

| Value | Meaning | When Fired |
|-------|---------|------------|
| `startup` | Fresh `claude` CLI invocation | User runs `claude` command |
| `resume` | Built-in `/resume` command | User types `/resume` in existing session |
| `clear` | Built-in `/clear` command | User types `/clear` to reset context |
| `compact` | Auto-compaction | Claude Code auto-compacts at ~95% context |

This field is critical for distinguishing session lifecycle events. Previously it was not parsed.

## The Stale Session Problem

**Catch-22:** SessionEnd hooks skip cleanup when `pathflow-active` flag exists (to preserve state for `/resume`). The flag is removed by PostToolUse on TeamDelete (after TeamDelete succeeds). But if a session dies unexpectedly (context overflow, user kills terminal, crash), the flag persists indefinitely. Over time, stale session directories accumulate in `.state/session/`.

**Previous approach (v1.3.0):** 24-hour timer-based cleanup. `find -mmin +1440` identified stale directories and deleted them, preserving those with active pathflow flags.

**Problem with timer approach:**

- Too aggressive: deletes sessions the user might want to `/resume` within 24h
- Too lenient: sessions abandoned for < 24h pollute state
- Wrong mechanism: age-based cleanup doesn't distinguish "abandoned" from "paused"

## Why Auto-Cleanup at Startup Was Rejected

The `/resume` command triggers SessionStart with `source: "startup"` first (the initial `claude` invocation), then the user types `/resume` to restore context. If auto-cleanup ran during `startup`, it would:

1. Detect stale session directories (no live tmux panes)
2. Delete them, including their pathflow-active flags, checkpoint files, and sentinel directories
3. User types `/resume` — but the state they need was just deleted

This is the fundamental catch-22: **cleanup and resume share the same entry point**.

## Design Decision: PID-Based Auto-Cleanup

**Approach:** Use the lead's OS process PID to distinguish teammates from stale sessions. Auto-cleanup when the lead PID is dead.

The previous warning-only approach was insufficient because LLMs consistently ignore stderr warnings and proceed with stale state. PID-based detection replaces warnings with automatic cleanup for crash scenarios.

### Session Detection Logic (Section 4 replacement)

```text
At SessionStart, BEFORE sourcing the env file:
  1. Check if env file exists → if not, fresh start, skip
  2. Source env file temporarily to get old CODEFLOW_SESSION_ID
  3. Check .state/session/{SID}/pathflow/pathflow-team.json:
     - MISSING: Check pathflow-active flag
       - FLAG MISSING: orphan env file → clean env file only
       - FLAG EXISTS: lead's own session pre-TeamCreate → proceed normally
     - EXISTS: Read lead_pid
       - kill -0 $lead_pid → ALIVE: teammate → skip cleanup, minimal init
       - kill -0 $lead_pid → DEAD: stale session → FULL CLEANUP
```

### Full Cleanup Scope

When the lead PID is dead, the following artifacts are removed:

| Artifact | Path | Reason |
|----------|------|--------|
| Team config | `~/.claude/teams/{team_name}/` | Stale member entries |
| Task list | `~/.claude/tasks/{team_name}/` | Stale task entries |
| Session directory | `.state/session/{SID}/` | Pathflow flag, checkpoint, team file |
| Sentinels | `.state/sentinels/pathflow/{SID}/` | Stale gate state |
| Env file | `.state/runtime/codeflow-env.sh` | Stale session ID |
| Active task | `.state/runtime/active-task.json` | Stale task context |
| Session ID ref | `.state/runtime/current-session-id` | Stale session reference |

After cleanup, `CODEFLOW_SESSION_ID` is unset so a fresh ID is generated.

### `source` Field Decision Matrix

| Source | Has Stale Session (env file)? | Action |
|--------|-------------------------------|--------|
| `startup` | Yes, lead PID dead | Auto-cleanup, fresh start |
| `startup` | Yes, lead PID alive | Teammate path, minimal init |
| `startup` | No env file | Fresh start, normal init |
| `resume` | N/A | Normal init (env file source proceeds) |
| `clear` | N/A | Normal init (env file source proceeds) |
| `compact` | N/A | Normal init (env file source proceeds) |

The `source` field is stored in `_SESSION_SOURCE` for future use. PID-based cleanup runs regardless of source value because it is safe: alive PIDs are never cleaned.

### Stale Team Config Cleanup

When a stale session is detected (lead PID dead), the cleanup includes team infrastructure at `~/.claude/`:

1. Read `team_name` from `pathflow-team.json`
2. Remove `~/.claude/teams/{team_name}/` (team config with stale member entries)
3. Remove `~/.claude/tasks/{team_name}/` (task list with stale entries)

This prevents the next session from detecting stale team configs and emitting redundant warnings (Section 9).

## Tmux Pane Health Check Approach

To determine if a session is stale (no live processes):

1. Read the pathflow-active flag file to get `team_name`
2. Look up team config at `~/.claude/teams/{team_name}/config.json`
3. For each team member, check if their `tmuxPaneId` is alive:

   ```bash
   tmux list-panes -a -F '#{pane_id}' | grep "^${pane_id}$"
   ```

4. If ANY member pane is alive, the session is considered active
5. If NO panes are alive (or team config missing), the session is stale

### Edge Cases

| Condition | Handling |
|-----------|----------|
| tmux not installed | Skip all pane checks, emit debug note to stderr |
| tmux server not running | Skip all pane checks, emit debug note to stderr |
| Session without `team_name` in flag | Treated as stale (no team to verify) |
| Team config missing for `team_name` | Treated as stale (team was deleted/cleaned) |
| All member panes dead | Session is stale — warn |
| At least one member pane alive | Session is active — skip |
| API-based agents (no tmux) | All pane checks skipped, no false positives |

## Stale Team Config Detection (Section 9)

Separate from stale session detection, the hook also checks for stale team
configs — teams where member tmux panes are dead but the config file persists.
This is a strong signal of context overflow recovery situations.

### Approach

1. Scan `~/.claude/teams/*/config.json` for team configs
2. If tmux is unavailable, pane checks silently treat panes as dead (no explicit
   skip or debug note — the `tmux list-panes` command fails and grep finds no match)
3. For each team config, iterate over `members` array
4. Skip members without a `tmuxPaneId` field (can't check pane state)
5. For members with `tmuxPaneId`, check pane liveness:
   `tmux list-panes -a -F '#{pane_id}' | grep "^{paneId}$"`
6. If any member pane is dead, emit warning with team name
7. Suggest `/cf-doctor` for recovery steps

### Edge Cases

| Condition | Handling |
|-----------|----------|
| `~/.claude/teams/` doesn't exist | Skip gracefully, no warning |
| `jq` not available | Skip team config parsing entirely |
| Malformed team config JSON | Skip config with jq error suppression (`\|\| continue`) |
| Member with empty/null `tmuxPaneId` | Skip that member, don't count as stale |
| tmux not running | Skip all pane checks, emit debug note |
| Zero members in team | Skip team, no warning |

## Code Locations

| Component | Path |
|-----------|------|
| SessionStart hook | `.claude/hooks/codeflow/session-start/cf-session-start-init.sh` |
| SessionEnd hook | `.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh` |
| `/cf-cleanup` command | `.claude/commands/cf-cleanup.md` |
| PathFlow state library | `.codeflow/scripts/state/cf-pathflow-state.sh` |
| Session state directory | `.state/session/{SID}/` |
| Pathflow flag | `.state/session/{SID}/pathflow/is-pathflow-active` |

## Future Work

- `/cf-cleanup --sessions` command implementation to manually clean stale sessions (for edge cases not covered by auto-cleanup)
- Go CLI integration: centralized session lifecycle management with proper cleanup
- `.state/` directory reorganization: separate shared vs session-scoped state at the directory level (partially addressed by worktree selective symlink)
