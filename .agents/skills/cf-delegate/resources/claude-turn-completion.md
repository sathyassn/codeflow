# Claude turn lifecycle adapter (schema v2)

Use this adapter for the codex-hosted, interactive Claude CLI lane. CodeFlow
owns the durable lifecycle records; the host launches the harness and delivers
the bytes. The protocol proves three things a terminal signal alone cannot:
the session started cleanly, the delivered prompt was accepted as the armed
turn, and the terminal event belongs to that turn. It never inspects
transcripts, and it makes zero tmux calls — waiting is pure file polling.

## Per-run setup

1. Choose a run id of 1–64 ASCII letters, digits, `.`, `_`, or `-`, and an
   absolute state-directory path **outside every Git worktree**.
2. `codeflow delegate init --run-id RUN --state-dir DIR` creates the owner-only
   (`0700`/`0600`) state directory and prints the generated
   `DIR/settings.json`, which wires `SessionStart`, `UserPromptSubmit`,
   `Stop`, and `StopFailure` to
   `codeflow hook delegate-turn --run-id RUN --state-dir DIR`.
3. **The generated settings file is immutable.** Every later `arm`, `wait`,
   and hook invocation regenerates the expected content from exactly
   (run id, state-dir spelling) and rejects any difference as unsafe. Never
   edit it or merge other keys into it. Production launches
   `--permission-mode bypassPermissions`. Consult and no-edit review keep
   `--permission-mode auto`. Make `autoMode.classifyAllShell` effective at
   user scope — Claude ignores it at project scope, and repeated `--settings`
   flags are not a supported composition mechanism — then prove the composed
   boundary with the preflight canary.

## Tracked synchronous task mode

For every schema-v2 Claude process, set
`CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` after its login shell initializes and
before Claude starts. Keep this environment launch-local: never add it to global
settings, generic project presets, or the immutable generated task settings.
Interactive fork mode otherwise forces subagents into the background and offers
no `run_in_background` control on the Agent tool. This environment keeps task
returns synchronous so the tracked turn can collect the actual child result.

The tradeoff is scoped to that Claude process: background Bash tasks, background
subagents, and Ctrl+B are disabled. Put servers or watchers in separate owned
panes; independent host-owned native sessions may still run in parallel. Before
relying on the lane, run a bounded named-child canary and observe its reviewer
result before that armed turn's `Stop`. A task notification, `UserPromptSubmit`,
backgrounded Agent, requested environment value, or launch string is not proof.

## Sibling Stop-hook preflight

Before launching, the operator enumerates the effective Stop-hook set from
every source the session will load: user, project, and local settings, enabled
plugins, and the task settings file. The task-owned
`delegate-turn --state-dir` hook must be present exactly once. **Reject any
sibling Stop hook you do not deterministically know to be nonblocking** —
remove it from the session's effective configuration or do not launch. The one
currently known-safe sibling is the official Codex plugin's
`stop-review-gate-hook.mjs`, and only when the operator has confirmed its
effective `stopReviewGate` is off through the plugin's own surface; then it
may be treated as nonblocking. CodeFlow does not read or infer plugin-private
state — this check is the operator's, made against the plugin's own
configuration, and an unknown or unverified sibling fails the preflight.

## Launch and drive one turn

```sh
# Read the managed defaults, then any doctor-validated project override.
CLAUDE_MODEL="<claude-primary native selector>"
CLAUDE_EFFORT="<default effort>"
codeflow delegate init --run-id run-42 --state-dir "$STATE"
tmux new-session -d -s cf-run-42 -x 220 -y 50 -c /absolute/worktree \
  "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 claude --model $CLAUDE_MODEL --effort $CLAUDE_EFFORT --permission-mode bypassPermissions --settings $STATE/settings.json"
# For consult/no-edit, use the same launch with --permission-mode auto.
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until ready --timeout-seconds 120
printf '%s' "$PROMPT" > "$RUN_TMP/turn-1.prompt"   # outside the repo
codeflow delegate arm --run-id run-42 --state-dir "$STATE" \
  --turn-id turn-1 --prompt-file "$RUN_TMP/turn-1.prompt"
tmux load-buffer -b cf-run-42-turn-1 "$RUN_TMP/turn-1.prompt"
tmux paste-buffer -p -b cf-run-42-turn-1 -t cf-run-42
sleep 0.3  # bounded TUI input-settle; not a completion heuristic
tmux send-keys -t cf-run-42 Enter
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until accepted --turn-id turn-1 --timeout-seconds 120
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until terminal --turn-id turn-1 --timeout-seconds 3600
```

For an evaluator-pinned CodeFlow executable, a login shell can reset `PATH`.
Set any task-scoped candidate `PATH` in the actual launched shell after login
startup, then check the public command resolution and executable SHA-256 in
the child environment. Probe a generated hook in that same environment and
retain its outcome; this binds the probed invocation, not every future hook.
A parent-shell lookup or absolute top-level CLI invocation alone does not
bind bare `codeflow` in hooks. Do not change the user's global installation.

Invoke the Claude primary using the selector and default effort from
`../../cf-model-orchestrator/resources/current-ensemble.json`; workers take
escalation. When
`.codeflow/model-selection.json` is nonempty, first require
`codeflow doctor --check model-bindings` to pass and use its effective
qualified override. Delivery must be
**exact-byte after one canonicalization boundary**: the prompt file must
already be non-empty UTF-8 text with internal LF line endings, no terminal line
break, and no other control characters. `arm` rejects noncanonical input before
creating durable turn
state, then records the
SHA-256 of the accepted file bytes. Deliver that same file through a uniquely
named tmux buffer with a literal paste into the exact pane, wait a bounded
300 ms for the TUI to attach it, and send one separate Enter. Acceptance
requires a `UserPromptSubmit` whose prompt matches the digest. If acceptance
times out, inspect only the dedicated pane; when it explicitly shows the paste
attachment still waiting in the editor, send Enter once more and re-wait once.
Never send blind or repeated Enter retries. A mismatched, unarmed, or duplicate
submission is blocked at the harness (hook exit 2) with run state preserved.
Prompts are capped at 1 MiB.

## Stable exit states

`wait` exits `0` for the observed state (for `--until terminal`, a completed
`Stop` result whose JSON is printed on stdout), `10` for a failed
`StopFailure` terminal, `11` for poison or unsafe/malformed state, `124` for
timeout, and `130` for interruption. `init` and `arm` exit `0` or `1`. Branch
on these codes — never on pane appearance. A completed result carries
`schema_version` 2, the run and turn ids, `session_id`, `prompt_id` (absent
only on the recorded pre-2.1.196 single-turn compatibility path), and the
bounded `last_assistant_message`; require the message to carry the requested
structured verdict or handoff fields, then independently verify cited
evidence and the worktree diff.

Session restarts (resume, clear, compact, fork), a conflicting startup, a
mis-correlated terminal event, or an interrupted wait after acceptance write
durable poison. Poison blocks every later transition; recovery is a new run
id in a fresh state directory — records are never edited.

## Sequential turns

Collect delegated worker results before the primary returns its final answer.
For peer-dependent turns, verify a supported public foreground native return
within the host turn; an intended wait flag is not proof. Never call plugin-internal
scripts or cached private paths. Keep host-side monitoring in that accepted
foreground turn: do not use Claude Bash `run_in_background` watchers or rely on
their task notifications to resume it. A notification can enter as a new,
unarmed `UserPromptSubmit` and be rejected. A persistent native peer process
behind the dedicated pane is permitted; collect its result in-turn. A worker
that resumes the primary
after its terminal result can emit an unsolicited second Stop; schema-v2 cannot
correlate that continuation and deliberately poisons the run. A terminal
message saying work is still running is incomplete, not a successful handoff.
Do not weaken correlation or count a later uncorrelated response as verified.
Recover in a fresh run and recheck the evidence; no internal worker registry is
required. If the harness cannot keep worker activity inside the accepted turn,
record that route as unqualified and use a supported bounded native route.

Each run permits one outstanding armed turn. After a terminal result, arm the
next turn under a new turn id in the same session and repeat
deliver → wait-accepted → wait-terminal; a terminal turn id can never be
re-armed. Sessions without `prompt_id` support (pre-2.1.196) are limited to
one turn per run.

## Dialogs and diagnosis-only pane capture

Auto mode can pause for a classifier escalation or an explicit user question.
A dialog is interactive input, never completion: answer it in the dedicated
pane without broadening authority and resume the bounded wait. Capture only
this dedicated task pane, and only for bounded diagnosis when a wait times
out or a result is malformed — never as a completion heuristic and never
against unrelated sessions.

## Bounded harvest and cleanup

After the terminal result is consumed, harvest what verification needs — the
verdict, the evidence to re-derive, the worktree diff — then kill the task
session, remove the state directory, and delete the private prompt files.
State records carry digests and bounded payloads, never the prompt text.

## Legacy compatibility only

`codeflow hook delegate-turn --run-id RUN --result FILE` is the legacy
one-shot record-and-signal mode, retained byte-compatible for existing
callers until a later major release. The two hook modes are mutually
exclusive and never fall back to one another. New work always uses the
schema-v2 lifecycle above.
