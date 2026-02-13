---
id: task-01KH9V2G4PVD874CTFYCYTN9MC
format_id: INF-TSK-RFCT-IDSY-009
epic_id: epic-01KH9V2FZDFSFQAV5WR2M7VVY8
epic_format_id: INF-EPC-RFCT-IDSY-001
title: "V4 Spec: Documentation, queries, and terminology (14+ items)"
description: Update terminology, documentation, query examples, and remaining spec files for dual-ID
status: todo
area_type: INF
work_type: RFCT
domain: IDSY
origin: research
file_scope: []
scope_policy: soft
scope_root: null
estimate: L
priority: normal
assignee_id: null
autorun_eligible: true
auto_commit: true
raise_pr: false
auto_merge: false
target_branch: null
acceptance: ["README terminology updated for dual-ID", "terminology.md format_id definition updated", "All query examples use ULID PKs correctly", "All 10 documentation files updated"]
tests: []
branch: null
pr_number: null
external_id: null
external_url: null
created_at: 2026-02-12T00:00:00Z
updated_at: 2026-02-12T00:00:00Z
started_at: null
completed_at: null
---

# INF-TSK-RFCT-IDSY-009: V4 Spec - Documentation, queries, and terminology

## Description

Update terminology, documentation files, and query examples across the V4 specification for dual-ID consistency. **Separate repository** (codeflow-specification-v4).

### Changes (14+ items from findings)

| Finding ID | File | Change |
|------------|------|--------|
| V4-TERM-01 | README.md L245 | "ULID everywhere" -> describe dual-ID |
| V4-TERM-02 | 01-overview/terminology.md L33 | Update format_id definition |
| V4-QRY-01 | 11-reference/database/db-schema.md L1216-1507 | All query examples (~50 locations) |
| V4-DOC-01 | 04-knowledge-layer/01-work-graph.md L585-643 | Stage event task_id refs |
| V4-DOC-02 | 04-knowledge-layer/01-work-graph.md L686-709 | Epic markdown frontmatter |
| V4-DOC-03 | 10-implementation/phase-4-agents.md | Task ID examples |
| V4-DOC-04 | 10-implementation/phase-5-commands.md | Command output examples |
| V4-DOC-05 | 10-implementation/phase-6-integration.md | Integration test examples |
| V4-DOC-06 | 10-implementation/phase-7-go-cli.md | CLI output examples |
| V4-DOC-07 | 11-reference/frameworks/cf-sub-agents-framework.md | Task ID refs |
| V4-DOC-08 | 11-reference/frameworks/cf-task-list-framework.md | Task ID refs |
| V4-DOC-09 | 11-reference/file-mapping/05-project-root.md | Epic dir structure |
| V4-DOC-10 | 09-autorun/batch-files.md | Batch file examples |

## Dependencies

### Blocked By

- INF-TSK-RFCT-IDSY-007 (schema specs)
- INF-TSK-RFCT-IDSY-008 (event specs)

### Blocks

- None (final documentation pass)

## Notes

- **Repository**: codeflow-specification-v4 (NOT this repo)
- V4-QRY-01 alone covers ~50 query example locations
- Research reference: `.codeflow/docs/research/dual-id-system-findings.md` Sections 3.2, 3.5, 3.6
