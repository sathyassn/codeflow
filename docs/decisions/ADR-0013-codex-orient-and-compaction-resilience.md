---
id: ADR-0013
title: Codex gets the orient digest (SessionStart); compaction resilience is a disposition, not a snapshot
date: 2026-07-05
status: accepted
superseded_by: null
architecture_impact: hook-plane paragraph notes session-orient binds an interactive Codex session via SessionStart
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0013: Codex gets the orient digest; compaction resilience is a disposition, not a snapshot

## Context

ADR-0008 established harness parity for the in-session *guard* plane: the same
`git-guard`/`exec-guard` binaries bind an interactive Codex session through a
byte-compatible PreToolUse payload. But `session-orient` and `session-summary`
stayed Claude-only, so a Codex session opened with no orientation digest — the
single largest remaining cross-harness gap. Separately, "how does a model recover
after a context compaction?" needed an answer that holds across harnesses, not
one that depends on a hook a given harness may not fire.

## Decision

Wire `session-orient` onto Codex `SessionStart` (matcher `startup|resume|clear|
compact`) in `.codex/hooks.json`, reusing the existing handler: it prints a
plain-text digest to stdout, which both Claude and Codex inject as session
context (per Codex's documented hooks contract), so no handler is duplicated.
Because SessionStart re-fires with `source=compact` after a compaction, orient is
also the post-compaction re-orientation on both harnesses.

Compaction resilience is owned by an AGENTS.md disposition ("Externalize state as
you go"), not a hook: the durable record — ADRs, `project-management/` status, the
harness's own memory — is what survives, and `codeflow orient` + `recall` rebuild
from it. Two hooks are deliberately NOT added:

- `session-summary` on Codex — Codex has no clean session-end event; `Stop` is
  turn-scoped and would append a session record every turn.
- `PreCompact → session-summary` on either harness — `session-summary` captures
  only git-derived state (branch, changed-file and commit counts), which orient
  re-derives live on the `source=compact` re-orient; it holds none of the volatile
  conversation. It would add a ledger event type (routing and compaction both key
  on `session_end`) for no recovery gain.

## Consequences

- Interactive Codex sessions open with, and re-orient after compaction from, the
  same digest Claude gets — one handler, config-only wiring. Headless `codex exec`
  (0.142.5) does not fire project hooks (ADR-0008), so this is an interactive-
  session aid; headless Codex still leans on the git-hook plane + CI.
- The wiring is pinned by `crates/codeflow-cli/tests/codex_hooks.rs` — structural:
  it asserts the shipped JSON, not a live Codex run. Live firing rests on Codex's
  documented contract, not a runtime test here.
- Codex sessions contribute nothing to recall's *session* corpus (no session_end,
  no PreCompact record). Acceptable: that record is only git counts,
  reconstructable from history.
- Compaction recovery is harness-agnostic (a disposition), degrading gracefully
  where a harness lacks the hooks — the deliberate trade for portability.

## Architecture impact

`docs/architecture.md` hook-plane paragraph notes `session-orient` now binds an
interactive Codex session via SessionStart (alongside the guards).
