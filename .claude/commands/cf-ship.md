---
description: "Prepare and verify PR for merging"
argument-hint: "[pr-number]"
---

# /cf-ship Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch (PAC-5 for merge safety)
- 🔧 decide: At decision points (merge strategy selection, conflict resolution)
- 🔧 respond-organized: When presenting merge results and session completion

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Prepare and verify an approved pull request for merging. For protected branches (main, master, release/\*, production), verify CI status and notify the user to merge via GitHub UI. Does NOT execute `gh pr merge` on protected branches.

**Usage:**

```text
/cf-ship [pr-number]
```

**Use When:**

- PR has been approved by cf-review (WS-REV verdict: APPROVED)
- QA gate passed (WS-QA verdict: PASS, if pipeline includes it)
- PF5-VERIFY has confirmed acceptance criteria are met
- Ready to merge and close out the session at PF6-COMPLETE

**Do Not Use When:**

- PR has not been created yet (create PR via cf-git-operations first)
- Review is still pending or changes were requested (address review findings first)
- QA gate has not passed (use `/cf-test` to run QA)
- Deploying to an environment (use `/cf-deploy`)

### Pipeline Position

```text
Phase: PF6-COMPLETE
Pipeline: /cf-test or /cf-review --> /cf-ship --> /cf-cleanup
                                      ^ you are here
Previous: /cf-test (PASS verdict) or /cf-review (for pipelines without WS-QA)
Next: /cf-cleanup (session end)
```

---

## 2. Arguments & Flags

**Arguments:**

| Argument | Required | Description |
|----------|----------|-------------|
| `pr-number` | No | GitHub PR number to merge (e.g., `42`) |

**Behavior When No Argument:**

- Auto-detect PR from current branch via `gh pr view --json number`
- If no PR exists for the current branch: error with guidance to create PR first
- If multiple PRs exist: present list and ask user to specify

**Protected Branch Handling:**

Protected branches (from `enforcement-policy.json` `merge_protection`):

- `main`, `master`, `release/*`, `production`

| Target Branch | Behavior |
|---------------|----------|
| Protected (main, master, release/\*, production) | Verify CI, notify user to merge via GitHub UI. Does NOT call `gh pr merge`. |
| Non-protected | Future consideration: automated merge may be supported in a later version. Currently treated the same as protected (verify + notify). |

**Merge Strategy (when merge is automated for non-protected branches):**

| Strategy | When Used | Command |
|----------|-----------|---------|
| Squash merge | Default for feature/fix branches | `gh pr merge --squash` |
| Merge commit | Explicitly requested by user | `gh pr merge --merge` |
| Rebase | Explicitly requested by user | `gh pr merge --rebase` |

The default strategy is squash merge. Automated merge is reserved for non-protected target branches only. Protected branch merges are always performed by the user via GitHub UI.

**Examples:**

```bash
# Merge PR for current branch (auto-detect)
/cf-ship

# Merge a specific PR
/cf-ship 42
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session at PF5-VERIFY or PF6-COMPLETE
- [ ] PR exists and is approved (no outstanding change requests)
- [ ] CI checks passed (if configured)
- [ ] No merge conflicts with base branch
- [ ] cf-git-operations teammate available
- [ ] cf-knowledge-layer teammate available

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-git-operations | PR validation, merge execution, branch cleanup |
| cf-knowledge-layer | Task completion, work status update, event logging |

### 3.5 Pre-Merge Validation

**Before dispatching merge to cf-git-operations:**

The team lead validates merge readiness through cf-git-operations:

1. Query PR status: `"Check PR #{number} merge readiness: approval, CI, conflicts"`
2. Verify PR has at least one approval with no outstanding change requests
3. Verify CI checks have passed (if configured)
4. Verify no merge conflicts exist with the base branch

```text
Pre-Merge Validation:
    |
    v
Get PR status (approved, CI, conflicts)
    |
    v
PR approved? ---------> NO ---------> ERROR: "PR not approved"
    |                                   "Complete review first"
    YES
    |
    v
CI passed? ------------> NO ---------> ERROR: "CI checks failing"
    |                                   "Fix CI failures first"
    YES
    |
    v
Merge conflicts? ------> YES --------> ERROR: "Merge conflicts detected"
    |                                   "Rebase or resolve conflicts"
    NO
    |
    v
PROCEED to merge
```

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF6-COMPLETE | Teammate: cf-git-operations

/cf-ship invoked
    |
    v
Parse arguments (pr-number or auto-detect)
    |
    v
Identify PR -------+-------------------+
    |               |                   |
    v               v                   v
  PR NUMBER      AUTO-DETECT          NO PR FOUND
    |               |                   |
    v               v                   v
Fetch PR info   Detect PR from      ERROR:
via cf-git-ops  current branch      "No PR for branch"   [cf-git-operations]
    |               |
    +-------+-------+
            |
            v
Pre-merge validation (3.5)                               [cf-git-operations]
    |
    v
All checks pass? ---NO---> ERROR with specific failure reason
    |
    YES
    |
    v
Advance to PF6-COMPLETE                                 [cf-knowledge-layer]
(cf-knowledge-layer: log phase transition)
    |
    v
Verify CI status via `gh pr checks`                     [cf-git-operations]
    |
    v
CI passed? ---NO---> Report failures, suggest fixes
    |
    YES
    |
    v
Target is protected branch?                              [cf-git-operations]
    |
    +---YES---> Notify user: "PR #{number} ready to merge via GitHub UI"
    |           Update WorkGraph (status: awaiting_review)
    |           |
    +---NO----> (Future: may auto-merge non-protected targets)
    |           Currently: same as protected (notify user)
    |
    v
Update WorkGraph                                         [cf-knowledge-layer]
(cf-knowledge-layer: update task status, log event)
    |
    v
Present results
(CI status, merge readiness, next steps)
    |
    v
Next: /cf-cleanup (PF7-END)
```

### 4.2 Execution Steps

**Step 1: Parse and Identify PR**

- If pr-number provided: use directly
- If no argument: auto-detect from current branch
  - Send to cf-git-operations: `"Get PR number for current branch"`
  - If no PR found: error with `"No PR found for branch {branch}. Create a PR first."`

**Step 2: Pre-Merge Validation (Section 3.5)**

- Send to cf-git-operations:
  - `"Check merge readiness for PR #{number}: approval status, CI checks, merge conflicts"`
- Evaluate response:
  - Not approved: block with guidance to complete review
  - CI failing: block with failure details
  - Conflicts: block with rebase guidance

**Step 3: Advance to PF6-COMPLETE**

- Send to cf-knowledge-layer:
  - `"LEAD: phase-transition -- phase=PF6-COMPLETE, session={session-id}"`
- Log phase transition event to JSONL
- Create PF6-COMPLETE sentinel

**Step 4: Verify CI Status**

- Send to cf-git-operations:
  - `"Check CI status for PR #{number} via gh pr checks"`
- Evaluate CI results:
  - All checks passed: proceed to Step 5
  - Any checks failing: report failures with details, suggest fixes, STOP

**Step 5: Protected Branch Check**

- Determine if target branch is protected (main, master, release/\*, production)
- **Protected target branch:**
  - Do NOT call `gh pr merge`
  - Notify user: `"PR #{number} is ready to merge via GitHub UI"`
  - Update WorkGraph status to `awaiting_review`
- **Non-protected target branch:**
  - Future consideration: automated merge may be supported
  - Currently: same as protected (verify + notify)

**Step 6: Update WorkGraph**

- Send to cf-knowledge-layer:
  - `"LEAD: update-task -- task_id={task-id}, pr_number={number}, status=awaiting_review"`
- Task status updated to `awaiting_review`
- Logs events: type='pr_verified', type='phase_transition'

**Step 7: Present Results**

- Show verification summary:
  - PR number and title
  - CI status: all checks passed
  - Target branch and protection status
  - Action required: "Merge PR #{number} via GitHub UI"
- Show next steps: "After merging, proceed to `/cf-cleanup` to end session."

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-git-operations | check-branch-status | Verify branch and PR state |
| cf-git-operations | review-changes | Inspect pending changes before merge |
| cf-git-operations | sync-remote | Ensure remote is up to date |
| cf-git-operations | verify-pr-and-sync | Verify CI status, check protected branch, notify user |
| cf-knowledge-layer | update-task | Update task status to `awaiting_review` |
| cf-knowledge-layer | complete-work | Finalize active work tracking (after user merges) |
| cf-knowledge-layer | phase-transition | Log PF6-COMPLETE transition |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before tool calls | Verify PathFlow phase consistency |
| PreToolUse:gh-pr | Before `gh pr merge` (Bash) | Validate PR operation format, block AI attribution |
| PostToolUse:logging | After tool calls | Log merge operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**Note:** `/cf-ship` operates at PF6-COMPLETE. Edit/Write hooks are not triggered because this command does not modify source files -- it only interacts with GitHub's PR API via `gh` CLI commands through cf-git-operations.

---

## 7. Memory Integration

### 7.1 PR Verification

**On Verification (Step 6):**

- Invoke cf-knowledge-layer:update-task:
  - task_id: from WorkGraph
  - pr_number: verified PR number
  - status: 'awaiting_review'
- Events logged: type='pr_verified', type='phase_transition'
- Phase transition logged: PF6-COMPLETE
- Note: Task status moves to `complete` only after user merges the PR via GitHub UI

### 7.2 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/logs/pathflow-events.jsonl` | Append PF6-COMPLETE transition, merge event |
| 1 | `.state/db/codeflow.db` | Update task status, PR tracking |
| 2 | `project-management/` | Derived work tracking views (updated if applicable) |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| No PR found | No PR for current branch | Create PR via cf-git-operations first |
| PR not approved | Missing review approval | Complete review via `/cf-review` |
| CI checks failing | Pipeline failures | Fix CI issues, push fixes, retry |
| Merge conflicts | Base branch diverged since PR creation | Rebase feature branch via cf-git-operations |
| Protected branch merge attempted | Target branch is protected | Notify user to merge via GitHub UI |
| Network error | Sandbox or connectivity issue | Consult cf-security for sandbox configuration |
| Session not at PF5+ | Shipping before verification | Complete all pipeline stages first |

**Recovery Procedures:**

```text
ON "No PR found" error:
  1. Verify current branch is a feature branch
  2. Check if commits exist via cf-git-operations: review-changes
  3. If commits exist: create PR via cf-git-operations
  4. Retry /cf-ship with new PR number

ON "Merge conflicts" error:
  1. Send to cf-git-operations: "Rebase current branch onto main"
  2. If conflicts during rebase: escalate to user for manual resolution
  3. After clean rebase: push and retry /cf-ship

ON "CI checks failing" error:
  1. Review CI failure details from PR status
  2. Route fix back to cf-development (rework loop)
  3. After fix: push, wait for CI to pass, retry /cf-ship

ON "Session not at PF5+" error:
  1. Check session state with /cf-stack
  2. Complete remaining pipeline stages (WS-DEV, WS-REV, WS-QA)
  3. Complete PF5-VERIFY
  4. Retry /cf-ship
```

---

## 9. Examples

**Example 1: Ship Current Branch PR (Auto-Detect, Protected Target)**

```bash
/cf-ship
```

Output:

```text
Verifying: PR #42 "feat(auth): configure OAuth2 provider integration"
Branch: feat/oauth2-providers → main (protected)

Pre-merge checks:
  [OK] PR approved (1 approval)
  [OK] CI checks passed (3/3)
  [OK] No merge conflicts

Target branch 'main' is protected.
PR #42 is ready to merge via GitHub UI.

WorkGraph updated:
  Task INF-TSK-AUTH-001: awaiting_review

Next: Merge PR #42 via GitHub UI, then proceed to /cf-cleanup.
```

**Example 2: Ship a Specific PR (Protected Target)**

```bash
/cf-ship 42
```

Output:

```text
Verifying: PR #42 "fix(webhooks): add retry logic to webhook handler"

Pre-merge checks:
  [OK] PR approved (1 approval)
  [OK] CI checks passed
  [OK] No merge conflicts

Target branch 'main' is protected.
PR #42 is ready to merge via GitHub UI.
Task BKD-TSK-FIX-WEBHOOK-001: awaiting_review

Next: Merge via GitHub UI, then /cf-cleanup.
```

**Example 3: Ship Blocked by Failing CI**

```bash
/cf-ship
```

Output:

```text
Shipping: PR #43 "feat(search): add full-text search"

Pre-merge checks:
  [OK] PR approved (1 approval)
  [FAIL] CI checks failing: test-suite (2 test failures)
  [OK] No merge conflicts

Cannot merge: CI checks must pass before merging.
Fix CI failures and push, then retry /cf-ship.
```

**Example 4: Ship Blocked by Merge Conflicts**

```bash
/cf-ship
```

Output:

```text
Shipping: PR #44 "refactor(hooks): simplify enforcement pipeline"

Pre-merge checks:
  [OK] PR approved (2 approvals)
  [OK] CI checks passed
  [FAIL] Merge conflicts detected in 2 files:
    - .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh
    - .codeflow/config/enforcement/enforcement-policy.json

Cannot merge: resolve conflicts first.
Rebase onto main via cf-git-operations, then retry /cf-ship.
```

---

## 10. References

- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-review command](cf-review.md)
- [cf-test command](cf-test.md)
- [cf-cleanup command](cf-cleanup.md)
