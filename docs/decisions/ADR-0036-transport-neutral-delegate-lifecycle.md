---
id: ADR-0036
uid: fa81c09a-4b6d-47fb-a948-bf3080dfa11c
title: make delegate turns a durable transport-neutral lifecycle
date: 2026-07-23
status: accepted
superseded_by: null
architecture_impact: engine gains the schema-v2 delegate lifecycle (delegate.rs state machine, the delegate CLI, a dual-mode delegate-turn hook, and a Fail-severity delegate-roundtrip doctor check); the binary still never launches a harness
---

# ADR-0036: transport-neutral durable delegate lifecycle

## Context

ADR-0023 fixed cross-vendor delegation to interactive-only lanes and gave the
reverse (Codex→Claude) lane a one-shot completion adapter: `codeflow hook
delegate-turn --result` writes owner-only terminal evidence and signals a
task-scoped tmux waiter. That adapter observes only the end of a turn. It
cannot tell the host that the harness started cleanly, that the delivered
prompt was accepted as the armed turn rather than swallowed, altered, or
duplicated, or that a terminal event belongs to the turn the host believes is
outstanding — and its signalling is welded to tmux, so no tmux-free lane can
reuse it. Workstream A — settled 2026-07-23 with both seats' approval and
recorded durably as EPC-002 and SPC-002 — required a lifecycle any host can
drive: durable correlation from
startup through acceptance to terminal, ambiguity treated as poison rather
than guessed through, and no new harness-launching responsibility inside the
binary.

## Decision

CodeFlow adds a schema-v2, file-based delegate lifecycle beside the legacy
adapter. The division of labor is strict: **the binary never launches a
harness and never delivers a prompt.** The host launches the delegated harness
and transmits text however its lane provides — the current reverse lane still
pastes through tmux, purely as a host-side mechanic — while CodeFlow owns only
durable protocol records and their observation. The current event adapter is
Claude hooks: `delegate init` generates task-scoped settings that wire
SessionStart, UserPromptSubmit, Stop, and StopFailure to `codeflow hook
delegate-turn --state-dir`. Schema-v2 waiting is pure file polling — zero tmux
calls anywhere, including exact retries — while the legacy `--result` mode
keeps its byte-compatible record-and-signal contract until a later major
release removes it.

The invariants, each enforced in code rather than convention:

- **Owner-only state outside Git worktrees.** The state directory is an
  absolute UTF-8 path with `0700` directories and `0600` files, ownership and
  mode revalidated on every open, and rejected if any ancestor contains a
  `.git` entry — protocol records never become repository content.
- **Exact reuse of the init path string.** `init` writes `settings.json` from
  exactly (run_id, state-dir string); every later `arm`, `wait`, and hook
  invocation regenerates and compares it. A different run id — or the same
  directory reached through a different path spelling — fails as unsafe, so
  records can never be bound to a run they were not created for.
- **Bounded locking.** Every transition serializes on one `.protocol.lock`
  with a bounded (one-second) exclusive acquisition; a held lock is a legible
  error, never an indefinite stall.
- **One outstanding armed turn, digest-bound.** `arm` records the SHA-256 of
  the exact prompt bytes and refuses while another turn is outstanding.
  Acceptance requires a `UserPromptSubmit` whose prompt matches that digest
  and whose session matches the startup record; anything else is rejected
  (exit 2) so the harness blocks the submission.
- **Deterministic terminal binding.** A terminal event binds in one order:
  exact retry of an existing result (idempotent, no state change) → the sole
  accepted-without-result turn, with session and `prompt_id` equality
  enforced → otherwise poison. A prior-result match observed while a
  different turn is accepted is ambiguous and poisons unless `prompt_id`
  equality proves the retry.
- **Durable poisoning, fail closed.** A non-`startup` SessionStart source
  (resume, clear, compact, fork), a conflicting startup, a mis-correlated or
  ambiguous terminal event, or an interrupted wait after acceptance writes a
  write-once `poison.json`. Poison precedes every success path, `arm` refuses
  a poisoned run, and recovery is a new run in a fresh state directory —
  records are never edited.
- **`prompt_id` correlation with a pinned compat path.** `prompt_id` is a
  bounded UUID validated wherever present and must agree across acceptance
  and terminal records. Its absence is permitted only on the recorded
  pre-2.1.196 compatibility path, which is limited to a single turn per
  session; arming a second turn there is refused.
- **Interactive dialogs fail closed.** If live canary evidence shows in-turn
  UI answers (AskUserQuestion, permission responses) traverse
  UserPromptSubmit, the guard's rejection rules stand unchanged and delegated
  turns requiring interactive dialogs are unsupported on this lane until a
  distinct one-shot correlation mechanism is designed and reviewed. The
  one-outstanding-turn invariant is never relaxed.
- **Platform boundary.** Native Windows is unsupported for delegate state and
  fails closed with a message directing to WSL2; Unix permission semantics
  are load-bearing, not decorative.
- **Privacy and removal.** Records carry digests, identifiers, and the
  bounded terminal payload — never the prompt text; sizes are capped
  (1 MiB prompt, 4 MiB message, 1 MiB error), and raw schema-v2 hook input is
  capped at 32 MiB before parsing. An unreadable or oversized schema-v2 input
  fails closed without a record. The host removes the state directory after
  consuming the result.

`codeflow doctor` gains a deterministic `delegate-roundtrip` check that drives
the installed binary through the full synthetic lifecycle at Fail severity.
The dated PR1 canary record verifies the effective Stop-hook set, one live
Unicode prompt, `Stop.prompt_id` binding across three turns, and both
AskUserQuestion and permission-response routing on Claude Code 2.1.218 for the
available macOS arm64 host. The reusable sibling-hook rejection procedure,
the full normalization/fake-TUI stress matrix, and broader native-platform
evidence remain PR2 and release gates, not delivered facts.

## Rejected options

1. **Extend the tmux signal protocol with per-stage signals.** Cheapest
   incrementally, but it deepens the tmux dependency this decision exists to
   remove, and signals are ephemeral — a crashed waiter loses the very
   evidence a durable protocol must preserve.
2. **Let the binary launch and manage the delegated harness.** One process
   would own the whole turn, but CodeFlow is deliberately not a harness or
   model runtime (charter non-goal; ADR-0023): launching implies owning auth,
   PTYs, and lifecycle supervision per vendor, and would migrate host policy
   into the engine.
3. **Parse harness terminal output instead of hook events.** Transport-neutral
   on its face, but screen-scraping is version-fragile and cannot prove
   correlation; the hooks contract is the documented, stable event source.

## Consequences

- Any host that can launch a harness, deliver bytes, and surface a
  compatible event adapter can now drive a delegate turn with durable,
  verifiable state — Claude hooks is the current adapter — and the tmux lane
  becomes one delivery choice among possible others rather than the protocol
  itself.
- Ambiguity is never guessed through: restarts, mis-correlated terminal
  events, and ambiguous delayed retries end in a legible poison instead of a
  silently misattributed result, while a duplicate or digest-mismatched
  prompt submission is blocked outright with run state preserved. The cost
  is deliberate friction — a poisoned run must be restarted.
- The legacy `--result` mode carries a dual-mode hook surface until its
  removal at a later major release.
- The lifecycle is only as trustworthy as its event adapter; the PR1/PR2
  canaries are recorded in
  `docs/verification/delegate-lifecycle-canary-2026-07-23.md`, while the
  reusable sibling-Stop rejection procedure, full stress matrix, and native
  platform evidence must land before the lane is claimed release-verified.

## Architecture impact

Engine gains `crates/codeflow-core/src/delegate.rs` (the schema-v2 state
machine), the `codeflow delegate init|arm|wait` CLI, the dual-mode
`delegate-turn` hook, and the `delegate-roundtrip` doctor check;
`docs/architecture.md` updated in this PR. The binary still never launches a
harness; host delivery remains outside CodeFlow.
