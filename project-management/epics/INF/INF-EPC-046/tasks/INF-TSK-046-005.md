---
id: "task-01KP0WRNT0RXX13V76P7XD11KP"
format_id: "INF-TSK-046-005"
epic_id: "epic-01KP0WRNT08J88E09QGQ6185BP"
epic_format_id: "INF-EPC-046"
title: "Self-host migration + hardcode retirement"
description: "Migrate CodeFlow self-host to the generic testing engine as the first production adopter. Author CodeFlow's own .codeflow/config/testing/test-config.json using the new schema, add codeflow-cli/.config/nextest.toml with a full profile emitting JUnit, migrate the six existing coverage exceptions 1:1, verify byte-equivalent PR body output against the old engine (parity check), delete the legacy Rust-specific TestValidator::full_validate path, update .gitignore to exclude .state/test-reports/ and .state/coverage/, verify WS-QA passes on the new engine with zero regressions on existing shell/Python/Rust test suites."
status: todo
area_type: "INF"
work_type: "RFCT"
domain: "GENL"
origin: planned
file_scope:
  - ".codeflow/config/testing/test-config.json"
  - "codeflow-cli/config/testing/test-config.json"
  - "codeflow-cli/.config/nextest.toml"
  - "codeflow-cli/core/src/testing/validation.rs"
  - "codeflow-cli/core/src/testing/legacy_validation.rs"
  - "codeflow-cli/cli/src/cmd/test.rs"
  - ".gitignore"
  - ".codeflow/testing/run-all-tests.sh"
scope_policy: hard
scope_root: null
estimate: L
priority: high
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance:
  - ".codeflow/config/testing/test-config.json authored via `codeflow test setup --template example-rust.json` then manually customized (or via `codeflow test config` subcommands) — NEVER via direct Edit/Write; the file is created by CLI, not by LLM"
  - "Authored config contains schema_version '1.0', execution.parallel false, one target named 'rust-core' with runner 'cargo', cwd 'codeflow-cli', modes.essential.command invoking `cargo nextest run --profile essential`, modes.full.command invoking `cargo nextest run --profile full` AND generating coverage via `cargo llvm-cov --no-run --lcov --output-path lcov.info`"
  - "Authored config has report.format 'junit', report.path 'target/nextest/full/junit.xml', report.derive_from 'junit' — uses the engine's internal JUnit→CTRF converter"
  - "Authored config has coverage.format 'lcov', coverage.path 'lcov.info', coverage.rules with a single per_file rule on `**/*.rs` minimum 85"
  - "Authored config has six exceptions in coverage.exceptions[], matching the current codeflow-cli/config/testing/test-config.json conventions.exceptions[] 1:1 — byte-equivalent for file, threshold, reason, remove_when fields"
  - "Six migrated exceptions: core/src/autorun/worker.rs (79%), cli/src/cmd/interactive.rs (65%), cli/src/cmd/init.rs (70%), cli/src/cmd/autorun.rs (67%), core/src/tui/mod.rs (0%), core/src/tui/widgets/mod.rs (0%); each with the existing reason and remove_when text preserved character-for-character"
  - "codeflow-cli/.config/nextest.toml created (new file) with profiles 'essential' and 'full'; both profiles set `junit.path = 'target/nextest/{profile}/junit.xml'`; 'full' profile may additionally set runner options as needed (verbosity, timeout)"
  - "Running `codeflow test --mode full --coverage` at the repo root produces (a) exit code 0, (b) a PR-body markdown via `codeflow test report show --format json` or the PR-body emitter with the same test count, same failure count (zero), and same coverage percentages as the pre-migration `codeflow test --mode full --coverage` run on the same commit — byte-equivalent for the Overall Test Pass Status table and the Overall Coverage table (modulo formatting whitespace)"
  - "Parity check: the migration PR includes a diff showing the PR body before (old engine) and after (new engine) on the same base commit; the Overall Test Pass Status and Overall Coverage tables are semantically identical (same numbers, possibly different layout); reviewer signs off"
  - "Legacy path `TestValidator::full_validate` in codeflow-cli/core/src/testing/validation.rs (or legacy_validation.rs if renamed by TSK-002) is REMOVED entirely; any call site in codeflow-cli/cli/src/cmd/test.rs now routes to the new engine; no `#[cfg(feature = \"legacy-test-engine\")]` gates remain"
  - "codeflow-cli/config/testing/test-config.json (the OLD path) is DELETED by this PR; any code references to this path are updated to `.codeflow/config/testing/test-config.json`; grep of the repo for `codeflow-cli/config/testing` returns zero matches post-migration"
  - ".gitignore at repo root appends entries '.state/test-reports/' and '.state/coverage/' so these per-worktree artifacts are not committed; existing .state/* gitignore rules are preserved (no regression on .state/db/, .state/coordination/, etc.)"
  - "Grep verification: `rg 'cargo|llvm-cov|nextest' codeflow-cli/cli/src/cmd/test.rs codeflow-cli/core/src/testing/validation.rs` returns zero matches (legacy hardcodes gone); `rg 'cargo|llvm-cov|nextest' codeflow-cli/core/src/testing/runner/` is expected to still match (the new runner invokes the commands declared in test-config.json, but only via `std::process::Command` with the shell string — no static cargo references in the module's own code)"
  - "CodeFlow self-host WS-QA passes on the new engine: `codeflow test --mode full --coverage` produces (a) all existing tests pass (0 failures); (b) workspace coverage meets or exceeds pre-migration numbers; (c) exempted files match exactly (no new exemptions added, no exemptions removed); (d) modified file coverage ≥85% on every file touched by this PR"
  - "Shell and Python test suites under .codeflow/testing/ are preserved by declaring a SECOND target named 'shell-scripts' with runner: custom in the new test-config.json; the target's modes.essential.command invokes `bash .codeflow/testing/run-all-tests.sh --mode=essential --format=ctrf --output=$CODEFLOW_TEST_REPORT` and modes.full.command invokes the same with `--mode=full --coverage`; target.report.format: ctrf; target.report.path: ctrf.json; existing run-all-tests.sh, its supporting lib/, and all shell/Python test files stay in .codeflow/testing/ (NOT deleted); ONLY the Rust-specific TestValidator::full_validate entry point and the old codeflow-cli/config/testing/test-config.json are deleted in this PR; run-all-tests.sh is extended in this task to accept `--format=ctrf --output=<path>` flags emitting a CTRF JSON (scope: add flag parsing and a small emitter that walks the existing test result struct); zero regression count on the existing shell/Python suites"
  - "Rollback mechanism: the legacy path is NOT retained behind a feature flag in this PR; previous design had `CODEFLOW_TEST_ENGINE=legacy` — this is dropped in favor of clean cutover since TSK-005 is the self-host migration and the legacy path has served its purpose. If cutover fails, the PR is reverted in git"
  - "Documentation: the authored test-config.json is committed to the repo (it is the adopter's config, not an LLM-protected artifact from CodeFlow's perspective; rather, it is a LLM-protected artifact for ANY adopter). This PR's commits include the config as an adopter-style addition"
  - "test-config.json file is ProtectionGuard-covered via `.codeflow/**` — after migration, an LLM attempt to Edit .codeflow/config/testing/test-config.json is blocked; verified manually by an LLM agent attempting an edit during QA"
  - "No task deliverable introduces an LLM code path that Edit/Writes into .state/** or .codeflow/config/testing/test-config.json directly. Every mutation has a corresponding codeflow test <subcommand>. Migration uses CLI subcommands (`codeflow test setup --template`, `codeflow test exceptions add`) or manual authoring by a human operator — never LLM Edit/Write"
  - "Cutover sequence documented in the PR description: (1) apply template, (2) wire nextest.toml, (3) migrate exceptions, (4) run parity check, (5) delete old config + legacy path, (6) run full WS-QA, (7) verify .gitignore; reviewer can replay the sequence"
  - "Rollback plan documented in the PR description: 'If parity check fails or WS-QA regresses, git revert the PR and reopen TSK-005 with findings.' — simple, unambiguous"
  - "Integration test: the existing `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` from TSK-002 is updated to reference the new `.codeflow/config/testing/test-config.json` path (not the old `codeflow-cli/config/testing/test-config.json`); test passes"
  - "Lint: cargo clippy --all-targets --all-features -- -D warnings passes"
  - "Format: cargo fmt --check passes"
  - "Post-migration self-test: a second developer (or CI) runs `codeflow test --mode full --coverage` on a fresh clone of the repo and reproduces the same PR-body output — deterministic"
  - "Exception drift audit: run `codeflow test exceptions list --target rust-core` — output matches the six migrated entries; `codeflow test config show` — output shows the full authored config with no placeholders"
  - "Per-file coverage verification: run the engine's threshold evaluator on the post-migration coverage output — every file not in exceptions[] meets 85%; every file in exceptions[] meets its listed threshold; any drift is a FAIL"
tests:
  - "codeflow-cli/tests/integration_testing_parallel_worktrees.rs (updated from TSK-002 to point at new config path)"
  - "codeflow-cli/tests/integration_self_host_parity.rs (new: verifies PR-body output byte-equivalent to a pinned golden fixture captured from the old engine on a known-good commit)"
branch: null
pr_number: null
external_id: null
external_url: null
dependencies:
  - "task-01KP0WRNT0BEMCW2T9GQ9V43A0"
  - "task-01KP0WRNT053AJKKKJWTZZHQ3R"
  - "task-01KP0WRNT0XMXFQQH549CCNNAR"
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-11T00:00:00Z"
started_at: null
completed_at: null
stage: null
stage_status: null
stage_history: "[]"
---

# INF-TSK-046-005: Self-host migration + hardcode retirement

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

Migrate CodeFlow self-host to the generic testing engine. Author CodeFlow's own `.codeflow/config/testing/test-config.json` (using `codeflow test setup --template example-rust.json` as the starting point, then customizing via CLI), add `codeflow-cli/.config/nextest.toml` with `essential` and `full` profiles emitting JUnit, migrate the six existing coverage exceptions 1:1, run a byte-equivalence parity check against the old engine's PR-body output, delete the legacy `TestValidator::full_validate` path, and delete the old `codeflow-cli/config/testing/test-config.json` file. Append `.state/test-reports/` and `.state/coverage/` to `.gitignore`.

Zero regressions on existing shell/Python/Rust test suites. Cutover is clean — no feature flag retained for the legacy path. If parity check fails, git revert.

## Deliverables

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| Authored `test-config.json` | config | `.codeflow/config/testing/test-config.json` | The new canonical config for CodeFlow self-host |
| nextest profile config | config | `codeflow-cli/.config/nextest.toml` | cargo-nextest reads per-profile settings incl. junit.path |
| Legacy path removal | refactor | `codeflow-cli/core/src/testing/validation.rs` (or legacy_validation.rs) + cli/src/cmd/test.rs | Clean cutover; no dual path |
| Old config deletion | refactor | `codeflow-cli/config/testing/test-config.json` (DELETED) | Remove stale config from codebase |
| `.gitignore` update | config | `.gitignore` | Ephemeral test artifacts not committed |
| Parity fixture + integration test | test | `codeflow-cli/tests/integration_self_host_parity.rs` | Regression guard |

**Expected Outcome:** After this task, CodeFlow self-host is the first adopter of its own generic testing engine. `codeflow test --mode full --coverage` produces the same PR-body output as before, but through the new engine. The legacy Rust-specific code path is gone. The config is at the canonical `.codeflow/config/testing/` location. `.state/test-reports/` and `.state/coverage/` are gitignored.

**Deployment:** PR merge on `refactor/self-host-testing-migration`. If parity check or WS-QA fails, revert and reopen with findings.

## Pre-Work Analysis

- [ ] Reviewed git log for recent changes to `codeflow-cli/config/testing/test-config.json` (exceptions list may have drifted)
- [ ] Confirmed TSK-002, TSK-003, TSK-004 all merged
- [ ] Ran `codeflow test --mode full --coverage` on the base commit (before migration) and captured the PR-body output as a golden fixture
- [ ] Noted the exact six exception entries and their current coverage thresholds

## Approach

1. **Capture parity baseline.** On the base commit (before migration), run the OLD `codeflow test --mode full --coverage` and save the full PR-body markdown to `codeflow-cli/tests/fixtures/self_host_pr_body_pre_migration.md`. This is the golden fixture.
2. **Author test-config.json.** Run `codeflow test setup --template example-rust.json --force`. Then customize via `codeflow test config` subcommands:
   - `codeflow test config set-command --target rust-core --mode essential --command 'cargo nextest run --profile essential'`
   - `codeflow test config set-command --target rust-core --mode full --command 'cargo nextest run --profile full && cargo llvm-cov --no-run --lcov --output-path lcov.info'`
   - `codeflow test config set-report --target rust-core --format junit --path target/nextest/full/junit.xml --derive-from junit`
   - `codeflow test config set-coverage --target rust-core --format lcov --path lcov.info`
3. **Migrate exceptions** via `codeflow test exceptions add` × 6 — one per existing exception, preserving reason and remove_when text character-for-character. Verify with `codeflow test exceptions list --target rust-core`.
4. **Author `codeflow-cli/.config/nextest.toml`.** Add profiles `essential` and `full` with `junit.path = "target/nextest/{profile}/junit.xml"`.
5. **Run engine locally.** `codeflow test --mode full --coverage`. Capture PR-body output to `codeflow-cli/tests/fixtures/self_host_pr_body_post_migration.md`.
6. **Parity diff.** Run semantic diff between pre- and post-migration PR-body outputs. Expect: same test counts, same failure count (0), same coverage percentages, same six exempted files with same thresholds. Whitespace differences allowed; semantic drift is a FAIL.
7. **Delete legacy path, preserve shell-scripts target.** Remove `TestValidator::full_validate` in `validation.rs` (or `legacy_validation.rs` from TSK-002's transient stub). Remove any `CODEFLOW_TEST_ENGINE=legacy` gating code. Update `cli/src/cmd/test.rs` to route solely to the new engine. **Do NOT rip out `.codeflow/testing/run-all-tests.sh` or its `lib/` helpers** — they stay as the backing for the `shell-scripts` target with `runner: custom` (per AC#15). Extend `run-all-tests.sh` to accept `--format=ctrf --output=<path>` flags emitting CTRF JSON; this is the only shell edit this task performs.
8. **Delete old config file.** `git rm codeflow-cli/config/testing/test-config.json`.
9. **Update code references.** Grep for `codeflow-cli/config/testing` across the repo; replace every occurrence with `.codeflow/config/testing`.
10. **Update `.gitignore`.** Append `.state/test-reports/` and `.state/coverage/`. Verify no existing `.state/*` rules regress.
11. **Update integration tests.** Adjust `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` (from TSK-002) to reference the new config path.
12. **Author parity integration test.** `codeflow-cli/tests/integration_self_host_parity.rs` reads the captured golden fixture and runs the new engine, asserting semantic equivalence (counts, coverage, exempt files).
13. **Full WS-QA run.** `codeflow test --mode full --coverage` — verify zero failures, zero regressions.
14. **Document cutover.** In the PR description, include the step-by-step sequence, the parity diff summary, and the rollback plan.

## Standards & Practices

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Rust | cf-rust-standards | No unsafe, clippy `-D warnings` clean, thiserror for errors |
| JSON | cf-markdown-standards (JSON section) | Valid, schema-validated, deterministic serialization |
| TOML (nextest.toml) | (no skill) | Valid TOML, documented profile sections |

Read the applicable skill BEFORE starting implementation.

## Files

### To Create

- `.codeflow/config/testing/test-config.json` — new canonical config (via CLI, not Edit/Write)
- `codeflow-cli/.config/nextest.toml` — nextest profiles emitting JUnit
- `codeflow-cli/tests/fixtures/self_host_pr_body_pre_migration.md` — parity baseline (captured before migration)
- `codeflow-cli/tests/fixtures/self_host_pr_body_post_migration.md` — parity result (captured after migration)
- `codeflow-cli/tests/integration_self_host_parity.rs` — regression guard

### To Modify

- `codeflow-cli/core/src/testing/validation.rs` (or legacy_validation.rs) — remove legacy path
- `codeflow-cli/cli/src/cmd/test.rs` — remove routing to legacy path
- `.gitignore` — append `.state/test-reports/` and `.state/coverage/`
- `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` — update config path reference
- Any Rust source referencing `codeflow-cli/config/testing` — grep and replace to `.codeflow/config/testing`

### To Delete

- `codeflow-cli/config/testing/test-config.json` — moved to `.codeflow/config/testing/`

### To Read

- Current `codeflow-cli/config/testing/test-config.json` (before deletion) — source of truth for the six exceptions
- `.codeflow/docs/analysis/generic-testing-subsystem.md` §§12.6 (Scenario F), 17 (Migration plan), 11 (PR body format)
- `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md` — decision rationale

## Concurrency Considerations

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| `.codeflow/config/testing/test-config.json` | write via CLI subcommands | TSK-002 config-writer atomic temp+rename |
| `.state/test-reports/` / `.state/coverage/` | write-once per run, per-worktree | Per-worktree LOCAL per TSK-002 |

## Acceptance Criteria

1. `.codeflow/config/testing/test-config.json` authored via `codeflow test setup --template example-rust.json` then manually customized (or via `codeflow test config` subcommands) — NEVER via direct Edit/Write; the file is created by CLI, not by LLM
2. Authored config contains `schema_version '1.0'`, `execution.parallel false`, one target named 'rust-core' with runner 'cargo', cwd 'codeflow-cli', `modes.essential.command` invoking `cargo nextest run --profile essential`, `modes.full.command` invoking `cargo nextest run --profile full` AND generating coverage via `cargo llvm-cov --no-run --lcov --output-path lcov.info`
3. Authored config has `report.format 'junit'`, `report.path 'target/nextest/full/junit.xml'`, `report.derive_from 'junit'` — uses the engine's internal JUnit→CTRF converter
4. Authored config has `coverage.format 'lcov'`, `coverage.path 'lcov.info'`, `coverage.rules` with a single per_file rule on `**/*.rs` minimum 85
5. Authored config has six exceptions in `coverage.exceptions[]`, matching the current `codeflow-cli/config/testing/test-config.json` `conventions.exceptions[]` 1:1 — byte-equivalent for file, threshold, reason, remove_when fields
6. Six migrated exceptions: `core/src/autorun/worker.rs` (79%), `cli/src/cmd/interactive.rs` (65%), `cli/src/cmd/init.rs` (70%), `cli/src/cmd/autorun.rs` (67%), `core/src/tui/mod.rs` (0%), `core/src/tui/widgets/mod.rs` (0%); each with the existing reason and remove_when text preserved character-for-character
7. `codeflow-cli/.config/nextest.toml` created (new file) with profiles 'essential' and 'full'; both profiles set `junit.path = 'target/nextest/{profile}/junit.xml'`; 'full' profile may additionally set runner options as needed (verbosity, timeout)
8. Running `codeflow test --mode full --coverage` at the repo root produces (a) exit code 0, (b) a PR-body markdown via `codeflow test report show --format json` or the PR-body emitter with the same test count, same failure count (zero), and same coverage percentages as the pre-migration run on the same commit — byte-equivalent for the Overall Test Pass Status table and the Overall Coverage table (modulo formatting whitespace)
9. Parity check: the migration PR includes a diff showing the PR body before (old engine) and after (new engine) on the same base commit; the Overall Test Pass Status and Overall Coverage tables are semantically identical (same numbers, possibly different layout); reviewer signs off
10. Legacy path `TestValidator::full_validate` in `codeflow-cli/core/src/testing/validation.rs` (or legacy_validation.rs if renamed by TSK-002) is REMOVED entirely; any call site in `codeflow-cli/cli/src/cmd/test.rs` now routes to the new engine; no `#[cfg(feature = "legacy-test-engine")]` gates remain
11. `codeflow-cli/config/testing/test-config.json` (the OLD path) is DELETED by this PR; any code references to this path are updated to `.codeflow/config/testing/test-config.json`; grep of the repo for `codeflow-cli/config/testing` returns zero matches post-migration
12. `.gitignore` at repo root appends entries `.state/test-reports/` and `.state/coverage/` so these per-worktree artifacts are not committed; existing `.state/*` gitignore rules are preserved (no regression on `.state/db/`, `.state/coordination/`, etc.)
13. Grep verification: `rg 'cargo|llvm-cov|nextest' codeflow-cli/cli/src/cmd/test.rs codeflow-cli/core/src/testing/validation.rs` returns zero matches (legacy hardcodes gone); `rg 'cargo|llvm-cov|nextest' codeflow-cli/core/src/testing/runner/` is expected to still match (the new runner invokes the commands declared in `test-config.json`, but only via `std::process::Command` with the shell string — no static cargo references in the module's own code)
14. CodeFlow self-host WS-QA passes on the new engine: `codeflow test --mode full --coverage` produces (a) all existing tests pass (0 failures); (b) workspace coverage meets or exceeds pre-migration numbers; (c) exempted files match exactly (no new exemptions added, no exemptions removed); (d) modified file coverage ≥85% on every file touched by this PR
15. Shell and Python test suites under `.codeflow/testing/` are preserved by declaring a **second target named `shell-scripts`** with `runner: custom` in the new `test-config.json`. Target config: `modes.essential.command` = `bash .codeflow/testing/run-all-tests.sh --mode=essential --format=ctrf --output=$CODEFLOW_TEST_REPORT`; `modes.full.command` = same with `--mode=full --coverage`; `report.format: ctrf`; `report.path: ctrf.json`. Existing `run-all-tests.sh`, its supporting `lib/`, and all shell/Python test files stay in `.codeflow/testing/` (NOT deleted). **Only** the Rust-specific `TestValidator::full_validate` entry point and the old `codeflow-cli/config/testing/test-config.json` are deleted in this PR. `run-all-tests.sh` is extended in this task to accept `--format=ctrf --output=<path>` flags emitting a CTRF JSON (scope: add flag parsing and a small emitter walking the existing test-result struct). Zero regression count on the existing shell/Python suites verified via parity diff against pre-migration output.
16. Rollback mechanism: **clean cutover — no feature flag retained in this PR.** The earlier `CODEFLOW_TEST_ENGINE=legacy` env-flag idea is explicitly rejected; design doc §17.2 and ADR-001 §Consequences #Risks both align with this decision. If cutover fails, the PR is reverted via `git revert`. Rationale: feature flags become permanent debt; parity check (AC#9) catches divergences before merge; `git revert` is atomic and restores the known-good engine state with no partial-adoption risk.
17. Documentation: the authored `test-config.json` is committed to the repo (it is the adopter's config, not an LLM-protected artifact from CodeFlow's perspective; rather, it is an LLM-protected artifact for ANY adopter). This PR's commits include the config as an adopter-style addition
18. `test-config.json` file is ProtectionGuard-covered via `.codeflow/**` — after migration, an LLM attempt to Edit `.codeflow/config/testing/test-config.json` is blocked; verified manually by an LLM agent attempting an edit during QA
19. No task deliverable introduces an LLM code path that Edit/Writes into `.state/**` or `.codeflow/config/testing/test-config.json` directly. Every mutation has a corresponding `codeflow test <subcommand>`. Migration uses CLI subcommands (`codeflow test setup --template`, `codeflow test exceptions add`) or manual authoring by a human operator — never LLM Edit/Write
20. Cutover sequence documented in the PR description: (1) apply template, (2) wire nextest.toml, (3) migrate exceptions, (4) run parity check, (5) delete old config + legacy path, (6) run full WS-QA, (7) verify `.gitignore`; reviewer can replay the sequence
21. Rollback plan documented in the PR description: "If parity check fails or WS-QA regresses, git revert the PR and reopen TSK-005 with findings." — simple, unambiguous
22. Integration test: the existing `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` from TSK-002 is updated to reference the new `.codeflow/config/testing/test-config.json` path (not the old `codeflow-cli/config/testing/test-config.json`); test passes
23. Lint: `cargo clippy --all-targets --all-features -- -D warnings` passes
24. Format: `cargo fmt --check` passes
25. Post-migration self-test: a second developer (or CI) runs `codeflow test --mode full --coverage` on a fresh clone of the repo and reproduces the same PR-body output — deterministic
26. Exception drift audit: run `codeflow test exceptions list --target rust-core` — output matches the six migrated entries; `codeflow test config show` — output shows the full authored config with no placeholders
27. Per-file coverage verification: run the engine's threshold evaluator on the post-migration coverage output — every file not in `exceptions[]` meets 85%; every file in `exceptions[]` meets its listed threshold; any drift is a FAIL

### Cutover step sequence (implementation blueprint)

| Step | Command / Action | Expected Output |
|------|-----------------|-----------------|
| 1 | `codeflow test --mode full --coverage > /tmp/pre_migration.log` (OLD engine, base commit) | Exit 0, capture PR body |
| 2 | Save `/tmp/pre_migration.log` → `codeflow-cli/tests/fixtures/self_host_pr_body_pre_migration.md` | Golden fixture |
| 3 | `codeflow test setup --template example-rust.json --force` | `.codeflow/config/testing/test-config.json` created |
| 4 | Customize commands via `codeflow test config set-command` × 2 (essential, full) | Config updated |
| 5 | Customize report/coverage via `codeflow test config set-report` + `set-coverage` | Config updated |
| 6 | Migrate exceptions via `codeflow test exceptions add` × 6 | Six exceptions present |
| 7 | Create `codeflow-cli/.config/nextest.toml` with essential + full profiles | File exists |
| 8 | `codeflow test --mode full --coverage > /tmp/post_migration.log` (NEW engine, same commit) | Exit 0, capture PR body |
| 9 | Save `/tmp/post_migration.log` → `codeflow-cli/tests/fixtures/self_host_pr_body_post_migration.md` | Result fixture |
| 10 | Run semantic diff: test counts, coverage %, exempted-files list | Zero drift |
| 11 | Delete `codeflow-cli/core/src/testing/legacy_validation.rs` (or equivalent) | File gone |
| 12 | Delete `codeflow-cli/config/testing/test-config.json` | File gone |
| 13 | Update `codeflow-cli/cli/src/cmd/test.rs` to remove legacy routing | Code clean |
| 14 | Append to `.gitignore` | `.state/test-reports/` + `.state/coverage/` present |
| 15 | Update `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` config path | Test passes |
| 16 | Create `codeflow-cli/tests/integration_self_host_parity.rs` | Test passes |
| 17 | Run `codeflow test --mode full --coverage` — final WS-QA | Exit 0, no regressions |

### Rollback plan

If any step 8-17 fails:

1. Do NOT partially commit. All changes are atomic in one PR.
2. Revert all local changes: `git reset --hard HEAD` (pre-migration state).
3. Reopen TSK-005 with the specific failure captured (parity delta, test regression, coverage drop).
4. Do NOT attempt incremental partial migration — the engine has one config source of truth; partial state will corrupt future runs.

### Scenarios covered

| # | Scenario | Covered by acceptance # |
|---|----------|------------------------|
| 1 | Author new config via CLI (never Edit/Write) | 1, 19 |
| 2 | Preserve 6 coverage exceptions 1:1 | 5, 6 |
| 3 | Parity with old engine on Test Pass Status | 8, 9 |
| 4 | Parity on coverage percentages | 8, 9 |
| 5 | Legacy path deletion | 10 |
| 6 | Old config file deletion | 11 |
| 7 | Code grep shows no lingering `codeflow-cli/config/testing` refs | 11 |
| 8 | `.gitignore` updated for ephemeral artifacts | 12 |
| 9 | Zero test regressions | 14 |
| 10 | Shell/Python test suites continue to work | 15 |
| 11 | ProtectionGuard still blocks direct LLM Edit on new config | 18 |
| 12 | Fresh-clone reproducibility | 25 |
| 13 | Exception drift audit | 26 |
| 14 | Per-file coverage threshold check | 27 |
| 15 | Rollback works if parity fails | 16, 21 |

### PII Handling Review

- [ ] Does this task involve code that handles PII? (N)

### Criteria Status

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DEV -> WS-SEC -> WS-REV -> WS-QA

| # | Criterion | DEV | SEC | REV | QA | Notes |
|---|-----------|-----|-----|-----|-----|-------|
| 1 | Config authored via CLI, not Edit/Write | -- | -- | -- | -- | |
| 2 | Config shape matches Scenario F (runner, modes, cwd) | -- | -- | -- | -- | |
| 3 | Report config uses JUnit derive_from | -- | -- | -- | -- | |
| 4 | Coverage config uses lcov + per_file rule | -- | -- | -- | -- | |
| 5 | Six exceptions migrated 1:1 | -- | -- | -- | -- | |
| 6 | Exception fields byte-equivalent | -- | -- | -- | -- | |
| 7 | `codeflow-cli/.config/nextest.toml` created with profiles | -- | -- | -- | -- | |
| 8 | `codeflow test --mode full --coverage` parity on counts + coverage | -- | -- | -- | -- | |
| 9 | Parity diff documented in PR | -- | -- | -- | -- | |
| 10 | Legacy `TestValidator::full_validate` removed | -- | -- | -- | -- | |
| 11 | Old `codeflow-cli/config/testing/test-config.json` deleted + grep clean | -- | -- | -- | -- | |
| 12 | `.gitignore` appends 2 entries, existing rules preserved | -- | -- | -- | -- | |
| 13 | Legacy cargo/nextest grep clean in cmd/test.rs + validation.rs | -- | -- | -- | -- | |
| 14 | WS-QA passes: 0 failures, coverage >= baseline, exemption match | -- | -- | -- | -- | |
| 15 | Shell/Python suites continue to run (if applicable) | -- | -- | -- | -- | |
| 16 | No feature flag for legacy; clean cutover | -- | -- | -- | -- | |
| 17 | Authored config committed as part of PR | -- | -- | -- | -- | |
| 18 | ProtectionGuard blocks LLM Edit on new config — verified | -- | -- | -- | -- | |
| 19 | `.state/` + test-config.json invariant preserved | -- | -- | -- | -- | |
| 20 | PR description documents cutover sequence | -- | -- | -- | -- | |
| 21 | PR description documents rollback plan | -- | -- | -- | -- | |
| 22 | Parallel-worktree integration test updated for new config path | -- | -- | -- | -- | |
| 23 | clippy clean | -- | -- | -- | -- | |
| 24 | fmt check passes | -- | -- | -- | -- | |
| 25 | Fresh-clone self-test reproduces output | -- | -- | -- | -- | |
| 26 | Exception drift audit passes | -- | -- | -- | -- | |
| 27 | Per-file coverage evaluator confirms thresholds | -- | -- | -- | -- | |

## Dependencies

### Blocked By

- INF-TSK-046-002 (engine must exist)
- INF-TSK-046-003 (setup wizard + `example-rust.json` template must exist)
- INF-TSK-046-004 (task template updates must be in place so this PR's task doc aligns with the new format)

### Blocks

- None (final task in the epic)

## Verification

### Automated

- [ ] `codeflow test --mode full --coverage` exits 0 on post-migration repo state
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt --check --all` passes
- [ ] `codeflow-cli/tests/integration_self_host_parity.rs` passes (semantic diff against golden fixture)
- [ ] `codeflow-cli/tests/integration_testing_parallel_worktrees.rs` passes with new config path
- [ ] `rg 'codeflow-cli/config/testing'` returns zero matches
- [ ] `rg 'cargo|llvm-cov|nextest' codeflow-cli/cli/src/cmd/test.rs codeflow-cli/core/src/testing/validation.rs` returns zero matches
- [ ] `codeflow test exceptions list --target rust-core` outputs exactly six entries matching the pre-migration list

### Manual

- [ ] Attempt to Edit `.codeflow/config/testing/test-config.json` as an LLM — verify ProtectionGuard blocks the Edit
- [ ] On a fresh clone (separate working tree), run `codeflow test --mode full --coverage` and verify the PR body is byte-equivalent to the captured post-migration fixture
- [ ] Review the parity diff document included in the PR description with a teammate
- [ ] Confirm the `.gitignore` changes do not inadvertently ignore anything new

## Stage Reports

### DEV Report

> Populated by cf-development before STAGE-COMPLETE: WS-DEV

**Implementation Summary:**
{TBD}

**Files Changed:**

| File | Action | Lines | Description |
|------|--------|-------|-------------|

**Deviations from Approach:** None

### SEC Report

> Populated by cf-security before STAGE-COMPLETE: WS-SEC

**Verdict:** {PASS | FAIL}

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

Design doc: `.codeflow/docs/analysis/generic-testing-subsystem.md` §§12.6 (Scenario F), 17 (Migration plan).
ADR: `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md`.

**Edge cases to watch:**

- The existing `codeflow-cli/config/testing/test-config.json` may drift between now (TSK-001 authoring) and when TSK-005 begins. The six-exception list should be re-verified at TSK-005 start and the task's acceptance #5/#6 updated if additional exceptions have been added.
- Running `codeflow test --mode full --coverage` on the post-migration state may produce slightly different coverage percentages due to nondeterminism in coverage tools (rare but observed). Semantic parity means the exempted-file set and the per-file threshold PASSes match — not necessarily identical percentage values. Reviewer judgment call if percentages drift by <0.5%.
- The old `run-all-tests.sh` shell test runner may still need to be invoked — either as a `runner: custom` target in the new config or removed entirely if all shell/Python tests have been migrated to Rust equivalents. Decide at implementation time based on current `.codeflow/testing/` contents.
- `codeflow-cli/.config/nextest.toml` must not conflict with any existing `nextest.toml` at the root; verify before creating.
- If the parity check shows coverage drop > 0.5% without explanation, halt the migration and investigate; do NOT paper over by adding new exceptions.
