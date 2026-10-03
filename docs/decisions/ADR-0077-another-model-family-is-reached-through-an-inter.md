---
id: ADR-0077
uid: 46deb2fd-7fa7-4c80-93b6-c802c988783c
title: "Another model family is reached through an interactive seat in a Herdr tab"
date: 2026-10-03
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: "`docs/architecture.md`: the doctor `delegates` check reports Herdr, with tmux as the last fallback, and reports the Codex plugin as an optional fallback instead of a prerequisite. Updated in the PR that records this decision (TSK-213)."
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. -->

# ADR-0077: Another model family is reached through an interactive seat in a Herdr tab

## Context

CodeFlow's skills gave hosts different answers to one question, how one
model family calls another (issues 30 and 31). The orchestrator's preflight
preferred the Codex app-server, while its seat table, cf-delegate,
cf-consult, cf-herdr, the CLAUDE.md row and the exec-guard refusal preferred
the official Codex plugin (ADR-0059), and cf-herdr drove Herdr only when
`HERDR_ENV=1`, so a Claude Code session outside a Herdr pane fell back to
tmux even with a Herdr server running. In practice that session drove Codex
and Grok seats in Herdr tabs correctly, and those seats showed the model and
effort that ran, which the plugin route never exposed. The operator decided
the rule on 2026-10-03.

## Decision

Another family is reached through an interactive seat, its own CLI in a
Herdr tab: Codex through the interactive Codex CLI on the local Codex
app-server, else that CLI on its own; Claude through the interactive Claude
Code CLI, its turns tracked by the `codeflow delegate` lifecycle; Grok
through the interactive Grok Build CLI. The same family runs inside the
caller's own harness as native subagents. Seats are always interactive:
never `codex exec`, `claude -p` / `--print` or `grok -p` / `--single`. The
official Codex plugin is an optional fallback on a Claude Code host, never
the preference, and tmux is the last fallback, only when no Herdr server is
reachable. Any host may drive any reachable Herdr server, inside or outside
a Herdr pane, under cf-herdr's anti-hijack rules: its own tabs only, unique
`cf-` names, a `cwd` check, never the operator's panes. Each seat starts in
its CLI's autonomous permission mode (Claude `bypassPermissions` for
production and building and `auto` for consults and reviews; Codex
`--ask-for-approval never`, building adding ADR-0075 D1's sandbox flag and
consults leaving `--sandbox` off; Grok `--always-approve` for production and
`--permission-mode auto` for consults). A new seat's folder trust prompt is
answered by the caller for the task's own folder only, its hook trust prompt
stays the operator's, and a self-update offer is skipped. The seat's own
Codex session record may be read for a long reply and for the observed
model and effort of that one session, never as a completion signal. The
rule has one home, `cf-model-orchestrator/resources/routing/transport.md`,
which every other skill and managed instruction cites.

## Consequences

- Every host gets the same answer, and an adopting project reads it in one
  file.
- Seats are visible in Herdr and report observed model and effort, so
  provenance moves from requested to observed on the default route.
- A Claude Code host needs Herdr (or tmux) and the Codex CLI for its Codex
  seat; the plugin is no longer the default and doctor stops reporting its
  absence as a gap.
- From a sandboxed host the Herdr socket sits outside the sandbox, so
  `herdr` runs through the session's permitted retry for a trusted
  installed tool.
- The Codex builder posture is unchanged: ADR-0075 D1 keeps full access
  until its spike passes. A per-project builder setting was considered and
  not adopted, since D1 binds the posture to that spike and the project
  model-selection file never owns commands.

## Architecture impact

`docs/architecture.md`: the doctor `delegates` check reports Herdr, with tmux
as the last fallback, and the Codex plugin as an optional fallback instead
of a prerequisite.
