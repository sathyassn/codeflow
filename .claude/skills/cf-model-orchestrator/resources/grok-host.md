# Grok Build as a CodeFlow host

On-demand detail for a Grok Build (`grok` CLI) host. Durable duties stay in
`SKILL.md`, `current-ensemble.json`, and `routing-policy.json`.

## In-session guards

CodeFlow binds `codeflow hook git-guard` / `exec-guard` on PreToolUse and
`session-orient` on SessionStart plus Grok `PreCompact`/`PostCompact`, via
`.grok/hooks/codeflow.json`.
The guard parser accepts Grok's camelCase stdin (`toolName`, `toolInput`,
`run_terminal_command`) as well as Claude/Codex snake_case. Grok also scans
`.claude/settings.json` when compat is on. Project hooks load only after
`/hooks-trust` or `--trust`; `grok inspect` is the local proof of discovery
and trust. Git hooks and CI remain the enforcement floor. This is not a Grok
schema-v2 Stop-hook lifecycle; Claude turn completion still uses schema-v2
when Grok hosts Claude.

## Launch

Take selector and effort from the current ensemble record. Production host:

```text
grok --model <selector> --reasoning-effort <effort> --always-approve
```

`--permission-mode auto` is the consult / no-edit alternative. Never
`grok -p` / `--single` for a work session.

`--sandbox <PROFILE>` when an OS sandbox is required. `grok --help` exposes
the flag; the Grok Build user guide names `workspace` / `read-only` /
`strict`. Profile names are VERIFY-ON-INSTALL against the installed CLI.

## Peer lanes

A Grok host reaches Claude through Herdr (`claude` + schema-v2). It reaches
Codex through the official `codex` CLI, which talks to the local app-server
daemon — start `codex app-server daemon start` when the socket is missing,
then Herdr `codex` (tmux degraded). If the daemon cannot start, keep the
interactive CLI in Herdr. Do not install third-party Grok Codex plugins. The
Claude-Code `codex-plugin-cc` is not a Grok-host lane.

Grok-started Claude schema-v2 (Herdr `send-text` of the armed file) and
Codex Herdr consult canaries are recorded in the CodeFlow repository under
`docs/verification/`. Consuming scaffolds do not ship that file. Those
records are consult-posture, host-specific, and not a qualified
`grok-engineering-primary` binding or a full promotion suite. Doctor does
not claim the lanes.

## Duties

Host is not duty. Apply the canonical responsibility-versus-execution and route
status rules in `capability-routing.md`; this adapter does not redefine them.
The Claude design owner produces direction and real design execution in its
native session unless Plan vN records an explicit task-specific operator
override—Claude absence alone is not one. A Grok high host stays the
orchestrator and may use its own permitted routes. Catalog Grok may execute or
take named extra-family review when a documented trigger fires and it is
available; actual authored lineage determines independent review, and the
assignment is never a silent third vote.
