---
name: cf-herdr
description: Host CodeFlow consults and delegates in Herdr tabs without hijacking existing panes. Use when HERDR_ENV=1 and starting, resuming, or closing Claude, Codex, or another coding agent for a dual-lineage review, consult, or delegate TTY. Use for Herdr workspaces, tabs, panes, agent names, follow-up on the same tab, and cleanup of self-created tabs. Do not use from outside Herdr; do not treat Herdr idle/done as turn completion; do not split the caller pane by default.
---

# cf-herdr — Herdr-primary TTY for CodeFlow seats

Herdr is the **visible terminal host** when this agent runs inside it. Official
Herdr CLI syntax is the authority (`herdr --help`; group help without a
mutating subcommand). This skill is the CodeFlow overlay: **tabs**, **naming**,
**anti-hijack**, **resume**, and **cleanup**.

`cf-consult` and `cf-delegate` own *what* a consult or handoff is. This skill
owns *where the TTY lives*. Schema-v2 `codeflow delegate` still judges Claude
turn completion (arm / accepted / terminal). Herdr `idle` or `done` is **not**
completion.

## When this skill applies

1. Require `test "${HERDR_ENV:-}" = 1`. If that fails, **stop driving Herdr**.
   Say you are outside a Herdr pane. Use the tmux lifecycle in `cf-delegate`
   and label that path **degraded**. Do not inspect or control Herdr from
   outside.
2. If Herdr is absent, same degraded tmux path.
3. Never `herdr server stop`. Never close a workspace, tab, or pane this run
   did not create. Never send keys to `$HERDR_PANE_ID` (the caller).

Load the official Herdr skill if present for CLI details. Where it defaults to
a sibling **split** in the current tab, CodeFlow overrides: prefer a **new
tab** (or resume one this work already owns).

## Discover before touching anything

Parse JSON; never invent IDs from sidebar order.

```bash
herdr workspace list
herdr tab list --workspace "$HERDR_WORKSPACE_ID"
herdr pane list --workspace "$HERDR_WORKSPACE_ID"
herdr agent list
herdr pane current --current
```

Protected: the caller pane (`$HERDR_PANE_ID`); any pane whose live agent name
does not start with `cf-`; any tab whose label is not in this run's created
set. `--no-focus` on create/split so the operator stays on their pane.

## Naming

Tab label (human, unique):

```text
cf/<repo>/<work>/<kind>/<nn>
```

Agent name (live unique, `[a-z][a-z0-9_-]{0,31}`):

```text
cf-<repo>-<work>-<k><nn>
```

| Part | Rule |
|---|---|
| repo | cwd basename, kebab, max 8 |
| work | `tsk014` if a task id is in play, else kebab of the work, max 12 |
| kind / k | `claude`/`cl`, `codex`/`cx`, `grok`/`gk` |
| nn | next free `01`–`99` among **live** agents with the same `cf-<repo>-<work>-<k>` prefix |

Examples: `cf-codeflow-skills-rev-cl01`, parallel Claude `…-cl02`, Codex on
the same work `…-cx01`.

List live agents first; increment `nn` until free. Never reuse a live name.
Never steal a name that does not start with `cf-`.

## Create or resume

**Resume** when a tab labeled `cf/<repo>/<work>/<kind>/<nn>` still exists and
its agent is idle or done: prompt that agent. Do not mint `…-cl02` for a
follow-up of the same work unless the first agent is gone or poisoned.

**Create** a named tab otherwise, on the **existing repo workspace** (create a
workspace only when none exists for this cwd):

```bash
created=$(herdr tab create --workspace "$HERDR_WORKSPACE_ID" \
  --label "cf/<repo>/<work>/<kind>/<nn>" --cwd "$PWD" --no-focus)
pane_id=$(printf '%s' "$created" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
herdr agent start "cf-<repo>-<work>-<k><nn>" --kind <claude|codex> \
  --pane "$pane_id" -- --model <selector> --effort <effort>
```

Pass native args after `--`. Wait until the agent is ready for input. Split a
pane only when the **same** tab needs a log or server sibling — not as the
default for a second model.

From a Grok or other non-Claude/non-Codex host, a visible `herdr agent start
--kind codex` (or `claude`) is the interactive seat. From Claude Code, Codex
still uses the official plugin. From Codex, Claude still uses schema-v2
lifecycle; when `HERDR_ENV=1`, **start that Claude process in the Herdr pane**
instead of a detached tmux session, then deliver the armed prompt into that
pane. Lifecycle records remain the completion signal.

## Cache (optional, not a record to maintain)

Live Herdr names are the index. Optional cache:

`${CODEFLOW_HOME:-$HOME/.codeflow}/herdr-runs/<repo>.json`

Owner-only. Never commit. Never put it in `docs/` or `project-management/`.
Write a row on create (tab id, pane id, agent name, native session/thread if
observed). Delete the row when the tab closes. On every use, drop rows whose
tab or pane is gone from `herdr pane list` / `tab list`. Missing file: list
Herdr. If cache and Herdr disagree, **Herdr wins**.

## Cleanup

After harvest, if no follow-up is planned and the operator did not ask to
keep the tab: record any native `claude --resume` / `codex resume` id, then
close **only** the tab this run created. Leave blocked or working agents.
Never close the caller tab.

## Completion

Report tab label, agent name, pane id, requested vs observed model/effort,
native thread/session id when exposed, and whether the tab was kept or closed.
A consult still needs `cf-consult` synthesis and a `VERDICT` line.
