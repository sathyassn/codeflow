# Worktree Path Resolution Architecture

## Problem

Claude Code hooks need to resolve the correct worktree path during autorun sessions, but environment variables set via tmux `send-keys` do not propagate to hook processes. Hooks are spawned as child processes of Claude Code, not of the tmux session shell.

## Why Environment Variables Don't Propagate

There are three environment propagation mechanisms, none of which solve hook visibility:

1. **`CLAUDE_ENV_FILE`** -- Only affects Bash tool calls, not hooks. Hooks are spawned by Claude Code's process manager, which reads `{"env":{}}` from settings (undocumented and unreliable).

2. **tmux `send-keys export`** -- Sets variables in the tmux session's shell, but hooks are not spawned from that shell. They are spawned by Claude Code's internal process, which inherits the Claude Code process's environment, not the tmux shell's.

3. **`{"env":{}}` in settings.json** -- Undocumented. Even if it worked, it would be static at session start, before the worktree path is known.

## Per-PID File Solution

The solution uses per-PID environment files written to the worktree's `.state/runtime/` directory:

```text
.state/runtime/codeflow-env-{PID}.sh
```

### How It Works

1. **Worker creates the file** (`worker.rs`): Before invoking Claude, the worker writes `codeflow-env.sh` with `CODEFLOW_WORKTREE_PATH`, `CODEFLOW_SESSION_ID`, and `CF_PROJECT_ROOT`.

2. **SessionStart writes per-PID env file** (`session_start.rs`): When Claude Code starts inside the worktree, the SessionStart hook writes a fresh per-PID file via `fs::write()` at `codeflow-env-{PID}.sh` containing the session exports. The PID is the Claude Code process PID (stable for the session lifetime).

3. **Hooks resolve via PID** (`helpers.rs`): Each hook call reads the per-PID file to get the correct worktree path, then resolves session ID and project root from the worktree's env file. This is deterministic -- no env var propagation needed.

### Why PID Is Stable

| Scenario | PID behavior |
|----------|-------------|
| Normal session | PID assigned at process start, stable until exit |
| Context overflow (compaction) | Same process continues, PID unchanged |
| Teammate spawn (tmux) | New process, new PID, new env file |
| Teammate spawn (in-process) | Same process, same PID |

PID is stable for the lifetime of a Claude Code process. It only changes when the process exits and a new one starts.

## Session Pointer

The main `codeflow-env.sh` file serves as a session-level pointer that:

1. Provides the canonical session ID for the worktree
2. Provides the worktree path for non-PID-aware consumers
3. Is created by the worker before Claude starts, ensuring it exists before any hooks fire

## Scenario Matrix

| Scenario | Worktree | PID file | Resolution |
|----------|----------|----------|------------|
| Interactive, no worktree | Main repo | Not needed | `detect_project_dir()` resolves to repo root |
| Interactive, worktree | Worktree | Created by SessionStart | Per-PID file in worktree `.state/runtime/` |
| Autorun worker | Worktree | Created by worker + SessionStart | Per-PID file, worker pre-creates `codeflow-env.sh` |
| Autorun worker teammate | Worktree | Per-PID for teammate's PID | SessionStart detects teammate, creates per-PID pointer |
| Context overflow | Same worktree | Same PID, file persists | No change needed |
| Session end cleanup | Worktree destroyed | Files cleaned with worktree | Automatic via worktree removal |
