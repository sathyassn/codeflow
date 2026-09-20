---
id: ADR-0011
title: update reconciles orphaned managed files (prune on upstream rename/removal)
date: 2026-07-04
status: accepted
superseded_by: null
architecture_impact: none
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0011: update reconciles orphaned managed files

## Context

`codeflow update` iterated only the entries of the **new** source manifest. A
managed file that the new manifest no longer ships — an artifact **removed or
renamed upstream** — was never reconciled: its file, its `.codeflow/.baseline/`
copy, and its `.codeflow/manifest.json` record all lingered. The
`.claude/commands/*` → `.claude/skills/*/SKILL.md` migration (v2.1.101 merged
custom commands into skills) exposed this concretely: `update` installed the new
`.claude/skills/cf-plan/SKILL.md` while leaving the stale
`.claude/commands/cf-plan.md` behind, and both resolve to `/cf-plan` — a
collision. This repo's own dogfood migration had to be done surgically for
exactly this reason.

The fix must not become a footgun: `update` deletes are irreversible from the
tool's point of view, and managed-region / user-owned files may hold user
content that codeflow does not own.

## Decision

After applying the new manifest, `update` runs an **orphan reconciliation** pass.
An orphan is an installed record whose `dest` is absent from the new manifest
**across all tiers** (so a tier *downgrade* — the file is still shipped at a
higher tier — never prunes; that stays "stop managing, never delete").

Reconciliation policy, by ownership — **never delete user data**:

- **`managed`** (whole-file, codeflow-owned): if the on-disk file is
  **unmodified** (current hash == recorded hash) or already gone, remove the
  file, its baseline, and its record (reported `removed`). If it is
  **user-modified**, keep the file on disk and merely drop it from management
  (baseline + record removed; reported `kept (user-modified)`).
- **`managed-region` / `user-owned`**: the file may contain content outside a
  codeflow block, so it is **never deleted** — only unmanaged (baseline + record
  removed; reported `skipped` with a note).

Now-empty ancestor directories of a removed file are cleaned up (stopping at the
first non-empty directory), so `.claude/skills/cf-plan/` does not linger after
`SKILL.md` is removed.

## Consequences

- Consumers migrating across an upstream rename/removal no longer accumulate
  orphaned managed files or collide with the renamed replacement — the primary
  motivation.
- No silent data loss: only unmodified codeflow-owned files are deleted; every
  file that could carry user content is preserved and the action is reported.
  The report/summary gains a `removed` line.
- The manifest invariant is unaffected (pruning removes records; it never
  rewrites a surviving record's hash).
- Scope guard: reconciliation is keyed on absence from the **whole** new
  manifest, not the tier-filtered subset, so it does not fight tier downgrades.

## Architecture impact

None — this completes the existing `update` reconciliation within the scaffold
engine; no new plane, boundary, or dependency.
