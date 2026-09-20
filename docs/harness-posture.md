# Harness posture — autonomy, sandbox, and settings

<!-- Split out of docs/adoption.md: the runtime posture each harness gets, and
     how far the discipline reaches when the harness is not the one CodeFlow
     integrates. Adoption keeps install, tiers, ownership, and the daily loop. -->

**Autonomy is fail-closed by default and widened only by the operator.**

| Harness | What the shipped preset enables | What stays gated |
|---|---|---|
| Claude Code | Fail-closed OS sandbox, sandbox-contained Bash, web search/fetch, wildcard public-domain egress for dependency and tool subprocesses, local port binding for dev/UI tests | Private, link-local, and internal-name destinations; destructive, privileged, publish, and secret-read boundaries; unsandboxed retry only when auto-classified for a trusted installed tool that needs host state; auto mode and classifier policy are never taken from the repository |
| Codex | Guarded workspace permission profile (no legacy `sandbox_mode`), live search, `approval_policy = "never"`, `model_reasoning_effort = "high"`, production `--sandbox danger-full-access`, which turns the OS sandbox off for that process | Catastrophic work still stops for the operator; git-guard, exec-guard, git hooks, and CI remain the floor; `never` grants no access of its own — operations outside the effective sandbox fail instead of asking |
| Grok Build | Project hooks wired in `.grok/hooks/codeflow.json` — the same `git-guard` and `exec-guard` payload | The hooks load only after the one-time `/hooks-trust` (or `--trust`); the doctor check reports structural wiring, not trust state (ADR-0054) |

These are enabled in runtime settings as well as described in the skills
(ADR-0025). Approval policy and sandbox authority are separate controls
([Codex security](https://learn.chatgpt.com/docs/security)).

## Platform assurance

CodeFlow releases target macOS, Linux, and x86-64 native Windows. Windows users
can also run the Linux build inside WSL2; this is the preferred route for a
Linux-native toolchain or Claude work that needs OS-enforced sandboxing.

- macOS uses each harness's native sandbox. Linux and WSL2 use their Linux
  sandbox implementations; install the dependencies reported by the harness
  and keep fail-closed startup enabled.
- Native Windows Codex uses its elevated Windows sandbox by default. The
  scaffolded deterministic guard recognizes both Bash and PowerShell command
  events and Windows drive, system, profile, disk, recovery, and permission
  operations.
- Native Windows Claude Code can use Git Bash or PowerShell, but Claude's OS
  sandbox is not available there. The settings and hooks still apply, but they
  are not equivalent containment. Use WSL2 or a container for catastrophic or
  otherwise high-blast-radius work; if that boundary is unavailable, stop.
- The test runner's per-target `shell` is `auto` by default (`sh` on
  macOS/Linux/WSL2, `cmd.exe` on native Windows). This keeps generated `&&`
  command chains compatible with the shells built into each OS. Set it to `sh`,
  `powershell`, or `cmd` only when the consuming project's toolchain requires a
  specific shell. `/cf-customize` must canary the selected shell and commands.

The catastrophic classifier is a non-relaxable floor, not a complete endpoint
security product. Managed organization policy, least-privilege host accounts,
verified backups, and authenticated human approval remain necessary at higher
blast radii.

## Credentials and tools

Authenticated command-line tools must be able to read their own configuration,
so the OS sandbox does not deny `~/.config/gh` or
`~/.docker/config.json` to every subprocess; doing so would also disable `gh`
and Docker. Prefer OS keychains and credential helpers rather than plaintext
tokens in those files. The harness denies direct file-reading tools, but a host
that cannot provide brokered or helper-backed credentials must treat arbitrary
shell access as credential-bearing and tighten that task's tool boundary.

- A settings file cannot install or authenticate every task-specific tool.
  `/cf-customize` inventories and canaries authoritative-doc research, GitHub,
  the stack format/lint/test/coverage/security toolchain, browser/Playwright,
  Computer Use or a surface driver, design tooling, and project-specific MCPs.
  It proposes only the missing pieces. Authentication stays in OAuth, keychain,
  app/MCP, or supported credential-broker paths; raw tokens do not enter the
  repository, prompts, logs, or arbitrary commands. GitHub/Docker configuration
  is permitted for autonomous tool use only after `/cf-customize` proves secure
  keychain or credential-helper storage; on a keyring-less inline-credential
  host it adds a file deny until a broker is configured.
- The shipped Claude sandbox removes the exact Anthropic, OpenAI, and AWS raw
  credentials named in ADR-0026 from Bash without stripping credentials from
  every hook or stdio MCP. When a project needs a raw GitHub, npm, Cargo, or
  provider token, prefer a broker/keychain; otherwise configure Claude's
  user/managed credential mask with TLS termination and exact `injectHosts`.
  `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` is a user/managed hardening option only
  after proving the project's hooks and stdio MCPs do not require those
  provider credentials.
- Claude deliberately ignores repository requests for auto mode and classifier
  policy, so a Codex-hosted peer launches interactively with
  `--permission-mode auto` and CLI-scoped `autoMode.classifyAllShell`;
  `/cf-customize` can offer the equivalent user default but never writes it
  without approval.
- Codex `auto_review` is configured but is not a human authorization path.
  Production `approval_policy = "never"` plus `--sandbox danger-full-access`
  turns the OS sandbox off for that process, and it does not pause for that
  subagent. Catastrophic work still stops for the
  operator. Use `on-request` and `approvals_reviewer = "user"` only on a
  consult/no-edit lane when policy requires a human decision.

## How far the discipline reaches across harnesses

codeflow has two kinds of thing: **enforcement** (gates that block) and
**guidance** (instructions and skills that inform). They reach different
distances, so be precise about what a given harness actually gets:

- **Git hooks and CI are harness-neutral.** The git
  client hooks and CI are harness-agnostic: they act on git operations and PRs,
  not on which tool produced them. So conventional-commit format, the secret
  scan, no-AI-attribution, branch/push/protected-merge rules, and the test gate
  run for Claude Code, Codex, a future CLI, or a human when the relevant hooks
  and CI execute. Installed files do not make these checks unbypassable.
- **In-session guards require qualified harness integration.** The PreToolUse
  `git-guard`/`exec-guard` add fast, pre-git feedback. They are wired for Claude
  (`.claude/settings.json`) and, via a byte-compatible payload, an **interactive**
  Codex session (`.codex/hooks.json`, ADR-0008); the Grok adapter has its own
  qualified event contract. Historical headless hook gaps motivated ADR-0018,
  but are not a universal claim about every current harness. The
  consult/delegate/duo flows remain interactive-only independently of whether
  a headless mode can run hooks.
- **Guidance (AGENTS.md + the `cf-*` skills) is Claude + Codex.** Both read the
  repo `AGENTS.md` operating contract; the skills ship to `.claude/skills/`
  (Claude) and `.agents/skills/` (Codex). The **workflow** runtime
  (`pipeline.workflow.js`) is Claude-Code-only.
- **A harness codeflow does not specifically integrate** (for example Google's
  Antigravity `agy`) can still receive the git-hook plane + CI because those
  are harness-agnostic — but does **not** receive the in-session guards, the
  skills, or (historically verified on `agy` 1.0.15) the `AGENTS.md`
  instructions. Its reliable coverage is the configured, verified hook/CI
  plane, not assumed guidance.

The one-line version: CodeFlow shares policy across harness-neutral checks;
native guidance and in-session guards depend on the installed integration.
Missing planes are disclosed without relaxing safety or review duties.

## Codex parity

When a repo is driven through OpenAI's Codex CLI instead of Claude, protection
comes from two layers, and it helps to be precise about which does what.

- **The git-hook plane is harness-agnostic when installed and executed.**
  A Codex `git push --force origin main` against protected `main` is refused by
  the `pre-push` shim (`codeflow pre-push: BLOCKED — policy rule
  git.push_to_protected`) exactly as any agent's would be. This needs no Codex
  configuration, but local hook files and Git configuration remain editable; it
  is not an unbypassable boundary.
  An active prepared-transaction check that cannot read or evaluate its input
  blocks the transaction. Inspect the reported cause and repository/toolchain
  compatibility before retrying; preserve work and repair the supported path,
  rather than disabling the hook or automatically converting repository storage.
- **The in-session PreToolUse guards are an interactive-Codex bonus.** The
  scaffold ships a `.codex/` starter (part of the enforcement floor, from
  `--minimal` up): `hooks.json`
  wires `codeflow hook git-guard` and `codeflow hook exec-guard` onto Codex's
  `PreToolUse` (Bash) event, and `config.toml` enables the hooks engine with the
  guarded workspace profile in the table above plus
  `approvals_reviewer = "auto_review"` (vestigial under `never`: it is not a
  human gate and does not fire on-request prompts).
  The config intentionally contains no legacy `sandbox_mode`, because that
  would shadow the named profile. Codex's hook payload is byte-compatible with
  Claude's, so the same binaries run unchanged.

One-time setup for the in-session guards: Codex loads a project's `.codex/hooks.json`
only when that project's `.codex/` layer is trusted. Run `/hooks` inside an
interactive `codex` session once to trust the CodeFlow hooks. **Historical
note:** in testing on codex-cli 0.142.5, headless `codex exec` did not run
project PreToolUse hooks even with `--dangerously-bypass-hook-trust` and the
layer trusted — so treat the in-session guards as an interactive-session
safeguard, and rely on the git-hook plane where Git invokes the installed
hooks. codeflow's own flows no
longer produce headless runs: ADR-0018 makes cross-model transport
interactive-only (consult/delegate/duo never shell out to `codex exec`), so a
headless Codex run is outside those flows. Do not generalize that historical
Codex observation to every harness: Claude's
[programmatic-mode documentation](https://code.claude.com/docs/en/headless)
states that ordinary noninteractive sessions load project hooks, while bare
mode skips their automatic discovery. This does not authorize headless
CodeFlow work. In every mode, verify actual local hook execution and any
required CI/remote rules rather than inferring protection from installed files.

A Codex-primary session can host the full duo, not only a consult. The host
owns orchestration and routes production by the qualified capability binding,
risk, evidence needs, and available capacity; no vendor receives implementation
work merely because of its name. From a Claude Code host, the official Codex
plugin provides the independent Codex lane. Both seats independently research
and plan before approving the same versioned contract; material design and
implementation receive cross-lineage review (ADR-0023, ADR-0046).

Google's Antigravity `agy` is **not** bound automatically (its hook dialect
differs and its macOS reliability is unresolved); the cf-delegate skill carries
an experimental, manual opt-in snippet for those who want it. As a *delegate*,
`agy` is retired: its only documented drive shape is headless one-shot, which
ADR-0018 prohibits.

## Verification stamps are historical

These surfaces — hook payload contracts, config schemas — move fast on both
sides, so every stamp below records what was true on its date, not a current
guarantee. The release checklist ([docs/releasing.md](releasing.md)) re-verifies
them before each codeflow tag.

| Claim | Observed on | Date |
|---|---|---|
| Hook payload and config-schema parity | codex-cli 0.144.3, Claude Code 2.1.220 | 2026-08-02 |
| `pre-push` refuses a Codex force-push to protected `main` | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| Headless `codex exec` did not run project PreToolUse hooks | codex-cli 0.142.5 | historical, retained by ADR-0008 |
| `agy` does not read `AGENTS.md` | Antigravity `agy` 1.0.15 | historical |

Earlier hook-specific evidence is retained by ADR-0008, ADR-0013, and ADR-0014.
