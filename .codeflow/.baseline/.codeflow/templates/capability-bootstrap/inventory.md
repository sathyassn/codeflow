## Description

Inventory the project using `.codeflow/rules/capabilities.md`. Start with existing capability records without recreating them. Enumerate product and README promises, commands, routes or public APIs, packages and top-level tests. Sweep internal surfaces including build, release, operations and agent use, then functional and non-functional promises. For each quality characteristic, record a capability or explain why the project makes no promise. Keep inventory incomplete until review.

## Outcome

After this merges, the project has a traceable map from its surfaces and promises to capability entries or justified exclusions.

## Deliverables

- Surface inventory and exclusions: `docs/capability-inventory.md`
- New capability records allocated with `codeflow capability new`: `docs/capabilities/`

## Acceptance Criteria

- AC-1 Each enumerated surface and promise is mapped to an existing or new capability id or a justified exclusion in `docs/capability-inventory.md`, including user-facing, internal and technical behavior; existing ids and content are preserved except for deliberate reviewed corrections. Evidence: the inventory and catalog diff. (serves bootstrap AC-1)
