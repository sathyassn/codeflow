# Delegate lifecycle live canary — 2026-07-23

## Scope

This record covers the PR1 live checks for ADR-0036 on the available host:

- macOS arm64;
- Claude Code 2.1.218;
- Claude Fable 5 at high effort;
- CodeFlow commit `d152ee89` with the locally built debug binary.

It does not establish Linux, macOS x64, WSL2, or cross-version behavior. The
fake-TUI stress matrix and broader native-platform evidence remain PR2 and
release-gate work.

## Results

| Check | Result | Evidence |
|---|---|---|
| Effective sibling Stop hooks (U1) | Pass for this host | Default user/project/local sources, enabled plugins, and the task settings were inspected before delivery. Two Stop hooks were active: the task-owned `delegate-turn --state-dir` command and the official Codex plugin 1.0.6 `stop-review-gate-hook.mjs`. The latter was deterministically nonblocking because this repo's plugin state had `stopReviewGate: false`, which makes the inspected script return without a block decision; live terminal completion confirmed that behavior. No other enabled plugin supplied a Stop hook. The reusable rejection procedure still belongs in the PR2 skill/preflight work. |
| Prompt normalization start (U4) | Pass for the exercised Unicode prompt | A prompt containing `α` was armed and delivered with tmux load-buffer/paste-buffer plus a separate Enter. `UserPromptSubmit` matched the armed SHA-256 (`c95f82b9bc761d3ec82b2528e9b1189390c758d74a96c6aca1189e319fb39fe1`) and completed as `ACK-NORMALIZATION`. CRLF, newline, size, and duplicate-delivery combinations remain in the PR2 fake-TUI matrix. |
| Stop prompt binding (U5) | Pass across three turns | Accepted and terminal records carried the same `prompt_id` for the normalization (`88aff526-26fe-4cc3-910b-88ecf0c91623`), AskUserQuestion (`1ea555a7-936c-4340-88bc-047fce50db4c`), and permission (`2573c8b5-c988-4ad2-9cd4-29947cb26875`) turns. |
| AskUserQuestion answer routing | Pass | Selecting `A` in an in-turn question did not emit another `UserPromptSubmit`, alter `accepted.json`, or poison the run. The turn completed as `UI-ANSWER:A`. |
| Permission-response routing | Pass | A harmless temporary-file command was forced through an explicit permission rule. Both the sandboxed confirmation and the subsequent unsandboxed-retry confirmation left `accepted.json` unchanged and created no poison; the turn completed as `PERMISSION-DONE`. This used manual/default permission mode to force the UI path and is routing evidence, not a change to the normal Auto-mode lane. |

The owner-only state directories and temporary canary file were removed after
the records were consumed.

## Decision

The exercised AskUserQuestion and permission responses do not collide with the
schema-v2 `UserPromptSubmit` guard on Claude Code 2.1.218, so ADR-0036's
interactive-dialog fail-closed contingency is not activated for this verified
environment. A future harness/version that produces different canary evidence
must be treated as unsupported for interactive-dialog turns until a distinct
correlation mechanism is designed and reviewed.
