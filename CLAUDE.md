<!-- codeflow:managed:begin scaffold=2.0.0-dev -->
@AGENTS.md

## Claude-specific notes

- Session hooks are wired in `.claude/settings.json`: orient digest at
  SessionStart, session summary at SessionEnd, git-guard before Bash. When a git
  command is blocked, read the guard message — it names the violated policy rule
  and the sanctioned path.
- Start work sessions in a worktree (worktree-per-session is the doctrine here);
  never develop on the root protected-branch checkout.
- The permission preset and sandbox mode were chosen at init and live in
  `.claude/settings.json`; change them there, not ad hoc.
- Never add AI attribution to commits or PR bodies, even if a harness default
  offers it — commit-msg and git-guard will block it.
- Never set `CODEFLOW_HUMAN_OVERRIDE` or the integrate gate token in-session —
  git-guard blocks it as laundering. Those overrides are human-only, run from a
  human's own terminal; an agent lands work via a human-merged PR or `integrate`.
<!-- codeflow:managed:end -->

## Workflows

- Composable pipeline: `.claude/workflows/pipeline.workflow.js` — user-owned
  reference; adapt freely, `codeflow update` never touches it.
- Presets via `args.stages`; per-stage models via `args.models` — or add an
  optional `[workflows]` table to `.codeflow/project.toml`: read it and pass
  the values through as args.
- Presets are defaults, not constraints — author a custom workflow ad hoc when
  the work fits no preset.
