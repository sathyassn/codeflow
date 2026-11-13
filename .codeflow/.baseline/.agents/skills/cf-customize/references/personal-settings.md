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

Claude Code reads, from highest to lowest: managed settings, `--settings`
at launch, `.claude/settings.local.json`, the shared `.claude/settings.json`,
then `~/.claude/settings.json`. A key the shared file sets therefore hides
the same key in a person's user file. A flag for one key, such as
`--effort`, overrides that key for the session (Claude Code settings docs,
"Settings precedence").

Codex reads, from highest to lowest: command-line flags and `-c`
overrides, then the project's `.codex/config.toml` in a trusted project,
then a profile file selected with `--profile`, then `~/.codex/config.toml`
(Codex configuration docs, "Configuration precedence"). A profile therefore
does not override a value the project sets; a `-c` flag does. Codex has no
untracked per-project file.

## Set your own

| Setting | Claude Code | Codex |
|---|---|---|
| Effort | `"effortLevel"` in `~/.claude/settings.json` (every project, since CodeFlow ships none), in `.claude/settings.local.json` (this repository), or `--effort` at launch | `model_reasoning_effort` in `~/.codex/config.toml`, or `-c model_reasoning_effort="<effort>"` at launch |
| Status line | `"statusLine"` in `.claude/settings.local.json`; the user file's line is hidden while the project ships one | not applicable |

`.claude/settings.local.json` is gitignored. Since Claude Code v2.1.211, a
session in a git repository reads and writes it at the repository root,
and a session in a worktree uses the one at the main checkout's root, so
one file serves every worktree of the repository. It stays beside
`.claude/settings.json` instead outside a git repository, when the
repository root is the home folder, on Windows, and when the repository
root or its `.git` or `.claude` entry is not owned by the person; there,
and before v2.1.211, a worktree needs its own copy. A copy an earlier
version left in a worktree is still read, and where both files set a key
the root file wins (Claude Code settings docs, "Where Claude Code keeps the
local file in a git repository").

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
- Only `statusLine` and `effortLevel` follow these rules. Every other
  plain top-level key, including `attribution` and `includeCoAuthoredBy`,
  which keep the host from adding the AI attribution project policy
  forbids, is restored whenever the project lacks it. `hooks`,
  `permissions`, `sandbox` and `env` follow their own merge rules:
  CodeFlow's own hook commands and missing sandbox entries are added back,
  while a shipped permission entry or `env` key the project removed stays
  removed when the recorded baseline shows CodeFlow shipped it.
- Update never reads or writes `.claude/settings.local.json` or a user file.
- With no recorded baseline, update cannot tell a shipped value from the
  project's own, so it keeps every value and adds only a missing
  `statusLine`.
