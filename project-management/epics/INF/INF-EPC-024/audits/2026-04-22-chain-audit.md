---
title: "INF-EPC-024 Chain Audit 2026-04-22"
date: 2026-04-22
epic_id: "epic-01kk242qx0myzsevaw2m47vj0a"
epic_format_id: "INF-EPC-024"
triggered_by: "INF-TSK-024-035 (PR #303) — post-migration gap detection"
methodology: "Two explore agents, source verification against HEAD, task format_id enumeration across filesystem and DB"
---

# INF-EPC-024 Chain Audit — 2026-04-22

## 1. Executive Summary

After INF-TSK-024-035 (pathflow-events.jsonl migration from `.state/logs/` to `.state/ledger/`) merged in PR #303, a chain audit identified 12 gaps across the data layer. Eleven came from the initial audit; the twelfth was discovered during format_id renumber when two orphan tasks (038, 039) were found on the filesystem that are not in the epic task table. Six new tasks (INF-TSK-024-040 through 045), one task rescope (020 — add SurrealDB rebuild cursor metadata), and one epic amendment (implementation order) are proposed to close the gaps without expanding scope beyond INF-EPC-024's data-layer-standardization charter.

**Net deliverables of this audit:**

| Category | Count | Details |
|----------|-------|---------|
| New tasks | 6 | 040 (consumer fix), 041 (event repair), 042 (session-id regression prevention), 043 (cross-worktree aggregator), 044 (auto compaction), 045 (bookkeeping + orphan reconciliation) |
| Task rescopes | 1 | 020 — rebuild cursor + incremental sync |
| Epic amendments | 1 | Add Implementation Order section |
| Memory hygiene | 1 | MEMORY.md UUID Session ID Bug (Systemic) block → resolved |
| Open observations (no task created) | 1 | SurrealDB `DELETE BY STRING ID` returns success but row persists — future tech-debt |

## 2. Epic Objective (Verbatim from INF-EPC-024.md Summary)

> This epic standardizes the CodeFlow data layer: JSONL ledger schemas, schema enforcement in the Rust CLI writer, retention and rotation policies, DB-to-JSONL rebuild mapping, and three-tier consistency tooling. It addresses accumulated schema drift across four ledger files and multiple log files from shell-era vs Rust-era writer differences and ad-hoc field additions by multiple agent eras.

## 3. Genesis and Motivation

### 3.1 Commit trail

The audit is grounded in the following merge history, which should be the reference frame for every gap claim below:

- **PR #221** (commit d2c51445, merged 2026-03-25) — Worktree ledger isolation. Moved ledgers to LOCAL per-worktree directories. This PR also delivered the `migrate_flat_to_subdirs` pattern that INF-TSK-024-035 reused.
- **PR #258** (commit f192ba04, merged 2026-04-07) — `codeflow interactive` command + INF-TSK-024-037.
- **PR #301** (merged on `refactor/inf-tsk-024-028-session-id-consolidation`) — Session ID consolidation (INF-TSK-024-028).
- **PR #303** (merged on `fix/inf-tsk-024-035-pathflow-events-migration`) — pathflow-events migration (INF-TSK-024-035). Trigger for this audit.

### 3.2 Relationship to downstream epics

- Epic A (INF-EPC-023 Parallel Execution) shipped the worktree + CRDT + claims machinery. PR #221 rested on Epic A's worktree lifecycle APIs.
- Epic C (Global Intelligence Layer) is a prospective consumer of the standardized data layer this epic builds. Unresolved gaps in INF-EPC-024 directly reduce Epic C's quality.

### 3.3 Why the audit was needed

INF-TSK-024-035 fixed the producer side of the pathflow-events file location (writer routed to the ledger subdir; session-start migration moved the historical file). The PR's task markdown and DEV Report explicitly called out unresolved consumer drift (`report.rs:22`, `doctor/mod.rs:682`) as out-of-scope pre-existing references. Rather than let those references rot into silent zero-event output, the team ran a broader audit to catalogue every downstream gap the migration surfaced and to check whether PR #221's "cross-session merging at compaction time" design promise is actually realized. It is not — compaction exists but is never auto-invoked. Those findings plus the verification-closure entries below drove the 6 new tasks.

## 4. Task State Rollup (authoritative as of 2026-04-22)

Filesystem enumeration at audit time: 39 task files (INF-TSK-024-001 through INF-TSK-024-039) existed before this audit's renumber; 6 new files (040–045) drafted in this session bring the total to 45.

**Status breakdown (pre-draft, from on-disk frontmatter `status:` fields — verified by grep at audit time):**

| Status | Count | Task IDs |
|--------|-------|----------|
| complete | 17 | 001, 002, 003, 004, 005, 006, 007, 016, 017, 028, 030, 032, 033, 034, 035, 037, 039 |
| in_progress | 2 | 025, 031 |
| cancelled | 1 | 013 (duplicate of 007) |
| todo | 19 | 008, 009, 010, 011, 012, 014, 015, 018, 019, 020, 021, 022, 023, 024, 026, 027, 029, 036, 038 |

Pre-draft total: 17 + 2 + 1 + 19 = 39. ✓

**Corrections to the earlier "14/37 complete" figure:** the memory file's snapshot was stale. Two completions (033, 034 — ledger audits for coordination-events and autorun-events) landed after that snapshot. Task 035 (pathflow-events migration) landed during the trigger PR #303. Task 039 (DOCS template alignment) is `complete` on disk but `in_progress` in the DB — task 045 reconciles the DB to match. Task 031 is `in_progress` on disk despite the epic table showing `complete` — this discrepancy is flagged as a separate bookkeeping note for task 045.

**Post-draft state (after drafting 040–045, before they execute):**

- 45 total
- 17 complete (unchanged by drafting)
- 2 in_progress (unchanged)
- 1 cancelled (unchanged)
- 25 todo (19 previous + 6 new drafts)

## 5. Verification Closure

During the audit, several initial claims were verified against HEAD source code and either confirmed, corrected, or closed. The table records every verification outcome for traceability:

| Initial claim | Verification outcome |
|---------------|---------------------|
| Tasks 016–027, 029 AC quality unknown | Verified: 016 & 017 COMPLETE (DOCS, retention policies well-specified). 018, 020, 021, 023, 024, 026, 027, 029 = WELL-SPECIFIED. 019 & 022 = mildly vague but acceptable. 025 = in_progress (ongoing epic refinement). No STUB tasks found. |
| Task 030 delivery evidence missing | Delivered via PR #221 (commit d2c51445, merged 2026-03-25). Task 030 and PR #221's ledger-isolation work were the same PR. |
| Task 037 delivery evidence missing | Delivered via PR #258 (commit f192ba04, merged 2026-04-07, branch `feat/interactive-command`). |
| pre_tool_use.rs GateCheck session_id source unclear | Verified canonical. `cli/src/cmd/hooks/pre_tool_use.rs:99-106` passes `current_session_id(project_dir)` — the env-file helper. Not stdin UUID. |
| TeamGuard flagged HIGH in memory UUID bug | False positive. TeamGuard has no `session_id` field at all. MEMORY.md "Systemic UUID Session ID Bug" block is stale — hygiene captured in task 042 AC 10. |
| Ledger compaction actual behavior unclear | Verified working but manual-only. `ledger/compact.rs:17-27` algorithm is correct; `pub fn compact_ledger_type` at line 33 and `pub fn compact_all` at line 141. Walks single ledger dir. No auto-trigger anywhere — promoted to gap G5 (task 044). |
| SurrealDB rebuild cursor existence unclear | Confirmed: no cursor. `sync_from_events()` at `store/surreal.rs:1051-1101` processes every event from t=0 on every invocation. No metadata table, no checkpointing. One UPDATE per event (not batched) — promoted to gap G6 (task 020 rescope). |

## 6. Gap Map — 12 Gaps

Severity is stated post-verification. The "Still a gap?" column records the disposition after source-level checking.

| # | Gap | Severity | Still a gap? | Source evidence | Resolution |
|---|-----|----------|-------------|----------------|------------|
| G1 | `codeflow report` / `codeflow doctor` silently read pathflow events from the stale `.state/logs/` path after INF-TSK-024-035 | CRITICAL | Yes | `cli/src/cmd/report.rs:22` `logs_dir.join("pathflow-events.jsonl")`; `core/src/doctor/mod.rs:682` `logs_dir.join("pathflow-events.jsonl")` | New Task 040 |
| G2 | Historical pathflow-events file contains mis-routed `begin_work` / `complete_work` / `progress` / `stale_work_cleanup` events from the shell era | HIGH | Yes | `ledger/routing.rs:52-57` routes these types correctly for NEW events; historical file retains shell-era residue | New Task 041 |
| G3 | Session ID UUID residue from shell-era writer | LOW (downgraded) | Marginal | Verified all hook handlers use `current_session_id()`; MEMORY.md block is stale | New Task 042 (regression prevention only) |
| G4 | Cross-worktree visibility: queries from main repo miss worktree-local ledger events | MEDIUM | Yes | Since PR #221, `.state/ledger/` is LOCAL per-worktree (`worktree/mod.rs:192` LOCAL_STATE_DIRS); no aggregator exists | New Task 043 |
| G5 | `compact_ledger_type` exists but is never auto-invoked; worktree destruction loses ledger events | MEDIUM | Yes | `ledger/compact.rs:33,141` — pub functions; grep shows zero callers from `hooks/session_end.rs` or `worktree/cleanup.rs` | New Task 044 |
| G6 | SurrealDB rebuild processes every event from t=0 on every run; no cursor, no batching | MEDIUM | Yes | `store/surreal.rs:1051-1101` — single UPDATE per event, full scan | Rescope Task 020 (add cursor AC, incremental sync, batched UPDATE) |
| G7 | Tier 2 drift detector | MEDIUM | Covered | Task 021 already well-specified; verify autorun clobber detection in its plan phase | No action — task exists |
| G8 | Bookkeeping: delivery evidence missing on 14 completed tasks (pr_number, branch, completed_at null) | LOW | Yes | Frontmatter spot-check on 001, 003, 035 | New Task 045 |
| G9 | File-scope overlap between TODO tasks would cause autorun claim conflicts if executed in parallel | MEDIUM | Yes | Tasks 008, 010, 011, 012, 014, 015 all touch `ledger/jsonl.rs`, `ledger/mod.rs`, `types/events.rs`, `types/ids.rs` | Epic amendment — add serial Implementation Order section |
| G10 | Unread TODO task scope | — | Closed (verified) | All 22 TODO tasks read and verified well-specified (except 019, 022 mildly vague) | No action |
| G11 | Audit deliverables for 033/034 already complete; schema-enforcement chain can consume them | LOW | Closed | Disk status: 033=complete, 034=complete (verified via frontmatter grep). The "audits-first" concern raised at audit time is already satisfied; remaining task is to ensure the Schema-enforcement chain actually consumes 033/034 outputs when drafting 007-amendment | Implementation Order documents the consumption order (033/034 outputs feed 007-amendment); no new task |
| G12 | Orphan tasks in filesystem not in epic task table (038 todo-on-disk, 039 DB/disk status drift) | LOW | Yes | `project-management/epics/INF/INF-EPC-024/tasks/INF-TSK-024-038.md` (task_id `task-pending-038`) and 039 (DB=in_progress, disk=complete) | Task 045 AC 8-11 |

## 7. Gap-to-Task Mapping

The six new tasks and the one rescope map 1:1 to the gaps:

| Gap | Action | Task / change | Key acceptance criteria |
|-----|--------|--------------|------------------------|
| G1 | Fix consumers, add `resolve_path` helper and regression lint | **INF-TSK-024-040** (S, FIX, high) | 14 ACs covering helper API, consumer refactor, regression test, coverage |
| G2 | Idempotent streaming repair + CLI subcommand | **INF-TSK-024-041** (S, FIX, normal) | 18 ACs covering function API, CLI, atomic rewrite, dry-run, corrupt-line tolerance, concurrent-writer behavior, SurrealDB rebuild acceptance |
| G3 | Regression-prevention helper + lint + SKILL doc + memory hygiene | **INF-TSK-024-042** (S, RFCT, normal) | 13 ACs covering helper, migration, lint, SKILL update, MEMORY.md staged edit |
| G4 | Streaming k-way merge aggregator with dedup | **INF-TSK-024-043** (M, FEAT, normal) | 16 ACs covering aggregator API, CLI flags, tolerance to missing/stale worktrees, deduplication, memory-profile test |
| G5 | Auto-trigger compaction + promotion at SessionEnd / worktree cleanup | **INF-TSK-024-044** (M, FEAT, normal) | 18 ACs covering SessionEnd wiring, compact_and_promote API, transactional promotion, opt-out, rescue-bundle preservation, crash-recovery test |
| G6 | Rebuild cursor metadata + incremental sync + checkpointing + batched UPDATE | **INF-TSK-024-020 RESCOPE** (append 4 ACs) | Existing 14 ACs preserved; 4 new: metadata table, incremental sync, checkpointing, batched UPDATE |
| G8, G12 | Evidence backfill + orphan reconciliation | **INF-TSK-024-045** (S, CHOR, normal) | 14 ACs covering frontmatter backfill, evidence format, no-scope-creep discipline, 038 orphan disposition, 039 DB sync, epic table update |
| G9, G11 | Sequencing through epic amendment | **Epic amendment** — `## Implementation Order` section | Three ordered chains: schema-enforcement serial, consumer+data-layer parallel, DB+autorun sequential |

## 8. Implementation Order (Rationale)

The `## Implementation Order` amendment to the epic specifies three chains. Rationale per chain:

**Schema-enforcement chain (serial execution required — G9 resolution).**
`007-amendment (if needed, informed by already-complete 033+034) → 014 → 008 → 010 → 011 → 009 → 012 → 015 → 023`

Tasks 008, 010, 011, 012, 014, 015, 023 all modify `ledger/jsonl.rs`, `ledger/mod.rs`, `types/events.rs`, and `types/ids.rs`. Parallel execution would produce autorun claim conflicts under any `scope_policy` setting; `soft` would block dynamically, `hard` would block immediately. Serial is the only viable order.

Note: Tasks 033 (coordination-events audit) and 034 (autorun-events audit) are already `complete` on disk as of 2026-04-22, so they are NOT scheduled in this chain — their audit outputs feed into `007-amendment` only if the amendment is deemed necessary after reviewing their findings.

**Consumer + data-layer (independent, parallelizable).**
`035 → 040 → 041 → 043 → 044` with `045` (bookkeeping, any time) and `042` (session-id regression-prevention, any time after 028).

Each of these tasks has a non-overlapping `file_scope` and works on different code paths: 040 fixes two consumers; 041 is a new module; 043 is another new module; 044 wires two existing functions into the lifecycle. Autorun can schedule them concurrently. The chain 040 → 043 is enforced by the aggregator needing `resolve_path` from 040; 043 → 044 is enforced by compaction-plus-promotion integration tests needing the aggregator. 045 and 042 share no files with the chain and can run whenever.

**DB + autorun (sequential).**
`019 → 020 (with cursor AC) → 021 → 022 → 026 → 027 → 029 → 036`

Each later task depends on the deliverable of the previous (the rebuild-mapping document feeds the validator feeds the three-tier validator feeds the SurrealDB target schemas…), and several share scope under `codeflow-cli/core/src/store/`. Serial enforcement prevents rework loops.

**Audit consumption (G11 resolution).** 033 (coordination-events audit) and 034 (autorun-events audit) are already `complete` on disk. Their outputs should be reviewed ONCE before the schema-enforcement chain begins: if either audit surfaces an event shape not covered by the current canonical schema (007), apply an amendment to 007 first; otherwise the chain proceeds without a 007-amendment step.

## Out-of-Scope Observations

Documented here because they were identified during the audit but do not warrant a task in this cycle:

- **SurrealDB DELETE by string id is broken** — identified during cf-knowledge-layer renumber cleanup; `DELETE task:<string-id>` returns success but the row persists. Relevant to the broader data-integrity work in task 020 / G6. Recorded here as a future tech-debt note; not creating a dedicated task.

## References

- Memory record: `project_inf_epc_024_followon_plan_2026_04_22.md`
- Session origin: `2077a0b3-178a-46f3-91a7-b6c5654651e6`
- Epic: `project-management/epics/INF/INF-EPC-024/INF-EPC-024.md`
- Task drafts: `INF-TSK-024-040.md` through `INF-TSK-024-045.md`
- Prior audits / analyses: `.codeflow/docs/analysis/parallel-work/schema-standardization.md`
- Trigger PR: #303 (`fix/inf-tsk-024-035-pathflow-events-migration`)
