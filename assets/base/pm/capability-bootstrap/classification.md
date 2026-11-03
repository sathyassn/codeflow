## Description

Classify the inventory using `.codeflow/rules/capabilities.md`. Use project areas, closed generic facets and the project's vocabulary. Distinguish functional and non-functional capabilities; a non-functional promise names its quality characteristic and measurable stimulus, metric and target. Set maturity from evidence; use unassessed when no stronger basis exists.

## Outcome

After this merges, readers can find capabilities by audience, area, kind and maturity without inferred claims about what ships.

## Deliverables

- Classified capability records and vocabulary: `docs/capabilities/`

## Acceptance Criteria

- AC-1 All inventoried capabilities have valid required facets; non-functional records state a quality and measure; lifecycle and maturity claims have a stated basis or remain unassessed. Evidence: `codeflow validate --docs` and the classified inventory. (serves bootstrap AC-1)
