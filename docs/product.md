# codeflow — product

<!-- WHY layer. Human-owned: agents propose changes here, the human accepts.
     Keep it small and stable — this file should change rarely.
     The non-goals below are what planning is checked against. -->

## Purpose

CodeFlow is the AI-development discipline layer you install into any repo: one
Rust binary (`codeflow`) plus an embedded scaffold that scaffolds, enforces,
verifies, and remembers — while Claude Code (or any harness) does the
developing. It fixes the failure v1 proved by exhaustion: process built as an
orchestration framework rots faster than it earns its keep. v2 keeps clarity of
inputs and verification of outputs, and lets the harness supply the middle.

## Users

Solo, AI-assisted developers first—someone running Claude Code, Codex, or
another capable host who wants git, secret, and test discipline plus a durable
why-record without standing up a framework. Cross-harness development is a
scaffold design center when it composes vendor-native interactive sessions;
multi-user coordination remains deferred.

## Scope

- One installable binary with the scaffold embedded (rust-embed), offline and
  version-locked; `init`/`update` lay and refresh managed files by ownership
  class.
- A handful of hard gates around git, secrets, and tests, read from
  `.codeflow/policy.json` by every enforcement plane.
- A six-layer knowledge model (product, capabilities, architecture, ADRs, work,
  ledger) maintained inside the ship flow, with `recall` over the record.
- Graduated weight — minimal / standard / full tiers, where the git-discipline
  enforcement floor is the same at every tier and the tiers scale only the
  project-management on top (ADR-0019) — the binary validating every shape so
  growth is mechanical.
- Host-neutral skill contracts for a Claude+Codex duo over every non-trivial
  repository task: stage-aware parallel independent work, capability-routed
  production and review, versioned dual approval, bounded worktree parallelism,
  effective network/tool autonomy, and evidence-based verification through
  each vendor's native interactive harness (ADR-0023, ADR-0024, ADR-0025,
  ADR-0046).

## Non-goals

<!-- The most load-bearing section in this file. Things this project will NOT
     do, stated explicitly so planning can be checked against them. -->

- **Not a runtime harness, agent framework, or model router.** No daemon or
  autorun. Vendor-native sessions, plugins, tools, worktrees, tasks, memory,
  sandbox, and permissions are composed at the process boundary, never
  reimplemented (charter §1; ADR-0023).
- **Not a process-enforcement engine.** No phase ordering, role boundaries, or
  review-before-X sequencing in code; gates exist only where a mistake is
  irreversible or invisible (charter §6.6).
- **Not a general GUI/TUI product or application shell.** The bounded
  `cf-present` review document defined by SPC-004 is the sole interactive
  exception: an explicit CLI action may launch an isolated browser window
  backed by a loopback-only, per-session service that self-terminates. Agents
  author this session's catalog document; the runtime owns chrome and Comment.
  It is not a dashboard, remote server, persistent service, consuming-product
  UI framework, or a clone of the design-exploration board. The opt-in
  documentation portal is a generated static artifact that applies the same
  utility craft to durable source-linked docs, not an operated CodeFlow
  interface.
- **No bespoke memory infrastructure.** No embeddings, vector DBs, GraphRAG, or
  database-as-authority; markdown + JSONL truth with an FTS5 cache (D17).
- **Not a substitute for an OS or harness security boundary.** CodeFlow ships
  macOS, Linux/WSL2, and x86-64 native-Windows binaries plus deterministic
  safety feedback, but it does not claim that every harness provides equal
  containment. In particular, native Windows Claude work that needs an OS
  sandbox moves to WSL2 or a container.
