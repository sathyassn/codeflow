---
name: cf-model-orchestrator
description: Coordinate the default Claude+Codex development duo from either Claude Code or Codex. Both models independently research, analyze, and plan; Claude leads design and final independent review; Codex implements and first-verifies; the host reconciles a versioned dual-approved plan and evidence ledger. Use for planned feature, fix, refactor, or documentation work with acceptance criteria. Requires native interactive sessions and degrades legibly when a required seat is unavailable; never uses headless model execution.
---

# cf-model-orchestrator — host-neutral development duo

Use the duo for all non-trivial development work with acceptance criteria.
Harness choice changes the transport and coordinator, not the roles or quality
bar. A conversational answer or pure question needs no duo.

Read [resources/quality-contract.md](resources/quality-contract.md) before
planning. It is the shared, harness-neutral contract for plans, evidence,
testing, coverage, UI validation, security, and final review. Harness-specific
agents are adapters to that contract, not alternate sources of truth.

## Invariants

- **Host coordinates.** The model running in the user's active harness owns the
  brief, task ledger, bounded reconciliation, durable evidence, and escalation.
- **Both think independently.** Claude and Codex research, analyze, identify
  risks, and draft a plan in parallel before seeing the other's conclusions.
- **Claude leads design.** Claude proposes the primary design and, unless the
  brief already fixes a clear direction, compares 2–3 viable options. Codex
  challenges feasibility, operability, security, and implementation detail.
- **Codex implements.** Once both approve the same versioned plan, Codex owns
  the implementation and first verification regardless of which harness hosts.
- **Claude final-reviews.** Claude independently reviews the diff, design
  conformance, security posture, and test evidence, and reruns relevant tests.
- **Evidence outranks agreement.** A model claim, consensus, or approval never
  substitutes for a source, file:line, command result, rendered UI observation,
  or other reproducible evidence.
- **Native interactive sessions only.** Each model runs in its own vendor
  harness with its configured tools and MCP servers. Never use `codex exec`,
  `claude -p` / `--print`, or another headless peer invocation.
- **Bounded loops.** Plan reconciliation and post-review rework are each bounded
  to at most two rounds. Unresolved disagreement or a red deterministic gate
  stops for the human; no model talks it green.

## Seat and transport matrix

Detect capabilities, not model identity.

| Active host | Peer lane | Coordinator | Fixed role binding |
|---|---|---|---|
| Claude Code | Official `codex-plugin-cc`, backed by Codex app-server | Claude host | Claude design/final review; Codex implementation/first verification |
| Codex App or interactive Codex CLI | Interactive Claude Code CLI in a task-scoped tmux session | Codex host | Claude design/final review; Codex implementation/first verification |
| Other harness, including Hermes | Only if it can attach to both sanctioned native interactive lanes and preserve their tools, sessions, and approvals | Other host | Same fixed Claude/Codex roles |

The desktop applications are not general automation endpoints for each other.
Claude Code reaches Codex through the official plugin/app-server integration.
Codex reaches Claude through the interactive Claude Code CLI; use tmux to keep
that TTY session durable and resumable. Do not automate either desktop GUI as a
peer protocol.

A non-Claude/Codex host may coordinate only when it can prove both seats and
their tool access. Otherwise run the harness-native solo flow and report which
seat or lane was absent. Never simulate a missing vendor with another instance
of the host model.

## Preflight

1. Pin the brief: objective, scope, constraints, acceptance criteria, and known
   non-goals. If any are missing, run the `cf-plan` clarity gate first.
2. Identify the active host and required lane from the matrix.
3. Verify command and tool readiness:
   - Codex: `codex` is present, `codex login status` succeeds, and
     `codex mcp list` shows the tools required by the task.
   - Claude: `claude` and `tmux` are present and `claude mcp list`
     succeeds. Verify account access with a short **interactive** Claude canary;
     do not treat a status subcommand as authoritative when it contradicts a
     working authenticated TTY.
   - Claude-host lane: the `codex@openai-codex` plugin is enabled and
     `/codex:setup` succeeds.
   - Codex-host lane: start Claude in a dedicated tmux session rooted at the
     worktree and complete one scoped interactive canary.
4. Verify task-specific tools before promising their evidence: test toolchain,
   security scanners, Playwright/browser tools for web UI, and Computer Use or
   a surface-specific driver for native/mobile/desktop UI.
5. Record the selected models and reasoning levels as run evidence. Choose the
   strongest supported model available for each seat; do not hard-code
   fast-aging model names into this skill.

An absent seat at preflight degrades legibly to the harness-native solo
`/cf-develop` flow with a separate read-only review pass. A mid-run failure
gets a bounded retry, diagnosis, and human escalation—never a silent downgrade.

## Workflow

### 1. Independent discovery in parallel

Give both seats the same immutable brief and repository scope. Before exchanging
conclusions, each seat independently returns:

- relevant source and documentation evidence;
- assumptions explicitly verified or still unresolved;
- edge/error/security cases;
- an implementation plan and test strategy;
- risks to compatibility, data, UX, and operations.

The host records both outputs without collapsing disagreements.

### 2. Design and plan settlement

Claude supplies the design options and recommendation. When the brief already
dictates one clear design direction, record that constraint and why option
exploration was waived. Codex reviews the design for implementation feasibility,
failure modes, security, testing, and maintainability.

The host reconciles the two drafts into **Plan v1** using the plan contract in
the quality resource. Both seats review exactly that version. Amendments create
v2, v3, and so on; approval of an older version does not carry forward.
Convergence is bounded to two reconciliation rounds. If both do not explicitly
approve the same version, stop for the human.

### 3. Detailed tasking

After dual approval, the host expands the agreed plan into ordered tasks with:

- owner and dependencies;
- files/interfaces expected to change;
- happy-path and edge/error acceptance criteria;
- unit, integration, end-to-end, UI, coverage, and security evidence required;
- rollback or recovery considerations where relevant.

Claude reviews design fidelity; Codex reviews executability. Both approve the
task breakdown before implementation begins.

### 4. Codex implementation and first verification

Codex works in the scoped feature worktree, implements the approved tasks, and
keeps the evidence ledger current. It runs formatting, static checks, unit and
integration tests, relevant end-to-end tests, coverage, dependency/security
checks, and UI-driven checks required by the quality contract.

For a Claude host, use the official plugin:

- `/codex:review` or `/codex:adversarial-review` for read-only critiques;
- `/codex:rescue` for implementation and first verification;
- `/codex:transfer` for a persistent task visible in Codex App/TUI.

For a Codex host, implementation stays in the current Codex worktree and
session; the Claude tmux session remains the design/review peer.

### 5. Claude independent final review

Claude reviews the actual diff rather than the implementation summary. It
reruns relevant tests, grades every acceptance criterion with evidence, checks
design conformance and UX/UI behavior, and performs the independent security
pass. In Claude Code, `cf-reviewer` and `cf-security-reviewer` may deepen the
pass; they do not replace Claude's cross-vendor review of Codex's work.

From a Codex host, this test-running review uses a separate interactive Claude
session with normal in-band permissions, not plan mode: plan mode is for pure
read-only analysis and may prevent the Bash/UI actions needed for verification.
Grant only the scoped test and inspection actions, explicitly prohibit source
edits, and require the worktree diff to remain unchanged after review. This is
verification authority, not an implementation handoff.

Any confirmed issue returns to Codex. Rework is bounded to two rounds and
requires fresh evidence. A deterministic failure or unverified criterion blocks
completion.

### 6. Joint closeout

Both seats approve the final diff and evidence ledger. The host reports:

- final plan version and both approvals;
- design option chosen (or the recorded waiver);
- acceptance criteria with reproducible evidence;
- exact test, coverage, security, and UI results;
- any explicit N/A with reason;
- residual risks or unresolved assumptions;
- the interactive transport used and session/canary evidence.

Only then hand off to `cf-ship`. If a remote exists, push committed logical
units for durability, but never use a backup push to imply review or merge
approval.
