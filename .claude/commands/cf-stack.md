---
description: "Show current session state and PathFlow stack"
argument-hint: "[--verbose]"
---

# /cf-stack Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Assess current session state for display
- 🔧 respond-organized: Progressive disclosure based on verbosity level

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Display the current PathFlow session stack -- phases, stages, teammates, and work state in a structured view.

**Usage:**

```text
/cf-stack [--verbose]
```

**Use When:**

- Need to see where you are in the PathFlow lifecycle
- Want to check which teammates are active and their states
- Debugging phase progression or stage ordering
- Verifying sentinel and flag consistency
- Quick status check during a long session

**Do Not Use When:**

- Need comprehensive help or command list (use `/cf-help`)
- Diagnosing infrastructure problems (use `/cf-doctor`)
- Resuming previous work (use `/cf-resume`)

---

## 2. Arguments & Flags

**Arguments:** None.

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--verbose` | `-v` | Show detailed state: sentinels, flag files, JSONL events, rework counters | false |

**Examples:**

```bash
# Standard stack view
/cf-stack

# Verbose with sentinel and event details
/cf-stack --verbose
```

---

## 3. Prerequisites

**Required State:**

- [ ] None -- works in any state (shows "No active session" if nothing is running)

This command is always available. It adapts its output based on what session state exists.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
flowchart TD
    Start([/cf-stack invoked]) --> Parse[Parse --verbose flag]
    Parse --> CheckSession{PathFlow session active?}

    CheckSession -->|No| NoSession[Display: No active PathFlow session]
    NoSession --> ShowBranch[Show git branch and basic state]
    ShowBranch --> End([Complete])

    CheckSession -->|Yes| GatherState[Gather session state]
    GatherState --> ReadPhase[Read current PathFlow phase]
    ReadPhase --> ReadStage[Read current work stage]
    ReadStage --> ReadTeam[Read active teammates]
    ReadTeam --> ReadTask[Read current task]

    ReadTask --> IsVerbose{--verbose?}
    IsVerbose -->|No| RenderStandard[Render standard stack view]
    IsVerbose -->|Yes| ReadSentinels[Read sentinels and flags]
    ReadSentinels --> ReadEvents[Read recent JSONL events]
    ReadEvents --> ReadRework[Read rework counters]
    ReadRework --> RenderVerbose[Render verbose stack view]

    RenderStandard --> End
    RenderVerbose --> End
```

### 4.2 Execution Steps

**Step 1: Parse Flags**

- Check for `--verbose` flag
- Set display depth accordingly

**Step 2: Check PathFlow Session**

- Check for `pathflow-active` flag at `/tmp/claude/managed/state/pathflow-active`
- If absent: display "No active PathFlow session" with branch info and exit
- If present: proceed to gather full state

**Step 3: Gather Session State**

- Read `.state/runtime/active-task.json` for current task context
- Read `.state/runtime/current-session-id` for session identity
- Query cf-knowledge-layer for: current phase, current stage, teammate roster, task details

**Step 4: Determine Phase and Stage**

- Read `.state/ledger/pathflow-events.jsonl` for latest `phase_transition` event
- Identify current phase (PF1-PF7) and stage within PF4 (WS-DEV, WS-REV, WS-QA, etc.)
- Calculate pipeline progress (completed stages / total stages)

**Step 5: Read Team State**

- Read team config to determine spawned teammates
- Classify each as: persistent (function) or on-demand (role)
- Note idle vs active status

**Step 6: Render Stack View**

Standard view shows the session overview. Verbose view adds sentinel details, recent events, and rework state.

### Output Formats

**Standard view:**

```text
PathFlow Stack

Session: {session-id}
Phase:   PF4-EXECUTE [=====>----] 4/7
Branch:  feat/user-auth
Work:    FEAT -- User authentication system

Pipeline: WS-DEV -> WS-REV -> WS-QA
           [done]   [active]  [pending]

Teammates:
  cf-security          persistent  active
  cf-knowledge-layer   persistent  active
  cf-git-operations    persistent  idle
  cf-review            on-demand   active (WS-REV)
```

**Verbose view (adds):**

```text
Sentinels:
  pathflow:pf-3          present
  pathflow:ws-dev-done   present
  pathflow:ws-rev-done   absent

Flags:
  pathflow-active        /tmp/claude/managed/state/pathflow-active  present
  is-pathflow-active     /tmp/claude/managed/state/is-pathflow-active  present

Recent Events (last 5):
  2026-02-15T10:30:00  phase_transition  PF4-EXECUTE
  2026-02-15T10:31:00  stage_start       WS-DEV
  2026-02-15T11:45:00  stage_complete    WS-DEV (pass)
  2026-02-15T11:46:00  stage_start       WS-REV
  2026-02-15T12:00:00  rework_request    WS-REV -> WS-DEV (iteration 1)

Rework State:
  Review iterations: 1/3
  QA retries: 0/2
```

**No session view:**

```text
PathFlow Stack

No active PathFlow session.

Branch: main (clean)
Mode:   Untracked

Start a tracked session with /cf-develop, /cf-plan, or /cf-document.
```

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | meta-awareness, respond-organized | State assessment and structured output |
| cf-knowledge-layer | query-events | PathFlow event history and task state |

**Note:** For quick checks, the lead reads runtime state files directly (Read tool). For comprehensive status, cf-knowledge-layer is queried.

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-stack` invoked | Validate invocation |
| Stop | Command completes | Standard session logging |

**Note:** This is a read-only command. No Edit/Write/Bash hooks fire.

---

## 7. Memory Integration

**Memory usage:** Read-only (no work registration).

This command reads session state files to display information. It does not modify any files.

### Files Accessed

| File | Purpose |
|------|---------|
| `/tmp/claude/managed/state/pathflow-active` | PathFlow session flag |
| `/tmp/claude/managed/state/is-pathflow-active` | PathFlow mode flag |
| `.state/runtime/active-task.json` | Current task context |
| `.state/runtime/current-session-id` | Session identity |
| `.state/ledger/pathflow-events.jsonl` | Phase/stage transition history |
| `.state/sentinels/` | Sentinel files (verbose mode) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| No runtime state | Session files missing or not initialized | Display "No active session" with branch info |
| JSONL unreadable | Corrupted or empty event log | Display available state, note "Event log unavailable" |
| Team config missing | Team not created or already dissolved | Display "No team active" |
| Stale flag file | `pathflow-active` exists but session ended abnormally | Note staleness; suggest `/cf-doctor` for cleanup |

**Recovery approach:** This command never fails -- it displays whatever state is available and notes what's missing.

---

## 9. Examples

**Example 1: Standard view during active session**

```bash
/cf-stack
```

Shows phase progress, pipeline status, and active teammates.

**Example 2: Verbose view for debugging**

```bash
/cf-stack --verbose
```

Adds sentinel state, flag file checks, recent JSONL events, and rework counters.

**Example 3: No active session**

```bash
/cf-stack
```

Output:

```text
PathFlow Stack

No active PathFlow session.

Branch: main (clean)
Mode:   Untracked

Start a tracked session with /cf-develop, /cf-plan, or /cf-document.
```

**Example 4: Mid-rework view**

```bash
/cf-stack
```

Output:

```text
PathFlow Stack

Session: sess-20260215-abc123
Phase:   PF4-EXECUTE [=====>----] 4/7
Branch:  fix/login-redirect
Work:    FIX -- Login redirect bug

Pipeline: WS-DEV -> WS-REV -> WS-QA
           [rework]  [done*]  [pending]
           * returned changes_requested (iteration 1/3)

Teammates:
  cf-security          persistent  active
  cf-knowledge-layer   persistent  active
  cf-git-operations    persistent  idle
  cf-development       on-demand   active (WS-DEV rework)
```

---

## 10. References

- [PathFlow config](../../.codeflow/config/pathflow/pathflow-config.json) -- Phase, stage, pipeline definitions
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md) -- Event queries
- [CLAUDE.md](../CLAUDE.md) -- PathFlow lifecycle documentation
- [cf-help command](./cf-help.md) -- Help and navigation
- [cf-doctor command](./cf-doctor.md) -- Infrastructure diagnostics
- [cf-resume command](./cf-resume.md) -- Resume previous work
