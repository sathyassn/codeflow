## Summary

Build a comprehensive initial catalog of the project's capabilities. Use `.codeflow/rules/capabilities.md` through inventory, classification, evidence and independent review. Existing catalog entries seed the work and keep their identities.

## Outcome

After this merges, users, contributors and agents can find the project's capabilities, their intended audiences, maturity and evidence gaps.

## Deliverables

- Capability records and facet vocabulary: `docs/capabilities/`
- Inventory coverage and justified exclusions: `docs/capability-inventory.md`
- Evidence and independent review: `docs/capability-review.md`

## Acceptance Criteria

- AC-1 Every product surface, internal and technical surface, and promised functional or non-functional behavior has a capability entry or a justified exclusion in `docs/capability-inventory.md`; records have valid facets and honest evidence or declared gaps; a recorded independent review in `docs/capability-review.md` confirms comprehensive coverage before `docs/capabilities/vocabulary.yaml` sets `inventory: complete`. Evidence: the inventory, catalog validation and review record. (journey)

## Out of scope

This work does not add product features or assume every discovered capability is shipped or stable. Inventory completeness does not mean every evidence gap is closed.
