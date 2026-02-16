---
description: "Get help, show status, and navigate CodeFlow features"
argument-hint: "[topic]"
---

# /cf-help Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Assess current session state for context-aware help
- 🔧 respond-organized: Progressive disclosure based on topic depth
- 🔧 decide: Route to appropriate help content

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Display help information, current session status, and navigation guidance for CodeFlow features.

**Usage:**

```text
/cf-help [topic]
```

**Use When:**

- First time using CodeFlow and need orientation
- Want to see available commands and their purposes
- Need to check current session state (PathFlow phase, active teammates)
- Want to understand workflow stages or agent roles
- Need quick reference for a specific feature area

**Do Not Use When:**

- Resuming previous work (use `/cf-resume`)
- Diagnosing infrastructure issues (use `/cf-doctor`)
- Checking approval mode (use `/cf-approval-mode`)

### Pipeline Position

```text
Phase: Any (always available) | Type: Information
No pipeline dependencies — can be invoked at any point during a session.
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `topic` | No | Help topic: `commands`, `status`, `workflow`, `agents`, `skills` |

**Topics:**

| Topic | Description |
|-------|-------------|
| *(none)* | Dashboard: project info, active work, quick reference |
| `commands` | List all available `/cf-*` commands with descriptions |
| `status` | Current PathFlow phase, active teammates, branch, pending tasks |
| `workflow` | PathFlow phases (PF1-PF7), work stages, pipelines by work type |
| `agents` | Teammate roster: 3 persistent + 5 on-demand, their roles and stages |
| `skills` | Available skills and on-demand standards references |

**Examples:**

```bash
# Show dashboard
/cf-help

# Show all commands
/cf-help commands

# Show current session status
/cf-help status
```

---

## 3. Prerequisites

**Required State:**

- [ ] None -- this command works in any state

This command is always available regardless of PathFlow phase, tracking mode, or session state. It adapts its output based on what context is available.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: Any | Type: Information

/cf-help invoked
    |
    v
Parse topic argument
    |
    v
Topic provided? ---NO---> Check session state --> Show dashboard
    |
    YES
    |
    v
Route by topic:
    +--commands --> List all /cf-* commands
    +--status ---> Show session status             [cf-knowledge-layer]
    +--workflow -> Show PathFlow phases and pipelines
    +--agents ---> Show teammate roster
    +--skills ---> Show available skills
    +--unknown --> Show dashboard + suggest valid topics
    |
    v
Present formatted output
    |
    v
Next: No pipeline progression — standalone info command.
```

### 4.2 Execution Steps

**Step 1: Parse Topic**

- Check for optional topic argument
- Validate against known topics: `commands`, `status`, `workflow`, `agents`, `skills`
- If invalid topic, default to dashboard with a note about valid topics

**Step 2: Gather Context (for dashboard and status)**

- Check for active PathFlow session (`/tmp/claude/managed/state/pathflow-active`)
- Read `.state/runtime/active-task.json` if present
- Read `.state/runtime/current-session-id` if present
- Check current git branch

**Step 3: Render Topic**

Render the appropriate content based on topic selection (see Output Formats below).

**Step 4: Suggest Next Steps**

- Based on current state, suggest the most relevant next action
- If no active session: suggest `/cf-resume` or starting new work
- If mid-session: suggest the command for the current stage

### Output Formats

**Dashboard (no topic):**

```text
CodeFlow -- AI-Native Development Framework

Project: {project name from PROJECT.md}
Branch: {current branch}
PathFlow: {current phase or "No active session"}
Team: {active teammates or "No team active"}

Quick Reference:
  /cf-resume        Resume previous work
  /cf-develop       Implement code
  /cf-plan          Design or analyze
  /cf-review        Review work
  /cf-test          Run QA or write tests
  /cf-document      Write documentation
  /cf-ship          Create PR
  /cf-doctor        Health check

Type /cf-help <topic> for details:
  commands, status, workflow, agents, skills
```

**Commands topic:**

```text
Available Commands (14)

Core / Infrastructure:
  /cf-resume         Restore work context from previous session
  /cf-help           Get help, show status, and navigate features
  /cf-approval-mode  Set Claude Code approval mode
  /cf-stack          Show session state (PathFlow stack)
  /cf-doctor         Health check and diagnostics

Workflow / Routing:
  /cf-develop        Implement code (routes to cf-development)
  /cf-plan           Design, analyze, investigate (routes to cf-planning)
  /cf-review         Independent code/design/doc review (routes to cf-review)
  /cf-test           Write tests or run QA gate (routes to cf-quality-assurance)
  /cf-document       Write documentation (routes to cf-documentation)
  /cf-deploy         CI/CD pipeline work (routes to cf-development)

Lifecycle:
  /cf-ship           Create PR and complete work
  /cf-cleanup        End session, shutdown team
  /cf-autorun        Launch unattended autorun session

Stage-Gated:
  Commands marked with (PF4+) require active PathFlow execution phase.
  Core commands are always available.
```

**Status topic:**

```text
Session Status

PathFlow Phase: {PF1-PF7 or "No active session"}
Tracking Mode: {tracked/untracked/pending}
Work Type: {FEAT/FIX/RFCT/... or "N/A"}
Branch: {current branch}

Active Teammates:
  {teammate-name} -- {status: active/idle} -- since {timestamp}
  ...

Current Stage: {WS-DEV/WS-REV/WS-QA/... or "N/A"}
Pipeline Progress: {completed stages} / {total stages}

Pending Tasks:
  {task-id}: {subject} -- {status}
  ...
```

**Workflow topic:**

```text
PathFlow Session Lifecycle

PF1-INIT ---- Spawn cf-security, register session
PF2-CONTEXT - Spawn cf-knowledge-layer, load context
PF3-CLASSIFY  Classify work, create branch, spawn cf-git-operations
PF4-EXECUTE - Run work pipeline (stages depend on work type)
PF5-VERIFY -- Verify all stages passed
PF6-COMPLETE  Create PR, mark task complete
PF7-END ----- Shutdown team, cleanup

Work Type Pipelines:
  FEAT/FIX/RFCT/CICD: WS-DEV -> WS-REV -> WS-QA
  HTFX/CHOR:          WS-DEV -> WS-REV
  DOCS:               WS-DOCS -> WS-REV
  TEST:               WS-TEST -> WS-REV
  PLAN/SPKE:          WS-PLAN -> WS-REV

Rework Limits:
  Max review iterations: 3
  Max QA retries: 2
  Stage timeout (autorun): 30 min
```

**Agents topic:**

```text
Teammate Roster (8 agents)

Persistent (PF1 through PF7):
  cf-security          Security checks, sandbox, protected resources
  cf-knowledge-layer   WorkGraph, memory, DB operations
  cf-git-operations    Branch, commit, PR, sync

On-Demand (PF4-EXECUTE, per stage):
  cf-development       Code implementation + unit tests (WS-DEV)
  cf-planning          Design, architecture, analysis (WS-PLAN)
  cf-documentation     Documentation writing (WS-DOCS)
  cf-review            Independent review, 4 modes (WS-REV)
  cf-quality-assurance QA gate or test implementer (WS-QA/WS-TEST)

Agent definitions: .claude/agents/cf-*.md
```

**Skills topic:**

```text
Skills

Active (loaded at session start):
  cf-working-protocol  Cognitive procedures (5 operations)

On-Demand (loaded by agents when needed):
  cf-shell-standards   Shell scripting conventions
  cf-python-standards  Python scripting conventions
  cf-markdown-standards Markdown formatting and templates

Skills location: .claude/skills/
```

---

## 5. Skills Integration

| Teammate | Operation | Purpose |
|----------|-----------|---------|
| cf-working-protocol | meta-awareness, respond-organized | Context-aware, well-structured output |
| cf-knowledge-layer | query-events (status topic only) | Current session and task state |

**Note:** For the `status` topic, the lead queries cf-knowledge-layer for live session data. Other topics use static information from project configuration files.

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| UserPromptSubmit | `/cf-help` invoked | Validate invocation |
| Stop | Command completes | Standard session logging |

**Note:** This is a read-only information command. No Edit/Write/Bash hooks fire.

---

## 7. Memory Integration

**Memory usage:** Read-only (no work registration).

This command does not create or modify any files. It reads project configuration and runtime state to display information.

### Files Accessed

| File | Purpose |
|------|---------|
| `.state/runtime/active-task.json` | Current task context (status topic) |
| `.state/runtime/current-session-id` | Session identity (status topic) |
| `/tmp/claude/managed/state/pathflow-active` | PathFlow session flag |
| `.codeflow/config/pathflow/pathflow-config.json` | Pipeline and stage definitions (workflow topic) |
| `project/PROJECT.md` | Project name and overview (dashboard) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Unknown topic | User typed invalid topic name | Show dashboard, list valid topics |
| No runtime state | Session not initialized or state files missing | Display what's available, note "No active session" |
| Database inaccessible | `.state/db/codeflow.db` missing | Fall back to file-based state (JSONL, runtime files) |
| No teammates active | Team not created yet | Display static help, note "No team active" |

**Recovery approach:** This command never fails -- it always provides useful output by gracefully degrading when state files are missing.

---

## 9. Examples

**Example 1: Default dashboard**

```bash
/cf-help
```

Shows project overview, current state, and quick command reference.

**Example 2: List all commands**

```bash
/cf-help commands
```

Shows categorized list of all 14 `/cf-*` commands with descriptions and routing.

**Example 3: Check session status mid-work**

```bash
/cf-help status
```

Shows PathFlow phase, active teammates, current stage, and pipeline progress.

**Example 4: Understand workflow for new user**

```bash
/cf-help workflow
```

Shows PathFlow lifecycle phases, work type pipelines, and rework limits.

**Example 5: See available agents**

```bash
/cf-help agents
```

Shows full teammate roster with roles, stages, and definition file locations.

---

## 10. References

- [CLAUDE.md](../CLAUDE.md) -- Team lead instructions and full command list
- [PathFlow config](../../.codeflow/config/pathflow/pathflow-config.json) -- Phase, stage, pipeline definitions
- [PROJECT.md](../../project/PROJECT.md) -- Project overview
- [cf-resume command](./cf-resume.md) -- Resume previous work
- [cf-doctor command](./cf-doctor.md) -- Infrastructure diagnostics
- [cf-stack command](./cf-stack.md) -- Session state view
