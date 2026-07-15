# Duo quality contract

This resource is the portable contract shared by Claude Code, Codex, and any
other capable host. Project rules may strengthen it but must not weaken it
silently.

## Versioned plan contract

Each settled plan records:

```text
PLAN_VERSION:
BRIEF_DIGEST:
SCOPE_AND_NON_GOALS:
VERIFIED_CONTEXT:
ASSUMPTIONS_OR_UNRESOLVED:
DESIGN_OPTIONS:
CHOSEN_DESIGN_AND_RATIONALE:
TASKS_AND_OWNERS:
ACCEPTANCE_CRITERIA:
EDGE_AND_ERROR_CASES:
SECURITY_AND_PRIVACY:
TEST_AND_UI_PLAN:
COVERAGE_PLAN:
ROLLBACK_OR_RECOVERY:
CLAUDE_APPROVAL:
CODEX_APPROVAL:
```

Use a content digest or durable link for the immutable brief. Approvals must name
the same plan version. A changed plan invalidates both approvals until each seat
reviews the new version.

## Evidence ledger

For every material claim, record the acceptance criterion or risk it supports,
the command/tool/source used, the observed result, and the responsible seat.
Prefer exact commands with exit codes, file:line references, test summaries,
coverage output, screenshots/snapshots, or links to authoritative documentation.

Model agreement is not evidence. “Tests pass” without the executed command and
result is not evidence. An assumption becomes verified only after a source,
tool, or direct observation supports it. Conflicting evidence remains visible
until resolved.

## Required verification

Apply the checks relevant to the changed surface:

- formatting, lint/static analysis, type checking, and documentation checks;
- focused unit tests for changed logic, including boundaries and failures;
- integration tests across changed interfaces and persistence/network edges;
- end-to-end tests for critical user journeys and irreversible operations;
- dependency and vulnerability scanning plus a source-to-sink security review;
- regression tests for each fixed defect;
- repository-specific validation, packaging, or migration checks.

A skipped category is explicitly `N/A` with the reason and evidence that the
surface is absent. Tool unavailability is a blocker or declared limitation, not
a pass.

## Coverage

Scenario coverage comes first: happy paths, boundaries, malformed input,
timeouts, partial failure, authorization, concurrency/idempotency, recovery,
and regression cases as applicable.

Where the stack supports line coverage, aggregate production-code coverage is a
hard floor of **80%** and the normal target is **90% or higher**. New or changed
critical logic should be covered at 90% or better when measurable. Generated,
vendor, fixture, and test code may be excluded only with a recorded owner,
reason, and removal condition. Meeting a percentage never excuses a missing
risk scenario. A repository may enforce a stronger floor; CodeFlow itself uses
its configured 90% aggregate Rust gate, so 80% is not sufficient for this
repository.

## UI and design verification

For web UI, exercise real rendered behavior with Playwright or an equivalent
browser driver. For native, mobile, or desktop UI, use Computer Use or a
surface-specific automation driver. Check at least:

- the approved design and the primary user journeys;
- loading, empty, error, disabled, and success states;
- responsive/layout behavior at relevant sizes;
- keyboard navigation, focus, labels, contrast, and other applicable
  accessibility requirements;
- validation, destructive-action safeguards, and recovery;
- console/runtime errors and network failures.

Use screenshots or stable snapshots where they materially demonstrate
conformance. A code-only review is not UI verification. If no UI changed,
record `UI: N/A — no user-facing surface changed`.

## Independent review

Codex performs the first verification after implementation. Claude then reviews
the diff independently, reruns the relevant gates, and checks conformance with
the chosen design. The final reviewer must challenge the evidence, not merely
accept the implementer's summary.

Classify findings by severity and support each with a concrete trigger or
reproduction. Security approval requires checking untrusted inputs through
their sinks, authentication/authorization, secrets and privacy, dependency
risk, injection, path/process boundaries, and the agent-facing
prompt/instruction surface where present.

## Completion gate

Completion requires:

- both seats approved the final plan version and task breakdown;
- every acceptance criterion is evidenced;
- required deterministic gates are green;
- coverage meets the applicable floor;
- UI/design evidence is present or explicitly N/A;
- the independent Claude review is approved;
- no unresolved critical/high security issue or material assumption remains.

A failing or missing gate cannot be overridden by model consensus.
