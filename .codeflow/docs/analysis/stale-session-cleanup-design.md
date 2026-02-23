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

**Catch-22:** SessionEnd hooks skip cleanup when `pathflow-active` flag exists (to preserve state for `/resume`). But if a session dies unexpectedly (context overflow, user kills terminal, crash), the flag persists indefinitely. Over time, stale session directories accumulate in `.state/session/`.

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

## Design Decision: Warning + Manual Cleanup

**Approach:** Emit warnings at startup, never auto-delete.

### Session Detection Logic (Section 4 replacement)

```text
For each session dir in .state/session/ses-*:
  1. Skip current session
  2. Skip sessions without pathflow-active flag (already cleaned up)
  3. Read team_name from flag file
  4. Check tmux pane health for team members:
     - If any pane is alive → session is active, skip
     - If no panes alive (or no team config) → session is stale
  5. Add to warning list
```

### Output Format

```text
WARNING: STALE SESSIONS DETECTED:
  - ses-1234567890abc (pathflow-active flag set, no live tmux panes)
  - ses-9876543210def (pathflow-active flag set, no live tmux panes)
Run '/cf-cleanup --sessions' to clean up stale session state.
```

### `source` Field Decision Matrix

| Source | Has Stale Sessions? | Action |
|--------|-------------------|--------|
| `startup` | Yes | Warn (user may be about to `/resume`) |
| `startup` | No | Silent |
| `resume` | N/A | Skip stale check (state is being restored) |
| `clear` | N/A | Skip stale check (clearing current context only) |
| `compact` | N/A | Skip stale check (auto-compaction, no user interaction) |

Currently, all source values get the warning since we only warn and never delete. The `source` field is stored in `_SESSION_SOURCE` for future use.

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

- `/cf-cleanup --sessions` command implementation to manually clean stale sessions
- Consider `source` field for smarter behavior (e.g., skip warnings on `compact`)
- Go CLI integration: centralized session lifecycle management with proper cleanup
