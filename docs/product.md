# codeflow — product

<!-- WHY layer. Human-owned: agents propose changes here, the human accepts.
     Keep it small and stable — this file should change rarely.
     `codeflow doctor` flags epics whose scope violates the non-goals below. -->

## Purpose

CodeFlow is the AI-development discipline layer you install into any repo: one
Rust binary (`codeflow`) plus an embedded scaffold that scaffolds, enforces,
verifies, and remembers — while Claude Code (or any harness) does the
developing. It fixes the failure v1 proved by exhaustion: process built as an
orchestration framework rots faster than it earns its keep. v2 keeps clarity of
inputs and verification of outputs, and lets the harness supply the middle.

## Users

Solo, AI-assisted developers first — someone running Claude Code (or Codex,
Cursor) who wants git, secret, and test discipline plus a durable why-record
without standing up a framework. Built for the actual first user, not a
speculative team (charter D18); multi-user and cross-harness support are
consumed-native or deferred, never the design center.

## Scope

- One installable binary with the scaffold embedded (rust-embed), offline and
  version-locked; `init`/`update` lay and refresh managed files by ownership
  class.
- A handful of hard gates around git, secrets, and tests, read from
  `.codeflow/policy.json` by every enforcement plane.
- A six-layer knowledge model (product, capabilities, architecture, ADRs, work,
  ledger) maintained inside the ship flow, with `recall` over the record.
- Graduated weight — minimal / standard / full tiers — the binary validating
  every shape so growth is mechanical.

## Non-goals

<!-- The most load-bearing section in this file. Things this project will NOT
     do, stated explicitly so planning can be checked against them. -->

- **Not a harness, agent framework, or orchestrator.** No model router, no
  daemon, no autorun. Claude Code's native primitives — subagents, workflows,
  worktrees, tasks, memory, sandbox, permissions — are consumed, never
  reimplemented (charter §1).
- **Not a process-enforcement engine.** No phase ordering, role boundaries, or
  review-before-X sequencing in code; gates exist only where a mistake is
  irreversible or invisible (charter §6.6).
- **Not a GUI or TUI.** Command-line and harness-native surfaces only.
- **No bespoke memory infrastructure.** No embeddings, vector DBs, GraphRAG, or
  database-as-authority; markdown + JSONL truth with an FTS5 cache (D17).
