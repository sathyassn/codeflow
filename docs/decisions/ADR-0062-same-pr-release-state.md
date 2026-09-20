---
id: ADR-0062
title: keep release state in the normal work PR
date: 2026-09-13
status: accepted
supersedes: ADR-0061
architecture_impact: docs/releasing.md — cumulative pending state replaces candidate branches and release-only PRs while cargo-dist remains the sole publisher
---

# ADR-0062: keep release state in the normal work PR

## Context

ADR-0061 improved publication checks but introduced a maintained candidate
branch, a second PR, and git-cliff-derived bookkeeping after the work had
already been reviewed. That topology could become stale independently of the
change it described. It also made the already-staged v3 transition a special
operational path instead of a bounded historical bridge.

The v2.1 public source and live tag are known to differ. That mismatch is
historical evidence and must be preserved, not repaired by moving the tag.

## Decision

Every normal work PR owns its release state. Each new pending CHANGELOG entry
has one adjacent `patch`, `minor`, or `major` annotation. The undated pending
heading is the latest verified stable public version bumped once by the highest
remaining annotation. `scripts/release.py sync` discovers that public baseline
read-only, checks the bounded v2.1 bootstrap, and updates the heading and coupled
workspace/scaffold stamps. Conventional commit markers only reject obvious
understatement; they do not calculate a second version.

PR CI checks the current target and the actual proposed merge tree. Main CI
checks the merged state without writing. Published sections are frozen, a
withdrawal cannot reuse a public version, and an unresolved tag, draft, or
partial upload blocks later state until an owner resolves it explicitly.
Read-scoped PR/main jobs verify only public inventory and never claim they can
see drafts; publisher-owned guards receive write scope so their API reads can
include draft releases without granting mutation authority to PR code.

Publication remains a deliberate human dispatch of cargo-dist's generated
tag-only workflow on `main`. Both the actor and rerunning actor must be GitHub
Users with current write, maintain, or admin permission. The selected
`GITHUB_SHA` must still be current main, come from an ordinary PR human-merged
into this repository's main (including a contributor fork), pass the
latest exact-source GitHub Actions main-push release-state, aggregate, Rust, Windows,
secret-scan, and security-review checks, match the pending
version and notes,
and have no conflicting host state. The supported cargo-dist local and global
custom jobs enforce those checks; failed or cancelled guards block hosting.
cargo-dist alone uploads without clobbering and announces last. A final verifier
compares the public tag and every asset digest with the same-run files.

## Consequences

There is no candidate branch, release-only PR, post-publication cut commit, or
git-cliff version authority. A stale but clean PR is rejected until the same
work PR recomputes against current main. After publication, later work
automatically starts a new pending section from the newly verified public
release; the v2.1 bootstrap is not a manually maintained release ledger.

The global recheck narrows the interval in which main can move between build
and host, but GitHub scheduling and API reads are not an atomic release queue.
A detected move requires deliberate redispatch. Semantic compatibility,
platform qualification, and the human publication decision remain outside
what the mechanical checks can prove.
