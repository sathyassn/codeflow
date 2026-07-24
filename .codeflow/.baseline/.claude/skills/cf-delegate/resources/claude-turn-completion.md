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
   edit it or merge other keys into it. Select auto mode with
   `--permission-mode auto` and make `autoMode.classifyAllShell` effective at
   user or CLI scope — Claude ignores it at project scope — then prove the
   composed boundary with the preflight canary.

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
codeflow delegate init --run-id run-42 --state-dir "$STATE"
tmux new-session -d -s cf-run-42 -x 220 -y 50 -c /absolute/worktree \
  "claude --model fable --effort high --permission-mode auto --settings $STATE/settings.json"
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until ready --timeout-seconds 120
printf '%s' "$PROMPT" > "$RUN_TMP/turn-1.prompt"   # outside the repo
codeflow delegate arm --run-id run-42 --state-dir "$STATE" \
  --turn-id turn-1 --prompt-file "$RUN_TMP/turn-1.prompt"
tmux load-buffer -b cf-run-42-turn-1 "$RUN_TMP/turn-1.prompt"
tmux paste-buffer -p -b cf-run-42-turn-1 -t cf-run-42
tmux send-keys -t cf-run-42 Enter
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until accepted --turn-id turn-1 --timeout-seconds 120
codeflow delegate wait --run-id run-42 --state-dir "$STATE" \
  --until terminal --turn-id turn-1 --timeout-seconds 3600
```

Invoke the latest available Fable-class model directly at high, or xhigh on
the escalation triggers in `cf-model-orchestrator`. Delivery must be
**exact-byte**: arm records the SHA-256 of the prompt file's bytes, and
acceptance requires a `UserPromptSubmit` whose prompt matches that digest —
so deliver the same file through a uniquely named tmux buffer with a literal
paste into the exact pane and a separate Enter. A mismatched, unarmed, or
duplicate submission is blocked at the harness (hook exit 2) with run state
preserved. Prompts are capped at 1 MiB.

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
