---
id: ADR-0012
title: version + changelog via git-cliff (replacing release-plz); cargo-dist releases
date: 2026-07-05
status: accepted
superseded_by: null
architecture_impact: none
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0012 — git-cliff for version + changelog; release-plz removed

## Context

ADR-0010 adopted release-plz to derive the SemVer bump + changelog from
conventional commits. It does not work for this project. release-plz is
publish-centric — it "compares local packages with the ones published in the
cargo registry" and runs `cargo package` to determine versions — and
`cargo package` **cannot succeed** on this workspace: `codeflow-cli` depends on
the never-published `codeflow-core`, so packaging first errors on a missing
version requirement and, once a nominal version is added, errors again trying to
resolve `codeflow-core` from the crates.io index. codeflow ships as cargo-dist
binaries and publishes nothing, so a publish-centric tool is a structural
mismatch. Confirmed live: every unarmed release-plz run on `main` failed on
`cargo package`.

The ecosystem was surveyed against our constraints — no registry publish,
conventional commits already enforced, human-merged PR discipline, cargo-dist for
artifacts, and a strong preference (after release-plz) for a tool we can verify
before trusting:

- **release-please** — a genuine no-publish PR-bot (the closest model fit;
  release-plz was itself "inspired by release-please"). But it is a server-side
  Node action needing a PAT for its tag to trigger cargo-dist and a
  `cargo-workspace` plugin for lockstep — not locally verifiable, which
  reintroduces the exact unverifiable-config risk that just failed.
- **cocogitto** — Rust-native, but errors on this repo's PR merge commits
  ("missing commit type separator") and miscomputes the version without config.
- **cargo-release / convco** — publish-oriented, or version-calculation only.
- **git-cliff** — reads git history only (no `cargo package`, no registry),
  computes BOTH the next SemVer (`git cliff --bumped-version`) and the changelog,
  and skips merge commits gracefully. **Verified live on this repo:**
  `--bumped-version` returned `v2.1.0` (2 `feat`, no breaking, since `v2.0.0`),
  and a tuned `cliff.toml` produced a clean, grouped Keep-a-Changelog.

## Decision

Use **git-cliff** for the version bump + changelog, keep **cargo-dist** for
artifacts, and drive releases from a maintainer-run runbook so a human stays in
the loop:

- `cliff.toml` configures grouped Keep-a-Changelog output; `git cliff
  --bumped-version` computes the next version from the enforced conventional
  commits. Neither touches a registry.
- `docs/releasing.md` is the runbook: bump `Cargo.toml`, regenerate the
  changelog, open a release PR; a human reviews and merges it (ADR-0006/0007);
  pushing the `vX.Y.Z` tag drives cargo-dist `release.yml`.
- A **real** tag push (a human, or a checkout with normal credentials) triggers
  cargo-dist directly — **no PAT and no bot required**, avoiding the
  `GITHUB_TOKEN`-does-not-trigger-workflows trap that also constrained release-plz.
- release-plz (`release-plz.toml` and its workflow) is **removed**. This ADR
  supersedes ADR-0010.

## Consequences

- Releases are automatable AND verifiable, with no publish coupling: the bump is
  computed from commits, the changelog is a git-cliff draft the maintainer refines
  before merging the release PR.
- Slightly less "zero-touch" than a PR-bot — a maintainer runs one runbook — which
  is the deliberate trade for reliability and local verifiability after the
  release-plz failure. **release-please** remains the documented upgrade path if
  full PR-bot automation is later wanted (it needs a PAT but, unlike release-plz,
  has no publish/`cargo package` coupling).
- `codeflow-core` keeps `publish = false` (correct hygiene for an internal,
  never-published crate — independent of the tool choice).
- Consumers are unaffected: codeflow scaffolds commit discipline + gates, not a
  release pipeline; their release/version tooling stays their choice, now
  documented in `docs/releasing.md`.

## Architecture impact

None — release tooling and documentation; no plane, boundary, or runtime change.
