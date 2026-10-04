# Harness posture

<!-- HOW layer. Split out of docs/adoption.md: the runtime posture each harness
     gets. Adoption keeps install, tiers, ownership, and the daily loop; the
     planes that block, and how far each reaches, live in
     docs/architecture/enforcement-planes.md. -->

## Concept

**Autonomy is fail-closed by default and widened only by the operator.** Each
supported harness ships with a preset that grants the access ordinary work
needs and refuses the rest; widening it is an operator act, and the enforcement
floor stays underneath either way.

The starters are executable policy, not prompt-only guidance: the preset is
enabled in runtime settings as well as described in the skills (architecture
decision record ADR-0025). Approval policy and sandbox authority are separate
controls ([Codex security](https://learn.chatgpt.com/docs/security)).

## Architecture

Each harness reads its own configuration files, and all three hand the same
payload to the same two guards. Read a row of the table as the boundary for one
harness: the left column is what the scaffold turns on, the right column is
what an operator still has to decide.

| Harness | What the shipped preset enables | What stays gated |
|---|---|---|
| Claude Code | Fail-closed OS sandbox on macOS, Linux and WSL2, sandbox-contained Bash with raw model and cloud credentials removed, web search/fetch, wildcard public-domain egress for dependency and tool subprocesses, local port binding for dev/UI tests, ask rules for destructive source-control operations; cross-family seats pass the current ensemble's model and effort explicitly | Private, link-local, and internal-name destinations; destructive, privileged, publish, and secret-read boundaries; an unsandboxed retry only when the native configuration permits it, auto-classified for a trusted installed tool that needs host state such as `herdr` or the official Codex plugin; the guards judge the retried command under the same policy, a retry never lifts a destructive, privileged, publishing, secret-read or enforcement-file refusal, and a delegated seat that runs sandboxed denies the retry natively; auto mode and classifier policy are never taken from the repository, so project `acceptEdits` remains the ordinary fallback (ADR-0026, ADR-0029) |
| Codex | Guarded workspace permission profile (no `sandbox_mode` key), live search, reviewer-subagent escalation review, workspace key-file denies, the current primary seat's configured fallback, the default secret-bearing environment filter, `approval_policy = "never"`, `model_reasoning_effort = "high"`, production `--sandbox danger-full-access`, which turns the OS sandbox off for that process | Catastrophic work still stops for the operator; git-guard, exec-guard, git hooks, and CI remain the floor; `never` grants no access of its own, so operations outside the effective sandbox fail instead of asking |
| Grok Build | Project hooks wired in `.grok/hooks/codeflow.json`, the same `git-guard` and `exec-guard` payload | The hooks load only after the one-time `/hooks-trust` (or `--trust`); the doctor check reports structural wiring, not trust state (ADR-0054) |

The catastrophic classifier is a non-relaxable floor, not a complete endpoint
security product. Managed organization policy, least-privilege host accounts,
verified backups, and authenticated human approval remain necessary at higher
blast radii. Per-platform behavior, the exact configuration keys, and the
credential rules are in Technical.

## Technical

The platform matrix, then the permission profiles, then credentials, then the
dated evidence.

### Platform assurance

CodeFlow releases target macOS, Linux, and x86-64 native Windows; 3.0.0 was
the one release without native Windows, which returns in 3.1.0. Windows users
can also run the Linux build inside WSL2; this is the preferred route for a
Linux-native toolchain or Claude work that needs OS-enforced sandboxing.

| Platform | Sandbox | What it means for the work |
|---|---|---|
| macOS | each harness's native sandbox | the default posture above applies as written |
| Linux, WSL2 | the harness Linux sandbox implementations | install the dependencies reported by the harness and keep fail-closed startup enabled; WSL2 follows the Linux path |
| Native Windows, Codex | its elevated Windows sandbox, selected by default | the scaffolded deterministic guard recognizes both Bash and PowerShell command events and Windows drive, system, profile, disk, recovery, and permission operations |
| Native Windows, Claude Code | none; Git Bash or PowerShell work, but Claude's OS sandbox is not available | settings and hooks still apply, but they are not equivalent containment: move catastrophic or otherwise high-blast-radius work to WSL2 or a container, and if that boundary is unavailable, stop (ADR-0033) |

The test runner's per-target `shell` is `auto` by default (`sh` on
macOS/Linux/WSL2, `cmd.exe` on native Windows). This keeps generated `&&`
command chains compatible with the shells built into each OS. Set it to `sh`,
`powershell`, or `cmd` only when the consuming project's toolchain requires a
specific shell. `/cf-customize` must canary the selected shell and commands.

### Codex permission profile

The `cf-guard` permission profile in `.codex/config.toml` (selected via
`default_permissions`, extending `:workspace`) denies the home-directory secret
stores and high-confidence workspace key material (`~/.ssh`, `~/.aws`, `.env`,
`*.key`, `*.p12`, …) at the OS-sandbox layer, so unlike the PreToolUse guards
it holds even in headless `codex exec` (ADR-0014). The `gh` and `docker`
tool-token stores are deliberately left readable so those tools can read their
own tokens.

| Key | Value | Why |
|---|---|---|
| `default_permissions` | `cf-guard`, extending `:workspace` | the profile is the only sandbox configuration, because a `sandbox_mode` key would shadow it |
| egress | broad public egress, exact loopback for local UI tests, live search | private destinations and arbitrary Unix sockets stay closed when a session is launched without `--sandbox danger-full-access` |
| `approval_policy` | `never`; a reviewer or consult seat launches with no `--sandbox` flag, a builder seat with `--sandbox danger-full-access` (ADR-0055) | under `cf-guard` the seat gets workspace writes and deletes, read-only enforcement files, no Unix sockets and no local binding; with full access the OS sandbox is off for the process and git-guard, exec-guard, edit-guard, git hooks and CI remain the floor |
| secret-file denies | `.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `*.pfx`, `.netrc`, `id_rsa*`, `id_ed25519*`, at the workspace root only | the recursive `**/` forms made Codex refuse every directory delete, so a nested secret file, such as `sub/.env` or a linked worktree's `.env`, stays readable, changeable and deletable by the seat |
| `cf-builder` profile | defined, not selected | its spike ran fetch, worktree add, commit and builds unattended but could not push to a remote outside the workspace root (ADR-0075 D1) |
| environment | Codex's default `KEY`/`SECRET`/`TOKEN` scrub is kept (ADR-0025, ADR-0026) | the shell does not inherit provider secrets |
| `approvals_reviewer` | `approvals_reviewer = "auto_review"` | vestigial under `never`: it is not a human gate and does not fire on-request prompts |

`.codex/hooks.json` wires `codeflow hook git-guard` and
`codeflow hook exec-guard` onto Codex's `PreToolUse` (Bash) event and
`codeflow hook edit-guard` onto its `apply_patch` edits, each with
`--contract 3`, and `config.toml` enables the hooks engine. The `.codex/`
starter is part of the enforcement floor, shipped from `--minimal` up. Codex
loads a project's `.codex/hooks.json` only when that project's `.codex/` layer
is trusted: run `/hooks` inside an interactive `codex` session to trust the
CodeFlow hooks. Codex records that trust against the absolute path of the
hooks file, so each new clone or linked worktree asks again, and a changed
hooks file, such as the move to contract 3, asks again too. Codex's hook payload is byte-compatible with Claude's, so the same
binaries run unchanged.

`session-orient` is wired for Codex `SessionStart` too, for all sources
(ADR-0013), so an interactive Codex session opens with the same orientation
digest Claude gets, and re-orients to it after a compaction
(`source=compact`). One handler serves both harnesses with plain-text stdout
that each injects as session context. The `codex_hooks` test pins the JSON
wiring, while live firing rests on Codex's documented hooks contract.

### Credentials and tools

Authenticated command-line tools must be able to read their own configuration,
so the OS sandbox does not deny `~/.config/gh` or `~/.docker/config.json` to
every subprocess; doing so would also disable `gh` and Docker. Prefer OS
keychains and credential helpers rather than plaintext tokens in those files.
The harness denies direct file-reading tools, but a host that cannot provide
brokered or helper-backed credentials must treat arbitrary shell access as
credential-bearing and tighten that task's tool boundary.

| Concern | Rule |
|---|---|
| Tool inventory | a settings file cannot install or authenticate every task-specific tool. `/cf-customize` inventories and canaries authoritative-doc research, GitHub, the stack format/lint/test/coverage/security toolchain, browser/Playwright, Computer Use or a surface driver, design tooling, and project-specific Model Context Protocol (MCP) servers, and proposes only the missing pieces |
| Where authentication lives | OAuth, keychain, app/MCP, or supported credential-broker paths; raw tokens do not enter the repository, prompts, logs, or arbitrary commands |
| GitHub and Docker configuration | permitted for autonomous tool use only after `/cf-customize` proves secure keychain or credential-helper storage; on a keyring-less inline-credential host it adds a file deny until a broker is configured |
| Claude's credential mask | the shipped sandbox removes the exact Anthropic, OpenAI, and AWS raw credentials named in ADR-0026 from Bash without stripping credentials from every hook or stdio MCP, and leaves brokered tools and MCP processes available. When a project needs a raw GitHub, npm, Cargo, or provider token, prefer a broker or keychain; otherwise configure Claude's user/managed credential mask with TLS termination and exact `injectHosts` |
| Subprocess scrub | `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` is a user/managed hardening option only after proving the project's hooks and stdio MCPs do not require those provider credentials |
| Auto mode | Claude deliberately ignores repository requests for auto mode and classifier policy, so a Codex-hosted peer launches interactively with `--permission-mode auto` and CLI-scoped `autoMode.classifyAllShell`; `/cf-customize` can offer the equivalent user default but never writes it without approval |
| Codex review approvals | `auto_review` is configured but is not a human authorization path, and it does not pause for that subagent. Use `on-request` and `approvals_reviewer = "user"` only on a consult/no-edit lane when policy requires a human decision |

### Hosting the duo from either seat

A Codex-primary session can host the full duo, not only a consult. The host
owns orchestration and routes production by the qualified capability binding,
risk, evidence needs, and available capacity; no vendor receives implementation
work merely because of its name. From a Claude Code host, the official Codex
plugin provides the independent Codex lane. Both seats independently research
and plan before approving the same versioned contract; material design and
implementation receive cross-lineage review (ADR-0023, ADR-0046).

Google's Antigravity `agy` is **not** bound automatically: its hook dialect
differs and its macOS reliability is unresolved. The `cf-delegate` skill carries
an experimental, manual opt-in snippet for those who want it. As a *delegate*,
`agy` is retired: its only documented drive shape is headless one-shot, which
ADR-0018 prohibits.

### Dated verification stamps

Hook payload contracts and config schemas move fast on both sides, so every
stamp below records what was true on its date, not a current guarantee. The
release runbook ([releasing](releasing.md), "Re-verification before tagging")
re-verifies them before each CodeFlow tag.

| Claim | Observed on | Date |
|---|---|---|
| Hook payload and config-schema parity | codex-cli 0.144.3, Claude Code 2.1.220 | 2026-08-02 |
| `pre-push` refuses a Codex force-push to protected `main` | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| Headless `codex exec` did not run project PreToolUse hooks, even with `--dangerously-bypass-hook-trust` and the layer trusted | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| `agy` does not read `AGENTS.md` | Antigravity `agy` 1.0.15 | historical |

Treat the in-session guards as an interactive-session safeguard and rely on the
git-hook plane where Git invokes the installed hooks. CodeFlow's own flows no
longer produce headless runs: ADR-0018 makes cross-model transport
interactive-only, so the consult, delegate and duo flows never shell out to
`codex exec` and a headless Codex run is outside those flows.

Do not generalize
that historical Codex observation to every harness: Claude's
[programmatic-mode documentation](https://code.claude.com/docs/en/headless)
states that ordinary noninteractive sessions load project hooks, while bare
mode skips their automatic discovery.

This does not authorize headless CodeFlow
work. Earlier hook-specific evidence is retained by ADR-0008, ADR-0013, and
ADR-0014, and how far each plane reaches is in
[enforcement planes](architecture/enforcement-planes.md).
