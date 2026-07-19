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
COMPLEXITY_JUSTIFICATION:
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

For research/analysis-only work, `TASKS_AND_OWNERS`, `TEST_AND_UI_PLAN`,
`COVERAGE_PLAN`, and `ROLLBACK_OR_RECOVERY` may be `N/A` only with a concrete
reason. For planning-only work they describe the future implementation rather
than work performed in the current run. Never imply that proposed evidence was
executed evidence.

## Design and implementation quality

Approve the smallest coherent solution that fully satisfies the accepted
behavior, not the fewest lines. Every material abstraction, public interface,
configuration surface, dependency, compatibility path, and operational concept
must map to a current requirement, observed constraint, or evidenced risk; if it
does not, remove or simplify it. Reject speculative generality, duplicate or
dead paths, cleverness that obscures control flow, and architecture that fights
the repository's established patterns.

Coherence includes justified structure, not merely less structure. Follow the
language, framework, and repository idioms; keep business rules single-sourced;
use focused composable units, clear interfaces, and explicit state and side
effects. Prefer declarative or reactive composition when it is native to the
stack, not as a universal mandate. Do not hard-code supported variability,
secrets, or duplicated domain decisions; named stable invariants need not become
configuration. Current variants, repeated behavior, observed constraints, and
evidenced edge or failure cases may require abstraction, reuse, configuration,
or defensive code. Unexplained hard-coding, duplicated business knowledge,
swallowed errors, or missing accepted edge/error handling is brittle
under-design and is `changes_requested`, even when the smaller diff passes.

Calibrate structure to the accepted operating context: expected lifetime,
scale, rate and shape of change, contributor and integration breadth,
operational or security risk, and cost of reversal. No factor—especially size
alone—proves an abstraction. If missing context would materially change the
settled design, clarify it before approval; if clarification is unavailable,
state the assumption and prefer established safe practices with reversible
boundaries, without speculative generality.

Both seats grade design proportionality before approval. Codex first-verifies
the implementation for necessity, clarity, idiomatic structure, maintainability,
failure behavior, and security. The directly invoked primary Fable seat reviews
the settled design and actual integrated diff and owns the final quality verdict;
helpers may collect evidence but cannot replace that judgment.
Material avoidable complexity is `changes_requested`, even when tests pass.
A non-Fable fallback records reduced assurance and never claims that Fable
reviewed the work.

## Parallel execution contract

Parallelize only workstreams whose inputs and outputs can be isolated. Record:

```text
DEPENDENCY_GRAPH:
PARALLEL_TASKS:
TASK_BRANCH_WORKTREE_OWNER:
SHARED_FILE_OWNER:
INTEGRATION_BRANCH_AND_ORDER:
HOST_RESOURCE_BUDGET:
PER_TASK_GATES:
POST_MERGE_GATES:
```

Each implementation task has one writer, branch, and worktree. Shared schemas,
migrations, lockfiles, generated registries, and other conflict hotspots have a
single integration owner or run sequentially. The coordinator caps concurrent
heavy builds, browsers, and model sessions from observed CPU, memory, disk, and
tool limits and preserves headroom; reduce fan-out before swap pressure,
duplicate caches/builds, or context dilution affects evidence quality. Never
use concurrent writers in one worktree, and never rebase a shared integration
branch.

Merge task branches in the recorded order through serialized integration,
running the affected gates after each merge and the aggregate gates on the final
combined diff. Passing task-local checks does not prove the integration.

## Evidence ledger

For every material claim, record the acceptance criterion or risk it supports,
the command/tool/source used, the observed result, and the responsible seat.
Prefer exact commands with exit codes, file:line references, test summaries,
coverage output, screenshots/snapshots, or links to authoritative documentation.

Model agreement is not evidence. “Tests pass” without the executed command and
result is not evidence. An assumption becomes verified only after a source,
tool, or direct observation supports it. Conflicting evidence remains visible
until resolved.

For a catastrophic or irreversible action, the ledger also records independent
Claude and Codex risk assessments, the authenticated human approval, exact
scope and command/tool input, preview or dry-run evidence when supported, the
current checkpoint/backup and tested restore path, execution result, and
postcondition verification. Model consensus and automatic safety review never
stand in for the human approval. Ordinary recoverable worktree edits and
deletions do not require this ceremony. An operation in CodeFlow's
non-relaxable deterministic class is performed by the human operator through a
separate controlled channel; the models record the operator's result and verify
the postcondition without weakening the guard.

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

Research, analysis, and planning runs verify source authority, freshness,
independence, contradiction handling, and traceability from each material claim
to the evidence actually read. They do not inherit code-test requirements for a
surface they did not change, but they still need independent Claude and Codex
work plus a settled, evidenced result.

## Editorial quality

Apply `cf-editorial-review` to substantial documentation, ADRs, proposals,
release notes, PR narratives, operator communications, and user-facing copy.
The primary Fable seat owns the final contextual editorial verdict; both seats
still verify technical meaning and evidence. Do not invoke the skill for every
short response, and do not let tone override truth, policy, or precision.

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

For web UI, exercise real rendered behavior through the active native harness
with repository tests plus a supported Playwright MCP or official CLI/skill.
Browser headless mode is valid for routine deterministic E2E and CI: it still
provides DOM/accessibility state, assertions, screenshots, traces, console
output, and network evidence, and is unrelated to the prohibited headless
peer-model transport. Use headed/UI mode when live observation, browser chrome,
interaction debugging, or environment-specific rendering is material. For
native, mobile, desktop, browser-chrome, or other surfaces outside Playwright's
controlled page/context, prefer a surface-specific driver and use Computer Use
only when no narrower driver reaches the surface. Check at least:

- the project's existing design system and component library before adding a
  new pattern;
- recurring foundations or tokens, accessible primitives, reusable
  application-specific components, and their composition into views or pages;
- minimal explicit interaction state with one clear owner and framework-native
  data flow;
- no repeated one-off styling, state logic, or components when current reuse is
  evidenced; no new design system or higher-order abstraction for a one-off
  surface without such evidence;

- the approved design and the primary user journeys;
- loading, empty, error, disabled, and success states;
- responsive/layout behavior at relevant sizes;
- keyboard navigation, focus, labels, contrast, and other applicable
  accessibility requirements;
- validation, destructive-action safeguards, and recovery;
- console/runtime errors and network failures.

Match evidence to the claim: locators, accessibility snapshots, and web-first
assertions for structure and behavior; screenshots or same-environment visual
comparisons for appearance; console/network evidence for runtime behavior; and
traces on failure or first retry, or a bounded trace for an ambiguous
exploratory flow. A screenshot alone does not prove interaction or
accessibility, automated accessibility evidence is partial, and tracing every
green run wastes resources. A code-only review is not UI verification. If no UI
changed, record `UI: N/A — no user-facing surface changed`.

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
- design and implementation proportionality are approved;
- substantial changed prose has its contextual editorial approval;
- no unresolved critical/high security issue or material assumption remains.
- every catastrophic action, if any, has the human authorization and recovery
  evidence required above; without it, the action was not executed.

For a mode without implementation, read “task breakdown” as the final research,
analysis, plan, or review artifact and apply only the relevant gates above. For
parallel implementation, completion additionally requires a green integrated
worktree and review of the combined diff—not a collection of green task
branches.

A failing or missing gate cannot be overridden by model consensus.
