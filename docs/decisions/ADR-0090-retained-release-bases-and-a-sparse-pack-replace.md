---
id: ADR-0090
uid: f7e927fa-bfdb-42ca-ae24-da0d2d02dc5c
title: "Retained release bases and a sparse pack replace the baseline tree and the manifest digest"
date: 2026-10-30
status: accepted          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: The Ownership classes row names the base source, the binary's retained releases and `.codeflow/bases/`, in place of `.codeflow/.baseline/` and the manifest hash
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0090: Retained release bases and a sparse pack replace the baseline tree and the manifest digest

## Context

`codeflow update` decides what to do with a managed file by comparing it
with the pristine copy the project installed. Today that copy is tracked in
the project twice: as a file under `.codeflow/.baseline/<dest>`
(`crates/codeflow-core/src/scaffold/state.rs:21`, `:565-602`) and as its
digest in `.codeflow/manifest.json` (`InstalledFile.sha256`,
`state.rs:512`). Every release changes both for every changed file, and the
digest is one line per file that every pull request touching that file
rewrites, so two pull requests that change the same managed file in
different places conflict in the manifest even when the file itself merges
(issue 119: six of ten open pull requests on 2026-10-08). This repository
carries 317 such copies (about 7 MB) and relabels every one on each resync.

The design that resolves this was approved by a Codex reviewer on
2026-10-15 after five rounds (`design-baseline-05.md`); it lived only in a
worktree's build output and is lost. The decisions the operator took on it
survive in the workspace `RELEASE-PLAN.md` and are the inputs here:

| Decision | Date | Content |
|---|---|---|
| Option A | 2026-10-15 | An incompatible manifest and a major release, shipped as 4.0.0 |
| Sparse pack | 2026-10-15 | A tracked sparse pack beside the manifest is accepted |
| Retention | 2026-10-17 | The binary keeps the shipped bases of every published release (about 0.6 MB then); an advisory 2 MB budget triggers a review when crossed |
| Version | 2026-10-28 | The current line ships as 4.0.0 and absorbs the replacement, so adopters migrate once |

## Decision

The shipped bases move into the binary. The binary carries the scaffold
assets of every published release (v2.0.0, v2.1.0, v3.0.0, v3.1.0, v3.1.1,
then each release from 4.0.0 on) as one deduplicated, solid zstd stream with
a TOML index under `assets/releases/`, and the release step adds the release
being prepared and refuses one whose bases are missing. `.codeflow/manifest.json`
moves to schema 2: an entry records `src`, `ownership`, `exec` and the
`version` its base is read at, and no digest; the top-level
`scaffold_version` stamp is dropped, since `.codeflow/project.toml` already
carries it. What the binary cannot reproduce lives in a tracked sparse pack,
`.codeflow/bases/<dest>`: the rendered copy of every `template = true`
managed or managed-region entry, and every file installed by a development
build (a binary whose assets are not a retained release's). The base of a
file is looked up in the pack first, then in the retained release the entry
names, then it is absent; every decision that compared a hash with the
record compares bytes with that base. `codeflow update` migrates a 3.x
project in one run: it derives each entry's base from the old baseline copy
and digest, writes the pack entries it needs, writes the schema 2 manifest,
and removes `.codeflow/.baseline/` last, following no link and writing
nothing outside `.codeflow/`. A published 2.x or 3.x binary cannot update
the result: at every tag from v2.0.0 to v3.1.1 its parser requires the
`sha256` field (at v3.1.1, `state.rs:498`, read at `:534`), so `update`
fails on the missing field before it writes a project file and without
naming the path, and a repeated `init` lowers the pin in `project.toml`
before it fails the same way. `doctor` differs by tag: at v3.1.0 and v3.1.1
it fails naming the manifest; at v3.0.0 it passes, skipping the drift check
on a manifest it cannot read (v3.0.0 `doctor/mod.rs:2383-2384`); at v2.0.0
and v2.1.0 it passes, since it only parses `.codeflow/` files as JSON
(`SPC-015` R-4). No published binary has a pin check; this line's
`check_write_version` first ships in 4.0.0.
The change ships as 4.0.0 with one migration step. `SPC-015` pins the
formats, the outcome of every decision arm and the edge cases; TSK-1086
builds the retained bases, TSK-1087 the switch and the migration, TSK-247
the read-only preview.

Displaced alternatives:

- Keeping `.codeflow/.baseline/` and the per-file digest (the status quo):
  every release relabels every changed file twice in the project, and the
  digest line is the conflict issue 119 reports.
- TSK-258's schema 2 (draft PR 125): dropped the digest and kept the baseline
  directory, so the tree still changed on every release and this repository
  kept its 317 copies; the 2026-10-18 release order ruled it out as an
  intervening format, and TSK-258 is cancelled as superseded.
- A content-addressed pack with one compressed blob per file: write-once and
  simple to extend, but 1.35 MB today against the 2 MB budget (measured in
  `SPC-015`), so the budget would be crossed within a release or two; the
  solid stream is 0.59 MB.
- Keeping the bases of the current release only: a project that skipped a
  release, or that kept a file at an older base after a conflict, would have
  no base to merge from; the operator chose every published release.

## Consequences

- Two pull requests that change different managed files no longer meet in
  the manifest or a baseline tree, and a resync changes only the entries of
  the files it changed; a shared stamp no longer moves on a resync. The
  remaining shared stamps are the live `scaffold=` markers in `AGENTS.md`
  and `CLAUDE.md` and the version fields of `Cargo.toml` and
  `.codeflow/project.toml`, which this decision does not touch.
- The project tracks about 3 files in the pack instead of 317 (the rendered
  `AGENTS.md`, `CLAUDE.md` and `.codeflow/rules/worktrees.md` bases at a
  tier), and nothing under `.codeflow/.baseline/`. This repository tracks
  more while a release is in development: a development build writes the
  base of every file it installs to the pack, and the first update by the
  published binary removes each entry it can reproduce.
- The trust surface shrinks: a base read from the binary cannot be edited by
  an agent session, where a committed baseline copy could be. The pack and
  the pending resolution records become protected paths when TSK-1087 adds
  them (`SPC-015` R-29); the current default protects installed bases under
  `.codeflow/.baseline/**`
  (`crates/codeflow-core/src/security/policy.rs:48`).
- The binary grows by the compressed pack, 0.59 MB today, and by at most the
  changed assets of each future release. The advisory budget is 2 MB; the
  build or release check warns, never fails, when it is passed, and the
  crossing is a review, not a refusal.
- `zstd` on its 0.14 line, the one the lockfile already resolves through
  rust-embed's `compression` feature (`Cargo.toml:38`; `zstd` 0.14.3 at
  `Cargo.lock:2183-2202`), becomes a direct dependency of the engine for
  decoding the stream, and `cargo test -p codeflow-core` then builds
  `zstd-sys`. Never 0.13: `Cargo.toml:53-62` patches that line out of the
  tree for its RUSTSEC advisories. The reader caps what it decodes before
  it allocates (`SPC-015` R-26). No other dependency is added.
- A published 2.x or 3.x `update` or `init` cannot read a 4.0.0 project,
  and only the v3.1.0 and v3.1.1 `doctor` fails on one; the older doctors
  pass it (`SPC-015` R-4). Adopters upgrade every checkout
  and CI pin to 4.0.0 together, run `codeflow update` once and commit the
  manifest and the baseline removal (TSK-1087's migration text). A project
  installed by a development build keeps its bases in the pack until the
  published binary updates it.
- The release step gains a duty: a release cannot be prepared without its
  bases in the index, and `check-state` refuses a version whose retained
  files differ from `assets/base` at the release commit. The typed repair
  rules lose their baseline rows (`docs/releasing.md` "Typed repair rule",
  `scripts/release.py:1491-1572`): a repair no longer touches the manifest
  or a baseline.
- Every reader of `.codeflow/.baseline/` changes in TSK-1087: the policy
  default, the model catalog scan, the eval kit scrub, the release scripts,
  the tests and the docs that name it; `SPC-015` lists them.
- The 2.1.0 stand-in copies for a missing baseline
  (`crates/codeflow-core/src/scaffold/prior_release.rs`, TSK-171) are kept
  as they are; the retained 2.1.0 release could replace them later, as a
  separate change.

## Architecture impact

The Ownership classes row of `docs/architecture.md` names where the merge
base comes from. It read "refresh by hash and 3-way merge from
`.codeflow/.baseline/`"; it now says the comparison is by bytes against the
shipped base, which is the committed copy and digest until TSK-1087 lands
and the binary's retained releases with the sparse pack `.codeflow/bases/`
from then on, and names this record. TSK-1086 and TSK-1087 update the row
again when they land.


## Design amendment, 2026-11-03

This amendment supersedes the decision's implicit pack/release fallback
and version-only base identity. Schema 2 requires `base_source` with one
of three values: `release` reads only the retained bytes for the recorded
source and version; `pack` requires its pack without a release fallback;
`unknown` authorizes no base lookup even when that version is retained.
A missing template pack never substitutes raw template bytes. An untrusted
legacy base is recorded as `unknown`; restart cannot turn it into deletion
authority. A present orphan with no base is kept and unmanaged
(SPC-015 R-1/R-6/R-14/R-19).

Only a non-template file whose base bytes (the file, or for a region the
block the binary extracts from it without project inputs) the recorded
release's `src` reproduces exactly is eligible for `release`. R-5/R-8 allow a proven source-only
conversion on publication without a shared stamp: store the release source
before removing the redundant pack. Unrelated resyncs keep other entries
unchanged.

Pending resolutions in R-17 retain actual old and proposed content hashes,
complete entry snapshots and proposed shipped bytes. A development proposal
uses `pack` even when the release index contains the same version with
other bytes. Acceptance stores the proposed pack before its entry and
removes the pending record last. Retry validates the reachable states:
before the entry store, the old entry with its old base or the already
written proposed pack; after the store, the proposed entry with the new
base. A person's resolution never becomes the base. These temporary
hashes do not reintroduce a manifest digest.

The protection plan extends both `SecurityPolicy::defaults` and
`security/actions.json`'s `claude_edit_denies` to the manifest, pack and
pending records. The edit guard derives direct and linked-worktree
protection from the action table, not from policy defaults. TSK-1087
regenerates permission presets and tests denial of edits to all three
inputs, including absent paths, while preserving normal engine writes.
Baseline protection stays while schema 1 migration still reads it
(R-29/R-30).

TSK-1087 owns these changes and tests for unknown-base orphan preservation,
missing template packs, same-version development proposals and interrupted
acceptance. TSK-247 consumes the same source-aware derivation without
writes. The retained-release reader in TSK-1086 is unchanged.
