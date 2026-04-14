---
id: "epic-01KP0WRNT08J88E09QGQ6185BP"
format_id: "INF-EPC-046"
title: "Generic Testing Subsystem — decouple CodeFlow testing from Rust self-host"
summary: "Generalize CodeFlow testing subsystem so downstream adopters on any stack (Node/Go/Python/Rust/mixed monorepos) can configure it via declarative config without per-stack code. Canonical internal model: CTRF-shaped with JUnit input adapter. Strip Rust-specific assumptions from Claude artifacts, the codeflow test CLI, and test-config.json. Migrate CodeFlow self-host as first adopter."
status: in_progress
area_type: "INF"
work_type: "PLAN"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-11T00:00:00Z"
---

# INF-EPC-046: Generic Testing Subsystem — decouple CodeFlow testing from Rust self-host

## Summary

Generalize CodeFlow's testing subsystem so downstream adopters on any stack (Node/Go/Python/Rust/mixed monorepos) can configure it via declarative config without per-stack code. Canonical internal model: CTRF-shaped with JUnit input adapter. Strip Rust-specific assumptions from Claude artifacts, the `codeflow test` CLI, and test-config.json. Migrate CodeFlow self-host as first adopter.

## Scope

### In Scope

- Declarative test configuration engine (CTRF output, JUnit input adapter)
- `codeflow test` CLI generalization (remove Rust-specific hardcodes)
- Setup wizard, doctor, and template library for supported stacks
- Claude artifact generalization (agent defs, CLAUDE.md, task/epic templates)
- CodeFlow self-host migration as first production adopter

### Out of Scope

- Runtime test execution (CodeFlow is a harness orchestrator, not a test runner)
- Language-specific test frameworks (pytest, jest, cargo test — invoked externally)
- CI/CD pipeline configuration (out of scope for this epic)

## Acceptance Criteria

- [ ] `codeflow test --mode full --coverage` works on a Node/Go/Python/Rust/mixed-monorepo project configured with declarative `.codeflow/config/testing/test-config.json` (no CodeFlow code changes required per adopter)
- [ ] Canonical internal model is CTRF-shaped; JUnit XML input is losslessly converted via the engine's JUnit→CTRF converter; `runner: custom` escape hatch accepts CodeFlow-canonical JSON
- [ ] Coverage supports four well-known formats (lcov, cobertura, istanbul-summary, go-cover); coverage uses an ordered rule list over five scope types (per_file, per_package, per_module, changed_files, global) with first-match-wins for file-level scopes
- [ ] `codeflow test setup` wizard (interactive, `--auto`, `--template`, `--list`, `--add-target`) supports fresh-project, per-stack, and monorepo scenarios via 9 templates
- [ ] `codeflow test doctor` validates test configuration via JSON schema + cwd/command/path/transform/glob/exception checks + dry-run probe per runner
- [ ] Fresh-project path: `codeflow test` on a project with empty `targets[]` emits a single-line "No test targets configured" notice, does not fail
- [ ] PR body renders the 5-section target-aware Test Results format (Overall Test Pass Status, Overall Coverage with per-target Exempted Files, Modified File Coverage, optional Test Failures when >0, optional Slowest Tests); gh-pr-guard validates via markdown parsing, not substring matching
- [ ] `.state/test-reports/` and `.state/coverage/` are per-worktree LOCAL in `WorktreePaths`; concurrent worktree test runs never collide on artifacts
- [ ] Ledger emits `test_result_recorded` event on run completion with target-aware canonical fields
- [ ] All Claude artifacts (agent defs, commands, skills, templates, CLAUDE.md) contain zero Rust-specific test assumptions in instructional text; grep invariant passes
- [ ] Stack-specific skills (cf-rust-standards, cf-python-standards, cf-shell-standards) remain as on-demand references, not mandatory
- [ ] CodeFlow self-host migrated: authored `.codeflow/config/testing/test-config.json` + `codeflow-cli/.config/nextest.toml`; existing shell/Python/Rust tests all pass through the generic engine with zero regressions
- [ ] Legacy `TestValidator::full_validate` code path removed; old `codeflow-cli/config/testing/test-config.json` deleted; `.gitignore` updated
- [ ] Invariant preserved: LLMs never directly Edit/Write `.state/**` or `.codeflow/config/testing/test-config.json`. Every required mutation has a `codeflow test <subcommand>` (documented in design doc §13 CLI Command Surface Matrix)

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Estimate | Status | Priority |
|----|-------|----------|--------|----------|
| INF-TSK-046-001 | Plan generic testing subsystem — ADR, design doc, epic breakdown, 5 implementation task specs | L | complete | high |
| INF-TSK-046-002 | Engine + parsers + CTRF model + JUnit converter | XL | complete | high |
| INF-TSK-046-003 | Setup wizard + doctor + template library | L | complete | high |
| INF-TSK-046-004 | Claude artifact generalization | XL | todo | high |
| INF-TSK-046-005 | Self-host migration + hardcode retirement | L | todo | high |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Architecture decision: CTRF as canonical internal model (CommonTestResultsFormat — language-agnostic JSON). JUnit XML is the most common input format across stacks; an adapter converts it to CTRF before processing. This allows the engine to be stack-agnostic while supporting the widest range of existing test tooling.

### Autorun Batching

TSK-002 through TSK-005 are implementation tasks with inter-dependencies. TSK-002 must complete before TSK-003/004/005 can start. TSK-003, TSK-004 can run in parallel after TSK-002. TSK-005 requires all prior tasks.

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 1 | INF-TSK-046-001 | 1 | Planning — sequential |
| 2 | INF-TSK-046-002 | 1 | Engine foundation — sequential |
| 3 | INF-TSK-046-003, INF-TSK-046-004 | 2 | Independent post-engine |
| 4 | INF-TSK-046-005 | 1 | Migration — requires all above |

**Estimated Duration:** 4 batches × ~2 sessions each = 8 sessions total

## Related

- `.codeflow/docs/analysis/shadow-testing-architecture.md` — prior shadow harness reference
- `.codeflow/docs/analysis/generic-testing-subsystem.md` — design/analysis document (produced by TSK-001)
- `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md` — architecture decision record (produced by TSK-001)
- <https://ctrf.io> — Common Test Report Format specification (canonical internal model)
- <https://junit.org/> — JUnit XML (primary universal input adapter)
