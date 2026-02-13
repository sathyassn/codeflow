# Context Detection: Hook-Level Caller Identification

## Purpose

Documents empirical findings on detecting the execution context (main agent, Task sub-agent, Agent Teams teammate) from within PreToolUse hooks. These findings inform how CodeFlow's read-delegation hook and other enforcement hooks determine whether to apply restrictions.

## Architecture

### Library: `context-lib.sh`

**Location**: `.codeflow/scripts/security/lib/context-lib.sh`

| Function | Purpose | Detection Method |
|---|---|---|
| `_find_progress_entry()` | Internal: search transcript for progress entries | Grep + jq parse |
| `is_sub_agent()` | Detect Task sub-agent context | `agent_progress` entries in transcript |
| `is_forked_context_skill()` | Detect forked skill context | `skill_progress` entries in transcript |
| `is_sub_context()` | Combined check (sub-agent OR forked skill) | Calls both above |
| `is_pathflow_active()` | Detect if PathFlow mode is active | Flag file in `.state/session/` |

### Read-Delegation Hook Logic

```text
Read/Edit/Write tool called
  │
  ├─ is_sub_context()? ──────── yes → ALLOW (isolated context)
  │
  ├─ is_pathflow_active()? ────── yes → ALLOW (PathFlow manages its own context)
  │
  ├─ File under threshold? ───── yes → ALLOW
  │
  └─ File over threshold? ───── yes → BLOCK (suggest delegation)
```

## Hook Stdin Fields (Empirical)

PreToolUse hooks receive JSON on stdin with exactly 8 fields:

| Field | Description |
|---|---|
| `cwd` | Current working directory |
| `hook_event_name` | Hook event type (e.g., "PreToolUse") |
| `permission_mode` | Permission mode (e.g., "default", "bypassPermissions") |
| `session_id` | Session identifier |
| `tool_input` | Tool input parameters (JSON string) |
| `tool_name` | Tool name (e.g., "Read", "Bash") |
| `tool_use_id` | Unique tool call identifier |
| `transcript_path` | Path to session transcript (.jsonl) |

## Detection Matrix

| Caller Type | `session_id` | `transcript_path` | Transcript Entry Type | Detectable? |
|---|---|---|---|---|
| Main agent | Lead's ID | Lead's transcript | `assistant` (standalone) | N/A (default) |
| Task sub-agent | **Same as lead** | **Same as lead** | `progress/agent_progress` | **YES** via `is_sub_context()` |
| Forked skill | **Same as lead** | **Same as lead** | `progress/skill_progress` | **YES** via `is_sub_context()` |
| Agent Teams teammate | **Same as lead** | **Same as lead** | `assistant` (standalone) | **NO** — indistinguishable from lead |

### Key Finding: All In-Process Entities Share Identity

All in-process entities (lead, sub-agents, teammates) receive **identical** hook stdin:

- Same `session_id`
- Same `transcript_path`
- Same `permission_mode`
- Only `tool_use_id` differs (unique per call, but opaque — no caller info encoded)

## Sub-Agent Detection: How `is_sub_context()` Works

### Mechanism

When a Task sub-agent makes a tool call, Claude Code writes an `agent_progress` entry to the main transcript **before** the PreToolUse hook fires. This entry contains the sub-agent's `tool_use_id` in its nested data.

### Timeline (Sub-Agent Read)

1. Sub-agent generates assistant message with `tool_use:Read` (id=`toolu_XXX`)
2. Claude Code writes `agent_progress` entry to transcript (contains `toolu_XXX`)
3. PreToolUse hook fires for the Read tool
4. Hook greps transcript for `toolu_XXX` → finds `agent_progress` entry
5. `is_sub_agent()` sees `data.type=agent_progress` with `agentId` → returns 0
6. Hook exits 0 → **ALLOWED**

### Timeline (Main Agent Read)

1. Main agent generates assistant message with `tool_use:Read` (id=`toolu_YYY`)
2. Claude Code writes `assistant` entry + `hook_progress` entries to transcript
3. PreToolUse hook fires
4. Hook greps for `toolu_YYY` → finds `assistant` and `hook_progress` entries
5. `_find_progress_entry()` finds `hook_progress` (type=progress) but `data.type=hook_progress` ≠ `agent_progress`
6. `is_sub_agent()` returns 1 → not a sub-agent
7. Hook continues to threshold check → **BLOCKED** if over threshold

## Teammate Detection: Why It Fails

### Root Cause

Agent Teams teammates (in-process, `backendType: "in-process"`) produce transcript entries that are **structurally identical** to the team lead's entries:

- Same `type=assistant` for tool calls
- Same `teamName` field value (both set to the team name)
- No `agentId` or `agentName` field
- No `agent_progress` wrapper entries
- No pre-hook transcript entries at all (only post-hook `user` tool results)

### Approaches Tested and Disproven

| Approach | Why It Fails |
|---|---|
| `session_id` comparison vs `leadSessionId` | In-process teammates share the lead's `session_id` |
| Transcript `agent_progress` entries | Teammates produce NO `agent_progress` entries |
| `teamName` field in transcript | Both lead and teammates have the same `teamName` value |
| Environment variables (`CLAUDE_CODE_AGENT_NAME`, etc.) | Not set despite documentation |
| Separate `transcript_path` | Teammates share the lead's transcript |

### Workaround: `is_pathflow_active()`

Since individual teammates cannot be identified, the hook checks whether **PathFlow mode is active** via a session flag file at `.state/session/<session-id>/is-pathflow-active`. When active:

- All sessions (lead + teammates) are exempted from read-delegation
- The flag is created at PF-1 (session start) and removed at PF-7/session-end

This is an acceptable trade-off because PathFlow mode implies structured delegation where context is already managed through the team coordination pattern.

### Long-Term Fix

GitHub issue #6885 requests additional fields in hook stdin JSON:

- `IsAgentContext` / `AgentName` / `ParentSessionID` / `AgentDepth`

This would enable precise per-caller detection without workarounds.

## Verified Scenarios

| # | Context | Caller | Result | Mechanism |
|---|---|---|---|---|
| 1 | Standalone | Main agent (large file) | BLOCKED | Threshold enforced |
| 2 | Standalone | Task sub-agent (large file) | ALLOWED | `is_sub_context()` |
| 3 | Standalone | Main agent (small file) | ALLOWED | Under threshold |
| 4 | Team active | Lead (large file) | ALLOWED | `is_pathflow_active()` |
| 5 | Team active | Teammate (large file) | ALLOWED | `is_pathflow_active()` |
| 6 | Team active | Sub-agent (large file) | ALLOWED | `is_sub_context()` |
| 7 | Team disbanded | Main agent (large file) | BLOCKED | Config deleted → threshold enforced |

## References

- Context library: `.codeflow/scripts/security/lib/context-lib.sh`
- Context tests: `.codeflow/testing/scripts/security/lib/test-context-lib.sh` (58 tests)
- Read-delegation hook: `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-read-delegation.sh`
- Read-delegation tests: `.codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-read-delegation.sh` (71 tests)
- GitHub issue: #6885 (agentName/teamName in hook stdin)
- Prior research: `agent-teams-test-findings.md`
