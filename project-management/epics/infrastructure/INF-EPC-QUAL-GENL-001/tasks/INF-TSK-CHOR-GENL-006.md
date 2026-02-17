---
id: INF-TSK-CHOR-GENL-006
epic_id: INF-EPC-QUAL-GENL-001
title: Enhance cf-review agent definition
description: Add file-type standards loading, security review checklist, logic/correctness checklist, and standards enforcement step to cf-review.md
status: todo
area_type: INF
work_type: CHOR
domain: GENL
origin: planned
file_scope: [".claude/agents/cf-review.md"]
scope_policy: hard
scope_root: null
estimate: M
priority: medium
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: true
auto_merge: false
target_branch: main
acceptance:
  - "File-type decision tree exists with clear skill-loading instructions"
  - "Security checklist section added, applies to ALL review modes"
  - "Logic/correctness checklist added to CODE_REVIEW mode"
  - "Standards enforcement step exists in SOP workflow"
  - "Quality checklist updated with new items"
  - "Agent definition passes format consistency tests"
  - "All existing review mode checklists preserved (enhanced, not replaced)"
tests: []
branch: chore/enhance-review-agent
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-16T00:00:00Z
updated_at: 2026-02-16T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-CHOR-GENL-006: Enhance cf-review agent definition

## Description

Improve `.claude/agents/cf-review.md` with five targeted changes (4a-4e) that make the review process more systematic by integrating language-specific standards, security checks, and logic verification.

### Change 4a: File-type decision tree for standards loading

Add a new SOP step between Step 3 (read files) and Step 4 (apply criteria) called "Step 3b: Load Relevant Standards". This step includes a decision tree:

```text
File under review:
+-- *.sh --> Load cf-shell-standards skill, apply ShellCheck rules
+-- *.py --> Load cf-python-standards skill, apply ruff/flake8 rules
+-- *.md --> Load cf-markdown-standards skill, apply doc structure rules
+-- *.json --> Validate schema structure, check for hardcoded values
+-- Agent defs (.claude/agents/cf-*.md) --> Apply agent format consistency + markdown standards
+-- Hook scripts (.claude/hooks/**/*.sh) --> cf-shell-standards + MANDATORY security review
+-- Command defs (.claude/commands/cf-*.md) --> cf-markdown-standards + instruction consistency
```

The reviewer MUST read the relevant skill file before applying review criteria. For each file type, load the corresponding skill and cross-reference findings against its rules.

### Change 4b: Security review checklist

Add a security review checklist that applies to ALL review modes (not just CODE_REVIEW):

- Command injection (unquoted variables, eval, unsanitized input in shell)
- Path traversal (relative paths, symlink following, user-controlled paths)
- Information leakage (secrets in logs/output, error messages exposing internals)
- Privilege escalation (unnecessary permissions, bypassing hooks/guards)
- Input validation (boundary checks, type checks at system boundaries)
- Hardcoded credentials/paths/tokens
- For instruction files (agent defs, commands): prompt injection vectors, scope creep, unauthorized capability grants

### Change 4c: Logic/correctness checklist

Enhance CODE_REVIEW mode with a logic/correctness checklist:

- Edge cases handled (empty input, null, overflow, concurrent access)
- Error paths tested (what happens on failure? are resources cleaned up?)
- State consistency (are temp files/locks cleaned up on all exit paths?)
- Idempotency (can this safely run twice without side effects?)
- Boundary conditions (off-by-one, empty arrays, max values)
- Race conditions (concurrent file access, shared state)

### Change 4d: Standards enforcement step

Add a new SOP step after applying review criteria (Step 4) and before verdict (Step 7) called "Step 6b: Cross-Reference Standards":

- For each file reviewed, verify findings against the loaded skill's rules
- Flag standards violations as MAJOR findings
- If no relevant skill was loaded for a file type, note it as a gap

### Change 4e: Update quality checklist

Add three items to the quality checklist:

- "Relevant standards skills loaded and cross-referenced for each file type"
- "Security checklist applied to all files (not just code)"
- "Logic/correctness checklist applied (CODE_REVIEW mode)"

## Approach

1. Read `.claude/agents/cf-review.md` to understand current structure, SOP steps, and review modes
2. Identify insertion points for new steps (3b after Step 3, 6b after Step 6)
3. Add file-type decision tree as Step 3b
4. Add security review checklist section (applicable to all modes)
5. Add logic/correctness checklist to CODE_REVIEW mode section
6. Add standards enforcement step as Step 6b
7. Update quality checklist with 3 new items
8. Verify step numbering consistency throughout the document
9. Verify agent definition passes format consistency tests

## Files

### To Modify

- `.claude/agents/cf-review.md` -- All five changes (4a-4e)

## Dependencies

### Blocked By

- None (this task can start independently)

### Blocks

- None (but once merged, subsequent reviews benefit from the enhanced process)

## Verification

### Automated

- [ ] Agent definition format consistency tests pass

### Manual

- [ ] File-type decision tree present with all 7 file patterns
- [ ] Each file pattern maps to a specific skill or validation approach
- [ ] Security checklist has 7 items (6 technical + 1 instruction-specific)
- [ ] Security checklist explicitly states it applies to ALL review modes
- [ ] Logic/correctness checklist has 6 items
- [ ] Logic/correctness checklist is scoped to CODE_REVIEW mode
- [ ] Standards enforcement step references skill cross-referencing
- [ ] Quality checklist has 3 new items
- [ ] All existing review mode checklists (CODE_REVIEW, DESIGN_REVIEW, DOCUMENTATION_REVIEW, TEST_REVIEW) preserved
- [ ] SOP step numbering is consistent (no gaps, no duplicates)

## Notes

- This is the largest change (estimate M) because cf-review.md is a complex agent definition with multiple review modes and detailed SOP steps.
- All existing checklists and review criteria must be preserved -- this task enhances, not replaces.
- The security checklist for instruction files (agent defs, commands) addresses prompt injection and scope creep, which are unique security concerns for AI agent definitions.
- Once this task is merged, all subsequent WS-REV stages benefit from the stricter review process. Tasks reviewed before this merge use the existing review criteria.
