---
id: task-01KH9V2FZSQ3Z5E556S5WNV60G
format_id: INF-TSK-FEAT-GENL-001
epic_id: epic-01KH9V2FYMB8M8R3GVT935CF82
epic_format_id: INF-EPC-FEAT-GENL-001
title: "Phase 4 V4: Agent definitions - 8 teammate .md files + spawn instructions"
description: Create 8 agent definition markdown files for CodeFlow V4 teammate agents with spawn instructions
status: todo
area_type: INF
work_type: FEAT
domain: GENL
origin: informal
file_scope: []
scope_policy: soft
scope_root: null
estimate: null
priority: normal
assignee_id: null
autorun_eligible: false
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-08T00:00:00Z
updated_at: 2026-02-08T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-FEAT-GENL-001: Phase 4 V4 - Agent Definitions

## Description

Create 8 agent definition markdown files for CodeFlow V4 teammate agents with spawn instructions. This is Phase 4 of the V4 implementation, following the completed Phase 3 (hooks, schema, security-lib, settings templates).

## Approach

Define each of the 8 teammate agent roles as structured markdown files under `.claude/agents/`, with spawn instructions that specify how to initialize each agent type in a team context.

## Files

### To Modify

- None expected

### To Create

- `.claude/agents/cf-planner.md` - Planner agent definition
- `.claude/agents/cf-developer.md` - Developer agent definition
- `.claude/agents/cf-reviewer.md` - Reviewer agent definition
- `.claude/agents/cf-qa.md` - QA agent definition
- `.claude/agents/cf-architect.md` - Architect agent definition
- `.claude/agents/cf-documenter.md` - Documenter agent definition
- `.claude/agents/cf-ops.md` - Ops agent definition
- `.claude/agents/cf-general-purpose.md` - General purpose agent definition

## Dependencies

### Blocked By

- None (Phase 3 already merged)

### Blocks

- None

## Verification

### Automated

- [ ] All 8 agent definition files exist
- [ ] Each file follows the agent definition template format

### Manual

- [ ] Each agent definition includes spawn instructions
- [ ] Role responsibilities are clearly defined
- [ ] Skill mappings are correct per V4 specification

## Notes

Phase 3 V4 was completed and merged as PR #4. This task continues the V4 implementation with agent definitions.
