# Claude turn-completion adapter

Use this adapter only for the codex-hosted, interactive Claude CLI lane. It
turns Claude's task-scoped `Stop` and `StopFailure` events into one result file
plus one tmux wait-channel signal. It does not inspect transcripts or panes.

## Per-turn setup

1. Generate a unique run id containing only ASCII letters, digits, `.`, `_`,
   or `-` (64 characters maximum).
2. Create a new result directory outside the repository and set its mode to
   `0700`. Choose an absolute, not-yet-existing `result.json` below it.
3. Create a task-only Claude settings file. It enables auto-mode shell
   classification and adds the completion hooks without replacing the native
   project tools or MCP servers. Substitute the same absolute result path and
   run id in both hook commands:

```json
{
  "autoMode": {
    "classifyAllShell": true
  },
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "codeflow hook delegate-turn --run-id review-42 --result /absolute/owner-only/run/result.json",
            "timeout": 30
          }
        ]
      }
    ],
    "StopFailure": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "codeflow hook delegate-turn --run-id review-42 --result /absolute/owner-only/run/result.json",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

Keep this file task-scoped: `--settings` loads it in addition to user and
project settings, so the delegate retains its native tools and MCP servers.
The handler rejects relative paths, result directories accessible to group or
other users, unsafe run ids, unknown events, a `Stop` event without
`last_assistant_message`, and any attempt to overwrite a terminal result.
An exact retry may re-signal the waiter without rewriting the result, so a
transient tmux signalling failure is recoverable; different terminal evidence
for the same result path is rejected.

## Launch and wait

Launch the dedicated session in the intended worktree. Use the latest available
Fable-class model at `xhigh` and auto permissions inside the effective
fail-closed project sandbox:

```sh
tmux new-session -d -s cf-review-42 -x 220 -y 50 -c /absolute/worktree \
  'claude --model fable --effort xhigh --permission-mode auto --settings /absolute/task-settings.json'
tmux send-keys -t cf-review-42 -l \
  'Review only the named scope. Edit nothing. Return evidence and VERDICT: approved|changes_requested.'
tmux send-keys -t cf-review-42 Enter
tmux wait-for codeflow-delegate-review-42
```

Before launch, verify that the effective project settings enable the OS sandbox,
set `sandbox.failIfUnavailable: true`, auto-allow sandboxed Bash, disallow
unsandboxed commands, allow the public-network and local-server access the task
needs, and keep secret/private-network/destructive-action controls. Auto mode is
not bypass mode; never use bypass mode on an ordinary host. A read-only consult
must say "edit nothing" and compare the worktree state before and after. A
final reviewer may run scoped tests or UI tools under the same boundary. An
edit-enabled handoff is a different session with explicit write authority;
never infer that authority from the transport.

Apply a bounded timeout to the `tmux wait-for` process through the host
harness. Auto mode can pause for a classifier escalation or user question; if
the terminal signal has not arrived within the bound, capture only this
dedicated pane, answer the explicit dialog without broadening authority, and
resume the bounded wait. A dialog is interactive input, never completion.

After the terminal signal returns, parse `result.json` and require all of these:

- `schema_version` is `1` and `run_id` exactly matches the caller's id;
- `status` is `completed`, `event` is `Stop`, and
  `last_assistant_message` is present; or explicitly handle a `failed`
  `StopFailure` result;
- the message carries the requested structured verdict or handoff fields.

Then independently verify cited evidence and the worktree diff. Capture the
dedicated pane only for bounded diagnosis when the signal times out or the
result is malformed. Finally kill the task session and remove its private
result directory and settings file.
