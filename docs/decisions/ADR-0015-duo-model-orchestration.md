---
id: ADR-0015
title: duo-model orchestration — the Claude+codex develop flow (cf-model-orchestrator)
date: 2026-07-10
status: accepted
superseded_by: null
architecture_impact: none — the duo flow ships as the `cf-model-orchestrator` skill, an opt-in `duo` pipeline preset, and the codex-app-server driver doctrine (a skill resource), all outside the core engine; the only architecture.md text in this batch is ADR-0016's security-review plane
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0015 — duo-model orchestration for the develop flow

## Context

A codeflow-consuming team runs Claude Code as the primary orchestrator and codex
as a second dev harness, and wants a develop loop where the two models
co-produce work: Claude plans, a second independently-trained model reviews that
plan and then executes and first-tests it (including UI-driven end-to-end),
Claude does the final verification grading every acceptance criterion, and both
iterate under a bounded fix loop until all criteria, hard gates, and
project-specific requirements pass — a must before push/PR. The existing surface
does not cover this. `cf-develop` is a solo build → review → verify loop with a
single `cf-reviewer` pass, and self-review — even by a fresh session of the same
model — shares correlated blind spots, so a genuinely independent second vendor
adds real signal, but only when the blast radius justifies the tax. ADR-0005
already composes codex at the process boundary, but only **headlessly** —
read-only consult or a full edit handoff via `codex exec` — and headless
`codex exec` cannot use codex's full MCP toolset (Playwright / computer-use) that
UI-driven e2e requires, nor does it fire codeflow's PreToolUse hooks (ADR-0008).
The duo develop loop is therefore a new case ADR-0005's headless boundary did not
cover: a multi-round, interactive, full-tool codex session that co-develops and
tests, with the two models negotiating to agreement rather than a single pass.

## Decision

Add `cf-model-orchestrator`, a skill that decides **solo vs duo** for planned dev
work and drives the chosen shape. Duo is Claude-orchestrated: Claude plans with
explicit acceptance criteria (happy path and edge/error cases); codex reviews the
plan and, on agreement, executes it and runs the first round of testing including
UI-driven e2e; Claude does the final verification, grading every acceptance
criterion with evidence; and the two iterate a bounded fix loop (≤2 rounds to
plan-agreement, then a human tiebreak; the build loop keeps `cf-develop`'s
max-3-rework bound). The acceptance criteria are the contract both models agree
to at plan-align and the rubric at execute-test and final-verify. The stage set
is a **named, opt-in pipeline preset** (plan-align → codex-executor build →
Claude final-verify, with the mandatory security / red-team stage of ADR-0016) —
never the default, so trivial work pays no duo tax.

**Driver.** Duo drives codex through the **codex app-server** (JSON-RPC/JSONL
over stdio) as the primary path, not headless `codex exec`: it is the same
interactive engine as the TUI, with the full MCP toolset (Playwright verified,
24 tools), resumable context-retaining sessions (`thread/resume` loads a thread
from disk by id, surviving process restarts — a superset of `codex exec
resume`), a deterministic `turn/completed` signal, a deterministic MCP precheck,
and code-answerable approvals so it never wedges. The v2 thread/turn API is
experimental (gated behind the `experimentalApi` capability, emitted only under
`--experimental`), so the codex version is pinned and the generated contract is
regenerated and diffed on each upgrade, wired to the harness-parity canary; a
single process owns one thread at a time and the model is kept stable across
resumes. A tmux driver (config `notify` hook → `tmux wait-for`, `capture-pane`
polling with an md5 stability hash) is the documented fallback. Each side runs
its vendor's latest frontier model at high reasoning effort (as of 2026-07:
Claude Fable 5 at high/xhigh, codex GPT-5.6 Sol at xhigh), recorded as a
principle plus dated pins tied to the parity canary so they do not silently rot.

**Auto-trigger and degradation.** Selection is via the skill's frontmatter
`description` alone (the only auto-trigger mechanism), which carries the
solo-vs-duo criteria and the safe-fallback rule as the single source of truth;
the cf-plan and cf-method weave and the AGENTS.md entry-points row point at it
without restating the criteria. When codex is missing or unauthenticated at flow
start, the duo **silently degrades to solo `/cf-develop`** — duo was never
promised, so it never blocks or nags for auth (the inverse of user-requested
`cf-consult`/`cf-delegate`, which stop and tell the user to `codex login`). codex
dying mid-duo, or missing for a *requested* duo security stage, blocks loudly
(ADR-0016), since losing the second vendor defeats the correlated-blindspot
reduction that justifies it.

**Gate.** The duo verdict plus the deterministic gate is a must before push/PR,
but the authoritative perimeter is unchanged: server-side CI plus a human-merged
PR (ADR-0006), not a new local hook level. Non-deterministic model verdicts never
hard-block CI (a flaky hard gate creates bypass pressure); the deterministic
gate (tests, `validate`, the ADR-0016 scanner floor) is authoritative, and the
human merger is the backstop for judgment a machine cannot adjudicate. No model
talks a red gate green.

## Consequences

- Higher-stakes planned work gets a genuinely independent second-vendor reviewer
  and executor with full UI-e2e reach, at a cost paid only when the skill's
  description matches real blast radius — trivial and docs-only work is untouched.
- The app-server driver is a second, interactive codex-composition mode alongside
  ADR-0005's headless boundary; it adds an experimental-API surface that must be
  version-pinned and contract-checked per upgrade (parity canary), and it depends
  on codex's MCP configuration (Playwright present for web e2e) being healthy.
- Degradation is legible by design: solo is always a safe fallback, so a machine
  without codex loses the duo benefit but never stalls; a duo that loses codex
  mid-flight fails loudly rather than silently dropping the second lens.
- The duo preset lives in the user-owned `pipeline.workflow.js` (ADR-0004), so
  its "mandatory" stages are convenience in-session; genuine non-bypassability
  binds in CI and policy exactly as the git and security rules do (ADR-0016,
  ADR-0006). The design must not be oversold as bound locally.
- Prior-art skills (the v1 `cf-model-orchestrator` and the pre-codeflow
  `model-orchestrator`) contributed tmux plumbing only; their headless-primary
  doctrine and whole-output error-grep completion checks are explicitly not
  inherited.

## Architecture impact

None required for this ADR. The duo flow ships as the `cf-model-orchestrator`
skill (mirrored across `.claude/skills`, `.agents/skills`, and the `assets/base`
scaffold source), an opt-in `duo` pipeline preset with a `plan-align` stage in
the user-owned `.claude/workflows/pipeline.workflow.js`, and the
`codex-app-server-driver` doctrine as a skill resource — none of which
`docs/architecture.md` enumerates at that granularity (it documents
harness composition at the ADR-0005/0008 plane level, which already covers codex
at the process boundary). The only `architecture.md` text in this batch is
ADR-0016's security-review plane. If a maintainer later wants the interactive
driver recorded there, its one-line home is the engine area's harness-composition
note as a second, interactive codex-driver mode beside the headless boundary.

## Update (2026-07-11) — prefer the official codex-plugin-cc as the driver

The **Driver** decision above stands on the app-server as the mechanism, but the
recommended way to reach it is refined: prefer OpenAI's official
[`codex-plugin-cc`](https://github.com/openai/codex-plugin-cc) Claude Code plugin,
which wraps this same app-server and is vendor-maintained — sparing a hand-rolled
driver that must be kept in step with the experimental v2 API. The plugin gives
the duo its review (`/codex:review`, `/codex:adversarial-review`), delegation
(`/codex:rescue`), and resumable-session (`/codex:transfer`) surfaces. Direct
app-server driving (the `codex-app-server-driver` resource) is retained as the
advanced fallback for fully-programmatic driving or where the plugin cannot be
installed; tmux stays the last resort. Verify-on-install that a delegated task
reaches the configured MCP servers (Playwright, for UI e2e). The skill carries
the install steps and the command mapping.

## Update (2026-07-11) — driver ladder collapses to plugin-only (ADR-0018)

ADR-0018 makes cross-model transport interactive-only with one lane per
direction, which retires two of this ADR's three rungs. For Claude Code →
codex the lane is the `codex-plugin-cc` plugin **alone**: the direct
app-server driver (the `codex-app-server-driver` skill resource) and the tmux
fallback for driving codex are withdrawn, and the resource is deleted from the
scaffold — hand-rolling the app-server protocol is now prohibited, not merely
dispreferred, because it chases an experimental API the vendor already wraps.
The reverse lane (codex → claude) is the interactive `claude` CLI driven via
tmux; no rung anywhere is headless. The Driver decision's substance survives —
the plugin wraps the same app-server this ADR chose, with the same MCP reach
(re-verified 2026-07-11 on codex-cli 0.144.1) — only the sanctioned way to
reach it narrowed. The duo also gains a symmetric seat gate: the skill's
preflight now checks the orchestrating harness and the plugin surface, not
just codex auth, and degrades to solo from either missing half. This ADR is
not fully superseded (the flow, roles, gate, and degradation doctrine stand);
`superseded_by` stays null.
