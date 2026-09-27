# Other hosts

Read this when the active host is Grok Build or another harness, or when a
Grok seat is used. Before a Grok preflight or launch, also read
[the Grok host detail](../resources/grok-host.md): its guards, launch,
sandbox profiles and peer lanes.

| Active host | Peer lane | Coordinator | Execution binding |
|---|---|---|---|
| Grok Build (interactive `grok` CLI), host detail above | Herdr `claude` + schema-v2; official `codex` CLI → app-server (Herdr; tmux degraded) | Grok host | Same contract. The Claude design owner authors real design natively. Catalog Grok may execute or take named extra-family review |
| Other harness, including Hermes | Delegate the repository task to one sanctioned native host by default; coordinate directly only if both lanes and the full contract are proven | One native host | Same capability-routed contract; no nested orchestration |

Grok seat autonomy boundary:

- Grok: `--always-approve` for production; `--permission-mode auto` for
  consult/no-edit; `--sandbox <PROFILE>` when required. Never `grok -p`.
