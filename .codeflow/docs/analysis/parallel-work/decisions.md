---
title: "Analyzed Recommendations"
type: analysis
status: active
author: cf-planning
created_at: "2026-03-04"
updated_at: "2026-03-05"
parent: "parallel-work/README.md"
---

# Analyzed Recommendations

[← Back to Overview](README.md)

## Table of Contents

- [1. Worktree Lifecycle](#1-worktree-lifecycle)
- [2. Worktree Creation Timing](#2-worktree-creation-timing)
- [3. Shared vs Per-Worktree Database](#3-shared-vs-per-worktree-database)
- [4. Team Name Convention](#4-team-name-convention)
- [5. Claim Conflict UX](#5-claim-conflict-ux)
- [6. Merge Queue Ordering](#6-merge-queue-ordering)
- [7. Interactive Visibility of Parallel Work](#7-interactive-visibility-of-parallel-work)
- [8. Loro CRDT as Foundation](#8-loro-crdt-as-foundation)
- [9. Memory File Coordination](#9-memory-file-coordination)
- [10. Maximum Concurrent Workers](#10-maximum-concurrent-workers)
- [11. SurrealDB Approach](#11-surrealdb-approach)
- [12. Documentation Structure](#12-documentation-structure)
- [13. Epic Organization](#13-epic-organization)
- [14. Rust Data Layer Architecture](#14-rust-data-layer-architecture)
- [15. Worktree Base Directory](#15-worktree-base-directory)
- [16. Rust CLI Migration Strategy](#16-rust-cli-migration-strategy)
- [17. SurrealDB as Single Database Platform](#17-surrealdb-as-single-database-platform)
- [18. Global Database Architecture](#18-global-database-architecture)
- [19. Project Identity and Scoping](#19-project-identity-and-scoping)
- [20. Cross-Project Visibility Model](#20-cross-project-visibility-model)
- [21. Multi-Model Orchestration Strategy](#21-multi-model-orchestration-strategy)
- [22. Revised Epic Structure](#22-revised-epic-structure)
- [23. Knowledge Graph Synchronization](#23-knowledge-graph-synchronization)

---

## 1. Worktree Lifecycle

**Question:** Should worktrees be created at session start and destroyed at session end, or persist for reuse?

| Option | Pros | Cons |
|--------|------|------|
| A. Ephemeral (create at SessionStart, destroy at SessionEnd) | Simple lifecycle. No stale state. Clean slate. | ~1-2s creation overhead. Cannot resume in same worktree. |
| B. Persistent (reuse across sessions) | Fast startup. Branch state preserved. | Stale state accumulation. Branch drift. Complex cleanup. |

**Recommendation: Option A (Ephemeral).** Create at SessionStart hook, destroy at SessionEnd hook. The 1-2 second overhead is negligible. One PR per session is the existing principle.

---

## 2. Worktree Creation Timing

**Question:** SessionStart hook or PF1-INIT?

| Option | Pros | Cons |
|--------|------|------|
| A. SessionStart hook (before PF1) | All PathFlow state written inside worktree from the start. No migration needed. Clean isolation. | SessionStart hook becomes more complex. |
| B. PF1-INIT | Aligns with PathFlow phase model. | PF1 state (checkpoint, flag, sentinels) written to main repo, must be migrated to worktree. Race condition window. |

**Recommendation: Option A (SessionStart hook).** The user is correct: if worktree is at PF1, the pathflow-phase-tasks and sentinels up to PF1 would be in the main session, creating migration complexity. SessionStart is the right place.

---

## 3. Shared vs Per-Worktree Database

**Question:** Should the SQLite database be shared (symlinked) or per-worktree?

| Option | Pros | Cons |
|--------|------|------|
| A. Shared (symlinked) | Single source of truth. No sync logic. WAL handles concurrency. | Write contention under heavy load. |
| B. Per-worktree (independent) | Zero write contention. Complete isolation. | Requires sync logic. Stale reads. Defeats single-source-of-truth. |

**Recommendation: Option A (Shared).** SQLite WAL with busy_timeout=15000 handles 3 concurrent sessions. Per-worktree databases would require complex sync and violate the three-tier data model.

---

## 4. Team Name Convention

**Question:** How should parallel sessions generate unique team names?

| Option | Pros | Cons |
|--------|------|------|
| A. `{project}-{SID-short}` | Simple. SID is unique. | Opaque, hard to identify which task. |
| B. `{project}-{task-format-id}` | Human-readable. Clear task association. | Requires task ID to be known (not available until PF4 for adhoc). |
| C. `{project}-{task-format-id}-{SID-short}` | Best of both. Readable + unique. | Longer name. |

**Recommendation: Option C (`{project}-{task-format-id}-{SID-short}`) for planned tasks, Option A (`{project}-{SID-short}`) for adhoc tasks.** Claude Code's `TeamCreate` accepts custom team names via the `team_name` parameter. For planned tasks where the task format ID is known from the epic task list, include it for readability. For adhoc tasks where the task ID is generated at PF4, use the session ID.

---

## 5. Claim Conflict UX

**Question:** When a session cannot proceed because another holds a file claim, what UX?

| Option | Pros | Cons |
|--------|------|------|
| A. Block and wait | Automatic resolution. | Session blocked. Bad interactive UX. |
| B. Advisory (log + warn) | Non-disruptive. Session continues. | May produce PR merge conflicts. |
| C. Two-phase: Advisory then Enforcing | Gradual rollout. Learn from data. | Two phases to manage. |

**Recommendation: Option C (Two-phase).** Start advisory. In autorun, queue conflicted task. In interactive, show notification. After data collection, switch to enforcing.

---

## 6. Merge Queue Ordering

**Recommendation: FIFO (completion order).** Simple, fair, deterministic. Priority ordering is premature optimization.

---

## 7. Interactive Visibility of Parallel Work

**Recommendation: Minimal (worker count + claim conflicts only).** Low noise, only actionable information. Details available via `codeflow autorun status`.

---

## 8. Loro CRDT as Foundation

**Question:** Should Loro integration be deferred, phased in after flock, or be the sole coordination mechanism from day one?

| Option | Pros | Cons |
|--------|------|------|
| A. Defer to future epic | Less scope. | Misses multi-machine sync. Delays V4 spec alignment. Two different coordination mechanisms to maintain. |
| B. Phase in: flock first, Loro later | Incremental. Lower initial risk. | Half-measure. Two coordination mechanisms. Migration cost. Technical debt from day one. |
| **C. Loro from day one (RECOMMENDED)** | **One mechanism for same-machine AND multi-machine. No migration. No flock technical debt. V4 spec aligned. Native Rust.** | **Rust CLI redesign (Epic 0) is a prerequisite.** |

**Recommendation: Option C (Loro from day one).** After Epic 0 (Rust CLI redesign), Loro is a direct crate dependency -- `use loro::LoroDoc;`. `LoroCoordinator` implements the `Coordinator` trait from Epic 0. No FFI, no community-maintained wrappers, no UniFFI indirection. One coordination mechanism handles both same-machine parallel sessions and multi-machine sync.

**Scope update (Decision #23):** Loro's scope now extends beyond coordination (claims, file ownership) to include knowledge graph entity/relationship sync across developers. Entities and relationships are small records (< 1KB) that fit naturally into LoroMap containers. Embeddings remain local-only (regenerated per machine via ONNX). See [Decision #23: Knowledge Graph Synchronization](#23-knowledge-graph-synchronization) for the full team-size decision matrix.

---

## 9. Memory File Coordination

**Question:** How should `.claude/memory/` files be coordinated across parallel sessions?

**Answer: Not needed -- git merge handles it.** `.claude/memory/` is git-tracked. Each worktree gets its own copy of memory files via `git worktree add`. When sessions create PRs, git's standard merge handles reconciliation. No symlinking, no flock, no CRDT needed for memory files. Memory files are Tier 2 derived context summaries -- git merge conflicts (if any) are trivially resolvable.

---

## 10. Maximum Concurrent Workers

**Recommendation: 3 concurrent workers.** Matches V4 spec's `max_session_workers`. Within SQLite WAL capacity. Reasonable disk and token costs.

---

## 11. SurrealDB Approach

**Recommendation: Pure Rust CLI (Option E from [Data Layer Protection](data-layer-protection.md) Section 5).** After Epic 0 (Rust CLI redesign), SurrealDB embedded is a direct crate dependency — `use surrealdb::Surreal;`. `SurrealStore` is the sole `DataStore` implementation — SQLite is not present in the Rust CLI (see [Decision #17](decisions.md#17-surrealdb-as-single-database-platform)). This provides true embedded operation (`surrealkv://`) with native performance and a unified SurrealQL query language across all operations. SurrealDB handles CRUD, vector, graph, and document capabilities in a single store.

---

## 12. Documentation Structure

**Recommendation: Package structure (the structure you are reading now).** Decompose the analysis into focused sub-documents linked from a README. Each file is self-contained with YAML frontmatter and TOC.

---

## 13. Epic Organization

**Updated: 6 epics.** See [Decision #22](decisions.md#22-revised-epic-structure) for the current epic structure. The original 3-epic recommendation (Epic 0, A, B) was superseded when the global intelligence layer and model orchestration scope were added. The current structure is: Epic 0 (Rust CLI — Idiomatic Redesign, prerequisite foundation), Epic A (Parallel Execution Core), Epic B (Data Layer Standardization), Epic C (Global Intelligence Layer), Epic D (Model Orchestrator), Epic E (Dashboard — deferred). Epic 0 must complete first. Epics A, B, and C can run in parallel after Epic 0. Epic D depends on Epic 0 and Epic C.

---

## 14. Rust Data Layer Architecture

**Question:** How should CodeFlow integrate SurrealDB and Loro CRDT given the Go embedded gap?

| Option | Pros | Cons |
|--------|------|------|
| A. SurrealDB sidecar + Loro community CGo | Established patterns. | Sidecar process management. Community bindings may lag. Two separate integrations. |
| B. SurrealDB sidecar + Loro via shared lib | Good Loro quality. | Still need sidecar for SurrealDB. Two different integration strategies. |
| C. Wait for Go embedded SDK | Zero build complexity. | Unknown timeline. May never happen. |
| D. Unified Rust shared library | Both native in one library. No sidecar. No community bindings. | Rust toolchain in CI. Two-language codebase. CGo FFI overhead. |
| **E. Pure Rust CLI (RECOMMENDED)** | **Single language. Maximum performance. No FFI overhead. No CGo complexity. Single binary. One build pipeline.** | **Large scope (full CLI port). Requires Rust expertise across team.** |

**Recommendation: Option E (Pure Rust CLI).** Redesign the Go CLI in idiomatic Rust as Epic 0 (prerequisite). Trait-based abstractions (`DataStore`, `Coordinator`) are defined in `codeflow-core`. `SurrealStore` is the sole `DataStore` implementation — SQLite is not present in the Rust redesign (see [Decision #17](decisions.md#17-surrealdb-as-single-database-platform)). SurrealDB and Loro become direct crate dependencies (`use surrealdb::Surreal;`, `use loro::LoroDoc;`). No FFI, no shared library, no CGo. The sync daemon becomes a `codeflow sync daemon` subcommand within the single binary.

This supersedes the earlier Option D (Rust shared library) recommendation. Option E eliminates the CGo FFI boundary entirely, removes two-language build complexity, and produces a single `codeflow` binary. The scope of the full CLI port is offset by the long-term simplicity gains: one language, one build pipeline, one test framework.

See [Data Layer Protection](data-layer-protection.md) Sections 5 and 6 for the full SurrealDB evaluation and pure Rust CLI architecture details.

---

## 15. Worktree Base Directory

**Question:** Should worktrees live under `.claude/worktrees/` (current code default) or `.git-worktrees/` (current `.gitignore` entry)?

| Option | Pros | Cons |
|--------|------|------|
| A. `.claude/worktrees/` (current code) | Groups under `.claude/`. | `.claude/` is config space, not runtime artifacts. Requires `.gitignore` change. Conflicts conceptually with Claude Code SDK's `EnterWorktree` tool which also uses `.claude/worktrees/`. |
| **B. `.git-worktrees/` (RECOMMENDED)** | **`.gitignore:67` already correct. Git-adjacent naming convention. Clear separation: config (`.claude/`) vs runtime (`.git-worktrees/`). No conflict with `.git/worktrees/` (different path).** | **Requires code change at `worktree.go:102`.** |
| C. `.worktrees/` (top-level) | Simple. | Adds another dotdir. No git naming convention alignment. |

**Recommendation: Option B (`.git-worktrees/`).** The `.gitignore` is already correct -- the code default is the mismatch. `.claude/` is Claude Code's configuration space (agents, settings, skills, memory); worktrees are ephemeral runtime artifacts that belong outside it. `.git-worktrees/` follows git-adjacent naming (`git` prefix like `.gitignore`, `.gitattributes`) and communicates purpose clearly. No conflict with `.git/worktrees/` which is git's internal metadata directory for tracking worktree state.

**Implementation:** Change the default return value at `worktree.go:102` from `filepath.Join(m.ProjectDir, ".claude", "worktrees")` to `filepath.Join(m.ProjectDir, ".git-worktrees")`. Update tests. Remove any empty `.claude/worktrees/` directory.

---

## 16. Rust CLI Migration Strategy

**Question:** How should the Go-to-Rust CLI migration be executed, and what is the migration philosophy?

**Key constraint: External interface is FROZEN.** Same binary name (`codeflow`), same subcommands, same hook stdin/stdout JSON contract, same exit codes, same file I/O paths. Everything internal is fair game for redesign.

| Option | Pros | Cons |
|--------|------|------|
| A. Line-for-line port | Lowest design risk. Mechanical translation. | Carries over Go-specific workarounds, DRY violations, and string-typed states. Misses Rust's type system advantages. |
| **B. Idiomatic redesign with incremental coexistence (RECOMMENDED)** | **Leverages Rust's type system (enums, traits, Result). Consolidates duplicate logic. Trait-based abstractions enable testing and future extensibility. Go and Rust binaries coexist for safe validation.** | **Higher upfront design effort (Phase 0A analysis). Requires careful contract conformance testing.** |
| C. Big bang rewrite | Clean break. No coexistence period. | High risk. Long gap with no working binary. No incremental validation. |

**Recommendation: Option B (Idiomatic redesign with incremental coexistence).**

The Rust CLI is a redesign, not a transliteration. The Go codebase is the functional specification, but the Rust implementation applies idiomatic patterns:

- **Trait-based abstractions** (`DataStore`, `Coordinator`, `Transport`, `LedgerWriter`, `HookHandler`) for swappable implementations and testability
- **Type system leverage** — session states as enums, phase transitions as state machines, event types as tagged unions (serde), sentinels as enum variants
- **Domain-specific error types** — `thiserror`-derived enums per domain (`SessionError`, `DbError`, `HookError`, `LedgerError`) replacing Go's `fmt.Errorf`
- **DRY consolidation** — duplicate logic across Go packages is identified in Phase 0A (codebase analysis) and consolidated into shared modules
- **Modular crate structure** — `codeflow-core` (library) + `codeflow-cli` (binary). Core logic is independently testable.
- **Builder pattern** for complex configuration (sessions, database connections, hook context)
- **Serde for all serialization** — no manual JSON construction

Go and Rust binaries coexist during migration. A contract conformance test suite verifies the Rust binary produces identical external behavior (stdout, exit codes, file output) to the Go binary. After all tests pass, the Go binary is removed.

**Migration phases (Epic 0 — 7 phases, 27 tasks):**
- Phase 0A: Codebase Analysis + Tooling (audit Go CLI, define trait hierarchy, create cf-rust-standards skill, configure Rust MCP servers)
- Phase 0B: Foundation + Testing Infrastructure (workspace, core types, error domains, unit test harness with coverage + proptest + insta + nextest, shell integration test bridge — testing ready BEFORE code migration)
- Phase 0C: Core Library Crate (DataStore, LedgerWriter, session, worktree, claims, workgraph)
- Phase 0D: Hook Handlers (HookHandler trait + implementations)
- Phase 0E: CLI Commands (Clap dispatch layer)
- Phase 0F: Integration Testing + Contract Conformance (port Go tests, contract conformance suite, full shell test suite validation)
- Phase 0G: CI/CD + Cutover (cargo build + test + clippy + fmt + coverage + shell tests, Go removal)

---

## 17. SurrealDB as Single Database Platform

**Question:** Should SurrealDB supplement SQLite (hybrid) or replace it entirely?

| Option | Pros | Cons |
|--------|------|------|
| A. Hybrid (SQLite + SurrealDB) | Incremental migration. SQLite handles CRUD. SurrealDB handles vector/graph. | Two databases. Data sync logic. Double infrastructure surface area. |
| **B. SurrealDB-only (DECIDED)** | **Single query language (SurrealQL). One data model. Unified embedded operation. No sync logic between two stores.** | **Full schema migration from 37 SQLite tables. All SQL rewritten in SurrealQL.** |

**Decision: Option B (SurrealDB-only).** SQLite is dropped entirely. SurrealDB is the sole Tier 1 database.

- **Project level:** SurrealDB embedded mode (`surrealkv://`) at `.state/db/codeflow/`. No server process, no sidecar.
- **Global level (optional):** SurrealDB daemon mode via Unix socket at `~/.codeflow/db.sock`. Enabled explicitly via `codeflow global enable`.
- **Same query language everywhere.** SurrealQL replaces SQL across all contexts. No dual-language query maintenance.
- **DataStore trait has ONE implementation:** `SurrealStore` with two connection modes — embedded (`surrealkv://`) for project-local and connected (`ws+unix://`) for global daemon. `SqliteStore` is not present in the Rust redesign — SurrealDB embedded replaces SQLite from the start of Epic 0.
- **Hot-path hooks use filesystem, not DB.** Gate-check, sentinel-write, checkpoint, team-guard, and edit-write-guard all operate on filesystem state. SurrealDB embedded latency (~1-3ms) is acceptable for the actual DB access patterns (session lifecycle, task CRUD, context search).
- **Three-tier model updated:** Tier 0 JSONL (unchanged, rebuild authority), Tier 1 SurrealDB embedded (replaces SQLite), Tier 1+ SurrealDB daemon (optional global layer), Tier 2 Markdown (unchanged).

---

## 18. Global Database Architecture

**Question:** How should a global (cross-project) database layer be architected, and what is the default operating mode?

| Option | Pros | Cons |
|--------|------|------|
| A. Always-on TCP server | Uniform access model. | Network exposure. Port conflicts. Requires external SurrealDB install or Docker. |
| B. Optional Unix socket daemon inside the codeflow binary | Local only (no network exposure). ~1-2ms latency. No external process. Opt-in. Graceful degradation. | Daemon lifecycle management (launchd/systemd). |
| **C. Option B (DECIDED)** | See above. | See above. |

**Decision: Optional Unix socket daemon, embedded inside the codeflow binary.**

- **Default mode:** Project-only with SurrealDB embedded (`surrealkv://`). No daemon required.
- **Global mode:** Enabled via `codeflow global enable`. Starts the SurrealDB daemon process, registers the current project. Can be added or removed at any time without affecting local project data.
- **Three operating modes:**

  | Mode | Description |
  |------|-------------|
  | Project-only (default) | SurrealDB embedded at `.state/db/codeflow/`. No daemon. |
  | Global-enabled | Embedded for local ops + daemon at `~/.codeflow/db.sock` for cross-project queries. |
  | Global-only (future) | Large org scenario — all state via daemon, no per-project embedded DB. |

- **Daemon is INSIDE the codeflow Rust binary.** Invoked as `codeflow db daemon start`. No separate SurrealDB install required. No Docker. The binary ships everything.
- **Unix socket, not TCP.** Path: `~/.codeflow/db.sock`. Local-only. No network exposure. Latency ~1-2ms.
- **Reliability:** `codeflow db install-service` creates a launchd plist (macOS) or systemd unit (Linux). `auto_start=true` in global config triggers auto-start on first use.
- **Graceful degradation:** If the daemon is down, all project-local operations continue unaffected via embedded mode. Cross-project features (global memory search, cross-project task queries) are unavailable until the daemon restarts.

---

## 19. Project Identity and Scoping

**Question:** What is a "project" in CodeFlow, and how is project identity determined?

**Decision: A project is a git repository.**

This is the simplest, most natural boundary that developers already understand.

- **`project_id`** is auto-generated on first registration (format: `proj_{short_hash}`). Stable identifier — not based on directory name or branch.
- **Monorepo:** One project, multiple AREA prefixes (e.g., `FE-EPC-001`, `BE-EPC-001`, `INF-EPC-001`). Already handled by the existing convention. No change needed.
- **Multi-repo team:** N projects = N repos. Each registers independently. Linked via global DB visibility config (see Decision #20).
- **Epic and task creation remain repo-scoped.** No change to epic/task creation flow. Global DB aggregates them with the `project_id` field for cross-project queries.
- **Project registration schema:**

  | Field | Type | Description |
  |-------|------|-------------|
  | `project_id` | string | Auto-generated (e.g., `proj_{short_hash}`) |
  | `name` | string | Human-readable project name |
  | `repo_path` | string | Absolute local filesystem path |
  | `repo_url` | option\<string\> | Remote git URL |
  | `area_prefixes` | array\<string\> | AREA prefix codes in use (e.g., `["INF", "PLN", "FE"]`) |
  | `registered_at` | datetime | Registration timestamp |
  | `last_synced` | datetime | Last sync with global daemon |

---

## 20. Cross-Project Visibility Model

**Question:** How do projects opt into sharing data with each other, and how are cross-project queries scoped?

**Decision: Per-project opt-in visibility config with SurrealDB graph relations.**

- **Config file:** `.codeflow/config/project.toml` specifies `visible_projects` (list of `project_id` values) and `visible_scopes` (e.g., `memory`, `tasks`, `epics`).
- **Sharing:** Projects opt in by setting `allow_sharing = true` and `shared_scopes = ["memory", "tasks"]` in their config.
- **Registration CLI:**

  ```text
  codeflow project register --name "backend-api" --tags "team-alpha"
  codeflow project link proj_backend_def --scopes memory,tasks
  ```

- **Graph relation in SurrealDB:** `can_access TYPE RELATION FROM project TO project` with a `scopes` array field. Visibility is queryable as a graph traversal.
- **Cross-project queries:** A single SurrealQL statement can traverse the visibility graph and include results from linked projects. Example: a frontend project queries the backend project's API design decisions via semantic vector search across linked project memories.
- **No implicit sharing.** Projects that have not registered with the global daemon or have not configured `visible_projects` are invisible to all other projects.

---

## 21. Multi-Model Orchestration Strategy

**Question:** Should CodeFlow support routing work to models other than Claude, and if so how?

**Decision: Claude Code remains the control plane; external models are delegated via tmux execution.**

- **Claude Code is the control plane.** PathFlow, hook enforcement, task graph management, and coordination remain Claude Code's responsibility. This is unchanged.
- **External model delegation:** Models like Codex, Gemini, and Ollama are invoked for work stage execution (WS-DEV, WS-REV, WS-QA) via tmux:
  - **T1 (one-shot):** Spawn model in a tmux pane, send prompt, collect output, close pane.
  - **T3 (persistent):** Keep model running in a tmux pane across multiple exchanges.
- **Config-driven routing:** Work stage → model assignment is a configuration table, not hardcoded. Teams configure which model executes which stage.
- **Source:** The archived `cf-model-orchestrator` skill (`.codeflow/docs/archived/skills/cf-model-orchestrator/SKILL.md`) will be revived and ported to the Rust CLI as part of Epic D.
- **Motivation:** Reduces platform dependency. Teams use their existing model subscriptions for work execution while Claude Code provides the orchestration discipline. Claude Code subscription (~$20/month) handles orchestration; other model subscriptions handle execution.
- **Competitive positioning:** CodeFlow is NOT competing with autonomous agent platforms (Factory.ai, Devin). CodeFlow = structured workflow discipline for human-directed AI development. The combination of Claude Code ($20) + CodeFlow ($10) = $30/dev/month targets a competitive price point.

---

## 22. Revised Epic Structure

**Question:** How should the expanded scope (global DB, model orchestration) be organized into epics?

**Decision: 6 epics (Epic 0 + Epics A through E), with Epic 0 as the universal prerequisite.**

| Epic | Title | Scope | Tasks | Dependencies |
|------|-------|-------|-------|-------------|
| Epic 0 | Rust CLI — Idiomatic Redesign | Replace Go CLI with pure Rust. `SurrealStore` as the ONLY `DataStore` impl (no `SqliteStore`). 7 phases. | 25 | None — prerequisite for all |
| Epic A | Parallel Execution Core | Loro CRDT, worktrees, singleton elimination, autorun integration. | 19 | Epic 0 |
| Epic B | Data Layer Standardization | Schema standardization in SurrealQL (not SQL). Phase F (SurrealDB) merged into Epic 0 — REMOVED from Epic B. | ~16 | Epic 0; A, B, C can run in parallel after Epic 0 |
| Epic C | Global Intelligence Layer | Daemon mode, project registry, sync engine, local embeddings (`all-MiniLM-L6-v2` via `ort` crate, 384 dims, ~50MB), cross-project queries, knowledge graph extraction pipeline (INGEST/COGNIFY/MEMIFY/SEARCH), Loro CRDT KG sync for teams < 15 devs, codeflow-knowledge.toml config, extended ontology (16 entity types, 18 relationship types), bootstrap CLI command, trigger points and scheduling. | 14-18 | Epic 0 |
| Epic D | Model Orchestrator | Port `cf-model-orchestrator` to Rust. Config-driven model-to-stage routing. T1/T3 tmux execution. | 8-10 | Epic 0 + Epic C |
| Epic E | Dashboard | SvelteKit + Tauri. Deferred until Epics 0-D stabilize. | TBD | Stable Epic 0 + B + C |

**Key changes from the previous 3-epic structure:**

- Epic 0 is updated: `SurrealStore` is now the ONLY `DataStore` implementation. `SqliteStore` is not present in the Rust redesign — SurrealDB embedded replaces SQLite from the start.
- Epic B is reduced: Phase F (SurrealDB) was previously in Epic B but is now integral to Epic 0 (since the Rust CLI starts with SurrealDB, not SQLite). Epic B now covers schema standardization and enforcement only (~16 tasks, 2 phases).
- Epic C is new: Global Intelligence Layer — daemon, project registry, sync engine, local ONNX embeddings, cross-project SurrealQL queries.
- Epic D is new: Model Orchestrator — revives and ports the archived `cf-model-orchestrator` skill.
- Epic E is new (deferred): Dashboard — SvelteKit + Tauri, after the data and orchestration layers stabilize.

**Dependency ordering:**

```text
Epic 0 (prerequisite — must complete first)
    |
    +---> Epic A (parallel execution — can start after Epic 0)
    +---> Epic B (data standardization — can start after Epic 0, parallel with A)
    +---> Epic C (global intelligence — can start after Epic 0, parallel with A, B)
              |
              +---> Epic D (model orchestrator — depends on Epic 0 + C)
                        |
                        +---> Epic E (dashboard — depends on stable 0 + B + C)
```

**Local embeddings for vector search (Epic C):**

- Model: `all-MiniLM-L6-v2` (384 dimensions, ~50MB)
- Runtime: Local ONNX model via `ort` crate (ONNX Runtime for Rust)
- Works fully offline, no API dependency
- Embeddings generated on write when memory events are synced to SurrealDB

---

## 23. Knowledge Graph Synchronization

**Question:** How should knowledge graph entities and relationships be synchronized across multiple developers/machines?

| Option | Pros | Cons |
|--------|------|------|
| A. TiKV cluster (SurrealDB distributed mode) | Transparent multi-node access. Strong consistency. | Requires 7+ servers (3 TiKV + 3 PD + 1 monitoring). 16+ cores, 32+ GB RAM per node. Massive overkill for small teams. |
| B. SurrealDB Cloud | Managed, no infrastructure. | Monthly cost. Vendor dependency. Network latency. Not offline-capable. |
| **C. Loro CRDT for small/medium teams + TiKV for large/enterprise (DECIDED)** | **Zero infrastructure for teams < 15. Reuses existing Loro infrastructure (Decision #8). Deterministic merge. Works offline. Embeddings local-only.** | **CRDT overhead grows with team size. Not suitable for 15+ developers.** |

**Decision: Option C (Loro CRDT for small/medium, TiKV for large/enterprise).**

Extend the existing Loro infrastructure (already designed in Decision #8 for coordination -- claims, file ownership) to handle knowledge graph entity/relationship sync. No new infrastructure for teams < 15 developers.

**Team size -> sync mechanism:**

| Team Size | KG Sync Mechanism | Database |
|-----------|-------------------|----------|
| Solo | N/A -- single embedded DB | surrealkv:// |
| Small team (2-5) | Loro CRDT via git refs | Each dev: local surrealkv://, KG entities synced via Loro |
| Medium team (5-15) | Loro CRDT + optional daemon | Local surrealkv:// + optional daemon for shared queries |
| Large team (15+) | SurrealDB TiKV cluster (self-hosted or cloud) | Shared TiKV-backed SurrealDB |
| Enterprise (50+) | SurrealDB Cloud Dedicated | Managed multi-node cluster |

**Key design choices:**

- **Entities and relationships synced via Loro:** Small records (< 1KB) mapped to LoroMap containers. Delta sync via git refs (30s interval). Deterministic merge -- no conflict resolution needed.
- **Embeddings are local-only:** Too large for CRDT (1.5KB per 384-dim embedding). Same ONNX model + same text = identical embeddings. Each machine regenerates locally.
- **TiKV cost analysis:** TiKV is 100% free (Apache 2.0, CNCF graduated) but the infrastructure cost is substantial -- minimum 7 servers with high-end specs. Impractical for CodeFlow's target audience (small dev teams, indie developers).
- **SurrealDB pricing:** Community edition is free for self-hosted. Cloud tiers start at $0/month (free tier, 1GB) through custom enterprise pricing.

**References:** [Knowledge Graph Engine Analysis, Section 8](knowledge-graph-engine.md#8-multi-user-synchronization-architecture), [Decision #8 (Loro CRDT)](#8-loro-crdt-as-foundation), [crdt-coordination.md](crdt-coordination.md).

---

[<- Back to Overview](README.md)
