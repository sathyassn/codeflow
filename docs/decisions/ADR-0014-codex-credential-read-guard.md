---
id: ADR-0014
uid: 842f1a32-5bb4-4edb-87c7-5b11a4823da5
title: Codex credential read-guard via a named permission profile (cf-guard)
date: 2026-07-05
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the Codex parity note records that credential READS are now guarded too, via an OS-sandbox `cf-guard` permission profile (default_permissions) in .codex/config.toml, not only the Bash PreToolUse guards
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0014: Codex credential read-guard via a named permission profile (cf-guard)

## Context

ADR-0008 gave Codex parity for the in-session *Bash guard* plane (git-guard,
exec-guard) but left a gap: Claude's presets also deny the model reading the
home-dir credential stores (`permissions.deny` + `sandbox.filesystem.denyRead`),
and Codex had no equivalent — a Codex-driven session could read `~/.ssh`,
`~/.aws`, `.env`, etc. freely. Codex 0.142.5 exposes a `[permissions.<id>]`
profile system (verified live against the installed binary: a profile selected
by `default_permissions`, extending a built-in like `:workspace`, with a
`filesystem` map of glob→`"deny"`) whose denies are enforced by the OS sandbox
for every subprocess — so unlike the PreToolUse hooks (which headless
`codex exec` does not fire), a filesystem deny holds headless too.

## Decision

Ship a named `cf-guard` permission profile in `assets/base/codex/config.toml`
(and the dogfood `.codex/config.toml`), selected with
`default_permissions = "cf-guard"`. It `extends = ":workspace"` (workspace-write:
read anywhere, write the workspace) and its `[permissions.cf-guard.filesystem]`
map denies the PURE-SECRET stores Claude's deny-list blocks: `~/.ssh/**`,
`~/.aws/**`, `~/.gnupg/**`, `~/.netrc`, `~/.kube/**`, `~/.cargo/credentials*`,
and `~/**/.env` (a workspace `.env` included). Deliberately NOT denied:
`~/.config/gh` and `~/.docker/config.json`. This mirrors the Claude nuance but
for a different reason — Claude's deny is app-level (only its Read tool), whereas
this OS deny blocks ALL subprocesses, so denying those would break the `gh` and
`docker` tools that read their own tokens; keeping them readable is the
tool-token carve-out.

## Consequences

- Codex credential reads are guarded, OS-sandbox-enforced, so the deny holds in
  headless `codex exec` too (where PreToolUse hooks do not fire) — closing the
  last credential-parity gap with Claude for both interactive and headless Codex.
- **Escalation caveat (honest limit).** A user-level profile can be bypassed if
  the model requests `require_escalated` and a human approves it under
  `approval_policy = "on-request"`. Headless `codex exec --ask-for-approval`
  never auto-approves an escalation, so the deny holds unattended; interactive
  sessions rest on the human not approving a read of a secret store.
- **Unbypassable option, pointed to but not shipped as default.** For org
  enforcement that no user config can override, the requirements-level
  `[permissions.filesystem]` `deny_read = [...]` key applies — but it loads only
  from a managed `requirements.toml` / MDM layer (e.g.
  `/etc/codex/requirements.toml`), not a plain user `config.toml`, so it is an
  admin deployment, out of scope for the shipped scaffold.
- Pinned by `crates/codeflow-cli/tests/codex_config.rs` (structural: parses the
  shipped TOML, asserts the selection, the `:workspace` base, the deny map, and
  the gh carve-out); adds `toml` as a cli dev-dependency. Live sandbox behavior
  rests on the verified Codex 0.142.5 mechanism, not a runtime test here.
- Additive and defaulted: a `.codex/config.toml` predating this ADR keeps working
  and gains the profile on the next `codeflow update`.

## Architecture impact

`docs/architecture.md` updated in this PR: the Codex parity note records that
credential READS are now guarded too — via the OS-sandbox `cf-guard` permission
profile in `.codex/config.toml` — not only the Bash PreToolUse guards of
ADR-0008. Extends ADR-0008 harness parity (ADR-0008 is not edited).
