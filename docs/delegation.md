# Delegation and Herdr

<!-- Guide layer. Reader: an operator who wants a second model's opinion or
     work. Sources: docs/capabilities.md (cross-vendor-delegation,
     transport-neutral-delegate-lifecycle),
     docs/capabilities/CAP-010-duo-model-orchestration.md, docs/adoption.md
     (Delegation quickstart), cf-consult, cf-herdr and cf-delegate skills,
     crates/codeflow-cli/src/cmd/delegate.rs, crates/codeflow-core/src/delegate.rs,
     crates/codeflow-cli/src/cmd/hook.rs. -->

## Concept

Consult and delegate let your agent bring in a model from another vendor,
either for an opinion or for a piece of work.

- A consult is read-only, so the peer reads, tests and reasons, then ends its
  reply with a verdict line that your agent checks against its own analysis.
- A delegate gets a scoped piece of implementation and does it in its own
  worktree on a feature branch. CodeFlow's gates judge its commits exactly as
  they judge yours.

Use them when you want an independent check or parallel work. Each model runs
in its own interactive session under your own login.

- Consult and delegate never run a peer headless, so `codex exec` and
  `claude -p` are prohibited.
- They never log in for you.
- They never count a same-vendor model as an independent reviewer.

## Architecture

Every peer runs as its own interactive CLI in a Herdr tab, whichever host
you start in, and every route ends in the same gates. The rule is stated once
in `cf-model-orchestrator/resources/routing/transport.md`.

- **A Codex peer** runs the interactive Codex CLI, on the local Codex
  app-server when it runs. A reply counts as Codex only with its native
  session id.
- **A Claude peer** runs the interactive `claude` CLI, and CodeFlow's
  delegate lifecycle proves each turn started, was accepted and ended.
- **A Grok peer** runs the interactive Grok Build CLI.
- **Fallbacks:** from Claude Code, the official Codex plugin is an optional
  fallback for a Codex peer; tmux is the last fallback, only when no Herdr
  server is reachable.
- **Herdr** is a terminal host with named tabs. Each peer gets its own tab
  labelled `cf/<repo>/<work>/<kind>/<nn>`, where the kind is `claude`, `codex`
  or `grok`. Your agent never types into your pane and never closes a tab it
  did not create.
- A Herdr `idle` or `done` status is not turn completion. Completion comes
  from the seat's confirmed turn and its native session, or from the
  lifecycle records.
- CodeFlow never launches a harness or delivers a prompt itself. It writes the
  lifecycle records and waits on them, and the host does the rest.
- A delegate edits only in a worktree on a feature branch. The git hooks bind
  it like any agent, and a human lands its work.

The [command reference](cli.md) lists every delegate flag, and the
[adoption guide](adoption.md) covers the one-time setup.

## Technical

The delegate lifecycle keeps owner-only records for one run in a state
directory outside every Git worktree, and each record marks one state of a
turn. A turn that goes well follows this path.

```text
Initialized --> Ready --> Armed --> Accepted --+--> Completed (Stop hook)
                                               |
                                               +--> Failed (StopFailure hook)
```

The table lists every state, including the poisoned, timed-out and rejected
cases that leave the path.

| State | Record in the state directory | What moves the turn here | What the host runs |
|---|---|---|---|
| Initialized | `settings.json` and `turns/` | `codeflow delegate init --run-id <ID> --state-dir <DIR>` | Launches `claude --settings <DIR>/settings.json`; the file wires `SessionStart`, `UserPromptSubmit`, `Stop` and `StopFailure` to `codeflow hook delegate-turn --run-id <ID> --state-dir <DIR>` |
| Ready | `ready.json` | A `SessionStart` hook with source `startup` | `codeflow delegate wait --run-id <ID> --state-dir <DIR> --until ready --timeout-seconds <N>` |
| Armed | `turns/<turn>/request.json`, holding the prompt's SHA-256 | `codeflow delegate arm --run-id <ID> --state-dir <DIR> --turn-id <TURN> --prompt-file <FILE>` | Pastes the same file into the session, then presses Enter |
| Accepted | `turns/<turn>/accepted.json` | A `UserPromptSubmit` hook whose prompt matches the armed digest | `codeflow delegate wait ... --until accepted --turn-id <TURN> --timeout-seconds <N>` |
| Completed | `turns/<turn>/result.json` with status `completed` and the last assistant message | A `Stop` hook | `codeflow delegate wait ... --until terminal --turn-id <TURN> --timeout-seconds <N>` exits 0 and prints the record |
| Failed | `turns/<turn>/result.json` with status `failed` and the bounded error | A `StopFailure` hook | The terminal wait exits 10 and prints the record |
| Poisoned | `poison.json` | A restarted, resumed or compacted session, an event that matches no accepted turn, or a wait interrupted after acceptance | Any wait exits 11. Start again with a new run id in a fresh directory |
| Timed out or interrupted | no new record | The wait deadline passes, or the waiter gets Ctrl+C | The wait exits 124 on timeout and 130 on interrupt |
| Hook rejected | no new record | `codeflow hook delegate-turn` cannot accept an event | The hook exits 2 to block a rejected prompt or unreadable input, and 1 on other failures |

### Consult or delegate once

Ask your agent for a consult or a delegate in plain words, and it runs the
matching skill for the host you are in.

1. Use a standard or full tier project. The minimal tier does not install
   these skills.
2. Check the prerequisites with `codeflow doctor --check delegates`, and the
   lifecycle itself with `codeflow doctor --check delegate-roundtrip`.
3. Log in yourself with `codex login`, and log in to `claude` in its own
   session. CodeFlow never automates login.
4. Run a Herdr server, which hosts each peer in a tab. The official Codex
   plugin for Claude Code is optional.
5. For an opinion, run `/cf-consult` and name the paths, diff or question and
   the criteria to judge by.
6. For work, start a worktree on a feature branch first, then run
   `/cf-delegate` scoped to it. The peer's tab works in that worktree.
7. Read your agent's synthesis. It lists where it agrees and disagrees with
   the peer, each point backed by its own evidence.

Check the result:

- A consult worked when the reply ends in `VERDICT: approved` or
  `VERDICT: changes_requested` with a Codex thread id or lifecycle records
  behind it and the worktree diff unchanged.
- A delegate worked when its commits sit on the feature branch and pass the
  same gates.

### Host a peer in Herdr

When a Herdr server is running, your agent hosts the peer in a new named tab
in your project's workspace, whether the agent itself runs inside Herdr or
not.

1. Check that `herdr status server` reports `status: running`. With no
   server reachable, the agent uses tmux and reports that path as degraded.
2. Ask for the consult or delegate. The agent first lists what exists with
   `herdr workspace list`, `herdr tab list --workspace "$WS"` and
   `herdr agent list`, where `$WS` is the project's workspace.
3. It creates the tab without taking focus with
   `herdr tab create --workspace "$WS" --label "cf/<repo>/<work>/<kind>/<nn>" --cwd "$PWD" --no-focus`.
4. It starts the peer with
   `herdr agent start "cf-<repo>-<work>-<k><nn>" --kind <claude|codex|grok> --pane <pane-id> -- <native-args>`.
5. A new peer can first ask to trust the folder or the project's hooks.
   The agent answers folder trust for the task's own folder only and skips
   any update offer. Hook trust stays yours
   ([ADR-0075's 2026-10-03 amendment](decisions/ADR-0075-agent-sessions-refuse-instead-of-prompting-under.md)):
   every hook prompt, and every Grok trust prompt, waits for you in the
   seat's Herdr tab, or its tmux pane under the fallback. The agent may tell
   you whether the hook files match the target and the managed copy; that
   is information, not a grant.
6. It delivers the prompt with the `cf-herdr` delivery script, which sends
   the exact file and confirms the turn started. For a Claude peer it arms
   the prompt first and then waits with
   `codeflow delegate wait ... --until terminal`.
7. A follow-up on the same work reuses the same tab. When the work is done,
   the agent closes only the tab it created.

Hosting worked when:

- a `cf/` tab appears beside yours with the peer running
- your own pane keeps focus
- the agent reports the tab label, agent name and the peer's native thread or
  session id

### Run a family under another account and read its limits

A long run can stop when one family's subscription reaches its 5-hour or
weekly limit. CodeFlow does not switch accounts or move a sign-in. It lets a
project say which folder each family's seats start under, and it shows the
limit state each CLI exposes.

1. Sign in to the second account yourself, in its own config home (for
   example `CODEX_HOME=~/.codex-second codex login`).
2. Write the untracked profile `.codeflow/local/accounts.toml` in the main
   checkout (or `accounts.toml` in your CodeFlow home). It holds folder
   paths, the name of an API-key variable and a spend cap, never a token:

   ```toml
   schema_version = 1

   [families.codex]
   config_home = "~/.codex-second"

   [families.codex.api_fallback]
   key_env = "CODEX_FALLBACK_API_KEY"   # the variable's name, never its value
   spend_cap_usd = 25
   ```

   The spend cap is a figure the profile records and `models limits`
   shows. CodeFlow does not enforce it or read the key.

   A key whose name says token, secret or credential, a value shaped like a
   secret, an unknown key, a relative path, a config home that is not a
   folder, and a config home that is or links into the project (the main
   checkout or any linked worktree) outside a real folder under
   `.codeflow/local/` each refuse the whole file. A parse error names a line
   and column, never the text. The managed `.gitignore` keeps
   `.codeflow/local/` untracked.
3. The agent launches each seat with only that family's variable
   (`CLAUDE_CONFIG_DIR`, `CODEX_HOME` or `GROK_HOME`) by running, in the
   seat's shell before its CLI starts:

   ```bash
   home_line=$(codeflow models launch-env --family <family>) || exit 1
   eval "$home_line" || exit 1
   codeflow models launch-env --family <family> --check || exit 1
   ```

   The line is `export VARIABLE='folder'` for a mapped family and
   `unset VARIABLE` for one the profile does not map, so a home inherited
   from the caller never reaches that seat. `--check` fails unless the shell
   holds what the profile says. A refused profile exits 2 with the reason and
   the first `|| exit 1` stops the launch itself, so a seat never starts under
   the default or an inherited account by mistake. The other two families'
   variables are not touched. In a Herdr seat the same three steps run in the
   pane, and the launch waits for a fresh acknowledgement before
   `herdr agent start` (`cf-herdr`, "Create or resume"). The acknowledgement
   carries a random value made for that launch and is printed only when
   `--check` passes, so a path, an echoed command, an error message or output
   left in the pane by an earlier launch can never stand for it.
4. `codeflow models limits` prints each family's account and limit state.
   Readings arrive from what each CLI already exposes:

   | Family | Source | How it reaches CodeFlow |
   |---|---|---|
   | Claude Code | statusline JSON `rate_limits.five_hour` and `seven_day` (Pro, Max and Ultra plans only, after the first response) | the statusline script pipes its input to `codeflow models record-limits --family claude` |
   | Codex | app-server `account/rateLimits/read` result and `account/rateLimits/updated` event | the payload piped to `codeflow models record-limits --family codex` |
   | Grok Build | no usage reading; only the limit-hit error | `codeflow models record-limits --family grok --limit-hit [--resets-at EPOCH]` |

   Claude Code runs the statusline command on every refresh, so a script
   that already reads the JSON from standard input adds one line:

   ```bash
   input=$(cat)
   printf '%s' "$input" | codeflow models record-limits --family claude >/dev/null 2>&1
   ```

   Codex's `rateLimits` is its default quota bucket; a payload that lists
   other buckets only under `rateLimitsByLimitId` is read for the default
   one. The Codex window names come from the installed release's own schema,
   not vendor documentation, so a payload the reader cannot parse is refused
   with a message and nothing is stored.

   A reading belongs to the config home of the process that recorded it
   (`record-limits --config-home DIR`, else the family's variable in that
   process, else the CLI's default home). When the profile later names
   another home, the old reading is not shown as that account's: the family
   shows `not exposed` and a note names the homes that have readings. A file
   written by the earlier build (version 1) names no home, so its readings
   belong to no account: they are ignored, the view says so, and the next
   `record-limits` for that family replaces the file. Nothing is carried over
   to any home.

   A window with no reading shows `not exposed`; a window that reset after
   its reading shows that, with the old figure. Choosing a route from these
   readings is TSK-432's work, not this command's.
