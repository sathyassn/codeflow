---
id: ADR-0005
title: cross-vendor delegation via harness-boundary composition
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: none
---

# ADR-0005 — cross-vendor delegation via harness-boundary composition

## Context

Claude is not always the best or the freshest set of eyes on a piece of work,
and other coding CLIs (OpenAI's `codex`, Google's Antigravity `agy`) are already
installed and authenticated on developer machines under their own
subscriptions. We wanted a sanctioned way to get a second opinion or hand off a
parallel/specialty unit of work to another vendor — without automating anyone's
auth, without a new orchestration engine, and without letting a non-Claude
author slip past this repo's gates. A boundary rule and a canonical invocation
were needed before ad-hoc `codex` calls accreted their own conventions.

## Decision

Compose harnesses at the process boundary, each running under its own
subscription auth; CodeFlow's existing gates judge the output, never the author.
`codex` is the primary tier: the verified headless shape is
`codex exec --json [--cd DIR] [--skip-git-repo-check] --sandbox
read-only|workspace-write "task"` (JSONL on stdout — `thread.started.thread_id`,
`item.completed.item.text`, `turn.completed.usage`), with `codex exec resume
<thread_id>` for multi-turn — both live-verified on 2026-07-02. Two modes:
**consult** (read-only second opinion) and **delegate** (full handoff with
`workspace-write`, permitted ONLY inside a worktree on a feature branch, where
the delegate's commits pass through pre-commit, commit-msg, the test gate, and
an independent `cf-reviewer` pass exactly as Claude's do). OpenAI's official
`openai/codex-plugin-cc` is documented as the interactive (human-in-the-loop)
tier but not bundled. `agy` is a degraded, opt-in, read-only consult tier only.
Per ADR-0003 (judgment → Claude, mechanics → CLI) the capability ships as Claude
artifacts — the `cf-delegate` skill, the `cf-consult` command, and an optional
`consult` pipeline stage — plus exactly one deterministic `delegates` doctor
check; no engine orchestration code is added.

## Consequences

- A second, independently-trained model becomes available exactly where
  self-review is weakest, and genuinely parallel or specialty work can be handed
  off — at the cost of a round-trip, quota, and a mandatory synthesis step
  (delegate output is an input to verify, never a finding to paste).
- Enforcement stays author-agnostic: because delegates edit only inside a
  worktree on a feature branch, no gate needs to know or care that a commit came
  from `codex`. A delegate cannot lower the bar.
- `tmux`-driving `codex` interactively for headless use is rejected: it is
  dominated by `codex exec resume`, which retains thread context without any
  TTY scraping or its fragility.
- `agy` is deliberately second-class: its stdout can drop the final response
  under non-TTY (so automation must read the transcript JSONL, never stdout), it
  has no structured-output mode, and multi-agent fan-out burns plan quota fast.
  It is read-only, opt-in, and used only when the user names it.
- Delegation is optional everywhere: the `consult` pipeline stage is off unless
  named in `args.stages` (default preset unchanged), and the `delegates` doctor
  check warns rather than fails when `codex` is absent or unauthenticated. Auth
  is never automated — the user runs `codex login` on a single account, and the
  system degrades legibly on 401.
- The pipeline is user-owned (ADR-0004): the shipped reference and this repo's
  copy gained the `consult` stage in lockstep, but consumers with an existing
  copy adopt it manually — there is no update path that would touch their file.

## Architecture impact

None — the capability is Claude scaffold artifacts plus one deterministic
doctor check reusing the existing check registry and injection seams; no shipped
module boundary moves and no orchestration organ is added.
