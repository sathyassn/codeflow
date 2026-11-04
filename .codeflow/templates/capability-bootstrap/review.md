## Description

Review the catalog independently using `.codeflow/rules/capabilities.md`. Compare the inventory to the actual project, including user-facing, internal, technical, functional and non-functional behavior. Resolve material findings in the catalog and record the reviewed revision, reviewer, evidence and remaining gaps. Set inventory complete only after comprehensive coverage is confirmed.

## Outcome

After this merges, the project has a comprehensive reviewed initial catalog with explicit evidence gaps.

## Deliverables

- Independent review and findings disposition: `docs/capability-review.md`
- Reviewed catalog with inventory state: `docs/capabilities/`

## Acceptance Criteria

- AC-1 An independent review records its reviewer, reviewed revision, coverage findings and dispositions in `docs/capability-review.md`; validation is green and `docs/capabilities/vocabulary.yaml` states `inventory: complete` after all enumerated surfaces and promises have entries or justified exclusions. Evidence: the review, inventory and catalog validation output. (serves bootstrap AC-1)
