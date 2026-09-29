<!-- codeflow:managed:begin scaffold=3.0.0 -->
@AGENTS.md

## Routing gate

The paths a task will touch decide its entry. Work that changes an
adopter-facing path (product code, managed instructions, hooks, policy, CI,
shipped templates, watched contracts), research or analysis that will drive
such a change, and plan, design, security or irreversible work are routed. Before repository or external research,
analysis, planning, design, substantive review or docs, implementation, or
multi-step verification of routed work, invoke
`/cf-model-orchestrator`. Do not inspect first and route later: its preflight
and independent discovery are part of the work. Skip it only for
conversation, one obvious local check, or an edit outside those paths; when
unsure, route.

## Claude-specific notes

- Session hooks are wired in `.claude/settings.json`: orient digest at
  SessionStart, session summary at SessionEnd, git-guard and exec-guard before
  Bash. When a command is blocked, read the guard message: it names the
  violated policy rule and the sanctioned path.
- The project permission preset and fail-closed sandbox live in
  `.claude/settings.json`: sandbox-contained Bash, public web research, and
  local dev binding are enabled; a failed sandboxed command may request one
  auto-classified unsandboxed retry for a trusted installed tool that needs
  host state (for example, the official Codex plugin). This is not a general
  bypass. Claude ignores repository requests for both auto mode and
  classifier policy, so select auto in the active host (or user settings) and
  supply `autoMode.classifyAllShell` at user/CLI scope. `/cf-customize`
  canaries the effective mode; never claim the repo file enabled it.
- The git rules are **hook-enforced** here: the commit-msg hook and
  `git-guard` (wired in `.claude/settings.json`) block violations before they
  land, so fix the cause, never route around them. Full rules:
  `.codeflow/rules/git-rules.md`.

## Workflows

- For release policy, impact or publication, follow the release route in
  `cf-ship` and the project-owned authority; do not create a Claude-specific
  versioning process or treat a normal merge as publication approval.
- `/cf-model-orchestrator` is the host-neutral Claude+Codex default for work
  the routing gate sends to it. In this Claude host, use the official Codex
  plugin; Claude leads design, while the host records each responsible
  primary, actual executor, and cross-lineage reviewer. The qualified Claude
  judgment primary owns the integrated Claude verdict. A missing seat
  degrades legibly only after preflight. The unattended pipeline is
  explicitly single-vendor and never substitutes for the interactive duo.
- Load the concrete primary selectors, effort defaults and escalations, and
  permitted worker classes from
  `.claude/skills/cf-model-orchestrator/resources/current-ensemble.json`.
  Invoke each primary directly; the Claude primary owns Claude-side internal
  routing and judgment. Apply the responsibility, candidate and
  scoped-qualified, native-evidence and actual-authorship rules in the
  adjacent `capability-routing.md`; workers never replace either primary
  seat, its approval, or a named cross-lineage reviewer. The Claude design
  owner authors and executes real design work through its primary until a
  matching Claude route is scoped-qualified; candidate design routes are
  disposable fixtures, and another family needs an explicit task-specific
  operator override.
- Match stages and process weight to the outcome. Research or planning-only
  work exits after joint settlement; implementation continues through routed
  execution, executor verification, primary acceptance, cross-lineage review,
  and integrated Claude-judgment-primary review. `/cf-plan` and `/cf-develop`
  are supporting or solo flows, not alternate entry points. After mode
  selection, read and follow
  `.claude/skills/cf-method/references/workflow-lifecycle.md` for the
  mandatory compositional transition and failed-stage return; stage skills
  own their details. Read `.claude/skills/cf-method/references/autonomy.md`
  for what to settle yourself and what to escalate.
- Compose stages and models in config (`args.stages`, `args.models`), never
  hardcode them; the pipeline file is user-owned and `codeflow update` never
  touches it.
- **Stay lean by delegating**, the Claude mechanism for "Guard your context"
  in `.codeflow/rules/workflow-discipline.md`. Wide search goes to the
  `Explore` subagent; independent review to the `cf-reviewer` subagent; hard
  in-family reasoning to the strongest capable permitted same-Claude route at
  the effort the unit needs; substantial bounded routine non-design work to a
  capable permitted worker, always as a native subagent of this session,
  never a separate CLI session or Herdr tab; batch, parallel or novel orchestration to a
  workflow. This session stays the orchestrator. Parallel branches use
  separate worktrees, bounded fan-out based on host memory and CPU, explicit
  file ownership, and serialized integration; do not trade machine pressure
  or merge ambiguity for nominal concurrency.
<!-- codeflow:managed:end -->
