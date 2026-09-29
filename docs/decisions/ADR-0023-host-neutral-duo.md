---
id: ADR-0023
uid: adca53dd-9a70-4679-b0e2-f62d2beae7bc
title: host-neutral Claude+Codex duo with fixed roles and evidence gates
date: 2026-07-15
status: accepted
superseded_by: null
architecture_impact: scaffold orchestration is host-neutral; doctor now verifies both interactive lane prerequisites; batch workflow naming no longer implies cross-vendor assurance
---

# ADR-0023 — host-neutral Claude+Codex duo

## Context

The prior duo was correct about interactive, cross-vendor work but bound the
entire flow to a Claude Code host. A Codex-hosted task therefore degraded to
solo even though ADR-0018 already allowed Codex to reach an interactive Claude
CLI through tmux. Planning also remained mostly sequential, model versions were
pinned in long-lived doctrine, and the unattended Claude workflow exposed a
`duo` preset that could only perform single-vendor self-critique.

The reverse lane used pane stability as turn completion. A stable terminal is
not a protocol event and pane capture can expose unrelated terminal content.
Claude Code now documents a `Stop` hook that fires after a completed turn and
provides `last_assistant_message`, plus `StopFailure` for API failures. The
official Codex plugin remains the supported Claude Code → Codex integration and
wraps local Codex app-server sessions with review, rescue, transfer, status, and
cancel surfaces.

The local verification record for 2026-07-15 established that Claude Code CLI 2.1.210 was
authenticated and responsive in an isolated interactive TTY even though
`claude auth status --json` reported `loggedIn:false`. Codex CLI 0.144.3,
tmux 3.6a, and `codex@openai-codex` 1.0.6 were present. This is evidence that
status output alone is not an authoritative Claude account probe; each lane
needs a scoped interactive canary. The commands, observations, and portability
boundary are retained in
[`../verification/host-neutral-duo-canary-2026-07-15.md`](../verification/host-neutral-duo-canary-2026-07-15.md).

The implemented adapter was then exercised through a real Sonnet 5 Claude Code
session in task-scoped tmux. Plan mode correctly paused at an interactive
question rather than firing a false completion; after the coordinator answered
that prompt, `Stop` produced the exact `last_assistant_message`, a matching run
id, and a `0600` result inside a `0700` directory, then released only its
matching tmux waiter. This confirms that a plan dialog is input, not a terminal
event, and that the hook is the completion authority.

## Decision

Make `cf-model-orchestrator` host-neutral and capability-gated:

- Claude Code hosts through the official `codex-plugin-cc` lane.
- Codex App or interactive Codex CLI hosts through a task-scoped interactive
  Claude CLI in tmux.
- Another host, including Hermes, may coordinate only when it can attach to
  both sanctioned native interactive sessions while preserving their tools,
  MCP servers, approvals, and resumability.
- Headless work sessions remain prohibited: no `codex exec`, `claude -p`,
  direct hand-rolled app-server driver, or desktop-GUI automation protocol.

Roles do not reverse with the host. Both models independently research,
analyze, identify risks, and plan from the same immutable brief. Claude leads
design and normally compares two or three options; Codex challenges
feasibility, security, operations, and testability. The host reconciles a
versioned plan that both explicitly approve. Codex implements and performs
first verification. Claude independently reviews the diff, reruns relevant
tests, and checks design/UI conformance. Plan and rework loops are each bounded
to two rounds before human escalation.

Add a shared quality resource under the orchestrator skill. It requires a
versioned plan contract, reproducible evidence ledger, unit/integration/e2e and
security checks as applicable, UI-driven verification, and scenario-first
coverage. Where line coverage is supported, production code has an 80% hard
floor and a normal 90% target. Agreement never substitutes for evidence or
turns a red deterministic gate green.

Harden the reverse lane by using task-scoped `Stop` and `StopFailure` hooks
as completion signals, with a unique run id, owner-only status path, bounded
wait, and `last_assistant_message`. Pane capture is limited to the dedicated
task pane after completion or bounded failure diagnosis. Transcript parsing and
visual pane stability are not completion signals. The binary implements the
adapter as `codeflow hook delegate-turn`: it validates the run id and absolute
private result directory, creates a `0600` result exactly once, preserves only
the terminal event's structured fields, and directly signals the scoped tmux
channel without a shell parser dependency. An exact retry may re-signal without
rewriting evidence; a conflicting retry is rejected.
Read-only consults additionally start Claude with `--permission-mode plan`;
deployments needing an OS-level Bash boundary enable Claude's sandbox with
`sandbox.failIfUnavailable: true`. Prompt scope and post-run diff review remain
defense in depth. Plan-mode `AskUserQuestion`/`ExitPlanMode` dialogs are handled
interactively in the dedicated pane and never mistaken for completion.

The `delegates` doctor check now verifies inspectable prerequisites for both
directions: Codex presence/auth/MCP, Claude presence/MCP, the enabled official
plugin, and tmux. It explicitly requires retained interactive canaries because
doctor cannot safely prove a live account/tool session. The unattended workflow
renames its assurance preset to `single-vendor-assurance` and rejects the old
`duo` semantics.

Model selection is version-neutral doctrine: use the strongest supported model
and high reasoning available to each seat, then record the actual selection as
run evidence. Fast-aging model names belong in user configuration, not the
shared skill.

## Consequences

- Claude-hosted and Codex-hosted work follow the same roles, gates, and evidence
  requirements; only the transport/coordinator changes.
- Research and planning gain independent parallel input before either model can
  anchor the other, while a versioned dual approval prevents stale consent.
- Agent OS and other consumers can adopt one behavioral contract without
  pretending byte-identical skill mirrors alone prove harness portability.
- Interactive canaries remain an operational requirement. Doctor can prove
  installed prerequisites but cannot prove a native session's authentication,
  tool permissions, or MCP behavior.
- The reverse lane is safer and more deterministic but depends on task-scoped
  Claude hook configuration. A missing or failed hook blocks the lane instead
  of falling back to terminal scraping.
- Batch workflows remain useful for single-vendor automation but cannot claim
  cross-vendor design, execution, or review assurance.

## Architecture impact

The scaffold now carries a harness-neutral orchestration/quality contract and
two explicit interactive adapters. The doctor support area inspects both lane
prerequisites, while the user-owned Claude workflow is explicitly
single-vendor. The core binary owns only the deterministic terminal-event
adapter; it still does not route or run models. Harnesses execute the skill,
and deterministic Codeflow gates remain author-agnostic.

## Sources

- Claude Code hooks: <https://code.claude.com/docs/en/hooks>
- Claude Code permission modes and sandbox:
  <https://code.claude.com/docs/en/permission-modes>,
  <https://code.claude.com/docs/en/sandboxing>
- Official Codex plugin for Claude Code:
  <https://github.com/openai/codex-plugin-cc/blob/main/README.md>
- Codex app-server: <https://developers.openai.com/codex/app-server>

## Note, 2026-09-29: ADR-0076 supersedes two clauses

ADR-0076 (one PR per task and planning once per epic) is accepted. It
supersedes two clauses of this record and no other; the roles, the evidence
gates and the rest stay accepted.

- "Plan and rework loops are each bounded to two rounds before human
  escalation." Superseded: there is no round cap. Review ends on evidence:
  continue while repairs produce relevant evidence, and diagnose a stalled
  mechanism, an invalid assumption or a materially changed scope.
- "Where line coverage is supported, production code has an 80% hard floor
  and a normal 90% target." Superseded: the coverage floor is the
  project's configured gate (CodeFlow's is `--fail-under-lines 90`), not
  prose.
