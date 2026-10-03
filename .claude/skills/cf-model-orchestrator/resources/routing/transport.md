## Cross-family transport

This section is the one statement of how a session reaches a model of
another family, and of how it reaches its own family. Every skill and
managed instruction cites it and does not restate it (operator decision of
2026-10-03, CodeFlow ADR-0077). `cf-delegate` owns what a consult or handoff
is and its evidence; `cf-herdr` owns the tab.

### Same family

A same-family worker runs inside the caller's own harness, as a native
subagent of the caller's session (the desktop app's subagents or the CLI's
own subagents), with `ROLE: worker`. It never runs as a separate CLI session
or a Herdr tab.

### Another family

Another family runs as an interactive seat, the callee's own CLI in a Herdr
tab that `cf-herdr` creates:

| Callee | Seat |
|---|---|
| Codex | The interactive Codex CLI on the local Codex app-server (`codex app-server daemon version` shows it running; `codex app-server daemon start` starts it). When the daemon cannot run, the interactive Codex CLI on its own |
| Claude | The interactive Claude Code CLI, its turns tracked by the `codeflow delegate` lifecycle |
| Grok | The interactive Grok Build CLI |

Any host may drive any reachable Herdr server (`herdr status server` reports
`status: running`), from inside a Herdr pane or outside one, under
`cf-herdr`'s rules: its own tabs only, unique `cf-` names, a `cwd` check
before every send, and never the operator's panes.

When the preferred seat cannot run, use these in order and record the
fallback as such:

1. From a Claude Code host to Codex only: the official Codex plugin, an
   optional fallback under `cf-delegate`'s qualified native fallback.
2. tmux, the last fallback, only when no Herdr server is reachable.

### Always interactive

Never `codex exec`, `claude -p` / `--print`, `grok -p` / `--single`, another
headless peer run, or hand-rolled app-server JSON-RPC. Status commands such as
`codex login status` or `codex --version` are not work sessions.

### Launch postures

Each seat starts in its CLI's autonomous permission mode, so it never stalls
on an approval prompt. This table is the one home of these flags:

| Seat | Production and building | Consult and review |
|---|---|---|
| Claude | `--permission-mode bypassPermissions` | `--permission-mode auto` |
| Codex | `--ask-for-approval never --sandbox danger-full-access` | `--ask-for-approval never`, no `--sandbox` flag, so the project's `cf-guard` profile applies |
| Grok | `--always-approve` | `--permission-mode auto` |

The Codex builder posture is ADR-0075 D1: it moves to the `cf-builder`
profile only after that decision's spike passes, and this row is the one
line that changes then. Grok adds `--sandbox <PROFILE>` when an OS sandbox is
required. A consult or review edits nothing whatever its mode. Never
`--dangerously-skip-permissions` unless the operator named it, and never
`--dangerously-bypass-hook-trust`.

### First-run prompts

A new seat can stop at a folder trust prompt, a project hook trust prompt or
a self-update offer before its first turn. The caller answers folder trust
only for the task's own folder (`cf-method/references/autonomy.md`, "Trust
prompts"). Hook trust is the operator's (ADR-0075): the caller checks that
the hooks the seat shows match the base branch's hooks, reports the seat as
waiting, and does not brief it until its hooks run. Skip a self-update offer.
