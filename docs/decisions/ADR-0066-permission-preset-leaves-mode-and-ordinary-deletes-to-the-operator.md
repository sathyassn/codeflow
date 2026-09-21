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
`permissions.defaultMode` to `default`. A project `.claude/settings.json` or
`.claude/settings.local.json` ignores `auto` and `bypassPermissions`; its
other values, `default` and `acceptEdits`, apply from project scope and
outrank the user file, while `auto` and `bypassPermissions` apply only from
user settings, managed settings, a `--settings` file or the
`--permission-mode` flag (see "which mode a session starts in" in the
permission-modes reference). The shipped value could therefore only pull an
operator's chosen mode back to manual prompting. The preset also asked before
every `rm -rf <path>` and every safe branch delete (`git branch -d`); ask
rules fire in every mode, including bypass, so both prompted on ordinary work.
A review of the settings, sandbox and hooks references on 2026-09-20
established these facts (SPC-011 planning record, EPC-015).

Dropping those two prompts removes a confirmation layer rather than a
redundant one, so the remaining controls are worth stating precisely. What
covers an in-tree recursive delete is sandbox write confinement, to the
working tree, the session temp directory and any configured allowWrite paths,
plus git for tracked files. That limits the blast radius but does not make
untracked unique data recoverable, and it holds only while the command stays
sandboxed: `allowUnsandboxedCommands` is true, so a classified unsandboxed
retry is not confined. `exec-guard` covers a narrower set, `rm -rf` on `/`,
`~` or a system directory (`crates/codeflow-core/src/hooks/exec_guard.rs`),
and `rm -rf target/debug` is clean there; those rooted, home and system paths
are exactly the ones that keep their ask rules. `exec-guard` classifies
catastrophic paths, not recovery availability. `git branch -d` refuses a
branch that is not merged into its configured upstream, or into HEAD when it
has no upstream, so a branch tracking an identical `origin/topic` can be
deleted before its work reaches main. The worktree doctrine still sets the
requirement before any branch force-delete: PR state `MERGED` with the branch
tip equal to its recorded head SHA, or `git cherry` against the updated target
showing no unapplied `+` entry, and the owner confirmed inactive before a
proven-landed resource is closed. This preset change does not relax any of
that.

## Decision

The default preset no longer sets `permissions.defaultMode`. The opt-in
presets keep the mode they name, for two different reasons. `acceptEdits` is
honored from project scope, so `acceptEdits.json` selects it directly.
`bypassPermissions` in `bypass-sandboxed.json` is retained as the documented
intent of that preset; the value takes effect only when it reaches user
settings, managed settings, a `--settings` file or the launch flag
(`--permission-mode bypassPermissions`), and an explicit launch mode outranks
project settings. Ask rules still fire in bypass mode, and a `dontAsk` entry
denies a matching call outright instead of prompting.

Every preset prompts for the rooted and home-anchored recursive deletes
(`rm -rf /`, `rm -rf /*`, `rm -rf ~*` and the `-fr` spellings) and for force
branch deletes, with a boundary worth stating exactly. Ask rules match the
command text as written, so the first option after `git branch` must be `-d`,
`--delete`, `-D`, or a cluster that itself begins with `-d`, `-D` or `-f`.
Within that shape the prompt covers force spelled `-f` or `--force` in any
position and the two-letter clusters `-df`, `-fd`, `-Df` and `-fD`. The
quiet-force clusters `-qf` and `-fq` are covered only after the delete flag,
as in `git branch -d topic -qf`, never as the leading option: nothing matches
`git branch -qf -d topic`. Two things fall outside the shape, because prefix
globs cannot express them: a force flag inside any other aggregated cluster,
and any option placed before the delete flag, as in
`git branch -q -d topic --force`.

The layer that does not depend on option order is `git-guard`, which blocks
deletion of a protected branch whether the delete flag stands alone or sits
inside a short option cluster, and which reads the branch name with each
command's own option arity, so a protected name is still found behind a
consumed option value or after a `--`. This change fixes the cluster case and
the operand case, both of which the guard previously missed. The ask layer
covers the listed force forms for any branch. The residual is therefore force
deletion of an unprotected unmerged branch through an uncovered option order,
which the worktree doctrine's landing evidence governs, not an ask rule.

A `rm -rf <path>` inside the tree and a `git branch -d` are ordinary work. The
residual risk there is plain: an in-tree recursive delete of untracked unique
data now proceeds without a prompt, and nothing in the preset restores that
data. Ordinary deletion stays limited to authorized, recoverable work by the
operator's own discipline, not by an ask rule.

The credential-file denies, the force-push, publish and privilege-escalation
prompts, the hooks and the sandbox are unchanged. The `~/.config/gh` and
`~/.docker/config.json` reads stay out of the deny list so that brokered tool
configuration keeps working, as the preset tests already require. No static
sandbox exclusion ships in the preset: ADR-0029 keeps the classified
unsandboxed retry as the route for a trusted tool that cannot run in the
sandbox, and the preset tests pin that. Machine-specific sandbox relief
(excluded commands, Unix socket paths, cache write paths) never ships in the
preset; it belongs in the git-ignored `.claude/settings.local.json` of the
machine that needs it.

## Consequences

`codeflow update` union-adds permission entries and adds keys, and never
removes a permission entry or a key. It does remove two managed things:
retired sandbox array entries that the previous baseline shipped, and stale
`codeflow hook` commands the preset no longer wants (`merge_sandbox` and
`remove_retired_array_entries` in
`crates/codeflow-core/src/scaffold/settings_merge.rs`, and the hook
regeneration near line 251 of the same file). Neither reaches
`permissions.defaultMode` or an ask entry, so an existing installation keeps
its `defaultMode` line and the old prompts until the project removes them by
hand; a fresh `codeflow init` gets the new shape. This repository's own
`.claude/settings.json` and update baseline were regenerated from the preset
in the same change.
Operators who want a prompt on each unsandboxed retry add
`Bash(dangerouslyDisableSandbox:true)` to their own ask list; that rule
matches the explicit `dangerouslyDisableSandbox` parameter on an unsandboxed
retry, not every hook-touching command. The preset does not ship it because
the prompt then fires on every retry in every mode.
ADR-0025's autonomy statement continues to apply; ADR-0029 amended its
sandbox-retry rule and this decision amends its mode line.
