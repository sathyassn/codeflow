# Design exploration Plan v4 review — 2026-08-01

## Scope

Plan v4 adds TSK-012 to close a bounded design-contract gap found after
TSK-005. It changes one graph edge into TSK-010 and allocates SPC-006 plus
ADR-0051. It does not reopen completed design work or alter the implementation
scope of cf-present or cf-docs-portal.

## Settlement

The operator authorized continued EPC-005 work and required the design method
to retain meaningful exploration, product-owned design systems, asset/tool
consideration, and Agent OS applicability without diluting prior intent.

Codex independently inspected the existing `cf-design` skill, its
design-choice audit, ADR-0043, SPC-003, the model-evaluation contracts, and the
EPC-005 graph. It found genuine missing behavior for post-selection variation,
asset sourcing/privacy, and reviewed-version provenance, while design-system,
accessibility, and fidelity behavior already had canonical homes. Codex approved
the delta-only TSK-012 topology and acceptance envelope after the corrections
below.

The directly invoked native Claude judgment primary ran as Fable 5/high in
Claude Code session `7c597939-8f84-4a35-a1fc-64bbe3ee5326` through the CodeFlow
schema-v2 lifecycle. The first review requested changes: allocate a new frozen
spec and append-only ADR, name shared-file ownership and landing order, prevent
doctrine duplication, and resolve Plan v3 provenance. Turn
`plan-v4-correction`, prompt `056e3e3b-57e4-4da9-b3f7-d8acea80e664`, then
independently verified the corrected files, the durable Plan v3 record on
TSK-009 commit `627deeec`, and the work graph. Its final verdict was `approved`.

Three non-blocking craft corrections from that approval were applied before
commit: align the task-graph join, use the canonical “error page” term, and
state TSK-012's responsibility as recording the later TSK-010 utility-asset
verification obligation rather than claiming the future verification itself.
They do not change a node, edge, owner, scope, acceptance behavior, interface,
or safety boundary.

## Exact Plan v4

```text
TSK-005
  ├─> TSK-006 ─> TSK-011 ─> TSK-007 ─┐
  ├─> TSK-008 ─> TSK-009 ──────────┤
  └─> TSK-012 ─────────────────┤
                                        v
                                     TSK-010
```

The integration host owns shared eval JSON, scaffold manifests, managed
baselines, changelog, and lockfiles. Shared-file landings are serialized
TSK-011, TSK-009, TSK-012, TSK-007, then TSK-010. This selected guard reduces
conflicts; it does not add false dependency edges or replace task-level
cross-lineage review.

## Verification

- `codeflow validate --docs`: policy, 29 records, and document graph clean.
- `git diff --check`: clean after the three craft corrections.
- Both primary seats approved Plan v4's topology, ownership, delta-only scope,
  sourcing/privacy boundary, revision provenance, and evaluation envelope.

No implementation, semantic model trial, managed-mirror result, runtime
behavior, or downstream Agent OS behavior was verified by this planning review.
Those claims remain owned by TSK-012, TSK-010, and the downstream repositories.
