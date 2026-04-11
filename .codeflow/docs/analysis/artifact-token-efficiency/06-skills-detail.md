# Skill Detail Specifications

## Per-Skill Specifications

Each skill below includes: purpose, YAML description (with Used-by scope), source content extraction table, target operations, restoration approach, and estimated size.

### cf-pathflow-protocol (NEW)

```text
description: "PathFlow lifecycle procedures: phase execution, stage completion,
  criteria reporting, task tracker mirroring, rework loops, context overflow
  recovery. Used by: team-lead, cf-development, cf-planning, cf-documentation,
  cf-quality-assurance, cf-review. Not used by: cf-git-operations,
  cf-knowledge-layer, cf-security."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | CLAUDE.md | S4.2 Steps 1-8 (PF1-PF7 execution) | ~200 | execute-phase |
| 2 | CLAUDE.md | S3 Token-aware delegation | ~15 | manage-teammates |
| 3 | CLAUDE.md | S3 Sub-agent policy | ~20 | manage-teammates |
| 4 | CLAUDE.md | S5 Spawn patterns + task spec quality | ~77 | manage-teammates |
| 5 | CLAUDE.md | S5 Stage reporting (pipeline mapping) | ~50 | complete-stage |
| 6 | CLAUDE.md | S5 Recycling + deferred shutdown | ~60 | manage-teammates |
| 7 | CLAUDE.md | S5 Parallel batch + health | ~70 | manage-teammates |
| 8 | CLAUDE.md | S5 Name preservation + persistence | ~30 | manage-teammates |
| 9 | CLAUDE.md | S6 Rework loop flow | ~20 | handle-rework |
| 10 | CLAUDE.md | S6 Smart utilization examples | ~40 | manage-teammates |
| 11 | CLAUDE.md | S7 Sentinel system | ~20 | execute-phase |
| 12 | CLAUDE.md | S7 Task tracker mirroring | ~25 | execute-phase |
| 13 | CLAUDE.md | S7 Graceful degradation | ~15 | recover-session |
| 14 | CLAUDE.md | S11 Context overflow recovery | ~83 | recover-session |
| 15 | 5 role agent defs | Stage completion protocol (x5) | ~50 | complete-stage |
| | **Total source** | | **~775** | **Deduplicates to ~500-600** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | execute-phase | Step-by-step PF1-PF7 execution with task tracker mirroring |
| 2 | complete-stage | Criteria status update, stage report writing, STAGE-COMPLETE signaling |
| 3 | handle-rework | Rework loop flow: WS-REV changes_requested, WS-QA fail |
| 4 | recover-session | Context overflow recovery, teammate verification, state reconstruction |
| 5 | manage-teammates | Spawn patterns, parallel batch, deferred shutdown, health checks |

**Estimated size:** ~500-600 lines (~4K tokens)

---

### cf-git-workflow (RESTORE + EXTRACT)

```text
description: "Git operations procedures (branch, commit, PR, sync). Used by:
  cf-git-operations. Not used by: team-lead, cf-development, cf-planning."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | cf-git-operations.md | Step 1: Create Branch | ~34 | create-branch |
| 2 | cf-git-operations.md | Step 2: Create Commit | ~44 | create-commit |
| 3 | cf-git-operations.md | Step 3: Commit Outstanding | ~20 | create-commit |
| 4 | cf-git-operations.md | Step 4: Squash | ~47 | squash-branch |
| 5 | cf-git-operations.md | Step 5: Create PR | ~27 | create-pr |
| 6 | cf-git-operations.md | Step 6: Sync Remote | ~19 | verify-pr-sync |
| 7 | cf-git-operations.md | Step 7: Verify PR + Sync | ~62 | verify-pr-sync |
| 8 | cf-git-operations.md | Worktree, Merging, Review, Status, Rebase | ~46 | manage-worktree, create-branch |
| 9 | Archived cf-git-workflow | Full skill (468 lines) | ~468 | All operations (merge/update) |
| | **Total source** | | **~767** | **Deduplicates to ~400-500** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | create-branch | Branch naming, prefix table, creation procedure |
| 2 | create-commit | Commit format, validation, printf format, pre-commit hook recovery |
| 3 | squash-branch | Soft reset procedure, synthesized commit message |
| 4 | create-pr | PR format, gh pr create, sandbox bypass |
| 5 | verify-pr-sync | CI polling, mode-specific merge, sync-local |
| 6 | manage-worktree | Create, cleanup, scope conflict check |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-git-workflow/`
2. Read CURRENT cf-git-operations agent def Execution Steps -- identify improvements since archival
3. Read CURRENT CLAUDE.md sections referencing git operations -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Agent def after extraction:**

- cf-git-operations keeps: Identity, Constraints, Workflow diagram, Quality Checklist, Skills Referenced
- cf-git-operations removes: Execution Steps (procedures move to cf-git-workflow skill)
- Target: ~465 --> ~200 lines

**Estimated size:** ~400-500 lines (~3.5K tokens)

---

### cf-memory-management (RESTORE + UPDATE)

```text
description: "Memory lifecycle operations: work detection, context loading, progress
  recording, session tracking. Used by: cf-knowledge-layer. Not used by:
  team-lead, cf-development."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | Archived cf-memory-management | Full skill (423 lines) | ~423 | detect-active-work, load-work-context, record-progress, complete-work |
| 2 | cf-knowledge-layer.md | Part 1: Memory Management | ~131 | Merge updates into matching operations |
| | **Total source** | | **~554** | **Deduplicates to ~350-400** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | detect-active-work | active-task.json, DB query, present options |
| 2 | load-work-context | Three-tier loading, context compilation |
| 3 | record-progress | Event types, JSONL append, milestone tracking |
| 4 | complete-work | Finalization, epic rollup, commit sentinel |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-memory-management/`
2. Read CURRENT cf-knowledge-layer agent def Execution Steps -- identify improvements since archival
3. Read CURRENT CLAUDE.md sections referencing memory operations -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Estimated size:** ~350-400 lines (~2.8K tokens)

---

### cf-task-management (RESTORE + UPDATE)

```text
description: "Task management operations: work classification, epic/task CRUD,
  field validation, work session lifecycle. Used by: cf-knowledge-layer.
  Not used by: team-lead, cf-development, cf-planning."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | Archived cf-task-management | Full skill (409 lines) | ~409 | classify-work, ensure-registered, validate-fields, begin-work |
| 2 | cf-knowledge-layer.md | Part 2: Task Management | ~125 | Merge updates into matching operations |
| | **Total source** | | **~534** | **Deduplicates to ~350-400** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | classify-work | Area/work type classification tables |
| 2 | ensure-registered | Idempotent epic/task creation with ULID |
| 3 | validate-fields | Task/epic YAML validation scripts |
| 4 | begin-work | Work registration, active-task.json creation |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-task-management/`
2. Read CURRENT cf-knowledge-layer agent def Execution Steps -- identify improvements since archival
3. Read CURRENT CLAUDE.md sections referencing task management -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Estimated size:** ~350-400 lines (~2.8K tokens)

---

### cf-db-operations (RESTORE + UPDATE)

```text
description: "Database and persistence operations: three-tier model (JSONL/SurrealDB/
  Markdown), event recording, PathFlow transitions. Used by: cf-knowledge-layer.
  Not used by: team-lead, cf-development."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | Archived cf-db-operations | Full skill (468 lines) | ~468 | record-transitions, append-jsonl, query-surrealdb, sync-markdown |
| 2 | cf-knowledge-layer.md | Part 3: DB Operations | ~20 | Merge updates |
| 3 | cf-knowledge-layer.md | Part 4: PathFlow Events | ~51 | record-transitions updates |
| 4 | cf-knowledge-layer.md | Three-Tier + Canonical Ledger + Go CLI | ~45 | Reference section updates |
| | **Total source** | | **~584** | **Deduplicates to ~350-400** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | record-transitions | Phase/stage/session PathFlow events |
| 2 | append-jsonl | JSONL file routing, event type mapping |
| 3 | query-surrealdb | Indexed lookups, rebuilds from JSONL |
| 4 | sync-markdown | Tier 2 markdown derived views |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-db-operations/`
2. Read CURRENT cf-knowledge-layer agent def Execution Steps -- identify improvements since archival
3. Read CURRENT CLAUDE.md sections referencing DB/persistence operations -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Agent def after extraction (cf-knowledge-layer):**

- Keeps: Identity, Constraints, Workflow diagram, Quality Checklist, Skills Referenced, canonical ledger table
- Removes: Execution Steps Parts 1-4 (procedures move to cf-memory-management, cf-task-management, cf-db-operations)
- Target: ~570 --> ~180 lines

**Estimated size:** ~350-400 lines (~2.8K tokens)

---

### cf-security-operations (RESTORE + EXTRACT)

```text
description: "Security operations: sandbox classification, protected resource
  staging, permission diagnosis, settings template sync. Used by: cf-security.
  Not used by: team-lead, cf-development."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | cf-security.md | Step 1: Sandbox Classification | ~31 | classify-sandbox |
| 2 | cf-security.md | Step 2: Protected Resource Staging | ~57 | stage-protected |
| 3 | cf-security.md | Step 2b: Merge Protection | ~23 | check-merge |
| 4 | cf-security.md | Step 3: Permission Error Diagnosis | ~22 | diagnose-permission |
| 5 | cf-security.md | Step 4: Settings Template Sync | ~44 | sync-settings |
| 6 | Archived cf-security-management | Full skill (344 lines) | ~344 | All operations (merge/update) |
| | **Total source** | | **~521** | **Deduplicates to ~300-400** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | classify-sandbox | Operation classification table, autorun detection |
| 2 | stage-protected | Staging workflow, tmp path convention, apply commands |
| 3 | check-merge | Merge protection policy, protected branches |
| 4 | diagnose-permission | Error decision tree, root cause mapping |
| 5 | sync-settings | Template comparison, hooks/version sync rules |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-security-management/`
2. Read CURRENT cf-security agent def Execution Steps -- identify improvements since archival
3. Read CURRENT CLAUDE.md sections referencing security operations -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Agent def after extraction:**

- cf-security keeps: Identity, Constraints, Workflow diagram, Quality Checklist, Skills Referenced
- cf-security removes: Execution Steps (procedures move to cf-security-operations skill)
- Target: ~323 --> ~140 lines

**Estimated size:** ~300-400 lines (~2.5K tokens)

---

### cf-team-communication (NEW, EXTRACT)

```text
description: "Team communication patterns: SendMessage routing, commit request
  formats, progress updates, escalation patterns, stage completion messaging.
  Used by: all agents."
```

**Source content extraction map:**

| # | Source Artifact | Source Section | Lines | Target Operation |
|---|----------------|---------------|:----:|-----------------|
| 1 | CLAUDE.md | S5 Communication Patterns | ~30 | peer-message (routing table) |
| 2 | cf-git-operations.md | Communication (common patterns) | ~23 | request-commit, peer-message |
| 3 | cf-knowledge-layer.md | Communication (common patterns) | ~24 | report-progress, peer-message |
| 4 | cf-security.md | Communication (common patterns) | ~18 | escalate-to-lead |
| 5 | cf-development.md | Communication (common patterns) | ~15 | request-commit, report-progress |
| 6 | cf-planning.md | Communication (common patterns) | ~15 | request-commit, report-progress |
| 7 | cf-documentation.md | Communication (common patterns) | ~15 | request-commit, report-progress |
| 8 | cf-review.md | Communication (common patterns) | ~15 | report-progress, escalate |
| 9 | cf-quality-assurance.md | Communication (common patterns) | ~15 | report-progress, escalate |
| | **Total source** | | **~170** | **Deduplicates to ~150-200** |

**Target operations:**

| # | Operation | Purpose |
|---|-----------|---------|
| 1 | request-commit | "Please commit: {type}: {description}" format, file lists |
| 2 | report-progress | "{PREFIX}-UPDATE: task={id}, status={status}" format |
| 3 | escalate-to-lead | BLOCKED, ESCALATE patterns with reason format |
| 4 | signal-stage | "STAGE-COMPLETE: WS-{stage}" pattern |
| 5 | peer-message | Direct teammate routing table (who sends to whom) |

**Agent def after extraction:**

- Each agent keeps: Skills Referenced row for cf-team-communication
- Each agent removes: Common communication patterns (commit request, progress, escalation formats)
- Savings: ~15-20 lines per agent x 8 = ~120-160 lines total

**Estimated size:** ~150-200 lines (~1.5K tokens)

---

### cf-code-exploration (RESTORE)

```text
description: "Codebase exploration patterns: Glob/Grep/Read strategies, search
  patterns, code comprehension procedures. Used by: cf-planning, cf-development,
  cf-review. Not used by: cf-git-operations, cf-knowledge-layer."
```

**Source content:**

| Source | Lines | Changes |
|--------|:----:|---------|
| Archived cf-code-exploration | ~265 | Remove V3 refs, add YAML frontmatter with "Used by: cf-planning, cf-development, cf-review" |

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-code-exploration/`
2. Read CURRENT agent defs (cf-development, cf-planning, cf-review) -- identify exploration patterns that evolved since archival
3. Read CURRENT CLAUDE.md sections referencing codebase exploration -- verify alignment
4. Reconcile: archived structure + current agent def improvements + current CLAUDE.md conventions
5. Remove V3 sentinel references, update to V4 SKILL.md format with YAML frontmatter
6. Add Used-by scope in description field
7. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Estimated size:** ~200-250 lines (~1.8K tokens)

---

### cf-documentation-standards (RESTORE + MERGE cf-markdown-standards)

```text
description: "Documentation standards: markdown formatting, document templates
  (epic, task, ADR), section structure, style conventions, heading standards,
  quality checks. Used by: cf-planning, cf-documentation. Not used by:
  cf-development, cf-git-operations."
```

**Source content:**

| Source | Lines | Changes |
|--------|:----:|---------|
| Archived cf-documentation-standards | ~267 | Remove V3 refs, add YAML frontmatter |
| Current cf-markdown-standards skill | ~410 | Merge templates, formatting rules, lint rules into combined skill |

**Merge rationale:** "Markdown" is too narrow (file format). "Documentation standards" is the broader discipline covering templates, structure, style, and formatting. One merged skill replaces both.

**Restoration approach (review + revamp, NOT blind restore):**

1. Start from archived version at `.codeflow/docs/archived/skills/cf-documentation-standards/`
2. Read CURRENT cf-markdown-standards skill at `.claude/skills/cf-markdown-standards/SKILL.md` -- this is the second merge source
3. Read CURRENT cf-documentation agent def -- identify documentation standards that evolved since archival
4. Read CURRENT CLAUDE.md sections referencing documentation -- verify alignment
5. MERGE all three sources: archived skill structure + current cf-markdown-standards content + agent def improvements
6. Remove V3 sentinel references, update to V4 SKILL.md format with YAML frontmatter
7. Add Used-by scope in description field
8. Delete or archive cf-markdown-standards after merge (move to `.codeflow/docs/archived/skills/`)
9. Validate no procedures are lost (lossless merge) and skill reflects current project state

**Estimated size:** ~350-450 lines (~3K tokens)

## LOSSLESS Verification Summary

Every skill restoration and extraction follows the same verification pattern:

1. **Source identified:** Exact sections and line counts from current artifacts
2. **Destination mapped:** Specific operations within the target skill
3. **Access preserved:** Agent's YAML description includes skill name; spawn prompt instructs reading referenced skills
4. **Net knowledge change:** Function agents gain MORE content (decision trees from archived versions); role agents and lead gain shared skill access

| Skill | Source Lines | Target Lines | Net | Verification |
|-------|:-----------:|:----------:|:---:|-------------|
| cf-pathflow-protocol | ~775 | ~500-600 | Deduplicated | All 15 source blocks mapped to 5 operations |
| cf-git-workflow | ~767 | ~400-500 | Deduplicated + merged | Archive + agent def merged losslessly |
| cf-memory-management | ~554 | ~350-400 | Deduplicated + merged | Archive + agent def merged losslessly |
| cf-task-management | ~534 | ~350-400 | Deduplicated + merged | Archive + agent def merged losslessly |
| cf-db-operations | ~584 | ~350-400 | Deduplicated + merged | Archive + agent def merged losslessly |
| cf-security-operations | ~521 | ~300-400 | Deduplicated + merged | Archive + agent def merged losslessly |
| cf-team-communication | ~170 | ~150-200 | Deduplicated | 9 source blocks consolidated into 5 operations |
| cf-code-exploration | ~265 | ~200-250 | Updated | V3 refs removed, V4 format added |
| cf-documentation-standards | ~267 | ~200-250 | Updated | V3 refs removed, V4 format added |
