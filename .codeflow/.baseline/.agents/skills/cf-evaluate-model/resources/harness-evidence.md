# Native harness capability evidence

This compact index travels with the scaffold so a consuming repository can
validate the capability-supported entries in `harnesses.json` without
receiving CodeFlow's internal verification archive. It records why CodeFlow
recognizes each harness surface; it does not qualify a concrete model binding
or prove that a particular installation is authenticated, current, correctly
configured, or running the requested model.

The source qualification combined:

- the 2026-07-15 host-neutral duo canary for native interactive Claude Code,
  Codex CLI/app-server provenance, configured tools, and scoped execution;
- the 2026-07-24 delegate-lifecycle canary for bounded failure, recheckable
  results, exact prompt binding, native task/thread/session provenance, and
  effective Claude/Codex boundaries;
- ADR-0008's harness-independent git backstop, ADR-0026's permission-boundary
  corrections, and ADR-0027's native-interactive evaluator contract;
- Grok Build 1.0.13 CLI and user-guide evidence recorded 2026-09-01:
  `grok --version` → `grok 1.0.13`; interactive TUI; session UUID under
  `~/.grok/sessions/`; `--cwd`/`--worktree`; MCP/skills; `--permission-mode
  auto`; `--always-approve` overlay; `grok --help` documents `--sandbox
  <PROFILE>` and the user guide names `workspace` / `read-only` / `strict`;
  `grok sessions` / `grok export` / `--resume`. This is harness-capability
  evidence, not a native-interactive Grok-hosted duo canary; and
- deterministic scaffold, settings, hook, lifecycle, and evaluator tests.

Catalog status for `grok-cli` is harness capability, not a completed
Grok-hosted duo canary. Headless `grok -p` / `--single` is not a work-session
lane.

| Capability | Claude Code | Codex CLI | Codex App | Grok Build CLI |
|---|---|---|---|---|
| Native interactive session and provenance | Native TTY session, prompt/session binding, and lifecycle canaries | Native CLI plus app-server task/thread evidence | Native app task/thread surface required by the evaluator contract | Native `grok` TUI; session UUID; `--resume` / `--continue`; `grok sessions` |
| Configured tools and scoped workspace | Tool/MCP preflight plus task-scoped worktree and lifecycle state | Tool/MCP preflight plus task-scoped worktree | App tool inventory plus task-scoped worktree | MCP/skills/tools plus `--cwd` and `--worktree` |
| Bounded failure and recheckable result | Acceptance/terminal binding, timeout, interruption, and poisoned-run cases | Native task/thread status and result recheck | Native task/thread recheck; absence remains unknown | Session directory, `grok export`, resume/fork; Grok-hosted peer lifecycle remains to prove |
| Effective permission boundary | Fail-closed sandbox, secret filtering, and classified trusted-tool retry canaries | Guarded workspace profile, public-network boundary, and escalation review tests | Same guarded project profile; effective settings must be rechecked natively | The modes and `--sandbox <PROFILE>` recorded above (user-guide: workspace / read-only / strict); the launch posture is cross-family transport's (`cf-model-orchestrator/resources/routing/transport.md`) |
| Git backstop | Repository hooks, CI, and remote protection | Repository hooks, CI, and remote protection | Repository hooks, CI, and remote protection | Repository hooks, CI, and remote protection |

Qualification is intentionally layered. This file supports only the
source-controlled *harness capability* entry. Catalog status therefore means
`capability-supported`, not that any concrete model binding is qualified. A
model, effort, harness version, settings profile, and tool set still require a
full native-interactive evaluation with requested/observed identity evidence
and human approval.
`codeflow doctor` can detect only catalog, record, version-probe, and declared
settings drift; it never turns this historical index into a live pass.

The retained canaries exercised macOS arm64. Shared deterministic tests cover
the documented macOS/Linux/WSL2 paths and Windows guards, but platform-native
claims must be rechecked on the release target and consuming installation.
