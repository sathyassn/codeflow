---
id: ADR-0018
uid: 20050cfd-8599-4daf-8a12-45df8beceafb
title: interactive-only cross-model transport — one lane per direction
date: 2026-07-11
status: accepted
superseded_by: ADR-0023
architecture_impact: none — the rule rebinds existing skill doctrine (cf-consult, cf-delegate, cf-model-orchestrator, cf-customize) and deletes one skill resource; no engine module or boundary moves
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0018 — interactive-only cross-model transport

## Context

Cross-model work (consult, delegate, and the duo develop loop) had accreted
four transports across the skills: headless `codex exec` (ADR-0005's verified
shape), the official `codex-plugin-cc` plugin, direct codex app-server JSON-RPC
driving, and tmux screen-driving of the codex TUI (ADR-0015's driver ladder) —
with any seat free to pick any rung. That surface is wide to verify and easy to
misuse: a headless session fires no in-session guards (ADR-0008 measured that
`codex exec` runs no project PreToolUse hooks), carries no full MCP toolset
(no Playwright e2e), and resumes nothing a follow-up can audit in place; a
hand-rolled app-server driver chases an experimental API the vendor already
wraps; and after a mixed-lane run, "which transport did this pass use" has no
cheap answer. The portability review compounded the case: the same skills
assumed a Claude seat throughout, so a codex-primary session was steered into
consulting its own vendor and labeling the result cross-vendor.

## Decision

Cross-model transport is **interactive-only, exactly one lane per direction**:

```
Claude Code ──codex-plugin-cc plugin──▶ codex
codex ──tmux-driven interactive claude CLI──▶ claude
```

- **Claude Code → codex: ONLY through the official `codex-plugin-cc` plugin**,
  which wraps the codex app-server — the interactive engine, full MCP toolset
  (Playwright included), a resumable thread, and in-band approvals.
- **codex → claude: ONLY the interactive `claude` CLI driven via tmux**
  (`send-keys` / `capture-pane` with output-stability polling).
- **Prohibited at all times**, batch and pipeline stages included: headless
  task execution in either direction (`codex exec`, `claude -p` / `--print`),
  direct app-server driving (hand-rolled JSON-RPC), and Claude → codex via
  tmux. No exceptions.
- **Not prohibited** — they are commands, not work sessions: `codex login
  status`, `codex --version`, `codex mcp list`, and plugin install/setup steps.

Verification status, stated plainly. The plugin lane's tool access is
empirically verified (2026-07-11, codex-cli 0.144.1): a delegated
`/codex:rescue` task reports codex's full MCP toolset — the `playwright` server
with its 24 browser tools — in a resumable multi-turn thread. The codex →
claude lane rests on standard tmux mechanics, but a live round-trip against the
interactive `claude` CLI has **not** been verified here — VERIFY-ON-INSTALL:
run one scoped read-only consult end to end before relying on that lane. The
interactive claude TUI has no sandbox flag, so a consult's read-only posture is
by instruction in the prompt, not enforced.

## Consequences

- One lane per direction is simpler to verify and harder to misuse: preflight
  checks one surface per seat, and any `codex exec` / `claude -p` in a
  transcript is a violation on sight, not a judgment call.
- Every cross-model session runs where the guards live: interactive sessions
  fire the in-session hooks (Claude always; interactive codex after the
  one-time `/hooks` trust, ADR-0008), keep the full MCP toolset, and leave a
  resumable thread for follow-up rounds.
- Headless `codex exec` is no longer a sanctioned worker shape: ADR-0005's
  headless composition boundary is superseded for cross-model work (its
  process-boundary principle, auth doctrine, and author-agnostic gates carry
  over unchanged). ADR-0015's driver ladder collapses to plugin-only for
  Claude → codex; the `codex-app-server-driver` skill resource is deleted from
  the scaffold.
- Antigravity `agy` drops out as a delegate tier: its only documented drive
  shape is headless one-shot CLI invocation, which this rule prohibits.
- Costs, honestly: the codex → claude lane has no structured-output path
  (prose over a TTY — synthesize, never parse) and is unverified until first
  install; a seat missing its lane's tooling loses cross-model work entirely
  and must degrade to solo, legibly; batch pipelines cannot fan work out to a
  headless second vendor anymore — the duo runs interactively or not at all.

## Architecture impact

None — the rule rebinds existing skill doctrine (cf-consult, cf-delegate,
cf-model-orchestrator, cf-customize) and deletes one skill resource; no engine
module or boundary moves.
