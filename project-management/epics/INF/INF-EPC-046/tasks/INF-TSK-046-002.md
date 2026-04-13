---
id: "task-01KP0WRNT0BEMCW2T9GQ9V43A0"
format_id: "INF-TSK-046-002"
epic_id: "epic-01KP0WRNT08J88E09QGQ6185BP"
epic_format_id: "INF-EPC-046"
title: "Engine + parsers + CTRF model + JUnit converter"
description: "Implement the core testing engine: CTRF-shaped canonical internal model, JUnit XML input adapter with lossless CTRF conversion, declarative test-config.json loader with JSON-schema validation, four coverage-format parsers (lcov, cobertura, istanbul-summary, go-cover), ordered coverage rule engine with five scope types, per-target shell command runner, canonical PR-body emitter, and the new codeflow test CLI subcommand surface (report/config/exceptions/setup/doctor/clean). .state/test-reports/ and .state/coverage/ become LOCAL per-worktree via WorktreePaths extensions. Ledger emits test_result_recorded on run completion."
status: todo
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
origin: planned
file_scope:
  - "codeflow-cli/core/src/testing/**"
  - "codeflow-cli/core/src/worktree/paths.rs"
  - "codeflow-cli/cli/src/cmd/test.rs"
  - "codeflow-cli/cli/src/cmd/mod.rs"
  - "codeflow-cli/core/src/ledger/**"
  - ".codeflow/schemas/test-config.schema.json"
  - ".codeflow/config/testing/test-config.json"
scope_policy: hard
scope_root: null
estimate: XL
priority: high
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - "CanonicalTestReport struct defined in codeflow-cli/core/src/testing/report/mod.rs with CTRF-shaped fields (tool, results, summary with total/passed/failed/skipped/pending/other/start/stop, tests[] array with name/status/duration/suite/message/trace/tags/flaky, environment, extra) and derives Serialize/Deserialize via serde"
  - "JUnit XML parser exists in codeflow-cli/core/src/testing/report/junit.rs using quick-xml, exposing parse_junit(path: &Path) -> Result<JunitReport, TestingError> and handling testsuite/testcase/failure/error/skipped elements and cargo-nextest, pytest-junitxml, jest-junit, vitest dialects"
  - "JUnit-to-CTRF converter exists in codeflow-cli/core/src/testing/report/junit_to_ctrf.rs as impl From<JunitReport> for CanonicalTestReport and preserves all fields CodeFlow consumes (counts, durations, failure messages, suite names); round-trip fixture tests pass for cargo-nextest, pytest-junitxml, jest-junit, vitest golden fixtures stored under codeflow-cli/core/src/testing/report/fixtures/"
  - "CTRF native reader exists in codeflow-cli/core/src/testing/report/ctrf.rs as parse_ctrf(path: &Path) -> Result<CanonicalTestReport, TestingError> using serde_json and validates against the CTRF schema"
  - "Config loader in codeflow-cli/core/src/testing/config/mod.rs reads .codeflow/config/testing/test-config.json into TestConfig struct with fields schema_version, execution, defaults, targets[]; validates against .codeflow/schemas/test-config.schema.json on load; returns TestingError::ConfigInvalid with a human-readable message on any schema violation"
  - "JSON schema published at .codeflow/schemas/test-config.schema.json in JSON Schema draft 2020-12 format, covering every field documented in the design doc §8 (targets, runner enum, modes, report, coverage, rules, scopes, exceptions); doctor validates every test-config.json against it"
  - "Coverage parser for lcov exists in codeflow-cli/core/src/testing/coverage/lcov.rs parsing SF/DA/BA records into FileCoverage { path, lines_found, lines_hit, percent }; golden-fixture test passes for llvm-cov output"
  - "Coverage parser for cobertura exists in codeflow-cli/core/src/testing/coverage/cobertura.rs using quick-xml; golden-fixture test passes for Jest --coverageReporters=cobertura and Python coverage.py output"
  - "Coverage parser for istanbul-summary exists in codeflow-cli/core/src/testing/coverage/istanbul.rs using serde_json; golden-fixture test passes for nyc and c8 output"
  - "Coverage parser for go-cover exists in codeflow-cli/core/src/testing/coverage/go_cover.rs; golden-fixture test passes for `go test -coverprofile=coverage.out` output"
  - "Threshold engine in codeflow-cli/core/src/testing/threshold/mod.rs evaluates CoverageRule array against FileCoverage set using first-match-wins for per_file and changed_files scopes, all-rules-evaluated for per_package/per_module/global; unit tests cover every permutation (at least 15 test cases) including empty rule list, overlapping globs, exception override, changed-files resolution via git diff"
  - "Command runner in codeflow-cli/core/src/testing/runner/mod.rs spawns each enabled target's mode-specific command via std::process::Command in the target's cwd, capturing stdout/stderr, propagating TARGET env plus the inherited env plus target-local env, and returning TargetRunResult { target_name, exit_code, stdout, stderr, duration, report_path, coverage_path }"
  - "Execution orchestrator runs targets sequentially by default and in parallel when execution.parallel is true, spawning subprocesses (not threads) for parallel execution; fail-fast off by default; no retries"
  - "WorktreePaths gains test_reports_dir() and coverage_dir() methods returning PathBuf joined from root + .state/test-reports and root + .state/coverage respectively; both are LOCAL (not symlinked); unit tests verify different worktrees produce different paths"
  - "Ledger event test_result_recorded emitted on run completion via codeflow-cli/core/src/ledger/events.rs with fields { session_id, run_id, targets[], overall_pass, total_passed, total_failed, total_skipped, workspace_coverage, duration_ms, timestamp }; ledger write goes through locked append, not direct file Write"
  - "codeflow test command in codeflow-cli/cli/src/cmd/test.rs supports --mode quick|essential|full, --coverage, --only <target>, --skip <target>, --report, --format human|json, --fail-fast; exit codes 0 (all pass), 1 (test or coverage failure), 2 (config error), 3 (infrastructure error)"
  - "Fresh-project path: when test-config.json.targets[] is empty or all targets are disabled, codeflow test prints 'No test targets configured. Run `codeflow test setup` to add targets.' and exits 0"
  - "codeflow test report show [--run-id <id>] [--format <fmt>] reads the ledger test_result_recorded event (or specified run-id) and prints the canonical summary"
  - "codeflow test report convert --from <junit|ctrf> --to <junit|ctrf> --input <path> --output <path> converts between formats using the internal JUnit→CTRF engine; used by self-host and by agents (replaces any direct file read on .state/test-reports)"
  - "codeflow test report diff --from <run-id> --to <run-id> prints regressions (new failures, per-file coverage decreases)"
  - "codeflow test config show prints the current test-config.json (human or JSON format)"
  - "codeflow test config add-target --name <n> --runner <r> --cwd <c> [--mode <m>=<cmd>]... appends a target to targets[]; validates against schema before writing; refuses duplicate name"
  - "codeflow test config remove-target --name <n> removes the target and its exceptions"
  - "codeflow test config enable --target <n> and codeflow test config disable --target <n> toggle the target.enabled field"
  - "codeflow test config set-command --target <n> --mode <m> --command '<cmd>' replaces a mode's command; refuses unknown mode; rejects command strings containing unbalanced quotes (basic shell lint)"
  - "codeflow test config set-threshold --target <n> --scope <s> --minimum <n> [--rule-index <i>] changes an existing rule's minimum; adds a new rule if no matching scope/index exists with a warning to stderr"
  - "codeflow test config set-report --target <n> --format <junit|ctrf> --path <p> [--derive-from junit] sets the target's report config"
  - "codeflow test config set-coverage --target <n> --format <lcov|cobertura|istanbul-summary|go-cover> --path <p> sets the target's coverage config"
  - "codeflow test exceptions add --target <n> --file <f> --threshold <t> --reason '<r>' --remove-when '<w>' appends an exception; CLI refuses to add without --reason and --remove-when"
  - "codeflow test exceptions remove --target <n> --file <f> removes the exception"
  - "codeflow test exceptions list [--target <n>] prints exceptions as a table; without --target, lists across all targets"
  - "codeflow test clean [--artifacts] [--yes] removes .state/test-reports/ and .state/coverage/ for the current worktree; without --yes, prompts; --artifacts is the default (implicit) scope"
  - "All config-mutation subcommands (config add-target, config set-*, exceptions add/remove, setup) write test-config.json via an internal config-writer that produces deterministic output (stable key order, 2-space indent, trailing newline) — byte-equivalent round-trip for no-op mutations"
  - "Config-writer never invokes Edit/Write tool against .codeflow/config/testing/test-config.json from the LLM surface; only the CLI subcommands modify the file; ProtectionGuard continues to block any direct LLM Edit attempt"
  - "No task deliverable introduces an LLM code path that Edit/Writes into .state/** or .codeflow/config/testing/test-config.json directly. Every mutation has a corresponding codeflow test <subcommand>. Task doc appendix 'CLI Command Surface Matrix' proves this before implementation begins."
  - "PR-body emitter in codeflow-cli/core/src/testing/pr_body.rs renders the five-section Markdown (Overall Test Pass Status, Overall Coverage, Modified File Coverage, Test Failures when >0, Slowest Tests optional) with target-aware tables including a Target column in every table; fresh-project path emits the single-line 'No test targets configured' notice"
  - "PR-body emitter sources Test Failures section from CTRF tests[].status=failed entries (when CTRF is available) or JUnit failure elements (fallback); sources Slowest Tests from CTRF tests[].duration sorted descending top 10 (or JUnit testcase/@time)"
  - "Modified-file-coverage routing: resolves git diff main...HEAD (or AUTORUN_INTEGRATION_BRANCH...HEAD in autorun) and routes each changed file to the target whose cwd is the longest matching path prefix; files matching no target appear in an 'Unattributed' row with status FAIL"
  - "Unit tests: cargo nextest run --profile full passes for every new module with per-file coverage ≥85% except exemptions added to the new test-config.json in TSK-005; lint: cargo clippy --all-targets --all-features -- -D warnings clean; format: cargo fmt --check passes"
  - "Integration test: spawns two worktrees, runs codeflow test --mode full --coverage in each concurrently against a minimal test-config.json, verifies each worktree's .state/test-reports/ contains its own run artifact and no cross-contamination"
  - "Error path test: JUnit parse failure (malformed XML) produces a structured TestingError with the file path and byte offset, does not panic, exits with code 3 (infrastructure error)"
  - "Error path test: coverage artifact missing (command ran but lcov.info not produced) emits a warning in the PR body's coverage section (target shows 'N/A — coverage artifact not produced at <path>') and does not fail the run unless a coverage rule explicitly requires the file"
  - "Error path test: invalid test-config.json schema violation prints a validation error pointing at the offending JSON path (e.g., 'targets[1].coverage.rules[0].scope: expected one of [per_file, ...], got \"line\"') and exits with code 2"
  - "Error path test: a target declared with modes.quick but invoked with --mode full is silently skipped (not an error); stderr carries a debug line 'skipping target <n>: mode full not configured'"
  - "No direct .state/** Edit/Write: grep of test-config handler code, ledger emitter, and PR-body emitter confirms all .state/** operations go through WorktreePaths methods and the ledger's locked-append API"
  - "--format json output for every subcommand is a stable documented schema (serde-derived); consumed by cf-knowledge-layer ledger recording; cf-git-operations consumes the Markdown emitter output (AC#36), not the JSON — JSON is for programmatic consumers"
  - "Protection policy extension: .codeflow/config/enforcement/enforcement-policy.json is updated in this PR to add '.state/test-reports/**' and '.state/coverage/**' to BOTH protected_resources.high[] AND worktree_protection.patterns[]; verified by reading the file post-PR and confirming both arrays contain both patterns"
  - "Schema-evolution behavior per design doc §8.5: (a) supported schema_version → accept; (b) missing schema_version → reject exit 2 with 'missing required field schema_version'; (c) unknown schema_version → reject exit 2 with 'unsupported schema_version=X; this engine supports: [1.0]'; (d) unknown top-level field → warn to stderr, accept; (e) unknown field inside required section (targets[].X) → reject exit 2 with JSON path; unit tests cover each case"
  - "Modified-file-coverage routing implements design doc §9.5 six ordered rules: normalize cwd (empty/./missing → root; trailing slashes stripped); extension-affinity filter for cargo (*.rs) and go (*.go) runners only; longest-prefix match; tie-break by declaration order; empty-cwd catch-all matches all; no-match → Unattributed row FAIL. Unit tests cover every row of the §9.5 examples table (at least 6 test cases)"
  - "Coverage transform non-zero exit behavior: when coverage.transform command returns non-zero exit code, the engine degrades that target's coverage reporting to 'N/A — coverage.transform failed with exit code N' in the PR body and does NOT fail the run; stderr warning emitted; unit test covers this path with a mock command that exits 1"
tests:
  - "codeflow-cli/core/src/testing/report/junit.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/report/junit_to_ctrf.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/report/ctrf.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/config/mod.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/coverage/lcov.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/coverage/cobertura.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/coverage/istanbul.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/coverage/go_cover.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/threshold/mod.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/runner/mod.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/pr_body.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/worktree/paths.rs (extended tests for test_reports_dir and coverage_dir)"
  - "codeflow-cli/cli/src/cmd/test.rs (inline #[cfg(test)] mod tests for every new subcommand)"
  - "codeflow-cli/tests/integration_testing_parallel_worktrees.rs (new integration test)"
branch: null
pr_number: null
external_id: null
external_url: null
dependencies:
  - "task-01KP0WRNT0PZEMHNP5VAF83VN2"
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-11T00:00:00Z"
started_at: null
completed_at: null
stage: null
stage_status: null
stage_history: "[]"
---

# INF-TSK-046-002: Engine + parsers + CTRF model + JUnit converter

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

Implement the core testing engine that makes `codeflow test` configuration-driven and stack-agnostic. Land the CTRF-shaped canonical internal model, JUnit XML input adapter with lossless conversion, declarative config loader with JSON-schema validation, four coverage-format parsers, ordered rule-list threshold engine with five scope types, per-target shell command runner, canonical PR-body emitter, and the new `codeflow test` CLI subcommand surface.

Drops `.state/test-reports/` and `.state/coverage/` into `WorktreePaths` as LOCAL (per-worktree) directories. Emits a `test_result_recorded` ledger event on run completion so downstream observers (cf-git-operations PR body, cf-knowledge-layer memory, cross-session reporting) have structured query access to every run.

The implementation is gated against LLM direct writes: `test-config.json` and `.state/**` remain protected, with every required mutation covered by a `codeflow test <subcmd>` CLI path. A "CLI Command Surface Matrix" appendix below proves the coverage before code is written.

## Deliverables

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| Canonical test-report model | feature | `codeflow-cli/core/src/testing/report/mod.rs` | Every downstream consumer (PR body, ledger, diff, external reporters) |
| JUnit → CTRF converter | feature | `codeflow-cli/core/src/testing/report/junit.rs` + `junit_to_ctrf.rs` | Universal input path for runners lacking native CTRF |
| CTRF native reader | feature | `codeflow-cli/core/src/testing/report/ctrf.rs` | Runners with native CTRF (jest, vitest, pytest, go, etc.) |
| Config loader + JSON schema | feature | `codeflow-cli/core/src/testing/config/mod.rs` + `.codeflow/schemas/test-config.schema.json` | Every subcommand that reads/mutates config |
| Coverage parsers (4 formats) | feature | `codeflow-cli/core/src/testing/coverage/{lcov,cobertura,istanbul,go_cover}.rs` | Per-target coverage ingestion |
| Threshold engine | feature | `codeflow-cli/core/src/testing/threshold/mod.rs` | Coverage gate evaluation |
| Target runner | feature | `codeflow-cli/core/src/testing/runner/mod.rs` | Orchestrates per-target shell execution (sequential or parallel) |
| WorktreePaths extension | feature | `codeflow-cli/core/src/worktree/paths.rs` | `test_reports_dir()` + `coverage_dir()` LOCAL paths |
| Ledger event | feature | `codeflow-cli/core/src/ledger/events.rs` | `test_result_recorded` queryable cross-session |
| `codeflow test` CLI | feature | `codeflow-cli/cli/src/cmd/test.rs` | User and agent entry point |
| `codeflow test report` subcmds | feature | `codeflow-cli/cli/src/cmd/test.rs` | Report show / convert / diff |
| `codeflow test config` subcmds | feature | `codeflow-cli/cli/src/cmd/test.rs` | Config show / add-target / remove-target / enable / disable / set-command / set-threshold / set-report / set-coverage |
| `codeflow test exceptions` subcmds | feature | `codeflow-cli/cli/src/cmd/test.rs` | Exceptions add / remove / list |
| `codeflow test clean` | feature | `codeflow-cli/cli/src/cmd/test.rs` | Worktree artifact cleanup |
| PR-body emitter | feature | `codeflow-cli/core/src/testing/pr_body.rs` | Consumed by cf-git-operations PR-body population |

**Expected Outcome:** `codeflow test --mode full --coverage` works on any stack expressed via a valid `test-config.json` and produces a CTRF-shaped canonical report, a five-section PR-body markdown, and a ledger event. Fresh-project path (empty targets) produces a single-line informational notice. The LLM attack surface is preserved: `test-config.json` and `.state/**` remain ProtectionGuard-blocked; every required mutation has a CLI subcommand.

**Deployment:** PR merge on `feat/generic-testing-engine` branch into main. Old `TestValidator::full_validate` path becomes a transient deprecation stub (renamed `legacy_validation.rs`) in this PR and is removed by TSK-005 — no feature flag, clean cutover per TSK-005 AC#16, design doc §17.2, and ADR-001 §Consequences.

## Pre-Work Analysis

- [ ] Reviewed git log for recent changes to `codeflow-cli/core/src/testing/`, `codeflow-cli/cli/src/cmd/test.rs`, `codeflow-cli/core/src/worktree/paths.rs`
- [ ] Confirmed no concurrent PRs modify the testing subsystem
- [ ] Verified CTRF schema at ctrf.io has not changed shape since design doc author date
- [ ] Verified cargo-nextest JUnit output shape unchanged (fixture still applicable)

## Approach

1. **Scaffold modules.** Create `codeflow-cli/core/src/testing/{report,config,coverage,threshold,runner,pr_body}/` as directory-based modules; wire into `codeflow-cli/core/src/testing/mod.rs`. Replace the existing single-file `validation.rs` with the new architecture. `validation.rs` becomes a transient deprecation stub (renamed `legacy_validation.rs`) removed by TSK-005 — no feature flag.
2. **Canonical model first.** Define `CanonicalTestReport` and its sub-structs matching CTRF shape. Write unit tests for serde round-trip (JSON↔struct) before any parser.
3. **JUnit parser + converter.** Implement `parse_junit` using quick-xml; cover cargo-nextest, pytest, jest-junit, vitest dialect variations. Implement `impl From<JunitReport> for CanonicalTestReport`. Add golden-fixture tests under `codeflow-cli/core/src/testing/report/fixtures/`.
4. **CTRF native reader.** Implement `parse_ctrf` with serde_json; validate against the CTRF schema (ship schema in-tree if ctrf.io version pinning is required).
5. **Config loader.** Author the JSON schema at `.codeflow/schemas/test-config.schema.json`. Implement `load_test_config(path)` with `jsonschema` crate validation. Return structured `TestingError::ConfigInvalid { json_path, message }` for violations.
6. **Coverage parsers.** Implement the four parsers with deterministic output shape (`FileCoverage { path, lines_found, lines_hit, percent }`). Ship golden-fixture per parser under `coverage/fixtures/`.
7. **Threshold engine.** Implement the ordered-rule evaluation with per-scope handling. Unit tests for every permutation.
8. **Target runner.** `TargetRunner::run(target, mode)` spawns the shell command via `std::process::Command`, captures output, resolves report/coverage paths relative to target cwd, returns `TargetRunResult`. Parallel execution uses separate child processes (not threads), each with its own pipe.
9. **Ledger event.** Add `test_result_recorded` variant to ledger events enum; emit via locked append.
10. **CLI subcommands.** Implement `codeflow test`, `codeflow test report <subcmd>`, `codeflow test config <subcmd>`, `codeflow test exceptions <subcmd>`, `codeflow test clean`. Use clap's subcommand derive.
11. **Config writer.** Implement deterministic serialization (stable key order via `BTreeMap`, 2-space indent, trailing newline). Every mutation subcommand calls the writer.
12. **PR-body emitter.** Render the five-section markdown; source failures and durations from CTRF. Target-aware column in every table.
13. **WorktreePaths extension.** Add `test_reports_dir()` and `coverage_dir()` methods; verify LOCAL (not symlinked).
14. **Integration test.** Author `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` spawning two worktrees and running `codeflow test` concurrently.
15. **Golden-fixture tests.** JUnit round-trip fixtures for cargo-nextest, pytest, jest-junit, vitest; coverage parser fixtures for lcov, cobertura, istanbul-summary, go-cover.
16. **Lint.** `cargo clippy --all-targets --all-features -- -D warnings` passes on every new module.
17. **Coverage.** Per-file ≥85% via `cargo llvm-cov` (except self-exemptions to be declared in TSK-005's new config).

## Standards & Practices

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Rust | cf-rust-standards | No unsafe, thiserror/anyhow for errors, clippy `-D warnings` clean, serde-derived types with stable JSON output, stable error messages |
| JSON Schema | cf-markdown-standards / upstream | Draft 2020-12, `$schema` field present, required/optional correctly declared |

Read the applicable skill BEFORE starting implementation.

## Files

### To Create

- `codeflow-cli/core/src/testing/report/mod.rs` — `CanonicalTestReport`, `CtrfSummary`, `CtrfTest`, `CtrfStatus` structs
- `codeflow-cli/core/src/testing/report/junit.rs` — JUnit XML parser
- `codeflow-cli/core/src/testing/report/junit_to_ctrf.rs` — conversion
- `codeflow-cli/core/src/testing/report/ctrf.rs` — native CTRF reader
- `codeflow-cli/core/src/testing/report/fixtures/*.xml` and `*.json` — golden fixtures
- `codeflow-cli/core/src/testing/config/mod.rs` — config loader + writer
- `codeflow-cli/core/src/testing/coverage/mod.rs` — dispatch
- `codeflow-cli/core/src/testing/coverage/lcov.rs`
- `codeflow-cli/core/src/testing/coverage/cobertura.rs`
- `codeflow-cli/core/src/testing/coverage/istanbul.rs`
- `codeflow-cli/core/src/testing/coverage/go_cover.rs`
- `codeflow-cli/core/src/testing/coverage/fixtures/*.{info,xml,json,out}` — golden fixtures
- `codeflow-cli/core/src/testing/threshold/mod.rs` — rule-list engine
- `codeflow-cli/core/src/testing/runner/mod.rs` — target shell runner
- `codeflow-cli/core/src/testing/pr_body.rs` — markdown emitter
- `codeflow-cli/core/src/testing/error.rs` — `TestingError` enum
- `.codeflow/schemas/test-config.schema.json` — JSON schema (draft 2020-12)
- `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` — concurrent worktree test

### To Modify

- `codeflow-cli/core/src/testing/mod.rs` — export new submodules; replace existing `validation` export with `legacy_validation` (the transient deprecation stub, removed by TSK-005) — no feature flag
- `codeflow-cli/core/src/testing/validation.rs` — rename to `legacy_validation.rs` as a **transient stub** containing only a deprecation shim (`pub fn full_validate_deprecated() -> Result<(), TestingError> { Err(TestingError::LegacyEngineRemoved) }`). The file continues to compile so any stale import during TSK-003/TSK-004 development breaks with a clear error pointing at TSK-005. **TSK-005 removes `legacy_validation.rs` entirely** — do NOT retain feature flags or `CODEFLOW_TEST_ENGINE=legacy` gating. This matches the clean-cutover decision in TSK-005 AC#16 and ADR-001 §Consequences.
- `codeflow-cli/core/src/worktree/paths.rs` — add `test_reports_dir()` + `coverage_dir()` methods and their tests
- `codeflow-cli/core/src/ledger/events.rs` — add `TestResultRecorded` variant
- `codeflow-cli/cli/src/cmd/test.rs` — rewrite command surface (new subcommands + flags)
- `codeflow-cli/cli/src/cmd/mod.rs` — wire `test::config`, `test::report`, `test::exceptions`, `test::clean` subcommand enums
- `.codeflow/config/testing/test-config.json` — this file's LOCATION moves (see TSK-005); TSK-002 creates the new path but leaves the old path in place until TSK-005

### To Read

- `codeflow-cli/core/src/testing/validation.rs` (current) — understand existing behavior before replacing
- `codeflow-cli/cli/src/cmd/test.rs` (current) — understand existing CLI before replacing
- `codeflow-cli/core/src/worktree/paths.rs` (current) — understand `WorktreePaths` shape before extending
- `.codeflow/docs/analysis/generic-testing-subsystem.md` — §§6-16 cover the design
- `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md` — decision rationale

## Concurrency Considerations

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| `.state/ledger/*.jsonl` | append on run completion | Existing locked append (per PR #221, per-worktree local) |
| `.codeflow/config/testing/test-config.json` | read-modify-write by mutation subcommands | Atomic temp+rename; refuses concurrent writers via file lock (`file_lock.rs`) |
| `.state/test-reports/<run-id>.ctrf.json` | write-once per run | Per-worktree LOCAL dir; run-id embeds session-id + monotonic counter to guarantee uniqueness |
| `.state/coverage/<target>/<format>` | write-once per run | Same as above; per-target subdir prevents collision across parallel targets |
| Subprocess child handles | per-target isolation | Each target runs in its own `std::process::Command`; parallel runs spawn N children, never shared threads |
| CTRF report parser | read-only input | Immutable `&Path` into `parse_*` functions |

## Acceptance Criteria

1. `CanonicalTestReport` struct defined in `codeflow-cli/core/src/testing/report/mod.rs` with CTRF-shaped fields (tool, results, summary with total/passed/failed/skipped/pending/other/start/stop, tests[] array with name/status/duration/suite/message/trace/tags/flaky, environment, extra) and derives `Serialize`/`Deserialize` via serde
2. JUnit XML parser exists in `codeflow-cli/core/src/testing/report/junit.rs` using quick-xml, exposing `parse_junit(path: &Path) -> Result<JunitReport, TestingError>` and handling testsuite/testcase/failure/error/skipped elements and cargo-nextest, pytest-junitxml, jest-junit, vitest dialects
3. JUnit-to-CTRF converter exists in `codeflow-cli/core/src/testing/report/junit_to_ctrf.rs` as `impl From<JunitReport> for CanonicalTestReport` and preserves all fields CodeFlow consumes (counts, durations, failure messages, suite names); round-trip fixture tests pass for cargo-nextest, pytest-junitxml, jest-junit, vitest golden fixtures stored under `codeflow-cli/core/src/testing/report/fixtures/`
4. CTRF native reader exists in `codeflow-cli/core/src/testing/report/ctrf.rs` as `parse_ctrf(path: &Path) -> Result<CanonicalTestReport, TestingError>` using serde_json and validates against the CTRF schema
5. Config loader in `codeflow-cli/core/src/testing/config/mod.rs` reads `.codeflow/config/testing/test-config.json` into `TestConfig` struct with fields schema_version, execution, defaults, targets[]; validates against `.codeflow/schemas/test-config.schema.json` on load; returns `TestingError::ConfigInvalid` with a human-readable message on any schema violation
6. JSON schema published at `.codeflow/schemas/test-config.schema.json` in JSON Schema draft 2020-12 format, covering every field documented in the design doc §8 (targets, runner enum, modes, report, coverage, rules, scopes, exceptions); doctor validates every `test-config.json` against it
7. Coverage parser for lcov exists in `codeflow-cli/core/src/testing/coverage/lcov.rs` parsing SF/DA/BA records into `FileCoverage { path, lines_found, lines_hit, percent }`; golden-fixture test passes for llvm-cov output
8. Coverage parser for cobertura exists in `codeflow-cli/core/src/testing/coverage/cobertura.rs` using quick-xml; golden-fixture test passes for Jest `--coverageReporters=cobertura` and Python coverage.py output
9. Coverage parser for istanbul-summary exists in `codeflow-cli/core/src/testing/coverage/istanbul.rs` using serde_json; golden-fixture test passes for nyc and c8 output
10. Coverage parser for go-cover exists in `codeflow-cli/core/src/testing/coverage/go_cover.rs`; golden-fixture test passes for `go test -coverprofile=coverage.out` output
11. Threshold engine in `codeflow-cli/core/src/testing/threshold/mod.rs` evaluates `CoverageRule` array against `FileCoverage` set using first-match-wins for per_file and changed_files scopes, all-rules-evaluated for per_package/per_module/global; unit tests cover every permutation (at least 15 test cases) including empty rule list, overlapping globs, exception override, changed-files resolution via git diff
12. Command runner in `codeflow-cli/core/src/testing/runner/mod.rs` spawns each enabled target's mode-specific command via `std::process::Command` in the target's cwd, capturing stdout/stderr, propagating TARGET env plus the inherited env plus target-local env, and returning `TargetRunResult { target_name, exit_code, stdout, stderr, duration, report_path, coverage_path }`
13. Execution orchestrator runs targets sequentially by default and in parallel when `execution.parallel` is true, spawning subprocesses (not threads) for parallel execution; fail-fast off by default; no retries
14. `WorktreePaths` gains `test_reports_dir()` and `coverage_dir()` methods returning PathBuf joined from root + `.state/test-reports` and root + `.state/coverage` respectively; both are LOCAL (not symlinked); unit tests verify different worktrees produce different paths
15. Ledger event `test_result_recorded` emitted on run completion via `codeflow-cli/core/src/ledger/events.rs` with fields { session_id, run_id, targets[], overall_pass, total_passed, total_failed, total_skipped, workspace_coverage, duration_ms, timestamp }; ledger write goes through locked append, not direct file Write
16. `codeflow test` command in `codeflow-cli/cli/src/cmd/test.rs` supports `--mode quick|essential|full`, `--coverage`, `--only <target>`, `--skip <target>`, `--report`, `--format human|json`, `--fail-fast`; exit codes 0 (all pass), 1 (test or coverage failure), 2 (config error), 3 (infrastructure error)
17. Fresh-project path: when `test-config.json.targets[]` is empty or all targets are disabled, `codeflow test` prints "No test targets configured. Run `codeflow test setup` to add targets." and exits 0
18. `codeflow test report show [--run-id <id>] [--format <fmt>]` reads the ledger `test_result_recorded` event (or specified run-id) and prints the canonical summary
19. `codeflow test report convert --from <junit|ctrf> --to <junit|ctrf> --input <path> --output <path>` converts between formats using the internal JUnit→CTRF engine; used by self-host and by agents (replaces any direct file read on `.state/test-reports`)
20. `codeflow test report diff --from <run-id> --to <run-id>` prints regressions (new failures, per-file coverage decreases)
21. `codeflow test config show` prints the current `test-config.json` (human or JSON format)
22. `codeflow test config add-target --name <n> --runner <r> --cwd <c> [--mode <m>=<cmd>]...` appends a target to `targets[]`; validates against schema before writing; refuses duplicate name
23. `codeflow test config remove-target --name <n>` removes the target and its exceptions
24. `codeflow test config enable --target <n>` and `codeflow test config disable --target <n>` toggle the target.enabled field
25. `codeflow test config set-command --target <n> --mode <m> --command '<cmd>'` replaces a mode's command; refuses unknown mode; rejects command strings containing unbalanced quotes (basic shell lint)
26. `codeflow test config set-threshold --target <n> --scope <s> --minimum <n> [--rule-index <i>]` changes an existing rule's minimum; adds a new rule if no matching scope/index exists with a warning to stderr
27. `codeflow test config set-report --target <n> --format <junit|ctrf> --path <p> [--derive-from junit]` sets the target's report config
28. `codeflow test config set-coverage --target <n> --format <lcov|cobertura|istanbul-summary|go-cover> --path <p>` sets the target's coverage config
29. `codeflow test exceptions add --target <n> --file <f> --threshold <t> --reason '<r>' --remove-when '<w>'` appends an exception; CLI refuses to add without `--reason` and `--remove-when`
30. `codeflow test exceptions remove --target <n> --file <f>` removes the exception
31. `codeflow test exceptions list [--target <n>]` prints exceptions as a table; without `--target`, lists across all targets
32. `codeflow test clean [--artifacts] [--yes]` removes `.state/test-reports/` and `.state/coverage/` for the current worktree; without `--yes`, prompts; `--artifacts` is the default (implicit) scope
33. All config-mutation subcommands (config add-target, config set-*, exceptions add/remove, setup) write `test-config.json` via an internal config-writer that produces deterministic output (stable key order, 2-space indent, trailing newline) — byte-equivalent round-trip for no-op mutations
34. Config-writer never invokes Edit/Write tool against `.codeflow/config/testing/test-config.json` from the LLM surface; only the CLI subcommands modify the file; ProtectionGuard continues to block any direct LLM Edit attempt
35. No task deliverable introduces an LLM code path that Edit/Writes into `.state/**` or `.codeflow/config/testing/test-config.json` directly. Every mutation has a corresponding `codeflow test <subcommand>`. Task doc appendix "CLI Command Surface Matrix" proves this before implementation begins.
36. PR-body emitter in `codeflow-cli/core/src/testing/pr_body.rs` renders the five-section Markdown (Overall Test Pass Status, Overall Coverage, Modified File Coverage, Test Failures when >0, Slowest Tests optional) with target-aware tables including a Target column in every table; fresh-project path emits the single-line "No test targets configured" notice
37. PR-body emitter sources Test Failures section from CTRF tests[].status=failed entries (when CTRF is available) or JUnit failure elements (fallback); sources Slowest Tests from CTRF tests[].duration sorted descending top 10 (or JUnit testcase/@time)
38. Modified-file-coverage routing: resolves `git diff main...HEAD` (or `AUTORUN_INTEGRATION_BRANCH...HEAD` in autorun) and routes each changed file to the target whose cwd is the longest matching path prefix; files matching no target appear in an 'Unattributed' row with status FAIL
39. Unit tests: `cargo nextest run --profile full` passes for every new module with per-file coverage ≥85% except exemptions added to the new `test-config.json` in TSK-005; lint: `cargo clippy --all-targets --all-features -- -D warnings` clean; format: `cargo fmt --check` passes
40. Integration test: spawns two worktrees, runs `codeflow test --mode full --coverage` in each concurrently against a minimal `test-config.json`, verifies each worktree's `.state/test-reports/` contains its own run artifact and no cross-contamination
41. Error path test: JUnit parse failure (malformed XML) produces a structured `TestingError` with the file path and byte offset, does not panic, exits with code 3 (infrastructure error)
42. Error path test: coverage artifact missing (command ran but lcov.info not produced) emits a warning in the PR body's coverage section (target shows "N/A — coverage artifact not produced at <path>") and does not fail the run unless a coverage rule explicitly requires the file
43. Error path test: invalid `test-config.json` schema violation prints a validation error pointing at the offending JSON path (e.g., `targets[1].coverage.rules[0].scope: expected one of [per_file, ...], got "line"`) and exits with code 2
44. Error path test: a target declared with `modes.quick` but invoked with `--mode full` is silently skipped (not an error); stderr carries a debug line "skipping target <n>: mode full not configured"
45. No direct `.state/**` Edit/Write: grep of test-config handler code, ledger emitter, and PR-body emitter confirms all `.state/**` operations go through `WorktreePaths` methods and the ledger's locked-append API
46. `--format json` output for every subcommand is a stable documented schema (serde-derived); consumed by cf-knowledge-layer ledger recording; cf-git-operations consumes the Markdown emitter output (AC#36), not the JSON — JSON is for programmatic consumers
47. Protection policy extension: `.codeflow/config/enforcement/enforcement-policy.json` is updated in this PR to add `".state/test-reports/**"` and `".state/coverage/**"` to BOTH `protected_resources.high[]` AND `worktree_protection.patterns[]`; verified by reading the file post-PR and confirming both arrays contain both patterns; closes partial gap tracked in memory note `project_worktree_protection_fix.md`
48. Schema-evolution behavior per design doc §8.5: (a) supported `schema_version` → accept; (b) missing `schema_version` → reject exit 2 with "missing required field schema_version"; (c) unknown `schema_version` (e.g., `"2.0"`) → reject exit 2 with `"unsupported schema_version=X; this engine supports: [1.0]"`; (d) unknown top-level field → warn to stderr, accept; (e) unknown field inside required section (targets[].X) → reject exit 2 with JSON path; unit tests cover each case
49. Modified-file-coverage routing implements design doc §9.5 six ordered rules: normalize cwd (empty/`.`/missing → root; trailing slashes stripped); extension-affinity filter for `cargo` (`*.rs`) and `go` (`*.go`) runners only; longest-prefix match; tie-break by declaration order; empty-cwd catch-all matches all; no-match → Unattributed row FAIL. Unit tests cover every row of the §9.5 examples table (at least 6 test cases)
50. Coverage transform non-zero exit behavior: when `coverage.transform` command returns non-zero exit code, the engine degrades that target's coverage reporting to "N/A — coverage.transform failed with exit code N" in the PR body and does NOT fail the run; stderr warning emitted; unit test covers this path with a mock command that exits 1

### Scenarios covered (cross-reference to design doc §§12, 18)

| # | Scenario | Covered by acceptance # |
|---|----------|------------------------|
| 1 | Fresh project no-tests run → single-line notice | 17, 36 |
| 2 | Node Vitest with native CTRF | 4, 16, 36 |
| 3 | Go with `go-ctrf-json-reporter` or go test -json + converter | 4, 19 |
| 4 | Python pytest with JUnit or pytest-ctrf | 2, 3, 4 |
| 5 | Rust nextest with JUnit → CTRF via internal converter | 2, 3, 19 |
| 6 | Monorepo polyglot with parallel execution | 13, 16, 36, 38 |
| 7 | Concurrent worktree test runs without collision | 14, 40 |
| 8 | Coverage rule-list ordering (per-file strict with per-file lax for legacy/**) | 11 |
| 9 | Modified-file-coverage routing across targets via cwd prefix | 38 |
| 10 | Custom runner escape hatch | 12, 16 (custom runner enum) |
| 11 | JUnit parse failure — graceful degradation | 41 |
| 12 | Coverage artifact missing — sections downgrade | 42 |
| 13 | `codeflow test --only <target>`, `--skip <target>` | 16 |
| 14 | Invalid config schema | 5, 43 |
| 15 | Mode not configured | 44 |

### CLI Command Surface Matrix (proving `.state/**` and `test-config.json` invariant)

| Need | Temptation (direct Edit/Write) | CLI replacement (required by this task) |
|------|-------------------------------|------------------------------------------|
| See last test run | Read `.state/test-reports/*.ctrf.json` | `codeflow test --report` |
| Compare runs | Diff `.state/test-reports/*` | `codeflow test report diff --from <id> --to <id>` |
| Convert JUnit→CTRF | Write a conversion script | `codeflow test report convert --from junit --to ctrf --input <p> --output <p>` |
| Add target | Edit `test-config.json` | `codeflow test config add-target` |
| Remove target | Edit `test-config.json` | `codeflow test config remove-target` |
| Change command | Edit `test-config.json` | `codeflow test config set-command` |
| Enable/disable | Edit `test-config.json` | `codeflow test config enable|disable` |
| Add exception | Edit `test-config.json` | `codeflow test exceptions add` |
| Remove exception | Edit `test-config.json` | `codeflow test exceptions remove` |
| List exceptions | Read `test-config.json` | `codeflow test exceptions list` |
| Change threshold | Edit `test-config.json` | `codeflow test config set-threshold` |
| Initial config | Write `test-config.json` from scratch | Deferred to TSK-003 (`codeflow test setup`) |
| Apply template | Copy a template | Deferred to TSK-003 (`codeflow test setup --template`) |
| Verify config | Read + eyeball | Deferred to TSK-003 (`codeflow test doctor`) |
| Purge artifacts | `rm -rf .state/coverage` | `codeflow test clean --artifacts` |
| Read ledger event | Read `.state/ledger/*.jsonl` | `codeflow test report show --run-id <id>` |

Every row covered; no Edit/Write path remains. TSK-003 covers `setup`/`doctor`; all other paths are in TSK-002.

### PII Handling Review

- [ ] Does this task involve code that handles PII? (N)

### Criteria Status

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DEV -> WS-SEC -> WS-REV -> WS-QA

| # | Criterion | DEV | SEC | REV | QA | Notes |
|---|-----------|-----|-----|-----|-----|-------|
| 1 | CanonicalTestReport struct defined with CTRF-shaped fields and serde derive | -- | -- | -- | -- | |
| 2 | JUnit XML parser handles cargo-nextest/pytest/jest/vitest dialects | -- | -- | -- | -- | |
| 3 | JUnit→CTRF converter + golden fixture round-trip tests | -- | -- | -- | -- | |
| 4 | CTRF native reader + schema validation | -- | -- | -- | -- | |
| 5 | Config loader with JSON-schema validation and structured errors | -- | -- | -- | -- | |
| 6 | JSON schema published at `.codeflow/schemas/test-config.schema.json` | -- | -- | -- | -- | |
| 7 | lcov coverage parser + fixture | -- | -- | -- | -- | |
| 8 | cobertura coverage parser + fixture | -- | -- | -- | -- | |
| 9 | istanbul-summary coverage parser + fixture | -- | -- | -- | -- | |
| 10 | go-cover coverage parser + fixture | -- | -- | -- | -- | |
| 11 | Threshold engine with ordered rule-list semantics, ≥15 test cases | -- | -- | -- | -- | |
| 12 | Target command runner returning `TargetRunResult` | -- | -- | -- | -- | |
| 13 | Sequential + parallel execution, fail-fast off, no retries | -- | -- | -- | -- | |
| 14 | WorktreePaths `test_reports_dir()` + `coverage_dir()` (LOCAL) | -- | -- | -- | -- | |
| 15 | Ledger `test_result_recorded` event via locked append | -- | -- | -- | -- | |
| 16 | `codeflow test` CLI with flags, exit codes 0/1/2/3 | -- | -- | -- | -- | |
| 17 | Fresh-project notice on empty targets | -- | -- | -- | -- | |
| 18 | `codeflow test report show` | -- | -- | -- | -- | |
| 19 | `codeflow test report convert` | -- | -- | -- | -- | |
| 20 | `codeflow test report diff` | -- | -- | -- | -- | |
| 21 | `codeflow test config show` | -- | -- | -- | -- | |
| 22 | `codeflow test config add-target` | -- | -- | -- | -- | |
| 23 | `codeflow test config remove-target` | -- | -- | -- | -- | |
| 24 | `codeflow test config enable|disable --target` | -- | -- | -- | -- | |
| 25 | `codeflow test config set-command` with shell-lint | -- | -- | -- | -- | |
| 26 | `codeflow test config set-threshold` | -- | -- | -- | -- | |
| 27 | `codeflow test config set-report` | -- | -- | -- | -- | |
| 28 | `codeflow test config set-coverage` | -- | -- | -- | -- | |
| 29 | `codeflow test exceptions add` with required reason/remove-when | -- | -- | -- | -- | |
| 30 | `codeflow test exceptions remove` | -- | -- | -- | -- | |
| 31 | `codeflow test exceptions list` | -- | -- | -- | -- | |
| 32 | `codeflow test clean --artifacts` | -- | -- | -- | -- | |
| 33 | Deterministic config writer (stable serialization) | -- | -- | -- | -- | |
| 34 | LLM cannot directly Edit `test-config.json` (ProtectionGuard preserved) | -- | -- | -- | -- | |
| 35 | `.state/**` and `test-config.json` invariant stated + proved via matrix | -- | -- | -- | -- | |
| 36 | PR-body emitter with five sections + target-aware tables | -- | -- | -- | -- | |
| 37 | Test Failures + Slowest Tests sections sourced from CTRF | -- | -- | -- | -- | |
| 38 | Modified-file-coverage routing by cwd prefix, Unattributed fail-closed | -- | -- | -- | -- | |
| 39 | Full suite passes with per-file coverage ≥85%, clippy clean, fmt passes | -- | -- | -- | -- | |
| 40 | Concurrent-worktree integration test passes | -- | -- | -- | -- | |
| 41 | JUnit parse failure produces structured error, no panic | -- | -- | -- | -- | |
| 42 | Missing coverage artifact degrades gracefully with warning | -- | -- | -- | -- | |
| 43 | Schema violation prints JSON-path + exit code 2 | -- | -- | -- | -- | |
| 44 | Missing mode silently skips target | -- | -- | -- | -- | |
| 45 | Grep verification: no direct `.state/**` Edit/Write paths in engine code | -- | -- | -- | -- | |
| 46 | `--format json` output schema stable, JSON for programmatic consumers not PR body | -- | -- | -- | -- | |
| 47 | enforcement-policy.json updated with 2 new patterns in both arrays | -- | -- | -- | -- | |
| 48 | Schema-evolution policy (5 behaviors) with unit tests | -- | -- | -- | -- | |
| 49 | Modified-file routing 6-rule algorithm + §9.5 examples table tests | -- | -- | -- | -- | |
| 50 | coverage.transform non-zero exit degrades to N/A, warns, does not fail | -- | -- | -- | -- | |

## Dependencies

### Blocked By

- INF-TSK-046-001 (plan)

### Blocks

- INF-TSK-046-003 (setup wizard depends on config loader + schema)
- INF-TSK-046-004 (artifact generalization depends on the universal `codeflow test` CLI surface being final)
- INF-TSK-046-005 (self-host migration depends on engine existing)

## Verification

### Automated

- [ ] `codeflow test --mode full --coverage` passes (after self-host migrates via TSK-005; during TSK-002 development, `cargo nextest run --profile full` + `cargo llvm-cov` directly)
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt --check --all` passes
- [ ] Integration test `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` passes
- [ ] Golden-fixture tests pass for cargo-nextest, pytest-junitxml, jest-junit, vitest JUnit dialects
- [ ] Golden-fixture tests pass for lcov, cobertura, istanbul-summary, go-cover parsers
- [ ] Grep verification: `rg '\.state/(test-reports|coverage|ledger)' codeflow-cli/ --type rust | rg -v 'WorktreePaths|ledger::' ` returns zero results

### Manual

- [ ] Run `codeflow test config show` on a live checkout and verify JSON output matches schema
- [ ] Run `codeflow test --mode full --coverage` end-to-end on Scenario A (empty targets) — confirm single-line notice, exit 0
- [ ] Run `codeflow test report convert --from junit --to ctrf --input <fixture> --output /tmp/out.ctrf.json` — verify output validates against CTRF schema
- [ ] Start two sessions in two worktrees; run `codeflow test --mode full --coverage` in each concurrently — verify `.state/test-reports/` contents are independent

## Stage Reports

### DEV Report

> Populated by cf-development before STAGE-COMPLETE: WS-DEV

**Implementation Summary:**
{TBD by cf-development}

**Files Changed:**

| File | Action | Lines | Description |
|------|--------|-------|-------------|

**Test Stats (draft — verified by WS-QA)**

#### 1. Pass Status

| Suite | Passed | Failed | Skipped |
|-------|--------|--------|---------|

#### 2. Workspace Coverage

| Crate | Coverage | Threshold | Status |
|-------|----------|-----------|--------|

#### 3. Modified File Coverage

| File | Coverage | Threshold | Status |
|------|----------|-----------|--------|

#### Code Path Audit

| Entry Point | Path Type | Path Description | Outcome | Verified |
|-------------|-----------|------------------|---------|----------|

**Unhandled paths identified and fixed:** {count}
**Silent failure check:** {result}
**Resource cleanup verification:** {result}
**Integration chain:** {upstream/downstream verification result}

**Deviations from Approach:**
{None, or description}

### SEC Report

> Populated by cf-security before STAGE-COMPLETE: WS-SEC

**Verdict:** {PASS | FAIL}
**Scope:** {files scanned}

#### OWASP Checklist

| # | Category | Result | Evidence |
|---|----------|--------|----------|
| A01 | Broken Access Control | {PASS/FAIL/N/A} | |
| A02 | Cryptographic Failures | {PASS/FAIL/N/A} | |
| A03 | Injection | {PASS/FAIL/N/A} | |
| A04 | Insecure Design | {PASS/FAIL/N/A} | |
| A05 | Security Misconfiguration | {PASS/FAIL/N/A} | |
| A06 | Vulnerable Components | {PASS/FAIL/N/A} | |
| A07 | Authentication Failures | {PASS/FAIL/N/A} | |
| A08 | Data Integrity Failures | {PASS/FAIL/N/A} | |
| A09 | Logging & Monitoring | {PASS/FAIL/N/A} | |
| A10 | SSRF | {PASS/FAIL/N/A} | |

**Note for SEC:** The shell-command runner (`TargetRunner`) accepts arbitrary shell strings from `test-config.json`. Since `test-config.json` is ProtectionGuard-protected, LLMs cannot inject commands by editing it directly — mutations flow through `codeflow test config set-command` with a basic shell-lint (unbalanced-quote rejection). Review whether the shell-lint is sufficient defence-in-depth (A03 Injection) or whether a `command: ["argv", "array"]` alternate form should be introduced.

**Confidence Score:** {0-100} -- {brief rationale}

### REV Report

> Populated by cf-review before STAGE-COMPLETE: WS-REV

**Review Mode:** CODE_REVIEW
**Verdict:** {APPROVED | CHANGES_REQUESTED}

### QA Report

> Populated by cf-quality-assurance before STAGE-COMPLETE: WS-QA
> All three Test Stats sections REQUIRED.

**Verdict:** {PASS | FAIL}
**Runner Mode:** full

#### 1. Overall Test Pass Status
#### 2. Overall Coverage
#### 3. Modified File Coverage

### Confidence Score

| Stage | Agent | Score (0-100) | Rationale |
|-------|-------|--------------|-----------|
| WS-DEV | cf-development | {n} | {brief rationale} |
| WS-SEC | cf-security | {n} | {brief rationale} |
| WS-REV | cf-review | {n} | {brief rationale} |
| WS-QA | cf-quality-assurance | {n} | {brief rationale} |

## Notes

Design doc: `.codeflow/docs/analysis/generic-testing-subsystem.md` §§6-16.
ADR: `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md`.

**Edge cases to watch:**

- JUnit dialect drift (pytest `classname` includes package path; nextest uses `::` separator) — fixtures cover both.
- CTRF `duration` is milliseconds; JUnit `@time` is seconds — converter must multiply.
- Coverage paths in lcov `SF:` records can be absolute or relative; normalize to repo-relative before threshold evaluation.
- `git diff` can produce renamed-file entries (`R100 old.rs new.rs`); modified-file-coverage routing uses the new path.
- Windows paths in test-config.json `command` strings will fail in v1 — document POSIX-shell-only constraint in a doc-comment on the config loader.
- `execution.parallel: true` with targets whose commands write to the same `report.path` (absolute path) is an adopter error; doctor detects this and warns.
- `test-config.json` location moves from `codeflow-cli/config/testing/` to `.codeflow/config/testing/` in TSK-005; TSK-002 creates the new path but reads from the old path as a fallback for self-host during transition.
