---
id: ADR-0010
uid: be944dda-fece-4c97-9824-582cd4056bc7
title: automated release — release-plz owns version/changelog/tag, cargo-dist owns artifacts
date: 2026-07-04
status: superseded
superseded_by: ADR-0012
architecture_impact: none
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0010 — automated release: release-plz proposes, cargo-dist builds

## Context

Version bumping was manual: edit `Cargo.toml [workspace.package] version`, write
the `CHANGELOG.md` section, push a `vX.Y.Z` tag. The tag already triggers
`cargo-dist` (`.github/workflows/release.yml`) to build the three target
binaries plus the shell installer and publish a GitHub Release. The manual step
is the weak link — it is easy to forget the changelog, mis-derive the SemVer
bump, or tag the wrong version. codeflow already *enforces* conventional commits
(the commit-msg hook: `fix` -> patch, `feat` -> minor, `!` / `BREAKING CHANGE:`
-> major), so the bump is mechanically derivable; nothing consumed it.

Constraints that shaped the choice:

- codeflow is distributed as **cargo-dist binaries, not a crates.io crate** —
  neither crate is published to a registry.
- The workspace has two crates that version in **lockstep** via
  `version.workspace = true` (`codeflow-core` is an internal library used only by
  `codeflow-cli`).
- This repo lands via **human-merged PRs** (ADR-0006/0007); agents never merge to
  a protected branch, and no automated actor should push to `main` unreviewed.

## Decision

Adopt **release-plz** for the version + changelog + tag half, and keep
**cargo-dist** as the artifact builder. The two compose at the tag:

- **release-plz** (`release-plz.toml` + `.github/workflows/release-plz.yml`)
  reads the conventional commits since the last tag and opens a **release PR**
  that bumps the workspace version and rewrites `CHANGELOG.md`. A **human merges
  that PR** — the same gate as any other change. On merge, release-plz creates
  the `vX.Y.Z` tag.
- **cargo-dist** stays exactly as-is; its existing tag trigger fires on the tag
  release-plz pushes and builds/publishes the release.

Configuration choices, each tied to a constraint above:

- `publish = false`, no `CARGO_REGISTRY_TOKEN` — nothing goes to a registry.
- `git_only = true` — with no registry, the last version is read from git tags.
- `git_release_enable = false` — **cargo-dist** authors the GitHub Release; if
  release-plz also created one they would collide.
- `git_tag_enable = true`, `git_tag_name = "v{{ version }}"` — release-plz cuts
  the single lockstep tag matching the established convention (`v2.0.0`), which is
  what `release.yml` matches; not the per-package default.
- `codeflow-core` is marked `publish = false` in its `Cargo.toml`, so release-plz
  sees one public package (`codeflow-cli`) and cuts one tag. `codeflow-core`'s
  version rides the workspace via inheritance (lockstep preserved).
- `semver_check = false` — an API semver check needs a registry baseline that
  does not exist here.
- `release_always = false` — a release happens only when the release PR merges,
  never on every push to `main`.

**Token.** Both jobs prefer a fine-grained PAT (`RELEASE_PLZ_TOKEN`) and fall
back to the built-in `GITHUB_TOKEN`. The PAT is effectively **required** for the
chain to work end to end: a PR or tag created with `GITHUB_TOKEN` does not
trigger downstream workflows, so without the PAT the release PR gets no CI and
the tag does not start cargo-dist. Adding the secret is a one-time human action
(it cannot be done by an agent). Add it **before merging this PR**, or at least
before merging the first release PR release-plz opens — otherwise the release job
still creates the `v2.1.0` tag but cargo-dist never fires, leaving a tag with no
Release (recoverable by re-pushing the tag or running `dist` manually, but
avoidable by ordering the secret first).

## Consequences

- The bump is derived from the commits codeflow already enforces, and the
  changelog is generated, not hand-maintained. The human still reviews and merges
  the release PR, so the landing discipline is unchanged — release-plz *proposes*,
  a human *disposes*.
- **Not verified locally, by design.** release-plz cannot be installed in this
  environment (no crates.io network access), so the exact lockstep-bump behavior
  for this two-crate workspace is **not** locally verified. Before this workflow
  is relied on, a maintainer MUST validate it once on a checkout and confirm:
    - (a) **version + changelog** — `release-plz update` (writes only local,
      discardable files): the workspace version becomes `2.1.0` (two `feat`
      commits, no breaking, since `v2.0.0`) and the **root** `CHANGELOG.md` is the
      file rewritten, not a per-crate `crates/codeflow-cli/CHANGELOG.md`.
    - (b) **single tag** — `release-plz release-pr --dry-run`: exactly one tag
      `v2.1.0` is planned, not a per-crate `codeflow-cli-v2.1.0`. (`release-plz
      update` does not surface the tag, only the file writes — hence the separate
      dry-run.)
    - (c) **changelog is reconciled and well-formed** — the generated `[2.1.0]`
      section does not duplicate or drop the pre-existing hand-written
      `## [Unreleased]` bullet (the `codeflow update` fix), and the file keeps a
      valid Keep-a-Changelog shape with a `[2.1.0]` reference. Clear or relocate
      any manual `[Unreleased]` bullets before merging the first release PR so
      release-plz owns the section cleanly from then on.
  If any differs, adjust `git_tag_name` / `changelog_path` / add a `version_group`
  before arming. The manual tag path remains the fallback.
- The release PR's commit is `chore(release): ...` — conventional, so it passes
  the commit-standards CI check; the `chore\(release\)` changelog parser skips it
  so it never appears in the notes.
- New files only (`release-plz.toml`, the workflow, this ADR) plus
  `publish = false` on the internal crate. No engine or scaffold behavior
  changes; `architecture_impact: none`.

## Architecture impact

None — this is release tooling and CI configuration, not a change to the four
enforcement planes or any runtime boundary.
