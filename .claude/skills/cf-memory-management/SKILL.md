---
name: cf-memory-management
description: Provides work lifecycle operations from initiation to completion. Manages active work tracking, progress recording, and context recovery. Use when starting work, recording progress, or completing tasks.
context: fork
agent: cf-general-purpose
---

# Memory Management Skill

## Type

**Procedural** - Provides step-by-step work lifecycle and memory operations.

## Purpose

**Ensures all work is properly tracked via the active_work table, enabling session continuity and scope validation.**

## Three-Tier Data Model

```text
┌─────────────────────────────────────────────────────────────┐
│ Tier 0: JSONL Ledger (Append-Only Authority)                │
│ Location: .state/memory/*.jsonl                             │
│ Purpose: Immutable audit trail, crash recovery source       │
│ Rule: NEVER modify, only append                             │
├─────────────────────────────────────────────────────────────┤
│ Tier 1: SQLite (Fast Queries)                               │
│ Location: .state/db/codeflow.db                             │
│ Purpose: Indexed queries, relationships, aggregations       │
│ Rule: Can rebuild from Tier 0 if corrupted                  │
├─────────────────────────────────────────────────────────────┤
│ Tier 2: Markdown (Human-Readable)                           │
│ Location: epics/**/*.md, .claude/memory/**                  │
│ Purpose: Human review, git diffs, documentation             │
│ Rule: Generated from Tier 1, not authoritative              │
└─────────────────────────────────────────────────────────────┘

Consistency Rules:
  - Tier 0 is source of truth (append-only)
  - Tier 1 can be rebuilt from Tier 0
  - Tier 2 is rendered from Tier 1
  - All writes go: Tier 1 → Tier 0 → Tier 2
```

## Responsibilities

- Detect active work at session start
- Load work context for resumption
- Search for related work across worktrees
- Evaluate scope and impact of proposed work
- Register work before modifications (begin-work)
- Track progress and decisions during work
- Archive context before commit (complete-work)
- Manage memory lifecycle and archival
- NOT: Task CRUD (that's cf-task-management)
- NOT: Database operations (that's cf-db-operations)

## Decision Tree

```text
Session starting?
└── 🔧 detect-active-work (find in-progress work)

Resuming work?
└── 🔧 load-work-context (restore full context)

Before creating worktree?
└── 🔧 search-related-work (check scope conflicts)

Before making changes?
└── 🔧 evaluate-context (assess scope/impact)

Starting new work?
├── Has task_id? → 🔧 begin-work
└── No task_id? → cf-task-management:ensure-work-registered first

After significant milestone?
└── 🔧 record-work-progress

Before committing?
└── 🔧 complete-work (creates sentinel for commit)

Periodic maintenance?
└── 🔧 manage-memory-lifecycle
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | detect-active-work | ENF-L3 Advisory | Find in-progress work at session start |
| 2 | load-work-context | None | Load existing work context |
| 3 | search-related-work | ENF-L1 Sentinel | Find related work across worktrees |
| 4 | evaluate-context | ENF-L3 Advisory | Assess scope and impact of proposed work |
| 5 | begin-work | ENF-L1 Sentinel | Register active work (task_id required) |
| 6 | record-work-progress | ENF-L3 Advisory | Track progress and decisions |
| 7 | complete-work | ENF-L1 Sentinel | Archive context before commit |
| 8 | manage-memory-lifecycle | ENF-L1 Sentinel | Archive and prune old data |

## Operation Details

### 🔧 detect-active-work

```text
When: Session start (via UserPromptSubmit hook)
Purpose: Find in-progress work for context recovery
Enforcement: ENF-L3 Advisory

Procedure:
  1. Query active_work via cf-db-operations:memory-query
  2. For each active work, get recent progress events
  3. Present options: resume, start new, cleanup stale

Output:
  active_work: [{id, topic, branch, last_progress}...]
  recommended: {most recent work ID}

Cross-skill: cf-db-operations:memory-query
Hook: UserPromptSubmit/user-prompt-submit.sh triggers
Query: .state/db/queries/memory-queries.sql#detect-active

📚 Resource: [active-work-schema.md](resources/active-work-schema.md)
   Load when: Understanding active work data structure or debugging detection
```

### 🔧 load-work-context

```text
When: Resuming work (/cf-resume or detect-active-work)
Purpose: Restore full work context for continuation
Enforcement: None

Procedure:
  1. Query work details via cf-db-operations:memory-query
  2. Load recent progress events from memory_events
  3. Update session with loaded context
  4. Display context summary

Output:
  work_id: {loaded work ID}
  task_id: {task ID}
  scope: {file patterns}
  progress: {recent events summary}
  deliverables: {remaining items}

Cross-skill: cf-db-operations:memory-query
Query: .state/db/queries/memory-queries.sql#load-context

📚 Resources:
   [active-work-schema.md](resources/active-work-schema.md) - Load when: Understanding context structure
   [recovery-procedures.md](resources/recovery-procedures.md) - Load when: Context load fails or data corrupted
```

### 🔧 search-related-work

```text
When: Before creating worktree OR starting work that may conflict
Purpose: Find related work across worktrees to prevent scope conflicts
Enforcement: ENF-L1 Sentinel (prerequisite for create-worktree)

Procedure:
  1. Extract scope patterns from proposed work
  2. Query active_work via cf-db-operations:memory-query
  3. For each, check pattern overlap
  4. If conflicts: Return details, suggest coordination

Output:
  conflicts: [{work_id, topic, branch, overlapping_files}...]
  safe_to_proceed: true | false

Cross-skill: cf-git-workflow:create-worktree (prerequisite), cf-db-operations:memory-query
Query: .state/db/queries/memory-queries.sql#search-related

📚 Resource: [scope-patterns.md](resources/scope-patterns.md)
   Load when: Understanding scope pattern syntax or debugging conflicts
```

### 🔧 evaluate-context

```text
When: Before making changes
Purpose: Assess scope and impact of proposed work
Enforcement: ENF-L3 Advisory

Procedure:
  1. Identify files likely to be affected
  2. Check for related work via search-related-work
  3. Assess impact radius
  4. Determine if scope expansion needed

Output: Scope assessment with conflict warnings

📚 Resource: [scope-patterns.md](resources/scope-patterns.md)
   Load when: Defining file scope or assessing impact radius
```

### 🔧 begin-work

```text
When: Starting work on a task (task_id required)
Enforcement: ENF-L1 Sentinel - Required before any Edit/Write/Bash modifications

Prerequisite: task_id must exist (from /cf-develop or ensure-work-registered)

CRITICAL: task_id is NOT NULL - every modification must be tracked

Autorun Mode Detection:
  If $AUTORUN_SESSION_ID is set:
    - task_id from $AUTORUN_TASK_ID (already assigned by orchestrator)
    - Work already registered by Go CLI
    - Skip work registration, just load context
    - Focus on acceptance criteria from $AUTORUN_ACCEPTANCE

Procedure:
  1. Check for autorun context ($AUTORUN_SESSION_ID)

  2. If in autorun:
     - task_id = $AUTORUN_TASK_ID
     - Load existing work context (Go CLI pre-registered)
     - Parse acceptance criteria from $AUTORUN_ACCEPTANCE
     - Skip registration (already done)

  3. If interactive:
     - Validate task exists and is actionable (no blocking deps)
     - Create work agreement in memory domain
     - Register active_work via cf-db-operations:memory-store
     - Append to JSONL ledger (Tier 0)
     - Update task status via cf-db-operations:task-update

Output:
  work_id: work-{ulid}
  task_id: {task_id}
  branch: {branch}
  scope: {file_scope}
  autorun_mode: true | false
  acceptance_criteria: [{criteria list}] (if autorun)

On Failure:
  - task_id NULL: BLOCK with "invoke /cf-develop first"
  - task blocked: BLOCK with "unresolved dependencies: {blockers}"

Cross-skill: cf-db-operations:memory-store, cf-db-operations:task-update
Query: .state/db/queries/memory-queries.sql#begin-work

📚 Resource: [active-work-schema.md](resources/active-work-schema.md)
   Load when: Understanding work registration requirements or troubleshooting
```

### 🔧 record-work-progress

```text
When: After significant milestones, decisions, or file modifications
Purpose: Track progress and decisions for context recovery
Enforcement: ENF-L3 Advisory | PostToolUse hook reminds after Edit/Write

Trigger Points:
  - After completing a deliverable item
  - After making a key decision
  - After modifying multiple files (batch)
  - Before context window rotation

Event Types:
  | Event Type | Trigger |
  |------------|---------|
  | milestone | Deliverable completed |
  | decision | Choice made with rationale |
  | progress | File modifications batch |
  | context_save | Before rotation |

Procedure:
  1. Determine event type
  2. Record event via cf-db-operations:memory-store
  3. Update work agreement markdown (Tier 2)
  4. Append to JSONL ledger (Tier 0)

Output:
  event_id: {generated ID}
  summary: {one-line description}

Cross-skill: cf-db-operations:memory-store
Hook: PostToolUse/post-tool-use-memory-progress.sh (reminder)
Query: .state/db/queries/memory-queries.sql#store

📚 Resource: [active-work-schema.md](resources/active-work-schema.md)
   Load when: Recording progress events or understanding event types
```

### 🔧 complete-work

```text
When: Before committing changes
Enforcement: ENF-L1 Sentinel | PreToolUse blocks "git commit" without this

CRITICAL: Must be invoked BEFORE cf-git-workflow:create-commit

Autorun Mode Behavior:
  In autorun context:
    - Verify ALL acceptance criteria from $AUTORUN_ACCEPTANCE
    - This verification is REQUIRED for Stop hook to approve exit
    - Record which criteria are met/unmet
    - If criteria unmet: Continue working (don't try to exit)

Procedure:
  1. Check for autorun context

  2. Verify deliverables:
     - Interactive: Check work agreement deliverables
     - Autorun: Check acceptance criteria from $AUTORUN_ACCEPTANCE

  3. Update work agreement status (Tier 2 markdown)

  4. Archive work context via cf-db-operations:memory-store (status='completed')

  5. Update task status via cf-db-operations:task-update

  6. Record completion event via cf-db-operations:memory-store

  7. Create sentinel for git commit (TTL: 600s)

  8. In autorun: Prepare verification summary for Stop hook

Output:
  work_id: {completed work ID}
  task_id: {task ID}
  sentinel_created: true
  commit_allowed_until: {expiry timestamp}
  autorun_criteria_met: [{criterion, met: true/false}] (if autorun)

Cross-skill: cf-git-workflow:create-commit (enables via sentinel), cf-db-operations:memory-store, cf-db-operations:task-update
Query: .state/db/queries/memory-queries.sql#complete-work

Note: In autorun, the Stop hook (Haiku LLM) performs additional
verification. Both complete-work and Stop hook must approve for
the task to be marked complete.

📚 Resource: [active-work-schema.md](resources/active-work-schema.md)
   Load when: Understanding completion requirements or sentinel mechanics
```

### 🔧 manage-memory-lifecycle

```text
When: Periodic maintenance or /cf-cleanup
Enforcement: ENF-L1 Sentinel

CRITICAL: JSONL ledger is append-only authority (Tier 0)

Three-Tier Update Pattern:
  1. Write to SQLite (Tier 1) - fast, indexed
  2. Append to JSONL (Tier 0) - immutable audit
  3. Update Markdown (Tier 2) - human-readable

Procedure:
  1. Identify stale data via cf-db-operations:memory-query (completed work > 30 days)
  2. Archive to cold storage (.claude/memory/archive/)
  3. Prune from SQLite hot tables (Tier 1)
  4. Append archive event to JSONL (Tier 0) - NEVER delete from JSONL
  5. Update/remove Tier 2 markdown files
  6. Optional: VACUUM database

Configuration:
  | Setting | Default | Purpose |
  |---------|---------|---------|
  | retention_days | 30 | Days before completed work archived |
  | orphan_threshold_days | 7 | Days before orphaned events pruned |
  | vacuum_threshold_mb | 50 | DB size threshold for auto-vacuum |

Output:
  archived_work_count: {number archived}
  archived_events_count: {number archived}
  pruned_claims_count: {number pruned}
  tier0_events_appended: {count}
  status: success | partial | failed

On Failure:
  - If archive write fails: ROLLBACK, keep data in hot storage
  - If JSONL append fails: CRITICAL - halt and alert
  - If Tier 2 update fails: WARN but continue (can regenerate)

Cross-skill: cf-db-operations:memory-query
Query: .state/db/queries/memory-queries.sql#lifecycle

📚 Resources:
   [active-work-schema.md](resources/active-work-schema.md) - Load when: Understanding archive structure
   [recovery-procedures.md](resources/recovery-procedures.md) - Load when: Recovery after failed lifecycle operation
```

## Memory Domains

| Domain | Primary Agent | Content Types |
|--------|---------------|---------------|
| planning | planner | Briefs, requirements, decisions |
| development | developer | Work agreements, progress logs |
| review | reviewer | Review notes, feedback |
| qa | qa | Test plans, coverage reports |
| ops | ops | Deployment logs, incident notes |
| documentation | documenter | Documentation notes, standards |

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [active-work-schema.md](resources/active-work-schema.md) | Full schema documentation | When debugging |
| [scope-patterns.md](resources/scope-patterns.md) | Pattern syntax reference | When defining scope |
| [recovery-procedures.md](resources/recovery-procedures.md) | Advanced recovery | When load-work-context fails |
