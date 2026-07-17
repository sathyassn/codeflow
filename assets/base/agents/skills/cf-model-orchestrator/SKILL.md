---
name: cf-model-orchestrator
description: Coordinate the default Claude+Codex duo for every non-trivial repository task from either Claude Code, Codex, or another capable host. Both models independently research, analyze, and plan; Claude leads design and final independent review; Codex implements and first-verifies when implementation is in scope; the host reconciles a versioned dual-approved result and evidence ledger. Use for material research, analysis, planning, design, feature, fix, refactor, review, security, documentation, or verification work. Requires native interactive sessions and degrades legibly when a required seat is unavailable; never uses headless model execution.
---

# cf-model-orchestrator — host-neutral development duo

Use the duo for every non-trivial repository task. Harness choice changes the
transport and coordinator, not the roles or quality bar. A conversational
answer or one obvious local edit needs no duo; uncertainty, material judgment,
external or repository research, multiple affected surfaces, or evidence beyond
one obvious check makes the task non-trivial.

Read [resources/quality-contract.md](resources/quality-contract.md) before
planning. It is the shared, harness-neutral contract for plans, evidence,
testing, coverage, UI validation, security, and final review. Harness-specific
agents are adapters to that contract, not alternate sources of truth.

## Outcome modes

Select the smallest complete stage set before starting; do not manufacture an
implementation stage for an analysis-only request.

- **Research / analysis:** independent discovery → evidence comparison → joint
  settled findings and recommendations → closeout.
- **Plan / design:** independent discovery → Claude-led options → versioned
  dual-approved plan and detailed tasks → closeout without edits.
- **Implementation:** full workflow through Codex implementation, first
  verification, Claude final review, and joint closeout.
- **Review / verification:** both inspect independently; Codex performs the
  first evidence pass and Claude owns the final verdict. Findings return to the
  designated implementer; review authority does not imply edit authority.
- **Substantive documentation:** use research/plan mode when only the proposed
  content is requested; use implementation mode when repository docs will be
  changed and verified.

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
- **Strongest reasoning seats.** Use the latest available Fable-class Claude
  model at xhigh effort for research, analysis, synthesis, design,
  coordination, and final review. Fable may delegate mechanistic browser,
  Playwright, Computer Use, or MCP operation to current Opus-class subagents,
  but Fable interprets their evidence and owns every judgment. Use the strongest
  supported Codex coding model at xhigh for orchestration, implementation, and
  difficult verification. Lower-effort Codex workers are optional only when the
  harness exposes safe per-worker routing; never assume that it does.
- **Evidence outranks agreement.** A model claim, consensus, or approval never
  substitutes for a source, file:line, command result, rendered UI observation,
  or other reproducible evidence.
- **Native interactive sessions only.** Each model runs in its own vendor
  harness with its configured tools and MCP servers. Never use `codex exec`,
  `claude -p` / `--print`, or another headless peer invocation.
- **Bounded loops.** Plan reconciliation and post-review rework are each bounded
  to at most two rounds. Unresolved disagreement or a red deterministic gate
  stops for the human; no model talks it green.
- **Bounded parallelism.** Parallelize independent discovery and implementation
  workstreams when it shortens the critical path. Give each implementation task
  one owner, branch, and worktree; serialize shared contracts and integration.
  The host sets and revises a concurrency cap from available memory, CPU, disk,
  context, and tool limits—reserve headroom and reduce fan-out before swapping,
  duplicate heavyweight builds, browser fleets, or context sprawl erode
  quality. Parallel output is provisional until the integrated diff is green.

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
     worktree with `--model fable --effort xhigh --permission-mode auto
     --settings '{"autoMode":{"classifyAllShell":true}}'` and complete one
     scoped interactive canary. The explicit CLI settings scope makes every
     shell action reach the auto-mode classifier; Claude intentionally ignores
     `autoMode` from repository settings. If Fable or auto mode is
     unavailable, record the exact capability gap and use the strongest
     supported Claude reasoning model with `acceptEdits` plus the same
     fail-closed sandbox; never fall through to bypass mode on an ordinary host.
4. Verify the autonomy boundary through the effective settings, not prose:
   - Claude: sandbox enabled, `failIfUnavailable: true`, sandboxed Bash
     auto-allowed, unsandboxed escape disabled, destructive and privileged
     actions still classified or prompted, and raw secret reads denied. A
     repository cannot set `defaultMode: auto` or classifier policy; select
     auto and `classifyAllShell` at CLI/user scope and canary the effective mode.
   - Codex: the named permission profile is active without a competing legacy
     `sandbox_mode`, public network and live search are enabled, approvals use
     `on-request`, and eligible escalations route to automatic review.
5. Verify task-specific capabilities before promising their evidence: live web
   research and authoritative docs; GitHub/source control; the stack format,
   lint, test, coverage, dependency, and security tools; Playwright/browser for
   web UI; Computer Use or a surface driver for native/mobile/desktop UI; and
   any design, issue-tracker, database, cloud, or private-doc MCP the task needs.
   Authenticated tools use their broker/OAuth/keychain/credential-mask path—raw
   tokens never enter prompts, logs, repository files, or arbitrary commands.
6. Record the selected models, reasoning levels, permission modes, tool
   inventories, and live-canary evidence. Model family names above are routing
   classes; record the actual current versions at run time rather than freezing
   them into a plan.

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

If implementation has independent tasks, add an explicit execution graph:

- dependencies and merge order;
- one file/component owner, branch, and worktree per parallel task;
- shared or conflict-prone files reserved to one integration owner;
- a host resource budget and maximum concurrent heavyweight builds/browsers;
- the `integration/<epic>` branch and serialized `codeflow integrate` order;
- focused checks per task and combined checks after each landing.

Do not parallelize a short task when coordination costs more than it saves.
Never use concurrent writers in one worktree or rebase a shared integration
branch.

### 4. Codex implementation and first verification

Skip this stage when implementation is outside the selected outcome mode.
Otherwise Codex works in the scoped feature worktree—or coordinates the
approved bounded set of task worktrees—implements the approved tasks, and keeps
the evidence ledger current. It runs formatting, static checks, unit and
integration tests, relevant end-to-end tests, coverage, dependency/security
checks, and UI-driven checks required by the quality contract. Task branches
are not final evidence: integrate them in the approved order, rerun affected
checks after each landing, and run the aggregate suite on the combined diff.

For a Claude host, use the official plugin:

- `/codex:review` or `/codex:adversarial-review` for read-only critiques;
- `/codex:rescue` for implementation and first verification;
- `/codex:transfer` for a persistent task visible in Codex App/TUI.

For a Codex host, implementation stays in the current Codex worktree and
session; the Claude tmux session remains the design/review peer.

### 5. Claude independent final review

For implementation/review modes, Claude reviews the actual integrated diff
rather than task summaries. It reruns relevant tests, grades every acceptance
criterion with evidence, checks design conformance and UX/UI behavior, and
performs the independent security pass. In Claude Code, `cf-reviewer` and
`cf-security-reviewer` may deepen the pass; they do not replace Claude's
cross-vendor review of Codex's work. For research/analysis/plan modes, Claude
instead final-reviews the settled artifact and its source/evidence coverage.

From a Codex host, this test-running review uses a separate interactive Claude
session in auto mode under the same fail-closed sandbox—not plan or bypass
mode—so Bash/UI verification can proceed without an unattended permission
stall. Keep shell classification enabled, grant only the scoped test and
inspection actions, explicitly prohibit source edits, and require the worktree
diff to remain unchanged after review. This is verification authority, not an
implementation handoff.

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

Only an implementation or repository-editing documentation run hands off to
`cf-ship`. If a remote exists, push committed logical units for durability, but
never use a backup push to imply review or merge approval.
