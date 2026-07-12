---
id: ADR-0019
title: enforcement is the floor — tiers scale project-management, not discipline
date: 2026-07-11
status: accepted
superseded_by: null
architecture_impact: tier-boundary only — the four enforcement planes now install from --minimal up; docs/architecture.md and the adoption/capabilities/product tier text are clarified to say so. No engine module, ownership class, or boundary moves; the update engine already reconciles in-tier-but-missing manifest entries.
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0019 — enforcement is the floor

## Context

codeflow bundles two orthogonal concerns onto one tier slider: (A) git-discipline
**enforcement** — universal, near-zero cost, no dependency on project shape; and
(B) **project-management machinery** — opinionated, scaling with the project.
Until now A was split across the slider. `--minimal` installed only the
pre-commit secret scan (plus `.gitignore`, `policy.json`, and a trimmed
`AGENTS.md`); the rest of the enforcement plane — the `commit-msg`, `pre-push`,
`pre-merge-commit`, and `reference-transaction` hooks, the scaffolded CI, and the
in-session `git-guard`/`exec-guard` — arrived only with `--standard`, which also
drags in the entire method (cf-* skills, reviewer agents, the pipeline) and the
traceability spine (product / capabilities / architecture / ADRs).

That is backwards. The armed `policy.json` already ships at every tier (the
earlier removal of minimal-policy softening: the rules are set to `block` from
the first commit regardless of tier), so at `--minimal` the rules were *declared*
but not *enforced* — the hooks that read them weren't installed. Meanwhile nobody
wants B without A, and many want A without B: doc-sets, config repos, and small
tools want the git discipline without the code-project scaffolding. The misfit
surfaced concretely with agent-os — a documentation set that wanted commit
format, branch, and protected-branch enforcement but had no use for the
capability registry, epics, or the six-layer knowledge model. Reaching for
enforcement forced the whole `--standard` payload on it.

## Decision

**Enforcement is the floor; the tiers scale project-management.** The tiers stay
a clean superset — minimal ⊂ standard ⊂ full — split on that seam:

- **minimal** = the COMPLETE four-plane enforcement floor + a lean agent
  contract, for ANY repo: all five git-hook shims (`pre-commit`, `commit-msg`,
  `pre-push`, `pre-merge-commit`, `reference-transaction`), the scaffolded CI
  (`codeflow-ci.yml`), the in-session guards (`.claude/settings.json` wiring
  `git-guard`/`exec-guard` + the orient/summary session hooks, and the `.codex/`
  starter for interactive Codex), the armed `policy.json`, `.gitignore`, and both
  `AGENTS.md` + a new lean `CLAUDE.md`.
- **standard** = + the develop-loop method (cf-* skills, reviewer agents, the
  pipeline) + the traceability spine (product / capabilities / architecture /
  ADRs) + the full contract + harness convenience, for a code project.
- **full** = + project-management (epics, tasks, specs), for a program.

The manifest entries that MOVED into the minimal tier (each becomes
`["minimal","standard","full"]`): `git-hooks/commit-msg`, `git-hooks/pre-push`,
`git-hooks/pre-merge-commit`, `git-hooks/reference-transaction`,
`ci/codeflow-ci.yml`, the three disjoint-selected settings presets
(`settings/default.json`, `settings/acceptEdits.json`,
`settings/bypass-sandboxed.json` → `.claude/settings.json`), and the two Codex
starter files (`codex/hooks.json`, `codex/config.toml`). A new
`CLAUDE.minimal.md.tmpl` ships the lean `CLAUDE.md` that documents the now-wired
session hooks and guards without the cf-* workflow ladder (disjoint from the full
`CLAUDE.md.tmpl` by the same same-dest / tier-selector shape `AGENTS.md` uses).
Everything standard-and-up (skills, reviewer agents, the pipeline, the spine, the
PR template) and full-only (project-management) is UNCHANGED. Standard and full
gain nothing and lose nothing; only minimal changes, gaining the enforcement it
should always have had.

## Consequences

- **Existing minimal installs GAIN full enforcement on their next `codeflow
  update` — additive and beneficial.** The update reconciliation already installs
  a manifest entry that is in-tier but missing on disk (every ownership branch
  has a "dest does not exist → install" path), so an old-minimal repo's next
  update simply *adds* the moved hooks, CI, settings, and Codex starter and
  records them. No engine change was required; the behavior is pinned by
  `crates/codeflow-cli/tests/tier_floor_e2e.rs`
  (`update_from_old_minimal_installs_the_newly_in_tier_files`).
- **The change is additive — nothing is removed from any tier — so it is a MINOR
  bump (v2.2.0).** A repo that deliberately opted a moved file out via
  `[scaffold] ignore` keeps that opt-out; the reconciliation respects the ignore
  globs.
- **The `AGENTS.minimal` honesty text reverses.** It previously stated that at
  minimal the commit-format / attribution / emoji / branch / push rules were
  "contract, not hook-blocked… until `--standard`." They are now all mechanically
  enforced at minimal; the `--standard` upgrade is reframed as adding the
  *method*, not the enforcement.
- **A universal working principle rides along.** "Think independently — not a
  yes-man" is added to both the minimal and full contracts' workflow-discipline
  section: analysis is owed on assertion, the operator's included, and the
  operator still makes the final call. It is universal, so it belongs at minimal
  too.
- **Prior tier definitions are superseded (append-only — noted here, not
  edited).** The charter §4.2 tier table (`--minimal` = "AGENTS.md + secret-scan
  pre-commit + gitignore. Branch policy at warn.") and charter decisions D9/D16
  describe both the old policy-softening (already removed) and the old thin
  minimal; they are historical plan-of-record and are superseded by this ADR.
  ADR-0007, ADR-0008, and ADR-0009 each reference a `--minimal` tier *transform*
  that softens the `git` policy section — that transform no longer exists (the
  armed policy ships at every tier), and this ADR does not reintroduce it: it
  moves the enforcement *machinery* to minimal, leaving the policy values armed
  everywhere as they already were.

## Architecture impact

Tier-boundary only. The four enforcement planes described in
`docs/architecture.md` are unchanged in shape; what changes is the tier at which
they install — now `--minimal` up. `docs/architecture.md`, `docs/adoption.md`,
`docs/capabilities.md`, and `docs/product.md` are updated to state that the
four-plane floor is the minimal floor. No engine module, ownership class, or
boundary moves; the manifest is the only code-adjacent surface that changed, plus
the new `CLAUDE.minimal.md.tmpl` asset and the tier-boundary tests.
