---
# GENERATED - DO NOT EDIT THIS FRONTMATTER
id: INF-TSK-PLAN-PMGT-001_009
epic_id: INF-EPC-PLAN-PMGT-001
title: "Agent Definition Updates"
description: "Update cf-knowledge-layer.md to reinforce JSONL event routing, add epic-scoped task numbering rules, add QUAL/PMGT domains, and strengthen template enforcement. Update cf-planning.md to clarify that cf-planning designs but cf-knowledge-layer registers."
status: todo
area_type: INF
work_type: PLAN
domain: PMGT
origin: planned
file_scope: [".claude/agents/cf-knowledge-layer.md", ".claude/agents/cf-planning.md"]
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
  - "cf-knowledge-layer.md contains JSONL event routing table mapping each event type to its correct file"
  - "cf-knowledge-layer.md documents epic-scoped task numbering pattern: {AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{TASK_NNN}"
  - "cf-knowledge-layer.md lists QUAL and PMGT as valid domain codes"
  - "cf-knowledge-layer.md includes template enforcement rule: all new epic/task markdown must use project-management/templates/ as base"
  - "cf-planning.md explicitly states that cf-planning produces planning artifacts (markdown) and delegates DB registration to cf-knowledge-layer"
  - "cf-planning.md does not contain any instructions to write directly to DB or JSONL files"
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

# INF-TSK-PLAN-PMGT-001_009: Agent Definition Updates

## Description

Two agent definitions need updates to codify the standardized project management processes:

### cf-knowledge-layer.md Updates

1. **JSONL Event Routing:** Add an explicit routing table so the agent knows which events go to which files. This prevents the misrouting problem identified in MEMORY.md ("LLMs write events to wrong files").

2. **Epic-Scoped Task Numbering:** Add the new task ID format to the format_id generation instructions. When creating tasks, the agent must use `{AREA}-TSK-{TYPE}-{DOMAIN}-{EPIC_NNN}_{TASK_NNN}` pattern.

3. **Domain Codes:** Add QUAL (Quality assurance and testing) and PMGT (Project management) to the list of valid domain codes.

4. **Template Enforcement:** Add instruction that all new epic/task markdown files must be created using the templates at `project-management/templates/` as the base, ensuring frontmatter fields are never incomplete.

### cf-planning.md Updates

1. **Delegation Clarity:** Reinforce that cf-planning produces design artifacts (markdown files) but delegates all DB/JSONL registration to cf-knowledge-layer. The planner designs; the knowledge layer records.

## Approach

1. Read current cf-knowledge-layer.md to identify insertion points
2. Read current cf-planning.md to identify insertion points
3. Draft additions matching the 5-section agent definition format
4. Insert JSONL routing table in the appropriate SOP section of cf-knowledge-layer.md
5. Insert numbering rules near format_id generation instructions
6. Add QUAL/PMGT to domain code list
7. Add template enforcement instruction
8. Verify cf-planning.md delegation language

## Files

### To Modify

- `.claude/agents/cf-knowledge-layer.md` -- Add routing table, numbering rules, domains, template enforcement
- `.claude/agents/cf-planning.md` -- Clarify delegation to cf-knowledge-layer

## Dependencies

### Blocked By

- None (independent documentation task, but best done after Tasks 004 and 006 define the numbering scheme and templates)

### Blocks

- None

## Verification

### Automated

- [ ] cf-knowledge-layer.md contains string "work-graph.jsonl" in a routing context (not just casual mention)
- [ ] cf-knowledge-layer.md contains string "EPIC_NNN" or "epic-scoped" (numbering docs)
- [ ] cf-knowledge-layer.md contains both "QUAL" and "PMGT" as domain codes
- [ ] cf-knowledge-layer.md contains string "project-management/templates/" (template enforcement)
- [ ] cf-planning.md contains string "cf-knowledge-layer" in a delegation context

### Manual

- [ ] Agent definitions still follow the 5-section format (Identity, Constraints, SOPs, Communication, Quality Checklist)
- [ ] No existing instructions contradicted by new additions
- [ ] Routing table is comprehensive (covers all JSONL files)

## Notes

- Not autorun eligible because agent definition changes affect AI behavior and require human review of wording.
- The JSONL routing table in cf-knowledge-layer.md should match the table added to CLAUDE.md Section 10 (Task 008). Keep them consistent.
