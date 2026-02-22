---
description: "Create, refine, and finalize epics for feature planning"
argument-hint: "\"<description>\" [--refine] [--finalize]"
---

# /cf-plan Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch
- 🔧 decide: At decision points (mode selection, scope assessment)
- 🔧 respond-organized: When presenting plan results

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Create, refine, or finalize epics and implementation plans by dispatching work to the cf-planning teammate.

**Usage:**

```text
/cf-plan "<description>" [--refine] [--finalize]
```

**Use When:**

- Starting a new feature that needs planning before implementation
- Refining an existing epic based on feedback or new requirements
- Finalizing a plan for handoff to development (locks the plan, creates PR)
- Decomposing a large request into epics and tasks

**Do Not Use When:**

- Ready to implement an already-planned task (use `/cf-develop`)
- Writing documentation without planning artifacts (use `/cf-document`)
- Investigating a bug (use `/cf-develop` with FIX classification)

### Pipeline Position

```text
Phase: PF4-EXECUTE | Stage: WS-PLAN
Pipeline: /cf-plan --> /cf-review
Previous: PF3-CLASSIFY (work classification)
Next: /cf-review (DESIGN_REVIEW mode)
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `description` | Yes (create mode) | Description of the feature or work to plan |
| `epic-id` | No | Epic identifier to refine or finalize (auto-detects from branch if omitted) |

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--refine` | `-r` | Iterate on an existing epic | false |
| `--finalize` | `-f` | Lock epic and create PR | false |

**Mode Selection:**

| Flags Present | Mode | Behavior |
|---------------|------|----------|
| Neither | Create | Create a new epic from description |
| `--refine` | Refine | Iterate on existing epic (auto-detect or specify epic-id) |
| `--finalize` | Finalize | Lock epic, validate completeness, create PR |

**Examples:**

```bash
# Create a new epic
/cf-plan "Add user authentication with OAuth2 and JWT tokens"

# Refine an existing epic (auto-detect from branch)
/cf-plan --refine

# Refine a specific epic
/cf-plan "INF-EPC-CICD-001" --refine

# Finalize and create PR
/cf-plan --finalize
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session initialized (PF1-INIT complete)
- [ ] cf-knowledge-layer teammate available (PF2-CONTEXT complete)
- [ ] For refine/finalize: existing epic in WorkGraph or detectable from branch

**Stage Availability:**

- WS-PLAN during PF4-EXECUTE (standard flow)
- Pre-stage (PF1-INIT or PF2-CONTEXT) for ad-hoc planning before work classification

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-knowledge-layer | Work item persistence (epic/task creation) |
| cf-git-operations | Branch creation (`plan/*`), PR creation |
| cf-planning | Planning execution (design, decomposition, documentation) |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF4-EXECUTE | Stage: WS-PLAN | Teammate: cf-planning

/cf-plan invoked
    |
    v
Parse arguments and flags
    |
    v
Determine mode ----+--------------------+
    |               |                    |
    v               v                    v
  CREATE          REFINE             FINALIZE
    |               |                    |
    v               v                    v
Classify work   Load existing        Load existing
(PLAN type)     epic from DB         epic from DB       [cf-knowledge-layer]
    |               |                    |
    v               v                    v
Register task   Verify epic          Validate plan
in WorkGraph    exists               completeness
    |               |                    |
    v               v                    v
Create plan/*   +---+                Lock epic
branch          |                    (status=finalized)  [cf-knowledge-layer]
    |           |                        |
    v           v                        v
Spawn cf-planning teammate           Create PR           [cf-git-operations]
    |                                    |
    v                                    v
Teammate creates/refines epic        Present PR URL
    |                                [cf-planning]
    v
Teammate decomposes into tasks                          [cf-planning]
    |
    v
Teammate requests commit                                [cf-git-operations]
    |
    v
Present results to user
    |
    v
Next: /cf-review (WS-REV, DESIGN_REVIEW mode)
```

### 4.2 Execution Steps

**Step 1: Parse Arguments**

- Parse description (positional argument)
- Parse flags: `--refine`, `--finalize`
- Determine mode: Create (no flags), Refine (`--refine`), Finalize (`--finalize`)
- If refine/finalize without explicit epic-id: detect from current branch (`plan/*` prefix)

**Step 2: Classify Work**

- Work type: `PLAN`
- Pipeline: `WS-PLAN --> WS-REV`
- Send to cf-knowledge-layer:
  - `"LEAD: classify-work -- type=PLAN, area={detected}, description={description}"`

**Step 3: Register Task in WorkGraph (Create mode)**

- Send to cf-knowledge-layer:
  - `"LEAD: create-task -- type=PLAN, title=Plan: {description}"`
- Receive task-id confirmation

**Step 4: Create Branch (Create mode)**

- Send to cf-git-operations:
  - `"Please create branch: plan/{epic-slug}"`
- Receive branch confirmation

**Step 5: Register Active Work**

- Send to cf-knowledge-layer:
  - `"LEAD: begin-work -- task_id={task-id}, topic={description}, branch={branch}"`
- Receive active_work.id

**Step 6: Assign to cf-planning Teammate**

- Ensure cf-planning is spawned (check team roster)
- If not alive: spawn via Task tool with instruction to read `.claude/agents/cf-planning.md`
- Assign task via SendMessage:
  - recipient: `"cf-planning"`
  - content: Task description with mode, context, and deliverable expectations:
    - **Create:** `"Create epic for: {description}. Analyze requirements, design solution, decompose into tasks with acceptance criteria. Write to project-management/epics/. Commit via cf-git-operations."`
    - **Refine:** `"Refine epic {epic-id}. Review feedback: {context}. Update scope, tasks, and estimates as needed. Commit updates via cf-git-operations."`
    - **Finalize:** `"Finalize epic {epic-id}. Validate all sections complete, acceptance criteria testable, no placeholders. Lock status."`
- Wait for teammate completion message

> **PLN area routing:** The planning session itself is tracked as a task under PLN-EPC-001 (Ongoing Planning epic). The resulting epic and tasks go under their target area (INF/, DOC/, etc.). Design analysis documents go to `docs/analysis/` with descriptive names referencing the epic ID, not inside `project-management/` subdirectories.

**Step 7: Complete Work**

- Send to cf-knowledge-layer:
  - `"LEAD: complete-work -- active_work_id={id}, summary={planning summary}"`

**Step 8: Present Results**

- Show epic summary (title, scope, task count)
- Show files created/modified
- Show task breakdown with estimates
- Show next steps:
  - Create: "Review the plan, then `/cf-plan --refine` to iterate or `/cf-plan --finalize` to lock"
  - Refine: "Iterate again or `/cf-plan --finalize` to lock"
  - Finalize: "Plan locked. PR created at {url}. Start implementation with `/cf-develop`"

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-knowledge-layer | classify-work | Determine work type and area |
| cf-knowledge-layer | create-task | Register planning task in WorkGraph |
| cf-knowledge-layer | begin-work | Register active work for tracking |
| cf-knowledge-layer | complete-work | Finalize work tracking |
| cf-git-operations | create-branch | Create `plan/*` branch |
| cf-git-operations | create-pull-request | Create PR for finalized plan |
| cf-planning | analyze-requirements | Parse and scope the planning request |
| cf-planning | design-architecture | Architectural decisions and trade-offs |
| cf-planning | decompose-work | Break into epics and tasks |
| cf-planning | create-epic | Define epic boundaries and metadata |
| cf-planning | create-task | Define individual work items |
| cf-planning | validate-plan-structure | Verify completeness before commit |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before Edit/Write | Verify PF3-CLASSIFY complete (sentinel check) |
| PreToolUse:edit-write | Before Edit/Write | Scope enforcement for file operations |
| PostToolUse:logging | After tool calls | Log planning operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**Stage Gating:**

The pathflow-gate hook enforces that Edit/Write operations only proceed after PF3-CLASSIFY creates the `pathflow:pf-3` sentinel. Planning commands that run pre-stage (during PF1/PF2) operate through teammate dispatch, which handles sentinel creation as part of the classification flow.

---

## 7. Memory Integration

### 7.1 Active Work Registration

**On Start (Step 5):**

- Invoke: cf-knowledge-layer:begin-work
- Creates record in active_work table with:
  - task_id: planning task
  - topic: plan description
  - branch: `plan/*` branch
  - scope: `project-management/epics/`, `.codeflow/docs/`
  - scope_policy: `hard`
- Logs event: type='work_started'

**On Complete (Step 7):**

- Invoke: cf-knowledge-layer:complete-work
- Updates active_work.status = 'complete'
- Logs event: type='work_completed'

### 7.2 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Event log (rebuild source) |
| 1 | `.state/db/codeflow.db` | Active state (query target) |
| 2 | `project-management/` | Human-readable derived views (epics, tasks) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| No description provided | Create mode without description | Prompt user for description |
| Epic not found | Refine/finalize with invalid epic-id | List available epics from WorkGraph |
| Branch already exists | `plan/*` branch name collision | Use existing branch or create with suffix |
| cf-planning not available | Teammate spawn failure | Retry spawn with fresh context |
| Active work exists | Previous planning work not completed | Complete or abandon previous work |
| Ambiguous requirements | Description too vague for planning | cf-planning escalates to team lead for clarification |

**Recovery Procedures:**

```text
ON "Epic not found" error:
  1. Query cf-knowledge-layer for available epics
  2. Present list to user
  3. Retry with valid epic-id

ON "Active work exists" error:
  1. Query active_work WHERE status='in_progress'
  2. Complete it: cf-knowledge-layer:complete-work
  3. OR abandon: cf-knowledge-layer:abandon-work
  4. Retry command
```

---

## 9. Examples

**Example 1: Create a New Epic**

```bash
/cf-plan "Add OAuth2 authentication with Google and GitHub providers"
```

Output:

```text
Planning: "Add OAuth2 authentication with Google and GitHub providers"
Mode: CREATE
Branch: plan/oauth2-authentication

Epic created: INF-EPC-AUTH-001
Tasks:
  1. INF-TSK-AUTH-001: Configure OAuth2 provider integration (M, high confidence)
  2. INF-TSK-AUTH-002: Implement token exchange flow (S, high confidence)
  3. INF-TSK-AUTH-003: Add session management (M, medium confidence)

Next: Review the plan, then /cf-plan --refine to iterate or /cf-plan --finalize to lock.
```

**Example 2: Refine an Existing Epic**

```bash
/cf-plan --refine
```

Output:

```text
Refining epic INF-EPC-AUTH-001 (detected from branch plan/oauth2-authentication)
Updated: Added error handling tasks, revised estimates.

Next: /cf-plan --finalize when ready to lock.
```

**Example 3: Finalize and Create PR**

```bash
/cf-plan --finalize
```

Output:

```text
Finalizing epic INF-EPC-AUTH-001
Validation: All sections complete, 5 tasks with acceptance criteria.
Status: FINALIZED (locked)
PR: #12 - plan(auth): OAuth2 authentication epic

Next: Start implementation with /cf-develop "INF-TSK-AUTH-001"
```

---

## 10. References

- [cf-planning agent](../agents/cf-planning.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [cf-markdown-standards skill](../skills/cf-markdown-standards/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-develop command](cf-develop.md)
- [cf-review command](cf-review.md)
