---
id: ADR-0092
uid: c96c6d5c-4dbb-4d3a-a00c-90c2754e835d
title: "The ignored-file rule matches with the ignore crate"
date: 2026-10-31
status: proposed          # proposed | accepted | superseded
superseded_by: null       # ADR id, set on supersession; a dated Note may also be appended
architecture_impact: none # none | one line naming what in architecture.md changes
---

<!-- ADRs are append-only: written at the moment of decision. Never rewrite
     existing content; you may set superseded_by, or append a clearly-dated
     Note. Warranted at Tier-3 decision
     points only (new dependency, schema change, boundary change), not one
     per task. If architecture_impact is not none, update docs/architecture.md
     in the same PR. -->

# ADR-0092: The ignored-file rule matches with the ignore crate

## Context

TSK-323 adds `git.ignored_files`, which judges each new or changed file
against the `.gitignore` files of the policy authority's tree. Gitignore
syntax has rules a plain glob does not: anchoring by a slash, a trailing
slash for directories, `**`, negation with `!`, escapes, and a directory
whose exclusion stops any re-inclusion beneath it. A home-grown translation
onto `globset` would reimplement those rules inside a security gate, where
a mismatch silently lets an ignored file through. The record names the
`ignore` crate or an equivalent and asks for this decision before any code.

## Decision

CodeFlow uses the `ignore` crate, pinned to `=0.4.33` (published
2026-08-04; 0.4.34, four days old at this decision, is left for a later
bump), with default features off, and only its `gitignore` module: one
`GitignoreBuilder` per `.gitignore` blob read from the authority's tree,
never from the working tree, built case-insensitively so a case variant
cannot slip past a rule. CodeFlow walks the directory chain itself so that
an excluded parent directory excludes everything beneath it, as git does.
Audit and licence check at this decision, on the lockfile with the crate
added:

- New packages: `ignore` 0.4.33 (Unlicense OR MIT, from the ripgrep
  repository), `crossbeam-deque` 0.8.8, `crossbeam-epoch` 0.9.21 and
  `crossbeam-utils` 0.8.23 (MIT OR Apache-2.0). `regex-automata` moves
  from 0.4.16 to 0.4.18 (MIT OR Apache-2.0), the floor `ignore` needs.
  `globset`, `walkdir`, `same-file`, `memchr`, `log` and `winapi-util`
  were already in the lockfile. Every licence is compatible with the
  workspace's MIT OR Apache-2.0.
- `cargo audit` (advisory database fetched 2026-10-31, 1,395 advisories,
  238 packages) reports no vulnerability; its one warning is the existing
  accepted RUSTSEC-2024-0449 for `unsafe-libyaml`.
- `osv-scanner scan source --lockfile Cargo.lock --config
  osv-scanner.toml` reports no issues after the same accepted entry.

## Consequences

- The matcher follows the implementation ripgrep ships, so its gitignore
  semantics match what developers already rely on.
- The binary gains four small crates, three of them the parallel walker's
  crossbeam support, which CodeFlow does not call. That is the cost of not
  vendoring the module.
- A future bump of `ignore` is a reviewed dependency change like any
  other; the pin keeps it explicit.
- Ruled out: a hand-written gitignore matcher on `globset`, and reading
  ignore rules from the working tree or the range being judged.

## Architecture impact

None. The rule runs inside the existing path gate and reads the existing
policy authority.
