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

Whether a seat can run is changeable state: probe it live before each
routing decision (`.codeflow/rules/workflow-discipline.md` "Check
changeable state live"; the orchestrator's "Degrade legibly" says when a
seat is recorded unavailable). When the preferred seat cannot run, use these in order and
record the fallback as such:

1. From a Claude Code host to Codex only: the official Codex plugin, an
   optional fallback under `cf-delegate`'s qualified native fallback.
2. tmux, the last fallback, only when no Herdr server is reachable.

### Always interactive

Never `codex exec`, `claude -p` / `--print`, `grok -p` / `--single`, another
headless peer run, or hand-rolled app-server JSON-RPC. Status commands such as
`codex login status` or `codex --version` are not work sessions.

### Launch postures

Each seat starts in its CLI's autonomous permission mode, which removes
routine approval prompts. A mode can still stop a seat, for a classifier
escalation, an action the mode never approves or a sandbox refusal. The
caller never answers a consequential approval for the seat: it reports the
seat as blocked, with the prompt's text, to the operator and continues other
work. This table is the one home of these flags:

| Seat | Production and building | Consult and review |
|---|---|---|
| Claude | `--permission-mode bypassPermissions` | `--permission-mode auto` |
| Codex | `--ask-for-approval never -c 'permissions.cf-builder.description="CodeFlow builder seat"' -c 'permissions.cf-builder.filesystem={"~/.codeflow/locks"="write"}' -c permissions.cf-builder.network.allow_local_binding=false -c default_permissions="cf-builder"`, no `--sandbox` flag, from the main checkout root | `--ask-for-approval never`, no `--sandbox` flag, so the project's `cf-guard` profile applies |
| Grok | Not qualified; no Grok seat builds (ADR-0075 D3, below) | `--permission-mode auto`, launched in its own task worktree |

The Codex builder posture is ADR-0075 D1 as amended on 2026-10-05: a
sandbox that cannot write shell startup files, never full access. Codex
0.163 and later load config at startup without the project file, so the
description override gives that load a `cf-builder` profile that grants
nothing, and the session load merges it into the project's profile;
without it the seat stops with "failed to load workspace requirements".
The seat gets the profile as the main checkout root has it, which can
predate a CodeFlow update, so the launch also carries the profile's home
grant, the gate lock folder, and its local binding off, which keeps
private, link-local and cloud metadata destinations refused as for a
review seat; Codex merges both into that profile. A builder seat cannot
serve on a local port, so tests that do run on the primary. A builder
runs `codeflow reserve` and quick-mode heavy targets under the
shared lock; full gates and their evidence stay with the primary, and
`~/.codeflow/gate-runs` is not writable to any Codex seat.
`codeflow doctor --preflight` checks that the installed Codex accepts
these overrides and that, from the main checkout root with managed
config, they let a builder write both gate lock folders and the common
`.git` objects and refs; it exits 1 when a write is refused. When it
cannot run that check, it says builder readiness is not established,
and the launched seat's own writes are the evidence. A review seat gets
none of these writes. A builder reports a failed push and the caller
pushes. The Grok builder posture is ADR-0075 D3:
`--always-approve --sandbox cf-guard-worktree`, launched in its own task
worktree after a separate step saves folder trust for that exact directory,
with readiness judged by the hooks that load. ADR-0075 still lists as
unestablished a Grok trust-only form and whether Grok resolves that
profile's `../../.git` entry, so the route is not qualified: until a canary
proves both, building goes to a Claude or Codex seat. A Grok seat never
launches with `--trust` from an agent, because Grok's project trust is the
operator's (First-run prompts). A consult or review edits nothing whatever
its mode. Never `--dangerously-skip-permissions` unless the operator named
it, and never `--dangerously-bypass-hook-trust`.

### Account profiles

A project can keep an untracked profile, `.codeflow/local/accounts.toml`
(or `accounts.toml` in the user's CodeFlow home), that maps a family to a
config home the operator has already signed in to. The file holds folder
paths, the name of an API-key variable and a spend cap, never a token;
`codeflow models launch-env` and `codeflow models limits` refuse a file
that names one. Every managed launch of a seat sets that family's own
variable and no other, in the seat's shell before its CLI starts:

| Family | Variable |
|---|---|
| Claude | `CLAUDE_CONFIG_DIR` |
| Codex | `CODEX_HOME` |
| Grok | `GROK_HOME` |

```bash
home_line=$(codeflow models launch-env --family <claude|codex|grok>) || exit 1
eval "$home_line" || exit 1
codeflow models launch-env --family <claude|codex|grok> --check || exit 1
```

Run it in the shell that starts the seat, before its CLI, and start the CLI
only after all three lines pass. The line is `export VARIABLE='folder'`
when the profile maps a home for the family, and `unset VARIABLE` when it
does not (no profile, or no entry for that family), so a home inherited from
the caller never reaches a seat the profile does not map. `--check` then
fails unless the shell holds what the profile says. A refused profile exits
2 and prints its reason on stderr; the first `|| exit 1` stops the launch
itself, because starting the seat anyway would run it under the CLI's
default or an inherited account. CodeFlow sets or unsets only the launching
family's variable; the other two families' variables are inherited
unchanged. A tracked Claude seat also passes the folder to
`codeflow delegate init --config-home <dir>`, so each turn's provenance
names it. CodeFlow never signs in, copies or moves a sign-in, or switches a
seat between accounts: choosing another account is the operator's, and
rotating one person's subscriptions to get past a limit is outside what
CodeFlow does. `codeflow models limits` shows each family's account and
limit state (5-hour and weekly windows, or `not exposed`).

### First-run prompts

A new seat can stop at a folder trust prompt, a hook trust prompt or a
self-update offer before its first turn. The caller answers folder trust
only for the task's own folder (`cf-method/references/autonomy.md`, "Trust
prompts"), and skips a self-update offer. Hook trust is the operator's
(ADR-0075, amendment of 2026-10-03): the caller never answers a hook trust
prompt, a Grok project trust prompt included. It tells the operator the seat
is waiting in its own surface, its Herdr tab or, under the fallback, its tmux
pane, and briefs it only after the operator answers. With that report it may
say, as information that grants nothing, whether each project hook file
matches the fetched target tip (`git show <tip>:<path> | cmp - <path>`) and
CodeFlow's managed copy under `.codeflow/.baseline/`. It never picks
"continue without trusting".
