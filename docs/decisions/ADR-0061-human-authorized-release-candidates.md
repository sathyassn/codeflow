---
id: ADR-0061
uid: 5f3c1939-616a-47ba-b541-03223e97343d
title: automate release candidates while keeping publication human-authorized
date: 2026-09-13
status: accepted
superseded_by: ADR-0062
architecture_impact: docs/releasing.md — one maintained candidate PR and exact-source publication guards replace the manual release branch and tag push
---

# ADR-0061: automate candidates; authorize exact-source publication

## Context

ADR-0012 correctly selected git-cliff for CodeFlow's version calculation and
cargo-dist for artifacts, but left the bookkeeping and tag push manual. The
workspace already identifies itself as 3.0.0 while v2.1.0 is the latest public
release, so blindly rerunning a bump can incorrectly suggest 4.0.0. The live
v2.1.0 lightweight tag resolves to
`d70c6f17d4bf199a545c843856b3ded4681aff20`, while the published source archive
proves source commit `3c3efdb91009361e18b0fabad699b5e875d4e4dd`; its SHA-256 is
`1501e0d81716dadd3aa4dc1c56348dd7321abd9cdca90b8f5deb89ea20d54beb`.
Those are distinct facts and do not authorize moving the tag.

## Decision

Every PR declares CodeFlow release impact and evidence. CI compares that
declaration with the actual base-relative paths and conventional commit markers;
watched paths require a compatibility assessment, but do not mechanically prove
a break. git-cliff 2.13.1 remains the sole bump calculator. For the one-time v3
bridge it compares from the reachable live tag as-is, records the independently
verified published-source provenance, and accepts the already-staged 3.0.0 as
the first candidate rather than incrementing it again.

After relevant changes land on `main`, a serialized workflow checks that an
existing generated candidate was not edited, promotes curated Unreleased prose,
uses the newly built CLI's supported update path for coupled scaffold stamps,
and maintains one draft `chore/release-codeflow` PR. Human edits belong in the
canonical Unreleased notes on `main`; refresh stops on unrecorded candidate
changes instead of overwriting reviewed prose.

A human merge of that exact candidate is the publication authorization. The
current repository permits merge commits only; the publication guard requires
the candidate head to be the merge's second parent, the recorded source to be
its first parent, and the merge and candidate trees to match. If repository
merge policy changes, the guard fails closed until a reviewed strategy replaces
this proof. cargo-dist 0.32.0 is configured for explicit dispatch and remains
the sole tag, release, installer, and artifact publisher. Its generated
workflow makes global packaging and hosting depend on a supported local-artifact
authority; platform compilation may overlap but cannot publish. That authority creates
or reuses only an exact candidate-bound empty draft with the reviewed curated
notes; wrong tags, public releases, foreign drafts, and partial assets block.
cargo-dist uploads without clobbering and its supported announce phase makes the
tag and completed release public only after all artifacts are available. A
supported post-announce job then compares the public source and asset digests
with the same-run staged files. Dry-run may build but cannot host. The exact
candidate ref is restored after repository auto-deletion
only after the merge, tree, reviewer and permission checks succeed.

## Consequences

Release bookkeeping becomes continuous without giving a bot publication
authority. A changing candidate invalidates review naturally through a new
head. A merged-but-unpublished candidate holds later preparation instead of
silently changing its version or source. Hosting retries are deliberately
conservative: an exact unpublished tag or the exact owned empty draft can resume,
but a conflicting draft or partial assets require explicit investigation. A
stale merged candidate is recovered only by a reviewed restoration of its
recorded source bytes and curated notes; deleting its record alone is invalid.

The generated cargo-dist 0.32 build jobs retain the workflow's write-scoped
token environment, although checkout credentials are not persisted. That
upstream permission breadth and unexecuted hosted publication remain explicit
limitations; they are not described as least-privilege or release evidence.

Syntactic checks cannot prove semantic compatibility, platform qualification,
or release readiness. Independent review, the repository gates, native
installer evidence, and the release checklist remain required. Consumers keep
their own release authorities; this automation applies only to CodeFlow.

## Architecture impact

`docs/releasing.md`, the repository PR contract, and generated cargo-dist
workflow now form one release path: reviewed change intent → maintained
candidate → human merge → exact-source guarded cargo-dist dispatch.
