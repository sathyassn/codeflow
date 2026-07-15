# Host-neutral duo canary — 2026-07-15

This is a historical local verification record for ADR-0023, not a promise
about another machine or a future install. Setup and release procedures must
repeat both interactive canaries with the versions and tools actually selected
for that run.

## Environment observed

The following read-only prerequisite commands were rerun immediately before
release review:

| Check | Observed result |
|---|---|
| `claude --version` | `2.1.210 (Claude Code)` |
| `codex --version` | `codex-cli 0.144.3` |
| `tmux -V` | `tmux 3.6a` |
| `claude plugin list --json` | `codex@openai-codex` 1.0.6, user scope, enabled |
| `codeflow doctor --check delegates` | prerequisite check passed |

`claude auth status --json` reported logged out, so it was not accepted as the
account-readiness oracle. A normal interactive `claude` session was launched in
a dedicated tmux session instead. It displayed the authenticated Claude Max
account, accepted an interactive turn, and retained its native tool/MCP status.
No token, credential, or transcript was copied into this repository.

## Codex-hosted Claude lane

The task-scoped Claude settings registered `Stop` and `StopFailure` commands of
this form:

```text
codeflow hook delegate-turn --run-id <unique-id> --result <absolute-private-result>
```

The coordinator waited on the matching
`codeflow-delegate-<unique-id>` tmux channel. In the plan-mode canary,
`AskUserQuestion` paused for input without producing a terminal result. After
the coordinator answered in the dedicated pane, the completed turn produced:

- event `Stop` and status `completed`;
- the matching run id and exact `last_assistant_message`;
- a `0600` result in a `0700` directory; and
- release of only the matching tmux waiter.

A separate normal-permission, test-running Claude review repeated the terminal
hook path. Its waiter exited successfully, its result again had `0600`/`0700`
permissions, and the CodeFlow and Agent OS source-diff SHA-256 digests were
identical before and after the review. The persisted verdict was `approved`.

## Claude-hosted Codex lane

The official `codex@openai-codex` plugin was installed and enabled. Its
presence is an inspectable prerequisite, not proof that a future task has the
required Codex authentication, MCP tools, or approvals. The Claude-hosted lane
therefore retains its own interactive task canary and records the actual Codex
thread and tool evidence in that task's evidence ledger.

## Evidence boundary

This record establishes what was directly observed on 2026-07-15. It does not
turn installed versions, status output, or a prior successful session into a
portable guarantee. On another install, every environment-specific claim is
`VERIFY-ON-INSTALL`; a missing or failed committed lane blocks the duo rather
than silently degrading after work begins.
