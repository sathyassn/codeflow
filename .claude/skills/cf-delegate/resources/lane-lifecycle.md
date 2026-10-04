# cf-delegate lane: a Claude seat, the durable lifecycle over the interactive claude CLI

Load this lane when the seat is Claude and the host is Codex or Grok, after
the common core in `../SKILL.md`. A Claude host does not load it.

## Preflight

The lifecycle owns startup, acceptance, and terminal correlation. Run that
Claude process where
[cross-family transport](../../cf-model-orchestrator/resources/routing/transport.md)
puts it: a named Herdr tab through `cf-herdr` whenever a Herdr server is
reachable, from any host, and tmux only as the last fallback. Herdr
`idle`/`done` is not turn completion. The account and interactive response
must be verified with a scoped TTY canary; verify the full lifecycle round
trip on install and whenever the Claude CLI or hook configuration changes.

`claude` on PATH, `claude mcp list` succeeds, and a short interactive TTY
canary gets an authenticated response. With a reachable Herdr server
(`herdr status server`), require `herdr` and load `cf-herdr`; require `tmux`
only when no Herdr server is reachable. Do not infer authentication from a status
subcommand when it conflicts with a working interactive session.

## Lifecycle

CodeFlow's schema-v2 lifecycle proves what a terminal signal alone cannot:
the session started cleanly, the delivered prompt was accepted as the armed
turn, and the terminal event belongs to that turn.

On this Codex host lane, before launching Claude, read and follow the
shipped [turn lifecycle adapter](claude-turn-completion.md), which states
the launch sequence, turn detection and the sibling Stop-hook preflight;
never improvise a parser, scrape transcripts, or use pane stability as
completion.

In a Herdr tab, launch per `cf-herdr` in place of the adapter's tmux
session (launch-local task environment, `--settings`, model/effort and the
Claude row of the transport posture table); deliver the armed file with
`herdr pane send-text` as that skill names, never `tmux load-buffer`.
Lifecycle waits stay the completion signal.

- **Pane access is diagnosis-only.** Capture only the dedicated task pane,
  and only for bounded diagnosis when a wait times out or a result is
  malformed, to answer an explicit in-turn dialog, or once after a paste to
  see whether the input shows a paste attachment. Never enumerate or
  capture unrelated tmux sessions; they may contain secrets or other users'
  work.
- **Effective autonomy is layered:** invoke the Claude primary with the selector
  and default effort from
  `../../cf-model-orchestrator/resources/current-ensemble.json`. The primary owns
  the native session, internal worker routing, interpretation, and judgment.
  Launch with default effort and the generated task settings; workers take
  escalation. Make
  `autoMode.classifyAllShell` effective at user scope (Claude ignores it at
  project scope, and repeated `--settings` flags are not a supported merge
  contract; the generated task file carries only the lifecycle hooks). On
  every host, keep the OS sandbox
  enabled with `sandbox.failIfUnavailable: true`, auto-allow sandboxed Bash,
  and permit an auto-classified unsandboxed retry only for a trusted
  installed tool that requires host state. That is not a
  write grant and not hook-trust bypass. Never
  `--dangerously-skip-permissions` unless the operator named it, and never
  `--dangerously-bypass-hook-trust`. Consults still edit nothing. See
  <https://code.claude.com/docs/en/permission-modes> and
  <https://code.claude.com/docs/en/sandboxing>.
- **Read-only consults:** keep "read and reason only; edit nothing" in the
  prompt, record the worktree state before launch, and compare the diff after
  completion. Auto mode enables useful inspection and test tools; it does not
  silently turn a consult into an edit handoff. If stronger write isolation is
  required, use a separate read-only checkout or filesystem boundary.
- **Edit handoff:** start a separate session in the transport table's
  production and building posture, with explicit write authority and the
  worktree as its working directory so edits and commits land where the gates
  guard them. Never reuse a consult session as an implicit write grant.
- **Test-running review:** use a separate interactive session in the
  transport table's consult and review posture with the same fail-closed
  sandbox. Instruct Claude to edit no source files and
  require a clean before/after worktree-diff comparison. If a fix is needed,
  return it to the task's responsible primary and designated executor.
- **Interactive prompts:** classifier escalations, ambiguity, and other user
  questions are handled in the same dedicated session. They never authorize a
  write silently, and a visible dialog never substitutes for the terminal
  lifecycle result.
- **Cleanup:** after harvesting the bounded result and the evidence
  verification needs, kill the task session and remove the state directory
  and private prompt files.

## Evidence on this lane

Launch is `wait --until ready`. Provenance is the schema-v2 records binding
session, digest, and `prompt_id`; the terminal `wait` result already carries
`provenance`: the thread (the Claude session), and model and effort as
`requested` at `init` and `observed` at session start, each `unknown` when not
given. Cite that record; do not record these by hand. Failure is a stable exit
state or durable poison, then one bounded retry with diagnosis in a fresh run.
Recheck is the durable state records until cleanup.
