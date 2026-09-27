---
id: ADR-0058
uid: de5163f3-8f5e-4c61-a428-1c329b55b11b
title: replace portal source merging with explicit runtime ownership
date: 2026-09-10
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — portal updates replace unchanged runtime files; explicit whole-runtime transfer freezes provenance without weakening derived-evidence validation
---

# ADR-0058 — replace portal source merging with explicit runtime ownership

## Context

ADR-0048's tracked pristine copies and automatic three-way source merging make
an optional runtime harder to maintain coherently, especially lockfiles and
bundled executable assets. Projects need their edits preserved without an
implicit promise that arbitrary forks can continue receiving safe upstream
merges. SPC-008 settles the ownership and migration contract for EPC-007.

## Decision

This supersedes only ADR-0048's portal reconciliation and pristine-copy
decision. Its explicit offline adoption, one embedded starter, source-in-place
authority, isolated Node runtime, strict v1 evidence, bounded read-only Rust
validator, utility craft and release-size budgets remain in force. The portal
does not become product design authority or share the present Comment runtime.

Managed updates classify a complete transaction before writing. Unchanged
files can be replaced, missing managed files repaired, and unchanged retiring
files removed. Incoming-identical bytes are accepted. Modified managed files,
unknown collisions and edited retirement stop portal writes and state advance.
No runtime file is line-merged and no new conflict sidecar is created. General
scaffold reconciliation remains separate and is not rolled back by a portal
conflict. Project-owned configuration is seeded only at initial adoption;
subsequent absence is preserved and reported.

`codeflow portal transfer --confirm` transfers the whole adopted runtime.
It preserves edits and intentional deletions on either adoption schema v1 or
v2, without installing an incoming release or repairing configuration. V2 state
records managed/transferred ownership and a declared generator identity. After
transfer, starter version and file hashes are frozen transferred-from
provenance, not current-runtime claims. Repeated transfer and future updates
preserve that ownership. A separately reviewed restoration from Git remains
possible with current work preserved; there is no new restore command.

Evidence remains schema v1. Managed identity is pinned to
`@codeflow/docs-portal` and the installed release. A genuine project fork may
rename/version its generator and declaration together. The Node producer and
collector share actual generator identity; Rust independently compares emitted
identity with adoption state and retains every existing evidence check. Neither
ownership nor a matching declaration attests runtime origin, security or
rendered quality. Rust never executes the project generator.

Pending legacy journals recover under the lease before full adoption parsing
and migration. Existing baseline blobs are removed only after bounded regular
bytes authenticate against recorded managed hashes, within the transaction.
Absent baselines are benign; unknown or altered content stops migration and
transfer. Manual recovery preserves/inspects content before reviewed exact
restoration or relocation, never automatic deletion or newline normalization.
New installs and successful migrations retain no pristine runtime copies.

Portal-local transactions hold directory descriptors/handles through lease,
stage, publication, removal and recovery. This prevents symlink/reparse
traversal and ancestor redirection, not leaf inode compare-and-swap or hostile
same-user isolation. Unix moved directories and hard links/mounts remain outside
byte-origin isolation. Windows denies delete sharing while handles are held;
cross-target lint is not native behavior evidence or a directory-fsync claim.

## Consequences

Ordinary consumers maintain one replaceable runtime through supported
configuration seams. Deep customization has an explicit owner and its own
upgrade/security obligations. Drift requires a deliberate choice instead of an
automatic merge, and corrupt legacy baseline content may require manual
preservation/recovery before transfer can proceed. The smaller persistent state
does not remove transaction recovery, strict evidence or browser qualification.

Rejected alternatives are continued source merging, silent transfer on conflict,
per-file runtime ownership and allowing transfer to bypass baseline integrity.
Each would blur coherent ownership or weaken preservation guarantees. A second
JavaScript adoption parser is unnecessary: Rust owns that interpretation, while
the generator reports its actual identity and tests cover their agreement.

## Architecture impact

The portal engine owns replace-only reconciliation, strict adoption v1/v2
parsing and explicit transfer. The Node starter owns generation and its shared
identity. Distributed guidance explains migration and project-owned maintenance;
the existing independent Rust verifier and browser lanes keep their distinct
claims. No package registry, new dependency or visual redesign is introduced.
