---
id: "epic-01kk4b8nw1q2yj6g3xn9sr7e5d"
format_id: "INF-EPC-026"
title: "Model Orchestration Layer (Epic D)"
summary: "Config-driven model orchestration in Rust CLI — stage routing engine, T1/T3 execution, PTY capture, CLI commands, model session tracking, audit log, cost tracking"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "ORCH"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-07T10:00:00Z"
updated_at: "2026-03-07T10:00:00Z"
---

# INF-EPC-026: Model Orchestration Layer (Epic D)

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic implements the Model Orchestration Layer (Epic D) for CodeFlow. It adds a config-driven system for routing work stages to different AI model CLIs, with Claude Code remaining the control plane and external models (Codex CLI, Gemini CLI, Ollama, etc.) serving as execution workers. The orchestrator is CLI tooling invoked via `codeflow orchestrate` subcommands -- not a teammate or daemon service.

The architecture centers on an `orchestration.toml` configuration file that maps `(work_stage, work_type)` tuples to model profiles with a three-tier precedence system (task override > work-type override > stage default). Two execution tiers are supported: T1 (one-shot via `tokio::process::Command`) for quick stateless tasks, and T3 (persistent tmux sessions with `model-{ulid}` naming) for multi-turn interactions requiring file modification support. A PTY-based capture layer using `portable-pty` and `vte` crates provides real-time subprocess output streaming with VT100/ANSI parsing, shared with Epic E's terminal rendering.

Epic D is organized into 3 phases with 12 tasks:

- **Phase D1 (Orchestration Core):** Config schema, model profile registry, stage routing engine, T1 one-shot executor, T3 persistent session executor
- **Phase D2 (Execution Engine):** PTY-based output capture, response normalization and completion detection, process lifecycle manager, CLI subcommands
- **Phase D3 (Tracking):** Model session tracking in SurrealDB, multi-model audit log, cost tracking and subscription configuration

## Scope

### In Scope

- Orchestration config schema at `.codeflow/config/orchestration.toml` with serde deserialization (Phase D1)
- Domain types: `ModelId`, `ModelProfile`, `ApiType`, `ExecutionTier`, `WorkStage`, `StageRouting`, `ModelSessionId`, `ExecutionStatus`, `ModelResponse`, `OrchestratorError` (Phase D1)
- Model profile registry with built-in profiles (Claude subagent, Codex CLI, Gemini CLI, Ollama local) and custom extensibility (Phase D1)
- Stage routing engine: `(work_stage, work_type)` -> `ModelProfile` with three-tier precedence (Phase D1)
- T1 one-shot executor via `tokio::process::Command` with async subprocess, timeout, process cleanup (Phase D1)
- T3 persistent session executor via tmux with `model-{ulid}` naming, send/poll/terminate lifecycle (Phase D1)
- PTY-based subprocess output capture via `portable-pty` crate with `vte` VT100 parsing (Phase D2)
- Response normalization with per-model completion detection, token counting, error detection (Phase D2)
- Process lifecycle manager with concurrent tracking, health checks, dead cleanup, max limit (Phase D2)
- CLI subcommands: `codeflow orchestrate exec`, `status`, `kill`, `config` with `--json` flag (Phase D2)
- Model session tracking in SurrealDB extending `active_work` table (Phase D3)
- Multi-model audit log as JSONL append-only ledger (Phase D3)
- Cost tracking with budget management, subscription bypass, fallback routing (Phase D3)

### Out of Scope

- Epic E: CodeFlow App (Tauri v2 + SvelteKit desktop application)
- Loro CRDT coordination for parallel execution (Epic A: INF-EPC-023)
- Schema standardization and enforcement (Epic B: INF-EPC-024)
- Global intelligence layer daemon, project registry, sync engine (Epic C: INF-EPC-025) -- consumed, not modified
- TiKV cluster deployment for large teams (future epic)
- Direct Claude Code subprocess spawning (Claude subagent ApiType returns error -- use Agent tool instead)
- Auto-selection of models based on task complexity (future enhancement)
- Model benchmarking and quality comparison (future enhancement)

## Acceptance Criteria

- [ ] Orchestration config parsed from `.codeflow/config/orchestration.toml` with full serde validation and meaningful error messages
- [ ] All domain types (`ModelId`, `ModelProfile`, `ApiType`, `ExecutionTier`, `WorkStage`, `StageRouting`, `ModelSessionId`, `ExecutionStatus`, `ModelResponse`) defined with newtype wrappers and serde derives
- [ ] `OrchestratorError` enum defined via `thiserror` with variants covering all orchestrator error domains
- [ ] Model profile registry supports built-in profiles, custom override, validation of CLI existence, and querying by stage
- [ ] Stage routing engine is stateless, deterministic, `Send + Sync`, and supports three-tier precedence with human-readable routing explanation
- [ ] T1 executor spawns async subprocess, captures stdout/stderr, enforces timeout with SIGTERM->SIGKILL, returns `ModelResponse`
- [ ] T3 executor manages tmux session lifecycle (spawn, send, poll, terminate) with idle timeout and health checks
- [ ] PTY capture provides real-time byte stream with `vte` VT100 parsing and bounded channel backpressure
- [ ] Response normalizer handles per-model completion detection, token counting, and error pattern recognition
- [ ] Process manager tracks concurrent processes, enforces max limit (default 5), auto-cleans dead processes
- [ ] All 4 CLI subcommands (`exec`, `status`, `kill`, `config`) operational with `--json` flag for machine-readable output
- [ ] Model sessions tracked in SurrealDB with `model`, `model_profile`, `execution_tier`, `cost_estimate`, `tokens_used` fields
- [ ] Audit log appends `model_selected`, `execution_started`, `execution_completed`, `execution_failed` events to JSONL
- [ ] Cost tracker aggregates costs, checks budgets before execution, falls back to next model on budget exceeded
- [ ] All existing tests pass; new Rust tests added (proptest, insta, cargo-llvm-cov >= 85%)

### PII Handling Review

- [x] Does this epic involve code that handles PII? (N -- orchestration layer handles model routing and process management, not user data)

## Tasks

| ID | Phase | Title | Work Type | Estimate | Dependencies |
|----|-------|-------|-----------|----------|--------------|
| INF-TSK-026-001 | D1 | Orchestration Config Schema and Domain Types | FEAT | M | Epic 0 complete |
| INF-TSK-026-002 | D1 | Model Profile Registry | FEAT | M | 001 |
| INF-TSK-026-003 | D1 | Stage Routing Engine | FEAT | M | 001, 002 |
| INF-TSK-026-004 | D1 | T1 One-Shot Executor | FEAT | L | 001, 002, 003 |
| INF-TSK-026-005 | D1 | T3 Persistent Session Executor | FEAT | L | 001, 002, 003, 004 |
| INF-TSK-026-006 | D2 | PTY-Based Subprocess Output Capture | FEAT | L | 004, 005 |
| INF-TSK-026-007 | D2 | Response Normalization and Completion Detection | FEAT | M | 004, 005, 006 |
| INF-TSK-026-008 | D2 | Process Lifecycle Manager | FEAT | M | 004, 005, 006 |
| INF-TSK-026-009 | D2 | CLI Subcommands (orchestrate) | FEAT | M | 003, 004, 005, 007, 008 |
| INF-TSK-026-010 | D3 | Model Session Tracking in SurrealDB | FEAT | M | 004, 005, 008, Epic C Task 005 |
| INF-TSK-026-011 | D3 | Multi-Model Audit Log | FEAT | S | 003, 010 |
| INF-TSK-026-012 | D3 | Cost Tracking and Subscription Configuration | FEAT | S | 010, 011 |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) -- provides Rust crate structure (`codeflow-core`, `codeflow-cli`), SurrealDB embedded mode (`surrealkv://`), clap CLI framework, trait-based abstractions (`DataStore`, `LedgerWriter`), core types and error patterns
- INF-EPC-025 (Epic C: Global Intelligence Layer) -- provides SurrealDB daemon and global schema (Task 005: Global SurrealDB Instance) for model session tracking

### Blocks

- INF-EPC-027 (Epic E: CodeFlow App) -- depends on Epic D's PTY capture infrastructure (`portable-pty`, `vte`) and process management for streaming terminal features

## Technical Notes

**Architectural constraints:**

- D17 (SurrealDB-only): All schema design targets SurrealDB. `SurrealStore` is the sole `DataStore` implementation.
- D21 (Multi-model orchestration): Claude Code remains the control plane; external models are delegated via tmux (T3) or direct subprocess (T1).
- D22 (Revised epic structure): Epic D depends on Epic 0 + Epic C. Epic D blocks Epic E.
- D24 (Combined D+E vision): Shared PTY infrastructure between Epic D and Epic E. Epic D is CLI-first and headless.
- Rust idioms: All tasks use thiserror error enums, newtype wrappers, serde derive macros. Testing uses proptest, insta, cargo-llvm-cov (>= 85%).
- Crate structure: Core library at `codeflow-rs/codeflow-core/src/orchestrator/`, CLI commands at `codeflow-rs/codeflow-cli/src/commands/orchestrate/`.
- Config file: `.codeflow/config/orchestration.toml` parsed via `toml` crate with serde deserialization.
- PTY infrastructure: `portable-pty` for cross-platform PTY allocation, `vte` (wezterm project) for VT100/ANSI parsing. Both shared with Epic E.

**Dependency graph:**

```text
001 (Config & Types)
 |
 +---> 002 (Profile Registry) --+---> 003 (Routing Engine)
 |                               |
 |                               +---> 004 (T1 Executor)
 |                                      |
 |                                      v
 |                                     005 (T3 Executor)
 |                                      |
 |    +-----------+-----------+---------+
 |    |           |           |
 |    v           v           v
 |   006 (PTY)  007 (Resp)  008 (Process Mgr)
 |    |           |           |
 |    +-----------+-----+-----+
 |                      |
 |                      v
 |             009 (CLI Commands)
 |                  uses 003 (Routing) for model selection
 |
 +---> 010 (Session Tracking) ---> 011 (Audit Log) ---> 012 (Cost Tracking)
```

> Note: Task table Blocked By fields are authoritative for dependency ordering. 004 and 005 are sequential (005 blocked by 004). 003 (Routing Engine) is used by 009 (CLI) for model selection, not directly by executors (004/005).

**New crate dependencies (Epic D):**

- `portable-pty` -- cross-platform PTY allocation (openpty on macOS, /dev/ptmx on Linux)
- `vte` -- VT100/ANSI escape sequence parser (from wezterm project)
- `ulid` -- session ID generation (ModelSessionId format: `model-{ulid}`)
- `dashmap` -- concurrent HashMap for process registry
- `toml` -- TOML configuration file parsing
- Existing workspace: `surrealdb`, `tokio`, `serde`, `serde_json`, `thiserror`, `clap`, `proptest`, `insta`, `tracing`

**Source analysis:**

- `.codeflow/docs/analysis/parallel-work/product-strategy.md` Section 10 -- Epic D task descriptions and architecture
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- decisions #21, #22, #24
- `.codeflow/docs/archived/skills/cf-model-orchestrator/SKILL.md` -- 14 operations, T1/T3 model, session tracking patterns

## Related

- INF-EPC-022: Rust CLI Idiomatic Redesign (Epic 0 -- prerequisite)
- INF-EPC-025: Global Intelligence Layer (Epic C -- prerequisite for session tracking)
- INF-EPC-023: Parallel Execution Core (Epic A -- parallel, independent)
- INF-EPC-024: Data Layer Standardization (Epic B -- parallel, independent)
- `.codeflow/docs/analysis/parallel-work/product-strategy.md` Section 10
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- D21, D22, D24
- `.codeflow/docs/archived/skills/cf-model-orchestrator/SKILL.md`
