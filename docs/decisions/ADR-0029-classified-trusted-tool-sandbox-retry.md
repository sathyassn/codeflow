---
id: ADR-0029
uid: 862f675b-9292-45f8-9d44-45470cc5cb67
title: permit classified sandbox retry for trusted installed tools
date: 2026-07-18
status: accepted
superseded_by: null
architecture_impact: the Claude runtime boundary now permits a classifier-reviewed host retry for trusted installed tools whose state cannot be used from the OS sandbox, while preserving the sandbox as the default execution boundary
---

# ADR-0029: classified trusted-tool sandbox retry

## Context

ADR-0025 disabled Claude's unsandboxed retry path. A native interoperability
canary then proved that the official `codex@openai-codex` plugin can be read and
launched inside the sandbox but Codex app-server cannot initialize its SQLite
state under `~/.codex`. Authentication succeeds when the same signed-in plugin
command is retried outside the sandbox. Granting the whole Codex home directory
write access would also let arbitrary sandboxed subprocesses modify auth,
configuration, rules, and session state. A broad `node *` sandbox exclusion
would be wider still.

## Decision

Set `sandbox.allowUnsandboxedCommands` to `true` while retaining the fail-closed
OS sandbox, Auto mode with `autoMode.classifyAllShell: true` at user or CLI
scope, secret/read and credential masks, private-network denies, and destructive
or privileged ask rules. A model may request `dangerouslyDisableSandbox` only
after a sandbox-boundary failure and only for a trusted installed tool that
requires host state. The official Codex plugin is the motivating case.

This is a classified retry, not bypass mode. Arbitrary commands, source edits,
network work, and routine tests stay sandboxed. The retry must preserve the
exact command, be disclosed in evidence, and fail loudly when the classifier or
an explicit rule rejects it. A consuming organization that requires human
approval for every host retry can add `Bash(dangerouslyDisableSandbox:true)` to
`permissions.ask` at user or managed scope.

## Consequences

- Claude-hosted Codex setup and task commands can use the authenticated native
  app-server without exposing `~/.codex` to every sandboxed subprocess.
- Auto mode reviews the exceptional command; existing ask/deny rules keep their
  precedence, and ordinary work remains confined to the sandbox.
- The first failed sandboxed attempt may add small latency. It is retained as
  evidence that the exceptional route was necessary.
- Managed policy may disable or human-gate the retry. In that environment the
  duo degrades explicitly rather than weakening the administrative boundary.
- The narrow `~/.claude` secret-state deny set is a versioned harness snapshot,
  not a future-proof namespace boundary. Qualification of a new Claude Code
  release must inspect newly introduced sensitive state and extend the set
  before that release is approved.

## Sources

- Claude sandbox escape and `allowUnsandboxedCommands`:
  <https://code.claude.com/docs/en/sandboxing>
- Claude Auto mode and input-parameter ask rules:
  <https://code.claude.com/docs/en/permissions>
