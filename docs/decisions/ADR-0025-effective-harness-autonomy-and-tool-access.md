---
id: ADR-0025
title: effective harness autonomy with broad tools and guarded side effects
date: 2026-07-16
status: accepted
superseded_by: null
architecture_impact: scaffolded Claude and Codex settings now enable real public-network and tool autonomy while keeping secret, destructive, privileged, private-network, and unsandboxed boundaries explicit
---

# ADR-0025 — effective harness autonomy and tool access

## Context

Instructions cannot grant runtime capability. A skill that says “research the
current documentation” is ineffective when command network access is disabled;
a promise of autonomous verification is false when the browser, test runner, or
authenticated MCP cannot run. The inverse is also unsafe: bypassing approvals
and sandboxing on an ordinary host exposes credentials, private services, and
unrelated files.

The previous Codex scaffold selected both a named permission profile and the
legacy `sandbox_mode`. Current Codex permissions documentation says these do not
compose: a loaded legacy sandbox setting shadows `default_permissions`. The
profile therefore did not provide the claimed credential boundary. Its `.env`
glob was also home-relative rather than explicitly rooted in each workspace.

Claude separates project settings from auto-mode classifier settings. Current
Claude Code deliberately ignores `defaultMode: auto` and `autoMode` in project
settings so a repository cannot grant itself classifier trust. Sandbox,
permission, and hook rules are valid project settings; classifier policy must
come from user, managed, or explicit CLI `--settings` scope. Bypass mode skips
ordinary safety checks and is appropriate only inside a separately isolated
container or VM.

## Decision

Enable capability in the settings layer as well as describing it in the skill.

For Codex, ship a single named `cf-guard` permission profile with no competing
legacy sandbox keys. It extends the workspace-write profile, enables broad
public egress and exact loopback for local UI tests, keeps private/link-local
destinations and arbitrary Unix sockets closed, enables live web search, uses
`approval_policy = "on-request"`, and routes eligible escalations through
`approvals_reviewer = "auto_review"`. Workspace-relative `.env` globs and
home pure-secret stores remain deny-read. The shipped presets keep GitHub CLI
and Docker configuration available to the tools that authenticate through them,
but autonomous use is conditional on `/cf-customize` verifying
keychain/credential-helper backing. A keyring-less inline credential makes that
config a secret and requires an added deny plus a brokered replacement.

For Claude, every shipped preset enables the OS sandbox, fails closed if it
cannot start, auto-allows commands that remain inside the sandbox, disables the
unsandboxed retry escape, permits local port binding for dev/UI tests, allows
web search/fetch, and sets `sandbox.network.allowedDomains: ["*"]` so ordinary
dependency and tool subprocesses can reach public services. Denied-domain
patterns keep common private, link-local, and internal-name destinations out of
that broad path. Content-scoped ask rules keep destructive git/filesystem
actions, publishing, releases, and privilege escalation visible. Secret reads
remain denied. The bypass preset remains available only for an externally
isolated environment and does not become the ordinary peer path.

Codex-hosted Claude peer sessions launch as an interactive TTY with the latest
Fable alias at xhigh effort, `--permission-mode auto`, and explicit
`--settings '{"autoMode":{"classifyAllShell":true}}'`. The CLI scope is
effective even though the repository scope is intentionally ignored. If auto or
Fable is unavailable, record the gap and use the strongest reasoning model with
`acceptEdits` plus the same fail-closed sandbox; never fall through to bypass on
an ordinary host.

Treat tool readiness as a runtime fact. `/cf-customize` inventories and canaries
live research, authoritative docs, source control, format/lint/test/coverage and
security tools, browser/Playwright, Computer Use or a surface driver, design
tools, and project-specific MCP services. It proposes missing configuration and
authentication but never silently installs tools or edits global settings.
Authenticated access uses OAuth, keychain, an app/MCP broker, or supported
credential masking with exact destinations. Raw tokens never enter prompts,
logs, repository files, or arbitrary commands.

`codeflow init` points standard/full consumers to `/cf-customize`, and
`codeflow doctor` warns while product, architecture, or agent-context sentinels
remain. The minimal tier stays method-free.

## Consequences

- Public research and dependency/tool access work without granting unrestricted
  host access.
- Local browser/UI verification can bind loopback ports while private-network
  services and dangerous socket escapes remain closed by default.
- Automatic review and Claude auto-mode classification reduce routine prompts
  without weakening deterministic boundaries.
- Repository settings cannot guarantee a user/organization classifier policy;
  preflight must inspect the effective mode and run a live canary.
- Projects with specialized authenticated tools may need user- or
  organization-scoped broker configuration, which `/cf-customize` surfaces and
  never fakes.

## Architecture impact

The scaffold settings are active runtime policy rather than documentation-only
defaults. Init and doctor expose the customization lifecycle. The core binary
still does not authenticate services, mutate global harness configuration, or
route model turns.

## Sources

- Codex permissions and automatic review:
  <https://developers.openai.com/codex/codex-manual.md#permissions>,
  <https://developers.openai.com/codex/codex-manual.md#auto-review>
- Claude Code permission modes, auto-mode configuration, and sandbox:
  <https://code.claude.com/docs/en/permission-modes>,
  <https://code.claude.com/docs/en/auto-mode-config>,
  <https://code.claude.com/docs/en/sandboxing>

## Note (2026-09-05)

ADR-0055 amends production-host autonomy: Claude `bypassPermissions` or
`auto`, Codex `approval_policy = "never"` plus `--sandbox danger-full-access`,
Grok `--always-approve`. Consult and no-edit review stay non-bypass. Git
hooks, git-guard, exec-guard, and CI remain the floor.
