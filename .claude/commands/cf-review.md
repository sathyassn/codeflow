---
description: "Review code changes and provide feedback"
argument-hint: "[pr-number|branch]"
---

# /cf-review Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch
- 🔧 decide: At decision points (review mode selection, verdict assessment)
- 🔧 respond-organized: When presenting review results

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Review code changes by dispatching an independent assessment to the cf-review teammate, producing a structured verdict with actionable findings.

**Usage:**

```text
/cf-review [pr-number|branch]
```

**Use When:**

- Implementation stage (WS-DEV) is complete and code needs peer review
- A PR needs independent assessment before merge
- Branch changes need review against the base branch
- Any work type has completed its primary stage and needs WS-REV

**Do Not Use When:**

- Code is still being implemented (wait for cf-development to complete)
- Running tests or QA verification (use `/cf-test`)
- Reviewing your own plan or design in progress (cf-review requires completed work)

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `pr-number` | No | GitHub PR number to review (e.g., `42`) |
| `branch` | No | Branch name to compare against base (e.g., `feat/auth`) |

**Behavior When No Argument:**

- If on a feature branch: compare current branch against `main`
- If a PR exists for the current branch: review that PR
- If neither: error with guidance

**Review Mode Auto-Detection:**

| Work Type | Review Mode | Criteria Focus |
|-----------|-------------|----------------|
| FEAT, FIX, RFCT, CICD, HTFX, CHOR | CODE_REVIEW | Correctness, security, tests, style |
| PLAN, SPKE | DESIGN_REVIEW | Completeness, feasibility, trade-offs |
| DOCS | DOCUMENTATION_REVIEW | Accuracy, clarity, examples |
| TEST | TEST_REVIEW | Coverage, independence, edge cases |

The review mode is auto-detected from the work type registered in the current session's WorkGraph entry. If no session context exists (ad-hoc review), defaults to CODE_REVIEW.

**Examples:**

```bash
# Review current branch against main
/cf-review

# Review a specific PR
/cf-review 42

# Review a specific branch
/cf-review feat/oauth2-providers
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session initialized (PF1-INIT through PF3-CLASSIFY complete for tracked sessions)
- [ ] Primary work stage complete (WS-DEV, WS-PLAN, WS-DOCS, or WS-TEST)
- [ ] Changes committed (review operates on committed code)
- [ ] cf-git-operations teammate available (for PR and diff operations)

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-review | Independent review execution |
| cf-git-operations | Diff generation, PR comment posting |
| cf-knowledge-layer | Work type lookup, review verdict recording |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
/cf-review invoked
    |
    v
Parse arguments (pr-number, branch, or auto-detect)
    |
    v
Identify changes ------+-------------------+
    |                   |                   |
    v                   v                   v
  PR NUMBER          BRANCH NAME        AUTO-DETECT
    |                   |                   |
    v                   v                   v
Fetch PR diff       Compare branch      Detect current
via cf-git-ops      against main        branch + PR
    |                   |                   |
    +-------------------+-------------------+
    |
    v
Determine review mode (from work type)
    |
    v
Assign to cf-review teammate
    |
    v
Teammate reads all changed files
    |
    v
Teammate applies review checklist
    |
    v
Teammate runs tests (CODE_REVIEW, TEST_REVIEW)
    |
    v
Teammate delivers verdict
    |
    v
Process verdict --------+-------------------+
    |                    |
    v                    v
  APPROVED           CHANGES_REQUESTED
    |                    |
    v                    v
Record verdict       Send findings to
in WorkGraph         originating teammate
    |                    |
    v                    v
Present results      Route back to primary
to user              stage for rework
```

### 4.2 Execution Steps

**Step 1: Parse Arguments**

- If PR number provided: use PR diff as review scope
- If branch name provided: compare branch against `main` (or detected base)
- If no argument: detect current branch, check for existing PR
- Determine the set of changed files

**Step 2: Identify Changes**

- Send to cf-git-operations:
  - PR mode: `"Get diff for PR #{pr-number}"`
  - Branch mode: `"Get diff between main and {branch}"`
  - Auto mode: `"Get diff for current branch against main"`
- Receive: list of changed files, diff summary

**Step 3: Determine Review Mode**

- Query cf-knowledge-layer for current session work type:
  - `"LEAD: query-work-type -- session=current"`
- Map work type to review mode (see Section 2 table)
- If no session context: default to CODE_REVIEW

**Step 4: Assign to cf-review Teammate**

- Ensure cf-review is spawned (check team roster)
- If not alive: spawn via Task tool with instruction to read `.claude/agents/cf-review.md`
- Assign task via SendMessage:
  - recipient: `"cf-review"`
  - content: Review assignment with full context:
    - Review mode (CODE_REVIEW, DESIGN_REVIEW, etc.)
    - Scope: list of changed files
    - Branch or PR reference
    - Acceptance criteria from task (if available)
    - `"Deliver verdict using standard format. If CHANGES_REQUESTED, send findings to the originating teammate."`
- Wait for teammate completion message

**Step 5: Process Verdict**

- Receive verdict from cf-review:
  - `"REVIEWER: {APPROVED|CHANGES_REQUESTED} - {summary}"`

- **If APPROVED:**
  - Record verdict in WorkGraph via cf-knowledge-layer
  - Present approval to user
  - Indicate next step: proceed to WS-QA (if pipeline includes it) or PF5-VERIFY

- **If CHANGES_REQUESTED:**
  - cf-review sends detailed findings to originating teammate (cf-development, cf-planning, etc.)
  - Record verdict and findings in WorkGraph
  - Route work back to primary stage for rework
  - Track rework iteration count (max 3)

**Step 6: Present Results**

- Show review verdict (APPROVED or CHANGES_REQUESTED)
- Show findings summary (CRITICAL/MAJOR/MINOR/NOTE counts)
- Show next steps:
  - Approved: "Proceed to testing with `/cf-test`" or "Verification at PF5"
  - Changes requested: "Rework items routed to {teammate}. Re-run `/cf-review` after fixes."

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-review | CODE_REVIEW | Code correctness, security, testing, style |
| cf-review | DESIGN_REVIEW | Plan completeness, feasibility, trade-offs |
| cf-review | DOCUMENTATION_REVIEW | Doc accuracy, clarity, examples |
| cf-review | TEST_REVIEW | Test coverage, independence, edge cases |
| cf-review | Rework Triggering | Route findings back for correction |
| cf-git-operations | Diff retrieval | Get changed files for review scope |
| cf-git-operations | PR comment posting | Post review comments on GitHub PRs |
| cf-knowledge-layer | query-work-type | Determine review mode from session |
| cf-knowledge-layer | record-verdict | Store review verdict in WorkGraph |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before tool calls | Verify PathFlow phase consistency |
| PostToolUse:logging | After tool calls | Log review operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**Note:** cf-review operates in read-only mode. No Edit/Write hooks fire during review because the reviewer never modifies files. The edit-write and protected-resource hooks are not triggered.

---

## 7. Memory Integration

### 7.1 Review Verdict Recording

**On Verdict Delivery:**

- Send to cf-knowledge-layer:
  - `"REV-COMPLETE: task={id}, verdict={APPROVED|CHANGES_REQUESTED}, findings={count}"`
- Records review event in WorkGraph:
  - stage: WS-REV
  - verdict: APPROVED or CHANGES_REQUESTED
  - findings_count: total findings
  - critical_count: blocking findings
  - iteration: rework iteration number

### 7.2 Rework Tracking

- Each CHANGES_REQUESTED verdict increments the rework iteration counter
- Maximum rework iterations: 3 (from `pathflow-config.json`)
- If limit reached: cf-review escalates to team lead
- Team lead decides: override approval, redesign, or end session

### 7.3 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Review events log |
| 1 | `.state/db/codeflow.db` | Verdict state, iteration tracking |
| 2 | `.claude/memory/` | Derived review summaries |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| No changes detected | Branch matches base, nothing to review | Verify correct branch; check for uncommitted work |
| PR not found | Invalid PR number | List open PRs via cf-git-operations |
| Primary stage incomplete | WS-DEV/WS-PLAN not finished | Wait for primary stage to complete |
| cf-review not available | Teammate spawn failure | Retry spawn with fresh context |
| Rework limit exceeded | 3 iterations without approval | Escalate to team lead for decision |
| Review mode ambiguous | No session work type detected | Default to CODE_REVIEW, confirm with user |

**Recovery Procedures:**

```text
ON "No changes detected" error:
  1. Verify current branch: cf-git-operations status
  2. Check for uncommitted changes
  3. If changes exist: request commit via cf-git-operations first
  4. Retry review

ON "Rework limit exceeded":
  1. Present 3-iteration history to user
  2. Options:
     a. Override: approve with noted exceptions
     b. Redesign: route back to /cf-plan
     c. End session: proceed to PF7-END
  3. Record decision in WorkGraph
```

---

## 9. Examples

**Example 1: Review Current Branch**

```bash
/cf-review
```

Output:

```text
Reviewing: feat/oauth2-providers vs main
Mode: CODE_REVIEW (from work type FEAT)
Files: 4 changed (2 added, 2 modified)

## Review Verdict
Verdict: APPROVED
Findings: 0 CRITICAL, 0 MAJOR, 2 MINOR, 1 NOTE

- [MINOR] Consider extracting token validation to a shared utility (src/auth/oauth2-provider.py:45)
- [MINOR] Missing docstring on public function (src/auth/token-exchange.py:12)
- [NOTE] Good error handling pattern in retry logic

Next: Proceed to testing with /cf-test
```

**Example 2: Review a Specific PR**

```bash
/cf-review 42
```

Output:

```text
Reviewing: PR #42 "feat(auth): configure OAuth2 provider integration"
Mode: CODE_REVIEW
Files: 4 changed

## Review Verdict
Verdict: CHANGES_REQUESTED
Findings: 1 CRITICAL, 1 MAJOR, 0 MINOR

- [CRITICAL] API key hardcoded in configuration (src/config/settings.py:28)
- [MAJOR] Missing test for error path in token exchange (tests/test_oauth2.py)

Required changes sent to cf-development.
Next: Fix issues, then re-run /cf-review (iteration 2/3)
```

**Example 3: Review a Design Plan**

```bash
/cf-review plan/oauth2-authentication
```

Output:

```text
Reviewing: plan/oauth2-authentication vs main
Mode: DESIGN_REVIEW (from work type PLAN)
Files: 3 changed (3 added)

## Review Verdict
Verdict: APPROVED
Findings: 0 CRITICAL, 0 MAJOR, 1 MINOR, 2 NOTE

- [MINOR] Consider adding rollback strategy to ADR consequences section
- [NOTE] Thorough trade-off analysis between OAuth providers
- [NOTE] Task decomposition is well-scoped with clear acceptance criteria

Next: Plan approved. Proceed to implementation with /cf-develop
```

---

## 10. References

- [cf-review agent](../agents/cf-review.md)
- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-develop command](cf-develop.md)
- [cf-test command](cf-test.md)
- [cf-plan command](cf-plan.md)
