<!-- codeflow:managed:begin scaffold=3.0.0 -->
@AGENTS.md

## Routing gate

Before repository or external research, analysis, planning, design, substantive
review/docs, implementation, or multi-step verification, invoke
`/cf-model-orchestrator`. Do not inspect first and route later: its preflight
and independent discovery are part of the work. Skip it only for conversation
or one obvious local check.

## Claude-specific notes

- Session hooks are wired in `.claude/settings.json`: orient digest at
  SessionStart, session summary at SessionEnd, git-guard and exec-guard before
  Bash. When a command is blocked, read the guard message — it names the
  violated policy rule and the sanctioned path.
- The project permission preset and fail-closed sandbox live in
  `.claude/settings.json`: sandbox-contained Bash, public web research, and
  local dev binding are enabled; a failed sandboxed command may request one
  auto-classified unsandboxed retry for a trusted installed tool that needs
  host state (for example, the official Codex plugin). This is not a general
  bypass. Claude ignores
  repository requests for both auto mode and classifier policy, so select auto
  in the active host (or user settings) and supply
  `autoMode.classifyAllShell` at user/CLI scope. `/cf-customize` canaries the
  effective mode; never claim the repo file enabled it.
- `.claude/settings.json` also sets `includeCoAuthoredBy` to false and an
  empty `attribution` for commit and PR with the session link off, so the
  host never injects the AI attribution that project policy forbids; the commit-msg hook and
  git-guard remain the floor if a host ignores the setting.
- The AGENTS.md git rules are **hook-enforced** here — the commit-msg hook and
  `git-guard` (wired in `.claude/settings.json`) block violations before they land, so
  fix the cause, never route around them. Full rules: AGENTS.md, "Git rules."

## Workflows

- For release policy, impact or publication, follow AGENTS.md's release route
  and the project-owned authority; do not create a Claude-specific versioning
  process or treat a normal merge as publication approval.
- `/cf-model-orchestrator` is the host-neutral Claude+Codex default for every
  non-trivial repository task, including research, analysis, planning, design,
  review, substantive docs, implementation, and verification. In this Claude
  host, use the official Codex plugin; Claude leads design, while the host
  records each responsible primary, actual executor, and cross-lineage reviewer. The
  qualified Claude judgment primary owns the integrated Claude verdict. A
  missing seat degrades legibly only
  after preflight. The unattended pipeline is
  explicitly single-vendor and never substitutes for the interactive duo.
- Load the concrete primary selectors, effort defaults/escalations, and
  permitted worker classes from
  `.claude/skills/cf-model-orchestrator/resources/current-ensemble.json`.
  Invoke each primary directly; the Claude primary owns Claude-side internal
  routing and judgment. Apply the responsibility, candidate/scoped-qualified,
  native-evidence and actual-authorship rules in the adjacent
  `capability-routing.md`; workers never replace either primary seat, its
  approval, or a named cross-lineage reviewer. The Claude design owner authors
  and executes real design work through its primary until a matching Claude
  route is scoped-qualified; candidate design routes are disposable fixtures,
  and another family needs an explicit task-specific operator override.
- Match stages and process weight to the outcome: trivial → just do it;
  otherwise begin `/cf-model-orchestrator`. Research/planning-only work exits
  after joint settlement; implementation continues through routed execution,
  executor verification, primary acceptance, cross-lineage review, and integrated
  Claude-judgment-primary review.
  `/cf-plan` and `/cf-develop` are supporting/solo flows,
  not alternate entry points. After mode selection, read and follow
  `.claude/skills/cf-method/references/workflow-lifecycle.md` for the mandatory
  compositional transition and failed-stage return; stage skills own their
  details.
- Compose stages and models in config (`args.stages`, `args.models`) — never
  hardcode them; the pipeline file is user-owned and `codeflow update` never
  touches it.
- **Stay lean by delegating** — the Claude mechanism for AGENTS.md's "Guard your
  context." Wide search → the `Explore` subagent; independent review → the
  `cf-reviewer` subagent; hard in-family reasoning → the strongest capable
  permitted same-Claude route at the effort the unit needs;
  substantial bounded routine non-design work → a capable permitted worker;
  batch/parallel or novel orchestration → a workflow. This session stays the
  orchestrator. Parallel branches use separate worktrees, bounded fan-out based on host
  memory/CPU, explicit file ownership, and serialized integration; do not trade
  machine pressure or merge ambiguity for nominal concurrency.
<!-- codeflow:managed:end -->
