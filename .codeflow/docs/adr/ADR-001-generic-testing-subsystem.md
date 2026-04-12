---
id: ADR-001
title: Generic Testing Subsystem — Declarative Config with CTRF-shaped Canonical Model
status: proposed
date: 2026-04-11
deciders: [team-lead, cf-planning]
consulted: [cf-development, cf-quality-assurance, cf-review]
informed: [downstream-adopters]
---

# ADR-001: Generic Testing Subsystem — Declarative Config with CTRF-shaped Canonical Model

## Status

Proposed

## Context

### Problem Statement

CodeFlow's testing subsystem is currently entangled with Rust self-host assumptions. The `codeflow test --mode full --coverage` command hard-codes `cargo test`, `cargo llvm-cov`, `codeflow-cli/config/testing/test-config.json`, the `codeflow-core` / `codeflow-cli` crate names, and an 85% per-file coverage rule that presumes line-coverage metrics shaped like llvm-cov's output. The Claude artifacts (CLAUDE.md, agent definitions, slash commands, hook scripts) repeat those assumptions as first-class instructions to teammates: `cargo test --workspace --no-fail-fast` appears in QA report templates, `cargo clippy` in the quality-gate checklists, and "Rust crates" in the PR body format.

Downstream adopters on Node, Go, Python, or mixed monorepo stacks cannot use `codeflow test` without forking and rewriting Rust-specific source. They also cannot adopt the Claude agent team unmodified — the agent definitions would instruct their developers to run `cargo test` on a Node project. The framework advertises itself as stack-agnostic but is, in practice, Rust-first.

### Constraints

- **`.state/**` is protected from direct LLM Edit/Write.** Test reports, coverage artifacts, and ledger entries must never be written by teammates via Edit/Write. All mutations go through `codeflow` CLI subcommands.
- **`.codeflow/config/testing/test-config.json` is protected via `.codeflow/**` ProtectionGuard coverage.** Teammates cannot Edit this file directly; every mutation must flow through a CLI command.
- **Ephemerality.** Test reports and coverage data are per-session; they must not be committed to git.
- **Per-worktree isolation.** Concurrent sessions in parallel worktrees must not collide when writing test reports or coverage artifacts.
- **No retries for flaky tests.** Existing user policy: flaky tests are fixed immediately, not retried.
- **Claude artifacts stack-agnostic.** CLAUDE.md, agent defs, commands, skills, and hooks must not reference any specific tech stack or tool — only the universal `codeflow test` wrapper.

### Assumptions

- Adopters can express their test execution as one or more shell commands per target.
- The JUnit XML report format is produced natively or via plugin by the dominant test runners in every supported ecosystem (pytest, cargo-nextest, go test -json→junit, jest, vitest, mocha, rspec, phpunit).
- CTRF (Common Test Report Format, ctrf.io) JSON is or will be the cross-ecosystem canonical format; it is supported natively by a growing roster of runners and can be derived losslessly from JUnit.
- Coverage can be expressed through four well-known formats: lcov, cobertura, istanbul-summary, go-cover. Runners emitting other formats can pre-transform via a shell `transform` step.
- The `codeflow` CLI can expand its surface to cover every read/write operation a teammate would otherwise want to perform on `.state/` or `test-config.json`.

## Decision

CodeFlow's testing subsystem will be **configuration-driven**. The canonical internal test-result model will be **CTRF-shaped**. **JUnit XML** is the primary universal input adapter. A **`runner: custom`** escape hatch accepts CodeFlow-canonical JSON directly. Coverage uses an **ordered rule list** over five scope types (`per_file | per_package | per_module | changed_files | global`). Claude artifacts reference only the universal `codeflow test` wrapper, with zero stack-specific content. CodeFlow self-host migrates as the first adopter: `cargo-nextest --profile` emits JUnit, the engine converts JUnit→CTRF internally, and existing coverage exceptions are preserved 1:1 in the new declarative config.

The declarative config file lives at `.codeflow/config/testing/test-config.json`. Mutations are routed through `codeflow test config <subcmd>` and `codeflow test exceptions <subcmd>`. Test reports (CTRF JSON, JUnit XML) and coverage files (lcov.info, cobertura.xml) are written to `.state/test-reports/` and `.state/coverage/` — both are **per-worktree LOCAL** in `WorktreePaths` (not symlinked). On run completion, the engine emits a `test_result_recorded` event to the ledger containing the canonical CTRF summary; raw reports are ephemeral and die with the session.

### Rationale

**Why CTRF as internal canonical:** CTRF is a modern JSON-native format explicitly designed to be cross-ecosystem. It natively represents tests, suites, status, durations, stderr, tags, flaky markers, and attachments. Native reporter plugins already exist for Jest, Vitest, Playwright, Cypress, Mocha, pytest, Go, and a growing list. Adopting it internally aligns CodeFlow with the same direction the testing ecosystem is heading, without forcing adopters onto a CodeFlow-proprietary format.

**Why JUnit as primary input adapter:** JUnit XML (the Ant/Surefire dialect) is the de facto universal test output format. Every major runner either emits it natively or via a plugin. Converting JUnit → CTRF is lossless for the fields CodeFlow cares about (pass/fail counts, durations, failure messages, suite names). Shipping a JUnit adapter means the engine covers ~95% of adopters on day one without per-stack code.

**Why four well-known coverage formats:** lcov (genhtml / llvm-cov), cobertura (Jest, nyc, Python coverage.py), istanbul-summary (nyc, c8), go-cover (go tool cover -func) together cover every mainstream tool. Exotic tools can pre-transform via a declarative `transform` shell step — the engine never special-cases.

**Why declarative shell config over plugin adapters:** Shipping runner adapters as Rust code (the "plugin" model) creates a maintenance treadmill — every adopter's unusual configuration becomes an engine change. A declarative model externalizes stack knowledge entirely to the adopter's `test-config.json`, matching the architecture of the pre-commit framework, commitlint, and most CI test-reporters. Adopters maintain their own config; we maintain a small, stable parser.

**Why ordered coverage rule list:** Adopters need flexibility: "85% per-file on changed files, 70% per-file on legacy/**, 80% per-package everywhere else." A single threshold can't express this. A rule list where first-matching-rule-wins is how tools like ESLint and pre-commit handle overrides — it's idiomatic and predictable.

**Why `.state/test-reports/` and `.state/coverage/` local (not symlinked):** Concurrent worktrees running `codeflow test` must not overwrite each other's reports. The ledger and sentinels are already local per-worktree since PR #221; extending that to test artifacts is the consistent choice.

**Why the ledger event not a shared file:** Other observers (the ledger audit, the PR body renderer, cross-session memory) should read test outcomes from the ledger. Raw reports are ephemeral session artifacts. Persisting structured summaries in the ledger gives us the queryable history; the raw files don't need to live past the session.

## Alternatives Considered

### Option 1: Plugin Adapter Architecture (Rust-coded per-stack adapters)

Ship Rust modules — `testing::adapters::cargo`, `testing::adapters::pytest`, `testing::adapters::jest` — each knowing how to invoke a specific runner and parse its output.

**Pros:**

- Tightest integration; engine can offer stack-aware error messages
- Easy onboarding for well-known stacks

**Cons:**

- Every new stack requires a CodeFlow release
- Unusual runner configurations require engine forks
- Rust code bloat: ~10 adapters × ~500 lines each = ~5,000 lines of parsing code
- Conflates CodeFlow's role (orchestrator) with test-runner concerns
- Fails on the "mixed monorepo" scenario unless adapters can be stacked arbitrarily

**Why rejected:** The maintenance burden grows with each stack. Declarative config pushes that burden to the adopter, where it belongs.

### Option 2: Task-Runner Delegation (e.g., Make / Just / npm scripts)

Have `codeflow test` invoke a pre-declared task-runner target like `make test` or `just test`. The adopter wires the plumbing in their task runner; CodeFlow just reads back exit codes.

**Pros:**

- Minimal engine work
- Adopters already know their task runner

**Cons:**

- No structured test results — just exit codes
- Cannot populate the PR body "Test Results" section
- Cannot detect per-file coverage regressions
- Violates CodeFlow's need for queryable, per-file coverage enforcement
- Pushes all reporter glue back onto the adopter

**Why rejected:** Loses the structured-result capabilities that WS-QA and the PR body gate depend on.

### Option 3: Lingua-Franca Formats Only (no internal canonical)

Accept JUnit XML and/or CTRF directly; do not convert or normalize. Every consumer (PR body renderer, ledger event, threshold engine) parses JUnit or CTRF independently.

**Pros:**

- Simplest engine code path
- No internal format maintenance

**Cons:**

- Every consumer duplicates parsing logic
- Extending the internal model (e.g., adding `consecutive_clean_runs`) requires parser changes everywhere
- JUnit and CTRF have different shapes — consumers need conditional logic
- Future formats (e.g., a CodeFlow-native "custom" runner) have no place to land

**Why rejected:** Without a canonical internal model, the engine becomes a pass-through with fragile consumers. The canonical model is small (~10 structs); the leverage is large.

### Option 4: Declarative Shell Config + CTRF Canonical + JUnit Adapter (chosen)

See Decision above.

**Pros:**

- Zero per-stack code in the engine
- Universal coverage via JUnit adapter (~95% of runners)
- CTRF-native path for modern runners
- Custom escape hatch for exotic tools
- Consistent with ecosystem direction (ctrf.io adoption curve)

**Cons:**

- JUnit parser must handle runner-specific quirks (pytest vs. jest vs. nextest attribute naming)
- CTRF ecosystem still maturing; Rust has no native CTRF reporter for cargo today — requires the JUnit-to-CTRF internal converter
- Coverage rule-list semantics must be carefully specified (first-matching-rule-wins) to avoid surprises

## Consequences

### Positive

- Downstream adopters on Node, Go, Python, Rust, and mixed stacks can use `codeflow test` via config, no code changes
- Claude artifacts become stack-agnostic — CLAUDE.md, agent defs, commands, skills no longer reference `cargo` / `rust` / `llvm-cov`
- CodeFlow self-host becomes just another adopter of its own generic engine — dogfoods the abstraction
- PR body test-results format extends to every adopter uniformly (target-aware tables)
- Per-worktree isolation eliminates cross-session collision on test artifacts
- Ledger gains structured test-result history queryable across sessions
- `codeflow test` can be run on a project with zero config (fresh-project no-tests path → single-line notice)
- Invariant preserved: LLMs never touch `.state/` or `test-config.json` directly; all mutations via CLI

### Negative

- `codeflow test` command gains ~15 new subcommands (`setup`, `doctor`, `report convert`, `config add-target`, `exceptions add`, etc.) — larger API surface
- Engine gains ~3,000 lines of new Rust (config loader, JUnit parser, CTRF converter, coverage parsers, threshold engine, report emitter)
- First-time adopters must learn the config schema (mitigated by wizard + templates + doctor)
- Migration risk on CodeFlow self-host: a parsing bug in the JUnit→CTRF converter could silently change PR test stats. Mitigated by TSK-005's "parity check" step against the old output.

### Risks

- **CTRF ecosystem immaturity (Rust).** No native CTRF reporter for cargo exists today. Mitigation: ship the JUnit→CTRF converter as a first-class engine feature, exposed as `codeflow test report convert --from junit --to ctrf`. Self-host uses this path.
- **Migration rollback.** TSK-005 performs a clean cutover — the legacy `TestValidator::full_validate` path is deleted in the same PR that lands the new engine. No `CODEFLOW_TEST_ENGINE=legacy` feature flag is retained. Rollback mechanism: `git revert` of the migration PR. This decision is recorded in TSK-005 AC#16, design doc §17.2, and supersedes any earlier draft that mentioned a feature-flag fallback. Rationale: feature flags in a migration path become permanent tech debt; a parity check before merge (TSK-005 AC#9) catches divergences; `git revert` is atomic and restores the known-good state with no partial-adoption risk.
- **JUnit dialect drift.** Different runners emit slightly different JUnit (attribute names, nested suites). Mitigation: ship round-trip tests with golden fixtures from cargo-nextest, pytest-junitxml, jest-junit, vitest — every covered runner gets a fixture.
- **Schema evolution.** Adopters will ship with an early schema version. Mitigation: `$schema` field in test-config.json; `codeflow test doctor` validates against JSON-schema file published in `.codeflow/schemas/`; introduce a `schema_version` field in the config root.
- **Cross-platform shell compatibility.** `command` fields are shell strings — subject to Windows/POSIX divergence. Mitigation: document POSIX-shell-only in v1; document argv-array form as the future-safe path; do not silently paper over cmd.exe differences.
- **`codeflow test` becoming a bottleneck for parallelism.** All targets through one process. Mitigation: `execution.parallel: true` + per-target `cwd` + per-target output directories; engine spawns targets as independent child processes.

## Implementation

### Action Items

- [ ] TSK-002: Implement engine (config loader, JUnit parser, CTRF model, converters, coverage parsers, threshold engine, canonical report, PR-body emitter, `--only`/`--skip`/`--mode` flags, `codeflow test report|config|exceptions` subcommands, per-worktree `.state/test-reports/` + `.state/coverage/`)
- [ ] TSK-003: Setup wizard (`codeflow test setup` with `--auto`, `--template`, `--list`), doctor (`codeflow test doctor`), template library in `.codeflow/templates/test-config/`
- [ ] TSK-004: Rewrite Claude artifacts (CLAUDE.md §§6-8 test-related sections, cf-quality-assurance, cf-development, cf-review, cf-git-operations; update `/cf-test`, `/cf-ship`, `/cf-deploy`; make gh-pr-guard target-aware)
- [ ] TSK-005: Self-host migration (author CodeFlow's own test-config.json, add `nextest.toml` profile, migrate exceptions 1:1, verify PR body parity, `.gitignore` additions)

### Timeline

Four sequential implementation tasks (TSK-002 gates TSK-003/004/005; TSK-005 depends on all three). Estimated 8 sessions total per epic autorun batching. TSK-002 is XL and drives the schedule.

## Related

- `.codeflow/docs/analysis/generic-testing-subsystem.md` — full design and analysis document
- `.codeflow/docs/analysis/shadow-testing-architecture.md` — prior shadow harness reference
- `project-management/epics/INF/INF-EPC-046/INF-EPC-046.md` — epic tracking this decision
- <https://ctrf.io> — Common Test Report Format specification
- <https://junit.org/junit5/docs/current/user-guide/> — JUnit XML schema (via Surefire/Ant historical dialect)
- `.codeflow/config/testing/test-config.json` — current Rust-specific config (to be replaced)
- `codeflow-cli/core/src/worktree/paths.rs` — `WorktreePaths` module (to be extended with `test_reports_dir()` / `coverage_dir()`)
