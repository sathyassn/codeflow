---
title: Generic Testing Subsystem — Design and Analysis
status: proposed
epic: INF-EPC-046
adr: ADR-001
author: cf-planning
date: 2026-04-11
---

# Generic Testing Subsystem — Design and Analysis

## 1. Summary

CodeFlow's testing subsystem is currently coupled to Rust self-host: the `codeflow test` command assumes `cargo test` + `cargo llvm-cov` + `codeflow-core`/`codeflow-cli` crates + per-file 85% thresholds, and the Claude artifacts (CLAUDE.md, agent defs, commands, hooks) encode these assumptions as first-class instructions. Downstream adopters on other stacks cannot use CodeFlow as an agent-orchestration framework without forking and rewriting.

This design generalizes the testing subsystem by introducing a declarative `test-config.json` schema, a CTRF-shaped canonical internal model, a JUnit XML input adapter, four well-known coverage-format parsers, a setup wizard, and a doctor command — then strips every stack-specific assumption out of the Claude artifacts. CodeFlow self-host migrates as the first adopter on day one, validating the abstraction.

Who benefits: (a) downstream adopters on Node/Go/Python/mixed stacks, who gain `codeflow test` as a configurable orchestrator; (b) CodeFlow maintainers, who can evolve the test subsystem without touching agent definitions; (c) future adopters on languages we haven't encountered yet — the config can extend without engine releases.

Scope: 4 implementation tasks (TSK-002 through TSK-005), ~3,000 lines of new Rust in the engine, ~10 Claude-artifact rewrites, zero regressions on the existing shell/Python/Rust test suites at self-host cutover.

## 2. Table of Contents

- [1. Summary](#1-summary)
- [2. Table of Contents](#2-table-of-contents)
- [3. Problem Statement](#3-problem-statement)
- [4. Goals & Non-Goals](#4-goals--non-goals)
- [5. Approach Alternatives Considered](#5-approach-alternatives-considered)
- [6. Chosen Approach](#6-chosen-approach)
- [7. Wire Formats](#7-wire-formats)
- [8. Config Schema](#8-config-schema)
- [9. Coverage Model](#9-coverage-model)
- [10. Modes & Execution](#10-modes--execution)
- [11. PR Report Format](#11-pr-report-format)
- [12. Usage Scenarios with Sample Configs](#12-usage-scenarios-with-sample-configs)
- [13. `.state/` Enforcement Analysis](#13-state-enforcement-analysis)
- [14. CLI Surface](#14-cli-surface)
- [15. Init & Setup Flow](#15-init--setup-flow)
- [16. Worktree Parallelism](#16-worktree-parallelism)
- [17. Migration Plan](#17-migration-plan)
- [18. Risks & Mitigations](#18-risks--mitigations)
- [19. References](#19-references)
- [20. Dual-Config Architecture](#20-dual-config-architecture)
- [21. Zero-Coverage File Detection](#21-zero-coverage-file-detection)
- [22. Parallel Worktree Path Safety](#22-parallel-worktree-path-safety)

## 3. Problem Statement

CodeFlow advertises itself as a stack-agnostic agent-team orchestration framework, but its testing subsystem and its Claude artifacts presume Rust self-host. The entanglement is visible at every layer:

- **CLI hard-codes.** `codeflow-cli/cli/src/cmd/test.rs` has only three modes (essential/standard/full) and two paths: run a shell script (`.codeflow/testing/run-all-tests.sh`) or run `TestValidator::full_validate()` which invokes `cargo test` and `cargo llvm-cov`.
- **Config hard-codes.** `codeflow-cli/config/testing/test-config.json` has `coverage.tool: cargo-llvm-cov`, `business_packages: [codeflow-core, codeflow-cli]`, `conventions.test_runner: cargo-nextest`, `property_testing: proptest`, `snapshot_testing: insta`. Every knob assumes Rust.
- **Claude artifact hard-codes.** CLAUDE.md §7 (Testing) says "codeflow test routes to all test suites (shell/Python, Rust) and enforces 85% per-file coverage threshold on business packages." cf-quality-assurance.md says "cargo test --workspace --no-fail-fast." The PR body format instructs teammates to populate a "CLI crate" and "Core crate" coverage table.
- **Hook hard-codes.** gh-pr-guard checks for test-stats sections whose column headers and coverage-format labels assume Rust crates.

A downstream Node adopter running `codeflow test` would either see the shell script fail (no run-all-tests.sh), or see `cargo test` fail (no Cargo.toml), and would then have to fork every Claude artifact to remove Rust references. That defeats the value of the agent-team framework.

### Current Rust-specific coupling points (grep evidence)

| Location | Coupling |
|----------|----------|
| `codeflow-cli/cli/src/cmd/test.rs` | `TestValidator::full_validate()` runs cargo directly |
| `codeflow-cli/core/src/testing/validation.rs` | Invokes `cargo test`, parses nextest output, invokes `cargo llvm-cov` |
| `codeflow-cli/config/testing/test-config.json` | `coverage.tool: cargo-llvm-cov`, `business_packages: [codeflow-core, codeflow-cli]` |
| `.claude/CLAUDE.md` §7 | "shell/Python, Rust" test suite language; "CLI crate", "Core crate" column headers |
| `.claude/agents/cf-quality-assurance.md` | `cargo test --workspace`, `cargo clippy`, `cargo fmt --check`, "Rust Quality Gate" section |
| `.claude/agents/cf-development.md` | Rust test-writing guidance, `#[cfg(test)] mod tests` presumption |
| `.claude/agents/cf-review.md` | Rust-specific review dimensions |
| `.claude/agents/cf-git-operations.md` | PR body template presumes Rust crate coverage |
| `.claude/commands/cf-test.md` | `codeflow test` invocation assumes Rust path |
| `.claude/commands/cf-ship.md` | PR body test-stats assumes Rust layout |
| `.claude/hooks/gh-pr-guard` (Rust impl) | Content-free section-header check; passes Rust-shaped stats through |
| `project-management/templates/task-template.md` | DEV Report/QA Report templates reference `cargo test`, `cargo llvm-cov`, "codeflow-core", "codeflow-cli" |

[↑ TOC](#2-table-of-contents)

## 4. Goals & Non-Goals

### Goals

- **G1.** `codeflow test` works on any supported stack via declarative config, no code changes.
- **G2.** Test output is normalized to a CTRF-shaped canonical model regardless of input format.
- **G3.** Coverage enforcement supports per-file, per-package, per-module, changed-files, and global scopes, ordered rule-list semantics.
- **G4.** Fresh-project path ("no targets configured") produces an informative notice, not an error.
- **G5.** Claude artifacts contain zero stack-specific text; they reference only the universal `codeflow test` wrapper.
- **G6.** `.state/` remains LLM-protected; every read/write on test artifacts routes through a `codeflow test <subcmd>`.
- **G7.** `test-config.json` remains LLM-protected; every mutation (add target, add exception, change threshold) has a corresponding CLI subcommand.
- **G8.** CodeFlow self-host migrates on the same PR as the engine lands; zero regressions on existing shell/Python/Rust test suites.
- **G9.** Per-worktree isolation: concurrent `codeflow test` invocations in parallel worktrees never collide on test artifacts.
- **G10.** Extensibility: adding a new runner, new coverage format, or new scope should be config-level in most cases, small-engine-change at worst.

### Non-Goals

- **NG1.** We are not becoming a test runner. Execution stays with `cargo`/`jest`/`pytest`/`go test`/etc.
- **NG2.** We are not reimplementing coverage collection. We parse existing tool output.
- **NG3.** We are not shipping per-stack plugin adapters as Rust code.
- **NG4.** We are not handling CI/CD pipeline configuration; that's the adopter's CI.
- **NG5.** We are not retrying flaky tests; existing user policy holds.
- **NG6.** We are not persisting raw test reports beyond the session; a structured summary goes to the ledger, the raw reports die with the worktree.
- **NG7.** We are not introducing a plugin/hook system for custom reporters; that's a future epic.

[↑ TOC](#2-table-of-contents)

## 5. Approach Alternatives Considered

Four models were evaluated. The full ADR ([ADR-001](../adr/ADR-001-generic-testing-subsystem.md)) records the trade-off matrix; this section summarizes.

| # | Model | Engine Code | Per-Stack Code | Adopter Work | Verdict |
|---|-------|-------------|----------------|-------------|---------|
| 1 | Plugin adapters (Rust-coded per stack) | Large, grows per stack | High | Low | Rejected — maintenance treadmill |
| 2 | Task-runner delegation (`make test`, `just test`) | Minimal | Zero | High (all reporter glue) | Rejected — no structured results |
| 3 | Lingua-franca only (no internal canonical) | Small, duplicated across consumers | Zero | Medium | Rejected — fragile consumers |
| 4 | **Declarative config + CTRF canonical + JUnit adapter + custom escape hatch (chosen)** | Medium, stable | Zero | Medium (config once) | Chosen |

### Decision Matrix (weighted)

| Criterion | Weight | Option 1 (Plugins) | Option 2 (Task-Runner) | Option 3 (Lingua-Franca) | Option 4 (Declarative) |
|-----------|--------|--------------------|-----------------------|--------------------------|-------------------------|
| Engine maintainability | H | 2 | 5 | 3 | 4 |
| Adopter onboarding effort | H | 5 | 3 | 3 | 4 |
| Structured result availability | H | 5 | 1 | 4 | 5 |
| Ecosystem alignment | M | 3 | 3 | 3 | 5 |
| Extensibility without releases | H | 1 | 5 | 3 | 5 |
| Parity with CodeFlow self-host needs | H | 5 | 2 | 4 | 5 |
| **Weighted score** | — | 3.3 | 3.0 | 3.3 | **4.7** |

Scale 1-5 (5 best). Weights: H = 2, M = 1.

**Industry analogs supporting the declarative choice:**

- **pre-commit framework** (pre-commit.com): declarative `hooks.yaml` with per-hook `repo`, `entry`, `language`, `files`. Adopters add hooks via config, not code.
- **commitlint**: declarative `commitlint.config.js` with extends/plugins/rules. Adopters customize via config, never patch the core.
- **CI test-reporters** (Codecov, SonarCloud, CircleCI test-summary): consume lcov/cobertura/JUnit as lingua franca; never ship per-stack code.

[↑ TOC](#2-table-of-contents)

## 6. Chosen Approach

The chosen approach has five elements:

1. **Declarative `test-config.json`** with `targets[]`, each target declaring its `name`, `runner`, `command` per mode, `report` format, `coverage` config, `cwd`, and optional `env`.
2. **CTRF-shaped canonical internal model** (`CanonicalTestReport`) — the single data structure every downstream consumer reads.
3. **Input adapters**: native CTRF pass-through, JUnit XML → CTRF converter, custom (CodeFlow-canonical JSON) pass-through. JUnit XML is the universal path for stacks whose runners don't yet emit CTRF.
4. **Ordered coverage rule list** per target: `scope: per_file | per_package | per_module | changed_files | global`, with `include`/`exclude` globs per rule, first-matching-rule-wins for file-level scopes.
5. **Execution model**: sequential across targets by default; opt-in `execution.parallel: true`; fail-fast off; no retries.

Around this core, the engine ships:

- `codeflow test setup` wizard (interactive, `--auto`, `--template`, `--list`)
- `codeflow test doctor` validator (dry-run commands, path checks, schema validation)
- `codeflow test report <subcmd>` (show last, convert formats)
- `codeflow test config <subcmd>` (add-target, remove-target, enable/disable, set-command, set-threshold)
- `codeflow test exceptions <subcmd>` (add, remove, list — per-target)
- Template library at `.codeflow/templates/test-config/`
- JSON schema at `.codeflow/schemas/test-config.schema.json`
- Ledger event `test_result_recorded` on run completion

Claude artifacts are rewritten to reference only `codeflow test` as the opaque universal wrapper. Stack-specific skills (`cf-rust-standards`, `cf-python-standards`, `cf-shell-standards`) remain as **on-demand** skills that an implementer can load — they're reference material, not mandatory agent instructions.

[↑ TOC](#2-table-of-contents)

## 7. Wire Formats

### 7.1 JUnit XML (input, universal)

JUnit XML in the Ant/Surefire dialect is the de facto universal test output. Every major runner supports it:

| Runner | JUnit Support |
|--------|---------------|
| cargo-nextest | Built-in (`--profile <name>` with `junit.path`) |
| pytest | Built-in (`--junitxml=out.xml`) |
| jest | Via `jest-junit` reporter |
| vitest | Via `vitest junit` reporter |
| mocha | Via `mocha-junit-reporter` |
| go test | Via `gotestsum` or `go-junit-report` |
| rspec | Via `rspec_junit_formatter` |
| phpunit | Built-in (`--log-junit`) |

**Pros:** Ubiquitous; lossless on the fields CodeFlow needs (pass/fail/skipped counts, durations, failure messages, suite names); XML is straightforward to parse with `quick-xml`.

**Cons:** Dialects vary slightly (e.g., `testsuite/@tests` vs. nested `testcase` counts; pytest uses `classname` where nextest uses `classname` differently). Mitigation: round-trip fixture tests per-runner.

### 7.2 CTRF (input native + canonical internal)

CTRF (Common Test Report Format, ctrf.io) is a modern JSON format native to a growing roster of runners:

| Runner | CTRF Support |
|--------|--------------|
| jest | `jest-ctrf-json-reporter` |
| vitest | `vitest-ctrf-json-reporter` |
| playwright | `playwright-ctrf-json-reporter` |
| cypress | `cypress-ctrf-json-reporter` |
| mocha | `mocha-ctrf-json-reporter` |
| pytest | `pytest-ctrf` |
| go test | `go-ctrf-json-reporter` |
| cargo | **No native reporter** — JUnit→CTRF conversion required |

**Pros:** Native JSON (no XML parsing); explicit representation of flakiness, attachments, tags; designed as cross-ecosystem from day one.

**Cons:** Ecosystem still maturing; no Rust-native reporter today. Mitigation: ship the JUnit→CTRF converter as an engine feature and use it for Rust self-host.

### 7.3 Custom (escape hatch)

Adopters with exotic runners can emit CodeFlow-canonical JSON directly and declare `runner: custom`, `report.format: ctrf`. The engine reads it pass-through. This unblocks any conceivable test tool at the cost of the adopter writing a small emitter.

### 7.4 Rejected formats

- **TAP (Test Anything Protocol):** text-stream, lossy, no structured failure details. Rejected.
- **SARIF:** designed for static analysis, not test results. Rejected.
- **xUnit-custom JSON (various):** fragmented across languages, no single canonical variant. Rejected.

### 7.5 Coverage formats (four supported)

| Format | Source Tools | Parser Strategy |
|--------|-------------|-----------------|
| lcov (`lcov.info`) | llvm-cov, lcov, genhtml | Line-based parser (SF/DA/BA records) |
| cobertura (`cobertura.xml`) | Jest `--coverageReporters`, Python `coverage.py`, nyc | XML parser (quick-xml) |
| istanbul-summary (`coverage-summary.json`) | nyc, c8, Jest | JSON parser (serde_json) |
| go-cover (`coverage.out`) | `go test -cover -coverprofile` | Line-based parser |

Exotic tools pre-transform via `coverage.transform` (a shell command converting tool output to one of the four supported formats). This keeps the engine's parser surface small.

[↑ TOC](#2-table-of-contents)

## 8. Config Schema

### 8.1 Top-level structure

```json
{
  "$schema": ".codeflow/schemas/test-config.schema.json",
  "schema_version": "1.0",
  "execution": {
    "parallel": false,
    "fail_fast": false
  },
  "defaults": {
    "coverage": [{ "scope": "changed_files", "minimum": 85 }]
  },
  "targets": [
    {
      "name": "<target-name>",
      "enabled": true,
      "cwd": "<relative-path>",
      "env": { "KEY": "value" },
      "runner": "cargo | pytest | jest | vitest | go | mocha | rspec | phpunit | custom",
      "modes": {
        "quick": { "command": "<shell-command>" },
        "essential": { "command": "<shell-command>" },
        "full": { "command": "<shell-command>" }
      },
      "report": {
        "format": "junit | ctrf",
        "path": "<relative-to-cwd>",
        "derive_from": "junit"
      },
      "coverage": {
        "format": "lcov | cobertura | istanbul-summary | go-cover",
        "path": "<relative-to-cwd>",
        "transform": "<optional-shell-command>",
        "rules": [
          {
            "scope": "per_file | per_package | per_module | changed_files | global",
            "include": ["<glob>", "..."],
            "exclude": ["<glob>", "..."],
            "minimum": 85
          }
        ],
        "exceptions": [
          {
            "file": "<path>",
            "threshold": 70,
            "reason": "<text>",
            "remove_when": "<text>"
          }
        ]
      }
    }
  ]
}
```

### 8.2 Field descriptions

| Field | Required | Type | Description |
|-------|----------|------|-------------|
| `$schema` | yes | string | Path to JSON schema for editor validation |
| `schema_version` | yes | string | Semver of the config schema (current: `1.0`) |
| `execution.parallel` | no | bool | Run targets in parallel. Default false. |
| `execution.fail_fast` | no | bool | Stop on first target failure. Default false. |
| `defaults.coverage` | no | array | Default coverage rules applied when target omits them |
| `targets[]` | yes | array | One or more test targets (can be empty for fresh projects) |
| `targets[].name` | yes | string | Unique identifier, used in `--only`/`--skip` flags and PR body tables |
| `targets[].enabled` | no | bool | Skip entirely when false. Default true. |
| `targets[].cwd` | no | string | Working directory for commands. Default repo root. |
| `targets[].env` | no | object | Environment variables. Merged with inherited env. |
| `targets[].runner` | yes | string | Enum: cargo, pytest, jest, vitest, go, mocha, rspec, phpunit, custom |
| `targets[].modes.{mode}.command` | yes (≥1) | string | Shell command for each mode. Mode omission = skip that mode. |
| `targets[].report.format` | yes (when coverage/report needed) | string | Enum: junit, ctrf |
| `targets[].report.path` | yes | string | Where the report file appears after command runs (relative to cwd) |
| `targets[].report.derive_from` | no | string | `junit` — used when format is CTRF but emitted format is JUnit; triggers internal conversion |
| `targets[].coverage.format` | yes (when coverage rules exist) | string | Enum: lcov, cobertura, istanbul-summary, go-cover |
| `targets[].coverage.path` | yes | string | Where the coverage file appears after command runs |
| `targets[].coverage.transform` | no | string | Shell command to convert tool-native output to one of the four supported formats |
| `targets[].coverage.rules[]` | yes (when coverage is configured) | array | Ordered rule list. First-match-wins for file-level scopes. |
| `targets[].coverage.rules[].scope` | yes | string | Enum: per_file, per_package, per_module, changed_files, global |
| `targets[].coverage.rules[].include` | no | array of globs | Match paths matching any include (default: `["**/*"]`) |
| `targets[].coverage.rules[].exclude` | no | array of globs | Exclude paths matching any exclude (default: `[]`) |
| `targets[].coverage.rules[].minimum` | yes | int 0-100 | Coverage threshold in percent |
| `targets[].coverage.exceptions[]` | no | array | Per-file threshold overrides, recorded in PR body |
| `targets[].coverage.exceptions[].file` | yes | string | File path (repo-relative) |
| `targets[].coverage.exceptions[].threshold` | yes | int 0-100 | Lowered threshold for this file |
| `targets[].coverage.exceptions[].reason` | yes | string | Justification (shown in PR body) |
| `targets[].coverage.exceptions[].remove_when` | yes | string | Condition under which the exception should be removed |

### 8.3 Defaults

- `execution.parallel`: false
- `execution.fail_fast`: false
- `defaults.coverage`: `[{ "scope": "changed_files", "minimum": 85 }]`
- A missing mode on a target means that mode is **skipped** (not an error).
- A target with `enabled: false` is skipped entirely.
- An empty `targets[]` array means "no tests configured" — produces the fresh-project notice (see §12 scenario A).

### 8.4 JSON Schema file

Published at `.codeflow/schemas/test-config.schema.json` by TSK-002. The schema is JSON Schema draft 2020-12. `codeflow test doctor` validates every `test-config.json` against it on `codeflow init` and on every `setup` wizard completion.

### 8.5 Schema evolution policy

Configs evolve. The engine MUST handle unknown versions and unknown fields predictably:

| Condition | Behavior | Exit | Rationale |
|-----------|----------|------|-----------|
| `schema_version` matches a supported value (`1.0` in v1) | Accept | 0 | Nominal path |
| `schema_version` missing | Reject with "missing required field schema_version" | 2 | Force explicit versioning |
| `schema_version` is an unknown value (e.g., `2.0` when engine only knows `1.0`) | Reject with "unsupported schema_version=X; this engine supports: [1.0]; run `codeflow test config migrate` once available to upgrade" | 2 | Explicit failure, listing supported versions; never silently downgrade |
| Unknown top-level field (e.g., `defaults`, `execution`, `targets` are the only known — an unknown `foo_bar` appears) | Warn to stderr, accept | 0 | Lenient forward-compat — allows config written for a newer engine to run on an older engine |
| Unknown field inside a required section (e.g., `targets[].foo_bar`) | Reject with JSON path and "unknown field" | 2 | Strict — required sections have a known shape; unknown fields there indicate a typo or schema mismatch |
| Unknown enum value (e.g., `runner: "fabricate"` when enum is `[cargo, pytest, ...]`) | Reject with JSON path and valid enum list | 2 | Strict — enums are closed |
| Legacy field renamed in newer schema (post-v1 scenario) | Migration helper `codeflow test config migrate` (future) rewrites in place | — | Future scope; not v1 |

Rationale: lenient top-level + strict-inside-required lets adopters opt into unknown future fields without breaking older engines, while still catching real errors (typos in required sections). This pattern matches eslint, tsconfig, and most config-driven tools.

TSK-002 acceptance criteria cover the four behavior rows marked "v1" (unknown version, missing version, unknown top-level, unknown in required).

[↑ TOC](#2-table-of-contents)

## 9. Coverage Model

### 9.1 Rule list semantics

Each target's `coverage.rules[]` is evaluated in order. Rule types:

| Scope | Granularity | Match Set |
|-------|------------|-----------|
| `per_file` | One threshold per file | Files matching include/exclude |
| `per_package` | One threshold per package | Aggregated coverage of files in a package (language-specific grouping via `include`/`exclude`) |
| `per_module` | One threshold per module | Aggregated coverage of files in a module (finer than package, e.g. directory level) |
| `changed_files` | Per-file on files in the PR diff only | Files changed in the PR |
| `global` | Single threshold across the entire target | All files matching include/exclude |

**First-match-wins for file-level scopes** (`per_file`, `changed_files`): when both rules match a file, only the first rule in the list applies.

**All rules evaluated for non-file scopes** (`per_package`, `per_module`, `global`): a target with multiple `per_package` rules enforces each independently.

### 9.2 Example rule combinations

**Strict on changed files, lax on legacy:**

```json
"rules": [
  { "scope": "per_file", "include": ["legacy/**"], "minimum": 70 },
  { "scope": "changed_files", "minimum": 85 },
  { "scope": "global", "minimum": 75 }
]
```

Evaluation:
1. A changed file in `legacy/` → matches rule 1 (first match) → 70% threshold.
2. A changed file NOT in `legacy/` → matches rule 2 → 85% threshold.
3. An unchanged file in `legacy/` → matches rule 1 (per_file applies to all files, not just changed) → 70% threshold.
4. An unchanged file NOT in `legacy/` → no file-level match; rule 3 (global, aggregated) applies as a sanity floor across the whole target.

**Package-level with file-level overrides:**

```json
"rules": [
  { "scope": "per_package", "include": ["src/core/**"], "minimum": 90 },
  { "scope": "per_package", "include": ["src/**"], "minimum": 80 },
  { "scope": "changed_files", "minimum": 85 }
]
```

Evaluation: per_package rules both evaluated (non-file-level); `changed_files` rule applies to each changed file.

### 9.3 Default policy

When a target declares `coverage` but omits `rules`, defaults apply: `[{ "scope": "changed_files", "minimum": 85 }]`. This mirrors current user preference and provides a reasonable out-of-the-box behavior.

When a target omits `coverage` entirely, no coverage is collected or enforced for that target. The PR body shows `N/A` in the coverage column for that target.

### 9.4 Exceptions

Per-file exceptions are stored in `coverage.exceptions[]`. They override the matching rule's threshold for that specific file. Exceptions must include a `reason` and `remove_when` — these appear verbatim in the PR body's "Exempted Files" section.

Rationale: user-directive "exhaust coverage before threshold" — exceptions are the formal paper-trail for files that genuinely cannot hit the bar. `codeflow test exceptions add` requires both `--reason` and `--remove-when`; the CLI refuses to add an exception without them.

### 9.5 Modified-file coverage routing

`changed_files` scope resolves the PR diff via `git diff main...HEAD` (or `AUTORUN_INTEGRATION_BRANCH...HEAD` in autorun). Each changed file is then routed to a target using the deterministic rules below.

**Routing algorithm (ordered):**

1. **Normalize target cwd.** Empty string, `.`, or missing `cwd` → repo root (length-0 prefix). `./foo` → `foo`. Trailing slashes stripped.
2. **Filter by file-extension affinity (optional guard).** If a target declares `runner: cargo` or `runner: go`, files outside that language's extension set (`.rs` for cargo; `.go` for go) are NOT routed to that target even if its cwd matches — prevents a `runner: cargo` target from claiming a stray `.ts` file sitting in its cwd. For `runner: custom`, `runner: mocha`, etc., no extension guard applies (the adopter declared a catch-all runner intentionally).
3. **Longest-prefix match.** Among remaining candidates, select the target whose normalized cwd is the longest matching path prefix of the file. `cwd: "services/api"` beats `cwd: "services"` beats `cwd: ""` (root).
4. **Tie break by declaration order.** If two or more targets have identical `cwd` (and pass the extension guard), the target appearing FIRST in `targets[]` wins. Doctor emits a warning for identical cwds — adopters should differentiate via the extension guard (runner) or restructure to avoid the ambiguity.
5. **Empty-cwd catch-all.** A target with `cwd: ""` (or `.`) matches every file. Use when the adopter runs all tests from the repo root; declare it LAST in `targets[]` so earlier targets take precedence.
6. **No match.** Files matching no target appear in the PR body's "Unattributed" row with status FAIL (fail-closed). Adopters resolve this by adjusting `cwd` or adding a catch-all target.

**Examples:**

| Targets (in declaration order) | Changed file | Routed to | Reason |
|-------------------------------|--------------|-----------|--------|
| `[api: cwd=services/api, web: cwd=packages/web]` | `services/api/main.go` | api | longest prefix |
| `[api: cwd=services/api, web: cwd=packages/web]` | `README.md` | Unattributed | no match |
| `[api-go: cwd=., runner=go, web: cwd=., runner=vitest]` | `main.go` | api-go | extension guard: `.go` matches go runner only |
| `[api-go: cwd=., runner=go, web: cwd=., runner=vitest]` | `app.ts` | web | extension guard: `.ts` doesn't match go runner |
| `[a: cwd=services, b: cwd=services]` | `services/api/main.rs` | a | tie → declaration order; doctor warns |
| `[api: cwd=services/api, catchall: cwd=., runner=custom]` | `tools/deploy.sh` | catchall | no api match, catchall empty-cwd matches |

TSK-002 acceptance criteria cover each rule with a test. Doctor raises a warning when ambiguous cwds exist.

[↑ TOC](#2-table-of-contents)

[↑ TOC](#2-table-of-contents)

## 10. Modes & Execution

### 10.1 Universal mode vocabulary

Three modes: `quick | essential | full`. A target declares which of these it supports. Modes not declared are skipped for that target.

| Mode | Intent | Typical Command |
|------|--------|-----------------|
| `quick` | Fast sanity, < 30s | Filter to a test tag, skip integration |
| `essential` | Default developer loop, < 5min | Unit + fast integration |
| `full` | WS-QA gate, < 15min, coverage required | Everything + coverage |

Modes are **optional per target**. A target that only defines `full` will be skipped when `codeflow test --mode quick` is invoked (silent skip, not error).

### 10.2 Execution defaults

- **Sequential across targets** by default. Output interleaving is unambiguous; failures are easy to attribute.
- **Fail-fast off** by default. All targets run even after one fails — supports wider debugging.
- **Parallel opt-in** per project via `execution.parallel: true`. When enabled, each target runs in its own subprocess with its own `.state/test-reports/<target>/` and `.state/coverage/<target>/` subdirectories.
- **No retries** (user policy: fix flaky tests, don't retry).

### 10.3 `--only` / `--skip` / `--mode`

- `codeflow test --only <target>` runs only the named target (comma-separated for multiple).
- `codeflow test --skip <target>` runs everything except the named target.
- `codeflow test --mode <quick|essential|full>` selects the mode. Default: `essential`.
- Coverage collection is configured per-target in `.codeflow/config/testing/test-config.json` (`coverage.format`, `coverage.rules`, `coverage.exceptions`). Full-mode runs apply per-file thresholds automatically — there is no CLI flag to toggle coverage.

[↑ TOC](#2-table-of-contents)

## 11. PR Report Format

### 11.1 Target-aware tables

The PR body's Test Results section has up to five sub-sections, emitted as Markdown. The canonical heading is `## Test Results`. **`enforcement-policy.json` `git_format.pr.required_sections` MUST be updated by TSK-004 to match** — the current value (`## Test Stats` with `#### Exempted Files`) is legacy and disagrees with this design; gh-pr-guard will reject valid PRs until reconciled. The required-sections array post-migration is:

```json
["## Summary", "## Testing", "## Test Results", "### 1. Overall Test Pass Status", "### 2. Overall Coverage", "### 3. Modified File Coverage"]
```

(The per-target "Exempted Files" block is rendered as a nested sub-heading under §2 Overall Coverage, not a top-level required section — it appears only when exceptions exist.)

```markdown
## Test Results

### 1. Overall Test Pass Status

| Target | Mode | Passed | Failed | Skipped | Duration |
|--------|------|--------|--------|---------|----------|
| rust-core | full | 1,892 | 0 | 0 | 42.3s |
| shell-scripts | full | 412 | 0 | 0 | 8.7s |
| python-tools | full | 96 | 0 | 0 | 2.1s |
| **Total** | | **2,400** | **0** | **0** | **53.1s** |

Runs: 2 consecutive clean.

### 2. Overall Coverage

| Target | Workspace-equivalent | Per-rule summary |
|--------|--------------------:|------------------|
| rust-core | 88.2% | changed_files ≥85% ✓, global ≥75% ✓ |
| shell-scripts | 92.5% | per_file ≥85% ✓ |
| python-tools | 89.0% | changed_files ≥85% ✓ |

#### Exempted Files (per-target)

| Target | File | Coverage | Configured Threshold | Reason |
|--------|------|---------:|---------------------:|--------|
| rust-core | core/src/autorun/worker.rs | 79% | 79% | Autorun worker contains process spawning ... |

### 3. Modified File Coverage

| Target | File | Coverage | Threshold | Status |
|--------|------|---------:|----------:|--------|
| rust-core | core/src/testing/report/junit.rs | 92% | 85% | PASS |

### 4. Test Failures

(emitted only when Failed > 0, sourced from CTRF)

| Target | Suite | Test | Message | File:Line |
|--------|-------|------|---------|-----------|
| ... |

### 5. Slowest Tests (optional)

(emitted when available, top 10, sourced from CTRF durations)

| Target | Test | Duration |
|--------|------|---------:|
| ... |
```

### 11.2 Fresh-project no-tests path

When `targets[]` is empty or all targets are `enabled: false`:

```markdown
## Test Results

No test targets configured. Run `codeflow test setup` to add targets.
```

A single line replaces all five sections. This is **informational, not an error**. PRs with no test targets do not fail the PR-body gate. Downstream adopters can start with this state and add targets over time.

### 11.3 CTRF-enabled extras

When the canonical report is CTRF (either native or derived), we get extras:

- **Test Failures table** (§4) from CTRF `tests[].status = failed` entries, with `message`, `trace`, and source file extracted from CTRF fields.
- **Slowest Tests table** (§5) from CTRF `tests[].duration` — top 10 descending.

When only JUnit is available and not derived to CTRF, §4 still works (JUnit has failure messages); §5 uses `testcase/@time` attributes.

### 11.4 Authoring policy and output contract

The PR body's Test Results section is **populated verbatim by `codeflow test --mode full` structured output**. Teammates do not compose coverage numbers by hand. When a subsequent push modifies the branch, cf-git-operations re-runs `codeflow test --mode full` and replaces the Test Results section in the open PR body. This is an invariant of the gh-pr-guard hook: PR body must contain `## Test Results` with all required sub-section headings present (or the single-line no-tests notice).

**Dual output contract** — the engine emits two orthogonal representations of every run:

| Representation | Format | Consumer | Purpose |
|----------------|--------|----------|---------|
| Markdown block | Plain Markdown, verbatim | cf-git-operations | Pasted into PR body under `## Test Results`; the paste-target heading is emitted by cf-git-operations, then the engine's Markdown block (which itself starts at the `### 1. ...` level) is concatenated verbatim |
| Structured JSON | `--format json` on any subcommand | Ledger (`test_result_recorded` event), cross-session memory, programmatic consumers | Queryable; stable serde-derived schema |

cf-git-operations consumes the Markdown block, NOT the JSON — the Markdown is authoritative for the PR body. The JSON is authoritative for the ledger event payload. TSK-002 ACs #36 (Markdown emitter) and #46 (JSON schema) both apply; they are complementary, not alternatives.

[↑ TOC](#2-table-of-contents)

## 12. Usage Scenarios with Sample Configs

### 12.1 Scenario A — Fresh project (empty targets)

**Config:**

```json
{
  "$schema": ".codeflow/schemas/test-config.schema.json",
  "schema_version": "1.0",
  "targets": []
}
```

**Commands:**

```text
$ codeflow test
No test targets configured. Run `codeflow test setup` to add targets.
$ codeflow test --mode full
No test targets configured. Run `codeflow test setup` to add targets.
```

**PR body:** single-line notice (see §11.2).

### 12.2 Scenario B — Node (Vitest, native CTRF)

**Config:**

```json
{
  "$schema": ".codeflow/schemas/test-config.schema.json",
  "schema_version": "1.0",
  "targets": [
    {
      "name": "web",
      "runner": "vitest",
      "cwd": "packages/web",
      "modes": {
        "essential": { "command": "pnpm test --run --reporter=vitest-ctrf-json-reporter" },
        "full":      { "command": "pnpm test --run --coverage --reporter=vitest-ctrf-json-reporter" }
      },
      "report": { "format": "ctrf", "path": "ctrf-report.json" },
      "coverage": {
        "format": "istanbul-summary",
        "path": "coverage/coverage-summary.json",
        "rules": [
          { "scope": "changed_files", "minimum": 85 },
          { "scope": "global", "minimum": 80 }
        ]
      }
    }
  ]
}
```

**Commands:**

```text
$ codeflow test --mode essential        # pnpm test runs in packages/web; CTRF parsed directly
$ codeflow test --mode full             # adds coverage parsing; PR body populated
```

### 12.3 Scenario C — Go (std test + go-ctrf-json-reporter)

**Config:**

```json
{
  "targets": [
    {
      "name": "go-service",
      "runner": "go",
      "cwd": "services/api",
      "modes": {
        "essential": { "command": "go test ./... -json | go-ctrf-json-reporter -o ctrf.json" },
        "full":      { "command": "go test ./... -json -coverprofile=coverage.out | go-ctrf-json-reporter -o ctrf.json" }
      },
      "report": { "format": "ctrf", "path": "ctrf.json" },
      "coverage": {
        "format": "go-cover",
        "path": "coverage.out",
        "rules": [{ "scope": "changed_files", "minimum": 85 }]
      }
    }
  ]
}
```

Alternative: use `gotestsum --format=standard-verbose --junitfile=out.xml`, set `report.format: junit` + `report.derive_from: junit`; engine internally converts JUnit→CTRF.

### 12.4 Scenario D — Python (pytest with JUnit)

**Config:**

```json
{
  "targets": [
    {
      "name": "python-backend",
      "runner": "pytest",
      "cwd": "backend",
      "modes": {
        "essential": { "command": "pytest --junitxml=report.xml -q" },
        "full":      { "command": "pytest --junitxml=report.xml --cov --cov-report=xml:cov.xml" }
      },
      "report": { "format": "junit", "path": "report.xml", "derive_from": "junit" },
      "coverage": {
        "format": "cobertura",
        "path": "cov.xml",
        "rules": [
          { "scope": "per_file", "include": ["legacy/**"], "minimum": 60 },
          { "scope": "changed_files", "minimum": 85 }
        ]
      }
    }
  ]
}
```

### 12.5 Scenario E — Monorepo Go + Node + Python

**Config:**

```json
{
  "execution": { "parallel": true },
  "targets": [
    { "name": "api",   "runner": "go",     "cwd": "services/api",   "...": "..." },
    { "name": "web",   "runner": "vitest", "cwd": "packages/web",   "...": "..." },
    { "name": "tools", "runner": "pytest", "cwd": "tools/analytics","...": "..." }
  ]
}
```

**Commands:**

```text
$ codeflow test --mode full                     # all three run in parallel
$ codeflow test --mode essential --only web     # only web target
$ codeflow test --mode essential --skip tools   # api + web, skip tools
```

PR body shows all three in Pass-Status and Coverage tables with `Target` column distinguishing them.

### 12.6 Scenario F — CodeFlow self-host Rust (migration destination)

**Config** (authored by TSK-005, replaces current Rust-only `test-config.json`):

```json
{
  "$schema": ".codeflow/schemas/test-config.schema.json",
  "schema_version": "1.0",
  "execution": { "parallel": false },
  "targets": [
    {
      "name": "rust-core",
      "runner": "cargo",
      "cwd": "codeflow-cli",
      "modes": {
        "essential": { "command": "cargo nextest run --profile essential" },
        "full":      { "command": "cargo nextest run --profile full && cargo llvm-cov --no-run --lcov --output-path lcov.info" }
      },
      "report": { "format": "junit", "path": "target/nextest/full/junit.xml", "derive_from": "junit" },
      "coverage": {
        "format": "lcov",
        "path": "lcov.info",
        "rules": [
          { "scope": "per_file", "include": ["**/*.rs"], "minimum": 85 }
        ],
        "exceptions": [
          { "file": "core/src/autorun/worker.rs", "threshold": 79, "reason": "Autorun worker contains process spawning ...", "remove_when": "Integration test harness with mock tmux/Claude processes ..." },
          { "file": "cli/src/cmd/interactive.rs", "threshold": 65, "reason": "68+ tests cover all extractable pure functions ...", "remove_when": "run_launch refactored ..." },
          { "file": "cli/src/cmd/init.rs", "threshold": 70, "reason": "init.rs TUI wizard ...", "remove_when": "TUI event loop refactored ..." },
          { "file": "cli/src/cmd/autorun.rs", "threshold": 67, "reason": "Large CLI command file ...", "remove_when": "TUI event loops refactored ..." },
          { "file": "core/src/tui/mod.rs", "threshold": 0, "reason": "Pure module declaration file ...", "remove_when": "File gains executable code" },
          { "file": "core/src/tui/widgets/mod.rs", "threshold": 0, "reason": "Pure module re-export file ...", "remove_when": "File gains executable code" }
        ]
      }
    },
    {
      "name": "shell-scripts",
      "runner": "custom",
      "cwd": ".codeflow/testing",
      "modes": {
        "essential": { "command": "bash run-all-tests.sh --format=ctrf --output=$CODEFLOW_TEST_REPORT" },
        "full":      { "command": "bash run-all-tests.sh --format=ctrf --output=$CODEFLOW_TEST_REPORT --coverage" }
      },
      "report": { "format": "ctrf", "path": "ctrf.json" }
    }
  ]
}
```

Note: `shell-scripts` target is shown for illustration; self-host migration may merge shell coverage into the same run or leave it separate. TSK-005 makes the final call.

[↑ TOC](#2-table-of-contents)

## 13. `.state/` Enforcement Analysis

This section is the **complete interaction matrix** proving that no LLM workflow requires direct Edit/Write on `.state/**` or `.codeflow/config/testing/test-config.json`.

### 13.1 Protected resources

**Ground truth** (from `.codeflow/config/enforcement/enforcement-policy.json`): `protected_resources.high[]` enumerates specific `.state/<subdir>/**` entries (sentinels, session, runtime, ledger, db, logs, coordination, worktrees). There is **no `.state/**` wildcard**. `worktree_protection.patterns[]` mirrors the same specific enumeration, expanded with the `.git-worktrees/*/` prefix. Therefore the new test-artifact directories do NOT inherit protection automatically — **they must be added explicitly to both arrays by TSK-002** (cross-referenced in TSK-005's post-migration verification).

| Path | Protection | Enforcement source | Action required |
|------|------------|-------------------|-----------------|
| `.state/test-reports/**` | No LLM Edit/Write | Must be added to `protected_resources.high[]` AND `worktree_protection.patterns[]` | TSK-002 AC (new) |
| `.state/coverage/**` | No LLM Edit/Write | Must be added to `protected_resources.high[]` AND `worktree_protection.patterns[]` | TSK-002 AC (new) |
| `.codeflow/config/testing/test-config.json` | No LLM Edit/Write | Covered by existing `.codeflow/config/**` entry in `protected_resources.high[]` | No change required — the existing `.codeflow/config/**` wildcard covers this path |
| `.codeflow/schemas/test-config.schema.json` | LLM may Edit (schema authoring) | Covered by existing `.codeflow/config/**`? | **Decision:** place schema under `.codeflow/schemas/` NOT under `.codeflow/config/`, so the `.codeflow/config/**` protection does NOT cover it and adopters'/engine's schema authoring is unblocked. TSK-002 AC covers the path choice. |
| `.codeflow/templates/test-config/*.json` | LLM may Edit (template authoring) | Not protected — `.codeflow/templates/**` is not listed in `protected_resources[]` | No change required |

**Related memory note:** `project_worktree_protection_fix.md` documents an existing gap where `.git-worktrees/**` was placed in the `high` tier too broadly. This task's TSK-002 additions close the specific test-artifact slice of that gap; the broader worktree_protection expansion work remains a separate future task.

### 13.2 Need-to-CLI mapping (per agent)

The table below enumerates every plausible operation an LLM agent would want to perform on the protected resources, and shows the corresponding `codeflow` CLI subcommand that replaces a direct Edit/Write.

| Need | Agent | Direct-edit temptation | CLI replacement |
|------|-------|------------------------|-----------------|
| See last test run | cf-quality-assurance | Read `.state/test-reports/*.ctrf.json` | `codeflow test --report` |
| Compare runs | cf-quality-assurance | Diff `.state/test-reports/*.ctrf.json` | `codeflow test report diff --from <run-id> --to <run-id>` |
| Convert JUnit→CTRF | cf-quality-assurance | Write a conversion script | `codeflow test report convert --from junit --to ctrf --input <path> --output <path>` |
| Add a new target | any | Edit `test-config.json` | `codeflow test config add-target --name <n> --runner <r> --cwd <c> ...` |
| Remove a target | any | Edit `test-config.json` | `codeflow test config remove-target --name <n>` |
| Change a target's command | any | Edit `test-config.json` | `codeflow test config set-command --target <n> --mode <m> --command '<cmd>'` |
| Enable/disable a target | any | Edit `test-config.json` | `codeflow test config enable --target <n>` / `disable --target <n>` |
| Add a per-file exception | cf-development | Edit `test-config.json` | `codeflow test exceptions add --target <n> --file <f> --threshold <t> --reason '<r>' --remove-when '<w>'` |
| Remove an exception | cf-development | Edit `test-config.json` | `codeflow test exceptions remove --target <n> --file <f>` |
| List exceptions | any | Read `test-config.json` | `codeflow test exceptions list --target <n>` (or without `--target` for all) |
| Change a threshold | cf-development | Edit `test-config.json` | `codeflow test config set-threshold --target <n> --scope <s> --minimum <n>` |
| Initial config generation | cf-development | Write `test-config.json` from scratch | `codeflow test setup` (interactive) or `codeflow test setup --auto` |
| Apply a template | cf-development | Copy-paste template file | `codeflow test setup --template <name>` |
| Verify config validity | cf-quality-assurance | Read `test-config.json`, check by eye | `codeflow test doctor` |
| Purge coverage artifacts | cf-git-operations | rm-rf `.state/coverage/` | `codeflow test clean --artifacts` |
| Read ledger event for test result | cf-knowledge-layer | Read `.state/ledger/*.jsonl` | `codeflow test report show --run-id <id>` (reads ledger event) |

### 13.3 Acceptance invariant

**Every task's acceptance criteria list MUST include:**

> No task deliverable introduces an LLM code path that Edit/Writes into `.state/**` or `.codeflow/config/testing/test-config.json` directly. Every mutation has a corresponding `codeflow test <subcommand>`.

TSK-002 additionally ships the "CLI command surface matrix" above as a markdown appendix in the task doc, proving the exhaustive coverage before implementation begins. TSK-004 additionally includes a post-rewrite grep verification that no agent def, command def, or CLAUDE.md section instructs a teammate to Edit/Write the protected paths.

[↑ TOC](#2-table-of-contents)

## 14. CLI Surface

### 14.1 `codeflow test` (primary)

```text
USAGE:
    codeflow test [OPTIONS]

OPTIONS:
    --mode <mode>         quick | essential | full  (default: essential)
    --only <targets>      Comma-separated target names to run (exclusive of others)
    --skip <targets>      Comma-separated target names to skip
    --format <fmt>        Output format: human | json  (default: human)
    --fail-fast           Override config; stop on first target failure

Coverage enforcement is driven by `.codeflow/config/testing/test-config.json`
(`coverage.rules`, `coverage.exceptions`). `--mode full` applies per-file
thresholds automatically; there is no CLI flag to toggle coverage. Saved
reports are viewed via `codeflow test report show [--format human|json] [--run-id <id>]`.

EXIT CODES:
    0   All targets passed, all coverage thresholds met
    1   One or more targets failed OR coverage thresholds breached
    2   Config error (invalid schema, missing file, unresolved path)
    3   Infrastructure error (worktree resolution failed, ledger unreachable)
```

### 14.2 `codeflow test setup`

```text
USAGE:
    codeflow test setup [OPTIONS]

OPTIONS:
    --auto              Non-interactive; detect stacks from package.json, Cargo.toml, go.mod, pyproject.toml
    --template <name>   Write the named template; fails if test-config.json exists (use --force)
    --list              Print available template names and exit
    --add-target        Interactive wizard to add a single new target to existing config
    --force             Allow overwriting existing test-config.json

EXIT CODES:
    0   Config written and validated
    1   User cancelled or validation failed
    2   Config already exists and --force not passed
```

### 14.3 `codeflow test doctor`

```text
USAGE:
    codeflow test doctor [OPTIONS]

OPTIONS:
    --format <fmt>   human | json  (default: human)

CHECKS:
    1. test-config.json exists at .codeflow/config/testing/test-config.json
    2. JSON-schema validation passes against .codeflow/schemas/test-config.schema.json
    3. For each enabled target:
        a. cwd exists and is a directory
        b. Each mode's command parses as a shell command
        c. report.path is a valid relative path with no parent traversal
        d. coverage.path (if configured) is a valid relative path
        e. coverage.transform (if configured) parses as a shell command
        f. coverage.rules[].include/exclude globs parse validly
        g. Each exception's file resolves to an existing path (warning only)
    4. Dry-run each target's quick-mode command with --help or equivalent probe (warning on failure)

EXIT CODES:
    0   All checks pass
    1   One or more check errors (config invalid)
    2   Infrastructure error
```

### 14.4 `codeflow test report <subcmd>`

```text
codeflow test report show [--run-id <id>] [--format <fmt>]
    Print CTRF-shaped summary for a given run (or latest).

codeflow test report convert --from <junit|ctrf> --to <junit|ctrf> --input <path> --output <path>
    Convert between formats. JUnit→CTRF uses the engine's converter.

codeflow test report diff --from <run-id> --to <run-id>
    Show delta: new failures, regressed coverage.
```

### 14.5 `codeflow test config <subcmd>`

```text
codeflow test config show [--format <fmt>]
codeflow test config add-target --name <n> --runner <r> --cwd <c> [--mode <m>=<cmd>]...
codeflow test config remove-target --name <n>
codeflow test config enable --target <n>
codeflow test config disable --target <n>
codeflow test config set-command --target <n> --mode <m> --command '<cmd>'
codeflow test config set-threshold --target <n> --scope <s> --minimum <n> [--rule-index <i>]
codeflow test config set-report --target <n> --format <junit|ctrf> --path <p> [--derive-from junit]
codeflow test config set-coverage --target <n> --format <lcov|cobertura|istanbul-summary|go-cover> --path <p>
```

### 14.6 `codeflow test exceptions <subcmd>`

```text
codeflow test exceptions add --target <n> --file <f> --threshold <t> --reason '<r>' --remove-when '<w>'
codeflow test exceptions remove --target <n> --file <f>
codeflow test exceptions list [--target <n>]
```

### 14.7 `codeflow test clean`

```text
codeflow test clean [--artifacts] [--yes]
    Remove .state/test-reports/ and .state/coverage/ (current worktree only).
```

### 14.8 Output modes

All subcommands accept `--format human` (default, markdown/table) or `--format json` (structured). JSON output is consumed by cf-git-operations when populating the PR body and by cf-knowledge-layer when recording ledger events.

[↑ TOC](#2-table-of-contents)

## 15. Init & Setup Flow

### 15.1 `codeflow init` prompt

At the end of `codeflow init` (the project bootstrap command), a new prompt appears:

```text
Set up testing now? [y/N/skip]
  y     Run the interactive testing setup wizard (codeflow test setup)
  N     Create an empty test-config.json (you can run `codeflow test setup` later)
  skip  Do not create test-config.json at all
```

Default: N (empty config). The wizard lives in `codeflow test setup`, not `codeflow init` — init stays fast.

### 15.2 `codeflow test setup` (canonical entry point)

Flow:

1. **Detect existing config.** If `.codeflow/config/testing/test-config.json` exists, ask: "Existing config found. [E]dit / [R]eplace / [A]bort?"
2. **Detect stacks.** Look for sentinel files in priority order:
    - `Cargo.toml` (root or workspace members) → offer `rust-core` target
    - `package.json` (with `"devDependencies"` containing jest/vitest/mocha) → offer `web` target
    - `go.mod` → offer `go-service` target
    - `pyproject.toml` or `setup.py` (with pytest in deps) → offer `python` target
3. **Confirm detected targets.** For each, ask: "Add target `<name>` (`<runner>`, `<cwd>`)? [Y/n]"
4. **Per-target wizard.** For each confirmed target:
    - Ask for modes (quick, essential, full) — commands
    - Ask if coverage is configured; if yes, ask format, path, default rules
5. **Validate with doctor.** Run `codeflow test doctor` against the generated config; fail back to edit prompts on error.
6. **Write** (via the internal config-writer, NOT LLM Edit).
7. **Summary.** Print the final config and a "next steps" message: `codeflow test --mode essential` to try it.

### 15.3 `codeflow test setup --auto`

Non-interactive. Detects stacks via the same file sentinels; emits best-guess commands per stack:

| Sentinel | Target | Commands (best-guess) |
|----------|--------|----------------------|
| `Cargo.toml` | rust-core | `cargo nextest run` (essential), `cargo nextest run --profile full` + `cargo llvm-cov` (full) |
| `package.json` + vitest | web | `pnpm test --run` (essential), `pnpm test --run --coverage --reporter=vitest-ctrf-json-reporter` (full) |
| `package.json` + jest | web | `jest --ci` (essential), `jest --ci --coverage --reporters=jest-junit` (full) |
| `go.mod` | go-service | `go test ./...` (essential), `go test ./... -coverprofile=coverage.out -json \| go-ctrf-json-reporter` (full) |
| `pyproject.toml` + pytest | python | `pytest -q` (essential), `pytest --cov --cov-report=xml --junitxml=report.xml` (full) |

If a runner's CTRF reporter isn't detected, `--auto` falls back to JUnit + `derive_from: junit`.

### 15.4 `codeflow test setup --template <name>`

Writes one of the named templates from `.codeflow/templates/test-config/`. Template library:

| Template | Use case |
|----------|----------|
| `minimal.json` | Empty `targets[]` — the fresh-project state |
| `single-target-basic.json` | One target, essential+full modes, no coverage |
| `single-target-with-coverage.json` | One target with lcov + rules + changed_files default |
| `monorepo-multi-target.json` | Three targets (api, web, tools), parallel execution |
| `hooks-escape-hatch.json` | One target using `runner: custom` with a shell script |
| `example-node.json` | Node + vitest + istanbul-summary |
| `example-python.json` | Python + pytest + cobertura |
| `example-go.json` | Go + go-ctrf-json-reporter + go-cover |
| `example-rust.json` | Rust + cargo-nextest + lcov (matches self-host) |

### 15.5 `codeflow test setup --list`

Prints template names, one per line, to stdout. Used by shell-completion and scripting.

### 15.6 `codeflow test doctor` checks

See §14.3 for the full check list. Doctor is run automatically at the end of `setup` and is a standalone command for drift detection. Cron-friendly via `--format json`.

[↑ TOC](#2-table-of-contents)

## 16. Worktree Parallelism

### 16.1 Per-worktree state isolation

`.state/test-reports/` and `.state/coverage/` are **LOCAL** in `WorktreePaths` (not symlinked to the main repo). Each worktree gets its own test-result namespace. Two concurrent sessions running `codeflow test` in separate worktrees:

- Write to separate `<worktree>/.state/test-reports/<run-id>.ctrf.json`
- Write to separate `<worktree>/.state/coverage/<target>/<format>`
- Do not collide on the filesystem

### 16.2 Ledger events

The ledger `test_result_recorded` event is emitted per-run, per-worktree, and the worktree's ledger file is LOCAL (per PR #221). The CRDT state.loro continues to be shared, but test results don't go there — they go through the ledger.

### 16.3 WorktreePaths extension (TSK-002)

```rust
impl WorktreePaths {
    /// Return .state/test-reports/ (LOCAL, per-worktree)
    pub fn test_reports_dir(&self) -> PathBuf {
        self.root.join(".state").join("test-reports")
    }

    /// Return .state/coverage/ (LOCAL, per-worktree)
    pub fn coverage_dir(&self) -> PathBuf {
        self.root.join(".state").join("coverage")
    }
}
```

Added as acceptance criteria in TSK-002.

### 16.4 Autorun implications

Autorun workers each run in their own worktree; each worker's `codeflow test --mode full` runs its target set in isolation. Concurrent workers do not share test-report state. Merge-queue PR creation reads each worker's ledger events to populate the PR body.

[↑ TOC](#2-table-of-contents)

## 17. Migration Plan

### 17.1 Sequence

| Step | Task | Action |
|------|------|--------|
| 1 | TSK-002 | Engine lands: config loader, JUnit parser, CTRF model, coverage parsers, threshold engine, PR-body emitter, new `codeflow test` subcommands. Old `codeflow test` path remains as fallback during self-host migration. |
| 2 | TSK-003 | Setup wizard + doctor + template library. |
| 3 | TSK-004 | Claude artifact rewrite: CLAUDE.md, cf-quality-assurance, cf-development, cf-review, cf-git-operations, commands, gh-pr-guard target-awareness. |
| 4 | TSK-005 | Self-host migration. Author CodeFlow's own `test-config.json` (Scenario F). Add `codeflow-cli/.config/nextest.toml` with a `full` profile and `junit.path` set. Migrate the six existing coverage exceptions 1:1 into the new config's `exceptions[]`. Run `codeflow test --mode full` and compare PR body output to the previous engine's output — byte-equivalent for the test-pass-status and coverage tables. Delete the old Rust-specific hardcodes from `TestValidator::full_validate`. Add `.state/test-reports/` and `.state/coverage/` to root `.gitignore`. |

### 17.2 Rollback posture

**Decision (post-review):** clean cutover, no feature flag. If TSK-005's parity check fails (new engine produces different numbers than old engine for a known-good branch), **roll back by `git revert` of the migration PR**. The legacy `TestValidator::full_validate` path is deleted in the same PR that introduces the new engine; there is no `CODEFLOW_TEST_ENGINE=legacy` env-flag.

Rationale for clean cutover over feature-flag retention:
- TSK-005 is explicitly a migration task; carrying legacy code alongside new code doubles the surface area indefinitely.
- Feature flags become permanent if no one flips them off; the CodeFlow codebase has no mechanism to force deprecation.
- `git revert` is a single atomic action and restores the known-good engine state with no partial-adoption risk.
- Parity check (TSK-005 AC#9) catches divergences before merge; revert is only used if a regression slips through review.

TSK-005 AC#16 is the canonical statement of this decision; this section and ADR-001 §Consequences #Risks both align.

### 17.3 Existing test coverage exceptions

Six existing exceptions are in `codeflow-cli/config/testing/test-config.json` under `conventions.exceptions[]`:

- `core/src/autorun/worker.rs` (79%)
- `cli/src/cmd/interactive.rs` (65%)
- `cli/src/cmd/init.rs` (70%)
- `cli/src/cmd/autorun.rs` (67%)
- `core/src/tui/mod.rs` (0%)
- `core/src/tui/widgets/mod.rs` (0%)

These migrate 1:1 to the new config's `targets[0].coverage.exceptions[]` (Scenario F). TSK-005 includes an explicit acceptance criterion verifying byte-equivalent `reason` and `remove_when` fields, since those appear in the PR body.

### 17.4 Old file removal

The current `codeflow-cli/config/testing/test-config.json` moves to `.codeflow/config/testing/test-config.json`. TSK-005 includes a migration step deleting the old path and updating all hardcoded references in code (grep `codeflow-cli/config/testing` and replace).

[↑ TOC](#2-table-of-contents)

## 18. Risks & Mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|------|-----------|--------|------------|
| 1 | JUnit dialect drift produces wrong pass/fail counts | M | H | Round-trip fixture tests per-runner (cargo-nextest, pytest, jest, vitest) shipped with TSK-002 |
| 2 | CTRF ecosystem immaturity for Rust silently blocks self-host | L | H | Ship JUnit→CTRF converter as first-class engine feature; Scenario F uses it |
| 3 | Schema evolution breaks adopter configs silently | M | H | `schema_version` field; doctor fails loudly on unknown version; migration helper command `codeflow test config migrate` for future versions |
| 4 | Cross-platform shell compatibility (cmd.exe vs POSIX) | M | M | Document POSIX-shell-only in v1; argv-array form as future-safe path flagged in TSK-002 backlog |
| 5 | Per-target parallel execution overloads CI runners | L | M | `execution.parallel` opt-in, defaults off; per-target subprocess isolation means one runaway doesn't crash others |
| 6 | Coverage parser bugs silently skip files | M | H | Golden-fixture tests per parser (lcov, cobertura, istanbul-summary, go-cover); snapshot tests for the canonical report output |
| 7 | `test-config.json` config-writer introduces JSON formatting drift (breaking review diffs) | M | L | Deterministic serialization (sorted keys, stable indent); golden-fixture test for every CLI subcommand that mutates config |
| 8 | Claude artifact rewrite (TSK-004) accidentally introduces stack-specific text elsewhere | M | H | TSK-004 acceptance includes post-migration grep for `cargo|rust|crate|llvm-cov|85%` across `.claude/` and `project-management/templates/`; any match is a FAIL |
| 9 | Fresh-project PR body "no tests" single-line notice is misread as a failure by reviewers | L | L | Clear text: "No test targets configured. Run `codeflow test setup` to add targets." — informational, not an error |
| 10 | Legacy `TestValidator::full_validate` code path left orphaned after cutover | L | L | TSK-005 acceptance includes direct deletion of the legacy path in the same PR as the cutover (no feature flag). Rollback is a `git revert` of the TSK-005 merge commit. Aligned with §17.2 and ADR-001 §Consequences. |

[↑ TOC](#2-table-of-contents)

## 19. References

- **CTRF specification:** <https://ctrf.io> — Common Test Report Format, JSON schema, reporter plugins
- **JUnit XSD (Ant Surefire dialect):** <https://github.com/jenkinsci/xunit-plugin/blob/master/src/main/resources/org/jenkinsci/plugins/xunit/types/model/xsd/junit-10.xsd>
- **lcov format:** <http://ltp.sourceforge.net/coverage/lcov/geninfo.1.php> — SF/DA/BA records
- **cobertura format:** <https://github.com/cobertura/cobertura/blob/master/cobertura/src/site/xsd/coverage-04.xsd>
- **istanbul/nyc coverage-summary.json:** <https://istanbul.js.org/>
- **go coverage profile format:** <https://pkg.go.dev/cmd/go/internal/test#hdr-Testing_flags>
- **pre-commit framework architecture:** <https://pre-commit.com/#adding-pre-commit-plugins-to-your-project> — declarative config analog
- **commitlint configuration model:** <https://commitlint.js.org/reference/configuration.html> — extends/plugins/rules pattern
- **cargo-nextest profiles:** <https://nexte.st/book/configuration.html#profiles> — self-host test runner
- **cargo-llvm-cov:** <https://github.com/taiki-e/cargo-llvm-cov> — self-host coverage tool
- **ADR-001:** [../adr/ADR-001-generic-testing-subsystem.md](../adr/ADR-001-generic-testing-subsystem.md)
- **Epic INF-EPC-046:** [../../../project-management/epics/INF/INF-EPC-046/INF-EPC-046.md](../../../project-management/epics/INF/INF-EPC-046/INF-EPC-046.md)
- **Shadow testing architecture (prior art):** [./shadow-testing-architecture.md](./shadow-testing-architecture.md)
- **Worktree path resolution:** `codeflow-cli/core/src/worktree/paths.rs` — `WorktreePaths`
- **Current Rust test-config:** `codeflow-cli/config/testing/test-config.json` — to be replaced

[↑ TOC](#2-table-of-contents)

## 20. Dual-Config Architecture

CodeFlow currently ships two `test-config.json` files that serve different subsystems. Understanding both is essential for contributors adding test coverage or modifying the testing infrastructure.

### 20.1 `.codeflow/testing/test-config.json` (shell structural checker)

**Location:** `.codeflow/testing/test-config.json`

**Schema version:** `2.0.0` (local schema at `.codeflow/testing/test-config.schema.json`)

**Purpose:** Controls the shell-based structural checker — a priority-driven test orchestrator that invokes individual `.sh` test scripts. This config is read and executed exclusively by the Bash scripts in `.codeflow/testing/`, primarily `run-all-tests.sh`.

**Schema shape (top-level keys):**

| Key | Type | Purpose |
|-----|------|---------|
| `priorities` | object | Groupings of test scripts by CRITICAL / HIGH / MEDIUM / LOW |
| `modes` | object | Which priority groups run under `essential`, `standard`, or `full` |
| `categories` | object | Directory-to-description mapping for test script organization |
| `pre_commit` | object | Pre-commit hook behavior (default mode, env override, consistency check) |
| `cli` | object | CLI-level overrides (allow priority filter, mode override) |
| `parallel` | object | Parallel job limit and progress style |
| `coverage_enforcement` | object | Shell-level coverage: structural threshold (85%), `kcov` is informational only |
| `reporting` | object | Output directory, formats, retention days |

**When it is read:** Whenever a shell-based test runner is invoked directly — typically by the git pre-commit hook, CI pipelines calling `run-all-tests.sh`, or any direct invocation of scripts under `.codeflow/testing/`. The `codeflow test` binary does NOT read this file.

**What it tests:** Shell scripts, security protection scripts, git hooks, consistency checks, and Rust CLI bridge tests (the `cli/rust/test-rust-cli.sh` bridge, which itself calls into the Rust crate test suite).

### 20.2 `.codeflow/config/testing/test-config.json` (generic engine, canonical)

**Location:** `.codeflow/config/testing/test-config.json`

**Schema version:** `1.0` (JSON Schema at `.codeflow/schemas/test-config.schema.json`)

**Purpose:** The declarative configuration for the generic testing engine — the one read and executed by the `codeflow test` CLI binary (implemented in `codeflow-cli/core/src/testing/`). This is the canonical post-migration config introduced by INF-EPC-046 (PR #293).

**Schema shape (top-level keys):**

| Key | Type | Purpose |
|-----|------|---------|
| `schema_version` | string | Required; must be `1.0`; engine rejects unknown versions |
| `execution` | object | `parallel` (bool) and `fail_fast` (bool) across targets |
| `defaults` | object | Default `coverage[]` rules applied when a target omits them |
| `targets` | array | Ordered list of test targets; each fully specifies its runner, modes, report, and coverage config |

**When it is read:** By `codeflow test [--mode <m>]` (the Rust CLI binary). The config is loaded by `codeflow-cli/core/src/testing/config/mod.rs` at invocation time and drives the entire pipeline: runner dispatch, report parsing, coverage threshold evaluation, and PR body generation. Coverage enforcement is driven by config rules, not a CLI flag.

**What it tests:** Rust workspace (via cargo nextest + llvm-cov), shell scripts (via `run-all-tests.sh` as a `custom` runner target), and any additional targets added by adopters.

### 20.3 Why both exist

The two configs coexist because they target architecturally separate concerns:

| Dimension | Shell structural checker (`.codeflow/testing/test-config.json`) | Generic engine (`.codeflow/config/testing/test-config.json`) |
|-----------|----------------------------------------------------------------|--------------------------------------------------------------|
| Caller | `bash run-all-tests.sh` | `codeflow test` CLI binary |
| Language | Bash (interpreted) | Rust (compiled) |
| Test scope | Shell scripts, hooks, consistency | Multi-target, multi-language |
| Coverage | Structural: "does every script have a test file?" | Quantitative: per-file line-hit thresholds via lcov/cobertura/etc. |
| Report format | Custom JSON (priority-driven result set) | CTRF-shaped canonical model |
| Protection | Unprotected (shell caller reads it) | Protected: `.codeflow/config/**` in `enforcement-policy.json` — all mutations via `codeflow test config` CLI |

The shell checker predates the generic engine. It filled the gap during CodeFlow's bootstrap era when a compiled test engine did not exist. The generic engine (INF-EPC-046) is the strategic replacement.

### 20.4 Convergence plan

The shell structural checker is a **legacy holdover**. The roadmap calls for its responsibilities to be absorbed into the generic engine:

1. **Near term (current epic, completed):** The generic engine covers the Rust and shell-script test targets via `test-config.json`. The shell checker continues to run for structural coverage and git-hook integration.
2. **Medium term:** Once the generic engine gains coverage-format support for shell coverage reports (or a reliable structural-check target), the shell checker can be retired. This requires the `run-all-tests.sh` output to be consumable by the generic engine (e.g., as a CTRF emitter).
3. **Retirement condition:** When `codeflow test --mode full` produces equivalent structural coverage results to the shell checker, remove `.codeflow/testing/test-config.json` and its schema, and update the pre-commit hook to invoke `codeflow test` exclusively.

Until retirement, both files must be kept in sync for the test targets they share (the shell-scripts target). Changes to shell test file organization should be reflected in both configs.

[↑ TOC](#2-table-of-contents)

## 21. Zero-Coverage File Detection

### 21.1 Behavior

Files with no coverage data in the coverage report (e.g., absent from `lcov.info`) are **silently skipped** by the threshold engine. They do not appear in `ThresholdResult` output, are not counted as passing or failing, and do not appear in the PR body's Modified File Coverage table.

This behavior originates in `codeflow-cli/core/src/testing/validation.rs` in the `parse_target_coverage` function. The function checks whether the coverage file path exists before parsing:

```rust
// validation.rs — parse_target_coverage
let cov_path = match &run_result.coverage_path {
    Some(p) if p.exists() => p,
    _ => return Ok(Vec::new()),  // <-- empty vec if file absent
};
```

An absent file returns an empty `Vec<FileCoverage>`. The threshold engine in `codeflow-cli/core/src/testing/threshold/mod.rs` (`evaluate_file_thresholds`) iterates only over files present in that vec:

```rust
// threshold/mod.rs — evaluate_file_thresholds
for cov in coverages {
    // files absent from lcov.info are never in `coverages`
    // → no ThresholdResult is generated for them
}
```

The consequence: a production file that is never executed by any test will not appear in `lcov.info`. The threshold engine will not generate a failing `ThresholdResult` for it. The file is invisible to coverage enforcement.

### 21.2 Workaround

To make a zero-coverage file visible to enforcement, add it to the `exceptions` list in `.codeflow/config/testing/test-config.json` with an explicit `threshold: 0`:

```json
{
  "file": "core/src/my_new_module.rs",
  "threshold": 0,
  "reason": "No tests written yet — module is unreachable until TSK-XXX wires it in",
  "remove_when": "First test for this module is written"
}
```

This records the deliberate absence in the PR body's Exempted Files section and signals reviewers that coverage is intentionally deferred. Use `codeflow test exceptions add` rather than editing the config directly:

```bash
codeflow test exceptions add \
  --target rust-core \
  --file core/src/my_new_module.rs \
  --threshold 0 \
  --reason "No tests written yet — module is unreachable until TSK-XXX wires it in" \
  --remove-when "First test for this module is written"
```

### 21.3 Author guidance

The silent-skip behavior creates a coverage blind spot: a production file can exist, contain logic, and receive zero test coverage without triggering any enforcement failure. Authors must be diligent:

- **New modules:** Add a zero-threshold exception immediately when a new source file is created if tests are deferred. This makes the absence explicit and visible in PRs.
- **Exceptions are temporary:** Every exception must have a concrete `remove_when` condition. Exceptions without a removal path accumulate silently and erode the value of coverage enforcement over time.
- **Exception audit:** `codeflow test exceptions list` shows all current exceptions across all targets. Review this list during epic planning to identify exceptions ready for removal.
- **Zero is not free:** A `threshold: 0` exception is not a statement that the file needs no tests — it is a statement that tests are deferred and the exception will be removed when tests are added.

The user directive on coverage exceptions applies: exhaust coverage before lowering thresholds. Add tests for all pure functions in a file before recording an exception for the remaining untestable paths.

[↑ TOC](#2-table-of-contents)

## 22. Parallel Worktree Path Safety

### 22.1 Guarantee

`report.path` and `coverage.path` values in `.codeflow/config/testing/test-config.json` are resolved **relative to the target's `cwd`**, which is itself resolved relative to the **per-worktree project root** (`project_dir`). This means every test artifact path is local to the worktree that ran the test — not to any shared `.state/` directory.

Two concurrent autorun workers running `codeflow test --mode full` in separate worktrees:

- Produce coverage files at `<worktree-A>/<cwd>/lcov.info` and `<worktree-B>/<cwd>/lcov.info` respectively.
- Produce report files at `<worktree-A>/<cwd>/target/nextest/full/junit.xml` and `<worktree-B>/<cwd>/target/nextest/full/junit.xml` respectively.
- Never read or write each other's artifacts.

### 22.2 Resolution logic

The path resolution is implemented in `codeflow-cli/core/src/testing/runner/mod.rs`:

```rust
// runner/mod.rs — run_target
let cwd = resolve_cwd(project_dir, target.cwd.as_deref())?;

let report_path = target.report.as_ref().map(|r| cwd.join(&r.path));
let coverage_path = target.coverage.as_ref().map(|c| cwd.join(&c.path));
```

Where `cwd` is `project_dir / target.cwd`. In worktree mode, `project_dir` is the per-worktree root (the path in `.git-worktrees/worktree-{SID}/`). This means:

- `project_dir` is worktree-local and unique per session.
- `cwd` is `project_dir + target.cwd` — still worktree-local.
- All artifact paths derived from `cwd` are therefore per-worktree, never shared.

The `parse_target_coverage` function in `validation.rs` receives `run_result.coverage_path` (already fully resolved by the runner) and strips the `cwd_dir` prefix for exception matching:

```rust
// validation.rs — parse_target_coverage (path normalization)
if let Some(cwd_dir) = run_result.coverage_path.as_ref().and_then(|p| p.parent()) {
    for cov in &mut coverages {
        if let Ok(relative) = std::path::Path::new(&cov.path).strip_prefix(cwd_dir) {
            cov.path = relative.to_string_lossy().to_string();
        }
    }
}
```

This normalization converts absolute paths from the coverage tool (e.g., `llvm-cov` reports absolute source paths) to paths relative to the target `cwd`. Exception entries in `test-config.json` must use these cwd-relative paths (e.g., `core/src/autorun/worker.rs`, not an absolute path).

### 22.3 Author guidance for `test-config.json` adopters

When authoring or modifying `test-config.json` for a new target:

- **`report.path` must be relative to the target's `cwd`.** Do not use absolute paths or paths that reach outside the `cwd` subtree. `../` traversal is rejected by `codeflow test doctor`.
- **`coverage.path` must be relative to the target's `cwd`.** The coverage file is generated by the test runner command in `cwd`; declare its output path accordingly. For the Rust target, `lcov.info` is generated in `codeflow-cli/` (the `cwd`), so `coverage.path: "lcov.info"` resolves to `<worktree>/codeflow-cli/lcov.info`.
- **`coverage.exceptions[].file` paths must be cwd-relative.** Exception matching uses the normalized path after stripping `cwd_dir`. For the Rust target with `cwd: "codeflow-cli"`, the exception path `core/src/autorun/worker.rs` is correct; `codeflow-cli/core/src/autorun/worker.rs` would NOT match.
- **Do not reference `.state/` in artifact paths.** Test artifacts belong in the target's `cwd` subtree, not in `.state/`. The engine's ledger event and PR body use `.state/` internally, but those paths are managed by the engine, not by `test-config.json`.
- **Parallel autorun safety is automatic** if these rules are followed. The worktree isolation is structural: as long as artifact paths are cwd-relative, multiple workers writing to the same `test-config.json` settings will write to their own worktree copies of those paths.

[↑ TOC](#2-table-of-contents)
