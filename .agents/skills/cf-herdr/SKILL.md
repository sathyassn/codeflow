---
name: cf-herdr
description: Host CodeFlow consults and delegates in Herdr tabs without hijacking existing panes. Use from any host that reaches a running Herdr server, inside a Herdr pane or outside one, when starting, resuming, or closing a Claude, Codex, or Grok seat for a dual-lineage review, consult, or delegate. Use for Herdr workspaces, tabs, panes, agent names, a new seat's first-run prompts, harvesting a reply, follow-up on the same tab, and cleanup of self-created tabs. Do not treat Herdr idle/done as turn completion; do not split or type into the caller's pane.
---

# cf-herdr: Herdr host for CodeFlow seats

Herdr is the **visible terminal host** of every cross-family seat whenever a
Herdr server is reachable, from any host, inside a Herdr pane or outside
one. Which seat runs, its fallbacks and its launch flags are
[cross-family transport](../cf-model-orchestrator/resources/routing/transport.md).
Official Herdr CLI syntax is the authority (`herdr --help`; group help
without a mutating subcommand). This skill is the CodeFlow overlay: **tabs**,
**naming**, **anti-hijack**, **resume**, **first-run prompts**, **harvest**
and **cleanup**.

`cf-consult` and `cf-delegate` own *what* a consult or handoff is. This skill
owns *where the TTY lives*. Schema-v2 `codeflow delegate` still judges Claude
turn completion (arm / accepted / terminal). Herdr `idle` or `done` is **not**
completion.

## When this skill applies

1. Check the server: `herdr status server` reports `status: running`. If no
   server is reachable, **stop driving Herdr**: say so, use `cf-delegate`'s
   tmux lifecycle, the last fallback, and label that path **degraded**.
   `HERDR_ENV=1` only marks a caller inside a Herdr pane.
2. From a sandboxed host, the Herdr socket can sit outside the sandbox: run
   `herdr` through the session's permitted unsandboxed retry for a trusted
   installed tool, never by changing sandbox or permission settings.
3. Never `herdr server stop`. Never close a workspace, tab, or pane this run
   did not create. Never send keys to `$HERDR_PANE_ID` (the caller), or to
   any pane this run did not create.

Load the official Herdr skill, if present, for CLI details; where it defaults
to a **split** in the current tab, prefer a **new tab** (or resume one this
work owns).

## Discover before touching anything

Parse JSON; never invent IDs from sidebar order. Live Herdr state is the
only index of seats; keep no cache of it.

```bash
herdr workspace list
herdr tab list --workspace "$WS"
herdr pane list --workspace "$WS"
herdr agent list
```

`$WS` is `$HERDR_WORKSPACE_ID` inside a Herdr pane. Outside one, it is the
workspace whose panes work in this project (`herdr pane list` shows each
pane's `cwd`); create that workspace with
`herdr workspace create --cwd "$PWD" --label <repo> --no-focus` only when
none exists, and never use another repository's workspace.

Protected: the caller pane (`$HERDR_PANE_ID` inside Herdr); every tab and
pane this run did not create, **including** other `cf-` agents, except a
same-cwd follow-up this work owns. The `cf-` prefix is a name namespace, not
permission to touch. Never send keys to an agent whose pane `cwd` is not the
intended worktree. `--no-focus` on create/split so the operator stays on
their pane.

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

**Create** otherwise in the project's workspace:

```bash
created=$(herdr tab create --workspace "$WS" \
  --label "cf/<repo>/<work>/<kind>/<nn>" --cwd "$PWD" --no-focus)
pane_id=$(printf '%s' "$created" | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
# Tracked Claude only: set after shell init in its dedicated pane; verify `1`.
herdr pane run "$pane_id" 'export CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1; echo $CLAUDE_CODE_DISABLE_BACKGROUND_TASKS'
herdr agent start "cf-<repo>-<work>-<k><nn>" --kind <claude|codex|grok> \
  --pane "$pane_id" -- <native-args>
```

Native args after `--` are the ensemble's primary selector and effort, then
the seat's posture flags from the transport table; workers own escalation.
Close the dedicated tracked-Claude pane after its lifecycle ends.

- Claude: `--model <selector> --effort <effort> <posture flags>`
- Codex: `--model <selector> -c model_reasoning_effort="<effort>" <posture flags>`
  (`codex app-server daemon start` first if the daemon is not running)
- Grok: `--model <selector> --reasoning-effort <effort> <posture flags>`

Herdr is not an external sandbox. The posture table is the boundary. When
`herdr agent start --kind grok` reports `agent_name_not_found` although the
seat started, address that seat by its pane id. No third-party Grok Codex
plugins.

Prompt rules: consults edit nothing; no force-push or rebase of a shared
branch; no merge of protected main; no `herdr server stop`; no keys to the
caller pane; no closing tabs this run did not create.

Wait until the agent is ready. Split a pane only for a same-tab log/server
sibling. A Claude seat keeps schema-v2: start that Claude process in its
Herdr pane, and its lifecycle records remain the completion signal.

## First-run prompts

A new seat can stop at folder trust, then project hook trust (Grok:
`/hooks-trust` or `--trust`), or a self-update offer; delivery exits 5
until they are answered. Folder trust:
Trust this task's project/worktree or this run's sample; ask for others (`autonomy.md`).
Hook trust: only for hooks byte-identical to the target tip's; else tell
the operator the seat waits. Skip a self-update offer. The transport
rule's "First-run prompts" section is the authority.

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
256 KiB (exit 2: `send-text` is argv and can `E2BIG`, so write the brief to
a file outside the repository that the seat can read, and deliver a short
prompt that names it),
or, without the hook, the seat is `working`, `blocked` or `unknown` (exit
5). It then confirms within 20 s that the seat reached
`working` or `blocked` (or a newer `done`), else exits 4 naming the pane. It
sends one Enter only: Herdr cannot tell the input from the scrollback.
Inspect with `herdr agent read`; never resend blindly. `--lifecycle` adds
the fold sentence the hook requires when the paste folded (see
cf-delegate); the `accepted` wait stays the check.

Do not `tmux load-buffer` / `paste-buffer` into a Herdr pane, and do not use
`herdr agent prompt`: submission alone does not prove a started turn. Prove
the Herdr path with the same lifecycle canary as tmux.

## Harvest, review and consult seats

When a Codex or Grok seat's turn has started, or before a cross-family
review or consult runs in Herdr, read
[review and harvest](references/review-and-harvest.md): the reply harvest,
its provenance, and the one way a review seat runs.

## Cleanup

same topic / same review / follow-up: **resume this tab**. Do not mint a new
pane for another turn of the same work. A **new topic** gets a new tab.

After harvest, when that work is fully done and no follow-up is planned (for
a review seat, once its unit lands): record any native resume id, then close
**only** the tab this run created. Leave
blocked or working agents. Never close the caller tab.

Before removing a worktree, check `herdr agent list`: keep a worktree that a
live seat uses as its folder until that seat's tab is closed. A seat whose
folder is gone cannot start turns.
