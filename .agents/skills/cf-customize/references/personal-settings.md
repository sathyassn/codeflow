# Status line and effort: shared and personal settings

Read this when a project or a person wants a different Claude Code status
line, or wants to set the reasoning effort for Claude Code or Codex.

## What CodeFlow ships

- **Status line.** `.claude/settings.json` sets a `statusLine` command that
  reads Claude Code's status JSON and shows the model name and id, the Claude
  Code version, the output style, the folder, the git branch when there is
  one, and the context used out of the window with a percentage: green below
  50, yellow below 80, red from 80. It needs `jq`. Without `jq` it prints the
  folder and branch followed by `jq not found`, and exits 0.
- **Effort.** Nothing. The shared files set no `effortLevel` and no
  `model_reasoning_effort` (CodeFlow ADR-0087). Seat launches through
  `cf-herdr` pass their own effort, so this does not change a seat's effort.

## Which setting wins

Claude Code reads, from highest to lowest: managed settings, command-line
flags, `.claude/settings.local.json`, the shared `.claude/settings.json`, then
`~/.claude/settings.json`. A key the shared file sets therefore hides the same
key in a person's user file.

Codex reads command-line flags (`-c key=value`) and profiles first, then the
project's `.codex/config.toml` in a trusted project, then
`~/.codex/config.toml`. Codex has no untracked per-project file.

## Set your own

| Setting | Claude Code | Codex |
|---|---|---|
| Effort | `"effortLevel"` in `~/.claude/settings.json` (every project, since CodeFlow ships none), in `.claude/settings.local.json` (this checkout), or `--effort` at launch | `model_reasoning_effort` in `~/.codex/config.toml`, or `-c model_reasoning_effort="<effort>"` at launch |
| Status line | `"statusLine"` in `.claude/settings.local.json`; the user file's line is hidden while the project ships one | not applicable |

`.claude/settings.local.json` is gitignored and belongs to one checkout. A new
worktree starts without it, so with one worktree per task a person copies it
in when they create the worktree, for example
`cp <main checkout>/.claude/settings.local.json .claude/`. The user file has no
such limit, which makes it the better home for effort.

## Opt a project in to a shared effort

A project that wants one effort for everyone adds it to its shared files:
`"effortLevel": "high"` in `.claude/settings.json` and
`model_reasoning_effort = "high"` in `.codex/config.toml`. The tradeoff
(ADR-0056 and ADR-0087): a shared value gives every session someone starts by
hand the same starting effort, and it replaces the effort of every
contributor who prefers another, `xhigh` included. Without it, a person with
no effort of their own gets the harness default, which may be lower than
high. Ask before adding it, and add it only when the project chooses it.

## What `codeflow update` does

- A `statusLine` still equal to the line CodeFlow last shipped, as recorded
  under `.codeflow/.baseline/`, moves to the new shipped line. A line the
  project changed stays and is reported. A line the project deleted stays
  deleted.
- An `effortLevel` still equal to the value CodeFlow last shipped is removed;
  a value the project changed stays and is reported. Update never adds one.
- `.codex/config.toml` is a managed file, merged against its recorded
  baseline: the shipped `model_reasoning_effort` line is removed, and a line
  the project added or changed is kept or reported as a conflict.
- Update never reads or writes `.claude/settings.local.json` or a user file.
- With no recorded baseline, update cannot tell a shipped value from the
  project's own, so it keeps every value and adds only a missing
  `statusLine`.
