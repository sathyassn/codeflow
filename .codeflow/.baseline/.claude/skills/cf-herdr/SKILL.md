---
name: cf-herdr
description: Host CodeFlow consults and delegates in Herdr tabs without hijacking existing panes. Use when HERDR_ENV=1 and starting, resuming, or closing Claude, Codex, or another coding agent for a dual-lineage review, consult, or delegate TTY. Use for Herdr workspaces, tabs, panes, agent names, follow-up on the same tab, and cleanup of self-created tabs. Do not use from outside Herdr; do not treat Herdr idle/done as turn completion; do not split the caller pane by default.
---

# cf-herdr: Herdr-primary TTY for CodeFlow seats

Herdr is the **visible terminal host** when this agent runs inside it. Official
Herdr CLI syntax is the authority (`herdr --help`; group help without a
mutating subcommand). This skill is the CodeFlow overlay: **tabs**, **naming**,
**anti-hijack**, **resume**, **unattended launch**, and **cleanup**.

`cf-consult` and `cf-delegate` own *what* a consult or handoff is. This skill
owns *where the TTY lives*. Schema-v2 `codeflow delegate` still judges Claude
turn completion (arm / accepted / terminal). Herdr `idle` or `done` is **not**
completion.

## When this skill applies

1. Require `test "${HERDR_ENV:-}" = 1`. If that fails, **stop driving Herdr**:
   say you are outside a Herdr pane, use `cf-delegate`'s tmux lifecycle,
   label that path **degraded**, and never read or drive Herdr from outside.
2. If Herdr is absent, same degraded tmux path.
3. Never `herdr server stop`. Never close a workspace, tab, or pane this run
   did not create. Never send keys to `$HERDR_PANE_ID` (the caller).

Load the official Herdr skill, if present, for CLI details; where it defaults
to a **split** in the current tab, prefer a **new tab** (or resume one this
work owns).

## Discover before touching anything

Parse JSON; never invent IDs from sidebar order. Live Herdr state is the
only index of seats; keep no cache of it.

```bash
herdr workspace list
herdr tab list --workspace "$HERDR_WORKSPACE_ID"
herdr pane list --workspace "$HERDR_WORKSPACE_ID"
herdr agent list
herdr pane current --current
```

Protected: the caller pane (`$HERDR_PANE_ID`); every tab and pane this run did
not create, **including** other `cf-` agents, except a same-cwd follow-up this
work owns. The `cf-` prefix is a name namespace, not permission to touch.
Never send keys to an agent whose pane `cwd` is not the intended worktree.
`--no-focus` on create/split so the operator stays on their pane.

## Naming

Tab label `cf/<repo>/<work>/<kind>/<nn>`; agent name
`cf-<repo>-<work>-<k><nn>` (`[a-z][a-z0-9_-]{0,31}`; `repo` the cwd
basename, at most 8 characters; `work` the task id such as `tsk014`, else a
kebab slug, at most 12; `k` is `cl`, `cx` or `gk`; `nn` the next free number
among live agents with that prefix), for
example `cf-codeflow-skills-rev-cx01`. List live agents first and never reuse
a live name; never take a name that does not start with `cf-`. Worktrees of
one project can share a name prefix, so the intended worktree `cwd`, not the
label, identifies the seat.

## Create or resume

**Resume** only a same-work tab: its label matches
`cf/<repo>/<work>/<kind>/<nn>`, pane `cwd` is `$PWD`, this run created it or it
is a same-cwd follow-up, and its prior turn was harvested (lifecycle terminal
or native thread result). Idle/done after harvest permits the *next* prompt;
it does not prove prior completion. Resume delivers through the same script,
which reports a seat whose folder is gone before sending. Never prompt a
different-cwd `cf-…` agent or mint `…-cl02` for a same-work follow-up unless
the first is gone/poisoned.

**Create** otherwise in the existing workspace for project `$PWD` (never
another repo); create that workspace only when none exists:

```bash
created=$(herdr tab create --workspace "$HERDR_WORKSPACE_ID" \
  --label "cf/<repo>/<work>/<kind>/<nn>" --cwd "$PWD" --no-focus)
pane_id=$(printf '%s' "$created" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
# Tracked Claude only: set after shell init in its dedicated pane; verify `1`.
herdr pane run "$pane_id" 'export CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1; echo $CLAUDE_CODE_DISABLE_BACKGROUND_TASKS'
herdr agent start "cf-<repo>-<work>-<k><nn>" --kind <claude|codex|grok> \
  --pane "$pane_id" -- <native-args>
```

Native args after `--` are kind-specific. Use the ensemble's primary
selector/effort; workers own escalation. Close the dedicated tracked-Claude
pane after its lifecycle ends.

- Claude: `--model <selector> --effort <effort> --permission-mode bypassPermissions`
- Codex: `--model <selector> -c model_reasoning_effort="<effort>" --ask-for-approval never --sandbox danger-full-access`
  (`codex app-server daemon start` if the socket is missing)
- Grok: `--model <selector> --reasoning-effort <effort> --always-approve`

Default production launch is ADR-conformant: Claude `bypassPermissions`; Codex
never + `danger-full-access` (ADR-0075 D1, until a `cf-builder` spike passes);
Grok `--always-approve`. Consult / no-edit review: Claude `--permission-mode
auto` (never bypass); Codex `--ask-for-approval never` with no `--sandbox`
flag, which selects the project's `cf-guard` profile (D2). Herdr is not an
external sandbox.
Never `--dangerously-skip-permissions` unless the operator named it. Consults
still verify an empty worktree diff. No third-party Grok Codex plugins.
Trust this task's project/worktree or this run's sample; ask for others (`autonomy.md`).

Prompt rules: consults edit nothing; no force-push or rebase of a shared
branch; no merge of protected main; no `herdr server stop`; no keys to the
caller pane; no closing tabs this run did not create.

Wait until the agent is ready. Split a pane only for a same-tab log/server
sibling. From a Grok or another qualified non-Claude, non-Codex host,
`herdr agent start --kind claude|codex` is the interactive seat. From Claude
Code, Codex still uses the official plugin. From Codex, Claude still uses
schema-v2; when `HERDR_ENV=1`, **start that Claude process in the Herdr
pane**. Lifecycle records remain the completion signal.

## Deliver a prompt

Deliver with the skill's script. It sends the file's exact UTF-8/LF bytes
with `herdr pane send-text`, then `herdr pane send-keys` Enter:

```bash
D=.agents/skills/cf-herdr/scripts/deliver.py
# Codex or Grok seat: confirms the turn started
python3 "$D" --pane "$pane_id" --file "$P"
# Tracked Claude, after `codeflow delegate arm`: the lifecycle confirms
python3 "$D" --pane "$pane_id" --file "$P" --lifecycle
codeflow delegate wait --run-id "$RUN" --state-dir "$STATE" \
  --until accepted --turn-id "$TURN" --timeout-seconds 120
codeflow delegate wait --run-id "$RUN" --state-dir "$STATE" \
  --until terminal --turn-id "$TURN" --timeout-seconds 3600
```

It sends nothing when the seat's folder is gone (exit 3), the file is over
256 KiB (exit 2: use degraded tmux paste-buffer, since `send-text` is argv
and can `E2BIG`), or, without the hook, the seat is `working`, `blocked` or
`unknown` (exit 5). It then confirms within 20 s that the seat reached
`working` or `blocked` (or a newer `done`), else exits 4 naming the pane. It
sends one Enter only: Herdr cannot tell the input from the scrollback.
Inspect with `herdr agent read`; never resend blindly. `--lifecycle` adds
the fold sentence the hook requires when the paste folded (see
cf-delegate); the `accepted` wait stays the check.

Do not `tmux load-buffer` / `paste-buffer` into a Herdr pane, and do not use
`herdr agent prompt`: submission alone does not prove a started turn. Prove
the Herdr path with the same lifecycle canary as tmux.

## Cleanup

same topic / same review / follow-up: **resume this tab**. Do not mint a new
pane for another turn of the same work. A **new topic** gets a new tab.

After harvest, when that work is fully done and no follow-up is planned: record
any native resume id, then close **only** the tab this run created. Leave
blocked or working agents. Never close the caller tab.

Before removing a worktree, check `herdr agent list`: keep a worktree that a
live seat uses as its folder until that seat's tab is closed. A seat whose
folder is gone cannot start turns.
