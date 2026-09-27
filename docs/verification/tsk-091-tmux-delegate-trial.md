# Tmux delegate transport live trial, 2026-09-27

## Scope

This record is TSK-091's live run of the `cf-delegate` tmux recipe: the
degraded TTY host that the skill names when Herdr is unavailable. Before this
run only the Herdr path had run live since the typed directive after a paste
(`d1236ad5c`) and the task notice continuation (TSK-090) shipped.

- Host: macOS arm64, Claude Code 2.1.283, tmux 3.6a.
- Session: `claude --model opus --effort high --permission-mode
  bypassPermissions --settings <state>/settings.json`, launched with
  `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` and the candidate first on `PATH`,
  as the recipe says. The observed model id was not read; only the requested
  selector is recorded.
- Candidate: `codeflow 3.0.0` built from
  `integration/EPC-020-delivery-system` at `c3dbd28a0`, which contains the
  reviewed TSK-090 implementation (`fix/delegate-task-notice-3.0.0` has no
  commit missing from that head by `git cherry`). Binary SHA-256 prefix
  `58b04ee7f8d774cc`. The session's pane resolved `codeflow` to that binary.
- Sample: a repository created for this trial and initialised by the
  candidate at the standard tier, plus one small workflow
  (`.claude/workflows/ping.workflow.js`, one agent step returning
  `{"status": "complete", "reply": ...}`).
- Driver: `scratchpad/models-stream/tmux-trial.sh` in the session scratchpad
  (not tracked); run root `tmux-trial-20260927T002418`.

## Results

| Check | Result | Evidence |
|---|---|---|
| Trust prompt | Pass | The session asked to trust the sample; the driver answered only after reading the dialog's folder and matching it to the sample by real path (the folder this run created). |
| Ready | Pass | `delegate wait --until ready` exit 0; `ready.json` records `SessionStart` from `startup` in the sample. |
| Turn 1, pasted prompt with the directive | Pass | A four-line prompt pasted with `load-buffer` and `paste-buffer -p` showed a `[Pasted text` attachment; the driver typed `Carry out the pasted instructions.` with `send-keys -l`, then one Enter. `accepted.json` records `delivery: paste_directive` with the armed prompt's SHA-256 (`a208db60...`); `wait --until accepted` and `--until terminal` exit 0; the result is `completed` ("I saw 13 entries at the repository root"). |
| Turn 2, exact delivery | Pass | A three-line prompt pasted without an attachment was accepted as `delivery: exact` (`6abcdd53...`); `wait --until terminal` exit 0 on the turn's first `Stop` ("The ping workflow is now running in the background (task `w110nkxb1`)"). |
| Task notice continuation | Pass | After that `Stop`, the workflow's completion notice arrived as a new prompt and was admitted as a continuation: `turns/turn-2/continuations/w110nkxb1/accepted.json` records `delivery: task_notification`, task `w110nkxb1`, its `tool_use_id`, `launched_by_prompt_id` equal to turn 2's prompt id, and the notice's SHA-256; `result.json` beside it is `Stop` `completed`, and the Stop-time origin check wrote it. The sample's `ping-result.json` holds `{"status":"complete","reply":"pong"}`, the workflow's own return. |
| Teardown | Pass | Two Ctrl-C to the owned session, then `tmux kill-session` of that session only; `tmux has-session` then reports it absent. No other tmux session was listed, captured or signalled. The sample and state directory are retained under the session scratchpad as evidence. |

Lifecycle files, as the state directory held them: `ready.json`,
`settings.json`, `turns/turn-1/{request,accepted,result}.json`,
`turns/turn-2/{request,accepted,result}.json` and
`turns/turn-2/continuations/w110nkxb1/{accepted,result}.json`. No poison
record was written.

## Not verified

- The sibling Stop-hook preflight was not performed as a separate step: the
  session loaded the operator's user settings. The run completed without a
  blocking Stop hook, which is evidence for this host only.
- The observed model and effort of the session were not read.
- One run on one host. Linux, WSL2 and other Claude Code versions are not
  covered.
- The release qualification on this head (TSK-091 AC-4) runs with the
  release candidate, from the harness carrying TSK-092.
