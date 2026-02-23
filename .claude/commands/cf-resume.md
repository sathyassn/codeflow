---
description: "Restore work context from previous session"
argument-hint: "[work-id]"
---

# /cf-resume Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Context state assessment
- 🔧 think-and-act: Before loading context or routing
- 🔧 decide: At routing decision points (Tier 1/2/3)
- 🔧 respond-organized: Status summary presentation

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Restore full work context from a previous session and resume where you left off.

**Usage:**

```text
/cf-resume [work-id]
```

**Use When:**

- Starting a new session after a break
- Recovering context after an interruption
- Returning to work after days or weeks away
- Resuming a specific tracked work item by ID

**Do Not Use When:**

- Starting entirely new work (use `/cf-develop`, `/cf-plan`, etc.)
- Just checking status without intending to resume (use `/cf-help status`)
- No previous work exists to resume

### Pipeline Position

```text
Phase: PF2-CONTEXT | Type: Session recovery
Flow: /cf-resume --> [routes to current stage command]
Entry point for resuming previous work. Routes to the appropriate stage command.
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `work-id` | No | Specific work ID to resume (e.g., task ID from WorkGraph) |

**Flags:**

None.

**Examples:**

```bash
# Resume most recent work (auto-detect)
/cf-resume

# Resume specific work item
/cf-resume "FRT-TSK-001-001"
```

---

## 3. Prerequisites

**Required State:**

- [ ] Team infrastructure initialized (TeamCreate completed)
- [ ] cf-knowledge-layer teammate spawned (PF2-CONTEXT or later)
- [ ] Database accessible (`.state/db/codeflow.db`)

**Required Tables:**

| Table | Purpose |
|-------|---------|
| `sessions` | Previous session records |
| `tasks` | Work items to resume |
| `pathflow_events` | Phase/stage state for restoration |

**No branch restrictions** -- this command works from any branch.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF2-CONTEXT | Type: Session Recovery

/cf-resume invoked
    |
    v
Parse work-id argument
    |
    v
Query active work                              [cf-knowledge-layer]
    |
    v
Active work found?
    |
    +--NONE------> "No active work. Start fresh."
    |
    +--MULTIPLE--> Show options to user
    |                  |
    |                  v
    |              User selects work item
    |                  |
    +--SINGLE------+---+
                   |
                   v
Load work context
    |
    v
Restore state
    |
    v
Verify/switch to work branch                  [cf-git-operations]
    |
    v
Identify PathFlow phase
    |
    v
Respawn teammates from saved roster
    |
    v
Present context summary
    |
    v
Route to appropriate stage:
    +--WS-DEV incomplete ---> cf-development
    +--WS-PLAN incomplete --> cf-planning
    +--WS-DOCS incomplete --> cf-documentation
    +--WS-REV incomplete ---> cf-review
    +--WS-QA incomplete ----> cf-quality-assurance
    +--All stages done -----> Advance to next PathFlow phase
    |
    v
Next: [routes to current stage command]
```

### 4.2 Execution Steps

**Step 1: Parse Arguments**

- Check for optional `work-id` argument
- If provided, use as filter for work query
- If omitted, query all active/in-progress work

**Step 2: Query Active Work**

- SendMessage to cf-knowledge-layer: `"load-work-context domain=all"`
- cf-knowledge-layer queries:
  - Active tasks (status = 'in_progress')
  - Recent sessions with incomplete work
  - PathFlow events for phase/stage state
- Returns: List of resumable work items with metadata

**Step 3: Handle Work Selection**

- **No work found:** Display "No active work found" with suggestions for starting new work
- **Single work item:** Proceed directly to context loading
- **Multiple items:** Present numbered list with details (topic, branch, last active, progress) and ask user to select

**Step 4: Load Work Context**

- SendMessage to cf-knowledge-layer: `"query-events task_id={id}"`
- Load: task details, work type, area, branch, scope, acceptance criteria
- Load: PathFlow phase markers and stage completion state
- Load: memory context for the work domain

**Step 5: Restore Branch State**

- Verify the work branch still exists
- Check current branch vs work branch
- If different: inform user which branch to switch to (delegate to cf-git-operations if needed)

**Step 6: Restore PathFlow Phase**

- Read pathflow events to determine last completed phase
- Identify the current phase and stage within PF4-EXECUTE (if applicable)
- Check sentinel state for consistency

**Step 7: Respawn Teammates**

- Respawn persistent teammates (cf-security, cf-knowledge-layer, cf-git-operations) if not already active
- Determine which on-demand teammate is needed based on current stage
- Spawn the appropriate on-demand teammate with work context

**Step 8: Present Summary**

- Display context recovery summary:
  - Work item: topic, ID, work type
  - Branch and git status
  - PathFlow phase and stage
  - Progress (completed stages, remaining stages)
  - Last activity timestamp
  - Next action

**Step 9: Route to Appropriate Teammate**

- Based on the current incomplete stage, route work to the matching teammate
- Pass full context (task details, scope, acceptance criteria, previous progress)
- If all PF4 stages complete, advance to PF5-VERIFY

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | meta-awareness, think-and-act, decide | Cognitive procedures |
| cf-knowledge-layer | load-work-context | Query active work and session state |
| cf-knowledge-layer | query-events | Load PathFlow event history |
| cf-knowledge-layer | search-related-work | Find related work items |
| cf-git-operations | verify-branch | Check branch exists and is current |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-resume` invoked | Validate invocation |
| PreToolUse | Before Read operations | Standard file access validation |
| Stop | Command completes | Log session resume event |

**Note:** This is primarily a read-and-route command. PreToolUse Edit/Write hooks are not triggered by the resume command itself -- they fire when the routed teammate begins work.

---

## 7. Memory Integration

**Memory usage:** Read-only during resume; write to register resumed session.

### 7.1 Context Loading

- **Query:** cf-knowledge-layer loads active work from Tier 1 (SQLite)
- **Events:** PathFlow events from Tier 0 (JSONL) determine phase state
- **Context:** Domain-specific memory from Tier 2 (markdown) provides work context

### 7.2 Session Registration

**On Resume:**

- cf-knowledge-layer registers new session record linked to existing task
- PathFlow events log `phase_transition` for resumed phase
- `pathflow-active` flag is set if entering tracked mode

### 7.3 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Event log (phase/stage history) |
| 1 | `.state/db/codeflow.db` | Active state (task, session queries) |
| 2 | `.claude/memory/` | Derived views (domain context) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| No active work found | All previous work completed or abandoned | Display idle state, suggest `/cf-develop` or `/cf-plan` |
| Work branch deleted | Branch was cleaned up after merge | Check if PR was merged; if yes, mark complete; if no, recreate branch |
| Database inaccessible | `.state/db/codeflow.db` missing or corrupt | Fall back to JSONL events (Tier 0) for state reconstruction |
| Stale PathFlow markers | Sentinels from previous session inconsistent | Re-derive phase state from JSONL events, regenerate sentinels |
| Teammate spawn failure | Context budget exceeded | Recycle: fresh spawn with essential context only |
| Work-id not found | Invalid or completed work ID | Show available active work items |

**Recovery Procedures:**

```text
ON "Database inaccessible":
  1. Check .state/db/ exists
  2. Query .state/logs/pathflow-events.jsonl for latest events
  3. Reconstruct task state from event log
  4. Rebuild SQLite from JSONL if needed

ON "Stale PathFlow markers":
  1. Read pathflow-events.jsonl for authoritative state
  2. Clear stale sentinels from .state/sentinels/
  3. Regenerate from event history
  4. Resume from verified state
```

---

## 9. Examples

**Example 1: Resume with single active work item**

```bash
/cf-resume
```

Output:

```text
Context Recovery Summary

Git Status: feat/user-auth (clean)
Last Active: 2026-02-14 16:30 (18 hours ago)
Work Type: FEAT

Current Work:
- Topic: User authentication system
- Task: FRT-TSK-001-001
- PathFlow Phase: PF4-EXECUTE
- Current Stage: WS-DEV (in progress)
- Progress: WS-DEV started, WS-REV and WS-QA pending

Next Steps:
- Resuming WS-DEV stage with cf-development...
```

**Example 2: Resume with multiple active items**

```bash
/cf-resume
```

Output:

```text
Multiple Active Work Items Found

1. feat/user-auth -- User authentication (FEAT)
   Last active: 18 hours ago | Stage: WS-DEV
2. fix/login-redirect -- Login redirect bug (FIX)
   Last active: 2 days ago | Stage: WS-REV

Which would you like to resume? (1 or 2)
```

**Example 3: Resume specific work item by ID**

```bash
/cf-resume "FRT-TSK-001-001"
```

Output:

```text
Context Recovery Summary

Resuming: FRT-TSK-001-001 (User authentication system)
Branch: feat/user-auth
Phase: PF4-EXECUTE / WS-DEV

Spawning cf-development to continue implementation...
```

**Example 4: No active work**

```bash
/cf-resume
```

Output:

```text
No Active Work Found

No in-progress work items detected.

Ready to start fresh:
- /cf-develop -- Implement a feature or fix
- /cf-plan -- Design or analyze
- /cf-document -- Write documentation
- /cf-help -- See all available commands
```

---

## 10. References

- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-security agent](../agents/cf-security.md)
- [PathFlow config](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-help command](./cf-help.md)
- [cf-stack command](./cf-stack.md)
