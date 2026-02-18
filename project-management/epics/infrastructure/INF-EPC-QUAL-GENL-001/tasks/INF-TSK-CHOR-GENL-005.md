---
id: INF-TSK-CHOR-GENL-005
epic_id: INF-EPC-QUAL-GENL-001
title: Enhance cf-quality-assurance agent definition
description: Update cf-quality-assurance.md to default to full test mode, add coverage verification step, and update quality checklist
status: complete
area_type: INF
work_type: CHOR
domain: GENL
origin: planned
file_scope: [".claude/agents/cf-quality-assurance.md"]
scope_policy: hard
scope_root: null
estimate: S
priority: medium
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance:
  - "WS-QA SOP defaults to --mode full (not --mode standard)"
  - "Coverage verification step exists between regression check and verdict"
  - "Coverage step includes escalation path for gaps found"
  - "Quality checklist includes: Test coverage verified"
  - "Quality checklist includes: Full test mode used"
  - "Agent definition passes format consistency tests"
tests: []
branch: chore/enhance-qa-agent
pr_number: 31
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-17T22:40:41Z
started_at: 2026-02-17T22:00:00Z
completed_at: 2026-02-17T22:40:41Z
---

# INF-TSK-CHOR-GENL-005: Enhance cf-quality-assurance agent definition

## Description

Improve `.claude/agents/cf-quality-assurance.md` with three targeted changes to prevent the quality gaps that allowed pre-existing test failures to go undetected.

### Change 3a: Default to full test mode

The WS-QA SOP Step 2 currently defaults to `--mode standard`. This mode missed 5 test failures that only surface in `--mode full`. Change the default to `--mode full` so the QA gate catches all failures.

### Change 3b: Add coverage verification step

Add a new SOP step between Step 5 (regressions) and Step 6 (verdict) called "Step 5b: Verify Test Coverage". This step:

- Checks that all changed/new files have corresponding test files
- Checks that `test-config.json` is up to date for any new scripts
- If coverage gaps found: escalate to cf-development with specific files needing tests, or escalate to team lead if scope is unclear

### Change 3c: Update quality checklist

Add two new items to the quality checklist:

- "Test coverage verified -- all changed scripts have corresponding tests"
- "Full test mode used (not standard or essential)"

## Approach

1. Read `.claude/agents/cf-quality-assurance.md` to understand current structure
2. Locate WS-QA SOP Step 2 and change `--mode standard` to `--mode full`
3. Add "Step 5b: Verify Test Coverage" with clear escalation logic
4. Add two items to the quality checklist
5. Verify the agent definition still passes format consistency tests

## Files

### To Modify

- `.claude/agents/cf-quality-assurance.md` -- All three changes (3a, 3b, 3c)

## Dependencies

### Blocked By

- INF-TSK-FIX-GENL-007 (tests must be fixed before changing the QA default to full mode)

### Blocks

- None

## Verification

### Automated

- [ ] Agent definition format consistency tests pass

### Manual

- [ ] WS-QA SOP Step 2 references `--mode full` (not standard)
- [ ] Coverage verification step has clear escalation paths (to cf-development and to lead)
- [ ] Quality checklist has both new items
- [ ] No existing SOP steps removed or broken
- [ ] Step numbering is consistent after insertion

## Notes

- The SOP step numbering may need adjustment after inserting Step 5b. Ensure all step references within the document remain consistent.
- The coverage verification step should be lightweight (check for corresponding test files) not a full coverage analysis tool.
- Existing acceptance criteria, verdicts, and escalation patterns in the agent definition must be preserved.
