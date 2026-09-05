# Grok Build as a CodeFlow host

On-demand detail for a Grok Build (`grok` CLI) host. Durable duties stay in
`SKILL.md`, `current-ensemble.json`, and `routing-policy.json`.

## In-session guards

CodeFlow binds the same `codeflow hook git-guard` / `exec-guard` /
`session-orient` binaries Grok as Codex, via `.grok/hooks/codeflow.json`.
Grok also scans `.claude/settings.json` when compat is on. Project hooks
load only after `/hooks-trust` or `--trust`. Git hooks and CI remain the
enforcement floor. This is not a Grok schema-v2 Stop-hook lifecycle;
Claude turn completion still uses schema-v2 when Grok hosts Claude.

## Launch

Take selector and effort from the current ensemble record. Default:

```text
grok --model <selector> --reasoning-effort <effort> --permission-mode auto
```

`--always-approve` is the unattended overlay, not the default. Never
`grok -p` / `--single` for a work session.

`--sandbox <PROFILE>` when an OS sandbox is required. `grok --help` exposes
the flag; the Grok Build user guide names `workspace` / `read-only` /
`strict`. Profile names are VERIFY-ON-INSTALL against the installed CLI.

## Peer lanes

A Grok host reaches Claude and Codex through Herdr:

- Claude: Herdr `claude` plus schema-v2 lifecycle (arm / accepted / terminal)
- Codex: Herdr `codex` interactive TTY — not the official Codex plugin
  (that plugin is Claude-Code-only)

Do not claim the Grok-hosted duo complete until both canaries exist.

## Duties

Host is not duty. Claude produces design in its native session. Catalog Grok
may produce or take named extra-family review; that assignment is never a
silent third vote.
