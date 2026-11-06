## Description

Assess evidence using `.codeflow/rules/capabilities.md`. Link each shipped capability to tests, journeys or verification records through typed verified_by entries. Record a declared gap where evidence is absent. Check what each reference actually proves before assigning maturity; never turn a declaration into a claim that a check ran.

## Outcome

After this merges, readers can distinguish verified behavior from evidence gaps for each shipped capability.

## Deliverables

- Evidence declarations and maturity bases: `docs/capabilities/`
- Check results and remaining gaps: `docs/capability-review.md`

## Acceptance Criteria

- AC-1 Each shipped capability has resolving typed evidence or explicit gaps, with executed checks and unverified boundaries recorded; beta or stable claims have supporting non-gap evidence. Evidence: catalog validation and `docs/capability-review.md`. (serves bootstrap AC-1)
