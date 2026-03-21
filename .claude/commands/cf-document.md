---
description: "Create or update documentation"
argument-hint: "\"<type>\" \"<description>\""
---

# /cf-document Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch (PAC-5 for file operations)
- 🔧 decide: At decision points (document type selection, scope assessment)
- 🔧 respond-organized: When presenting documentation results

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Create or update project documentation by dispatching work to the cf-documentation teammate, with proper work tracking, branch management, and quality validation.

**Usage:**

```text
/cf-document "<type>" "<description>"
/cf-document "<description>"
```

**Use When:**

- Creating new ADRs, briefs, runbooks, guides, or API docs
- Updating existing documentation to reflect code changes
- Documenting architecture decisions or design rationale
- Writing project README or contributor guides

**Do Not Use When:**

- Writing code or inline comments (use `/cf-develop`)
- Writing tests as primary deliverable (use `/cf-test`)
- Creating planning artifacts like epics (use `/cf-plan`)
- Reviewing documentation (use `/cf-review`)

### Pipeline Position

```text
Phase: PF4-EXECUTE | Stage: WS-DOCS
Pipeline: /cf-document --> /cf-review --> /cf-ship --> /cf-cleanup
                  ^ you are here
Previous: PF3-CLASSIFY (work classification)
Next: /cf-review (DOCUMENTATION_REVIEW mode)
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `type` | No | Document type: `adr`, `brief`, `runbook`, `readme`, `api`, `guide` |
| `description` | Yes | Description of the documentation to create or update |

**Document Type Details:**

| Type | Template | Default Location |
|------|----------|-----------------|
| `adr` | Architecture Decision Record | `.codeflow/docs/decisions/` |
| `brief` | Analysis or findings document | `.codeflow/docs/research/` |
| `runbook` | Operational procedure | `.codeflow/docs/runbooks/` |
| `readme` | Project or module README | Project root or module directory |
| `api` | API reference documentation | `docs/api/` |
| `guide` | How-to or getting started guide | `docs/guides/` |

**Type Auto-Detection:**

If `type` is omitted, the team lead classifies the document type from the description:

| Keywords in Description | Detected Type |
|------------------------|---------------|
| decision, alternative, chose, trade-off | `adr` |
| analysis, findings, research, investigation | `brief` |
| procedure, runbook, steps, operations | `runbook` |
| readme, getting started, overview | `readme` |
| api, endpoint, reference, schema | `api` |
| guide, how-to, tutorial, walkthrough | `guide` |

**Examples:**

```bash
# Explicit type: ADR
/cf-document "adr" "Choose JWT over sessions for authentication"

# Explicit type: runbook
/cf-document "runbook" "Create deployment rollback procedure"

# Explicit type: readme update
/cf-document "readme" "Update project README with new setup instructions"

# Auto-detected type (ADR detected from "architecture decisions")
/cf-document "Document the webhook retry architecture decisions"
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

**Stage Availability:**

- WS-DOCS during PF4-EXECUTE (primary documentation work)
- WS-DEV as companion documentation alongside code changes

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-knowledge-layer | Work registration, task tracking |
| cf-git-operations | Branch creation (`docs/*`), commit operations |
| cf-documentation | Documentation writing and quality validation |

### 3.5 Pre-Invocation Branch Check

**Before dispatching to cf-documentation teammate:**

The team lead verifies the current branch is safe for documentation work:

1. Query current branch via cf-git-operations: `"Verify current branch is not protected"`
2. Protected branches: `main`, `master`, `develop`, `production`
3. Expected branch prefix: `docs/*` (primary) or `feat/*`, `fix/*` (companion docs)

```text
Branch Safety Check:
    |
    v
Get current branch
    |
    v
Is branch protected? ---> YES ---> ERROR: "Cannot write docs on protected branch"
    |                                       "Create docs/* branch first"
    NO
    |
    v
Is branch prefix valid? ---> NO ---> WARN: "Unexpected branch prefix"
    |                                        "Expected: docs/*, feat/*, fix/*"
    YES
    |
    v
PROCEED to documentation
```

If on a protected branch, the team lead asks cf-git-operations to create a `docs/*` branch before proceeding.

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF4-EXECUTE | Stage: WS-DOCS | Teammate: cf-documentation

/cf-document invoked
    |
    v
Parse arguments (type, description)
    |
    v
Type provided? ---NO---> Auto-detect type from description
    |
    YES
    |
    v
Classify work (DOCS type)
    |
    v
Register task in WorkGraph                          [cf-knowledge-layer]
(cf-knowledge-layer: create-task)
    |
    v
Branch safety check (3.5)
    |
    v
Safe? ---NO---> Create docs/* branch                [cf-git-operations]
    |           via cf-git-operations
    YES             |
    |               |
    v               v
Register active work                                [cf-knowledge-layer]
(cf-knowledge-layer: begin-work)
    |
    v
Build context bundle
(existing docs, codebase references, templates)
    |
    v
Spawn cf-documentation teammate                     [cf-documentation]
    |
    v
Teammate researches + writes docs                   [cf-documentation]
    |
    v
Teammate validates (lint, structure, links)          [cf-documentation]
    |
    v
Teammate requests commit                            [cf-git-operations]
(cf-documentation --> cf-git-operations)
    |
    v
Complete work                                       [cf-knowledge-layer]
(cf-knowledge-layer: complete-work)
    |
    v
Present results
    |
    v
Next: /cf-review (WS-REV, DOCUMENTATION_REVIEW mode)
```

### 4.2 Execution Steps

**Step 1: Parse and Classify**

- Parse first argument: check if it matches a known document type
- If type provided: use it directly, treat second argument as description
- If no type: auto-detect from description keywords (see Section 2 table)
- Work type: `DOCS`
- Pipeline: `WS-DOCS --> WS-REV`

**Step 2: Register Task in WorkGraph**

- Send to cf-knowledge-layer:
  - `"LEAD: create-task -- type=DOCS, title=Document: {description}"`
- Receive task-id confirmation

**Step 3: Branch Safety Check (Section 3.5)**

- Send to cf-git-operations:
  - `"Verify current branch is not protected (main, master, develop, production)"`
- If on protected branch:
  - Send to cf-git-operations: `"Please create branch: docs/{doc-slug}"`
- Receive branch confirmation

**Step 4: Register Active Work**

- Send to cf-knowledge-layer:
  - `"LEAD: begin-work -- task_id={task-id}, topic={description}, branch={branch}, scope={doc-paths}, scope_policy=hard"`
- Receive active_work.id
- active_work.status set to 'in_progress'

**Step 5: Build Context Bundle**

- Determine documentation scope:
  - Document type and applicable template
  - Existing documentation in target location (if updating)
  - Relevant source code for extracting technical details
  - Related documentation for cross-referencing

**Step 6: Assign to cf-documentation Teammate**

- Ensure cf-documentation is spawned (check team roster)
- If not alive: spawn via Task tool with instruction to read `.claude/agents/cf-documentation.md`
- Assign task via SendMessage:
  - recipient: `"cf-documentation"`
  - content: Documentation assignment with full context:
    - Document type and template to use
    - Description and target scope
    - Target file path
    - Existing content reference (if updating)
    - Source material references
    - `"Apply apply-standard SOP for the {type} template. Validate with lint-file, validate-structure, and check-links before requesting commit via cf-git-operations."`
- Wait for teammate completion message

**Step 7: Complete Work**

- Send to cf-knowledge-layer:
  - `"LEAD: complete-work -- active_work_id={id}, summary={documentation summary}"`
- Updates active_work.status = 'complete'

**Step 8: Present Results**

- Show documentation summary from cf-documentation:
  - Document type and title
  - File path created or updated
  - Validation results (lint, structure, links)
  - Commit hash (from cf-git-operations)
- Show next steps: "Proceed to review with `/cf-review`"

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-knowledge-layer | create-task | Register documentation task in WorkGraph |
| cf-knowledge-layer | begin-work | Register active work for tracking |
| cf-knowledge-layer | complete-work | Finalize work tracking |
| cf-git-operations | create-branch | Create `docs/*` branch if needed |
| cf-documentation | Documentation Workflow | Full documentation creation process |
| cf-documentation | apply-standard | Apply correct document type template |
| cf-documentation | lint-file | Markdown quality validation |
| cf-documentation | validate-structure | Verify all required sections present |
| cf-documentation | check-links | Validate internal and external links |
| cf-documentation | generate-toc | Table of contents for large documents |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before Edit/Write | Verify `pathflow:pf-3` sentinel exists |
| PreToolUse:edit-write | Before Edit/Write | Validate active_work exists, check file scope |
| PreToolUse:protected-resource | Before Edit/Write on protected files | Route through staging area |
| PostToolUse:logging | After tool calls | Log documentation operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**PreToolUse Enforcement:**

```text
Edit/Write tool called by cf-documentation
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
                    +--> YES --> File in scope? (*.md in docs paths)
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

- Invoke cf-knowledge-layer:begin-work
- Creates record in active_work table:
  - task_id: from WorkGraph
  - topic: documentation description
  - branch: `docs/*` or current feature branch
  - scope: documentation file paths
  - scope_policy: 'hard'
  - status: 'in_progress'
- Logs event: type='work_started'

**On Complete (Step 7):**

- Invoke cf-knowledge-layer:complete-work
- Updates active_work:
  - status: 'complete'
  - summary: documentation summary
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
| Unknown document type | Type not in valid list | Show valid types: adr, brief, runbook, readme, api, guide |
| No description provided | Missing description argument | Prompt user for description |
| Protected branch | On main/master/develop/production | Create docs/* branch via cf-git-operations |
| Active work exists | Previous work not completed | Complete or abandon previous work |
| cf-documentation not available | Teammate spawn failure | Retry spawn with fresh context |
| Lint validation failed | Markdown quality issues | cf-documentation auto-fixes via fix-file SOP |
| Commit failed | cf-git-operations reports failure | Check branch state, retry commit |
| Scope violation | cf-documentation edits file outside doc scope | Expand scope or create separate task |

**Recovery Procedures:**

```text
ON "Unknown document type" error:
  1. Present valid document types with descriptions
  2. If description contains type hints: suggest auto-detection
  3. Retry with valid type

ON "Protected branch" error:
  1. Determine doc slug from description
  2. Send to cf-git-operations: "Please create branch: docs/{doc-slug}"
  3. Retry documentation on new branch

ON "Active work exists" error:
  1. Query active_work WHERE status='in_progress'
  2. Complete it: cf-knowledge-layer:complete-work
  3. OR abandon: cf-knowledge-layer:abandon-work
  4. Retry command

ON "Lint validation failed":
  1. cf-documentation runs fix-file SOP for auto-fixable issues
  2. For manual-fix issues: cf-documentation updates content
  3. Re-validates until clean
  4. Proceeds to commit
```

---

## 9. Examples

**Example 1: Create an ADR with Explicit Type**

```bash
/cf-document "adr" "Choose JWT over sessions for authentication"
```

Output:

```text
Documenting: ADR "Choose JWT over sessions for authentication"
Type: ADR (Architecture Decision Record)
Branch: docs/jwt-auth-decision

cf-documentation: writing...
  - Created: .codeflow/docs/decisions/auth-jwt-adr.md
  - Sections: Status, Context, Decision, Consequences (all complete)
  - Lint: passed (0 errors)
  - Links: 3 internal (all valid)
  - Committed: docs(decisions): add JWT authentication ADR

DOCS-COMPLETE: ADR written at .codeflow/docs/decisions/auth-jwt-adr.md
Next: Proceed to review with /cf-review
```

**Example 2: Update Project README**

```bash
/cf-document "readme" "Update project README with new setup instructions"
```

Output:

```text
Documenting: README update
Type: README
Branch: docs/update-readme

cf-documentation: writing...
  - Updated: project/README.md
  - Sections: Setup instructions rewritten, prerequisites updated
  - Lint: passed (0 errors)
  - Links: 5 internal (all valid), 2 external (flagged for review)
  - Committed: docs(readme): update setup instructions

DOCS-COMPLETE: README updated at project/README.md
Next: Proceed to review with /cf-review
```

**Example 3: Auto-Detected Type from Description**

```bash
/cf-document "Document the webhook retry architecture decisions"
```

Output:

```text
Documenting: "Document the webhook retry architecture decisions"
Type: ADR (auto-detected from "architecture decisions")
Branch: docs/webhook-retry-adr

cf-documentation: writing...
  - Created: .codeflow/docs/decisions/webhook-retry-adr.md
  - Sections: Status, Context, Decision, Consequences (all complete)
  - TOC: generated (5 sections)
  - Lint: passed (0 errors)
  - Committed: docs(decisions): add webhook retry architecture ADR

DOCS-COMPLETE: ADR written at .codeflow/docs/decisions/webhook-retry-adr.md
Next: Proceed to review with /cf-review
```

**Example 4: Create a Runbook**

```bash
/cf-document "runbook" "Create deployment rollback procedure"
```

Output:

```text
Documenting: Runbook "Create deployment rollback procedure"
Type: Runbook
Branch: docs/rollback-runbook

cf-documentation: writing...
  - Created: .codeflow/docs/runbooks/deployment-rollback-runbook.md
  - Sections: Prerequisites, Steps, Rollback, Verification (all complete)
  - Lint: passed (0 errors)
  - Committed: docs(runbooks): add deployment rollback procedure

DOCS-COMPLETE: Runbook at .codeflow/docs/runbooks/deployment-rollback-runbook.md
Next: Proceed to review with /cf-review
```

---

## 10. References

- [cf-documentation agent](../agents/cf-documentation.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [cf-markdown-standards skill](../skills/cf-markdown-standards/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-develop command](cf-develop.md)
- [cf-review command](cf-review.md)
- [cf-plan command](cf-plan.md)
