---
id: "epic-01KJ55G4DMR9SQ5BCBRD8TN10T"
format_id: "INF-EPC-013"
title: "Infrastructure Bug Fixes"
summary: "Ad-hoc bug fixes to CodeFlow infrastructure: hooks, scripts, and enforcement pipeline"
status: complete
area_type: "INF"
work_type: "FIX"
domain: "ENFC"
is_ongoing: true
file_scope:
  - ".claude/hooks/codeflow/"
  - ".codeflow/scripts/"
  - ".claude/commands/"
  - ".codeflow/testing/claude-hooks/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23"
updated_at: "2026-02-23"
---

# INF-EPC-013: Infrastructure Bug Fixes

## Summary

Ongoing epic for ad-hoc bug fixes to CodeFlow infrastructure: hooks, scripts, enforcement pipeline, and related tooling. Tasks are added here when a targeted fix is needed outside the scope of a planned feature epic.

## Scope

### In Scope

- Hook script bug fixes (SessionStart, PreToolUse, PostToolUse, SessionEnd)
- Script correctness fixes in `.codeflow/scripts/`
- Enforcement pipeline bug fixes
- Slash command fixes

### Out of Scope

- New features (use FEAT epic)
- Documentation-only changes (use DOC epic)
- Refactors without bug fixes

## Acceptance Criteria

- [ ] Each task's bug is resolved and verified by tests
- [ ] No regressions introduced

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-013-001 | Fix stale session cleanup in SessionStart hook | in_progress | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Bug fixes to protected hook scripts require cf-security consultation per enforcement-policy.json protected resource policy.

## Related

- INF-EPC-010: Phase Checkpoint Enforcement (parent feature)
