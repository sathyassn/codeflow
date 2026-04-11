---
title: "Design Analysis: PathFlow PR Verification, Merge Protection & Validation Hardening"
epic: "INF-EPC-008"
status: accepted
created_at: "2026-02-21"
updated_at: "2026-02-21"
---

# Design Analysis: PathFlow PR Verification, Merge Protection & Validation Hardening

## 1. Problem Statement

Three gaps exist in the current PathFlow pipeline, plus two cleanup items:

### Gap 1: PF6-to-PF7 Transition Gap

After PR creation (PF6-TSK-05), PF7 immediately shuts down the team. If CI fails or the merge has issues, no team exists to address them. By the time the user reviews and merges via the GitHub UI, the session is already closed.

**Current flow:**

```text
PF6-TSK-05: create-pr (cf-git-operations)
    |
    v
PF7-TSK-01: shutdown on-demand teammates  <-- immediate, no verification
PF7-TSK-02: shutdown persistent teammates
PF7-TSK-03: TeamDelete
```

**Impact:** The pipeline has no feedback loop between PR creation and team shutdown. CI failures are discovered post-session with no automated response path.

### Gap 2: No Merge Protection

Agents can execute `gh pr merge` targeting any branch, including `main`. No configurable enforcement prevents auto-merge to protected branches. The existing `protected_branches` array in `enforcement-policy.json` (line 411-416) is only used conceptually -- no hook reads it for merge blocking.

**Risk:** An autorun session or misconfigured agent could merge directly to main without human review.

### Gap 3: No Field Validation

Task and epic fields are set by LLMs with no deterministic validation. Invalid format IDs, missing required fields, illegal status transitions, and constraint violations enter the system unchecked. The Go CLI (Phase 7) will add schema validation, but until then the LLM data layer is unguarded.

### Cleanup 1: Vestigial `auto_commit` Field

The `auto_commit` field exists in the tasks table (schema.sql line 168), task template (line 20), and agent definitions, but is never read by any PathFlow logic. PathFlow commits unconditionally at PF6-TSK-03. The field is dead weight that confuses agents.

### Cleanup 2: PF3 Task Ordering Fragility

The current PF3 task order is:

1. PF3-TSK-01: Classify work type
2. PF3-TSK-02: Register task in WorkGraph (requires `ensure-work-registered`)
3. PF3-TSK-03: Spawn cf-git-operations
4. PF3-TSK-04: Create feature branch (triggers pf-3 sentinel)
5. PF3-TSK-05: Begin work (`begin-work`)

For adhoc tasks (not pre-planned), task registration (TSK-02) must happen before branch creation (TSK-04). But for planned tasks, the record already exists from WS-PLAN. The current ordering is rigid and doesn't distinguish between these cases. Additionally, the pf-3 sentinel (created by branch creation) gates Edit/Write operations, but adhoc task registration via cf-knowledge-layer may need Bash operations that interact with protected paths.

## 2. Design Decisions

### D1: PF6-TSK-06 — verify-pr-and-sync (3-mode operation)

A new final step in PF6-COMPLETE that bridges the gap between PR creation and team shutdown.

**Three operational modes:**

| Mode | Trigger | Behavior |
|------|---------|----------|
| Interactive | `interaction=interactive` | Poll CI status -> notify user "PR ready for review" -> wait for user to merge via GitHub UI -> `git pull` to sync main |
| Autorun + auto_merge=true | `interaction=autorun`, `auto_merge=true` | Poll CI status -> `gh pr merge --delete-branch` to integration branch (NOT squash) -> `git pull` target branch |
| Autorun + auto_merge=false | `interaction=autorun`, `auto_merge=false` | Poll CI status -> task already complete from PF6-TSK-01 -> proceed to PF7 |

**CI polling design:**

- Poll interval: 15 seconds
- Max wait: 10 minutes (configurable in pathflow-config.json)
- On timeout: warn user (interactive) or proceed to PF7 (autorun)
- Uses: `gh pr checks {number} --watch --fail-fast` (built-in polling)

**Integration branch merge:**

- Uses `gh pr merge --delete-branch` (regular merge, NOT `--squash`)
- Preserves conventional commit format from individual commits
- Target MUST be a non-main branch (enforced by D2 and D4)

**Rationale:** This design covers all three usage scenarios without adding complexity. Interactive mode is the default path for most users. Autorun modes enable batch processing with appropriate guardrails.

### D2: Protected Branch Merge — Hard Block

A PreToolUse hook hard-blocks `gh pr merge` targeting protected branches. No bypass mechanism, no authorization files, no exceptions.

**Hook flow:**

```text
Agent calls: Bash(gh pr merge {number})
    |
    v
Hook reads enforcement-policy.json -> merge_protection.protected_branches[]
    |
    v
Resolve PR target branch: gh pr view {number} --json baseRefName
    |
    v
Target in protected_branches[] ?
    |
    +-- YES --> BLOCK (exit 2): "BLOCKED: Cannot auto-merge to protected branch '{branch}'"
    |
    +-- NO  --> ALLOW (exit 0)
```

**Configuration location:** `enforcement-policy.json` -> `merge_protection` section (new).

**Why hard block with no bypass:** Protected branch merges are the highest-risk git operation. A human MUST initiate the merge via GitHub UI. This aligns with GitHub's own branch protection model.

### D3: Autorun Integration Branch Convention

When `auto_merge=true`, the target MUST be a non-main branch. Convention: `autorun/{batch-name}`.

**Lifecycle:**

1. Autorun session start: create `autorun/{batch-name}` off `main`
2. Workers create feature branches off `autorun/{batch-name}`
3. Worker PRs merge to `autorun/{batch-name}` via `gh pr merge --delete-branch`
4. After all workers complete: human reviews summary PR `autorun/{batch-name}` -> `main`
5. cf-autorun cleanup removes worktrees + integration branch after merge

**Rationale:** This prevents any automated path from merging directly to main while still enabling efficient batch processing. The integration branch serves as a staging area for human review.

### D4: auto_merge + protected branch = FORBIDDEN

`auto_merge=true` combined with `target_branch` in `protected_branches` is a validation error. Caught at two points:

1. Batch file parsing (cf-autorun validates before starting workers)
2. Hook enforcement (D2 blocks the merge command itself)

This is a defense-in-depth constraint -- both validation and enforcement prevent the combination.

### D5: Deprecate `auto_commit`

The `auto_commit` field is vestigial. PathFlow commits unconditionally at PF6-TSK-03.

**Deprecation plan:**

- Schema: leave column, add `-- DEPRECATED` comment. Do NOT drop (backward compat)
- Task template: remove from YAML frontmatter
- Agent definitions: remove references to auto_commit behavior
- Batch file format: remove from documentation, ignore if present

**Rationale:** Dropping the column would break existing DB records and require data migration. Deprecation-in-place is safer and communicates intent.

### D6: Task Status — `awaiting_review` (REMOVED)

**Decision reversed.** The `awaiting_review` status has been removed. Task completion = work done (code written, reviewed, tested). PR review state is self-evident from GitHub and does not need to be duplicated in the task status machine.

**Status lifecycle (simplified):**

```text
todo --> in_progress --> complete
  |         |
  v         v
blocked --> in_progress
```

**Valid transitions:**

| From | To | Trigger |
|------|-----|---------|
| todo | in_progress | Work started |
| todo | blocked | Dependency not met |
| blocked | in_progress | Blocker resolved |
| in_progress | blocked | New blocker discovered |
| in_progress | complete | Work done |

**Schema change:** Migration `004_remove_awaiting_review_status.sql` removes `awaiting_review` from CHECK constraint. Existing rows migrated to `complete`.

### D7: `/cf-ship` as Notification for Protected Branches

Currently `/cf-ship` is conceptual. With merge protection, it becomes a preparation + notification command for protected branches.

**Behavior:**

- Verify CI status on PR
- Notify user: "PR #{number} is ready to merge. Please merge via GitHub UI."
- Does NOT execute `gh pr merge` on protected branches
- For non-protected branches: may still execute merge (future consideration)

### D8: PF3 Reorder for Adhoc Tasks

Branch creation BEFORE task registration, conditional on task origin.

**Revised PF3 order:**

```text
PF3-TSK-01: Classify work type (team-lead, always)
PF3-TSK-02: Spawn cf-git-operations (team-lead, always)
PF3-TSK-03: Create feature branch (cf-git-operations, always) --> triggers pf-3 sentinel
PF3-TSK-04: Register task in WorkGraph (cf-knowledge-layer, CONDITIONAL: adhoc only)
PF3-TSK-05: Begin work (cf-knowledge-layer, always)
```

**Key change:** Branch creation (TSK-03) now happens BEFORE task registration (TSK-04). The pf-3 sentinel unlocks Edit/Write, which cf-knowledge-layer may need for adhoc task registration. For planned tasks (origin=planned), TSK-04 is skipped because the record already exists from WS-PLAN.

### D9: Deterministic Task/Epic Validation

Shell scripts that validate task and epic fields deterministically (no LLM involvement).

**Execution points:**

| Point | What's Validated | Why |
|-------|-----------------|-----|
| PF4-TSK-02 | Task fields before work starts | Catch bad data early |
| WS-PLAN (before commit) | All planning output (epics + tasks) | Validate planner output |
| PF6-TSK-01 | Status transition + final state | Ensure completeness |

**Validation script interface:**

```bash
# validate-task.sh
# Input: path to task markdown file (reads YAML frontmatter)
# Output: exit 0 (valid) or exit 1 (invalid with error messages to stderr)
# Checks: required fields, format_id regex, status values, field constraints

# validate-epic.sh
# Input: path to epic markdown file (reads YAML frontmatter)
# Output: exit 0 (valid) or exit 1 (invalid with error messages to stderr)
# Checks: required fields, format_id regex, status values, completeness
```

### D10: Configurable Merge Protection in enforcement-policy.json

New section in enforcement-policy.json:

```json
{
  "merge_protection": {
    "description": "Configurable merge protection for automated PR merging",
    "protected_branches": ["main", "master", "release/*", "production"],
    "policy": "hard_block",
    "message": "BLOCKED: Cannot auto-merge to protected branch '{branch}'. Human must merge via GitHub UI.",
    "applies_to": "gh pr merge"
  }
}
```

**Note:** The existing `protected_branches` array at the root level (line 411-416) covers git branch operations (delete, force-push). The new `merge_protection.protected_branches` specifically covers `gh pr merge`. These may have the same values but serve different enforcement purposes.

### D11: Autorun Merge Strategy

Autorun merge to integration branch uses regular merge (`gh pr merge --delete-branch`), NOT `--squash`. This preserves the conventional commit format from individual worker commits.

**Rationale:** Each worker's commits follow conventional commit format (e.g., `feat: add login form`). Squash-merging would collapse these into a single commit, losing the granular commit history. Regular merge preserves each commit's type, scope, and description.

## 3. Files to Change (21 total)

### A. PathFlow Pipeline (7 files)

| # | File | Changes |
|---|------|---------|
| 1 | `.codeflow/config/pathflow/pathflow-config.json` | Reorder PF3 tasks (conditional for adhoc); add PF4-TSK-02 validation gate; add PF6-TSK-06 verify-pr-and-sync |
| 2 | `.claude/agents/cf-git-operations.md` | New verify-pr-and-sync SOP; FORBIDDEN rules for protected branch merge; integration branch handling |
| 3 | `.claude/agents/cf-knowledge-layer.md` | Update ensure-work-registered (runs after pf-3 for adhoc); add validation to begin-work and complete-work; remove auto_commit references |
| 4 | `.claude/CLAUDE.md` | Sections 4.2 (Execution Steps), 4.3 (Phase Reference), 6 (Work Pipelines) updates for PF3 reorder, PF4 validation, PF6 verify-pr-and-sync |
| 5 | `.claude/commands/cf-ship.md` | Redefine as notification for protected branches |
| 6 | `.claude/commands/cf-autorun.md` | Integration branch convention; validation rules; worker lifecycle updates |
| 7 | `.claude/agents/cf-planning.md` | Add validation step before commit in WS-PLAN workflow |

### B. Schema & Templates (4 files)

| # | File | Changes |
|---|------|---------|
| 8 | `.codeflow/scripts/db/schema.sql` | Remove `awaiting_review` from tasks status CHECK constraint; add deprecation comment to auto_commit |
| 9 | `.codeflow/scripts/db/migrations/004_remove_awaiting_review_status.sql` | Migration: remove `awaiting_review` from CHECK constraint, migrate existing rows to `complete` |
| 10 | `project-management/templates/task-template.md` | Update status values in YAML comment; remove auto_commit field |
| 11 | `project-management/templates/epic-template.md` | Verify status values are correct (no change expected) |

### C. Enforcement (3 files)

| # | File | Changes |
|---|------|---------|
| 12 | `.codeflow/config/enforcement/enforcement-policy.json` | Add `merge_protection` section with protected_branches and policy |
| 13 | `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh` | Extend to block `gh pr merge` targeting protected branches |
| 14 | `.claude/agents/cf-security.md` | Document merge protection policy in security posture |

### D. Validation Scripts (2 new files)

| # | File | Purpose |
|---|------|---------|
| 15 | `.codeflow/scripts/validation/validate-task.sh` | Deterministic task field validation (required fields, format_id regex, status values, constraint checks) |
| 16 | `.codeflow/scripts/validation/validate-epic.sh` | Deterministic epic field validation (required fields, format_id regex, status values, completeness) |

### E. V4 Spec Alignment (5 files)

> **External repository:** These files are in a separate repository at `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/`, NOT inside the codeflow project. Use full absolute paths when referencing or editing them.

| # | File | Changes |
|---|------|---------|
| 17 | `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/06-flows/session-lifecycle.md` | PF3 reorder, PF4 validation gate, PF6 verify-pr-and-sync |
| 18 | `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/09-autorun/architecture.md` | Integration branch model, merge protection |
| 19 | `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/09-autorun/worker-lifecycle.md` | Worker lifecycle, merge flow |
| 20 | `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/09-autorun/batch-files.md` | Validation rules, deprecate auto_commit |
| 21 | `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/10-implementation/phase-5-commands.md` | cf-ship as notification for protected branches |

## 4. Task Validation Rules

### Task Validation (`validate-task.sh`)

| Rule | Check | Error |
|------|-------|-------|
| Required fields | id, format_id, epic_id, title, status, area_type, work_type present and non-empty | "Missing required field: {field}" |
| Format ID | Matches `^[A-Z]{2,4}-TSK-[0-9]{3}-[0-9]{3}$` | "Invalid format_id: {value}" |
| Status value | In: todo, blocked, in_progress, complete, cancelled | "Invalid status: {value}" |
| auto_merge + target | If auto_merge=true: target_branch must be non-null and not in protected_branches | "auto_merge requires non-protected target_branch" |
| raise_pr + auto_merge | If raise_pr=false: auto_merge must be false | "auto_merge requires raise_pr=true" |
| Acceptance criteria | Non-empty for tracked tasks (autorun_eligible=true) | "Autorun tasks require acceptance criteria" |
| Status transition | Validates from->to is in allowed transitions set | "Invalid transition: {from} -> {to}" |

### Epic Validation (`validate-epic.sh`)

| Rule | Check | Error |
|------|-------|-------|
| Required fields | id, format_id, title, status, area_type present and non-empty | "Missing required field: {field}" |
| Format ID | Matches `^[A-Z]{2,4}-EPC-[0-9]{3}$` | "Invalid format_id: {value}" |
| Status value | In: draft, planning, in_progress, blocked, complete, archived | "Invalid status: {value}" |
| Completeness | If status=complete: all tasks must be complete or cancelled | "Epic marked complete but task {id} is {status}" |

## 5. Field Matrix (Final)

| Field | Action | Default | Notes |
|-------|--------|---------|-------|
| autorun_eligible | Keep | FALSE | Controls batch processing eligibility |
| auto_commit | DEPRECATE | TRUE (legacy) | Leave column, add deprecation comment, remove from templates/agents |
| raise_pr | Keep | TRUE | Controls whether PR is created |
| auto_merge | Keep | FALSE | Controls automated merging to integration branch |
| target_branch | Keep | NULL (-> main) | Must be non-protected when auto_merge=true |

## 6. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Hook fails to block merge to main | Low | Critical | Defense-in-depth: hook + agent instructions + GitHub branch protection |
| CI polling timeout in interactive mode | Medium | Low | User notified, can check manually |
| SurrealDB schema migration breaks existing data | Low | High | Migration script validates existing data first |
| Validation script false positives block legitimate work | Medium | Medium | Script exits with descriptive errors, not silent blocks |
| auto_commit deprecation breaks existing agents | Low | Low | Field stays in schema, just ignored -- no breaking change |

## 7. Decision Log

| ID | Decision | Status | Rationale |
|----|----------|--------|-----------|
| D1 | PF6-TSK-06 verify-pr-and-sync (3 modes) | Accepted | Covers interactive, autorun+merge, autorun+review scenarios |
| D2 | Hard block on protected branch merge | Accepted | Highest-risk operation, no bypass justified |
| D3 | autorun/{batch-name} integration branch | Accepted | Prevents automated merge to main |
| D4 | auto_merge + main = FORBIDDEN | Accepted | Defense-in-depth with D2 |
| D5 | Deprecate auto_commit (leave column) | Accepted | Safe, non-breaking, communicates intent |
| D6 | awaiting_review task status | Reversed | Removed — PR state is self-evident from GitHub |
| D7 | /cf-ship as notification | Accepted | Aligns with merge protection policy |
| D8 | PF3 reorder (conditional for adhoc) | Accepted | Fixes ordering fragility |
| D9 | Deterministic validation scripts | Accepted | Guards against LLM data errors |
| D10 | merge_protection in enforcement-policy.json | Accepted | Config-driven, consistent with existing patterns |
| D11 | Regular merge (not squash) for integration branch | Accepted | Preserves conventional commit format |
