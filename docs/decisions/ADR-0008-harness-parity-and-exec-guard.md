---
id: ADR-0008
title: harness parity and the exec-guard security stage
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the in-session guard plane gains a second PreToolUse (Bash) handler (exec-guard) and now binds Codex as well as Claude via a byte-compatible payload contract; the enforcement bullet and the four-planes description name it
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0008 — harness parity and the exec-guard security stage

## Context

Two gaps sat next to each other. First, the enforcement planes protected git
integrity thoroughly but had no in-session backstop for *destructive* Bash
commands (`rm -rf /`, `dd` to a block device, `mkfs`, fork bombs) — the
`security/dangerous.rs` and `security/privilege.rs` modules existed from v1 but
nothing in v2 ran them. Second, all in-session protection was Claude-only: the
`git-guard` PreToolUse hook bound a Claude session and nothing else, so an agent
driving the same repo through OpenAI's Codex CLI slipped every in-session guard.

The owner posture (verified against the charter and settled 2026-07-02) is
autonomy by default: project-scoped work runs promptless, and hard protections
exist for exactly three things — credential/secret reads, truly destructive
commands, and protected refs. The presets and this stage implement that
posture; the guards must bind whatever harness is driving the repo, not just
Claude.

Codex 0.142.5 turns out to ship a hooks engine whose `PreToolUse` payload is
byte-compatible with Claude's: the same field names (`hook_event_name`,
`tool_name:"Bash"`, `tool_input.command`, `session_id`, `cwd`, a nullable
`transcript_path`) plus a few extras (`turn_id`, `model`, `permission_mode`),
the same exit-code contract (0 allow / 2 block with a stderr reason), and a
per-project `<repo>/.codex/hooks.json` config with a one-time `/hooks` trust
step. That compatibility makes a single guard binary serving both harnesses
possible with no dialect layer.

## Decision

**exec-guard.** Add a `codeflow hook exec-guard` PreToolUse (Bash) stage
(`hooks/exec_guard.rs`) that runs the existing `dangerous` and `privilege`
security modules against `tool_input.command` and maps each to a level in a new
`policy.json` `security` section: `dangerous_commands` (default `block`) and
`privilege_escalation` (default `warn`), each `block|warn|off`. The layering
is deliberate:

- **dangerous = block.** These commands are never a legitimate project
  operation, so there is no sanctioned path to name — a hard block is the point.
- **privilege = warn, not block.** A `PreToolUse` exit-2 wins unconditionally,
  including over the settings `ask` tier that exists precisely to prompt a human
  before a `sudo` runs. A hard block here would override even an explicit human
  approval, so the exec-guard only advises; the `ask` tier owns the decision. A
  repo may set it to `block` in `policy.json`.

`network.rs` (v1 semantics conflict with the v2 PR doctrine) and `tmp.rs`
(inert without managed scratch folders) are deliberately not wired. The
`--minimal` tier transform softens only the `git` section, so on a scratch repo
`dangerous_commands` stays `block` exactly as `secret_scan` does.

**Harness parity.** The exec-guard payload parsing reuses the git-guard
`HookPayload`, which is lenient by construction (serde ignores unknown fields
and tolerates the nullable `transcript_path`), so both `codeflow hook <guard>`
binaries serve Claude and Codex unchanged. Ship a `.codex/` starter — a
`hooks.json` wiring both guards on the `^Bash$` matcher (10s timeout) and a
`config.toml` (hooks engine on, `sandbox_mode = "workspace-write"`,
`approval_policy = "on-request"`) — as managed scaffold assets at the standard
and full tiers. One-time hook trust is a `/hooks` step inside an interactive
Codex session; CI/headless runs pass `--dangerously-bypass-hook-trust`, which
runs enabled hooks without persisted trust and is safe only where the hook
source is already vetted (here, the repo's own committed config).

**Presets.** The three Claude settings presets adopt the autonomy posture:
`allow` grants the common project toolchain promptlessly (cargo/codeflow/git/gh
plus stack-agnostic npm/npx/python3/pytest/uv, `WebSearch`, and doc-domain
`WebFetch`); `ask` keeps the hard prompts (sudo/su/doas, `rm -rf` on `/` and
`~`, cargo/npm publish, `gh release`, `gh repo delete`); `deny` extends read
protection past the project cwd to the home-dir credential stores an agent must
never read (`~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, `~/.kube`,
`~/.docker/config.json`, `~/.cargo/credentials*`, `~/.claude`). Both PreToolUse
guards are wired. The init default preset flips to `acceptEdits` (the choice
stays user-owned). The `bypass-sandboxed` preset fixes the sandbox network key
to the schema's `allowedDomains` (the shipped value was `allowedHosts`, which is
not in the schema and was silently ignored) and adds `sandbox.filesystem.denyRead`
for OS-level read-blocking of the pure-secret home stores. `~/.config/gh` and
`~/.docker/config.json` are deliberately excluded from `denyRead` — the
allow-listed `gh`/`docker` tools must read their own tokens, and OS-denying them
would break the tooling; those paths stay protected at the `permissions.deny`
layer, which stops Claude's Read tool from dumping them without blocking a tool's
own subprocess.

**agy deferred.** Google's Antigravity `agy` CLI is not shipped as a bound
harness: its hook dialect differs (an `allow_tool` JSON contract, exit-0-always)
and its macOS reliability is unresolved. The cf-delegate skill documents a
manual, experimental opt-in snippet instead.

## Consequences

- Destructive Bash commands are blocked in-session for any bound harness;
  privilege escalation surfaces as feedback without overriding a human prompt.
- The same protection binds Codex, verified live: a headless `codex exec`
  instructed to force-push to `main` in a worktree is blocked by the git-guard
  the `.codex/hooks.json` wires (see the PR for captured evidence), and a clean
  command runs untouched.
- The presets make project-scoped work promptless while the deny/ask tiers and
  the guard hooks carry the three hard protections — no reliance on the
  permission mode for safety.
- CI and remote protection remain the authoritative perimeter (charter §6.5);
  these in-session guards are fast, harness-agnostic feedback, not the hard line.
- The `security` section is additive and defaulted, so a policy.json predating
  this ADR keeps working (missing section → strict defaults) and gains the keys
  on the next `codeflow update`.

## Architecture impact

`docs/architecture.md` updated in this PR: the in-session guard plane is now two
PreToolUse (Bash) handlers (`git-guard` + `exec-guard`) rather than one, and it
binds Codex as well as Claude through the byte-compatible payload contract. The
plane count is unchanged (the exec-guard is a second guard within the existing
harness-guard plane, not a fifth plane); the doctor check count is unchanged.
