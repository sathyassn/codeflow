# Grok-hosted duo canary — 2026-09-07

Live Grok Build host inside Herdr (`HERDR_ENV=1`, workspace `w2`, caller
`w2:p1` untouched). This record satisfies ADR-0054 Decision 9: a
Grok-started Claude schema-v2 canary and a Grok-started Codex Herdr
canary. It is not a full native-interactive promotion suite and does not
qualify `grok-engineering-primary`.

## Environment

| Item | Observed |
|---|---|
| Host | Grok Build (`grok` TUI), `HERDR_ENV=1` |
| Worktree | `/Volumes/DATA/Local/software-workspace/projects/codeflow-grok-first-class` |
| Branch / HEAD | `feat/grok-first-class-host` / `2b37350ec` |
| CodeFlow binary | worktree `target/release/codeflow` 3.0.0 |
| Claude CLI | 2.1.263 |
| Codex | CLI/app-server 0.153.4, daemon `running` |
| Consult posture | Claude `--permission-mode auto`; Codex `--ask-for-approval on-request --sandbox workspace-write` |

## (a) Grok-started Claude schema-v2

Prompt file (canonical UTF-8, internal LF, no terminal newline, 124 bytes,
SHA-256 `bb8e9b08c12ef8347842d0fc8aab73faa7fa2862f180399ceff12603cdca493b`):

```text
ROLE: peer. Read-only. Reply with exactly GROK_HOST_SCHEMAV2_OK then stop. Do not edit files. Do not start the orchestrator.
```

Delivery: `codeflow delegate arm` then `herdr pane send-text` of those exact
bytes plus one Enter. Completion is schema-v2 `wait --until terminal`, not
Herdr idle/done.

### Fable medium — transport pass, model rate-limited

Requested: `--model fable --effort medium`. Run `grok-host-canary`. Tab
`cf/codeflow/ghcanary/claude/01` (`w2:t25` / `w2:p25`), agent
`cf-codeflo-ghcanary-cl01`.

| Step | Result |
|---|---|
| `wait --until ready` | `session_id` `b57c4039-7f2f-46d4-9891-bce70d40ab74` |
| `wait --until accepted` | `prompt_id` `d91dcd7e-b5e7-4ac0-810c-dcad8c6432a3`; digest matched |
| `wait --until terminal` | `StopFailure` `rate_limit` (HTTP 429); wait exit 10 |

The lane correlated session, digest, and `prompt_id`. Fable credits were
unavailable. Recovery used a new run id and state directory (poisoned runs
are not reused).

### Opus medium — completed (Fable fallback)

Requested: `--model opus --effort medium` (ensemble: Opus is the recorded
Claude fallback when Fable is unavailable). Run `grok-host-opus`. Tab
`cf/codeflow/ghcanary/claude/02` (`w2:t27` / `w2:p27`), agent
`cf-codeflo-ghcanary-cl02`.

| Step | Result |
|---|---|
| `wait --until ready` | `session_id` `28f8b652-9ab3-4dd7-8187-2112a759d0db` |
| `wait --until accepted` | `prompt_id` `a576a185-a972-4c50-aea9-71e79b897b30`; same digest |
| `wait --until terminal` | `Stop` `completed`; `last_assistant_message` `GROK_HOST_SCHEMAV2_OK`; wait exit 0 |

Do not report that Fable reviewed or answered this turn.

## (b) Grok-started Codex Herdr

Tab `cf/codeflow/ghcanary/codex/01` (`w2:t26` / `w2:p26`), agent
`cf-codeflo-ghcanary-cx01`. Launch:
`codex --model gpt-6-astra -c model_reasoning_effort=medium --ask-for-approval on-request --sandbox workspace-write`.
Observed in-pane: `gpt-6-astra medium`. Delivery: `herdr agent prompt` of the
same ROLE/read-only text (Codex has no schema-v2 arm/digest). Native thread
`01a07c4c-92ec-7b21-9bdb-889bd8f4d6c7` (recheckable in Codex App/TUI).
Harvested reply in the dedicated pane: `GROK_HOST_CODEX_OK`. Herdr
`idle`/`done` was not treated as completion.

This is a consult-thread canary, weaker than lane (a)'s schema-v2
ready/accepted/terminal records. It still proves Grok can start official
`codex` in Herdr and harvest a native thread result.

## Shared proofs

- Worktree `git status --short` stayed empty before and after both lanes.
- Caller pane `w2:p1` was not sent keys.
- `codeflow doctor --check delegates` still does not claim these lanes.
- Claude schema-v2 state dirs (owner-only, not committed):
  `~/.codeflow/canary-runs/grok-host-2026-09-07` (Fable 429) and
  `~/.codeflow/canary-runs/grok-host-opus-2026-09-07` (Opus completed).
  Recheck: `codeflow delegate wait --run-id grok-host-opus --state-dir
  ~/.codeflow/canary-runs/grok-host-opus-2026-09-07 --until terminal --turn-id turn-1`.
  Durable proof for the PR is this markdown harvest, not the private state.

## Not this canary

- Full three-trial native-interactive promotion of `grok-engineering-primary`
- Production `bypassPermissions` / `danger-full-access` (consult posture only)
- Multi-turn sequential arm, AskUserQuestion, or permission-dialog routing
- Linux, Windows, or WSL2
