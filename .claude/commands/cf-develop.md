---
description: "Implement a task using the cf-development teammate"
argument-hint: "\"<task-id>\" [--model MODEL] [--dry-run]"
---

# /cf-develop Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch (PAC-5 for branch safety)
- 🔧 decide: At decision points (formal vs informal mode, scope validation)
- 🔧 respond-organized: When presenting implementation results

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Implement a planned task or ad-hoc work by registering active work and dispatching to the cf-development teammate.

**Usage:**

```text
/cf-develop "<task-id>" [--model MODEL] [--dry-run]
/cf-develop "<description>"
```

**Use When:**

- Task exists in WorkGraph with approved plan (formal mode)
- Ready to write code for a planned task
- Need structured implementation with scope validation
- Ad-hoc implementation without a pre-existing task (informal mode)

**Do Not Use When:**

- Task doesn't exist and needs planning first (use `/cf-plan`)
- Writing documentation (use `/cf-document`)
- Writing tests as primary deliverable (use `/cf-test`)
- Reviewing code (use `/cf-review`)

### Pipeline Position

```text
Phase: PF4-EXECUTE | Stage: WS-DEV
Pipeline: /cf-develop --> /cf-review --> /cf-test --> /cf-ship --> /cf-cleanup
                 ^ you are here
Previous: PF3-CLASSIFY (work classification) or /cf-plan
Next: /cf-review (CODE_REVIEW mode)
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `task-id` | Yes (formal) | Task identifier from WorkGraph (e.g., "INF-TSK-008-001") |
| `description` | Yes (informal) | Free-text description of work to implement |

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--model` | `-m` | Override model (sonnet, opus, haiku) | CLI setting |
| `--dry-run` | | Validate prerequisites without executing | false |

**Mode Selection:**

| Input Pattern | Mode | Behavior |
|---------------|------|----------|
| Matches task-id format (`*-TSK-*`) | Formal | Load task from WorkGraph, enforce scope |
| Free-text description | Informal | Auto-classify, create task, then implement |

**Examples:**

```bash
# Formal: implement a planned task
/cf-develop "INF-TSK-008-001"

# Formal: with model override
/cf-develop "INF-TSK-008-001" --model opus

# Informal: ad-hoc implementation
/cf-develop "Add retry logic to the webhook handler"

# Dry run: validate without executing
/cf-develop "INF-TSK-008-001" --dry-run
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session initialized (PF1-INIT through PF3-CLASSIFY complete)
- [ ] `pathflow:pf-3` sentinel exists (Edit/Write operations enabled)
- [ ] cf-knowledge-layer teammate available
- [ ] cf-git-operations teammate available
- [ ] Not on protected branch (main, master, develop, production)
- [ ] No conflicting active_work in progress

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-knowledge-layer | Task queries, active work registration |
| cf-git-operations | Branch verification, commit operations |
| cf-development | Code implementation execution |

### 3.5 Pre-Invocation Branch Check

**Before dispatching to cf-development teammate:**

The team lead verifies the current branch is safe for implementation:

1. Query current branch via cf-git-operations: `"Verify current branch is not protected"`
2. Protected branches: `main`, `master`, `develop`, `production`
3. Expected branch prefixes: `feat/*`, `fix/*`, `refactor/*`, `ci/*`

```text
Branch Safety Check:
    |
    v
Get current branch
    |
    v
Is branch protected? ---> YES ---> ERROR: "Cannot implement on protected branch"
    |                                       "Create feature branch first"
    NO
    |
    v
Is branch prefix valid? ---> NO ---> WARN: "Unexpected branch prefix"
    |                                        "Expected: feat/*, fix/*, refactor/*, ci/*"
    YES
    |
    v
PROCEED to implementation
```

If on a protected branch, the team lead asks cf-git-operations to create an appropriate feature branch before proceeding.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF4-EXECUTE | Stage: WS-DEV | Teammate: cf-development

/cf-develop invoked
    |
    v
Parse arguments
    |
    v
Determine mode --------+
    |                   |
    v                   v
  FORMAL             INFORMAL
    |                   |
    v                   v
Query task from      Classify work
WorkGraph            (auto-detect type)              [cf-knowledge-layer]
    |                   |
    v                   v
Task exists? ---NO---> Create task in
    |                   WorkGraph
    YES                 |
    |                   |
    v                   v
Branch safety check (3.5)
    |
    v
Safe? ---NO---> Create feature branch               [cf-git-operations]
    |           via cf-git-operations
    YES             |
    |               |
    v               v
Register active work                                [cf-knowledge-layer]
(cf-knowledge-layer:begin-work)
    |
    v
Build context bundle
    |
    v
Spawn cf-development teammate                       [cf-development]
    |
    v
Teammate implements + unit tests                    [cf-development]
    |
    v
Teammate requests commit                            [cf-git-operations]
(cf-development --> cf-git-operations)
    |
    v
Complete work                                       [cf-knowledge-layer]
(cf-knowledge-layer:complete-work)
    |
    v
Present results
    |
    v
Next: /cf-review (WS-REV, CODE_REVIEW mode)
```

### 4.2 Execution Steps

**Step 1: Parse and Validate**

- Parse first argument: task-id or description
- Parse flags: `--model`, `--dry-run`
- Determine mode: Formal (task-id pattern) or Informal (free-text)
- If `--dry-run`: validate prerequisites and report, then stop

**Step 2: Load or Create Task**

- **Formal mode:** Query task from WorkGraph via cf-knowledge-layer:
  - `"LEAD: query-task -- id={task-id}"`
  - Verify task exists, extract scope, deliverables, acceptance criteria
  - If task not found: error with available tasks
- **Informal mode:** Auto-classify and create task via cf-knowledge-layer:
  - `"LEAD: classify-work -- description={description}"`
  - `"LEAD: create-task -- type={detected-type}, title={description}"`
  - Receive task-id

**Step 3: Branch Safety Check (Section 3.5)**

- Send to cf-git-operations:
  - `"Verify current branch is not protected (main, master, develop, production)"`
- If on protected branch:
  - Determine appropriate prefix from work type (FEAT=feat/, FIX=fix/, RFCT=refactor/, CICD=ci/)
  - Send to cf-git-operations: `"Please create branch: {prefix}/{task-slug}"`
- Receive branch confirmation

**Step 4: Register Active Work**

- Send to cf-knowledge-layer:
  - `"LEAD: begin-work -- task_id={task-id}, topic={task.title}, branch={branch}, scope={task.scope}, scope_policy={task.scope_policy or 'hard'}"`
- Receive active_work.id
- active_work.status set to 'in_progress'

**Step 5: Build Context Bundle**

- Gather task details: title, description, acceptance criteria, file scope
- Gather epic context (if task belongs to an epic): epic title, related tasks
- Gather branch context: current branch, recent commits

**Step 6: Assign to cf-development Teammate**

- Ensure cf-development is spawned (check team roster)
- If not alive: spawn via Task tool with instruction to read `.claude/agents/cf-development.md`
- Assign task via SendMessage:
  - recipient: `"cf-development"`
  - content: Implementation assignment with full context:
    - Task details (title, description, acceptance criteria)
    - Scope constraints (file paths, scope_policy)
    - Deliverables expected
    - Testing requirements
    - `"When complete, request commit via cf-git-operations and report to team lead."`
- Wait for teammate completion message

**Step 7: Complete Work**

- Send to cf-knowledge-layer:
  - `"LEAD: complete-work -- active_work_id={id}, summary={implementation summary}"`
- Updates active_work.status = 'complete'

**Step 8: Present Results**

- Show implementation summary from cf-development
- Show files created/modified
- Show test results (pass/fail counts)
- Show commit hash (from cf-git-operations)
- Show next steps: "Proceed to review with `/cf-review`"

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-knowledge-layer | query-task | Load task details from WorkGraph |
| cf-knowledge-layer | classify-work | Auto-classify informal requests |
| cf-knowledge-layer | create-task | Create task for informal mode |
| cf-knowledge-layer | begin-work | Register active work in DB |
| cf-knowledge-layer | complete-work | Finalize work in DB |
| cf-git-operations | verify-branch | Branch safety check |
| cf-git-operations | create-branch | Create feature branch if needed |
| cf-development | Implementation Workflow | Full implementation with tests |
| cf-development | lint-shell / lint-python | Script standards compliance |
| cf-development | ensure-test-coverage | Test coverage verification |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before Edit/Write | Verify `pathflow:pf-3` sentinel exists |
| PreToolUse:edit-write | Before Edit/Write | Validate active_work exists, check file scope |
| PreToolUse:protected-resource | Before Edit/Write on protected files | Route through staging area |
| PostToolUse:logging | After tool calls | Log implementation operations |
| SubagentStop | cf-development returns | Validate results, log session |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**PreToolUse Enforcement:**

```text
Edit/Write tool called by cf-development
    |
    v
pathflow-gate: pathflow:pf-3 sentinel exists?
    |
    +--> NO --> BLOCK (exit 2): "Complete PF3-CLASSIFY first"
    |
    +--> YES --> edit-write check
                    |
                    v
                active_work exists with status='in_progress'?
                    |
                    +--> NO --> BLOCK (exit 2): "Register active work first"
                    |
                    +--> YES --> File in scope?
                                    |
                                    +--> YES --> ALLOW
                                    +--> NO --> Apply scope_policy
                                                +--> hard: BLOCK
                                                +--> soft: WARN + expand
                                                +--> permissive: ALLOW (interactive only)
```

---

## 7. Memory Integration

### 7.1 Active Work Registration

**On Start (Step 4):**

- Invoke: cf-knowledge-layer:begin-work
- Creates record in active_work table:
  - task_id: from WorkGraph
  - topic: task title
  - branch: current feature branch
  - scope: task file scope
  - scope_policy: from task (default 'hard')
  - status: 'in_progress'
- Logs event: type='work_started'

**On Complete (Step 7):**

- Invoke: cf-knowledge-layer:complete-work
- Updates active_work:
  - status: 'complete'
  - summary: implementation summary
  - updated_at: current timestamp
- Logs event: type='work_completed'

### 7.2 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Event log (rebuild source) |
| 1 | `.state/db/codeflow.db` | Active state (query target) |
| 2 | `.claude/memory/` | Derived markdown views |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Task not found | Invalid task-id in formal mode | Query available tasks from WorkGraph |
| Protected branch | On main/master/develop/production | Create feature branch via cf-git-operations |
| Active work exists | Previous work not completed | Complete or abandon previous work |
| Scope violation | cf-development edits file outside task scope | Expand scope or create separate task |
| cf-development not available | Teammate spawn failure | Retry spawn with fresh context |
| Commit failed | cf-git-operations reports failure | Check branch state, retry commit |
| Tests failing | cf-development reports test failures | Review failures, fix and re-test |

**Recovery Procedures:**

```text
ON "Task not found" error:
  1. Query cf-knowledge-layer for available tasks
  2. Present matching tasks to user
  3. Retry with valid task-id or switch to informal mode

ON "Protected branch" error:
  1. Determine branch prefix from work type
  2. Send to cf-git-operations: "Please create branch: {prefix}/{task-slug}"
  3. Retry implementation on new branch

ON "Active work exists" error:
  1. Query active_work WHERE status='in_progress'
  2. Complete it: cf-knowledge-layer:complete-work
  3. OR abandon: cf-knowledge-layer:abandon-work
  4. Retry command

ON "Scope violation" during implementation:
  1. cf-development reports scope issue to team lead
  2. Team lead evaluates: expand scope or create separate task
  3. If expanding: cf-knowledge-layer:expand-scope
  4. If separate: create new task, defer out-of-scope changes
```

---

## 9. Examples

**Example 1: Formal Implementation of Planned Task**

```bash
/cf-develop "INF-TSK-008-001"
```

Output:

```text
Implementing: INF-TSK-008-001 "Configure OAuth2 provider integration"
Mode: FORMAL
Branch: feat/oauth2-providers (verified safe)
Active work: registered

cf-development: implementing...
  - Created: src/auth/oauth2-provider.py
  - Created: src/auth/token-exchange.py
  - Updated: src/config/settings.py
  - Created: tests/test_oauth2_provider.py
  - Tests: 12 passed, 0 failed
  - Committed: feat(auth): configure OAuth2 provider integration

DEV-COMPLETE: 4 files changed, 12 tests passing.
Next: Proceed to review with /cf-review
```

**Example 2: Informal Ad-Hoc Implementation**

```bash
/cf-develop "Add retry logic to the webhook handler"
```

Output:

```text
Implementing: "Add retry logic to the webhook handler"
Mode: INFORMAL
Classified: FIX (auto-detected)
Task created: BKD-TSK-FIX-WEBHOOK-001
Branch: fix/webhook-retry (created)
Active work: registered

cf-development: implementing...
  - Updated: src/webhooks/handler.py
  - Created: tests/test_webhook_retry.py
  - Tests: 8 passed, 0 failed
  - Committed: fix(webhooks): add retry logic to webhook handler

DEV-COMPLETE: 2 files changed, 8 tests passing.
Next: Proceed to review with /cf-review
```

**Example 3: Dry Run Validation**

```bash
/cf-develop "INF-TSK-008-001" --dry-run
```

Output:

```text
DRY RUN: Validating prerequisites for INF-TSK-008-001

  [OK] Task exists in WorkGraph
  [OK] Branch: feat/oauth2-providers (not protected)
  [OK] No conflicting active work
  [OK] pathflow:pf-3 sentinel present
  [OK] Scope: src/auth/*, tests/test_auth_*

Ready to implement. Run without --dry-run to proceed.
```

---

## 10. References

- [cf-development agent](../agents/cf-development.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [cf-shell-standards skill](../skills/cf-shell-standards/SKILL.md)
- [cf-python-standards skill](../skills/cf-python-standards/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-plan command](cf-plan.md)
- [cf-review command](cf-review.md)
- [cf-test command](cf-test.md)
