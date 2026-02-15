# Agent Teams Hook Findings

> Empirical findings on how Claude Code Agent Teams tools interact with the hook
> system (PreToolUse, PostToolUse). Tested 2026-02-15 on the CodeFlow project.

---

## 1. Background

Claude Code has two hook lifecycle events that can intercept tool calls:

- **PreToolUse**: Fires BEFORE a tool executes. Can block (exit 2) or allow (exit 0).
- **PostToolUse**: Fires AFTER a tool executes. Cannot block, used for logging/side-effects.

Both use a `matcher` field (regex) to filter which tools trigger the hook.

**Question:** Which Agent Teams tools fire these hooks, and what `tool_name` values
appear in the hook's stdin JSON?

---

## 2. Tools Involved

### Filesystem Tools (standard)

| Tool | Purpose |
|------|---------|
| Read | Read file contents |
| Write | Create/overwrite files |
| Edit | String replacement in files |
| Bash | Execute shell commands |
| Grep | Search file contents |
| Glob | Find files by pattern |

### Agent Teams Tools

| Tool | Purpose | LLM-Facing? |
|------|---------|-------------|
| TeamCreate | Create a new team (config + task list) | Yes |
| TeamDelete | Delete team and clean up | Yes |
| Task | Spawn sub-agents OR teammates (with team_name param) | Yes |
| SendMessage | Send messages between teammates, shutdown requests/responses | Yes |
| TaskCreate | Create task in team task list | Yes |
| TaskUpdate | Update task status/details | Yes |
| TaskList | List all tasks | Yes |
| TaskGet | Get single task details | Yes |

### Internal Tools (not directly called by LLM)

| Tool | Purpose | Notes |
|------|---------|-------|
| Teammate | Internal routing target for team member operations | Seen ONLY by PreToolUse hooks with explicit `"Teammate"` matcher |

---

## 3. Test Methodology

### Test 1: `.*` Matcher (Passive Observation)

Examined the PostToolUse logging hook output (`tool-use-2026-02-15.jsonl`) from a
full session with an active team (phase5-commands, 5 teammates spawned).

**Result:** 2,189 tool calls logged. Only 5 tool types appeared:

| Tool | Count |
|------|-------|
| Read | 951 |
| Bash | 772 |
| Grep | 274 |
| Edit | 121 |
| Write | 71 |

Zero entries for TeamCreate, Task (teammate spawn), SendMessage, TeamDelete,
TaskCreate, TaskUpdate, or TaskList.

**Conclusion:** `.*` regex does NOT match Agent Teams tools.

### Test 2: Explicit Matcher (Active Test)

Created a test PostToolUse hook script at `/tmp/claude/test-posttool-team.sh` that
logs `tool_name` to a file. Added to `settings.local.json` with matcher:

```json
{
  "matcher": "TeamCreate|TeamDelete|Teammate|SendMessage|Task",
  "hooks": [{
    "type": "command",
    "command": "bash /tmp/claude/test-posttool-team.sh",
    "timeout": 1
  }]
}
```

Then executed the following operations and observed the log:

| # | Operation Performed | Log Entry |
|---|-------------------|-----------|
| 1 | `TeamCreate(team_name="tool-test-2")` | `tool_name=TeamCreate` |
| 2 | `Task(name="test-buddy-2", team_name="tool-test-2")` | `tool_name=Task` |
| 3 | `SendMessage(type="message", recipient="test-buddy-2")` | `tool_name=SendMessage` |
| 4 | `SendMessage(type="shutdown_request", recipient="test-buddy-2")` | `tool_name=SendMessage` |
| 5 | Teammate's shutdown_response (automatic) | `tool_name=SendMessage` |
| 6 | `TeamDelete()` (failed - teammate still active) | `tool_name=TeamDelete` |
| 7 | Automatic idle/notification message | `tool_name=SendMessage` |
| 8 | `TeamDelete()` (succeeded) | `tool_name=TeamDelete` |

**All operations fired PostToolUse when explicitly matched.**

---

## 4. Key Findings

### Finding 1: `.*` Does NOT Match Agent Teams Tools

The wildcard regex `.*` only matches filesystem-interaction tools (Read, Write,
Edit, Bash, Grep, Glob). Agent Teams tools require explicit naming in the matcher.

This is likely a Claude Code platform behavior where team management tools are in
a different tool category that `.*` doesn't cover.

### Finding 2: Teammate Spawn Shows as `Task`, Not `Teammate`

When spawning a teammate via `Task(name=..., team_name=...)`:

- **PreToolUse** sees `tool_name="Teammate"` (settings.json has `"matcher": "Teammate"` for team-guard)
- **PostToolUse** sees `tool_name="Task"`

This means Claude Code internally routes the `Task` call to the `Teammate` tool
for PreToolUse hooks, but reports it back as `Task` for PostToolUse hooks.

### Finding 3: All SendMessage Variants Share One tool_name

Regular messages, shutdown requests, shutdown responses, and automatic notifications
all appear as `tool_name="SendMessage"`. Differentiate by parsing `tool_input`:

- `tool_input.type` = "message", "shutdown_request", "shutdown_response", "broadcast"
- `tool_input.content` = message body (check for patterns like "STAGE-COMPLETE")

### Finding 4: TeamDelete Fires Even on Failure

When TeamDelete fails (e.g., "Cannot cleanup team with active members"), the
PostToolUse hook still fires. The hook can check `tool_result` for success/failure.

### Finding 5: TaskCreate/TaskUpdate/TaskList Not Tested

These tools were not explicitly tested with a dedicated matcher. They likely
follow the same pattern (require explicit matcher, won't match `.*`).

---

## 5. Implications for PathFlow Sentinel Hook

The sentinel PostToolUse hook needs this matcher to detect all phase transitions:

```text
TeamCreate|Task|SendMessage|Bash
```

| Sentinel | tool_name | Detection Logic |
|----------|-----------|-----------------|
| pathflow-pf-1 | `TeamCreate` | Always create on TeamCreate |
| pathflow-pf-2 | `Task` | Check `tool_input.name` contains "cf-knowledge-layer" |
| pathflow-pf-3 | `Bash` | Check command matches `git (checkout -b\|switch -c)` |
| pathflow-ws-* | `SendMessage` | Check `tool_input.content` matches `STAGE-COMPLETE: WS-*` |
| pathflow-pf-6 | `Bash` | Check command matches `gh pr create` |

`Bash` is already covered by `.*`, but including it explicitly for clarity and
to avoid depending on `.*` behavior which may change.

---

## 6. PreToolUse vs PostToolUse Tool Name Comparison

| Operation | PreToolUse tool_name | PostToolUse tool_name |
|-----------|---------------------|----------------------|
| Teammate spawn | `Teammate` | `Task` |
| TeamCreate | (no PreToolUse hook) | `TeamCreate` |
| TeamDelete | `TeamDelete` (team-guard) | `TeamDelete` |
| SendMessage | (no PreToolUse hook) | `SendMessage` |
| Bash | `Bash` (pathflow-gate) | `Bash` |
| Edit | `Edit` (pathflow-gate, edit-write) | `Edit` |
| Write | `Write` (pathflow-gate, edit-write) | `Write` |

Note: The team-guard PreToolUse hook has `"matcher": "Teammate"` and checks for
both `tool_name == "Teammate"` (line 58) and `tool_name == "TeamDelete"` (line 53).
This confirms "Teammate" is a distinct internal tool name used during PreToolUse
routing, even though the LLM calls `Task` and PostToolUse reports `Task`.

---

## 7. Related Documents

- [agent-teams-test-findings.md](agent-teams-test-findings.md) — Earlier research on teammate detection, context sharing, and session behavior
- [pathflow-v3/08-enforcement-model.md](pathflow-v3/08-enforcement-model.md) — PathFlow enforcement design including flag file format
- GitHub issue #6885 — Request for agentName/teamName in hook stdin JSON
