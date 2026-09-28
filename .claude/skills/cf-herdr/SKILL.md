---
name: cf-herdr
description: Host CodeFlow consults and delegates in Herdr tabs without hijacking existing panes. Use when HERDR_ENV=1 and starting, resuming, or closing Claude, Codex, or another coding agent for a dual-lineage review, consult, or delegate TTY. Use for Herdr workspaces, tabs, panes, agent names, follow-up on the same tab, and cleanup of self-created tabs. Do not use from outside Herdr; do not treat Herdr idle/done as turn completion; do not split the caller pane by default.
---

# cf-herdr — Herdr-primary TTY for CodeFlow seats

Herdr is the **visible terminal host** when this agent runs inside it. Official
Herdr CLI syntax is the authority (`herdr --help`; group help without a
mutating subcommand). This skill is the CodeFlow overlay: **tabs**, **naming**,
**anti-hijack**, **resume**, **unattended launch**, and **cleanup**.

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

Protected: the caller pane (`$HERDR_PANE_ID`); every tab and pane this run did
not create, **including** other `cf-` agents, except a same-cwd follow-up this
work owns. The `cf-` prefix is a name namespace, not permission to touch.
Never send keys to an agent whose pane `cwd` is not the intended worktree.
`--no-focus` on create/split so the operator stays on their pane.

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

Examples: `cf-codeflow-skills-rev-cl01`, a parallel Claude on the same work
`cf-codeflow-skills-rev-cl02`, and Codex on the same work
`cf-codeflow-skills-rev-cx01`.

Repo slug truncation collides across worktrees of the same project. The
**intended worktree `cwd`** is the disambiguator, not the label. Put full cwd,
repo basename, and work identity in the optional cache.

List live agents first; increment `nn` until free. Never reuse a live name.
Never steal a name that does not start with `cf-`.

## Create or resume

**Resume** only a same-work tab: its label matches
`cf/<repo>/<work>/<kind>/<nn>`, pane `cwd` is `$PWD`, this run created it or it
is a same-cwd follow-up, and its prior turn was harvested (lifecycle terminal
or native thread result). Idle/done after harvest permits the *next* prompt;
it does not prove prior completion. Never prompt a different-cwd `cf-…` agent
or mint `…-cl02` for a same-work follow-up unless the first is gone/poisoned.

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
never + `danger-full-access`; Grok `--always-approve`. Consult / no-edit review:
Claude `--permission-mode auto` (never bypass); Codex `--ask-for-approval
on-request --sandbox workspace-write`. Herdr is not an external sandbox.
Never `--dangerously-skip-permissions` unless the operator named it. Consults
still verify an empty worktree diff. No third-party Grok Codex plugins.

Prompt rules: consults edit nothing; no force-push or rebase of a shared
branch; no merge of protected main; no `herdr server stop`; no keys to the
caller pane; no closing tabs this run did not create.

Wait until the agent is ready. Split a pane only for a same-tab log/server
sibling. From a Grok or another qualified non-Claude, non-Codex host,
`herdr agent start --kind claude|codex` is the interactive seat. From Claude
Code, Codex still uses the official plugin. From Codex, Claude still uses
schema-v2; when `HERDR_ENV=1`, **start that Claude process in the Herdr
pane**. Lifecycle records remain the completion signal.

## Deliver an armed prompt

After `codeflow delegate arm`, send the same canonical UTF-8/LF file bytes:

```bash
herdr pane send-text "$pane_id" "$(cat "$P")"
sleep 2  # input settle; not completion detection
# Only if the input line shows a folded "[Pasted text" attachment; the hook
# then requires this exact sentence (see cf-delegate):
herdr pane send-text "$pane_id" "Carry out the pasted instructions."
sleep 0.3
herdr pane send-keys "$pane_id" Enter
codeflow delegate wait --run-id "$RUN" --state-dir "$STATE" \
  --until accepted --turn-id "$TURN" --timeout-seconds 120
codeflow delegate wait --run-id "$RUN" --state-dir "$STATE" \
  --until terminal --turn-id "$TURN" --timeout-seconds 3600
```

Do not `tmux load-buffer` / `paste-buffer` into a Herdr pane. `herdr agent
prompt` is for a consult that is not lifecycle-armed; it does not replace the
armed-file digest. If `"$P"` exceeds 256 KiB, use degraded tmux paste-buffer
(`send-text` is argv and can `E2BIG`). Prove the Herdr path with the same
lifecycle canary as tmux.

## Cache (optional, not a record to maintain)

Live Herdr names are the index. Optional cache:

`${CODEFLOW_HOME:-$HOME/.codeflow}/herdr-runs/<repo>.json`

Owner-only. Never commit. Never put it in `docs/` or `project-management/`.
Write a row on create (tab id, pane id, agent name, **cwd**, full repo
basename, work identity, native session/thread if observed). Delete the row
when the tab closes. On every use, drop rows whose tab or pane is gone from
`herdr pane list` / `tab list`. Missing file: list Herdr. If cache and Herdr
disagree, **Herdr wins**. Resume still requires live cwd match even when the
cache looks right.

## Cleanup

same topic / same review / follow-up: **resume this tab**. Do not mint a new
pane for another turn of the same work. A **new topic** gets a new tab.

After harvest, when that work is fully done and no follow-up is planned: record
any native resume id, then close **only** the tab this run created. Leave
blocked or working agents. Never close the caller tab.

## Completion

Report tab label, agent name, pane id, requested vs observed model/effort,
native thread/session id when exposed, and whether the tab was kept or closed.
A consult still needs `cf-consult` synthesis and a `VERDICT` line.
