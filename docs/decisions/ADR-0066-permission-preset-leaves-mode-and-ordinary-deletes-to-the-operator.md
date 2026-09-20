---
id: ADR-0066
title: "Permission preset leaves the mode and ordinary deletes to the operator"
status: accepted
date: 2026-09-20
supersedes: []
superseded_by: []
architecture_impact: none
---

# ADR-0066: Permission preset leaves the mode and ordinary deletes to the operator

## Context

The shipped Claude Code preset (`assets/base/settings/default.json`) set
`permissions.defaultMode` to `default`. Since Claude Code 2.1.257 the `auto`
and `bypassPermissions` modes are honored only from user settings or the
launch flag, and a project setting outranks the user file, so the shipped
value could only pull an operator's chosen mode back to manual prompting. The
preset also asked before every `rm -rf <path>` and every safe branch delete
(`git branch -d`); ask rules fire in every mode, including bypass, so both
prompted on ordinary sandbox-confined work while adding no protection: the
sandbox confines writes to the working tree and the session temp directory,
`exec-guard` blocks the destructive command class outright, and git refuses to
delete an unmerged branch with `-d`. A review of the settings, sandbox and
hooks references on 2026-09-20 established these facts (SPC-011 planning
record, EPC-015).

## Decision

The default preset no longer sets `permissions.defaultMode`; the opt-in
presets keep theirs because `acceptEdits` remains valid from project scope.
Every preset prompts only for the rooted and home-anchored recursive deletes
(`rm -rf /`, `rm -rf /*`, `rm -rf ~*` and the `-fr` spellings) and for force
deletes of branches; `rm -rf <path>` inside the tree and `git branch -d` are
ordinary work. The credential-file denies, the force-push, publish and
privilege-escalation prompts, the hooks and the sandbox are unchanged. The
`~/.config/gh` and `~/.docker/config.json` reads stay out of the deny list so
that brokered tool configuration keeps working, as the preset tests already
require. No static sandbox exclusion ships in the preset: ADR-0029 keeps the
classified unsandboxed retry as the route for a trusted tool that cannot run
in the sandbox, and the preset tests pin that. Machine-specific sandbox
relief (excluded commands, Unix socket paths, cache write paths) never ships
in the preset; it belongs in the
git-ignored `.claude/settings.local.json` of the machine that needs it.

## Consequences

`codeflow update` adds new keys and union-adds array entries but never
removes a key or an entry, so an existing installation keeps its `defaultMode`
line and the old prompts until the project removes them by hand; a fresh
`codeflow init` gets the new shape. This repository's own
`.claude/settings.json` was edited in the same change so that it matches the
preset it ships.
Operators who want a prompt on every unsandboxed retry add
`Bash(dangerouslyDisableSandbox:true)` to their own ask list; the preset does
not, because it prompts on every hook-touching command in bypass mode.
ADR-0025's autonomy statement continues to apply; ADR-0029 amended its
sandbox-retry rule and this decision amends its mode line.
