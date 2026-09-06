---
name: cf-model-orchestrator
description: Coordinate the default Claude+Codex standing pair for every non-trivial repository task from Claude Code, Codex, or Grok Build. Both models independently research, analyze, and plan; Claude produces design in its native session; the host routes each approved task to a capable producer and cross-lineage reviewer; extra catalog families review on trigger if available, never as a silent third vote; the qualified Claude judgment primary owns the integrated Claude verdict; and the host reconciles a versioned dual-approved result and evidence ledger. Use for material research, analysis, planning, design, feature, fix, refactor, review, security, documentation, or verification work. Requires native interactive sessions and degrades legibly when a required seat is unavailable; never uses headless model execution.
---

# cf-model-orchestrator — host-neutral development duo

Use the duo for every non-trivial repository task. Harness choice changes
transport/coordinator, not duties or quality. One obvious local edit needs no
duo; material judgment, research, multiple surfaces, or deeper evidence does.

Read [resources/quality-contract.md](resources/quality-contract.md) and
[resources/capability-routing.md](resources/capability-routing.md), then load
the current concrete seats from
[resources/current-ensemble.json](resources/current-ensemble.json) and the
extra-family rule from
[resources/routing-policy.json](resources/routing-policy.json), before
planning. The markdown resources own durable quality and task assignment;
the JSON records own selectors, effort, internal workers, escalation, and
when a catalog family is named.
If `.codeflow/model-selection.json` contains project overrides, run
`codeflow doctor --check model-bindings` and use only the effective qualified
role bindings it reports. An absent or empty file keeps the managed defaults;
an invalid or drifted active selection blocks preflight without partial
application or silent fallback. The project file may reference binding IDs
only—it never owns raw selectors, worker routes, or commands.
Harness-specific agents are adapters, not alternate sources of truth.
For a multi-task plan or a possible dependency/decision change, also read
[resources/task-graph.md](resources/task-graph.md). When choosing or reviewing
test strength, read
[resources/verification-selection.md](resources/verification-selection.md).
For a new or materially reshaped user-facing surface, load `cf-design` before
settling Plan vN; a bounded change may record its explicit `conform` or `N/A`
path instead.
These on-demand resources keep the always-loaded path concise without making
their contracts optional.

## Outcome modes

Select the smallest complete stage set before starting; do not manufacture an
implementation stage for an analysis-only request.

- **Research / analysis:** independent discovery → evidence comparison → joint settled findings → closeout.
- **Plan / design:** independent discovery → Claude-led options → versioned dual-approved plan/tasks → closeout without edits.
- **Implementation:** routed production/verification → cross-lineage unit review → Claude-judgment-primary integrated judgment → closeout.
- **Review / verification:** independent inspection without self-review → Claude-judgment-primary integrated verdict; review grants no edit authority.
- **Substantive documentation:** use research/plan mode when only the proposed
  content is requested; use implementation mode when repository docs will be
  changed and verified. Apply `cf-editorial-review` before final approval.

## Invariants

- **Host coordinates.** The model running in the user's active harness owns the
  brief, task ledger, bounded reconciliation, durable evidence, and escalation.
- **Both think independently.** Claude and Codex research, analyze, identify
  risks, and draft a plan in parallel before seeing the other's conclusions.
- **Claude leads design.** The qualified Claude judgment primary **produces**
  the primary solution design in its own native interactive session and,
  unless the brief already fixes a clear direction, compares 2–3 viable
  options. For material product, UX, UI, interaction, or visual design, it
  also applies `cf-design` and settles `DESIGN_INTENT`. A Grok or Codex host
  may pass options and review; it never drafts design for Claude to
  rubber-stamp. Codex challenges feasibility, operability, security,
  proportionality, and implementation detail.
- **Host routes execution.** Once both approve the same versioned plan, the host
  assigns every task a producer and cross-lineage reviewer by task fit, tools/
  context, independence, verified availability/routing, resources, and observed
  native usage signals only. Seat/lineage reassignment invalidates approvals.
- **Review is producer-relative.** The producer first-verifies its own unit; the
  other lineage reviews it independently. Self-review is never independent.
  Name extra families on trigger if available; never a silent third vote.
- **The Claude judgment primary owns integrated Claude judgment.** The directly
  invoked model qualified for `claude-judgment-primary` reviews the settled
  design and integrated diff, reruns relevant tests, and owns the final quality
  verdict. Codex supplies independent review for a unit authored by that
  primary; its integrated pass is not independent review of its own unit.
- **Qualified reasoning seats.** Use the concrete selectors, default effort,
  escalation effort/triggers, and permitted internal worker classes in the
  current ensemble record: invoke each primary directly, let the owning primary
  control its internal routing, retain primary planning/approval duties, never
  infer worker routing or usage state, and never let a worker replace a primary
  or named cross-lineage reviewer. Spawn same-family high/xhigh workers; the
  medium primary stays the orchestrator. A binding change needs
  native-interactive qualification and promotion.
- **One orchestration owner.** Every invoked session declares `host`, `peer`, or
  `worker`. Only the host runs this top-level flow. A peer or worker completes
  its bounded assignment and returns evidence; it never starts a nested duo.
- **Evidence outranks agreement.** A model claim, consensus, or approval never
  substitutes for a source, file:line, command result, rendered UI observation,
  or other reproducible evidence.
- **Cross-lineage evidence carries native provenance.** Other-lineage output
  counts only with native runtime provenance (session/thread/task id plus
  model/effort labeled `observed` or `requested` by its actual evidence
  source); otherwise reclassify it as the author seat's lineage and redo the
  cross half. A relay is transport, not author; same-lineage
  worker output remains same-lineage, and vendor self-simulation is
  fabrication. Every delegated exchange meets the `cf-delegate` five-obligation
  evidence contract — launch, provenance, return, failure, recheck: verify the
  launch; on return verify native provenance plus the scoped diff and cited
  evidence (a relay's idle or completion signal is evidence of neither); keep
  the evidence recheckable through the native surface — the resumable Codex
  thread ID forward, the durable lifecycle records reverse. Record model and
  effort as observed only when the transport exposes actual values, otherwise
  as requested, and grade inferred completion explicitly as inferred.
- **Catastrophic actions remain human-gated.** Ordinary task-scoped project
  edits and deletions stay autonomous when recoverable. For a system-level,
  cross-boundary, credential/IAM, production, destructive-disk, security-
  weakening, irreversible, or high-blast-radius action, both seats assess risk
  and the host stops. Model consensus, Claude auto mode, Codex auto-review, or
  peer approval is never human authorization. Present the exact bounded action,
  preview where supported, current verified checkpoint or backup, and tested
  restore path; missing evidence, an untested restore path, or ambiguity fails
  closed. The non-relaxable class remains agent-blocked: a human operator
  performs it through a separate controlled channel. For another host-permitted
  high-risk action, execute one bounded step at a time and verify it; never use
  the peer to evade the host boundary. The quality contract owns full evidence.
- **Native interactive sessions only.** Each model runs in its own vendor
  harness with its configured tools and MCP servers. Never use `codex exec`,
  `claude -p` / `--print`, `grok -p` / `--single`, or another headless peer
  invocation.
- **Bounded, evidence-moving loops.** Plan reconciliation and post-review
  rework are each bounded to at most two rounds. A repeated attempt without a
  new hypothesis or changed evidence is not another round. At the bound,
  diagnose the persistent constraint and either take an
  approved-outcome-preserving strategic route with fresh evidence or surface a
  genuine external/owner block. A deterministic or safety gate is fixed or
  honored; its redness alone neither authorizes bypass nor makes the operator
  choose an implementation tactic.
- **Bounded parallelism.** Parallelize independent discovery and implementation
  only when it shortens the critical path. Use one owner/branch/worktree per
  task; serialize shared contracts and integration. Cap concurrency from
  observed memory, CPU, disk, context, and tool limits, preserve headroom, and
  reduce fan-out before pressure erodes evidence. Task output is provisional
  until the integrated diff and gates are green.

## Seat and transport matrix
Detect capabilities, not model identity.

| Active host | Peer lane | Coordinator | Execution binding |
|---|---|---|---|
| Claude Code | Official `codex-plugin-cc` / Codex app-server only | Claude host | Per-task producer/reviewer assignment; the Claude judgment primary leads design and integrated judgment |
| Codex App or interactive Codex CLI | Interactive Claude Code CLI via Herdr (tmux degraded) | Codex host | Per-task producer/reviewer assignment; the Claude judgment primary leads design and integrated judgment |
| Grok Build (interactive `grok` CLI) | Herdr `claude` + schema-v2; official `codex` CLI → app-server (Herdr; tmux degraded) | Grok host | Same contract. Claude produces design natively. Catalog Grok may produce or take named extra-family review |
| Other harness, including Hermes | Delegate the repository task to one sanctioned native host by default; coordinate directly only if both lanes and the full contract are proven | One native host | Same capability-routed contract; no nested orchestration |

Herdr/tmux cwd is the project being worked. Same topic reuses the tab; a new
topic gets a new tab; close it when that work is done. Claude Code reaches
Codex via plugin/app-server only (missing plugin degrades; no Herdr CLI third
lane). Grok reaches Codex via official `codex` CLI and the app-server daemon
(Herdr CLI if daemon missing; no third-party Grok Codex plugins). Codex reaches
Claude via Herdr (tmux degraded) plus schema-v2. Hermes and other non-catalog
harnesses delegate to one native host unless both lanes are proven. Missing
lane: solo fallback. Host is not duty; Claude produces design.

## Preflight

1. Pin the brief: objective, scope, constraints, acceptance criteria, and known
   non-goals. Use `cf-plan`'s clarity gate: discover repository and external
   facts autonomously, and ask only when a missing answer changes an
   operator-owned outcome, public behavior, authority, material security
   boundary, or irreversible action.
2. Identify the active host and required lane from the matrix. Set the current
   session role to `host`; the first line of every cross-harness task declares
   `ROLE: peer` or `ROLE: worker`, limits the task to that bounded assignment,
   and explicitly prohibits starting the top-level orchestrator or delegating
   back to the host lineage. A generic same-lineage subagent never satisfies
   the named cross-lineage assignment.
3. Verify command and tool readiness:
   - Codex: `codex` is present, `codex login status` succeeds, and
     `codex mcp list` shows the tools required by the task.
   - Claude: `claude` is present and `claude mcp list` succeeds. Prefer Herdr
     when `HERDR_ENV=1`; `tmux` is the degraded TTY. Verify account access with
     a short **interactive** Claude canary; do not treat a status subcommand as
     authoritative when it contradicts a working authenticated TTY.
   - Claude-host lane: the `codex@openai-codex` plugin is enabled and
     `/codex:setup` succeeds. Pass the Codex primary selector and default
     effort from the current ensemble record on the plugin task/rescue
     invocation at default effort; workers take escalation; do not inherit
     an unobserved user default.
   - Grok: `grok` present, `grok --version` succeeds, short interactive canary.
     Launch `--model <selector> --reasoning-effort <effort> --always-approve`
     from the ensemble. `--permission-mode auto` is consult/no-edit. Never
     `grok -p` / `--single`. Claude via Herdr+schema-v2 (tmux degraded); Codex
     via official `codex` CLI and app-server daemon. No third-party Grok Codex
     plugins. Do not claim those lanes complete without canaries.
   - Codex-host lane: start Claude via Herdr (tmux degraded) at the worktree
     with `--model <selector> --effort <effort> --permission-mode bypassPermissions --settings
     <state-dir>/settings.json` from the ensemble, then one scoped interactive
     canary. Make `autoMode.classifyAllShell` effective at user scope; Claude
     ignores it from repository settings, and repeated `--settings` flags are
     not a supported merge contract. Delegated work uses schema-v2 — `delegate init` → wait-ready → `arm` → canonical UTF-8/internal-LF exact-byte delivery → wait-accepted → wait-terminal
     with bounded cleanup — and the `cf-delegate` sibling Stop-hook preflight. If Fable is
     unavailable, record Opus as the fallback plus the same fail-closed
     sandbox; spawn workers at escalation effort and keep the primary at
     default.
     Consult and no-edit review stay on `--permission-mode auto`. Never claim
     the fallback was the selected primary.
4. Verify the autonomy boundary through the effective settings, not prose:
   - Claude: require sandbox + `failIfUnavailable: true`, sandboxed Bash
     autonomy, and raw-secret denies. Production uses `bypassPermissions`.
     Consult/no-edit uses auto plus user-scope `classifyAllShell`. Only a
     trusted installed tool may receive one classified unsandboxed retry;
     arbitrary unsandboxed commands remain out of bounds.
   - Codex: prefer app-server (`codex app-server daemon version` running);
     otherwise interactive CLI. Production: `--ask-for-approval never` and
     `--sandbox danger-full-access`. public network and live search are enabled.
     Auto-review is not human authorization; catastrophic work still stops for
     the operator.
   - Grok: `--always-approve` for production; `--permission-mode auto` for
     consult/no-edit; `--sandbox <PROFILE>` when required. Never `grok -p`.
   - Platform: use native macOS/Linux sandboxes; on Windows, prefer WSL2 for
     Linux-equivalent tooling. Native Windows Codex must use its elevated
     sandbox and the guard must cover PowerShell/Bash. Claude Code has no
     native-Windows OS sandbox; move catastrophic work to WSL2/container or fail
     closed. Record the actual platform and effective boundary.
5. Verify task-specific capabilities before promising their evidence: live web
   research and authoritative docs; GitHub/source control; the stack format,
   lint, test, coverage, dependency, and security tools; Playwright/browser for
   web UI; Computer Use or a surface driver for native/mobile/desktop UI; and
   any design, issue-tracker, database, cloud, or private-doc MCP the task needs.
   Authenticated tools use their broker/OAuth/keychain/credential-mask path—raw
   tokens never enter prompts, logs, repository files, or arbitrary commands.
6. Record models, effort/escalation, permissions, tools, live canaries, and any
   observed native usage signal used. Native status, routing metadata/canaries,
   and explicit harness errors are admissible; unknown remains unknown. Never
   infer quota or availability. Record actual versions at run time.

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

Claude supplies the design options and recommendation **from its native
session**. When the brief already dictates one clear design direction, record
that constraint and why option exploration was waived. Codex reviews the
design for implementation feasibility, failure modes, security, testing, and
maintainability. It also challenges whether a simpler proportionate design
satisfies the same requirements. A Grok host does not author that design pass.
When a user-facing surface materially changes, apply `cf-design` and include its
evidence-grounded `DESIGN_INTENT` in the plan. A cosmetic correction or
conformance-only change records the skill's compact `N/A` or `conform` path
rather than manufacturing design ceremony.

The host reconciles the two drafts into **Plan v1** using the plan contract in
the quality resource. Both seats review exactly that version. Amendments create
v2, v3, and so on; approval of an older version does not carry forward.
Convergence is bounded to two reconciliation rounds. If both do not explicitly
approve the same version, stop for the human.

For multi-task work, both approvals cover the same canonical task graph. A
material node, dependency, decision guard, ownership, acceptance, interface, or
safety-boundary change creates Plan vN+1 under the task-graph contract. Ordinary
steps and bounded implementation choices inside an approved node remain ledger
evidence and do not manufacture replanning ceremony.

### 3. Detailed tasking

After dual approval, the host expands the agreed plan into ordered tasks with:

- task id, producer/reviewer seat@effort, routing evidence, and dependencies;
- for multi-step work, the current critical dependency or blocker, resource
  focus, and the evidence event that causes reassessment;
- files/interfaces expected to change;
- happy-path and edge/error acceptance criteria;
- unit, integration, end-to-end, UI, coverage, and security evidence required,
  including the affected journey topology and each real boundary the
  end-to-end run must traverse or explicitly disclose as controlled/unverified;
- for concurrent UI work, the owner and run-scoped browser profile/context,
  service/application endpoints, test-data namespace, artifact directory,
  retention, and teardown verification required by the quality contract;
- rollback or recovery considerations where relevant.

Multi-task plans use the node/edge notation and mutation boundary in
`resources/task-graph.md`; durable task records materialize the same direct
dependencies. A single obvious task uses the resource's explicit N/A path.
The test plan applies `resources/verification-selection.md` and names the
trigger evidence for any property/generative, mutation, or architecture
fitness check—or records `none selected`.

Claude reviews design fidelity; Codex reviews executability. Both approve tasks
and assignments. A producer/reviewer seat or lineage change creates Plan vN+1
and requires both approvals; same-seat medium→high or high→xhigh on a documented
trigger is ledger evidence, not reassignment.

If implementation has independent tasks, add an explicit execution graph:

- the settled task graph and a valid integration order;
- one file/component owner, branch, and worktree per parallel task;
- shared or conflict-prone files reserved to one integration owner;
- a host resource budget and maximum concurrent heavyweight builds/browsers,
  with non-overlapping browser resources for every parallel UI task;
- the `integration/<epic>` branch and serialized `codeflow integrate` order;
- focused checks per task and combined checks after each landing.

Do not parallelize a short task when coordination costs more than it saves.
Never use concurrent writers in one worktree or rebase a shared integration
branch.

After both seats approve the exact Plan vN and task graph, invoke `cf-plan` to
materialize only the warranted epic/spec/task/ADR records on a `plan/` branch.
Validate them and merge that planning PR into each task's declared
`integration_target`. Before implementation, each durable task uses
`task/TSK-NNN-<slug>` and passes the read-only `codeflow work start TSK-NNN`
anchor check. The orchestrator owns independent discovery and settlement;
`cf-plan` owns clarification discipline and durable materialization. Neither
silently replaces the other.

### 4. Routed production and producer verification

Skip this stage when implementation is outside the selected outcome mode.
Otherwise each approved producer works in its scoped feature worktree and
implements the smallest clear, idiomatic, durable diff that satisfies the task
without speculative scope, preserves justified reuse and modular boundaries,
and handles accepted failure and edge cases. The producer keeps the evidence
ledger current and runs formatting, static checks, unit and
integration tests, relevant end-to-end tests, coverage, dependency/security
checks, and UI-driven checks required by the quality contract. Task branches
are not final evidence: integrate them in the approved order, rerun affected
checks after each landing, and run the aggregate suite on the combined diff.
The producer fixes and verifies a clear, safe, local, in-scope improvement when
validation is bounded rather than reflexively deferring it. Only uncertain
secondary observations enter the consolidated deferral batch; work does not
switch to cosmetic bait while actionable material work remains.

For a Claude host, use the official plugin for Codex-produced or Codex-reviewed
units:

- `/codex:review` or `/codex:adversarial-review` for read-only critiques;
- `/codex:rescue --model <selector> --effort <effort>` for production and
  verification, taking both values from the current ensemble record;
- `/codex:transfer` for a persistent task visible in Codex App/TUI.

Apply the same explicit selector and effort selection to every plugin task
that starts a primary Codex reasoning turn. Every plugin exchange must yield a
native Codex thread ID, recheckable through the plugin or the native Codex
surface — a generic Claude subagent or an unverified relay never counts as
Codex. Record model and effort as observed only when the transport exposes the
actual values; otherwise label them requested — a project-level medium default is
a fallback, not evidence that the requested turn used it, and requested is
never silently upgraded to observed.

For a Codex host, Codex-produced work stays in the current worktree and session;
Claude-primary- or Claude-worker-produced units stay in their native Claude
session. In either direction, the approved other-lineage reviewer independently
inspects the unit and its evidence before integration. Workers return to their
primary seat; workers never approve plans or replace the named reviewer.

### 5. Cross-lineage review and integrated Claude judgment

For implementation/review modes, each unit carries the named other-lineage
review and any finding returns to that unit's producer. Then the directly
invoked `claude-judgment-primary` reviews the actual integrated diff rather than
task summaries. It reruns relevant tests, grades every acceptance criterion
with evidence, rejects unnecessary or non-idiomatic complexity and brittle
under-design, checks design and design-system conformance plus UX/UI behavior,
performs the independent security pass, applies `cf-editorial-review` to
substantial changed prose and user-facing copy, and owns the final code and
design quality verdict. In Claude Code, `cf-reviewer` and
`cf-security-reviewer` may deepen the pass; they do not replace the required
other-lineage review or primary judgment. For a unit authored by the Claude
judgment primary, record the Codex independent review and describe the primary's
pass only as integrated judgment.
For research/analysis/plan modes, Claude instead final-reviews the settled
artifact and its source/evidence coverage.

From a Codex host, this test-running review uses a separate interactive Claude
session in auto mode under the same fail-closed sandbox—not plan or bypass
mode—so Bash/UI verification can proceed without an unattended permission
stall. Keep shell classification enabled, grant only the scoped test and
inspection actions, explicitly prohibit source edits, and require the worktree
diff to remain unchanged after review. This is verification authority, not an
implementation handoff.

Any confirmed issue returns to its designated producer. Rework is bounded to two rounds and
requires fresh evidence. A deterministic failure or unverified criterion blocks
completion. If the selected Claude judgment primary is unavailable, record the
fallback and reduced assurance;
never report that the selected Claude judgment primary reviewed the work.

Before closeout, both primary seats inspect the consolidated deferral batch
once. They choose `fix now`, `track once`, or `drop` for each related set,
challenge any convenience-based postponement, and investigate repeated minor
symptoms as one possible material cause. A fix returns to its designated
producer and repeats the affected verification/review. A worthwhile deferral
uses one existing tracking altitude and an event-based revisit trigger; no
batch, missing seat, or preference-only note is silently upgraded to agreement
or durable work. A scheduled/background peer task, notification promise, or
transport completion is not a disposition: the host waits for the actual
bounded result, verifies its native provenance and content, and only then
closes the checkpoint.

### 6. Joint closeout

Both seats approve the final diff and evidence ledger. The host reports:

- final plan version and both approvals;
- session roles and every producer/reviewer assignment with routing evidence;
- design option chosen (or the recorded waiver);
- acceptance criteria with reproducible evidence;
- exact test, coverage, security, and UI results;
- any explicit N/A with reason;
- residual risks or unresolved assumptions;
- the deferral batch outcome—fix now, one durable home plus event trigger, drop,
  or `none`—and both primary-seat dispositions or the recorded degradation;
- the interactive transport used and session/canary evidence.

Only an implementation or repository-editing documentation run hands off to
`cf-ship`. If a remote exists, push committed logical units for durability, but
never use a backup push to imply review or merge approval.
