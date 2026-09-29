---
id: ADR-0026
uid: 650c8f3b-0968-4eb5-bd04-19c5a0844b56
title: correct harness credential and destructive-action boundaries
date: 2026-07-17
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the runtime-settings plane now names the high-confidence workspace secret globs, raw environment credentials, and destructive operations enforced by each native harness
---

# ADR-0026: harness permission-boundary corrections

## Context

ADR-0025 enabled public research, dependency access, local UI testing, and
autonomous work inside the Claude and Codex sandboxes. A follow-up audit against
the current vendor references and live canaries found narrower gaps:

- the Codex profile did not explain or test the intentional choice of
  `mode = "full"` rather than `limited`;
- the Codex profile denied workspace `.env` files but allowed private-key and
  certificate-container files such as `client.key` and `client.p12`;
- Claude's sandbox protected secret files but exposed inherited raw model and
  cloud credentials to arbitrary sandboxed Bash, while its allow list covered
  destructive `git restore`, checkout-discard, force-push, remote-delete, and
  forced local-branch-delete forms more broadly than its ask list.

The repository fallback mode is not the issue. Claude Code deliberately ignores
`defaultMode: "auto"` and `autoMode` in project settings so an untrusted
repository cannot grant itself classifier trust. `acceptEdits` remains the
ordinary consuming-project fallback; Codex-hosted Claude peers select Auto and
`classifyAllShell` explicitly at CLI scope. `bypassPermissions` remains an
isolated-container/VM-only preset.

## Decision

Keep the public-network and local-verification capabilities from ADR-0025 and
correct the boundaries around them.

The Codex `cf-guard` profile:

- explicitly selects the supported `network.mode = "full"` subprocess proxy so
  research, dependency resolution, and tool APIs can reach public hosts, while
  retaining exact loopback exceptions, live web search, private-destination
  blocking, and automatic review of requested escalations;
- denies `*.pem`, `*.key`, `*.p12`, `*.pfx`, workspace `.netrc`, and common
  `id_rsa*` / `id_ed25519*` key names under every workspace root, in addition to
  `.env` files;
- denies sandboxed subprocess reads of `~/.codex/auth.json` without blocking
  the Codex host process that authenticates before it enters the command
  sandbox; and
- pins `shell_environment_policy.ignore_default_excludes = false`, retaining
  Codex's built-in case-insensitive `KEY` / `SECRET` / `TOKEN` environment
  filter if the upstream default changes.

The workspace glob scan is bounded by `glob_scan_max_depth = 6`. That is a
performance/safety tradeoff, not a claim that arbitrarily deep secret files are
unreadable. Project customization must tighten the depth when a deeper tree
requires it.

`approvals_reviewer = "auto_review"` sends eligible `on-request` escalation
prompts to Codex's reviewer subagent. It improves autonomy, but it does not make
the permission-profile denies absolute: an approved escalation can cross the
sandbox boundary. A consuming organization that requires a human decision for
every such request must select `user` in the project or launch override and may
restrict allowed reviewers through managed requirements.

Every Claude preset adds `sandbox.credentials.envVars` deny entries for
`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `OPENAI_API_KEY`,
`AWS_SECRET_ACCESS_KEY`, and `AWS_SESSION_TOKEN`. Claude Code 2.1.187+ removes
those exact variables from sandboxed Bash while the parent process keeps the
credential needed for its own API session. Existing Read deny rules already
merge into the OS sandbox's deny-read policy, so they are not duplicated under
`sandbox.filesystem`.

Every Claude preset also asks before any `git restore`, bare or targeted stash
drop, plus common checkout discard, forced branch reset/move, branch delete,
plain force-push, mirror/prune, and flag-based remote-delete spellings. Ask
rules take precedence over stale allow entries, so existing consumers gain the
correction through the additive settings merge. A generic deletion refspec such
as `git push origin :branch` cannot be represented safely: Claude treats a rule
ending in `:*` as a legacy literal-prefix rule, not a wildcard-anywhere rule.
The presets therefore do not ship a misleading inert rule; the git guard and
protected-branch policy remain the deterministic backstop for that grammar
gap. `--force-with-lease` remains available on feature branches under the same
backstop.

## Alternatives rejected

- Do not add `**/*secret*` or `**/*credentials*` to the Codex OS deny list.
  Unlike an app-level Read rule, that would also prevent compilers and tests
  from reading legitimate modules such as `secrets.rs`.
- Do not deny `GITHUB_TOKEN`, `GH_TOKEN`, `NPM_TOKEN`, or
  `CARGO_REGISTRY_TOKEN` in the generic project preset. Claude's deny mode
  removes the value and would break the corresponding authenticated tool.
  Prefer OAuth/keychain/credential-helper paths, or a user/managed/CLI
  credential mask with TLS termination and exact `injectHosts`.
- Do not ship `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB=1` as a generic project
  default. It also removes provider credentials from hooks and stdio MCP
  servers, breaking legitimate task-specific tools. `/cf-customize` may
  recommend it at user or managed scope when those subprocesses do not need the
  credentials.
- Do not put Claude Auto in the shared project settings and do not replace the
  ordinary fallback with bypass mode. The former is ignored; the latter removes
  the classifier boundary and is only appropriate under separate host
  isolation.

## Consequences

- Public research, package resolution, authenticated MCPs, and loopback UI
  tests remain available.
- High-confidence workspace key material and raw model/cloud environment
  credentials are unavailable to ordinary sandboxed commands.
- Common destructive source-control spellings no longer inherit a broad allow
  through a narrower ask list; protected deletion refspecs remain hook-backed.
- Claude versions older than 2.1.187 degrade to the prior environment-variable
  posture; `/cf-customize` must surface the version gap during its live CLI
  preflight.
- Organization-enforced, non-bypassable policy still belongs in managed Claude
  settings and Codex `requirements.toml`; repository settings remain a strong
  project default, not an administrative boundary.

## Sources

- Claude Code permission modes and project-scope Auto behavior:
  <https://code.claude.com/docs/en/permission-modes>
- Claude Code sandbox credentials and environment-variable protection:
  <https://code.claude.com/docs/en/sandboxing>
- Claude Code environment variables:
  <https://code.claude.com/docs/en/env-vars>
- Codex configuration and named permission profiles:
  <https://learn.chatgpt.com/docs/config-file/config-reference>,
  <https://learn.chatgpt.com/docs/permissions>
