---
id: ADR-0054
uid: 8c2e907b-5ea1-4b48-acd1-fb16b43375e6
title: Grok as first-class host and catalog family; standing pair remains the quality floor
date: 2026-09-01
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — Grok Build joins Claude Code and Codex as a first-class host; the ensemble splits standing pair from catalog families; extra-family review is named, never a silent third vote
---

# ADR-0054: Grok as first-class host and catalog family

## Context

CodeFlow's host-neutral duo (ADR-0023) treated Claude Code and Codex as
first-class hosts and every other harness, including Hermes and Grok Build, as
an outer coordinator that should usually delegate the whole repository task to
one of those two. ADR-0039 additionally refused to predeclare an untested Grok
binding: protocol presence is not a native capability contract.

Grok Build is now a native interactive coding harness with its own TTY,
session identity, tools, MCP, permission modes, sandbox profiles, and git
worktree support. Treating it as "another harness, including Hermes" understates
the seat: the operator can start CodeFlow from `grok` and expect a real
engineering and orchestration host, not a consult-only relay. Locking the
product to exactly two families forever is also the wrong shape. Two primaries
remain the usual quality floor; extra families exist for named production or
review when policy or the operator requires them.

Host is not duty. Whoever starts the session coordinates. Claude still
*produces* design in its own native interactive session. A Grok or Codex host
may pass options and challenge feasibility; it never drafts design for Claude
to rubber-stamp. Extra-family output is evidence, never a silent third vote.

The Codex plugin remains Claude-Code-only. A Grok host reaches Claude and Codex
through Herdr (schema-v2 still owns Claude turn completion). Those Grok-hosted
lanes are not claimed complete until their native canaries exist.

## Decision

1. **Host matrix.** Interactive Grok Build (`grok` CLI) is a first-class
   CodeFlow host alongside Claude Code and Codex. Hermes and other
   non-catalog harnesses still delegate the whole repository task to one
   native host unless both lanes and the full contract are proven.
2. **Standing pair vs catalog.** `current-ensemble.json` schema 3 names a
   standing pair — `claude-judgment-primary` and `codex-engineering-primary` —
   as the usual quality floor, and may list additional catalog bindings. The
   current catalog seat is `grok-engineering-primary` on `grok-cli` /
   `grok-4.6` at high, with xhigh only on a documented trigger. Extra families
   are not locked to one forever; adding another family still requires catalog
   capability evidence and a qualified concrete binding.
3. **Internal routes are CodeFlow instructions.** Grok's `internal_routes`
   tell the Grok primary which selector and effort to keep. They are not a
   claim about vendor-secret routers. The same rule already held for Claude
   and Codex internals.
4. **Design production.** The Claude judgment primary produces design in its
   native interactive session regardless of host. `cf-design` remains the
   method. Other seats review; they do not author the direction for Claude to
   accept.
5. **Extra-family review.** `routing-policy.json` defaults to the standing
   pair. All-qualified or named extra-family review happens only on a
   documented trigger or explicit operator instruction. The extra seat is
   assigned in Plan vN; its output is recorded evidence. It never replaces
   either standing primary or silently outvotes them.
6. **In-session hook plane.** Scaffold `.grok/hooks/codeflow.json` with the
   same PreToolUse payload as `.codex/hooks.json` (git-guard, exec-guard,
   session-orient). Doctor `grok` reports structural wiring and the one-time
   `/hooks-trust` step. Git hooks and CI remain the floor. Grok-as-host still
   uses Claude schema-v2 for Claude turns; there is no second Grok Stop-hook
   adapter.
7. **Harness catalog.** `grok-cli` is `capability-supported` from native Grok
   Build evidence (interactive TTY, session UUID provenance, tools/MCP,
   `--cwd`/`--worktree`, session resume/export, `--permission-mode auto` as
   the ADR-shaped default, `--always-approve` as the unattended overlay,
   `--sandbox`, repository git backstop). Catalog status is not model
   qualification. Headless `grok -p` / `--single` is forbidden for work
   sessions, matching `claude -p` and `codex exec`.
8. **Doctor.** `grok-cli-version` is a code-allowlisted probe of
   `grok --version`. A catalog edit cannot invent a new executable. Doctor
   `grok` reports in-session hook wiring separately from Claude↔Codex
   `delegates`.
9. **Honest incompleteness.** Do not report a Grok-hosted duo as complete
   until (a) a Grok-started Claude schema-v2 canary and (b) a Grok-started
   Codex Herdr lifecycle canary exist. Until then, Grok may host, consult, and
   take named catalog assignments; missing-lane degradation stays legible.

## Consequences

- Operators can start the same CodeFlow contract from Grok Build without
  pretending Grok is only a consult.
- The standing Claude+Codex pair remains the default dual-approval floor, so
  quality does not silently become a three-way vote.
- Catalog families can grow without rewriting durable doctrine for every
  vendor.
- Grok-hosted Claude and Codex transports stay Herdr/TTY; the official Codex
  plugin is not a Grok-host lane.
- A later family still pays the catalog-plus-qualification cost. Presence in
  marketing or a protocol list is not enough.

## Note (2026-09-05)

ADR-0055: Grok reaches Codex through the official `codex` CLI and local
app-server daemon (Herdr, tmux degraded), not a third-party Grok plugin. The
Claude-Code `codex-plugin-cc` remains Claude-host-only; a missing plugin
degrades to solo and does not open a Claude→Codex Herdr CLI lane. Decision 2's
"at high" is superseded: default effort is medium. Production Grok launch is
`--always-approve`.

## Rejected

- Replacing the standing pair with a Grok+Claude or Grok+Codex default.
- Treating Grok as consult-only while calling it first-class.
- Encoding vendor-internal Grok routers as CodeFlow policy.
- Claiming the Grok-hosted duo complete from CLI flags and Herdr kind names
  alone.
- Letting extra-family output silently settle Plan vN or closeout.

## Note (2026-09-06)

This note amends Decision 5. Extra-family review is invoke-when-warranted,
not a two-model cap. When a documented trigger fires (architecture, material
technical depth, security, unclear pair, consequential work, high/xhigh
complexity) and the catalog family is available, the host names it on Plan
vN. Output remains evidence; it still never silently settles Plan vN.

## Note (2026-09-07)

Decision 9 canaries exist:
`docs/verification/grok-host-duo-canary-2026-09-07.md`. Grok-started Claude
schema-v2 (Opus medium after Fable 429; Fable was not the answering seat)
and Grok-started Codex Herdr (`gpt-6-astra` medium). Catalog Grok is still
not a promoted qualified binding.
---

## Note (2026-09-25)

Item 2 is restated by ADR-0069. The managed catalog is schema 5; the standing
seats are `claude-primary` and `codex-primary`; seat `grok-primary` serves its
product line, whose current version is in ADR-0069's roster. The version named
in item 2 is retired. Grok joins a review as a triggered participant on the
routing-policy triggers, unchanged until the operator answers Q4.

## Note (2026-10-03)

ADR-0077 supersedes the 2026-09-05 Note's sentence that a missing plugin
degrades to solo and does not open a Claude to Codex Herdr CLI lane. Every
host, a Claude Code host included, reaches Codex through the interactive
Codex CLI in a Herdr tab; the Claude Code plugin is an optional fallback
and still not a Grok-host lane; tmux is the last fallback.
