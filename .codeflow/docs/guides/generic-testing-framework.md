---
title: Generic Testing Framework — User Guide
type: guide
status: current
created: 2026-04-16
updated: 2026-04-18
---

# Generic Testing Framework

<!-- TOC -->

## Table of Contents

- [1. Overview](#1-overview)
- [2. Architecture](#2-architecture)
- [3. Quickstart](#3-quickstart)
- [4. Configuration Reference](#4-configuration-reference)
- [5. Per-Stack Guides](#5-per-stack-guides)
  - [5a. Rust](#5a-rust)
  - [5b. Node.js](#5b-nodejs)
  - [5c. Python](#5c-python)
  - [5d. Go](#5d-go)
  - [5e. Monorepo](#5e-monorepo)
  - [5f. Custom Runner](#5f-custom-runner)
- [6. Coverage Model](#6-coverage-model)
- [7. CLI Command Reference](#7-cli-command-reference)
- [8. PR Body Format](#8-pr-body-format)
- [9. WS-QA Integration](#9-ws-qa-integration)
- [10. Template Library](#10-template-library)
- [11. Troubleshooting](#11-troubleshooting)
- [12. CI Integration](#12-ci-integration)
- [13. Structural Check](#13-structural-check)
- [14. Test Tags](#14-test-tags)
- [15. Adopter Migration (Dual-Config to Single-Config)](#15-adopter-migration-dual-config-to-single-config)

<!-- /TOC -->

---

## 1. Overview

[^ back to top](#generic-testing-framework)

The Generic Testing Framework lets you run tests for any language or framework using a single command — `codeflow test --mode full` — regardless of whether your project is in Rust, Node.js, Python, Go, or a mixed monorepo.

**Elevator pitch:** CodeFlow provides the orchestration engine; you provide the shell commands. Declare your test runner commands, report file paths, and coverage rules once in `.codeflow/config/testing/test-config.json`. After that, every teammate, hook, and CI step calls `codeflow test` — and the engine handles running targets, parsing results, evaluating coverage thresholds, generating the PR body, and emitting ledger events.

**What it does:**

- Config-driven, stack-agnostic test orchestration — one interface for every language
- Declarative coverage enforcement with five scope types and first-match-wins rule evaluation
- Canonical CTRF-shaped internal model from JUnit or native CTRF reporters
- PR body generation with per-target pass/fail stats, coverage summary, and exempted-files table
- Ledger events for session-scoped test history

**Why it exists:**

CodeFlow's original testing subsystem hard-coded `cargo test`, `cargo llvm-cov`, and an 85% per-file lcov rule — the Rust self-host assumptions leaked into CLAUDE.md, agent definitions, and QA checklists. Downstream adopters on Node, Go, or Python stacks couldn't use `codeflow test` without forking source code. The Generic Testing Framework decouples the engine from the stack, making all Claude artifacts stack-agnostic. See [ADR-001](../adr/ADR-001-generic-testing-subsystem.md) for the full decision record.

**Problems it solves:**

| Problem | Solution |
|---------|---------|
| Multi-stack projects with different test runners | Multiple named `targets` in one config file |
| Per-file coverage enforcement | Declarative `rules` with `per_file` scope |
| PR body generation without manual test stats | Engine renders sections 1-5 from parsed results |
| Ledger audit trail for test history | `test_result_recorded` event emitted per run |
| Stack-locked Claude artifacts | Agents call only `codeflow test`, never runner commands |

**Cross-references:** [ADR-001](../adr/ADR-001-generic-testing-subsystem.md) · [ADR-002](../adr/ADR-002-testing-consolidation.md) · [Design doc §12 (Usage Scenarios)](../analysis/generic-testing-subsystem.md) · [CLAUDE.md §7](../../../.claude/CLAUDE.md) · [cf-quality-assurance](../../../.claude/agents/cf-quality-assurance.md)

---

## 2. Architecture

[^ back to top](#generic-testing-framework)

### 2a. Full Pipeline Flow

```text
.codeflow/config/testing/test-config.json
              |
              v
     ┌─────────────────┐
     │  Config Loader  │  codeflow-cli/core/src/testing/config/mod.rs
     │  (load + valid) │  Validates schema_version, rejects unknown fields
     └────────┬────────┘
              |  Vec<TargetConfig>
              v
     ┌─────────────────┐
     │  Runner         │  codeflow-cli/core/src/testing/runner/mod.rs
     │  (shell cmds)   │  Spawns each target's mode command in target's cwd
     └────────┬────────┘
              |  report file(s) written to target cwd
              v
     ┌──────────────────────────────────────────────────┐
     │  Report Parsers                                  │
     │    JUnit XML  → codeflow-cli/core/src/testing/   │
     │                 report/junit.rs + junit_to_ctrf  │
     │    CTRF JSON  → report/ctrf.rs                   │
     └───────────────────────┬──────────────────────────┘
                             |
                             v
                   ┌──────────────────┐
                   │  CanonicalTest-  │  report/mod.rs
                   │  Report (CTRF)   │  CtrfTest, CtrfSummary, CtrfStatus
                   └────────┬─────────┘
                            |
              ┌─────────────┼────────────┐
              v             v            v
    ┌──────────────┐  ┌──────────┐  ┌─────────────────────────┐
    │  Coverage    │  │Threshold │  │  PR Body Renderer       │
    │  Parsers     │  │Engine    │  │  pr_body.rs             │
    │  lcov        │  │threshold │  │  Sections 1-5 markdown  │
    │  cobertura   │  │/mod.rs   │  └───────────┬─────────────┘
    │  istanbul    │  │first-    │              |
    │  go-cover    │  │match-    │              v
    │  coverage/   │  │wins      │     .state/test-reports/
    │  mod.rs      │  └──────────┘     (per-worktree LOCAL)
    └──────────────┘
              |
              v
     ┌────────────────────┐
     │  Ledger Event      │  test_result_recorded
     │  .state/ledger/    │  (CTRF summary + coverage summary)
     └────────────────────┘
```

### 2b. CTRF Data Model

The engine converts all input formats to a single canonical representation before evaluation:

```text
CanonicalTestReport
├── target_name: String
└── results: CtrfResults
    ├── summary: CtrfSummary
    │   ├── total: u64
    │   ├── passed: u64
    │   ├── failed: u64
    │   ├── skipped: u64
    │   ├── pending: u64
    │   ├── other: u64
    │   ├── start: u64   (epoch ms)
    │   └── stop: u64    (epoch ms)
    └── tests: Vec<CtrfTest>
        ├── name: String          (fully qualified test name)
        ├── status: CtrfStatus    (passed | failed | skipped | pending | other)
        ├── duration: f64         (milliseconds)
        ├── suite: Option<String> (module path, class name)
        ├── message: Option<String> (failure message)
        ├── trace: Option<String>   (stack trace)
        ├── tags: Vec<String>
        └── flaky: bool
```

Source: `codeflow-cli/core/src/testing/report/mod.rs`

---

## 3. Quickstart

[^ back to top](#generic-testing-framework)

Go from zero to running tests in under 5 minutes.

### Step 1: Auto-detect your stack

Run from your project root:

```bash
codeflow test setup --auto
```

Expected output (Rust project example):

```
Auto-detected config written to .codeflow/config/testing/test-config.json
```

The auto-detector scans for sentinel files and infers targets:

| Sentinel | Detected target | Runner |
|----------|----------------|--------|
| `Cargo.toml` | `rust-core` | `cargo` |
| `package.json` + vitest dep | `web` | `vitest` |
| `package.json` + jest dep | `web` | `jest` |
| `go.mod` | `go-service` | `go` |
| `pyproject.toml` / `setup.py` with pytest | `python` | `pytest` |

**Auto-detect decision tree** (source: `codeflow-cli/core/src/testing/setup/detect.rs`):

```text
codeflow test setup --auto
        |
        ├── Cargo.toml exists?
        │       YES ──► add target: rust-core (runner: cargo)
        │
        ├── package.json exists?
        │       |
        │       ├── vitest in dependencies?
        │       │       YES ──► add target: web (runner: vitest)
        │       └── jest in dependencies? (checked only if no vitest)
        │               YES ──► add target: web (runner: jest)
        │
        ├── go.mod exists?
        │       YES ──► add target: go-service (runner: go)
        │
        ├── pyproject.toml or setup.py with pytest?
        │       YES ──► add target: python (runner: pytest)
        │
        └── no match ──► no targets written; prompt user to choose a template
```

Detection order: Rust → Node (vitest > jest) → Go → Python. Multiple stacks produce multiple targets.

If auto-detect does not find your stack, use a template instead (Step 1b).

### Step 1b: Apply a named template (alternative)

```bash
# List available templates
codeflow test setup --list

# Apply a specific template
codeflow test setup --template example-rust.json
```

Expected output:

```
Template 'example-rust.json' written to .codeflow/config/testing/test-config.json
```

### Step 2: Review and adjust the config

```bash
codeflow test config show
```

Edit `.codeflow/config/testing/test-config.json` to set your actual test commands and coverage paths. The generated config has best-guess defaults.

### Step 3: Validate the config

```bash
codeflow test doctor
```

Expected output (all passing, Rust example with `cwd` and one coverage rule):

```
  1. [PASS] config-exists: config file exists
  2. [PASS] schema-valid: config validates against schema
  3. [PASS] rust-core.cwd-exists: cwd 'codeflow-cli' exists
  4. [PASS] rust-core.essential.command-parses: command parses: cargo nextest run --profile essential
  5. [PASS] rust-core.full.command-parses: command parses: cargo llvm-cov nextest --profile full ...
  6. [PASS] rust-core.report-path: path is safe: target/nextest/full/junit.xml
  7. [PASS] rust-core.coverage-path: path is safe: lcov.info
  8. [PASS] rust-core.glob-valid: glob compiles: **/*.rs
  9. [PASS] rust-core.probe: runner probe passed: cargo
```

Fix any `FAIL` items before proceeding. `WARN` items are advisory and do not block the run.

### Step 4: Run your first test

```bash
codeflow test --mode full
```

Expected output (Rust example):

```
PASS: rust-core (12.3s)

### 1. Overall Test Pass Status

| Target    | Mode | Passed | Failed | Skipped | Duration |
|-----------|------|--------|--------|---------|----------|
| rust-core | full | 142    | 0      | 0       | 12.3s    |
| **Total** |      | **142**| **0**  | **0**   | **12.3s**|

Runs: 1 consecutive clean.

### 2. Overall Coverage

| Target    | Workspace-equivalent | Per-rule summary |
|-----------|--------------------:|------------------|
| rust-core | 87.2%               | per_file >=85% pass |
...
```

---

## 4. Configuration Reference

[^ back to top](#generic-testing-framework)

Config file location: `.codeflow/config/testing/test-config.json`

Mutations must use `codeflow test config <subcmd>` — direct edits are blocked by ProtectionGuard. See [Section 7](#7-cli-command-reference) for mutation commands.

### 4a. Schema Overview

```text
TestConfig (top-level)
├── _description      String?     optional description (preserved round-trip)
├── $schema           String?     JSON schema path for editor validation
├── schema_version    String      REQUIRED; currently "1.0"
├── execution         ExecutionConfig
├── defaults          DefaultsConfig
└── targets           Vec<TargetConfig>
```

### 4b. Top-level Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `schema_version` | String | — | **Required.** Must be `"1.0"`. |
| `execution.parallel` | bool | `false` | Run all targets concurrently. |
| `execution.fail_fast` | bool | `false` | Stop after first target failure. |
| `defaults.coverage` | `Vec<CoverageRule>` | `[]` | Coverage rules applied to targets that omit their own. |
| `targets` | `Vec<TargetConfig>` | `[]` | List of test targets. |

### 4c. Per-Target Fields (`targets[*]`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | String | — | **Required.** Unique target identifier. |
| `enabled` | bool | `true` | Set to `false` to skip without removing. |
| `cwd` | String? | repo root | Working directory for commands; relative to repo root. |
| `runner` | RunnerType | — | **Required.** One of: `cargo`, `pytest`, `jest`, `vitest`, `go`, `mocha`, `rspec`, `phpunit`, `custom`. |
| `modes` | `Map<String, ModeCommand>` | — | **Required.** At least one mode key (e.g., `essential`, `full`). |
| `report` | ReportConfig? | none | Test report output configuration. |
| `coverage` | CoverageConfig? | none | Coverage artifact and rules configuration. |
| `env` | `Map<String, String>` | `{}` | Environment variables merged with inherited env. |

### 4d. Mode Command

```json
"modes": {
  "essential": { "command": "cargo nextest run" },
  "full":      { "command": "cargo llvm-cov nextest --lcov --output-path lcov.info" }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `command` | String | Shell command to run for this mode. |

Supported modes: `quick`, `essential`, `full` (custom names allowed). `codeflow test --mode full` looks for a key named `full`.

### 4e. Report Configuration (`report`)

| Field | Type | Description |
|-------|------|-------------|
| `format` | `"junit"` or `"ctrf"` | Report format produced by the test command. |
| `path` | String | Path to the report file, relative to `cwd`. |
| `derive_from` | String? | When `format` is `ctrf` but the runner emits JUnit, set to `"junit"` to trigger internal JUnit→CTRF conversion. |

**derive_from example:** cargo-nextest emits JUnit XML. Set `format: "ctrf"` and `derive_from: "junit"`. The engine reads the JUnit file, converts it to CTRF internally, and treats it as CTRF for all downstream consumers.

### 4f. Coverage Configuration (`coverage`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `format` | CoverageFormat | — | **Required.** One of: `lcov`, `cobertura`, `istanbul-summary`, `go-cover`. |
| `path` | String | — | **Required.** Path to coverage artifact, relative to `cwd`. |
| `transform` | String? | none | Optional shell command to transform raw coverage output into `format` before parsing. |
| `rules` | `Vec<CoverageRule>` | `[]` | Ordered coverage rule list. First matching rule wins. |
| `exceptions` | `Vec<CoverageException>` | `[]` | Per-file threshold overrides. |

### 4g. Coverage Rule (`coverage.rules[*]`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `scope` | CoverageScope | — | **Required.** One of: `per_file`, `per_package`, `per_module`, `changed_files`, `global`. |
| `minimum` | u32 | — | **Required.** Minimum threshold 0-100. |
| `include` | `Vec<String>` | `["**/*"]` | Glob patterns for files to include. Empty = all files. |
| `exclude` | `Vec<String>` | `[]` | Glob patterns for files to exclude. |

### 4h. Coverage Exception (`coverage.exceptions[*]`)

| Field | Type | Description |
|-------|------|-------------|
| `file` | String | Repo-relative file path. |
| `threshold` | u32 | Lowered threshold for this file (0-100). |
| `reason` | String | **Required.** Why this file cannot meet the standard threshold. |
| `remove_when` | String | **Required.** Condition under which this exception should be deleted. |

### 4i. Complete Annotated Example

```json
{
  "_description": "My project test configuration",
  "$schema": ".codeflow/schemas/test-config.schema.json",
  "schema_version": "1.0",
  "execution": {
    "parallel": false,
    "fail_fast": false
  },
  "defaults": {
    "coverage": []
  },
  "targets": [
    {
      "name": "rust-core",
      "enabled": true,
      "cwd": "codeflow-cli",
      "runner": "cargo",
      "modes": {
        "essential": {
          "command": "cargo nextest run --profile essential"
        },
        "full": {
          "command": "cargo llvm-cov nextest --profile full --lcov --output-path lcov.info"
        }
      },
      "report": {
        "format": "junit",
        "path": "target/nextest/full/junit.xml",
        "derive_from": "junit"
      },
      "coverage": {
        "format": "lcov",
        "path": "lcov.info",
        "rules": [
          {
            "scope": "per_file",
            "minimum": 85
          }
        ],
        "exceptions": [
          {
            "file": "core/src/autorun/worker.rs",
            "threshold": 79,
            "reason": "Process spawning paths require real tmux/Claude processes.",
            "remove_when": "Mock harness implemented for process spawning."
          }
        ]
      }
    }
  ]
}
```

This is CodeFlow's own configuration at `.codeflow/config/testing/test-config.json`. It is the canonical Rust self-host reference throughout this guide.

---

## 5. Per-Stack Guides

[^ back to top](#generic-testing-framework)

### 5a. Rust

[^ back to top](#generic-testing-framework)

Rust projects use `cargo nextest` for test execution and `cargo-llvm-cov` for coverage. JUnit output is produced via cargo-nextest's JUnit reporter; the engine converts it to CTRF internally. See [cf-rust-standards](../../../.claude/skills/cf-rust-standards/SKILL.md) for Rust development conventions used alongside this testing config.

**CodeFlow's own config** (`.codeflow/config/testing/test-config.json`) is the canonical reference:

```json
{
  "schema_version": "1.0",
  "execution": { "parallel": false, "fail_fast": false },
  "targets": [
    {
      "name": "rust-core",
      "enabled": true,
      "cwd": "codeflow-cli",
      "runner": "cargo",
      "modes": {
        "essential": {
          "command": "cargo nextest run --profile essential"
        },
        "full": {
          "command": "cargo llvm-cov nextest --profile full --lcov --output-path lcov.info"
        }
      },
      "report": {
        "format": "junit",
        "path": "target/nextest/full/junit.xml",
        "derive_from": "junit"
      },
      "coverage": {
        "format": "lcov",
        "path": "lcov.info",
        "rules": [
          { "scope": "per_file", "minimum": 85 }
        ]
      }
    }
  ]
}
```

**Key points:**

- `derive_from: "junit"` tells the engine to read the JUnit XML file at `report.path` and convert it to CTRF internally
- `coverage.path` is relative to `cwd` — `lcov.info` resolves to `codeflow-cli/lcov.info`
- `--profile full` in the llvm-cov command corresponds to a `[profile.full]` section in `.cargo/config.toml`

**Coverage exceptions (real-world example):** CodeFlow uses 9 coverage exceptions for files with structurally untestable code (TUI event loops, process exec() calls). See the full config at `.codeflow/config/testing/test-config.json`. To add an exception:

```bash
codeflow test exceptions add \
  --target rust-core \
  --file core/src/new/file.rs \
  --threshold 70 \
  --reason "TUI event loop requires live terminal" \
  --remove-when "Headless rendering test harness implemented"
```

**Template:** `example-rust.json`

---

### 5b. Node.js

[^ back to top](#generic-testing-framework)

Node.js projects use either vitest (preferred) or jest. Vitest produces native CTRF via `vitest-ctrf-json-reporter`; jest produces JUnit via `jest-junit` and is converted internally.

**Vitest config:**

```json
{
  "schema_version": "1.0",
  "targets": [
    {
      "name": "web",
      "enabled": true,
      "runner": "vitest",
      "modes": {
        "essential": {
          "command": "pnpm test --run"
        },
        "full": {
          "command": "pnpm test --run --coverage --reporter=vitest-ctrf-json-reporter"
        }
      },
      "report": {
        "format": "ctrf",
        "path": "ctrf/ctrf-report.json"
      },
      "coverage": {
        "format": "istanbul-summary",
        "path": "coverage/coverage-summary.json",
        "rules": [
          { "scope": "per_file", "minimum": 85 }
        ]
      }
    }
  ]
}
```

**Jest config:**

```json
{
  "schema_version": "1.0",
  "targets": [
    {
      "name": "web",
      "enabled": true,
      "runner": "jest",
      "modes": {
        "essential": {
          "command": "jest --ci"
        },
        "full": {
          "command": "jest --ci --coverage --reporters=jest-junit"
        }
      },
      "report": {
        "format": "junit",
        "path": "junit.xml",
        "derive_from": "junit"
      },
      "coverage": {
        "format": "istanbul-summary",
        "path": "coverage/coverage-summary.json",
        "rules": [
          { "scope": "per_file", "minimum": 85 }
        ]
      }
    }
  ]
}
```

**Required packages:**

- vitest: `pnpm add -D vitest-ctrf-json-reporter @vitest/coverage-v8`
- jest: `pnpm add -D jest-junit`

**Template:** `example-node.json`

---

### 5c. Python

[^ back to top](#generic-testing-framework)

Python projects use pytest with `--junitxml` for the test report and `coverage.py` for coverage in Cobertura format.

```json
{
  "schema_version": "1.0",
  "targets": [
    {
      "name": "python",
      "enabled": true,
      "runner": "pytest",
      "modes": {
        "essential": {
          "command": "pytest -q"
        },
        "full": {
          "command": "pytest --cov --cov-report=xml --junitxml=report.xml"
        }
      },
      "report": {
        "format": "junit",
        "path": "report.xml",
        "derive_from": "junit"
      },
      "coverage": {
        "format": "cobertura",
        "path": "coverage.xml",
        "rules": [
          { "scope": "per_file", "minimum": 85 }
        ]
      }
    }
  ]
}
```

**Required packages:** `pytest`, `pytest-cov`, `coverage[toml]`

**Template:** `example-python.json`

---

### 5d. Go

[^ back to top](#generic-testing-framework)

Go projects use `go test` with `-coverprofile` for coverage. CTRF output is produced via `go-ctrf-json-reporter` (pipe from `go test -json`). Coverage uses the `go-cover` format.

```json
{
  "schema_version": "1.0",
  "targets": [
    {
      "name": "go-service",
      "enabled": true,
      "runner": "go",
      "modes": {
        "essential": {
          "command": "go test ./..."
        },
        "full": {
          "command": "go test ./... -coverprofile=coverage.out -json | go-ctrf-json-reporter"
        }
      },
      "report": {
        "format": "ctrf",
        "path": "ctrf/ctrf-report.json"
      },
      "coverage": {
        "format": "go-cover",
        "path": "coverage.out",
        "rules": [
          { "scope": "per_file", "minimum": 85 }
        ]
      }
    }
  ]
}
```

**Required tools:** `go install github.com/ctrf-io/go-ctrf-json-reporter/cmd/go-ctrf-json-reporter@latest`

**Template:** `example-go.json`

---

### 5e. Monorepo

[^ back to top](#generic-testing-framework)

Monorepos define multiple named targets with per-target `cwd` paths. Enable `execution.parallel: true` to run them concurrently.

```json
{
  "schema_version": "1.0",
  "execution": {
    "parallel": true,
    "fail_fast": false
  },
  "targets": [
    {
      "name": "api-go",
      "enabled": true,
      "cwd": "services/api",
      "runner": "go",
      "modes": {
        "essential": { "command": "go test ./..." },
        "full":      { "command": "go test ./... -coverprofile=coverage.out" }
      },
      "coverage": {
        "format": "go-cover",
        "path": "coverage.out",
        "rules": [ { "scope": "per_file", "minimum": 85 } ]
      }
    },
    {
      "name": "web-node",
      "enabled": true,
      "cwd": "apps/web",
      "runner": "vitest",
      "modes": {
        "essential": { "command": "pnpm test --run" },
        "full":      { "command": "pnpm test --run --coverage" }
      },
      "coverage": {
        "format": "istanbul-summary",
        "path": "coverage/coverage-summary.json",
        "rules": [ { "scope": "per_file", "minimum": 85 } ]
      }
    },
    {
      "name": "tools-python",
      "enabled": true,
      "cwd": "tools",
      "runner": "pytest",
      "modes": {
        "essential": { "command": "pytest -q" },
        "full":      { "command": "pytest --cov --cov-report=xml" }
      },
      "coverage": {
        "format": "cobertura",
        "path": "coverage.xml",
        "rules": [ { "scope": "per_file", "minimum": 85 } ]
      }
    }
  ]
}
```

**Parallel execution details:**

When `execution.parallel: true`, the runner launches all enabled targets' mode commands concurrently (via threads). Each target runs in its own `cwd`, so file artifacts (coverage.out, ctrf-report.json) do not collide as long as each target has a distinct `cwd`.

**Caveats with parallel execution:**

- Targets sharing the same `cwd` may collide on output files (e.g., two targets both writing `coverage.out` in the repo root)
- `fail_fast` with parallel targets: the first failure signals remaining targets to stop, but in-flight commands may still complete
- stdout/stderr from parallel targets is interleaved; use `--format json` for machine-readable output

**Template:** `monorepo-multi-target.json`

---

### 5f. Custom Runner

[^ back to top](#generic-testing-framework)

The `runner: custom` type is the escape hatch for any test framework not listed above. Commands are arbitrary shell strings. CodeFlow's own `shell-scripts` target uses this approach.

**CodeFlow self-host reference** (from `.codeflow/config/testing/test-config.json`):

```json
{
  "name": "shell-scripts",
  "enabled": true,
  "runner": "custom",
  "modes": {
    "essential": {
      "command": "bash -c 'for f in .codeflow/testing/**/*-critical.sh; do bash \"$f\" || exit 1; done'"
    },
    "full": {
      "command": "bash -c 'for f in .codeflow/testing/**/*.sh; do bash \"$f\" || exit 1; done'"
    }
  },
  "report": {
    "format": "ctrf",
    "path": "ctrf.json"
  },
  "structural": {
    "source_glob": [".codeflow/hooks/codeflow/**/*.sh"],
    "test_glob": [".codeflow/testing/**/test-*.sh"],
    "pattern_map": [
      {
        "source_pattern": "hooks/codeflow/{category}/{name}.sh",
        "test_pattern": "testing/{category}/test-{name}.sh"
      }
    ],
    "exclusions": {
      "source_patterns": ["**/lib/**"]
    }
  },
  "test_files": [
    { "path": ".codeflow/testing/pre-tool-use/test-gate-check.sh", "tags": ["critical"] },
    { "path": ".codeflow/testing/post-tool-use/test-sentinel-write.sh", "tags": ["critical"] },
    { "path": ".codeflow/testing/pre-tool-use/test-gh-pr-guard.sh", "tags": ["high"] }
  ]
}
```

**Key points:**

- Shell commands are run via `sh -c "..."` in the target's `cwd`
- The command is responsible for producing the report file at `report.path`
- The `structural` block enforces bidirectional source↔test mapping (see [§13](#13-structural-check))
- The `test_files` array registers individual test files with priority tags (see [§14](#14-test-tags))
- No coverage is configured for this target — coverage is optional for `runner: custom`
- `runner: custom` has no extension-affinity filter for modified-file routing (all file extensions pass through)

**Template:** `hooks-escape-hatch.json`

---

## 6. Coverage Model

[^ back to top](#generic-testing-framework)

Source: `codeflow-cli/core/src/testing/threshold/mod.rs`

### 6a. Scope Types

The coverage model supports five scope types. Choose the scope that matches how tightly you want to enforce coverage:

| Scope | Evaluation | When to use |
|-------|-----------|-------------|
| `per_file` | Each file individually against threshold | Default; catches files with no tests |
| `per_package` | Aggregated over files matching `include` globs | Package-level average; allows low-coverage files if package average passes |
| `per_module` | Same as `per_package` but intended for language module semantics | Logical modules that don't map to filesystem packages |
| `changed_files` | Only files that appear in the git diff for the current PR | Incremental enforcement; legacy code exempt until touched |
| `global` | Single aggregate across all matching files | Lowest bar; only catches catastrophic regressions |

### 6b. First-Match-Wins Semantics

Rules are evaluated in declaration order. For each file, the engine walks the rules list and applies the **first rule whose scope and globs match**. Subsequent rules are skipped for that file.

This applies to `per_file` and `changed_files` scopes. Aggregate scopes (`per_package`, `per_module`, `global`) are **all evaluated independently** — they do not participate in first-match-wins.

**Worked example:**

```json
"rules": [
  { "scope": "per_file", "include": ["legacy/**"], "minimum": 70 },
  { "scope": "per_file", "minimum": 85 }
]
```

Given files `legacy/old.rs` (75% coverage) and `src/new.rs` (75% coverage):

- `legacy/old.rs` → matches rule 1 (include=`legacy/**`) → threshold 70% → **PASS** (75 >= 70)
- `src/new.rs` → does not match rule 1 → falls through to rule 2 → threshold 85% → **FAIL** (75 < 85)

### 6c. Coverage Evaluation Flow

```text
For each file in coverage data:
    |
    v
Is there a CoverageException for this file?
    YES → use exception.threshold instead of rule.minimum for all matching rules
    |
    v
Walk rules list in order:
    Rule 1: scope=per_file, include=["legacy/**"]
        Does file match globs? NO → skip
    Rule 2: scope=per_file, include=[]  (empty = all)
        Does file match globs? YES
            coverage_percent >= minimum?  → PASS or FAIL
            STOP (first-match-wins)
    |
    v
No rule matched this file?
    → file not evaluated (no threshold applied)
    (Note: files with 0% coverage not present in lcov are silently skipped)
```

### 6d. Exception Workflow

Exceptions allow individual files to have a lower threshold than the general rule, with a mandatory justification and removal condition.

**Add an exception:**

```bash
codeflow test exceptions add \
  --target rust-core \
  --file core/src/autorun/worker.rs \
  --threshold 79 \
  --reason "Process spawning paths require real tmux/Claude processes" \
  --remove-when "Mock harness implemented"
```

**List exceptions:**

```bash
codeflow test exceptions list
codeflow test exceptions list --target rust-core
```

**Remove an exception:**

```bash
codeflow test exceptions remove --target rust-core --file core/src/autorun/worker.rs
```

### 6e. Supported Coverage Formats

| Format | Key | Produced by |
|--------|-----|-------------|
| LCOV | `lcov` | `cargo llvm-cov --lcov`, `genhtml`, `gcov` |
| Cobertura XML | `cobertura` | `pytest --cov --cov-report=xml`, `nyc --reporter=cobertura` |
| Istanbul Summary JSON | `istanbul-summary` | `nyc --reporter=json-summary`, `c8 --reporter=json-summary`, `vitest --coverage` |
| Go cover profile | `go-cover` | `go test -coverprofile=coverage.out` |

Source: `codeflow-cli/core/src/testing/coverage/` (lcov.rs, cobertura.rs, istanbul.rs, go_cover.rs)

### 6f. Known Behavior: Files with 0% Coverage

Files with 0% line coverage that are not present in the coverage artifact (e.g., a source file never imported by any test) are **silently skipped** by the threshold engine. The engine only evaluates files that appear in the parsed coverage data. This is a known limitation: if a file has zero tests, it produces no coverage data, so no threshold violation fires.

To catch completely uncovered files, configure a `structural` block on the target (see [§13](#13-structural-check)) to enforce bidirectional source↔test mapping. This does not measure line coverage but guarantees every source file has at least one test file.

---

## 7. CLI Command Reference

[^ back to top](#generic-testing-framework)

All subcommands verified against `codeflow-cli/cli/src/cmd/test.rs`.

### 7a. `codeflow test` (run tests)

**Synopsis:**

```
codeflow test [OPTIONS]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--mode <MODE>` | Test mode: `quick`, `essential`, `full`. Default: `essential`. |
| `--format <FORMAT>` | Output format: `human` (default) or `json`. |
| `--only <TARGETS>` | Run only the named targets, comma-separated. |
| `--skip <TARGETS>` | Skip the named targets, comma-separated. |
| `--only-tag <TAGS>` | Run only test files tagged with at least one of these tags, comma-separated (see [§14](#14-test-tags)). |
| `--skip-tag <TAGS>` | Exclude test files tagged with at least one of these tags, comma-separated. |
| `--fail-fast` | Stop on first target failure. |

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | All tests passed, all coverage thresholds met |
| `1` | One or more test targets failed OR coverage threshold violations |

**Note:** `--coverage` and `--report` are accepted for backwards compatibility but have no effect. Coverage is controlled by `test-config.json` rules; reports are always written to `.state/test-reports/`.

**Examples:**

```bash
# Run essential tests (default mode)
codeflow test

# Run full suite
codeflow test --mode full

# Run only the rust-core target
codeflow test --mode full --only rust-core

# Skip shell-scripts target
codeflow test --mode full --skip shell-scripts

# JSON output for machine consumption
codeflow test --mode full --format json
```

---

### 7b. `codeflow test setup`

**Synopsis:**

```
codeflow test setup [OPTIONS]
```

**Options:**

| Option | Description |
|--------|-------------|
| `--auto` | Non-interactive auto-detection of stacks. |
| `--template <NAME>` | Apply a named template from `.codeflow/templates/test-config/`. |
| `--list` | List available templates. |
| `--add-target` | Add a new target to existing config (interactive wizard). |
| `--force` | Overwrite existing config when using `--template`. |

**Examples:**

```bash
codeflow test setup --auto
codeflow test setup --template example-rust.json
codeflow test setup --list
codeflow test setup --add-target
codeflow test setup --template monorepo-multi-target.json --force
```

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | Config written successfully |
| `1` | Template not found |
| `2` | Config already exists (use `--force` to overwrite) |

---

### 7c. `codeflow test config`

**Synopsis:**

```
codeflow test config <SUBCOMMAND>
```

**Subcommands:**

| Subcommand | Description |
|-----------|-------------|
| `show` | Print current config as JSON. |
| `add-target --name <N> --runner <R> [--cwd <D>] [--mode <MODE=CMD>...]` | Add a new target. |
| `remove-target --name <N>` | Remove a target by name. |
| `enable --target <N>` | Enable a disabled target. |
| `disable --target <N>` | Disable a target without removing it. |
| `set-command --target <N> --mode <M> --command <CMD>` | Set a mode command. |
| `set-report --target <N> --format <F> --path <P> [--derive-from <D>]` | Set report config. |
| `set-coverage --target <N> --format <F> --path <P>` | Set coverage format and path. |
| `set-threshold --target <N> --scope <S> --minimum <M> [--rule-index <I>]` | Set coverage threshold. |

**Examples:**

```bash
# Show config
codeflow test config show

# Add a new target
codeflow test config add-target \
  --name api \
  --runner go \
  --cwd services/api \
  --mode essential="go test ./..." \
  --mode full="go test ./... -coverprofile=coverage.out"

# Update a mode command
codeflow test config set-command \
  --target rust-core \
  --mode full \
  --command "cargo llvm-cov nextest --profile full --lcov --output-path lcov.info"

# Set report configuration
codeflow test config set-report \
  --target rust-core \
  --format junit \
  --path target/nextest/full/junit.xml \
  --derive-from junit

# Set coverage threshold
codeflow test config set-threshold \
  --target rust-core \
  --scope per_file \
  --minimum 85
```

---

### 7d. `codeflow test exceptions`

**Synopsis:**

```
codeflow test exceptions <SUBCOMMAND>
```

**Subcommands:**

| Subcommand | Description |
|-----------|-------------|
| `add --target <N> --file <F> --threshold <T> --reason <R> --remove-when <W>` | Add an exception. |
| `remove --target <N> --file <F>` | Remove an exception. |
| `list [--target <N>]` | List exceptions, optionally filtered by target. |

---

### 7e. `codeflow test report`

**Synopsis:**

```
codeflow test report <SUBCOMMAND>
```

**Subcommands:**

| Subcommand | Description |
|-----------|-------------|
| `show [--run-id <ID>] [--format <F>]` | Show the last (or specified) test run report. |
| `convert --from <F> --to <F> --input <P> --output <P>` | Convert between report formats. |
| `diff --from <ID> --to <ID>` | Compare two test runs by run ID. |

**Examples:**

```bash
# Show last test run in human format
codeflow test report show

# Show as JSON (used by cf-git-operations for PR body extraction)
codeflow test report show --format json

# Convert JUnit to CTRF
codeflow test report convert \
  --from junit \
  --to ctrf \
  --input junit.xml \
  --output ctrf-report.json

# Compare two runs
codeflow test report diff --from run-1713000000000 --to run-1713001000000
```

---

### 7f. `codeflow test doctor`

**Synopsis:**

```
codeflow test doctor [--format <FORMAT>]
```

Runs health checks against your `test-config.json`. There are two global checks plus a set of per-target checks (only run for enabled targets).

**Global checks (always run):**

| Check name | Status on fail | Description |
|-----------|---------------|-------------|
| `config-exists` | FAIL (early exit) | Config file found at `.codeflow/config/testing/test-config.json` |
| `schema-valid` | FAIL (early exit) | Config parses and validates against schema |

**Per-target checks (one set per enabled target, named `{target}.{check}`):**

| Check name | Status on fail | Description |
|-----------|---------------|-------------|
| `{target}.cwd-exists` | FAIL | `cwd` directory exists (only checked if `cwd` is set) |
| `{target}.{mode}.command-parses` | FAIL | Mode command has balanced quotes (one check per mode) |
| `{target}.report-path` | FAIL | `report.path` is a safe relative path (no absolute, no `..`) |
| `{target}.coverage-path` | FAIL | `coverage.path` is a safe relative path |
| `{target}.coverage-transform-parses` | FAIL | `coverage.transform` command has balanced quotes (only if transform set) |
| `{target}.glob-valid` | FAIL | Each `include`/`exclude` glob in coverage rules compiles (one check per glob) |
| `{target}.exception-file-exists` | **WARN** | Exception file exists on disk (warning only — file may not exist yet) |
| `{target}.probe` | **WARN** | Runner version check (`cargo --version`, `pytest --version`, etc.) passes within 5s; custom runner always PASS with "no probe" |

Note: only FAIL status blocks the exit (exit code 1). WARN items are advisory and do not block the run. Custom runners skip the probe check.

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | All checks pass or warn |
| `1` | One or more checks fail |

---

### 7g. `codeflow test clean`

**Synopsis:**

```
codeflow test clean [--artifacts] [--yes]
```

Removes test artifacts from `.state/test-reports/` and `.state/coverage/`.

**Options:**

| Option | Description |
|--------|-------------|
| `--artifacts` | Remove test artifacts (default: true). |
| `--yes` | Skip confirmation prompt. |

---

### 7h. `codeflow test structural-check`

**Synopsis:**

```
codeflow test structural-check [--format human|json] [--only <TARGET>]
```

Runs the structural integrity check for all targets that have a `structural` block configured. Returns immediately with no output for targets without a `structural` block.

**Options:**

| Option | Default | Description |
|--------|---------|-------------|
| `--format human\|json` | `human` | Output format. `human` renders a readable table; `json` emits one `StructuralResult` object per target. |
| `--only <TARGET>` | (all targets) | Restrict to a single named target. |

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | All structural checks passed (or no targets have a `structural` block). |
| `1` | One or more targets failed: missing test files or orphaned test files detected. |

**Examples:**

```bash
# Check all targets
codeflow test structural-check

# Check only the shell-scripts target
codeflow test structural-check --only shell-scripts

# JSON output for CI artifact upload
codeflow test structural-check --format json
```

See [§13](#13-structural-check) for the full structural check reference including config schema, output format, and integration with `codeflow test --mode full`.

---

## 8. PR Body Format

[^ back to top](#generic-testing-framework)

Source: `codeflow-cli/core/src/testing/pr_body.rs`

The engine renders a five-section markdown block that cf-git-operations inserts under `## Test Results` in the PR description.

### 8a. Five-Section Structure

**Section 1 — Overall Test Pass Status**

Per-target pass/fail counts, total row, and consecutive clean run count.

```markdown
### 1. Overall Test Pass Status

| Target    | Mode | Passed | Failed | Skipped | Duration |
|-----------|------|--------|--------|---------|----------|
| rust-core | full | 142    | 0      | 0       | 12.3s    |
| **Total** |      | **142**| **0**  | **0**   | **12.3s**|

Runs: 2 consecutive clean.
```

**Section 2 — Overall Coverage**

Per-target workspace-equivalent coverage percentage and per-rule summary. If exceptions exist, an Exempted Files table is appended.

```markdown
### 2. Overall Coverage

| Target    | Workspace-equivalent | Per-rule summary         |
|-----------|--------------------:|--------------------------|
| rust-core | 87.2%               | per_file >=85% pass      |

#### Exempted Files (per-target)

| Target    | File                          | Coverage | Configured Threshold | Reason |
|-----------|-------------------------------|---------:|---------------------:|--------|
| rust-core | core/src/autorun/worker.rs    | 79%      | 79%                  | Process spawning... |
```

**Section 3 — Modified File Coverage**

Per-PR-changed-file coverage against the 85% threshold (or configured threshold). cf-git-operations determines changed files via `git diff`.

```markdown
### 3. Modified File Coverage

| Target    | File                    | Coverage | Threshold | Status |
|-----------|-------------------------|---------:|----------:|--------|
| rust-core | core/src/new_feature.rs | 92%      | 85%       | PASS   |
```

**Section 4 — Test Failures** (omitted when no failures)

Individual failing test names, suite, and failure message.

```markdown
### 4. Test Failures

| Target | Suite         | Test             | Message             |
|--------|---------------|------------------|---------------------|
| web    | AuthService   | test_login_fails | assertion failed... |
```

**Section 5 — Slowest Tests** (emitted when test data is available)

Top 10 slowest tests by duration.

```markdown
### 5. Slowest Tests (optional)

| Target    | Test                     | Duration |
|-----------|--------------------------|--------:|
| rust-core | test_full_session_start  | 850ms    |
```

### 8b. Report Format Interop

The framework supports three input paths to generate the canonical CTRF model:

| Path | When to use | Config |
|------|-------------|--------|
| **JUnit → CTRF conversion** | Runner emits JUnit XML (cargo-nextest, pytest, jest-junit) | `report.format: "junit"` and `report.derive_from: "junit"` |
| **Native CTRF** | Runner emits CTRF JSON directly (vitest-ctrf-json-reporter, go-ctrf-json-reporter) | `report.format: "ctrf"` (no `derive_from`) |
| **Format conversion via CLI** | Transform an existing JUnit file to CTRF outside a test run | `codeflow test report convert --from junit --to ctrf --input junit.xml --output ctrf.json` |

When `derive_from: "junit"` is set and `format: "junit"`, the engine reads the JUnit file at `report.path`, converts it via `report/junit_to_ctrf.rs`, and all downstream consumers (PR body, ledger) see a CTRF model.

### 8c. No Test Targets Configured

When `targets` is empty (or all targets disabled), the engine outputs a single informational line instead of the five sections:

```markdown
No test targets configured. Run `codeflow test setup` to add targets.
```

This is not an error. PRs with no configured targets still pass the PR-body gate.

### 8d. Section Omission Rules

| Section | Omitted when |
|---------|-------------|
| Section 4 (Failures) | No test failures in any target |
| Section 5 (Slowest) | No test data available |
| Exempted Files table | No targets have exceptions |

---

## 9. WS-QA Integration

[^ back to top](#generic-testing-framework)

See also: [CLAUDE.md §7.4](../../../.claude/CLAUDE.md) · [cf-quality-assurance definition](../../../.claude/agents/cf-quality-assurance.md)

### 9a. Pipeline Position

```text
WS-DEV ──► WS-REV ──► WS-QA ──► PF5-VERIFY
                          │
                cf-quality-assurance
                invokes:
                  codeflow test --mode full
```

WS-QA is the quality gate in the FEAT, FIX, RFCT, CICD, HTFX, and CHOR pipelines. cf-quality-assurance is the only teammate that runs tests — cf-development writes unit tests during WS-DEV but does not run the full suite.

### 9b. How cf-quality-assurance Invokes Tests

cf-quality-assurance runs:

```bash
codeflow test --mode full
```

This is the single authoritative invocation. It:

1. Loads `test-config.json`
2. Runs all enabled targets in `full` mode
3. Parses reports (JUnit → CTRF or native CTRF)
4. Evaluates coverage thresholds
5. Renders PR body sections 1-5 to stdout
6. Writes test report to `.state/test-reports/`
7. Emits `test_result_recorded` ledger event
8. Exits 0 (pass) or 1 (failure or threshold violation)

### 9c. QA Report Structure

The QA Report in the task markdown has three mandatory sections. A report missing any section is a rework trigger.

**Section 1 — Overall Test Pass Status**

Copied verbatim from the `codeflow test --mode full` stdout (Section 1 of PR body). Must show zero failures. Must include at least 2 consecutive clean runs.

**Section 2 — Overall Coverage**

Copied verbatim from stdout (Section 2 of PR body). Shows per-target workspace-equivalent coverage. Any file below 85% that is not in the configured exceptions list is a rework trigger. The Exempted Files table must include ALL entries from `test-config.json` exceptions, not just files modified in this session.

**Section 3 — Modified File Coverage**

Copied verbatim from stdout (Section 3 of PR body). Per-file coverage for every file modified in the PR, at or above 85% (or configured threshold). Any modified file below threshold is a rework trigger.

### 9d. Test Stats Extraction for PR Body

cf-git-operations extracts test stats for the PR body using:

```bash
codeflow test report show --format json
```

This reads the most recent test report from `.state/test-reports/` and emits a JSON summary. cf-git-operations inserts the five sections under `## Test Results` in the PR description.

### 9e. Modified File Coverage Enforcement

Modified file coverage uses the same `per_file` rule logic as the overall threshold. The engine routes each changed file to the target whose `cwd` is the longest matching path prefix (six-rule algorithm from `pr_body.rs::route_changed_file`):

1. Normalize target cwd (empty/`.`/missing → root)
2. Extension-affinity filter (`cargo` = `*.rs` only, `go` = `*.go` only, others = all)
3. Longest-prefix match
4. Tie-break by declaration order (first wins)
5. Empty-cwd target is a catch-all
6. No match → "Unattributed"

---

## 10. Template Library

[^ back to top](#generic-testing-framework)

Templates are located at `.codeflow/templates/test-config/`. Apply with:

```bash
codeflow test setup --template <filename>
```

All 9 templates verified against actual JSON files in `.codeflow/templates/test-config/`.

### 10a. Template Descriptions

| Template | Target Stack | What It Configures | When to Use |
|----------|-------------|-------------------|-------------|
| `minimal.json` | Any | Empty targets list, schema scaffold only | Fresh project, add targets manually |
| `single-target-basic.json` | Any (custom) | Single target, essential + full modes, no coverage | Simplest possible working config |
| `single-target-with-coverage.json` | Any (custom) | Single target with lcov coverage rules and a sample exception | Starting point for coverage enforcement |
| `hooks-escape-hatch.json` | Shell scripts | Custom runner with bash scripts | Projects using shell-based test runners |
| `example-rust.json` | Rust | cargo nextest, JUnit via derive_from, lcov coverage | Rust projects with cargo-nextest |
| `example-node.json` | Node.js (vitest) | vitest, native CTRF, istanbul-summary coverage | Node.js projects with vitest |
| `example-python.json` | Python | pytest, JUnit XML, cobertura coverage | Python projects with pytest |
| `example-go.json` | Go | go test, CTRF via go-ctrf-json-reporter, go-cover | Go projects |
| `monorepo-multi-target.json` | Monorepo (Go + Node + Python) | Three targets, parallel execution, per-target cwd | Multi-stack monorepos |

### 10b. Comparison Table

| Template | Runner(s) | Report Format | Coverage Format | Parallel |
|----------|----------|--------------|----------------|---------|
| `minimal.json` | — | — | — | false |
| `single-target-basic.json` | custom | — | — | false |
| `single-target-with-coverage.json` | custom | — | lcov | false |
| `hooks-escape-hatch.json` | custom | — | — | false |
| `example-rust.json` | cargo | junit (derive_from) | lcov | false |
| `example-node.json` | vitest | ctrf | istanbul-summary | false |
| `example-python.json` | pytest | junit (derive_from) | cobertura | false |
| `example-go.json` | go | ctrf | go-cover | false |
| `monorepo-multi-target.json` | go + vitest + pytest | — | go-cover + istanbul-summary + cobertura | **true** |

### 10c. How to Apply

```bash
# List all templates with descriptions
codeflow test setup --list

# Apply a template (fails if config exists)
codeflow test setup --template example-rust.json

# Overwrite existing config
codeflow test setup --template example-rust.json --force
```

---

## 11. Troubleshooting

[^ back to top](#generic-testing-framework)

Source: `codeflow-cli/core/src/testing/doctor/mod.rs` and `codeflow-cli/core/src/testing/error.rs`

### Issue 1: Config not found

**Symptom:**

```
Error: test-config.json not found at .codeflow/config/testing/test-config.json
```

**Cause:** No config file exists.

**Fix:**

```bash
codeflow test setup --auto
# or apply a template
codeflow test setup --template minimal.json
```

---

### Issue 2: Schema validation error

**Symptom:**

```
Error: schema validation failed: targets[0].unknown_stuff: unknown field
```

**Cause:** A field in `test-config.json` that is not in the schema. Unknown target-level fields are rejected; unknown top-level fields emit a warning but are accepted.

**Fix:** Remove the unknown field. Use `codeflow test config show` to inspect the current parsed config.

---

### Issue 3: Coverage below threshold

**Symptom:**

```
FAIL: coverage thresholds not met:
  core/src/new_module.rs (72.0% < 85%)
```

**Cause:** A file's coverage is below the threshold in the matching rule.

**Fix options:**

1. Add tests for the uncovered lines (preferred)
2. Add a coverage exception with a mandatory reason and remove_when condition:

```bash
codeflow test exceptions add \
  --target rust-core \
  --file core/src/new_module.rs \
  --threshold 72 \
  --reason "Structurally untestable paths: ..." \
  --remove-when "Refactored to extract testable pure functions"
```

---

### Issue 4: Missing coverage file

**Symptom:**

```
warning: coverage artifact not produced at lcov.info
```

**Cause:** The test command ran but did not produce the coverage file at `coverage.path`. Common causes: wrong path configured, coverage instrumentation not enabled in the test command, or coverage command failed silently.

**Fix:**

1. Verify the `full` mode command includes coverage instrumentation (e.g., `--lcov --output-path lcov.info` for llvm-cov)
2. Verify `coverage.path` matches where the runner writes the file (relative to `cwd`)
3. Run the test command manually and confirm the file is produced

---

### Issue 5: JUnit parse failure

**Symptom:**

```
Error: report parse error for rust-core: invalid JUnit XML
```

**Cause:** The file at `report.path` is not valid JUnit XML, or the file does not exist.

**Fix:**

1. Verify the test command produces JUnit output — run it manually and inspect the output file
2. Confirm `report.path` is correct relative to `cwd`
3. If the runner uses a non-standard JUnit dialect, try native CTRF output instead

---

### Issue 6: CTRF parse failure

**Symptom:**

```
Error: report parse error for web: invalid CTRF JSON at ctrf/ctrf-report.json
```

**Cause:** The CTRF file is malformed or the reporter version changed the schema.

**Fix:**

1. Run the test command manually and inspect the CTRF file content
2. Verify the reporter package version matches what your CTRF config expects
3. Use `codeflow test report convert --from junit --to ctrf ...` if you can produce JUnit instead

---

### Issue 7: Target command fails

**Symptom:**

```
FAIL: target rust-core exited with code 1
```

**Cause:** The test command itself failed (test failures or non-zero exit from runner).

**Fix:**

1. Run the mode command manually from the target's `cwd` to see full output
2. Check if the failure is real test failures or a configuration error
3. For "command not found" errors, verify the runner binary is installed and in PATH

---

### Issue 8: Parallel execution conflicts

**Symptom:** Two targets writing to the same file path, one overwriting the other.

**Cause:** Two targets have the same `cwd` (or both use the repo root) and produce artifacts with the same filename (e.g., both write `coverage.out`).

**Fix:** Ensure each parallel target has a distinct `cwd`, or configure each target to write artifacts to distinct paths:

```json
{ "name": "api", "cwd": "services/api", "coverage": { "path": "coverage.out" } },
{ "name": "web", "cwd": "apps/web",     "coverage": { "path": "coverage.out" } }
```

Each `coverage.out` is relative to its own `cwd`, so they do not conflict.

---

### Issue 9: Stale reports from previous run

**Symptom:** `codeflow test report show` displays results from a previous run.

**Cause:** The most recent run did not produce a new report (e.g., it exited before writing the report, or the target was skipped).

**Fix:**

```bash
# Clean stale artifacts
codeflow test clean --yes

# Re-run to produce fresh report
codeflow test --mode full
```

---

### Issue 10: Doctor check failures

**Symptom:**

```
FAIL  cwd-exists   target 'api': cwd 'services/api' does not exist
```

**Cause:** The `cwd` path configured for a target does not exist in the filesystem.

**Fix:** Verify the directory exists and the path is correct relative to the repo root:

```bash
ls services/api
codeflow test config set-command --target api --mode full --command "..."
# or update cwd via config edit
```

---

### Issue 11: Mode not configured for target

**Symptom:**

```
skipping target rust-core: mode full not configured
```

**Cause:** The target does not have a `full` key in its `modes` map.

**Fix:**

```bash
codeflow test config set-command \
  --target rust-core \
  --mode full \
  --command "cargo llvm-cov nextest --profile full --lcov --output-path lcov.info"
```

---

### Issue 12: Schema version unsupported

**Symptom:**

```
Error: unsupported schema_version "2.0"; supported: 1.0
```

**Cause:** The config was written with a future schema version that this binary does not support.

**Fix:** Downgrade `schema_version` to `"1.0"` or upgrade the `codeflow` binary to a version that supports the schema.

---

## 12. CI Integration

[^ back to top](#generic-testing-framework)

### 12a. CodeFlow's CI Architecture

CodeFlow uses `codeflow test --mode full` as the single test execution command both locally and in CI. The shell harness (`run-all-tests.sh`) was retired in INF-TSK-046-008; structural integrity is now enforced inside `codeflow test --mode full` via the `structural` block on the `shell-scripts` target.

**Why Rust tests don't run in CI:** Rust tests require `cargo llvm-cov` with LLVM instrumentation overhead. The full Rust test suite with coverage takes 15–20 minutes on a cold CI build (2 minutes with cache). CI builds the `codeflow` binary with `--profile ci` (optimized for build speed, not performance) to verify compilation, but runs only the shell and Python targets via `codeflow test --mode full`. Rust tests run locally before every PR.

### 12b. CodeFlow's Own CI Setup

Source: `.github/workflows/test-suite.yml`

The `test` job runs on every PR and push to `main`:

```text
jobs/test steps:
  1. checkout
  2. Set up Rust toolchain (stable + llvm-tools-preview)
  3. Install cargo-nextest
  4. Install cargo-llvm-cov
  5. Install mold linker (Linux)
  6. Cache Rust build (Swatinem/rust-cache, key: "ci", workspace: codeflow-cli)
  7. Install codeflow binary (cargo build --profile ci → $HOME/.cargo/bin/codeflow)
  8. Run test suite (codeflow test --mode full --skip rust-core)
```

**Step 7** builds the `codeflow` binary with `--profile ci`. This verifies Rust compilation and produces the binary needed for step 8.

**Step 8** runs `codeflow test --mode full --skip rust-core`. The `shell-scripts` and `python` targets run. The `rust-core` target is skipped in CI because it requires the full LLVM coverage instrumentation overhead. Structural integrity for the shell-scripts target runs inside this invocation via the `structural` block — no separate CI step is needed.

**Cache behavior:**

- `Swatinem/rust-cache@v2` caches the `codeflow-cli/target` directory
- Cache hit: build step takes approximately 2 minutes
- Cache miss (new dependencies or Rust toolchain change): build step takes approximately 16 minutes
- Cache key includes the `ci` shared-key and Rust toolchain hash

### 12c. SKIP_RUST_BUILD Variable

The `build-release` job has a condition:

```yaml
if: vars.SKIP_RUST_BUILD != 'true'
```

Setting the GitHub repository variable `SKIP_RUST_BUILD=true` disables cross-platform release builds (macOS aarch64, macOS x86_64, Linux x86_64). This is useful during rapid iteration when binaries are not needed for every PR.

### 12d. Wiring `codeflow test` into Your Own CI

If your project uses the Generic Testing Framework and you want CI to call `codeflow test` directly:

**Step 1: Install the binary in CI**

```yaml
- name: Install codeflow
  run: |
    # Option A: build from source (if you have a Rust workspace)
    cargo build --release
    cp target/release/codeflow "$HOME/.cargo/bin/"

    # Option B: download a pre-built release artifact
    curl -L https://releases.example.com/codeflow/latest/codeflow-linux -o codeflow
    chmod +x codeflow && mv codeflow "$HOME/.cargo/bin/"
```

**Step 2: Run tests**

```yaml
- name: Run test suite
  run: codeflow test --mode full
```

**Step 3: Handle exit codes**

```yaml
- name: Tests passed
  if: success()
  run: echo "All tests passed"

- name: Tests failed
  if: failure()
  run: |
    echo "Test suite failed. To reproduce locally:"
    echo "  codeflow test --mode full"
    exit 1
```

**Step 4: Publish test report (optional)**

If your CI supports CTRF report uploads:

```yaml
- name: Publish test results
  if: always()
  run: codeflow test report show --format json > ctrf-report.json
  # then upload ctrf-report.json to your CI reporting tool
```

### 12e. Diagram: CI vs Local Test Path

```text
Developer (local)                    GitHub Actions (CI)
──────────────────────               ─────────────────────────────────
git commit && git push               on: pull_request / push/main
       │                                         │
       │                                         v
       │                             1. checkout + setup Rust toolchain
       │                             2. Install cargo-nextest, llvm-cov
       │                             3. Restore Rust build cache (~2m hit)
       │                             4. Build codeflow binary (CI profile)
       │                             5. codeflow test --mode full --skip rust-core
       │                                (shell-scripts + python targets only)
       │                                (structural check runs inside codeflow test)
       │                                         │
       v                                         v
codeflow test --mode full             CI passes / fails
 ├─ rust-core target (Rust)          (build + shell/python tests only)
 │   cargo llvm-cov nextest           structural integrity via structural block
 │   Parse JUnit → CTRF
 │   Evaluate per_file coverage
 │   Render PR body sections
 └─ shell-scripts target (custom)
     runs test_files[] entries
     structural check: source↔test map
     Parse CTRF output
     No coverage configured
          │
          v
  PR body Test Results section
  Ledger event recorded
  Coverage threshold enforced
```

---

## 13. Structural Check

[^ back to top](#generic-testing-framework)

The structural check enforces a bidirectional mapping between source files and test files within a target. It closes the "silent no-test hole": a source file added without a test file is caught at run time, before CI, without requiring developers to remember a manual checklist.

For the full technical reference, see [Design doc §24](../analysis/generic-testing-subsystem.md).

### 13a. When to use it

Use the `structural` block when:

- Your target has source files that must each have a corresponding test file (one-to-one or one-to-many)
- You want to catch "orphaned" test files (test files whose source has been deleted)
- You want to replace a separate structural integrity CI step with built-in enforcement

Do not use it for targets where there is no source↔test mapping requirement (e.g., a target that just runs linters).

### 13b. Config schema

The `structural` block is optional and per-target:

```json
{
  "name": "shell-scripts",
  "runner": "custom",
  "structural": {
    "source_glob": [".codeflow/hooks/codeflow/**/*.sh"],
    "test_glob": [".codeflow/testing/**/test-*.sh"],
    "pattern_map": [
      {
        "source_pattern": "hooks/codeflow/{category}/{name}.sh",
        "test_pattern": "testing/{category}/test-{name}.sh"
      }
    ],
    "exclusions": {
      "source_patterns": ["**/lib/**"],
      "test_patterns": ["**/fixtures/**"]
    }
  }
}
```

| Field | Required | Description |
|-------|----------|-------------|
| `source_glob` | Yes | Globs identifying source files (relative to project root) |
| `test_glob` | Yes | Globs identifying test files (relative to project root) |
| `pattern_map` | Yes | Rules mapping source path patterns to expected test path patterns |
| `exclusions.source_patterns` | No | Source paths matching these globs are excluded |
| `exclusions.test_patterns` | No | Test paths matching these globs are excluded |

### 13c. Running structural checks

**As part of `codeflow test --mode full`** (default — no extra step needed):

```bash
codeflow test --mode full
```

If any target's structural check fails, the run exits non-zero and reports the failing target. The target's runner is not invoked for that target.

**Standalone check** (faster, skips test execution):

```bash
codeflow test structural-check

# Single target only
codeflow test structural-check --only shell-scripts

# JSON output
codeflow test structural-check --format json
```

### 13d. Interpreting output

Human output when checks pass:

```text
Target: shell-scripts
  Sources scanned: 24   Tests scanned: 24   Sources excluded: 2   Tests excluded: 0
  Result: PASS
```

Human output when checks fail:

```text
Target: shell-scripts
  Sources scanned: 24   Tests scanned: 22   Sources excluded: 2   Tests excluded: 0

  MISSING (source has no test):
    hooks/codeflow/post-tool-use/new-hook.sh
      → expected: testing/post-tool-use/test-new-hook.sh

  ORPHAN (test has no matching source):
    testing/pre-tool-use/test-old-hook.sh
      → no corresponding source file found

  Result: FAIL (1 missing, 1 orphan)
```

**MISSING** means a source file exists but no test file was found at the expected mapped path. Create the test file.

**ORPHAN** means a test file exists but no source file matches it. Either the source was deleted (remove the test) or the naming convention drifted (rename the test).

---

## 14. Test Tags

[^ back to top](#generic-testing-framework)

Test tags let you annotate individual test files with a priority label and filter test runs to a subset of files without editing the config. This is the generic engine's replacement for the legacy `priorities.CRITICAL.files[]` model from the retired shell harness.

For the full technical reference, see [Design doc §25](../analysis/generic-testing-subsystem.md).

### 14a. Tag values

| Tag | Serialized as | When to use |
|-----|--------------|-------------|
| `critical` | `"critical"` | Enforcement gates, security, data integrity invariants |
| `high` | `"high"` | Primary user-facing behavior |
| `medium` | `"medium"` | Secondary behavior, edge cases, regression guards |
| `low` | `"low"` | Cosmetic output, optional features, rarely-exercised paths |

### 14b. Declaring tags in config

Tags are per-file inside a target's `test_files` array:

```json
{
  "name": "shell-scripts",
  "runner": "custom",
  "test_files": [
    { "path": ".codeflow/testing/pre-tool-use/test-gate-check.sh", "tags": ["critical"] },
    { "path": ".codeflow/testing/post-tool-use/test-sentinel-write.sh", "tags": ["critical"] },
    { "path": ".codeflow/testing/pre-tool-use/test-gh-pr-guard.sh", "tags": ["high"] },
    { "path": ".codeflow/testing/post-tool-use/test-checkpoint-register.sh", "tags": ["medium"] },
    { "path": ".codeflow/testing/session/test-session-start.sh", "tags": ["low"] }
  ]
}
```

A file with no `tags` (or `"tags": []`) is untagged. Untagged files run in normal (unfiltered) runs but are excluded when `--only-tag` is non-empty.

### 14c. Filtering runs

```bash
# Run only critical tests
codeflow test --only-tag critical

# Run critical and high tests
codeflow test --only-tag critical,high

# Run all tests except low-priority
codeflow test --skip-tag low

# Combine: only critical, but not if also tagged medium (unusual edge case)
codeflow test --only-tag critical --skip-tag medium
```

### 14d. Composition rules

When both `--only-tag` and `--skip-tag` are specified:

1. `--only-tag` restricts the candidate set (untagged files are excluded)
2. `--skip-tag` removes from that set
3. Skip wins over only — if a file has a tag in both lists, it is excluded

### 14e. Interaction with `--mode`

Tag filters and `--mode` are orthogonal:

```bash
# Critical tests only, full mode (coverage enforced for those tests)
codeflow test --mode full --only-tag critical

# Critical tests only, essential mode (quick smoke check)
codeflow test --mode essential --only-tag critical
```

---

## 15. Adopter Migration (Dual-Config to Single-Config)

[^ back to top](#generic-testing-framework)

If your project previously ran tests through a separate shell harness (a `run-all-tests.sh` style runner plus a legacy `test-config.json` or `priorities` file), this section walks through migrating to the single-config model. This is the same migration CodeFlow self-host went through in INF-TSK-046-008.

For the decision record, see [ADR-002](../adr/ADR-002-testing-consolidation.md).

### Step 1: Confirm the canonical config exists

```bash
codeflow test config show
```

If no config exists, run setup first:

```bash
codeflow test setup --auto
# or apply the custom-runner template
codeflow test setup --template hooks-escape-hatch.json
```

### Step 2: Add a custom runner target for your shell tests

If your shell tests are not yet in the canonical config, add them:

```bash
codeflow test config add-target \
  --name shell-scripts \
  --runner custom \
  --mode essential="bash -c 'for f in .codeflow/testing/**/*-critical.sh; do bash \"\$f\" || exit 1; done'" \
  --mode full="bash -c 'for f in .codeflow/testing/**/*.sh; do bash \"\$f\" || exit 1; done'"
```

Then set the report config:

```bash
codeflow test config set-report \
  --target shell-scripts \
  --format ctrf \
  --path ctrf.json
```

### Step 3: Register your test files with tags

For each test file in your shell suite, add it to the `test_files` array. Use `codeflow test config` to add entries or edit `.codeflow/config/testing/test-config.json` through the CLI:

```bash
# There is no single-file add-test-file subcommand; edit via codeflow test config
codeflow test config show
# Manually add test_files entries for your shell-scripts target, then validate
codeflow test doctor
```

A minimal `test_files` entry:

```json
{ "path": ".codeflow/testing/pre-tool-use/test-gate-check.sh", "tags": ["critical"] }
```

### Step 4: Add structural checking (optional but recommended)

If your shell tests have a source↔test mapping convention, add a `structural` block to catch files that drift out of sync:

```json
"structural": {
  "source_glob": [".codeflow/hooks/codeflow/**/*.sh"],
  "test_glob": [".codeflow/testing/**/test-*.sh"],
  "pattern_map": [
    {
      "source_pattern": "hooks/codeflow/{category}/{name}.sh",
      "test_pattern": "testing/{category}/test-{name}.sh"
    }
  ]
}
```

Verify the structural config before deleting the old harness:

```bash
codeflow test structural-check --only shell-scripts
```

### Step 5: Verify the full suite runs correctly

```bash
codeflow test --mode full
```

If this passes and the output matches what your old harness produced, the migration is complete.

### Step 6: Delete the old harness

Once step 5 passes on at least two consecutive runs, delete:

- The primary runner script (e.g., `run-all-tests.sh`)
- Any `lib/` shell helper files used only by that script
- The legacy config file (e.g., `.codeflow/testing/test-config.json`)
- The harness README

Verify after deletion:

```bash
codeflow test --mode full
codeflow test structural-check
```

### Step 7: Update CI

Replace any CI step that invokes the old runner with a single `codeflow test --mode full` invocation. Remove any separate structural integrity step — it now runs inside `codeflow test`.

```yaml
# Before
- name: Run structural integrity check
  run: bash .codeflow/testing/validate-structural.sh
- name: Run test suite
  run: bash .codeflow/testing/run-all-tests.sh --mode full

# After
- name: Run test suite
  run: codeflow test --mode full
```

### Step 8: Update contributor documentation

Search for references to the old runner command in `CONTRIBUTING.md`, `AGENTS.md`, agent definitions, and any onboarding docs. Replace with `codeflow test --mode full`.

---

*This guide documents the Generic Testing Framework as of INF-EPC-046. For the original adoption decision, see [ADR-001](../adr/ADR-001-generic-testing-subsystem.md). For the consolidation decision (single-config model), see [ADR-002](../adr/ADR-002-testing-consolidation.md). For the design analysis, see [Design Doc](../analysis/generic-testing-subsystem.md).*
