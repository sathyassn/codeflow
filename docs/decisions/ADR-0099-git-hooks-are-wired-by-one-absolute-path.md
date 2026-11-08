---
id: ADR-0099
uid: ef785c47-cfaf-4545-bc68-54bc344df23c
title: "Git hooks are wired by one absolute path"
date: 2026-11-08
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0099: Git hooks are wired by one absolute path

## Context

ADR-0055 item 5 set `core.hooksPath` to the project-relative managed hooks
folder, so each worktree ran the shims from its own checkout, and ruled out
machine-absolute hook paths. Git reads a relative `core.hooksPath` against
the directory a command runs in, so whether CodeFlow's git hooks run
depended on how an invocation names its repository and working directory
(TSK-1395). The shims in every worktree are the same file, and each one only
calls the installed `codeflow` binary.

## Decision

`codeflow init` writes `core.hooksPath` as one absolute path: the managed
hooks folder of the repository's main working tree, spelled as git spells
working trees (forward slashes on Windows). Linked worktrees share it through
the common configuration. A bare repository with only linked worktrees takes
the working tree the writing command ran in, and so does a repository whose
main working tree does not hold the managed hooks. `codeflow update`
rewrites the relative form, and a path that no current working tree of the
repository holds (a moved or renamed repository, a removed worktree), and
wires an unset path where the project records its hooks as wired; a path to
another hook manager's folder is left alone. `codeflow doctor` reports the
relative form and a stale path, and names `codeflow update`. This supersedes
ADR-0055 item 5's hooks sentences; the rest of that item stands.

## Consequences

- Hooks run the same for every invocation in the repository, with no step
  for adopters: the next `codeflow update` migrates the path.
- A worktree no longer runs shims from its own checkout. A branch that edits
  its shims therefore no longer changes which hooks run for that worktree,
  and testing a shim change needs a scratch project.
- The path is machine-specific. Moving or renaming the repository, or
  removing the working tree it names, leaves it stale until `codeflow
  update` runs; doctor reports it.
- Per-worktree configuration (`extensions.worktreeConfig`) was not used: it
  turns on a repository extension, and a worktree made by plain `git
  worktree add` would fall back to the shared value anyway.
- To revert, restore the relative path in init and doctor and remove the
  migration from update; each project then sets the relative path again,
  which the restored doctor check names.

## Architecture impact

none
