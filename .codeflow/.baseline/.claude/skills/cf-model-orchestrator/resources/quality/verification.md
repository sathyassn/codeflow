## Required verification

Apply the checks relevant to the changed surface: formatting, lint/static
analysis, type checking and documentation checks; focused unit tests for
changed logic, including boundaries and failures; integration tests across
changed interfaces and persistence/network edges; end-to-end tests for
critical changed journeys and irreversible operations; dependency and
vulnerability scanning plus a source-to-sink security review; regression
tests for each fixed defect; and repository-specific validation, packaging or
migration checks.

For each material changed journey, map the exercised path before selecting the
end-to-end evidence: user or system entry point; every affected in-project
frontend, service, job, queue, persistence, and authorization boundary; relevant
deployment/runtime configuration; and the observable outcome and recovery path.
At least one test at the highest faithful surface drives the real changed path
through every applicable affected boundary. A UI test backed by a mocked changed
service, or a service test that bypasses changed persistence or runtime wiring,
does not prove that whole journey; unit and integration tests are faster
diagnostics that complement this vertical proof.

Use a controlled double only beyond the system's ownership boundary when the
real dependency is unsafe, unavailable, non-deterministic, or prohibitively
costly. Pin that seam with a contract/integration check where feasible and
record it as controlled and unexercised; never claim that dependency was live
or exercised. Infrastructure, packaging, installer, migration, and
configuration changes require an ephemeral/deployed-runtime canary or
equivalent native evidence for the affected path. A category may still be
`N/A` for a change with no material journey, but the plan names the topology
evidence that makes it so.

Record deterministic and contextual evidence as complementary layers: the
deterministic layer selects applicable syntax/style, dependency/SCA,
data/control-flow, taint, secret, and project-owned architecture checks; the
contextual layer independently verifies intent, business logic, deep
semantics, material performance behavior, state/environment interactions, and
emergent anomalies. A deterministic red result cannot be overridden by model
agreement; an agentic verdict cannot claim an analyzer ran when it did not. If
a relevant SAST/taint lane is unavailable, record the residual risk and
disposition rather than turning absence into a pass.

Quality review is also longitudinal when the repository has relevant history:
inspect the changed surface, nearby patterns, and the smallest useful history
slice for repeated exceptions, dependency-direction erosion, growing
duplication, unstable abstractions, or complexity no single diff exposes. Do
not infer a trend from one point or run an unbounded archaeology exercise:
cite the concrete sequence, its material consequence, and the corrective or
tracking decision.

A property or generative test, a targeted mutation run or an architecture
fitness check is earned only on evidence. The signals: a parser, serializer,
normalizer or state machine with a stateable property or independent oracle;
changed guard, validator, policy, authorization, recovery or other decision
logic that needs proof its assertions detect a wrong decision; a defect that
escaped a green suite; an invariant a current decision record states. When a
changed path carries one of these signals, read
[the verification-selection contract](../verification-selection.md); name a
selected technique with its trigger, and write nothing when none is
selected. These techniques strengthen the normal checks and never replace them.

A skipped category is explicitly `N/A` with the reason and evidence that the
surface is absent. Tool unavailability is a blocker or declared limitation, not
a pass. For a performance-, scale-, or concurrency-sensitive path, also apply
[performance](performance.md). If no user-facing surface changed, record
`UI: N/A` with that reason; otherwise [UI and design](ui-design.md) applies.
