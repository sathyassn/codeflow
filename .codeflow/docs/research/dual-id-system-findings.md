# Dual-ID System: Findings & Implementation Tracker

**Date**: 2026-02-12
**Branch**: fix/pre-phase4-gaps
**Task**: INF-TSK-FIX-GENL-005
**Decision**: Tier 3 - Project-wide architectural change

## 1. Design Decision

**Option B (Confirmed)**: Every table uses `{prefix}-{ulid}` as PRIMARY KEY. Work graph tables (epics, tasks) additionally have a `format_id` column (TEXT UNIQUE NOT NULL) for human-readable identification.

```text
Epic PK:     epic-{ulid}                         (e.g., epic-01ARZ3NDEKTSV4RRFFQ69G5FAV)
Epic FID:    {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}    (e.g., FRT-EPC-FEAT-AUTH-001)
Task PK:     task-{ulid}                         (e.g., task-01BRZ4PDFLUTW5SSGG70H6GBW)
Task FID:    {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}    (e.g., FRT-TSK-FEAT-AUTH-001)
```

### Usage Rules

| Context | Use id (ULID PK) | Use format_id |
|---------|-------------------|---------------|
| Database FK references | Always | Never |
| Internal lookups/joins | Always | Never |
| User-facing display | Never | Always |
| File/folder names | Never | Always |
| Commit messages | Never | Always |
| Branch names | Never | Always |
| API responses | Include both | Include both |
| active-task.json | task_id, epic_id | task_format_id, epic_format_id |
| Ledger events | id, epic_id, task_id | format_id (also include) |
| Markdown frontmatter | id (ULID) | format_id (human-readable) |

---

## 2. Current State (Problem)

- V3 and V4 specs both claim "ULID everywhere" but schemas use structured format IDs as PKs
- format_id defined once in terminology.md but never implemented in any schema
- Shell generate_epic_id() produces EPC-{ulid} - neither the current format ID nor the correct ULID PK prefix
- Shell is_valid_epic_id() validates EPC-{ulid} - wrong
- Python validation.py validates {AREA}-EPC-{TYPE}-{DOMAIN}-{NNN} - correct for format_id but labeled as epic_id
- No migration path exists

---

## 3. Findings: V4 Specification

### 3.1 Schema Definitions (5 files, 10 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| V4-SCH-01 | 11-reference/database/db-schema.md | 150-176 | DONE | epics table: add format_id, change id comment |
| V4-SCH-02 | 11-reference/database/db-schema.md | 192-253 | DONE | tasks table: add format_id, change id comment |
| V4-SCH-03 | 11-reference/database/db-schema.md | 1513-1517 | DONE | ID Formats reference table |
| V4-SCH-04 | 11-reference/database/db-schema.md | new | DONE | Add UNIQUE indexes on format_id |
| V4-SCH-05 | 11-reference/database/lifecycle/04-work-graph-tables.md | 28-47 | DONE | epics schema |
| V4-SCH-06 | 11-reference/database/lifecycle/04-work-graph-tables.md | 145-193 | DONE | tasks schema |
| V4-SCH-07 | 04-knowledge-layer/01-work-graph.md | 216-243 | DONE | epics schema |
| V4-SCH-08 | 04-knowledge-layer/01-work-graph.md | 283-334 | DONE | tasks schema |
| V4-SCH-09 | 10-implementation/phase-2-knowledge-layer.md | 68-89 | DONE | epics schema |
| V4-SCH-10 | 10-implementation/phase-2-knowledge-layer.md | 91-122 | DONE | tasks schema |

### 3.2 Terminology and Overview (2 files, 2 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| V4-TERM-01 | README.md | 245 | DONE | "ULID everywhere" -> describe dual-ID |
| V4-TERM-02 | 01-overview/terminology.md | 33 | DONE | Update format_id definition |

### 3.3 JSONL Event Schemas (1 file, 5 items covering ~20 events)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| V4-EVT-01 | 12-jsonl-event-reference.md | 179-198 | DONE | epic_created: id -> ULID, add format_id |
| V4-EVT-02 | 12-jsonl-event-reference.md | 200-213 | DONE | epic_status_changed: id -> ULID |
| V4-EVT-03 | 12-jsonl-event-reference.md | 215-234 | DONE | task_created: id -> ULID, epic_id -> ULID, add format_id |
| V4-EVT-04 | 12-jsonl-event-reference.md | 236-280 | DONE | task_completed, dependency_added, criterion_met |
| V4-EVT-05 | 12-jsonl-event-reference.md | 307-616 | DONE | progress, stage events, work_started |

### 3.4 Validation and Shell Specs (2 files, 3 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| V4-VAL-01 | shell-modularization-architecture.md | 1030-1040 | DONE | Update is_valid_epic_id/task_id regex, add format_id validators |
| V4-VAL-02 | shell-modularization-architecture.md | 1222-1228 | DONE | Update generate_epic_id/task_id, add format_id generators |
| V4-VAL-03 | python-modularization-architecture.md | 529-536 | DONE | Split PATTERNS dict: ULID PK + format_id |

### 3.5 Query Examples (1 file, 1 item covering ~50 locations)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| V4-QRY-01 | 11-reference/database/db-schema.md | 1216-1507 | DONE | All query examples |

### 3.6 Markdown and Other Docs (10 files, 10 items)

| ID | File | Status | Notes |
|----|------|--------|-------|
| V4-DOC-01 | 04-knowledge-layer/01-work-graph.md L585-643 | DONE | Stage event task_id refs |
| V4-DOC-02 | 04-knowledge-layer/01-work-graph.md L686-709 | DONE | Epic markdown frontmatter |
| V4-DOC-03 | 10-implementation/phase-4-agents.md | DONE | Task ID examples updated to format_id |
| V4-DOC-04 | 10-implementation/phase-5-commands.md | DONE | Command output examples updated to format_id |
| V4-DOC-05 | 10-implementation/phase-6-integration.md | DONE | Integration test examples updated to format_id |
| V4-DOC-06 | 10-implementation/phase-7-go-cli.md | DONE | CLI output examples |
| V4-DOC-07 | 11-reference/frameworks/cf-sub-agents-framework.md | DONE | Task ID refs |
| V4-DOC-08 | 11-reference/frameworks/cf-task-list-framework.md | DONE | Task ID refs |
| V4-DOC-09 | 11-reference/file-mapping/05-project-root.md | DONE | Epic dir structure |
| V4-DOC-10 | 09-autorun/batch-files.md | DONE | Batch file examples |

---

## 4. Findings: CodeFlow Repository

### 4.1 Database Schema (1 file, 3 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| CF-SCH-01 | .codeflow/scripts/db/schema.sql | 110-142 | DONE | epics: add format_id, change id |
| CF-SCH-02 | .codeflow/scripts/db/schema.sql | 144-202 | DONE | tasks: add format_id, change id |
| CF-SCH-03 | .codeflow/scripts/db/schema.sql | 204-227 | DONE | FK comment updates |

### 4.2 ID Generation and Validation (4 files, 4 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| CF-GEN-01 | .codeflow/scripts/shell-lib/ulid.sh | 87-94 | DONE | generate_epic_id -> "epic-", generate_task_id -> "task-" |
| CF-GEN-02 | .codeflow/scripts/shell-lib/validation.sh | 78-88 | DONE | Update regex, add format_id validators |
| CF-GEN-03 | .codeflow/scripts/codeflow_py_lib/validation.py | 13-20 | DONE | Rename patterns, add format_id patterns |
| CF-GEN-04 | NEW: .codeflow/scripts/db/generate-format-id.sh | - | DONE | Format ID generator |

### 4.3 State and Ledger Scripts (3 files, 3 items)

| ID | File | Lines | Status | Notes |
|----|------|-------|--------|-------|
| CF-STATE-01 | .codeflow/scripts/state/work-state.sh | 62-93 | DONE | Accept both ULID + format_id |
| CF-STATE-02 | .codeflow/scripts/state/ledger.sh | 254-288 | DONE | Add format_id to record_* functions |
| CF-STATE-03 | .codeflow/scripts/memory/cf-memory-store.py | 67-68 | DONE | Document work_id = ULID PK |

### 4.4 Coordination Scripts (5 files, 5 items - docs/comments)

| ID | File | Status | Notes |
|----|------|--------|-------|
| CF-COORD-01 | .codeflow/scripts/coordination/cf-claim-acquire.py | DONE | Document work_id = ULID |
| CF-COORD-02 | .codeflow/scripts/coordination/cf-claim-check.py | DONE | Same |
| CF-COORD-03 | .codeflow/scripts/coordination/cf-claim-list.py | DONE | Same |
| CF-COORD-04 | .codeflow/scripts/coordination/cf-claim-release.py | DONE | Same |
| CF-COORD-05 | .codeflow/scripts/coordination/cf-claim-renew.py | DONE | Same |

### 4.5 Skill Documentation (8 files, 8 items)

| ID | File | Status | Notes |
|----|------|--------|-------|
| CF-SKILL-01 | .claude/skills/cf-task-management/SKILL.md L149-286 | DONE | Update operations |
| CF-SKILL-02 | .claude/skills/cf-task-management/resources/id-convention.md | DONE | Complete rewrite |
| CF-SKILL-03 | .claude/skills/cf-task-management/resources/task-lifecycle.md | DONE | Update task_id refs |
| CF-SKILL-04 | .claude/skills/cf-db-operations/SKILL.md L88-152 | DONE | Update create operations |
| CF-SKILL-05 | .claude/skills/cf-db-operations/resources/schema-reference.md L15-68 | DONE | Add format_id |
| CF-SKILL-06 | .claude/skills/cf-db-operations/resources/query-templates.md L22-82 | DONE | Add format_id to queries |
| CF-SKILL-07 | .claude/skills/cf-memory-management/SKILL.md L110-346 | DONE | Clarify ULID in active work |
| CF-SKILL-08 | .claude/skills/cf-memory-management/resources/active-work-schema.md L8-16 | DONE | Add format_id fields |

### 4.6 Test Files (6+ files, 6 items)

| ID | File | Status | Notes |
|----|------|--------|-------|
| CF-TEST-01 | .codeflow/testing/scripts/shell-lib/test-ulid.sh | DONE | Update generate_* tests |
| CF-TEST-02 | .codeflow/testing/scripts/shell-lib/test-validation.sh | DONE | Update is_valid_* tests, add format_id tests |
| CF-TEST-03 | .codeflow/testing/scripts/codeflow_py_lib/test_validation.py | DONE | Split PK + format_id tests |
| CF-TEST-04 | .codeflow/testing/scripts/coordination/test_cf_claim_*.py | DONE | Update mock task_id |
| CF-TEST-05 | .codeflow/testing/claude-hooks/pre-tool-use/test-cf-pre-tool-use-task-sentinel.sh | DONE | Update mock active-task.json |
| CF-TEST-06 | .codeflow/testing/claude-hooks/session-start/test-cf-session-start-*.sh | DONE | Update mock task_id |

### 4.7 Project Management (varies, 4 items)

| ID | File | Status | Notes |
|----|------|--------|-------|
| CF-PM-01 | project-management/README.md L26-29 | DONE | Clarify format_id in filenames |
| CF-PM-02 | project-management/epics/README.md | DONE | Same |
| CF-PM-03 | All existing epic .md frontmatter | DONE | Add format_id, change id to ULID (3 epics) |
| CF-PM-04 | All existing task .md frontmatter | DONE | Add format_id, epic_format_id (14 tasks) |

### 4.8 New Files (2 items)

| ID | File | Status | Notes |
|----|------|--------|-------|
| CF-NEW-01 | .codeflow/scripts/db/generate-format-id.sh | DONE | Sequence counter per AREA-TYPE-DOMAIN |
| CF-NEW-02 | .codeflow/scripts/db/migrate-to-dual-id.sh | DONE | Migration script |

---

## 5. Implementation Phases

| Phase | Items | Task (CF Repo) | Task (V4 Spec) | Depends On | Status |
|-------|-------|----------------|----------------|------------|--------|
| 1. Schema | V4-SCH-*, CF-SCH-* | INF-TSK-RFCT-IDSY-001 | INF-TSK-RFCT-IDSY-007 | None | DONE |
| 2. ID Generation | V4-VAL-*, CF-GEN-* | INF-TSK-RFCT-IDSY-002 | INF-TSK-RFCT-IDSY-008 | Phase 1 | DONE |
| 3. Business Logic | CF-STATE-*, CF-COORD-* | INF-TSK-RFCT-IDSY-003 | - | Phase 2 | DONE |
| 4. Events | V4-EVT-* | - | INF-TSK-RFCT-IDSY-008 | Phase 2 | DONE |
| 5. Documentation | V4-TERM-*, CF-SKILL-* | INF-TSK-RFCT-IDSY-004 | INF-TSK-RFCT-IDSY-009 | Phase 1-2 | DONE |
| 6. Tests | CF-TEST-* | INF-TSK-RFCT-IDSY-005 | - | Phase 2-3 | DONE |
| 7. Migration/Docs | CF-PM-*, CF-NEW-* | INF-TSK-RFCT-IDSY-006 | INF-TSK-RFCT-IDSY-009 | Phase 1-3 | DONE |

**Epic**: INF-EPC-RFCT-IDSY-001 (Dual-ID System: ULID Primary Keys with Human-Readable Format IDs)

---

## 6. Progress Log

| Date | Phase | Items | Status | Notes |
|------|-------|-------|--------|-------|
| 2026-02-12 | Research | All | COMPLETE | 3 audit agents, 46+ files found |
| 2026-02-12 | Document | This file | COMPLETE | Findings documented |
| 2026-02-12 | Task Creation | 9 tasks | COMPLETE | Epic + 9 tasks created in project-management |
| 2026-02-12 | CF Implementation | 30/37 CF items | COMPLETE | Team of 5 agents, 2 batches. Phases 1-6 done for CF repo |
| 2026-02-12 | CF Remaining | 7/7 CF items | COMPLETE | Team of 3 agents. Scripts, PM READMEs, frontmatter (3 epics, 14 tasks) |
| 2026-02-12 | V4 Spec | 31/31 V4 items | COMPLETE | Team of 3 agents (schema, events/val/term, docs/queries). 3 minor warns in phase-4/5/6 pseudo-code |
| 2026-02-12 | Verification | All 68 items | COMPLETE | 2 verification agents. CF: 7/7 pass. V4: 13/16 pass, 3 warn (cosmetic) |
