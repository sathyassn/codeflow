---
id: ADR-0008
uid: 2266298b-4f1d-45f1-bdcd-cc581db309f5
title: harness parity and the exec-guard security stage
date: 2026-07-02
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the in-session guard plane gains a second PreToolUse (Bash) handler (exec-guard) and can bind an interactive Codex session via a byte-compatible payload contract (headless codex exec 0.142.5 does not run project PreToolUse hooks, so headless Codex leans on the git-hook plane); the enforcement bullet and the four-planes description name it
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

Codex 0.142.5 ships a hooks engine whose `PreToolUse` payload is byte-compatible
with Claude's: the same field names (`hook_event_name`, `tool_name:"Bash"`,
`tool_input.command`, `session_id`, `cwd`, a nullable `transcript_path`) plus a
few extras (`turn_id`, `model`, `permission_mode`), the same exit-code contract
(0 allow / 2 block with a stderr reason — the binary carries the string
`Tool call blocked by PreToolUse hook`), and a per-project `<repo>/.codex/hooks.json`
config with a one-time `/hooks` trust step. That schema compatibility means a
single guard binary can serve both harnesses with no dialect layer — the
in-session guard binds an interactive Codex session exactly as it binds Claude.
(Live testing surfaced one important limit, recorded under Consequences: headless
`codex exec` did not invoke project PreToolUse hooks in 0.142.5, so headless
Codex leans on the git-hook plane rather than the in-session guard.)

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
binaries serve Claude and Codex from one code path. Ship a `.codex/` starter — a
`hooks.json` wiring both guards on the `^Bash$` matcher (10s timeout) and a
`config.toml` (hooks engine on, `sandbox_mode = "workspace-write"`,
`approval_policy = "on-request"`) — as managed scaffold assets at the standard
and full tiers. Codex auto-discovers a project `.codex/hooks.json` **only when
the project `.codex/` layer is trusted**; the trust is granted once via the
`/hooks` command inside an interactive Codex session (it records a per-hook hash).
`--dangerously-bypass-hook-trust` bypasses only the per-hook trust check, not the
project-layer trust. These in-session guards are therefore an **interactive**
Codex safeguard; headless `codex exec` is covered by the git-hook plane (see
Consequences for the measured behavior).

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

- Destructive Bash commands are blocked in-session (Claude, and interactive
  Codex once its `.codex/` layer is trusted); privilege escalation surfaces as
  feedback without overriding a human prompt.
- **Live validation (codex-cli 0.142.5), recorded honestly.** Two facts hold and
  one limit was found:
  - *Git-hook plane binds Codex — verified.* A Codex-driven
    `git push --force origin main` against protected `main` is refused:
    `codeflow pre-push: BLOCKED — policy rule git.push_to_protected` (git exits 1,
    nothing pushed). This is harness-agnostic and independent of Codex's hook
    engine.
  - *Payload schema is compatible — verified.* The Codex `PreToolUse` payload and
    the exit-2 block contract match Claude's (confirmed against the Codex binary's
    embedded schema and the `Tool call blocked by PreToolUse hook` string).
  - *Headless `codex exec` did not run project PreToolUse hooks — measured limit.*
    With the project layer trusted, `--dangerously-bypass-hook-trust`, and
    `features.hooks = true`, an observable marker hook never fired for either the
    `.codex/hooks.json` or the inline `[[hooks.PreToolUse]]` form, and a plain
    `echo` ran unblocked. The `.codex/` in-session guards therefore apply to
    interactive Codex sessions, not headless `exec`; headless Codex protection
    rests on the git-hook plane above. The starter still ships (interactive Codex
    is a real use), framed as an interactive safeguard.
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
PreToolUse (Bash) handlers (`git-guard` + `exec-guard`) rather than one, and can
bind an interactive Codex session through the byte-compatible payload contract
(headless `codex exec` protection stays on the git-hook plane). The plane count
is unchanged (the exec-guard is a second guard within the existing harness-guard
plane, not a fifth plane); the doctor check count is unchanged.

## Amendment: 2026-09-27 (appended, TSK-137)

Removed: the v1 scanner modules that no guard ran, `security/path.rs`,
`fileops.rs`, `branch.rs`, `tmp.rs` and `network.rs`, the git scanning in
`security/git.rs` (only `is_on_protected_branch` stays, for the policy
loader), the unused helpers in `security/pattern.rs`, and `SecurityChecker`
with its module list, about 3,000 lines with their tests. Why: exec-guard runs
only `dangerous.rs` and `privilege.rs` through its own stage, and git-guard
has its own rules, so the removed code had no production caller: a second set
of rules that never ran beside the ones the live guards own. Nothing an
adopter configures or sees changes.

Changed at the same time: recursive removal strictly below a temp root is
no longer classified as a protected target. An agent's scratch space lives
there, below the protected `/private` and `/var`. The roots are a fixed set:
`/tmp`, `/var/tmp` (canonically `/private/tmp` and `/private/var/tmp` on
macOS) and the macOS per-user `/private/var/folders/<xx>/<id>/T`.
Containment is canonical: the operand's longest existing prefix is resolved
through symlinks, and a `..` in the rest, a failed resolution or a glob
before the last component grants nothing, so a link under temp space that
leads to `/etc` stays blocked. A path that reads as temp space but whose
target cannot be established, including an operand still holding an escape
or quote, is refused through every spelling. `$TMPDIR` adds no root; one that canonically
is or lies below a root is protected itself. The roots, a glob over one and
every other system directory stay blocked.
