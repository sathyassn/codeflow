---
id: "task-01KP0WRNT053AJKKKJWTZZHQ3R"
format_id: "INF-TSK-046-003"
epic_id: "epic-01KP0WRNT08J88E09QGQ6185BP"
epic_format_id: "INF-EPC-046"
title: "Setup wizard + doctor + template library"
description: "Implement codeflow test setup interactive wizard (with --auto, --template, --list, --add-target), codeflow test doctor validator (schema + dry-run + path checks), and the template library under .codeflow/templates/test-config/ (minimal, single-target variants, monorepo, hooks-escape-hatch, and per-stack examples for Node/Go/Python/Rust). Extends codeflow init with the optional 'Set up testing now?' prompt. All writes to test-config.json go through the TSK-002 config-writer."
status: todo
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
origin: planned
file_scope:
  - "codeflow-cli/cli/src/cmd/test.rs"
  - "codeflow-cli/cli/src/cmd/init.rs"
  - "codeflow-cli/core/src/testing/setup/**"
  - "codeflow-cli/core/src/testing/doctor/**"
  - ".codeflow/templates/test-config/**"
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
  - "codeflow test setup command exists in codeflow-cli/cli/src/cmd/test.rs with flags --auto, --template <name>, --list, --add-target, --force; when no flags given, runs interactive wizard"
  - "Interactive wizard (setup module at codeflow-cli/core/src/testing/setup/mod.rs) detects existing config and asks Edit/Replace/Abort; asks user to confirm each detected target; walks per-target through mode commands, report config, coverage config; runs doctor validation at the end; fails back to edit prompts on validation error"
  - "Wizard calls the TSK-002 config-writer (not Edit/Write) to persist test-config.json; verified by grep — no fs::write call directly to the config path in the setup module"
  - "--auto detection priority (in codeflow-cli/core/src/testing/setup/detect.rs) checks in order: Cargo.toml at root or Cargo.toml with [workspace] members → rust-core target; package.json (with devDependencies or dependencies containing vitest) → web target with vitest commands; package.json (with jest in deps) → web target with jest commands; go.mod → go-service target; pyproject.toml or setup.py (with pytest in deps) → python target; each detection is covered by a unit test with a temporary directory fixture"
  - "--auto emits best-guess commands per stack per design doc §15.3 mapping table; fallback to JUnit + derive_from:junit when no native CTRF reporter is detected in the package's deps"
  - "--template <name> writes one of the named templates from .codeflow/templates/test-config/; refuses overwrite unless --force is passed; valid template names enumerated in --list output"
  - "--list prints template names one per line to stdout, sorted alphabetically, with a one-line description each; used by shell-completion and scripting"
  - "--add-target walks the per-target wizard only (skips the overall-config prompts) and appends to an existing config; refuses if no config exists (instructs user to run `codeflow test setup` first)"
  - "Template library ships 9 files in .codeflow/templates/test-config/: minimal.json, single-target-basic.json, single-target-with-coverage.json, monorepo-multi-target.json, hooks-escape-hatch.json, example-node.json, example-python.json, example-go.json, example-rust.json; every template validates against .codeflow/schemas/test-config.schema.json; every template has a header comment field '// description' in a dedicated '_description' top-level key documenting its use case"
  - "minimal.json: empty targets[] array, schema_version 1.0 — matches the fresh-project no-tests state"
  - "single-target-basic.json: one target (name 'app') with essential+full modes, no coverage — simplest non-empty config"
  - "single-target-with-coverage.json: one target (name 'app') with essential+full modes + lcov coverage + per-file + changed_files rules; includes a sample exception entry with reason + remove_when"
  - "monorepo-multi-target.json: three targets (api-go, web-node, tools-python) with execution.parallel: true; each target has distinct cwd"
  - "hooks-escape-hatch.json: one target with runner: custom, using a shell script at .codeflow/scripts/run-tests.sh as the command — demonstrates the escape hatch"
  - "example-node.json: web target with vitest + vitest-ctrf-json-reporter native CTRF + istanbul-summary coverage"
  - "example-python.json: python target with pytest + --junitxml derive_from + cobertura coverage"
  - "example-go.json: go-service target with go test -json | go-ctrf-json-reporter + go-cover coverage"
  - "example-rust.json: rust-core target with cargo nextest --profile + JUnit derive_from + lcov coverage (matches Scenario F in design doc; used as the starting point by TSK-005)"
  - "codeflow test doctor command exists in codeflow-cli/core/src/testing/doctor/mod.rs and codeflow-cli/cli/src/cmd/test.rs"
  - "Doctor check 1 (config exists): verifies .codeflow/config/testing/test-config.json exists; exit 1 with clear message if missing"
  - "Doctor check 2 (schema validation): validates the config against .codeflow/schemas/test-config.schema.json using the same validator as TSK-002's config loader; reports JSON-path of first violation"
  - "Doctor check 3a (cwd exists): for each enabled target, verifies target.cwd resolves to an existing directory relative to repo root"
  - "Doctor check 3b (command parses as shell): for each mode's command in each enabled target, runs a basic POSIX-shell tokenizer (shell-words crate or equivalent) and reports parse errors"
  - "Doctor check 3c (report.path safe): for each target, verifies report.path is a relative path with no parent traversal (no .. components); absolute paths rejected"
  - "Doctor check 3d (coverage.path safe): for each target with coverage, same path safety check as 3c"
  - "Doctor check 3e (coverage.transform parses): for each target with coverage.transform, same shell parse check as 3b"
  - "Doctor check 3f (globs valid): for each coverage rule's include/exclude, compiles the glob via globset crate; reports invalid patterns"
  - "Doctor check 3g (exception file exists): for each coverage.exceptions[].file, checks file exists in the repo; warning (not error) if missing — exception may precede the file being created"
  - "Doctor check 4 (dry-run probe): for each enabled target's quick mode (if declared), invokes the runner's canonical probe with a 5-second per-probe timeout: cargo → cargo --version; pytest → pytest --version; jest → jest --version; vitest → vitest --version; go → go version; mocha → mocha --version; rspec → rspec --version; phpunit → phpunit --version; custom → no probe; probe failure (non-zero exit OR timeout) is a WARNING, not an error; timeout produces the warning message 'probe for runner=X exceeded 5s timeout; not blocking' — allows intentionally Docker-wrapped or slow-cold-start commands to pass; unit test covers both timeout and non-zero-exit paths"
  - "Doctor output: --format human (default) prints numbered sections with PASS/FAIL/WARN labels; --format json emits a structured report consumable by cf-knowledge-layer"
  - "Doctor exit codes: 0 (all checks pass or only warnings), 1 (one or more errors), 2 (infrastructure error — cannot read config at all)"
  - "Doctor runs automatically at end of `codeflow test setup` before writing the final config; fails back to edit prompts on error"
  - "codeflow init (in codeflow-cli/cli/src/cmd/init.rs) gains a 'Set up testing now?' prompt immediately before the existing finalization step; prompt offers [y/N/skip]; y runs `codeflow test setup`; N writes minimal.json template; skip does nothing; default is N"
  - "init prompt is skipped when --non-interactive flag is set (existing init flag); in that case, minimal.json is written (matching the default)"
  - "init prompt documented in init.rs: existing wizard flow is updated to account for the new step; existing init integration tests updated to cover each of the three branches (y, N, skip)"
  - "Wizard never invokes Edit/Write tool against test-config.json; uses the TSK-002 config-writer exclusively; verified by grep"
  - "No task deliverable introduces an LLM code path that Edit/Writes into .state/** or .codeflow/config/testing/test-config.json directly. Every mutation has a corresponding codeflow test <subcommand>. Setup wizard and doctor strictly use read/write through CLI internal APIs, not direct filesystem mutation of the protected paths."
  - "Unit tests for --auto detection: at least one test per sentinel file (Cargo.toml, package.json+vitest, package.json+jest, go.mod, pyproject.toml+pytest) using tempfile fixtures; one test for 'no sentinels found' case producing empty config"
  - "Unit tests for --template: every template in the library has a BYTE-EQUIVALENT round-trip test (read template file as bytes → load via schema → write via TSK-002 config-writer to a tempfile → re-read tempfile bytes → assert byte-equal to original); struct-equal is insufficient — byte-equal catches config-writer formatting drift (key order, indentation, trailing newline, quoting). The template files are themselves authored by the same config-writer so round-trip is well-defined."
  - "Unit tests for doctor checks: every check (1, 2, 3a-3g, 4) has a positive test (valid config passes) and a negative test (invalid config fails with specific message); at least 20 doctor test cases total"
  - "Interactive wizard testable via injectable `PromptProvider` trait (per the pattern used in init.rs); unit tests inject a scripted prompt provider to exercise the full wizard flow without a live terminal"
  - "Integration test: `codeflow test setup --template example-node.json --force` followed by `codeflow test doctor` on a fresh tempdir → both exit 0; resulting test-config.json byte-equivalent to the template (modulo absolute paths)"
  - "Integration test: `codeflow test setup --auto` on a tempdir containing a Cargo.toml and a package.json → produces a config with both rust-core and web targets; doctor passes"
  - "Integration test: `codeflow init --non-interactive` produces .codeflow/config/testing/test-config.json matching minimal.json byte-for-byte"
  - "Error path test: `codeflow test setup --template nonexistent.json` exits 1 with message 'Template not found. Run `codeflow test setup --list` to see available templates.'"
  - "Error path test: `codeflow test setup --template minimal.json` when test-config.json already exists exits 2 with 'Config already exists at <path>. Use --force to overwrite.'"
  - "Error path test: `codeflow test doctor` with a malformed JSON test-config.json exits 1 with a parse-error message pointing to line:column"
  - "Lint: cargo clippy --all-targets --all-features -- -D warnings passes on all new modules"
  - "Format: cargo fmt --check passes"
  - "Coverage: per-file ≥85% on setup/mod.rs, setup/detect.rs, doctor/mod.rs, and every CLI subcommand handler; any exceptions declared in test-config.json must have reason + remove_when"
tests:
  - "codeflow-cli/core/src/testing/setup/mod.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/setup/detect.rs (inline #[cfg(test)] mod tests)"
  - "codeflow-cli/core/src/testing/setup/wizard.rs (inline #[cfg(test)] mod tests with injectable PromptProvider)"
  - "codeflow-cli/core/src/testing/doctor/mod.rs (inline #[cfg(test)] mod tests per check)"
  - "codeflow-cli/cli/src/cmd/test.rs (subcommand-handler tests)"
  - "codeflow-cli/cli/src/cmd/init.rs (updated for the new prompt branches)"
  - "codeflow-cli/tests/integration_testing_setup.rs (new integration test: setup + doctor + template round-trip)"
branch: null
pr_number: null
external_id: null
external_url: null
dependencies:
  - "task-01KP0WRNT0BEMCW2T9GQ9V43A0"
created_at: "2026-04-11T00:00:00Z"
updated_at: "2026-04-11T00:00:00Z"
started_at: null
completed_at: null
stage: null
stage_status: null
stage_history: "[]"
---

# INF-TSK-046-003: Setup wizard + doctor + template library

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

Implement the three adopter-onboarding and validation surfaces that sit atop the TSK-002 engine:

1. **`codeflow test setup`** — interactive wizard (default), `--auto` best-guess, `--template <name>` apply-named, `--list` enumerate, `--add-target` append-only variant.
2. **`codeflow test doctor`** — 9-check validator (config exists, schema, per-target cwd/command/path/transform/glob/exception, dry-run probe).
3. **Template library** at `.codeflow/templates/test-config/` — 9 templates covering fresh project, single-target variants, monorepo, custom-runner escape hatch, and per-stack examples for Node/Go/Python/Rust.

Extends `codeflow init` with an optional "Set up testing now?" prompt. All `test-config.json` mutations route through the TSK-002 config-writer — LLMs never Edit/Write the protected file directly.

## Deliverables

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| Setup wizard | feature | `codeflow-cli/core/src/testing/setup/` + `cli/src/cmd/test.rs` | Adopter onboarding; first-run UX |
| Doctor validator | feature | `codeflow-cli/core/src/testing/doctor/` + `cli/src/cmd/test.rs` | Config drift detection; post-setup validation |
| Template library (9 files) | feature | `.codeflow/templates/test-config/*.json` | `setup --template` source; adopter copy-paste reference |
| init prompt integration | feature | `codeflow-cli/cli/src/cmd/init.rs` | Testing appears on the happy-path onboarding |
| Detection heuristics | feature | `codeflow-cli/core/src/testing/setup/detect.rs` | `--auto` stack-discovery |
| Injectable prompt provider | feature | `codeflow-cli/core/src/testing/setup/prompt.rs` | Testability of the wizard without a live terminal |

**Expected Outcome:** A new adopter can run `codeflow init` → answer the testing prompt → get a working `codeflow test` setup with zero manual edits. An existing adopter can run `codeflow test doctor` to verify their config. An agent authoring a config uses `codeflow test setup --template <n>` rather than Edit/Write.

**Deployment:** PR merge on `feat/generic-testing-setup` after TSK-002 merges.

## Pre-Work Analysis

- [ ] Reviewed git log for recent changes to `codeflow-cli/cli/src/cmd/init.rs` (large file with existing TUI wizard — avoid merge conflicts)
- [ ] Confirmed TSK-002 merged; config loader, writer, JSON schema all present
- [ ] Verified `.codeflow/templates/` directory exists or needs creating

## Approach

1. **Scaffold setup module.** `codeflow-cli/core/src/testing/setup/{mod.rs, detect.rs, wizard.rs, prompt.rs}`. Define `PromptProvider` trait in prompt.rs (parallels existing init.rs pattern).
2. **Detection heuristics.** Implement `detect_stacks(repo_root: &Path) -> Vec<DetectedTarget>` in detect.rs. Each sentinel file triggers a best-guess target. Cover the five sentinels: Cargo.toml, package.json+vitest, package.json+jest, go.mod, pyproject.toml+pytest.
3. **Wizard.** Implement in wizard.rs using the injectable PromptProvider. Flow: detect existing config → confirm action → per-detected-target confirm → per-target modes/report/coverage → doctor → write via TSK-002 config-writer → summary.
4. **Template library.** Author 9 templates under `.codeflow/templates/test-config/`. Each validates against the TSK-002 JSON schema. Include `_description` top-level string field.
5. **`codeflow test setup` CLI.** Wire to `codeflow-cli/cli/src/cmd/test.rs` as a new subcommand with clap derive. Implement flag dispatch: `--auto` → non-interactive detection path; `--template <n>` → template-copy path; `--list` → enumerate; `--add-target` → append-only variant; default → interactive wizard.
6. **Doctor module.** `codeflow-cli/core/src/testing/doctor/mod.rs`. Each check is a function returning `DoctorCheck { name, status: PASS|WARN|FAIL, message, evidence }`. Dispatcher runs all checks and collects.
7. **`codeflow test doctor` CLI.** Wire as a new subcommand. Support `--format human|json`. Exit codes per spec.
8. **init prompt integration.** Extend `codeflow-cli/cli/src/cmd/init.rs` existing step list with the "Set up testing now?" prompt. Three branches: y → call setup wizard; N → write minimal.json via config-writer; skip → no-op. Respect `--non-interactive` flag (defaults to N path).
9. **Auto-validate after setup.** Final wizard step calls `doctor::run_all_checks(config)` and displays result. If any FAIL, prompt to edit or abort.
10. **Comprehensive tests.** Unit tests per detection sentinel, per template round-trip, per doctor check (positive + negative). Injectable PromptProvider enables full-wizard unit tests. Integration test for `setup --auto` + `doctor`. Integration test for `init --non-interactive`.

## Standards & Practices

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Rust | cf-rust-standards | No unsafe, thiserror/anyhow, clippy `-D warnings` clean, injectable traits for testability |
| JSON | cf-markdown-standards | Valid JSON, schema-validated, 2-space indent |

Read the applicable skill BEFORE starting implementation.

## Files

### To Create

- `codeflow-cli/core/src/testing/setup/mod.rs` — orchestrator
- `codeflow-cli/core/src/testing/setup/detect.rs` — stack-detection heuristics
- `codeflow-cli/core/src/testing/setup/wizard.rs` — interactive flow
- `codeflow-cli/core/src/testing/setup/prompt.rs` — `PromptProvider` trait + default terminal impl
- `codeflow-cli/core/src/testing/doctor/mod.rs` — doctor checks
- `codeflow-cli/tests/integration_testing_setup.rs` — integration test
- `.codeflow/templates/test-config/minimal.json`
- `.codeflow/templates/test-config/single-target-basic.json`
- `.codeflow/templates/test-config/single-target-with-coverage.json`
- `.codeflow/templates/test-config/monorepo-multi-target.json`
- `.codeflow/templates/test-config/hooks-escape-hatch.json`
- `.codeflow/templates/test-config/example-node.json`
- `.codeflow/templates/test-config/example-python.json`
- `.codeflow/templates/test-config/example-go.json`
- `.codeflow/templates/test-config/example-rust.json`

### To Modify

- `codeflow-cli/core/src/testing/mod.rs` — export `setup` and `doctor` submodules
- `codeflow-cli/cli/src/cmd/test.rs` — add `setup` and `doctor` subcommands
- `codeflow-cli/cli/src/cmd/init.rs` — add "Set up testing now?" prompt step; update existing wizard tests for the three branches

### To Read

- `codeflow-cli/cli/src/cmd/init.rs` (current) — understand the existing step sequence and `--non-interactive` handling
- `codeflow-cli/core/src/testing/config/mod.rs` (from TSK-002) — config loader + writer API
- `.codeflow/docs/analysis/generic-testing-subsystem.md` §§15, 12 — setup flow and sample configs

## Concurrency Considerations

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| `.codeflow/config/testing/test-config.json` | read-modify-write | TSK-002 config-writer atomic temp+rename + file lock |
| `.codeflow/templates/test-config/*.json` | read-only at setup time | no mutation |
| Subprocess (doctor probes) | spawned per-check | each `std::process::Command`, ignored on failure |

## Acceptance Criteria

1. `codeflow test setup` command exists in `codeflow-cli/cli/src/cmd/test.rs` with flags `--auto`, `--template <name>`, `--list`, `--add-target`, `--force`; when no flags given, runs interactive wizard
2. Interactive wizard (setup module at `codeflow-cli/core/src/testing/setup/mod.rs`) detects existing config and asks Edit/Replace/Abort; asks user to confirm each detected target; walks per-target through mode commands, report config, coverage config; runs doctor validation at the end; fails back to edit prompts on validation error
3. Wizard calls the TSK-002 config-writer (not Edit/Write) to persist `test-config.json`; verified by grep — no `fs::write` call directly to the config path in the setup module
4. `--auto` detection priority (in `codeflow-cli/core/src/testing/setup/detect.rs`) checks in order: Cargo.toml at root or Cargo.toml with [workspace] members → rust-core target; package.json (with devDependencies or dependencies containing vitest) → web target with vitest commands; package.json (with jest in deps) → web target with jest commands; go.mod → go-service target; pyproject.toml or setup.py (with pytest in deps) → python target; each detection is covered by a unit test with a temporary directory fixture
5. `--auto` emits best-guess commands per stack per design doc §15.3 mapping table; fallback to JUnit + derive_from:junit when no native CTRF reporter is detected in the package's deps
6. `--template <name>` writes one of the named templates from `.codeflow/templates/test-config/`; refuses overwrite unless `--force` is passed; valid template names enumerated in `--list` output
7. `--list` prints template names one per line to stdout, sorted alphabetically, with a one-line description each; used by shell-completion and scripting
8. `--add-target` walks the per-target wizard only (skips the overall-config prompts) and appends to an existing config; refuses if no config exists (instructs user to run `codeflow test setup` first)
9. Template library ships 9 files in `.codeflow/templates/test-config/`: minimal.json, single-target-basic.json, single-target-with-coverage.json, monorepo-multi-target.json, hooks-escape-hatch.json, example-node.json, example-python.json, example-go.json, example-rust.json; every template validates against `.codeflow/schemas/test-config.schema.json`; every template has a '_description' top-level string field documenting its use case
10. `minimal.json`: empty `targets[]` array, `schema_version 1.0` — matches the fresh-project no-tests state
11. `single-target-basic.json`: one target (name 'app') with essential+full modes, no coverage — simplest non-empty config
12. `single-target-with-coverage.json`: one target (name 'app') with essential+full modes + lcov coverage + per-file + changed_files rules; includes a sample exception entry with reason + remove_when
13. `monorepo-multi-target.json`: three targets (api-go, web-node, tools-python) with `execution.parallel: true`; each target has distinct cwd
14. `hooks-escape-hatch.json`: one target with `runner: custom`, using a shell script at `.codeflow/scripts/run-tests.sh` as the command — demonstrates the escape hatch
15. `example-node.json`: web target with vitest + vitest-ctrf-json-reporter native CTRF + istanbul-summary coverage
16. `example-python.json`: python target with pytest + `--junitxml derive_from` + cobertura coverage
17. `example-go.json`: go-service target with `go test -json | go-ctrf-json-reporter` + go-cover coverage
18. `example-rust.json`: rust-core target with `cargo nextest --profile` + JUnit derive_from + lcov coverage (matches Scenario F in design doc; used as the starting point by TSK-005)
19. `codeflow test doctor` command exists in `codeflow-cli/core/src/testing/doctor/mod.rs` and `codeflow-cli/cli/src/cmd/test.rs`
20. Doctor check 1 (config exists): verifies `.codeflow/config/testing/test-config.json` exists; exit 1 with clear message if missing
21. Doctor check 2 (schema validation): validates the config against `.codeflow/schemas/test-config.schema.json` using the same validator as TSK-002's config loader; reports JSON-path of first violation
22. Doctor check 3a (cwd exists): for each enabled target, verifies `target.cwd` resolves to an existing directory relative to repo root
23. Doctor check 3b (command parses as shell): for each mode's command in each enabled target, runs a basic POSIX-shell tokenizer (shell-words crate or equivalent) and reports parse errors
24. Doctor check 3c (report.path safe): for each target, verifies `report.path` is a relative path with no parent traversal (no `..` components); absolute paths rejected
25. Doctor check 3d (coverage.path safe): for each target with coverage, same path safety check as 3c
26. Doctor check 3e (coverage.transform parses): for each target with `coverage.transform`, same shell parse check as 3b
27. Doctor check 3f (globs valid): for each coverage rule's include/exclude, compiles the glob via globset crate; reports invalid patterns
28. Doctor check 3g (exception file exists): for each `coverage.exceptions[].file`, checks file exists in the repo; warning (not error) if missing — exception may precede the file being created
29. Doctor check 4 (dry-run probe): for each enabled target's quick mode (if declared), invokes the runner's canonical probe with a **5-second per-probe timeout**: cargo → `cargo --version`; pytest → `pytest --version`; jest → `jest --version`; vitest → `vitest --version`; go → `go version`; mocha → `mocha --version`; rspec → `rspec --version`; phpunit → `phpunit --version`; custom → no probe; probe failure (non-zero exit OR timeout) is a WARNING, not an error; timeout produces the warning message "probe for runner=X exceeded 5s timeout; not blocking" — allows intentionally Docker-wrapped or slow-cold-start commands to pass; unit test covers both timeout and non-zero-exit paths
30. Doctor output: `--format human` (default) prints numbered sections with PASS/FAIL/WARN labels; `--format json` emits a structured report consumable by cf-knowledge-layer
31. Doctor exit codes: 0 (all checks pass or only warnings), 1 (one or more errors), 2 (infrastructure error — cannot read config at all)
32. Doctor runs automatically at end of `codeflow test setup` before writing the final config; fails back to edit prompts on error
33. `codeflow init` (in `codeflow-cli/cli/src/cmd/init.rs`) gains a "Set up testing now?" prompt immediately before the existing finalization step; prompt offers `[y/N/skip]`; y runs `codeflow test setup`; N writes minimal.json template; skip does nothing; default is N
34. init prompt is skipped when `--non-interactive` flag is set (existing init flag); in that case, minimal.json is written (matching the default)
35. init prompt documented in `init.rs`: existing wizard flow is updated to account for the new step; existing init integration tests updated to cover each of the three branches (y, N, skip)
36. Wizard never invokes Edit/Write tool against `test-config.json`; uses the TSK-002 config-writer exclusively; verified by grep
37. No task deliverable introduces an LLM code path that Edit/Writes into `.state/**` or `.codeflow/config/testing/test-config.json` directly. Every mutation has a corresponding `codeflow test <subcommand>`. Setup wizard and doctor strictly use read/write through CLI internal APIs, not direct filesystem mutation of the protected paths.
38. Unit tests for `--auto` detection: at least one test per sentinel file (Cargo.toml, package.json+vitest, package.json+jest, go.mod, pyproject.toml+pytest) using tempfile fixtures; one test for 'no sentinels found' case producing empty config
39. Unit tests for `--template`: every template in the library has a **byte-equivalent** round-trip test (read template file as bytes → load via schema → write via TSK-002 config-writer to a tempfile → re-read tempfile bytes → assert byte-equal to original); struct-equal is insufficient — byte-equal catches config-writer formatting drift (key order, indentation, trailing newline, quoting). Template files are authored by the same config-writer so round-trip is well-defined.
40. Unit tests for doctor checks: every check (1, 2, 3a-3g, 4) has a positive test (valid config passes) and a negative test (invalid config fails with specific message); at least 20 doctor test cases total
41. Interactive wizard testable via injectable `PromptProvider` trait (per the pattern used in `init.rs`); unit tests inject a scripted prompt provider to exercise the full wizard flow without a live terminal
42. Integration test: `codeflow test setup --template example-node.json --force` followed by `codeflow test doctor` on a fresh tempdir → both exit 0; resulting `test-config.json` byte-equivalent to the template (modulo absolute paths)
43. Integration test: `codeflow test setup --auto` on a tempdir containing a Cargo.toml and a package.json → produces a config with both rust-core and web targets; doctor passes
44. Integration test: `codeflow init --non-interactive` produces `.codeflow/config/testing/test-config.json` matching minimal.json byte-for-byte
45. Error path test: `codeflow test setup --template nonexistent.json` exits 1 with message "Template not found. Run `codeflow test setup --list` to see available templates."
46. Error path test: `codeflow test setup --template minimal.json` when `test-config.json` already exists exits 2 with "Config already exists at <path>. Use --force to overwrite."
47. Error path test: `codeflow test doctor` with a malformed JSON `test-config.json` exits 1 with a parse-error message pointing to line:column
48. Lint: `cargo clippy --all-targets --all-features -- -D warnings` passes on all new modules
49. Format: `cargo fmt --check` passes
50. Coverage: per-file ≥85% on setup/mod.rs, setup/detect.rs, doctor/mod.rs, and every CLI subcommand handler; any exceptions declared in `test-config.json` must have reason + remove_when

### Scenarios covered

| # | Scenario | Covered by acceptance # |
|---|----------|------------------------|
| 1 | Adopter runs `codeflow init` → y branch → wizard → working config | 33, 2, 42 |
| 2 | Adopter runs `codeflow init` → N branch → minimal.json | 33, 10 |
| 3 | Adopter runs `codeflow init --non-interactive` → minimal.json | 34, 44 |
| 4 | Adopter runs `codeflow test setup --auto` on Rust+Node monorepo | 4, 5, 43 |
| 5 | Adopter applies a template | 6, 42 |
| 6 | Adopter adds a second target later | 8 |
| 7 | Existing adopter runs doctor to check drift | 19-31 |
| 8 | Config file missing | 20 |
| 9 | Schema violation in config | 21, 47 |
| 10 | cwd directory removed | 22 |
| 11 | Command string with syntax error | 23 |
| 12 | Absolute path in report.path (unsafe) | 24 |
| 13 | Invalid glob | 27 |
| 14 | Exception file doesn't exist yet | 28 |
| 15 | Runner binary not installed (cargo missing) | 29 |
| 16 | Wizard triggered but user cancels mid-way | 2 (Abort branch) |

### PII Handling Review

- [ ] Does this task involve code that handles PII? (N)

### Criteria Status

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DEV -> WS-SEC -> WS-REV -> WS-QA

| # | Criterion | DEV | SEC | REV | QA | Notes |
|---|-----------|-----|-----|-----|-----|-------|
| 1 | `codeflow test setup` flags and wizard path | -- | -- | -- | -- | |
| 2 | Interactive wizard: detect-existing, per-target, doctor at end | -- | -- | -- | -- | |
| 3 | Wizard uses config-writer (no direct Edit/Write) | -- | -- | -- | -- | |
| 4 | `--auto` detection order for 5 sentinel files, per-sentinel tests | -- | -- | -- | -- | |
| 5 | `--auto` best-guess commands, JUnit fallback when no CTRF reporter | -- | -- | -- | -- | |
| 6 | `--template <name>` with --force semantics | -- | -- | -- | -- | |
| 7 | `--list` sorted output with descriptions | -- | -- | -- | -- | |
| 8 | `--add-target` append-only semantics | -- | -- | -- | -- | |
| 9 | 9 templates ship, schema-valid, _description field | -- | -- | -- | -- | |
| 10 | minimal.json shape | -- | -- | -- | -- | |
| 11 | single-target-basic.json shape | -- | -- | -- | -- | |
| 12 | single-target-with-coverage.json includes sample exception | -- | -- | -- | -- | |
| 13 | monorepo-multi-target.json, parallel execution | -- | -- | -- | -- | |
| 14 | hooks-escape-hatch.json, runner:custom | -- | -- | -- | -- | |
| 15 | example-node.json shape | -- | -- | -- | -- | |
| 16 | example-python.json shape | -- | -- | -- | -- | |
| 17 | example-go.json shape | -- | -- | -- | -- | |
| 18 | example-rust.json matches Scenario F | -- | -- | -- | -- | |
| 19 | `codeflow test doctor` command exists | -- | -- | -- | -- | |
| 20 | Check 1 (config exists) | -- | -- | -- | -- | |
| 21 | Check 2 (schema validation, reports JSON-path) | -- | -- | -- | -- | |
| 22 | Check 3a (cwd exists) | -- | -- | -- | -- | |
| 23 | Check 3b (command parses as shell) | -- | -- | -- | -- | |
| 24 | Check 3c (report.path safe, no absolute paths / no ..) | -- | -- | -- | -- | |
| 25 | Check 3d (coverage.path safe) | -- | -- | -- | -- | |
| 26 | Check 3e (coverage.transform parses) | -- | -- | -- | -- | |
| 27 | Check 3f (globs compile) | -- | -- | -- | -- | |
| 28 | Check 3g (exception file exists, warning only) | -- | -- | -- | -- | |
| 29 | Check 4 (dry-run probe per runner enum, warning only) | -- | -- | -- | -- | |
| 30 | Doctor output --format human / json | -- | -- | -- | -- | |
| 31 | Doctor exit codes 0/1/2 | -- | -- | -- | -- | |
| 32 | Doctor runs automatically at end of setup | -- | -- | -- | -- | |
| 33 | `codeflow init` has "Set up testing now?" prompt with 3 branches | -- | -- | -- | -- | |
| 34 | `--non-interactive` branch writes minimal.json | -- | -- | -- | -- | |
| 35 | init integration tests cover 3 branches | -- | -- | -- | -- | |
| 36 | Wizard grep: no direct Edit/Write on test-config.json | -- | -- | -- | -- | |
| 37 | `.state/` + test-config.json invariant stated | -- | -- | -- | -- | |
| 38 | 5+ auto-detect unit tests | -- | -- | -- | -- | |
| 39 | 9 template round-trip tests | -- | -- | -- | -- | |
| 40 | 20+ doctor check tests (positive + negative per check) | -- | -- | -- | -- | |
| 41 | PromptProvider injectable; wizard unit-testable | -- | -- | -- | -- | |
| 42 | Integration test: setup --template + doctor | -- | -- | -- | -- | |
| 43 | Integration test: setup --auto on Rust+Node monorepo | -- | -- | -- | -- | |
| 44 | Integration test: init --non-interactive produces minimal.json | -- | -- | -- | -- | |
| 45 | Error path: setup --template nonexistent | -- | -- | -- | -- | |
| 46 | Error path: setup --template without --force when config exists | -- | -- | -- | -- | |
| 47 | Error path: doctor on malformed JSON | -- | -- | -- | -- | |
| 48 | Clippy clean | -- | -- | -- | -- | |
| 49 | fmt check passes | -- | -- | -- | -- | |
| 50 | Per-file coverage ≥85% | -- | -- | -- | -- | |

## Dependencies

### Blocked By

- INF-TSK-046-002 (engine, config loader, schema, config-writer)

### Blocks

- INF-TSK-046-005 (self-host migration uses example-rust.json template as starting point)

## Verification

### Automated

- [ ] `codeflow test --mode full --coverage` passes post-migration; during development, `cargo nextest run --profile full` on the new modules
- [ ] `cargo clippy --all-targets --all-features -- -D warnings` passes
- [ ] `cargo fmt --check --all` passes
- [ ] Integration test `codeflow-cli/tests/integration_testing_setup.rs` passes
- [ ] Template round-trip tests pass for all 9 templates
- [ ] Doctor unit tests cover all 9 checks with positive + negative cases (≥20 tests)

### Manual

- [ ] Run `codeflow test setup` interactively on a fresh tempdir; verify wizard flow matches §15.2 of design doc
- [ ] Run `codeflow test setup --auto` on the CodeFlow repo itself — verify a Rust target is detected
- [ ] Run `codeflow test setup --list` — verify 9 templates listed, alphabetical, with descriptions
- [ ] Run `codeflow test doctor` on an intentionally broken config (wrong scope value) — verify clear error pointing at JSON path

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

**Verdict:** {PASS | FAIL}

### Confidence Score

| Stage | Agent | Score (0-100) | Rationale |
|-------|-------|--------------|-----------|
| WS-DEV | cf-development | {n} | {brief rationale} |
| WS-SEC | cf-security | {n} | {brief rationale} |
| WS-REV | cf-review | {n} | {brief rationale} |
| WS-QA | cf-quality-assurance | {n} | {brief rationale} |

## Notes

Design doc: `.codeflow/docs/analysis/generic-testing-subsystem.md` §§15, 12, 14.3.
ADR: `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md`.

**Edge cases:**

- `package.json` with both vitest AND jest in devDependencies — detection prefers vitest (newer); emit stderr note "Both vitest and jest detected; defaulting to vitest. Re-run setup to choose jest."
- `Cargo.toml` with no `[workspace]` but plain `[package]` — treat as rust-core target with `cwd: .`
- `pyproject.toml` using poetry vs pip — both detected via the `[project]` or `[tool.pytest.ini_options]` sections
- User sets `--template minimal.json --force` on a dir with an existing config — clobbers, no confirmation; intentional for scripting
- Probe dry-run in doctor may be slow in CI with cold PATH; cache the result per-session if needed
