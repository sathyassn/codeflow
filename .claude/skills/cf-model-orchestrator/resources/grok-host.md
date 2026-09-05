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
then Herdr `codex` (tmux degraded). Do not install third-party Grok Codex
plugins. The Claude-Code `codex-plugin-cc` is not a Grok-host lane.

Do not claim the Grok-hosted duo complete until both canaries exist.

## Duties

Host is not duty. Claude produces design in its native session. Catalog Grok
may produce or take named extra-family review; that assignment is never a
silent third vote.
