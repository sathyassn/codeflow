---
id: "task-01KP0WRNT0PZEMHNP5VAF83VN2"
format_id: "INF-TSK-046-001"
epic_id: "epic-01KP0WRNT08J88E09QGQ6185BP"
epic_format_id: "INF-EPC-046"
title: "Plan generic testing subsystem — ADR, design doc, epic breakdown, 5 implementation task specs"
description: "PLAN session producing ADR, design/analysis doc, epic markdown, and 5 implementation task specifications for the generic testing subsystem"
status: complete
area_type: "INF"
work_type: "PLAN"
domain: "GENL"
origin: informal
file_scope: []
scope_policy: soft
scope_root: null
estimate: L
priority: high
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "ADR exists at .codeflow/docs/adr/ADR-NNN-generic-testing-subsystem.md with status proposed, problem statement, options considered, decision, and consequences sections"
  - "Design doc exists at .codeflow/docs/analysis/generic-testing-subsystem.md with full TOC, options analysis (why CTRF, why declarative config), usage scenarios for fresh/Node/Go/monorepo/self-host Rust, .state/ enforcement analysis, and PR report format"
  - "Epic markdown exists at project-management/epics/INF/INF-EPC-046/INF-EPC-046.md with summary, scope, acceptance criteria, and task table populated"
  - "Task doc INF-TSK-046-002 populated with XL estimate, full acceptance criteria tracing engine code paths, named artifact files, CLI commands, and test scenarios"
  - "Task doc INF-TSK-046-003 populated with L estimate, full acceptance criteria for wizard/doctor/templates covering fresh-project and migration scenarios"
  - "Task doc INF-TSK-046-004 populated with XL estimate, full acceptance criteria enumerating every Claude artifact file to modify and the specific Rust-specific text to remove"
  - "Task doc INF-TSK-046-005 populated with L estimate, full acceptance criteria for self-host migration verifying zero regressions on existing shell/Python/Rust test suites"
  - "All task docs preserve the invariant: LLMs never directly Edit/Write .state/ or test-config.json (stated explicitly in each task's acceptance criteria)"
tests: []
branch: "plan/generic-testing-subsystem"
pr_number: null
external_id: null
external_url: null
dependencies: []
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-12T13:30:00Z"
started_at: "2026-04-11T00:00:00Z"
completed_at: "2026-04-12T13:30:00Z"
stage: done
stage_status: complete
stage_history: "[]"
---

# INF-TSK-046-001: Plan generic testing subsystem — ADR, design doc, epic breakdown, 5 implementation task specs

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

This PLAN task produces all design artifacts needed to implement the generic testing subsystem (INF-EPC-046). CodeFlow currently hardcodes Rust-specific testing assumptions throughout its CLI, Claude artifacts (agent definitions, CLAUDE.md, task/epic templates), and test-config.json. Downstream adopters on Node/Go/Python/mixed stacks have no path to use `codeflow test` without forking and modifying Rust-specific code.

The output of this task is the full specification that unblocks the 4 implementation tasks (TSK-002 through TSK-005).

## Deliverables

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| ADR | doc | `.codeflow/docs/adr/ADR-NNN-generic-testing-subsystem.md` | Design decision record for future contributors |
| Design/analysis doc | doc | `.codeflow/docs/analysis/generic-testing-subsystem.md` | Reference for TSK-002 through TSK-005 implementers |
| Epic markdown (INF-EPC-046) | doc | `project-management/epics/INF/INF-EPC-046/INF-EPC-046.md` | WorkGraph epic record |
| TSK-002 task doc | doc | `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-002-engine-parsers-ctrf-junit.md` | Spec for engine implementation |
| TSK-003 task doc | doc | `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-003-setup-wizard-doctor-templates.md` | Spec for wizard/doctor |
| TSK-004 task doc | doc | `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-004-claude-artifact-generalization.md` | Spec for artifact generalization |
| TSK-005 task doc | doc | `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-005-self-host-migration.md` | Spec for self-host migration |

**Expected Outcome:** A complete, reviewable design package that allows any engineer (or agent) to pick up TSK-002 and implement the generic testing engine without needing to re-derive requirements.

**Deployment:** PR merge on `plan/generic-testing-subsystem` branch.

## Pre-Work Analysis

- [ ] Reviewed git log for recent changes to files in scope
- [ ] Checked if related tasks/PRs modified shared code
- [ ] Verified acceptance criteria still accurate
- [ ] Updated approach if implementation landscape changed

## Approach

1. Survey current Rust-specific assumptions: scan `codeflow test` CLI source, `test-config.json`, agent definitions, CLAUDE.md test references, task/epic templates
2. Research CTRF (CommonTestResultsFormat) spec and JUnit XML schema for adapter design
3. Define declarative config schema: stack declaration, runner commands, output format, coverage config
4. Write ADR: problem, options (CTRF vs TAP vs custom), decision, consequences
5. Write design/analysis doc: architecture diagram, config schema, usage scenarios, enforcement analysis
6. Populate TSK-002 through TSK-005 task docs with full acceptance criteria

## Standards & Practices

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Markdown | cf-markdown-standards | Lint rules, templates |

## Files

### To Create

- `.codeflow/docs/adr/ADR-NNN-generic-testing-subsystem.md` — Architecture decision record
- `.codeflow/docs/analysis/generic-testing-subsystem.md` — Design and analysis document
- `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-002-engine-parsers-ctrf-junit.md` — populated task doc
- `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-003-setup-wizard-doctor-templates.md` — populated task doc
- `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-004-claude-artifact-generalization.md` — populated task doc
- `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-005-self-host-migration.md` — populated task doc

### To Modify

- `project-management/epics/INF/INF-EPC-046/INF-EPC-046.md` — update task table once specs are finalized

### To Read

- `codeflow-cli/core/src/` — current test subsystem implementation
- `.codeflow/testing/` — existing test suite structure
- `codeflow-cli/config/testing/test-config.json` — current config schema
- `.claude/agents/cf-*.md` — current agent definitions (scan for Rust-specific test text)
- `.claude/CLAUDE.md` — current test references
- `project-management/templates/task-template.md` — template for populating task docs

## Concurrency Considerations

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| project-management/ markdown | write-once new files | N/A — new files only |
| .codeflow/docs/ | write-once new files | N/A — new files only |

## Acceptance Criteria

1. ADR exists at `.codeflow/docs/adr/ADR-NNN-generic-testing-subsystem.md` with status proposed, problem statement, options considered, decision, and consequences sections
2. Design doc exists at `.codeflow/docs/analysis/generic-testing-subsystem.md` with full TOC, options analysis (why CTRF, why declarative config), usage scenarios for fresh/Node/Go/monorepo/self-host Rust, `.state/` enforcement analysis, and PR report format
3. Epic markdown exists at `project-management/epics/INF/INF-EPC-046/INF-EPC-046.md` with summary, scope, acceptance criteria, and task table populated
4. Task doc INF-TSK-046-002 populated with XL estimate, full acceptance criteria tracing engine code paths, named artifact files, CLI commands, and test scenarios
5. Task doc INF-TSK-046-003 populated with L estimate, full acceptance criteria for wizard/doctor/templates covering fresh-project and migration scenarios
6. Task doc INF-TSK-046-004 populated with XL estimate, full acceptance criteria enumerating every Claude artifact file to modify and the specific Rust-specific text to remove
7. Task doc INF-TSK-046-005 populated with L estimate, full acceptance criteria for self-host migration verifying zero regressions on existing shell/Python/Rust test suites
8. All task docs preserve the invariant: LLMs never directly Edit/Write `.state/` or `test-config.json` (stated explicitly in each task's acceptance criteria)

### PII Handling Review

- [ ] Does this task involve code that handles PII? (N)

### Criteria Status

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-PLAN -> WS-REV

| # | Criterion | PLAN | REV | Notes |
|---|-----------|------|-----|-------|
| 1 | ADR exists at correct path with required sections | DONE | PASS | `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md` verified: Status (Proposed), Context (problem + constraints + assumptions), Decision + Rationale, 4 Alternatives w/ decision matrix, Consequences (positive/negative/risks × 10 risks), Implementation action items, References. ADR numbering 001 valid (no prior entries in dir). |
| 2 | Design doc exists with full TOC, options analysis, scenarios, enforcement analysis | DONE | PASS | `.codeflow/docs/analysis/generic-testing-subsystem.md` verified: 19 numbered sections, clickable TOC with back-links, 6 scenarios (A-F), `.state/` enforcement matrix §13 with 16-row need-to-CLI mapping, 8 CLI subsections §14, 10-risk table §18. |
| 3 | Epic markdown populated with summary, scope, criteria, task table | DONE | PASS | `INF-EPC-046.md` verified: summary, in/out scope, 14 ACs spanning full chain, task table (L/XL/L/XL/L estimates), autorun batching, dependencies, related links. |
| 4 | TSK-002 task doc fully populated (XL, engine code paths, artifact names, CLI commands) | DONE | PASS | 46 ACs verified — every CLI subcommand named with signature, every module cited by path (e.g., `codeflow-cli/core/src/testing/report/junit.rs`), CLI Command Surface Matrix appendix present, 15 scenarios cross-referenced to design doc, 4 error-path ACs (#41-44). |
| 5 | TSK-003 task doc fully populated (L, wizard/doctor/templates, fresh + migration scenarios) | DONE | PASS | 50 ACs verified — setup wizard flags, 5-sentinel detection heuristics (#4), 9 templates #9-18 each shape-specified, 9 doctor checks #20-29, init prompt integration #33-35, injectable PromptProvider #41, 16 scenarios table, 3 error-path tests. |
| 6 | TSK-004 task doc fully populated (XL, every artifact file enumerated, Rust text identified) | DONE | PASS | Iteration 2: TSK-004 now has 34 ACs including AC#33 (enforcement-policy.json required_sections update) and AC#34 (gh-pr-guard round-trip test); `enforcement-policy.json` in file_scope + "To Modify". F1 resolved. |
| 7 | TSK-005 task doc fully populated (L, self-host migration, zero regression criteria) | DONE | PASS | Iteration 2: TSK-002 AC#47 covers the protection-policy extension (both arrays × both patterns); TSK-005 AC#15 committed to runner:custom shell-scripts target. F2, F10 resolved. |
| 8 | All task docs explicitly preserve .state/ and test-config.json invariant | DONE | PASS | Iteration 2: design doc §13.1 rewritten to reflect actual policy shape; TSK-002 AC#47 adds explicit acceptance criterion for updating `protected_resources.high[]` AND `worktree_protection.patterns[]` with both `.state/test-reports/**` and `.state/coverage/**`. Invariant now backed by verifiable enforcement entries. F2 resolved. |

## Dependencies

### Blocked By

- None

### Blocks

- INF-TSK-046-002
- INF-TSK-046-003
- INF-TSK-046-004
- INF-TSK-046-005

## Verification

### Automated

- [ ] `codeflow validate task project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-001-plan-generic-testing-subsystem.md`
- [ ] `codeflow validate epic project-management/epics/INF/INF-EPC-046/INF-EPC-046.md`

### Manual

- [ ] Each task doc's acceptance criteria are specific enough for cf-review to issue PASS/FAIL per criterion
- [ ] Design doc covers all 6 usage scenarios (fresh project, Node, Go, monorepo, self-host Rust, migration)
- [ ] ADR documents rejected alternatives with rationale

## Stage Reports

### PLAN Report

> Populated by cf-planning before STAGE-COMPLETE: WS-PLAN

**Design Decisions:**

Key decisions locked in during planning:

1. **Declarative config + CTRF canonical + JUnit adapter + custom escape hatch** chosen over plugin-adapter, task-runner delegation, and lingua-franca-only alternatives. Weighted decision matrix recorded in design doc §5 (4.7/5 vs 3.0–3.3 for alternatives). Industry analogs: pre-commit, commitlint, CI test-reporters.
2. **CTRF-shaped internal canonical model.** JUnit XML is the primary universal input adapter (covers pytest, nextest, jest, vitest, go, mocha, rspec, phpunit). `runner: custom` escape hatch for exotic tools. Rust has no native CTRF reporter — resolved by shipping a first-class JUnit→CTRF converter as an engine feature.
3. **Four well-known coverage formats only**: lcov, cobertura, istanbul-summary, go-cover. Exotic tools pre-transform via `coverage.transform`. Keeps parser surface small and maintainable.
4. **Coverage rule list** over five scopes (per_file, per_package, per_module, changed_files, global); first-match-wins for file-level scopes; all-rules-evaluated for non-file scopes. Supports strict-on-changed-files + lax-on-legacy patterns idiomatically.
5. **Universal mode vocabulary**: `quick | essential | full`. Modes are optional per target — missing mode silently skipped, not an error.
6. **Sequential execution default**; opt-in `execution.parallel: true`. Fail-fast off. No retries (matches user policy).
7. **Per-worktree LOCAL `.state/test-reports/` and `.state/coverage/`** via `WorktreePaths` extensions — concurrent worktree runs never collide.
8. **Canonical `test-config.json` location: `.codeflow/config/testing/test-config.json`** (moved from `codeflow-cli/config/testing/`), protected by the existing `.codeflow/**` ProtectionGuard tier.
9. **`.state/` + `test-config.json` invariant preserved.** A full CLI Command Surface Matrix (design doc §13) proves every LLM need maps to a `codeflow test <subcmd>` — no Edit/Write on protected paths. Every implementation task carries this as an explicit acceptance criterion.
10. **Five-section target-aware PR body**: Overall Test Pass Status, Overall Coverage with per-target Exempted Files, Modified File Coverage, optional Test Failures (when >0), optional Slowest Tests. Fresh-project path emits a single-line notice instead of empty tables.
11. **gh-pr-guard rewritten to parse markdown via pulldown-cmark**, not substring match — fixes the truncation-bypass vulnerability noted in memory (`project_pr_body_truncation_analysis.md`).
12. **Self-host as first adopter**: CodeFlow migrates to its own generic engine via TSK-005 on the same PR sequence. Six existing coverage exceptions migrate 1:1; parity check against the old engine's PR-body output is a hard gate; legacy path deleted on cutover (no feature-flag retention).

**Key findings from code survey:**

- Current `codeflow test` CLI is a 372-line routing layer (`codeflow-cli/cli/src/cmd/test.rs`) delegating to `TestValidator::full_validate` (Rust-only) or a shell runner fallback.
- Current `TestValidator` (`codeflow-cli/core/src/testing/validation.rs`, 1750 lines) hardcodes `cargo test`, `cargo llvm-cov`, and `codeflow-core`/`codeflow-cli` crate names.
- Current `codeflow-cli/config/testing/test-config.json` has 6 coverage exceptions with detailed reason + remove_when text that must be preserved 1:1 in migration.
- `WorktreePaths` at `codeflow-cli/core/src/worktree/paths.rs` is the single-source path resolver; already handles LOCAL vs symlinked distinction (ledger, sentinels are LOCAL per PR #221) — extending with `test_reports_dir()`/`coverage_dir()` is additive and aligned with existing pattern.
- Claude artifacts contain **99 Rust-specific references in instructional text** across 17 files (full inventory in TSK-004 acceptance); heaviest in cf-quality-assurance.md (20), CLAUDE.md (26), cf-development.md (14).
- `.codeflow/docs/adr/` directory did not exist; created for this ADR. Future ADRs follow the `ADR-NNN-<slug>.md` pattern.
- Existing analysis docs at `.codeflow/docs/analysis/` use plain filenames without prefix — matched by `generic-testing-subsystem.md`.
- `codeflow validate epic` and `codeflow validate task` both confirm deliverable correctness (epic OK; tasks WARN on filename length only — pre-existing skeleton naming, not introduced by this plan).

**Deliverables:**

| File | Type | Description |
|------|------|-------------|
| `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md` | ADR | Decision record — 4 alternatives considered, weighted matrix, consequences + risks + mitigations, implementation action items |
| `.codeflow/docs/analysis/generic-testing-subsystem.md` | design doc | 19-section design/analysis — problem, goals, alternatives, chosen approach, wire formats, schema, coverage model, modes, PR format, 6 usage scenarios, `.state/` enforcement matrix, CLI surface, init/setup flow, worktree parallelism, migration plan, risks, references |
| `project-management/epics/INF/INF-EPC-046/INF-EPC-046.md` | epic | Updated — task table with estimates, 14 chain-coverage acceptance criteria, autorun batching, ADR/design-doc links |
| `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-002-*.md` | task | 46 acceptance criteria — engine, 4 coverage parsers, JUnit parser + converter, CTRF native reader, config loader + JSON schema, threshold engine, target runner, PR-body emitter, CLI surface, CLI Command Surface Matrix appendix |
| `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-003-*.md` | task | 50 acceptance criteria — setup wizard (interactive + auto + template + list + add-target), 9 templates, 9 doctor checks, init prompt, injectable PromptProvider, integration tests |
| `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-004-*.md` | task | 32 acceptance criteria — CLAUDE.md + 8 agent defs + 5 commands + 2 skills + 2 templates + gh-pr-guard rewrite; per-file Rust-reference inventory; post-migration grep invariant |
| `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-005-*.md` | task | 27 acceptance criteria — self-host migration, 1:1 exception preservation, parity check, legacy path removal, `.gitignore`, 17-step cutover blueprint, rollback plan |

**Deviations from Approach:**

- None in iteration 1. All planned deliverables produced. The original intent ("capture every detail of the acceptance criteria — every link of the chain tracing code paths of scenarios considered/relevant") is met: post-iteration 2, task docs contain 50/50/34/27 acceptance criteria respectively, each traced to file paths, function signatures, CLI invocations, and scenarios.
- The ADR chose `ADR-001` — verified correct: `.codeflow/docs/adr/` had no prior entries at the time this plan was authored (directory was created in this PR).
- TSK-005's rollback mechanism is a clean `git revert` strategy — no `CODEFLOW_TEST_ENGINE=legacy` feature flag. This decision is now consistently recorded in TSK-005 AC#16, design doc §17.2, and ADR-001 §Consequences (post iteration-2 reconciliation).

**Revision History:**

| Iteration | Trigger | Findings addressed | Changes made |
|-----------|---------|--------------------|--------------|
| 1 → 2 | WS-REV feedback (confidence 82; 4 MAJOR + 6 MINOR + 2 NOTE) | F1 PR-body heading drift | design doc §11.1 explicit reconciliation text + §11.4 output-contract table (Markdown vs JSON consumer routing); TSK-004 AC#33 requiring `enforcement-policy.json` `required_sections` update to the canonical `## Test Results` array; TSK-004 AC#34 round-trip test for gh-pr-guard; enforcement-policy.json added to TSK-004 `file_scope` + "To Modify" list |
| | | F2 protection-scope claim wrong | design doc §13.1 rewritten to reflect actual policy shape (specific subdirs, no `.state/**` wildcard); new TSK-002 AC#47 requiring additions to BOTH `protected_resources.high[]` AND `worktree_protection.patterns[]`; cross-reference to memory note `project_worktree_protection_fix.md` |
| | | F3 schema evolution undefined | design doc §8.5 new subsection with 5-row behavior table; TSK-002 AC#48 with per-behavior unit tests |
| | | F4 routing ambiguity | design doc §9.5 rewritten with 6-rule ordered algorithm + extension-affinity guard + tie-break rules + examples table; TSK-002 AC#49 requiring tests for every §9.5 example row |
| | | F5 speculative ADR-001 deduction | Confidence rationale below corrected — removed the "without sequence consultation" qualifier; number verified correct |
| | | F6 rollback inconsistency | design doc §17.2 rewritten; ADR-001 §Consequences #Risks added Migration-rollback entry aligning; TSK-005 AC#16 rewritten referencing both |
| | | F7 TSK-002 validation.rs ambiguity | TSK-002 "Files > To Modify" bullet rewritten: rename to `legacy_validation.rs` as a TRANSIENT stub, TSK-005 removes |
| | | F8 engine output contract ambiguity | design doc §11.4 "Authoring policy and output contract" subsection added with Markdown vs JSON consumer table; TSK-002 AC#46 updated — JSON is for programmatic consumers, Markdown (AC#36) is for PR body |
| | | F9 template round-trip weak | TSK-003 AC#39 strengthened to byte-equivalent round-trip (struct-equal is insufficient; byte-equal catches config-writer formatting drift) |
| | | F10 shell/Python disposition undecided | TSK-005 Approach §7 rewritten to commit: keep `.codeflow/testing/run-all-tests.sh` + `lib/` intact; declare `shell-scripts` target with `runner: custom`; extend `run-all-tests.sh` with `--format=ctrf --output=<path>`; AC#15 rewritten; `file_scope` updated |
| | | F11 coverage.transform non-zero | TSK-002 AC#50 added: non-zero exit from `coverage.transform` → warn + degrade that target's coverage to N/A, does not fail run; unit test covers |
| | | F12 doctor probe timeout | TSK-003 AC#29 rewritten to include 5-second per-probe timeout and explicit timeout-vs-non-zero-exit warning paths; unit tests cover both |
| 2 → 3 | WS-REV feedback (confidence 92; 1 MAJOR residual + 1 NOTE regression) | F6 residual at design doc §18 Risks row #10 | Rewrote row #10 mitigation text to drop "`CODEFLOW_TEST_ENGINE=legacy` env flag for one release, then removed" and replace with "direct deletion of the legacy path in the same PR as the cutover (no feature flag). Rollback is a `git revert` of the TSK-005 merge commit. Aligned with §17.2 and ADR-001 §Consequences." |
| | | NF-1 TSK-002 line 158 (Approach §1) | Removed "retaining a thin `LegacyValidator` behind a feature flag"; replaced with "`validation.rs` becomes a transient deprecation stub (renamed `legacy_validation.rs`) removed by TSK-005 — no feature flag" |
| | | NF-1 TSK-002 line 210 ("To Modify" mod.rs bullet) | Removed "replace or gate existing `validation` behind legacy flag"; replaced with "replace existing `validation` export with `legacy_validation` (the transient deprecation stub, removed by TSK-005) — no feature flag" |

### REV Report

> Populated by cf-review before STAGE-COMPLETE: WS-REV

**Review Mode:** DESIGN_REVIEW
**Verdict:** APPROVED (iteration 3 final)
**Reviewer iterations:** 3

#### Dimensional Assessment

| Dimension | Verdict | Key Evidence |
|-----------|---------|-------------|
| Functional Correctness | PASS | ADR, design doc, epic, 4 task docs all exist at specified paths with the content described. `codeflow validate` epic passes. |
| Security | PARTIAL | Design correctly flags LLM direct-write temptations and maps every one to a CLI subcommand via §13 need-to-CLI matrix. TSK-002 SEC note flags shell injection via `set-command`. HOWEVER, the design's protection claim ("`.state/test-reports/**` inherits .state/**") does NOT match actual `.codeflow/config/enforcement/enforcement-policy.json` which lists specific `.state/<subdir>/**` entries with no catch-all. See F2. |
| Standards Compliance | PASS | Markdown structure of all 8 artifacts matches `project-management/templates/task-template.md`. Pipeline headings (DEV/SEC/REV/QA) correct per work-type per CLAUDE.md §5. ADR follows a standard proposed/context/decision/consequences layout. |
| PII Check | PASS | No PII in any artifact; epic + each task doc answers `(N)` to the PII handling review checkbox. |
| Scope Compliance | PASS | Changeset touches only planning artifacts: ADR, design doc, epic, 4 task doc specs, and the primary task doc (this file). No code, config, or agent-def edits — consistent with PLAN work type. |
| Problem Statement | PASS | ADR §Problem Statement and design doc §3 independently and concretely state the coupling (CLI hardcodes, config hardcodes, Claude artifact hardcodes, hook hardcodes) with grep-evidence table. |
| Architecture Soundness | PARTIAL | Core architecture (declarative config + CTRF-canonical-internal + JUnit adapter + custom escape hatch) is sound and industry-analog-supported (pre-commit, commitlint, CI test-reporters). Coverage rule-list model (5 scopes, first-match-wins for file-level) is well specified with worked examples. However, F1 (PR body naming mismatch `## Test Stats` vs `## Test Results`), F3 (schema_version/migrate path under-specified), and F5 (cwd-prefix routing ambiguity on nested targets) create implementation-time rework risk. |
| Trade-off Analysis | PASS | ADR documents 4 alternatives each with pros/cons/why-rejected. Design doc §5 adds a weighted decision matrix (4.7 vs 3.0-3.3). Risks table (§18, 10 rows) names likelihood × impact × mitigation. |

#### Findings Log

| # | Severity | Finding | File:Line | Iteration | Resolution |
|---|----------|---------|-----------|-----------|------------|
| 1 | MAJOR | **PR-body section-name mismatch.** Design doc §11.1 and TSK-004 AC#20 specify `## Test Results` with sub-sections `### 1. Overall Test Pass Status`, `### 2. Overall Coverage`, `### 3. Modified File Coverage`. However, the current `enforcement-policy.json` at `.codeflow/config/enforcement/enforcement-policy.json:139` requires `## Test Stats` plus those same sub-sections. The design silently changes the top-level heading without adding an acceptance criterion to update `enforcement-policy.json` `git_format.pr.required_sections`. After TSK-004 lands, `gh-pr-guard` will parse markdown looking for `## Test Results`, but the policy still declares `## Test Stats` as required. Result: ambiguity — one or the other wins, silently. CLAUDE.md §7 PR Workflow also currently uses `## Test Results` / `### 1. Overall Test Pass Status` etc., so a rewrite is needed regardless, but the policy JSON must be updated too. | `.codeflow/config/enforcement/enforcement-policy.json:139`; TSK-004 task doc AC list; design doc §11.1 | 1 | OPEN — add acceptance criterion to TSK-004 requiring `enforcement-policy.json` `pr.required_sections` update; clarify in design doc §11.4 that the canonical heading is `## Test Results` and policy JSON MUST be updated. |
| 2 | MAJOR | **Protection scope gap for `.state/test-reports/` and `.state/coverage/`.** Design doc §13.1 states these paths "inherit .state/**" via ProtectionGuard (high tier). Ground truth at `.codeflow/config/enforcement/enforcement-policy.json:71-92`: the `high` tier lists specific `.state/<subdir>/**` entries (sentinels, session, runtime, ledger, db, logs, coordination, worktrees) — there is NO `.state/**` catch-all. Similarly `worktree_protection.patterns` at `:97-121` enumerates specific subdirs, not `.state/**`. TSK-002/TSK-005 MUST add `.state/test-reports/**` and `.state/coverage/**` to both lists, or LLMs can Edit/Write these paths directly, defeating the invariant. This is also flagged in memory (`project_worktree_protection_fix.md`). TSK-005 AC#18 says ProtectionGuard must block — but without adding the entries, the guard won't block. | `.codeflow/config/enforcement/enforcement-policy.json:71-92, 95-123`; design doc §13.1; TSK-002 AC list; TSK-005 AC#18 | 1 | OPEN — add explicit acceptance criterion to TSK-002 (or TSK-005) to add `.state/test-reports/**` and `.state/coverage/**` to both `protected_resources.high` and `worktree_protection.patterns`. Correct design doc §13.1 to remove the "inherits" claim and state the explicit entries as required additions. |
| 3 | MAJOR | **Schema evolution path under-specified.** Design doc §8.1 introduces `schema_version: "1.0"`. ADR §Risks mentions a future `codeflow test config migrate` command for schema evolution. TSK-002 AC#5 checks `schema_version` is parsed but neither TSK-002 nor TSK-003 has an acceptance criterion for: (a) what happens when the config declares a `schema_version` the engine does not know, (b) whether unknown fields are silently ignored or rejected, (c) forward-compat rules for adopters who pin `1.0` then upgrade. This is a semver contract that adopters will lock into — leaving the behavior unspecified creates a breaking-change footgun. | Design doc §8.1, §18 Risk #3; TSK-002 AC#5, #43; TSK-003 doctor check #2 | 1 | OPEN — add acceptance criterion to TSK-002 that unknown `schema_version` fails with a clear error listing supported versions; add criterion that unknown top-level fields produce a warning (lenient) vs unknown required-section fields fail (strict). Document the decision in design doc §8. |
| 4 | MAJOR | **Modified-file-coverage routing ambiguity for nested targets.** Design doc §9.5 and TSK-002 AC#38 specify "longest matching path prefix" routing. But the algorithm is underspecified for nested cwd cases. Example: monorepo with targets `cwd: "services/"` and `cwd: "services/api/"`. A file at `services/api/src/handler.rs` matches both; longest-prefix picks `services/api/`. But `services/common/util.go` matches only `services/`. That's fine. What about a file at `services/api/docs/README.md` — is it "source" and included in changed-files? §9.5 doesn't say. More critically: what if a target has `cwd: ""` or `cwd: "."` (repo root)? Is that the catch-all? What if two targets both declare `cwd: "."` (monorepo with overlapping test scopes)? The "unattributed" row (TSK-002 AC#38 "fail-closed") is a reasonable fallback, but the selection algorithm must be explicit. | Design doc §9.5; TSK-002 AC#38 | 1 | OPEN — specify in design doc §9.5: (a) behavior when target cwd is `.` or empty (counts as longest-prefix match of length 0; ties broken by declaration order); (b) whether `changed_files` scope filters by file extension (e.g., a target.runner=cargo should not pick up .ts files); (c) specify resolution when two targets have identical cwd. Add ACs to TSK-002. |
| 5 | MAJOR | **Confidence score deduction item (a) is a verifiable check cf-planning skipped.** cf-planning's own confidence rationale says "ADR numbering chose 001 without consulting a running sequence". I verified this independently: `.codeflow/docs/adr/` contains only `ADR-001-generic-testing-subsystem.md` (no prior entries; directory created by this task). So 001 is correct — but cf-planning counted it as a deduction. This is not a design flaw; it is a self-review quality issue: the deduction was speculative, not evidence-based. Confidence of 96 is fine, but the rationale must be accurate. | Confidence Score table row 1; `.codeflow/docs/adr/` directory | 1 | OPEN — correct the WS-PLAN rationale to drop deduction (a). This is a low-stakes correction but affects review confidence calibration. |
| 6 | MINOR | **TSK-005 AC#16 contradicts ADR §Consequences/Negative and §17.2.** TSK-005 AC#16 says "Rollback mechanism: the legacy path is NOT retained behind a feature flag". Design doc §17.2 says legacy is kept behind `CODEFLOW_TEST_ENGINE=legacy` for one full release cycle. ADR §Consequences/Negative §Risks #10 says "behind `CODEFLOW_TEST_ENGINE=legacy` env flag for one release, then removed". PLAN Report §10 notes this as a "simplified" Tier-2 refinement but the ADR and design doc are not updated to match. A reviewer reading the design doc and ADR will expect the flag; a reviewer reading TSK-005 will not. Pick one and align. | TSK-005 AC#16; design doc §17.2; ADR §Consequences/Risks #10 | 1 | OPEN — update design doc §17.2 and ADR §Risks #10 to match the clean-cutover decision (or vice versa). Document the decision transition in the ADR "Changes" section. |
| 7 | MINOR | **TSK-002 §"To Modify" retains internal contradiction on legacy path.** TSK-002 lists two approaches for the legacy path — rename to `legacy_validation.rs` behind cfg OR stub out with `disabled_error()` — and defers the choice. Then TSK-005 AC#10 says "legacy path is REMOVED entirely". A deferred decision in TSK-002 that TSK-005 then overrides is low-harm but muddles the commitment: TSK-002 should either drop the stub/rename route or state that the stub is a transient state that TSK-005 cleans up. As written, a reviewer reading TSK-002 alone will not know the stub is transient. | TSK-002 §Files > To Modify (validation.rs bullet); TSK-005 AC#10 | 1 | OPEN — change TSK-002 bullet to "rename to `legacy_validation.rs` as a TRANSIENT stub, removed by TSK-005". |
| 8 | MINOR | **Design doc §11.4 "verbatim" policy is strict but TSK-002 AC#36/46 allow `--format json` transform.** §11.4 says "populated verbatim by `codeflow test --mode full --coverage` structured output". TSK-002 AC#46 says `--format json` is "consumed by cf-git-operations PR-body rendering". These are consistent only if cf-git-operations is a dumb serializer — but §11.4 also says "cf-git-operations re-runs `codeflow test` and replaces the Test Results section". Clarify: does the engine emit the Markdown directly (verbatim), or does it emit JSON that cf-git-operations renders into Markdown? Both are defensible, but consumers need to know which. | Design doc §11.4; TSK-002 AC#36, #46 | 1 | OPEN — state explicitly in §11.4: the engine emits BOTH a verbatim Markdown block (for cf-git-operations to paste into the PR body) AND a structured JSON (for ledger recording and programmatic consumers). cf-git-operations uses the Markdown block directly, not the JSON. |
| 9 | MINOR | **TSK-003 template round-trip test is tautological as written.** TSK-003 AC#39 says "every template in the library has a round-trip test (load via schema → write via config-writer → load again → compare structs for equality)". If the config-writer is deterministic (TSK-002 AC#33) and the load is serde-derived, this test trivially passes by construction unless the template is malformed — in which case TSK-003 AC#9 (schema validation) catches it first. The round-trip test adds real value ONLY if it asserts the written output byte-equals the original template file (not just struct-equal). Strengthen the AC. | TSK-003 AC#39 | 1 | OPEN — strengthen TSK-003 AC#39 to require BYTE-equivalent round-trip (written output == original template file) — catches config-writer drift like formatting changes. |
| 10 | MINOR | **TSK-005 "shell/Python suites" disposition is unspecified.** TSK-005 AC#15 says "Shell and Python test suites (if still present under `.codeflow/testing/`) continue to run; either they are declared as additional targets … OR they are invoked directly from the engine's target runner; in either case, regression count is zero". Leaving the choice open is reasonable for an implementation-time decision but creates two materially different final states — one where `.codeflow/testing/run-all-tests.sh` stays and is wrapped via `runner: custom`, one where it's deleted and shell tests become Rust-harness orchestrated. These produce different diffs in TSK-005's PR. Pre-decide, or split into two acceptance options with explicit criteria. | TSK-005 AC#15; Scenario F design doc §12.6 | 1 | OPEN — in TSK-005 Approach §7 or a new AC, commit to the specific disposition (runner:custom target wrapping run-all-tests.sh is the likely path). Document what stays, what goes. |
| 11 | NOTE | **Coverage.transform step doesn't have a named rejection behavior for non-zero exit.** TSK-002 AC#42 says missing coverage artifact "emits a warning in PR body … does not fail the run". But what if `coverage.transform` runs and exits non-zero? §7.5 says exotic tools "pre-transform" — what happens if the transform fails? Missing ACs. | Design doc §7.5; TSK-002 AC list | 1 | OPEN — add AC to TSK-002 specifying transform non-zero exit behavior (warn + degrade coverage to N/A, OR fail the target entirely — likely the former). |
| 12 | NOTE | **Doctor check 4 "dry-run probe" is named but behavior is hopeful.** TSK-003 AC#29 says the probe runs `cargo --version` (etc.) and the failure is a warning. But it doesn't say whether the probe happens synchronously (slows doctor), or in parallel, or with a timeout. Docker-wrapped commands (a stated exemption) will silently hang the probe. | TSK-003 AC#29 | 1 | OPEN — add timeout (e.g., 5s) to doctor probe in AC#29; document that hang-on-probe is treated as warning. |

#### Rework History

| Iteration | Trigger | Changes Requested | Changes Made | Re-review Verdict |
|-----------|---------|-------------------|-------------|-------------------|
| 1 | Initial review | 12 findings (4 MAJOR, 6 MINOR, 2 NOTE) | N/A (pending rework) | CHANGES_REQUESTED |
| 2 | Iteration 2 re-review (commit `13f06b15`) | F1-F12 all addressed by cf-planning | Verified each fix independently — see Iteration 2 Re-Review below | CHANGES_REQUESTED (iteration 3 requested for 1 residual MAJOR + 1 NEW-FINDING NOTE) |

### Iteration 2 Re-Review

> Targeted re-audit of the 12 F-numbered fixes from iteration 1.

#### Per-finding verification

| F# | Verdict | Evidence |
|----|---------|----------|
| F1 | RESOLVED | design doc §11.1 (line 470) explicitly names `## Test Results` as canonical and calls out `enforcement-policy.json` update requirement with the new required_sections array. TSK-004 AC#33 adds `enforcement-policy.json` to file_scope + "To Modify"; AC#34 adds round-trip test (new format passes, legacy `## Test Stats` rejected with clear message). |
| F2 | RESOLVED | design doc §13.1 rewritten: the "inherits .state/**" claim is gone, replaced with "Must be added to protected_resources.high[] AND worktree_protection.patterns[]" (lines 773-774). TSK-002 AC#47 explicitly requires BOTH arrays updated with BOTH patterns; memory note `project_worktree_protection_fix.md` referenced. |
| F3 | RESOLVED | design doc §8.5 new subsection (lines 326-342) with the 5-row behavior table (supported/missing/unknown-version/unknown-top-level-warn/unknown-in-required-fail) plus an additional row for unknown enum. TSK-002 AC#48 requires per-behavior unit tests. |
| F4 | RESOLVED | design doc §9.5 rewritten (lines 406-427) with 6 ordered rules: normalize cwd, extension-affinity filter (cargo→.rs, go→.go only), longest-prefix, tie-break by declaration order, empty-cwd catch-all LAST, no-match→Unattributed FAIL. Examples table with 5 rows. TSK-002 AC#49 requires tests for every example row. |
| F5 | RESOLVED | WS-PLAN rationale (line 302) drops the speculative ADR deduction; explicitly states "verified correct via `ls .codeflow/docs/adr/`". |
| F6 | **PARTIAL** | design doc §17.2 (lines 1055-1065) AND ADR-001 §Risks (line 163) AND TSK-005 AC#16 are aligned on clean cutover with `git revert` rollback. **HOWEVER**, design doc §18 Risks table row #10 (line 1099) STILL says: "TSK-005 acceptance includes deletion of the legacy path (behind `CODEFLOW_TEST_ENGINE=legacy` env flag for one release, then removed)". This is the same contradiction F6 flagged — iteration 2 reconciled §17.2 but missed the §18 risk-table row. One-cell fix required. |
| F7 | RESOLVED | TSK-002 "Files > To Modify" bullet for `validation.rs` rewritten (line 211) to "rename to `legacy_validation.rs` as a **transient stub**" with deprecation shim code, "TSK-005 removes `legacy_validation.rs` entirely", explicitly forbidding feature flags. Cross-references AC#16 + ADR. |
| F8 | RESOLVED | design doc §11.4 "Authoring policy and output contract" subsection added with Markdown-vs-JSON consumer routing table (line 558-559): Markdown block = verbatim paste by cf-git-operations under `## Test Results`; structured JSON = ledger + programmatic consumers. Both emitted by the engine. |
| F9 | RESOLVED | TSK-003 AC#39 strengthened to byte-equivalent round-trip (line 241): "read template file as bytes → load via schema → write via config-writer → re-read bytes → assert byte-equal". Explicit "struct-equal is insufficient" reasoning included. |
| F10 | RESOLVED | TSK-005 AC#15 rewritten (line 46): commits to `shell-scripts` target with `runner: custom`, keeps `run-all-tests.sh` + `lib/` + shell/Python test files intact, extends `run-all-tests.sh` with `--format=ctrf --output=<path>` flags. `file_scope` updated to include `.codeflow/testing/run-all-tests.sh`. |
| F11 | RESOLVED | TSK-002 AC#50 (line 288) added: `coverage.transform` non-zero exit → degrade target's coverage to "N/A — coverage.transform failed with exit code N" + stderr warning + does NOT fail run. Unit test required. |
| F12 | RESOLVED | TSK-003 AC#29 rewritten (line 231) with 5-second per-probe timeout; both timeout AND non-zero exit produce warnings (not errors); specific warning-message text included ("probe for runner=X exceeded 5s timeout; not blocking"); unit tests cover both paths. |

#### New findings (iteration 2 regression scan)

| NF# | Severity | Finding | File:Line | Resolution |
|-----|----------|---------|-----------|------------|
| NF-1 | NOTE | **TSK-002 Approach §1 retains stale feature-flag phrasing.** Line 158 says "Replace the existing single-file `validation.rs` with the new architecture, retaining a thin `LegacyValidator` behind a feature flag." This conflicts with the F7-resolved "To Modify" bullet (line 211) which commits to a transient stub (NOT a feature flag) removed by TSK-005. Low harm — the binding contract is the Acceptance Criteria + "To Modify", not the Approach section — but still inconsistent. Recommend rewriting line 158 to "Replace the existing single-file `validation.rs` with the new architecture; `validation.rs` becomes a transient deprecation stub (renamed `legacy_validation.rs`), removed by TSK-005 — no feature flag." Same family as F6's §18-row residual; the clean-cutover decision needs one more consistency pass. | TSK-002 line 158, line 210 ("To Modify" mod.rs bullet which says "gate existing validation behind legacy flag") | OPEN — clean up both lines in iteration 3 alongside the F6 §18-row fix. |

#### Iteration 2 overall

**10 of 12 fixes are complete and verifiably correct.** F1, F2 (the most load-bearing), F3, F4 are all solid — the four MAJOR findings from iteration 1 are materially addressed. F6 is 80% resolved (three of four call-sites reconciled) but keeps one residual contradiction (design doc §18 row #10) that re-exposes the clean-cutover-vs-feature-flag ambiguity to any reader who opens the risks table. NF-1 is a same-family residual in TSK-002 Approach §1 and the mod.rs "To Modify" bullet.

**Remaining work is trivial: 3 line-edits total** (design doc:1099; TSK-002:158; TSK-002:210). No new design work, no acceptance-criterion changes. Iteration 3 should close in one precise edit pass.

**Confidence impact:** 10/12 fixes + 4/4 MAJOR fully resolved = strong iteration-2 pass. The residual F6 slice is MINOR (contradicts established design but doesn't block implementation — TSK-005 AC#16 is authoritative). NF-1 is NOTE. Score: 92 — still below the 95 gate, but close; the one-pass cleanup is low-risk.

### Iteration 3 Final Verification

> Commit `2fb7339c` — 3 targeted line-edits verified, final consistency grep run, 1 residual self-patched by reviewer.

#### Per-edit verification

| Edit | Location | Verdict | Evidence |
|------|----------|---------|----------|
| 1 | design doc §18 Risks row #10 (line 1099) | RESOLVED | Now reads: "TSK-005 acceptance includes direct deletion of the legacy path in the same PR as the cutover (no feature flag). Rollback is a `git revert` of the TSK-005 merge commit. Aligned with §17.2 and ADR-001 §Consequences." F6 residual closed. |
| 2 | TSK-002 Approach §1 (line 158) | RESOLVED | Now reads: "`validation.rs` becomes a transient deprecation stub (renamed `legacy_validation.rs`) removed by TSK-005 — no feature flag." NF-1 part 1 closed. |
| 3 | TSK-002 "To Modify" mod.rs bullet (line 210) | RESOLVED | Now reads: "export new submodules; replace existing `validation` export with `legacy_validation` (the transient deprecation stub, removed by TSK-005) — no feature flag". NF-1 part 2 closed. |

#### Additional residual found and self-patched

| NF# | Location | Action taken |
|-----|----------|--------------|
| NF-2 | TSK-002 **Deployment** block (line 147) | Stale text "Old `TestValidator::full_validate` path retained behind `CODEFLOW_TEST_ENGINE=legacy` env flag for one release cycle, then removed by TSK-005" found during the consistency grep. Same family as F6/NF-1. Iteration 3 missed this specific call-site because the team-lead scope only named lines 158 and 210. Per team-lead's instruction ("if under 1-2 trivial lines, patch directly and accept"), reviewer rewrote line 147 in-place to: "Old `TestValidator::full_validate` path becomes a transient deprecation stub (renamed `legacy_validation.rs`) in this PR and is removed by TSK-005 — no feature flag, clean cutover per TSK-005 AC#16, design doc §17.2, and ADR-001 §Consequences." Self-patched in the same session as this review verdict. |

#### Consistency grep result

Sanity grep (`CODEFLOW_TEST_ENGINE` / `feature flag` / `behind legacy flag` across design doc + ADR + epic + task docs) post-patch: remaining hits are all in documented-rejection context (explicitly calling out the rejected alternative) OR in this REV Report's own revision history. No stale commitment to feature-flag retention remains anywhere.

**Clean-cutover decision now consistent across 7 call-sites:**

| # | Location | Status |
|---|----------|--------|
| 1 | design doc §17.2 | aligned (iter 2) |
| 2 | design doc §18 Risks row #10 | aligned (iter 3 Edit 1) |
| 3 | ADR-001 §Consequences Risks | aligned (iter 2) |
| 4 | TSK-005 AC#16 | aligned (iter 2) |
| 5 | TSK-002 Approach §1 | aligned (iter 3 Edit 2) |
| 6 | TSK-002 "To Modify" mod.rs + validation.rs bullets | aligned (iter 3 Edit 3) |
| 7 | **TSK-002 Deployment block** | aligned (iter 3 NF-2 reviewer self-patch) |

#### Rework History update

| Iteration | Trigger | Changes Requested | Changes Made | Re-review Verdict |
|-----------|---------|-------------------|-------------|-------------------|
| 3 | Iteration 3 re-review (commit `2fb7339c`) | F6 residual + NF-1 | All 3 edits verified RESOLVED; 1 additional same-family residual (NF-2 at TSK-002:147) self-patched by reviewer | APPROVED |

#### Final verdict

All 12 iteration-1 findings (F1-F12) RESOLVED. Both iteration-2 residuals (F6 residual + NF-1) RESOLVED in iteration 3. NF-2 found during iter-3 grep and self-patched. No MAJOR / MINOR / NOTE findings remain. The plan is approved.

**Verdict:** APPROVED

### Confidence Score

| Stage | Agent | Score (0-100) | Rationale |
|-------|-------|--------------|-----------|
| WS-PLAN | cf-planning | 98 | Iteration 3 (final; iteration 2 residuals resolved). Three precise line edits applied: design doc §18 Risks row #10 mitigation text rewritten to remove the stale `CODEFLOW_TEST_ENGINE=legacy` feature-flag reference (F6 residual); TSK-002 Approach §1 line 158 rewritten to remove "retaining a thin `LegacyValidator` behind a feature flag" (NF-1 regression); TSK-002 "To Modify" mod.rs bullet line 210 rewritten to remove "gate existing `validation` behind legacy flag" (NF-1 regression). Clean-cutover decision now consistently stated in all four call-sites: design doc §17.2, design doc §18 row #10, ADR-001 §Consequences #Risks, TSK-005 AC#16, TSK-002 Approach §1, TSK-002 "To Modify" bullets (mod.rs + validation.rs). `codeflow validate` passes (epic OK; tasks WARN-only on filename length — pre-existing). Remaining deductions: (a) TSK-005 parity check depends on a golden fixture captured during cutover — verifiable only at TSK-005 runtime; (b) cross-platform shell compatibility (POSIX vs cmd.exe) documented as v1 limitation, not resolved in plan — both acceptable noted constraints, not plan gaps. |
| WS-REV | cf-review | 82 | Iteration 1. Design core is strong: architecture is sound, ADR & design doc are thorough and well cross-referenced, task ACs (46+50+32+27 = 155) are exhaustive and trace code paths. However, 4 MAJOR findings materially threaten implementation correctness: (F1) PR-body section naming mismatch between design doc (`## Test Results`) and enforcement-policy.json (`## Test Stats`); (F2) `.state/test-reports/**` and `.state/coverage/**` NOT in actual ProtectionGuard or worktree_protection patterns — invariant claimed but not enforced; (F3) schema_version evolution policy undefined; (F4) modified-file routing algorithm underspecified for nested/overlapping cwds. 3 MINOR contradictions (F6-F8) need reconciliation. Below 95 — rework required. |
| WS-REV (iter 2) | cf-review | 92 | Iteration 2 verification. 10/12 fixes fully resolved. All four iteration-1 MAJOR findings (F1, F2, F3, F4) are materially addressed: F1 via design doc §11.1 + TSK-004 AC#33/#34 + enforcement-policy.json in scope; F2 via design doc §13.1 rewrite + TSK-002 AC#47 requiring both arrays × both patterns; F3 via design doc §8.5 5-row behavior table + TSK-002 AC#48; F4 via design doc §9.5 6-rule algorithm + TSK-002 AC#49. MINOR/NOTE fixes F5, F7, F8, F9, F10, F11, F12 all verified. **F6 PARTIAL: 3 of 4 call-sites reconciled (design §17.2, ADR Risks, TSK-005 AC#16) but design doc §18 Risks table row #10 line 1099 still reads "behind `CODEFLOW_TEST_ENGINE=legacy` env flag" — contradicts the clean cutover decision.** NEW NF-1: TSK-002 line 158 (Approach §1) + line 210 ("To Modify" mod.rs bullet) retain stale feature-flag phrasing. Remaining work: 3 line-edits in 2 files. Confidence below 95; iteration 3 approved for a single precise cleanup pass. |
| WS-REV (iter 3) | cf-review | 97 | Iteration 3 final verification. All 3 iteration-3 edits verified correct (design doc §18 row #10, TSK-002 Approach §1, TSK-002 "To Modify" mod.rs bullet). F6 residual and NF-1 both RESOLVED. **One additional same-family residual (NF-2) found in TSK-002 Deployment block line 147** — reviewer self-patched per team-lead instruction ("if under 1-2 trivial lines, patch directly and accept"). Clean-cutover decision now consistent across all 7 call-sites. Consistency grep confirms no stale commitment to feature-flag retention anywhere in design doc + ADR + epic + task docs. All 12 iteration-1 findings (F1-F12), both iteration-2 residuals (F6, NF-1), and iteration-3-discovered NF-2 are RESOLVED. Confidence ≥95 gate met. Remaining deductions: inherited from WS-PLAN — (a) TSK-005 parity check fixture only verifiable at TSK-005 runtime, (b) cross-platform shell compatibility documented as v1 limitation. Both are acceptable noted constraints, not plan gaps. **APPROVED.** |

## Notes

This is a PLAN work type task. No code is produced. All deliverables are markdown documents. The `.state/` invariant must be preserved: implementation task docs must explicitly state that test-config.json and `.state/` files are never directly edited by LLMs — all mutations go through `codeflow` CLI commands.
