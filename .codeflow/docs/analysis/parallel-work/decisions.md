---
title: "Analyzed Recommendations"
type: analysis
status: draft
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

---

## 9. Memory File Coordination

**Question:** How should `.claude/memory/` files be coordinated across parallel sessions?

**Answer: Not needed -- git merge handles it.** `.claude/memory/` is git-tracked. Each worktree gets its own copy of memory files via `git worktree add`. When sessions create PRs, git's standard merge handles reconciliation. No symlinking, no flock, no CRDT needed for memory files. Memory files are Tier 2 derived context summaries -- git merge conflicts (if any) are trivially resolvable.

---

## 10. Maximum Concurrent Workers

**Recommendation: 3 concurrent workers.** Matches V4 spec's `max_session_workers`. Within SQLite WAL capacity. Reasonable disk and token costs.

---

## 11. SurrealDB Approach

**Recommendation: Pure Rust CLI (Option E from [Data Layer Protection](data-layer-protection.md) Section 5).** After Epic 0 (Rust CLI redesign), SurrealDB embedded is a direct crate dependency -- `use surrealdb::Surreal;`. `SurrealStore` implements Epic 0's `DataStore` trait alongside `SqliteStore`, making backend selection a configuration choice. This provides true embedded operation (no sidecar process) with native performance. SurrealDB handles vector/graph/document capabilities; SQLite continues handling operational CRUD. Both coexist in the same Rust binary. Evaluate SQLite-to-SurrealDB consolidation for CRUD operations as a future initiative.

---

## 12. Documentation Structure

**Recommendation: Package structure (the structure you are reading now).** Decompose the analysis into focused sub-documents linked from a README. Each file is self-contained with YAML frontmatter and TOC.

---

## 13. Epic Organization

**Recommendation: 3 epics.** Epic 0 (Rust CLI Migration -- prerequisite foundation), Epic A (Parallel Execution Core), and Epic B (Data Layer Standardization). Epic 0 must complete first. Epic A depends on Epic 0 (Loro is a native crate dep). Epic B Phase D can run after Epic 0. Epic B Phase F depends on Epic 0 (SurrealDB is a native crate dep).

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

**Recommendation: Option E (Pure Rust CLI).** Redesign the Go CLI in idiomatic Rust as Epic 0 (prerequisite). Trait-based abstractions (`DataStore`, `Coordinator`) enable both `SqliteStore` and `SurrealStore` implementations behind the same interface. SurrealDB and Loro become direct crate dependencies (`use surrealdb::Surreal;`, `use loro::LoroDoc;`). No FFI, no shared library, no CGo. The sync daemon becomes a `codeflow sync daemon` subcommand within the single binary.

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

**Migration phases (Epic 0 — 7 phases, 25 tasks):**
- Phase 0A: Codebase Analysis + Tooling (audit Go CLI, define trait hierarchy, create cf-rust-standards skill, configure Rust MCP servers)
- Phase 0B: Foundation + Testing Infrastructure (workspace, core types, error domains, unit test harness with coverage + proptest + insta + nextest, shell integration test bridge — testing ready BEFORE code migration)
- Phase 0C: Core Library Crate (DataStore, LedgerWriter, session, worktree, claims, workgraph)
- Phase 0D: Hook Handlers (HookHandler trait + implementations)
- Phase 0E: CLI Commands (Clap dispatch layer)
- Phase 0F: Integration Testing + Contract Conformance (port Go tests, contract conformance suite, full shell test suite validation)
- Phase 0G: CI/CD + Cutover (cargo build + test + clippy + fmt + coverage + shell tests, Go removal)

---

[← Back to Overview](README.md)
