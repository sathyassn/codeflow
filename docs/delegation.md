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

A consult is read-only, so the peer reads, tests and reasons, then ends its
reply with a verdict line that your agent checks against its own analysis. A
delegate gets a scoped piece of implementation and does it in its own worktree
on a feature branch, where CodeFlow's gates judge its commits exactly as they
judge yours. This is for an operator who wants an independent check or
parallel work, with each model running in its own interactive session under
your own login. It never runs a peer headless, so `codex exec` and `claude -p`
are prohibited, and it never logs in for you or counts a same-vendor model as
an independent reviewer.

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
turn.

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

A consult worked when the reply ends in `VERDICT: approved` or
`VERDICT: changes_requested` with a Codex thread id or lifecycle records behind
it and the worktree diff unchanged, and a delegate worked when its commits sit
on the feature branch and pass the same gates.

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
   any update offer. It answers hook trust only in the narrow Codex case of
   [ADR-0075's 2026-10-03 amendment](decisions/ADR-0075-agent-sessions-refuse-instead-of-prompting-under.md):
   an unchanged hooks file that runs only CodeFlow's shipped commands. Any
   other hook prompt, and every Grok trust prompt, waits for you in the
   seat's Herdr tab, or its tmux pane under the fallback.
6. It delivers the prompt with the `cf-herdr` delivery script, which sends
   the exact file and confirms the turn started. For a Claude peer it arms
   the prompt first and then waits with
   `codeflow delegate wait ... --until terminal`.
7. A follow-up on the same work reuses the same tab. When the work is done,
   the agent closes only the tab it created.

It worked when a `cf/` tab appears beside yours with the peer running, your
own pane keeps focus, and the agent reports the tab label, agent name and the
peer's native thread or session id.
