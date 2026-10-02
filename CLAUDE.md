<!-- codeflow:managed:begin scaffold=3.0.1 -->
@AGENTS.md

## Routing gate

Invoke `/cf-model-orchestrator` (the Skill tool) before you inspect. For
routed work, which the map's "Route by touched paths" rule decides, it comes
before repository or external research, analysis, planning, design,
substantive review or docs, implementation, or multi-step verification. Do
not inspect first and route later: its preflight and independent discovery
are part of the work. Skip it only for conversation or one obvious local
check; when unsure, route.

## Claude-specific notes

- Session hooks are wired in `.claude/settings.json`: the orient digest at
  SessionStart, the session summary at SessionEnd, and `git-guard` and
  `exec-guard` before every Bash call. A blocked command's message names the
  rule and the sanctioned path; fix the cause.
- The project permission preset and fail-closed sandbox live in
  `.claude/settings.json`: sandbox-contained Bash, public web research and
  local dev binding are enabled; a failed sandboxed command may request one
  auto-classified unsandboxed retry for a trusted installed tool that needs
  host state (for example, the official Codex plugin). This is not a general
  bypass. Claude ignores repository requests for both auto mode and
  classifier policy, so select auto in the active host (or user settings) and
  supply `autoMode.classifyAllShell` at user or CLI scope. `/cf-customize`
  canaries the effective mode; never claim the repo file enabled it.
- `.claude/settings.json` also sets `includeCoAuthoredBy` to false and an
  empty `attribution` for commit and PR with the session link off, so the
  host never injects the AI attribution that project policy forbids; the
  commit-msg hook and git-guard remain the floor if a host ignores the
  setting.
- The git rules are **hook-enforced** here: the commit-msg hook and
  `git-guard` block violations before they land, so fix the cause, never
  route around them. Full rules: `.codeflow/rules/git-rules.md`.

## How the map's rows run in Claude Code

| Row | Claude mechanism |
|---|---|
| take a new request (routed work; when unsure, route) | `/cf-model-orchestrator` once per brief, with the seat selectors and efforts in `.claude/skills/cf-model-orchestrator/resources/current-ensemble.json`; the other lineage through the official Codex plugin, or a Herdr tab when `HERDR_ENV=1`; the Claude seat owns design and the integrated verdict; a missing seat degrades legibly after preflight and is recorded; after mode selection, follow `.claude/skills/cf-method/references/workflow-lifecycle.md`. Read `.claude/skills/cf-method/references/autonomy.md` for what to settle yourself and what to escalate. `/cf-plan` and `/cf-develop` are supporting or solo flows, not alternate entry points |
| build | this session stays the orchestrator; wide search goes to the `Explore` subagent, bounded routine work to a capable worker as a native subagent of this session, never a separate CLI session or Herdr tab; parallel branches use separate worktrees, bounded fan-out from host memory and CPU, explicit file ownership and serialized landing |
| give or ask for a review | the other lineage first; `cf-reviewer` as the same-family fresh-context pass, in the foreground (`run_in_background: false`) so its verdict lands before the turn ends; `cf-security-reviewer` on its trigger |
| show something complex | when the session offers the inline widget tool (`show_widget`), the figure goes in the reply as an inline HTML widget; when it offers the Artifact tool, a page to share or comment on is an Artifact page, or `/cf-present` when anchored review is needed; with neither (the terminal), fenced ASCII |
| unattended, batch or parallel runs | `.claude/workflows/pipeline.workflow.js`, composed through `args.stages` and `args.models`, never hardcoded; the pipeline is single-vendor and never substitutes for the interactive duo; the file is user-owned and `codeflow update` never touches it |
| release | the release route in `cf-ship` and the project's own authority; no Claude-specific versioning, and a normal merge is not publication approval |
<!-- codeflow:managed:end -->
