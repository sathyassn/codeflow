---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_008
epic_id: INF-EPC-PLAN-PMGT-001
title: "CLAUDE.md Process Updates"
description: "Add epic/task creation delegation rules to CLAUDE.md Section 4 (PF3-CLASSIFY) and Section 6 (Work Pipelines). Document formal planning vs ad-hoc creation paths. Add JSONL event routing table to Section 10."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: [".claude/CLAUDE.md"]
scope_policy: hard
scope_root: null
estimate: S
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "CLAUDE.md Section 4 (PF3-CLASSIFY) documents that epic/task creation is delegated to cf-knowledge-layer, not done by team lead directly"
  - "CLAUDE.md Section 6 (Work Pipelines) includes a 'Work Item Creation' subsection documenting two paths: formal planning (via cf-planning -> cf-knowledge-layer) and ad-hoc (via cf-knowledge-layer directly)"
  - "CLAUDE.md Section 10 (Memory) includes a JSONL Event Routing table mapping event types to their correct JSONL files (work-graph.jsonl, sessions.jsonl, memory-events.jsonl)"
  - "No existing content in CLAUDE.md is deleted or reworded beyond what is needed for the additions"
  - "All additions use the same formatting style (tables, code blocks, section headers) as the rest of CLAUDE.md"
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-18T19:00:00Z
updated_at: 2026-02-18T19:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-PLAN-PMGT-001_008: CLAUDE.md Process Updates

## Description

CLAUDE.md currently lacks explicit documentation on how epics and tasks are created. The team lead needs clear delegation rules and the two creation paths must be documented so future sessions follow a consistent process.

### Additions Required

**Section 4 addition (PF3-CLASSIFY):**
Add a note after the work classification step that epic/task creation is always delegated:

- Formal planning: team lead spawns cf-planning for design, cf-planning messages cf-knowledge-layer for DB registration
- Ad-hoc work: team lead messages cf-knowledge-layer directly to register task created during session

**Section 6 addition (Work Pipelines):**
Add "Work Item Creation" subsection documenting:

| Path | When | Who Creates Epic | Who Registers in DB |
|------|------|-----------------|---------------------|
| Formal planning | Planned feature, multi-task effort | cf-planning (markdown) | cf-knowledge-layer (DB + JSONL) |
| Ad-hoc work | Bug fix, chore, single-task session | Team lead defines scope | cf-knowledge-layer (DB + JSONL + markdown) |

**Section 10 addition (Memory):**
Add JSONL Event Routing table:

| JSONL File | Valid Event Types | Written By |
|-----------|------------------|-----------|
| work-graph.jsonl | epic_created, task_created, epic_status_changed, task_status_changed, dependency_added, dependency_removed | cf-knowledge-layer |
| sessions.jsonl | session_started, session_ended, phase_transition, stage_transition, begin_work, complete_work | cf-knowledge-layer |
| memory-events.jsonl | memory_stored, memory_updated, memory_recalled | cf-knowledge-layer |
| memory-events.jsonl | decision_recorded | cf-knowledge-layer |

## Approach

1. Read current CLAUDE.md to identify exact insertion points for each addition
2. Draft additions matching existing formatting style
3. Insert at correct locations without disrupting existing content
4. Verify section numbering and cross-references remain intact

## Files

### To Modify

- `.claude/CLAUDE.md` -- Add 3 content blocks (Section 4, Section 6, Section 10)

## Dependencies

### Blocked By

- None (independent documentation task)

### Blocks

- None

## Verification

### Automated

- [ ] CLAUDE.md contains string "Work Item Creation" (new subsection)
- [ ] CLAUDE.md contains string "JSONL Event Routing" (new table)
- [ ] CLAUDE.md contains string "cf-knowledge-layer" in Section 4 PF3-CLASSIFY context

### Manual

- [ ] Section numbering still correct (no gaps, no duplicates)
- [ ] New content follows same markdown formatting as surrounding sections
- [ ] Cross-references (e.g., "See Section 10") still point to correct targets

## Notes

- This task modifies CLAUDE.md which is the team lead's primary instruction file. Changes must be minimal and precise — add content, don't restructure existing content.
- Not autorun eligible because CLAUDE.md changes require human judgment about wording and placement.
