---
id: "epic-01KHSQPQRNQP0XTXCRHXX9YW1T"
format_id: "INF-EPC-005"
title: "Project Management Standardization"
summary: "Rationalize format IDs, folder structure, DB schema, validation scripts, and documentation across the CodeFlow project"
status: complete
area_type: "INF"
work_type: "CHOR"
domain: "PMGT"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: "INF-EPC-PLAN-PMGT-001"
external_url: null
created_at: "2026-02-18T23:00:00Z"
updated_at: "2026-02-22T00:00:00Z"
---

# INF-EPC-005: Project Management Standardization

## Summary

Rationalize format IDs, folder structure, DB schema, validation scripts, and documentation across the CodeFlow project. This epic implements the decisions from PLN-TSK-001-001 (Plan PM Standardization), migrating from the old verbose format ID convention ({AREA}-EPC-{WTYPE}-{DOMAIN}-{NNN}) to the new simplified convention ({AREA}-EPC-{NNN}).

## Scope

### In Scope

- DB schema migration (new columns, seed data updates)
- Format ID convention updates (schema.sql, validation scripts)
- ID convention documentation rewrite
- Project management folder restructuring
- Work-graph.jsonl cleanup
- Agent definition updates for new format
- CLAUDE.md process reference updates
- Template updates
- Old folder cleanup

### Out of Scope

- New features or capabilities beyond PM standardization
- Go CLI implementation
- Phase 5 V4 commands

## Acceptance Criteria

- [x] DB schema updated with V4 columns and new seed data
- [x] Format ID validation scripts match new convention
- [x] id-convention.md fully rewritten
- [x] Project management folders use area codes (PLN/, INF/, DOC/)
- [x] work-graph.jsonl entries use new format IDs
- [x] Agent definitions reference new format patterns
- [x] CLAUDE.md updated for new convention
- [x] Old epic/task folders removed

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-005-001 | DB schema migration — add V4 columns to tasks table | complete | high |
| INF-TSK-005-002 | Update schema.sql for new format ID convention | complete | high |
| INF-TSK-005-003 | Rewrite id-convention.md | complete | normal |
| INF-TSK-005-004 | Update validation scripts for new format | complete | normal |
| INF-TSK-005-005 | Clean up work-graph.jsonl | complete | normal |
| INF-TSK-005-006 | Update project-management READMEs | complete | normal |
| INF-TSK-005-007 | Update dual-id-system-findings.md and revamp-proposal | complete | low |
| INF-TSK-005-008 | Update cf-markdown-standards templates | complete | normal |
| INF-TSK-005-009 | Update agent definitions for new format | complete | normal |
| INF-TSK-005-010 | Delete old epic/task folders | complete | normal |
| INF-TSK-005-011 | Update CLAUDE.md process references | complete | normal |
| INF-TSK-005-012 | Remaining pre-Phase 4 infrastructure gaps | complete | low |
| INF-TSK-005-013 | Fix task tracker registration instructions | complete | normal |

## Dependencies

### Blocked By

- PLN-TSK-001-001 (complete — planning session that produced this epic)

### Blocks

- None

## Technical Notes

Old format ID: INF-EPC-PLAN-PMGT-001. This epic supersedes INF-EPC-004 (Dual-ID System) and absorbs remaining work from INF-TSK-003-005.

## Related

- PLN-TSK-001-001: Plan PM Standardization (produced this epic)
- INF-EPC-004: Dual-ID System (superseded by this epic)
- INF-TSK-003-005: Fix pre-Phase 4 infrastructure gaps (remaining work absorbed)
