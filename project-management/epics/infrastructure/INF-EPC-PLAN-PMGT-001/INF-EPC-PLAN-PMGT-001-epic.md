---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-EPC-PLAN-PMGT-001
title: "Project Management Standardization"
summary: "Standardize project management infrastructure: DB schema migration, epic/task format normalization, epic-scoped task numbering, JSONL cleanup, work item templates, tracking regeneration, and process documentation updates."
status: planning
area_type: INF
work_type: PLAN
domain: PMGT
is_ongoing: false
priority: high
file_scope: ["project-management/**", ".claude/agents/cf-knowledge-layer.md", ".claude/agents/cf-planning.md", ".claude/CLAUDE.md", ".codeflow/scripts/db/**", ".state/ledger/work-graph.jsonl"]
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-18T19:00:00Z
updated_at: 2026-02-18T19:00:00Z
---

# INF-EPC-PLAN-PMGT-001: Project Management Standardization

## Summary

Standardize the CodeFlow project management infrastructure to resolve 16 identified issues across DB schema, epic/task formats, naming conventions, JSONL integrity, templates, tracking, and process documentation. This epic establishes the canonical format for all future work items and aligns markdown artifacts 1:1 with the database schema.

## Scope

### In Scope

- DB schema migration: add missing V4 columns, add QUAL/PMGT domain codes
- Epic format normalization: reclassify invalid work types, reconcile DB vs filesystem, sync statuses
- Task format normalization: convert markdown-table tasks to YAML frontmatter, convert format-ID-as-ID tasks to dual-ID format
- Epic-scoped task numbering: migrate from global sequential to `{EPIC_NNN}_{TASK_NNN}` pattern
- work-graph.jsonl cleanup: remove misrouted events, normalize keys, deduplicate
- Work item templates: create standardized templates aligned to DB columns
- Tracking regeneration: rebuild epic-tracker.md from normalized data
- CLAUDE.md process updates: document epic/task creation delegation and JSONL routing
- Agent definition updates: reinforce routing rules in cf-knowledge-layer and cf-planning
- Script updates: update generate-format-id.sh for epic-scoped numbering

### Out of Scope

- Dual-ID migration (ULID PKs) -- covered by INF-EPC-RFCT-IDSY-001
- New epic/task creation workflow tooling (Go CLI Phase 7)
- Test suite changes for project management scripts
- Automated CI/CD enforcement of format standards

## Acceptance Criteria

- [ ] DB schema includes all V4 columns (acceptance, tests, auto_commit, raise_pr, auto_merge, target_branch, stage, stage_status, stage_history) and QUAL/PMGT domain codes
- [ ] All epic markdown files have complete YAML frontmatter aligned 1:1 with DB epics table columns
- [ ] All task markdown files have complete YAML frontmatter aligned 1:1 with DB tasks table columns
- [ ] All tasks use epic-scoped numbering pattern (`{AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{TASK_NNN}`)
- [ ] work-graph.jsonl contains only valid work_graph events with consistent key names
- [ ] project-management/templates/ contains epic-template.md and task-template.md aligned to DB schema
- [ ] epic-tracker.md regenerated from normalized data
- [ ] CLAUDE.md documents epic/task creation delegation rules and JSONL event routing
- [ ] cf-knowledge-layer.md and cf-planning.md updated with PMGT-specific rules
- [ ] generate-format-id.sh supports epic-scoped task numbering

## Tasks

| ID | Title | Status | Estimate | Priority | Depends On |
|----|-------|--------|----------|----------|------------|
| INF-TSK-PLAN-PMGT-001_001 | DB Schema Migration | todo | M | high | -- |
| INF-TSK-PLAN-PMGT-001_002 | Epic Standardization | todo | M | normal | 001 |
| INF-TSK-PLAN-PMGT-001_003 | Task Standardization | todo | M | normal | 001 |
| INF-TSK-PLAN-PMGT-001_004 | Epic-Scoped Task Numbering | todo | L | high | 001 |
| INF-TSK-PLAN-PMGT-001_005 | work-graph.jsonl Cleanup | todo | S | normal | 001 |
| INF-TSK-PLAN-PMGT-001_006 | Work Item Templates | todo | S | normal | -- |
| INF-TSK-PLAN-PMGT-001_007 | Tracking Regeneration | todo | XS | low | 002, 003 |
| INF-TSK-PLAN-PMGT-001_008 | CLAUDE.md Process Updates | todo | S | normal | -- |
| INF-TSK-PLAN-PMGT-001_009 | Agent Definition Updates | todo | S | normal | -- |
| INF-TSK-PLAN-PMGT-001_010 | Script Updates | todo | S | normal | 004 |

## Dependencies

### Blocked By

- None

### Blocks

- Future work item creation (all new epics/tasks will follow standardized format)
- INF-EPC-RFCT-IDSY-001 (dual-ID migration benefits from clean schema baseline)

## Technical Notes

### Dependency Graph

```text
001 (DB Schema) ──┬──> 002 (Epic Std)  ──┬──> 007 (Tracking)
                  ├──> 003 (Task Std)  ──┘
                  ├──> 004 (Numbering) ────> 010 (Scripts)
                  └──> 005 (JSONL)

006 (Templates)    (independent)
008 (CLAUDE.md)    (independent)
009 (Agent Defs)   (independent)
```

### Key Decisions

- **Epic-scoped numbering:** Tasks numbered within their epic (`001_001`) rather than globally (`001`). Prevents ID collisions, makes task-to-epic relationship visible in the ID.
- **format_id as id:** Because the dual-ID migration (INF-EPC-RFCT-IDSY-001) hasn't been applied yet, the live DB uses format_id strings as primary keys. All frontmatter uses `id:` with the format_id value.
- **Dog-fooding:** This epic and its tasks are the first artifacts following the standardized format, serving as both deliverable and reference implementation.

## Related

- INF-EPC-RFCT-IDSY-001 (Dual-ID System -- dependent on clean schema baseline)
- INF-EPC-FEAT-GENL-001 (Infrastructure features -- prior work tracking)
- INF-EPC-QUAL-GENL-001 (Quality improvements -- will be reclassified by Task 002)

## Progress

- 2026-02-18: Epic created, planning session initiated on branch `plan/project-management-standardization`
- 2026-02-18: Analysis completed, 10 tasks defined with full specifications
