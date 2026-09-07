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
DESIGN_INTENT: <N/A with reason | conform to named system | settled cf-design record>
COMPLEXITY_JUSTIFICATION:
TASK_ASSIGNMENTS:
  TASK_ID | PRODUCER seat@effort | CROSS_LINEAGE_REVIEWER seat@effort | ROUTING_EVIDENCE | DEPENDENCIES
TASK_GRAPH:
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

For multi-task work, `TASK_GRAPH` follows
[the settled task graph contract](task-graph.md), covers exactly the assigned
tasks, and is part of what both seats approve. A material node, edge, decision
guard, ownership, acceptance/interface, or safety-boundary change creates Plan
vN+1 before dependent work continues. In-node execution detail remains ledger
evidence under the resource's explicit non-mutation rules. A single obvious
task records `TASK_GRAPH: N/A (single task)`.

For research/analysis-only work, `TASK_ASSIGNMENTS`, `TEST_AND_UI_PLAN`,
`COVERAGE_PLAN`, and `ROLLBACK_OR_RECOVERY` may be `N/A` only with a concrete
reason. For planning-only work they describe the future implementation rather
than work performed in the current run. Never imply that proposed evidence was
executed evidence.

`DESIGN_INTENT` is proportional. A cosmetic correction records `N/A` with the
unchanged accepted direction; a bounded change inside an established design
system records `conform` and names that authority. A new or materially reshaped
user-facing surface applies `cf-design` and records the creator intent,
audience/job/context evidence, experience target, language and voice,
applicable appearance modes, systems and operator direction, proportionate
research/options, subject and governing idea, user action and primary
composition, settled direction, earned system scope, accessibility target,
pre-registered review rubric, and fidelity plan. Collapse irrelevant dimensions
instead of filling a template. While the operator owns an open material
direction or composition, an operator-visible rendered board settles it before
implementation; agreement between model seats is not that decision. Do not
create a separate design document unless the project needs a durable product or
design-system decision at its normal spec/ADR altitude.

Design exploration remains bounded after direction selection: vary only a
named unresolved choice that can materially change the outcome, retain
proportionate source/rights/consent/transformation and product-use evidence for
material references or assets, never send private material to an external
service without explicit authority, and bind material feedback to the exact
reviewed version plus accepted/rejected rationale in Plan vN+1.

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

Both seats grade design proportionality before approval. Each producer
first-verifies its implementation for necessity, clarity, idiomatic structure,
maintainability, failure behavior, and security; the named other-lineage seat
reviews that unit independently. The directly invoked model qualified for the
`claude-judgment-primary` role reviews the settled design and actual integrated
diff and owns the final quality verdict; helpers may collect evidence but cannot
replace that judgment. Its integrated judgment is not independent review of a
unit it authored.
Material avoidable complexity is `changes_requested`, even when tests pass.
A fallback records reduced assurance and never claims that the selected Claude
judgment primary reviewed the work.

## Materiality and prioritization

Discovery is broad; action is selective. Use this sequence for code, design,
documentation, research, operations, and proactive observations:

1. **Find** candidate problems, risks, and opportunities without filtering for
   what is easiest to fix.
2. **Substantiate** each candidate with a concrete trigger, source, reproduction,
   or observable consequence. A hunch may guide investigation but is not yet a
   finding.
3. **Classify severity** by the consequence if the issue remains unresolved.
   Keep evidence confidence separate from consequence.
4. **Prioritize action** from severity plus confidence, likelihood or
   reachability, blast radius, urgency or cost of delay, recurrence or systemic
   leverage, and dependencies. State that rationale with the finding.
5. **Route** it to the current work, immediate escalation, one tracked follow-up,
   or a clearly non-blocking batch.

Materiality identifies consequence and value; the critical path identifies the
current dependency or blocker controlling the accepted outcome. For multi-step
work, keep that focus explicit, allocate capable attention, tools, and bounded
resources there, and reassess it when evidence, dependencies, blockers,
integration, or gates change. Allied work belongs in the current run when it
unblocks or de-risks that path, satisfies this quality contract, or is a clear,
safe, local, in-scope improvement with bounded validation. Critical-path focus
never waives accepted quality, testing, security, review, documentation, or
recovery, and it never licenses unrelated bundling.

Do not postpone an earned improvement merely because it is secondary. Fix and
verify it while context is warm when the conditions above hold. Collect only
uncertain deferral candidates instead of interrupting the peer for each
observation. At the next natural cross-lineage review or closeout checkpoint,
both primary seats inspect the consolidated batch and choose `fix now`, `track
once`, or `drop`; an unavailable seat is recorded as reduced assurance, not
silent agreement. A worthwhile deferral uses one existing tracking altitude
with its evidence/value and a deterministic event trigger such as the next
touch of the surface, a named dependency landing, a named release/quality gate,
or recurrence of the symptom. Age alone, vague "later", one task per nit, and
preference-only backlog entries are invalid.

Dispatch is not disposition. A background task, notification promise, relay
idle signal, or transport completion does not close the batch. The host waits
for the actual bounded peer result and verifies native provenance plus content
before reporting the checkpoint complete.

## Blocker navigation

Do not confuse missing evidence with missing operator intent. Classify an
impediment before escalating it:

- a discoverable fact or technical failure is reproduced, isolated, and tested
  with a bounded probe tied to a new hypothesis. For a defect that resists a
  first glance, that probe is one command already run that fails on the
  **exact reported symptom** before hypothesising;
- a local reversible implementation choice inside the accepted outcome uses
  repository evidence and the safest durable route, with the choice disclosed;
- an external dependency or enforced gate is recorded with the exact evidence
  or input that clears it; and
- a choice that changes desired outcome, public contract, scope or authority,
  risk tolerance, or an irreversible tradeoff belongs to the operator.

Record the last failed attempt and what evidence changed. One bounded
confirmation of a prior failure is allowed when current provenance or freshness
materially matters; state that evidence question. If it reproduces the same
failure, change hypothesis or strategy and never retry it again unchanged. When
a bounded tactical cycle fails, move up a level: restate the actual constraint
and current critical path, compare viable strategies, and reroute only if
accepted outcome, scope, authority, and every quality/safety gate remain intact.
Ask the operator only for a real external dependency or owner decision, and
present verified state, attempts, options with consequences, and a
recommendation. Gate failure is information to fix or honor, not automatic
evidence that the operator must decide.

A **gate** is the verification check (`codeflow test` target, coverage floor,
OSV/security scan, `validate --docs`, …), not the CI job that happens to run
it. Classify redness before acting:

1. **Assertion-red.** The check ran to completion and failed its contract.
   Honor it: fix, or do not ship. Model consensus and a green sibling job for a
   *different* check cannot override it.
2. **Infra-incomplete.** The job never finished (hosted runner lost
   communication, SIGTERM/shutdown, OOM, billing or minutes cutoff, timeout
   with no test result). That is missing *job* evidence, not a failed check.
   Do not treat the job name as a failed test. If the same check already
   completed green in a sibling CI job or a local `codeflow test` /
   `codeflow validate` run of that target, the gate is evidenced; record the
   infra death as `track once` (runner capacity or job shape), not a product
   defect. Retrying the same unfinished umbrella job without a new hypothesis
   is orbiting.
3. **Never ran.** The owed check has no completed result anywhere. That is a
   missing gate: blocker or declared limitation, never a pass.

A red job that only restacks already-green checks is (2), not (1). Asking the
operator to pick an implementation tactic because a job name is red is the
failure ADR-0038 forbids. Asking them to wait, rerun, or override a host
required-status that is infra-incomplete *is* operator-owned: it is merge
authorization on that host, not a failed test. Agents still never merge.

Remediation effort is planning input only. It may change sequence or ownership;
it never lowers severity or justifies choosing an easy cosmetic change over a
material one. Repeated minor symptoms may be evidence of one major systemic
cause, so investigate the pattern before reporting a pile of isolated nits.

General reviews retain `blocker | major | minor`: blocker and major findings
lead the report; cosmetic, stylistic, and personal-preference nits are minor and
non-blocking, appear afterward, and do not prevent approval when they are the
only findings. Security reviews retain their CVSS-aligned
`critical | high | medium | low | info` severity and independent confidence;
do not translate that vocabulary inside the security report. When a general
review consumes a security verdict, a confirmed or likely critical/high
security finding is a blocker.

A product, UX, UI, interaction, or visual-design finding anchored in the brief,
settled `DESIGN_INTENT`, applicable accessibility target, or observed user
behavior is graded by this same materiality rule; it is not demoted merely
because it concerns design. An unanchored aesthetic preference remains a minor,
non-blocking observation.

Do not silently absorb out-of-scope work. An evidenced imminent severe risk is
escalated immediately; another material observation becomes one tracked item
with evidence and a proposed route. Isolated nits are noted or batched, not
turned into one issue each. No external mutation or scope expansion follows
from discovery without the authority required by the task. "Nothing material
found" is a valid result; issue farming and fabricated proactive signals are
failures.

## Parallel execution contract

Parallelize only workstreams whose inputs and outputs can be isolated. Record:

```text
SETTLED_TASK_GRAPH:
PARALLEL_TASKS:
TASK_BRANCH_WORKTREE_OWNER:
SHARED_FILE_OWNER:
INTEGRATION_BRANCH_AND_ORDER:
HOST_RESOURCE_BUDGET:
PER_TASK_GATES:
POST_MERGE_GATES:
```

`SETTLED_TASK_GRAPH` references the exact graph already approved in Plan vN; it
is not a divergent second copy. Parallel eligibility comes from graph topology,
but fan-out still requires a critical-path benefit and safe isolation. Each
implementation task has one writer, branch, and worktree. Shared schemas,
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
- end-to-end tests for critical changed journeys and irreversible operations;
- dependency and vulnerability scanning plus a source-to-sink security review;
- regression tests for each fixed defect;
- repository-specific validation, packaging, or migration checks.

For each material changed journey, map the exercised path before selecting the
end-to-end evidence: user or system entry point; every affected in-project
frontend, service, job, queue, persistence, and authorization boundary; relevant
deployment/runtime configuration; and the observable outcome and recovery path.
At least one test at the highest faithful surface drives the real changed path
through every applicable affected boundary. A UI test backed by a mocked changed
service, or a service test that bypasses changed persistence or runtime wiring,
does not prove that whole journey. Keep unit and integration tests as faster
diagnostics; they complement rather than replace this vertical proof.

Use a controlled double only beyond the system's ownership boundary when the
real dependency is unsafe, unavailable, non-deterministic, or prohibitively
costly. Pin that seam with a contract/integration check where feasible and
record it as controlled and unexercised; never claim that dependency was live
or exercised. Infrastructure, packaging, installer, migration, and
configuration changes require an ephemeral/deployed-runtime canary or
equivalent native evidence for the affected path. A category may still be
`N/A` for a change with no material journey, but the plan names the topology
evidence that makes it so.

For performance-, scale-, or concurrency-sensitive paths, review the actual
operating shape rather than only functional output: algorithmic complexity and
N+1 access, bounded work and memory, backpressure and cancellation, blocking in
async paths, state ownership and synchronization, lost updates, races,
deadlocks, idempotency, retry amplification, and resource cleanup as
applicable. Require a benchmark, profiler, load/stress test, race/concurrency
test, or direct operational measurement when a material claim or evidenced risk
needs it; do not add ceremonial performance tests to an unaffected path.

Record deterministic and contextual evidence as complementary layers. The
deterministic layer selects applicable syntax/style, dependency/SCA,
data/control-flow, taint, secret, and project-owned architecture checks. The
contextual layer independently verifies intent, business logic, deep semantics,
material performance behavior, state/environment interactions, and emergent
anomalies. A deterministic red result cannot be overridden by model agreement;
an agentic verdict cannot claim an analyzer ran when it did not. If a relevant
SAST/taint lane is unavailable, record the residual risk and disposition rather
than turning absence into a pass. Each layer may mark a category `N/A` only with
surface evidence.

Quality review is also longitudinal when the repository has relevant history.
Inspect the changed surface, nearby patterns, and the smallest useful history
slice to detect repeated exceptions, dependency-direction erosion, growing
duplication, unstable abstractions, or complexity that no single diff exposes.
Do not infer a trend from one point or run an unbounded archaeology exercise:
cite the concrete sequence, its material consequence, and the corrective or
tracking decision.

Use [the verification-selection contract](verification-selection.md) to decide
whether evidence earns property/generative tests, targeted mutation testing, or
project-owned architecture fitness checks. Record the trigger or `none
selected`; these techniques strengthen the normal checks and never replace
them.

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
The Claude judgment primary owns the final contextual editorial verdict; both
seats still verify technical meaning and evidence. Do not invoke the skill for
every short response, and do not let tone override truth, policy, or precision.

Presentation is graded for proportionality in both directions: simple content
stays simply formatted, while a structure that is materially clearer visually
— relationships, hierarchy, state, timelines, mappings, decisions — uses an
ASCII diagram whose scope and detail fit the explanation. Prefer the least
complicated form that remains complete, not the physically smallest; complex
subjects may need a larger, layered, or multi-view diagram. Add a brief caption
or legend when it aids orientation. Decorative or forced diagrams, headings,
tables, and recaps are findings, not polish.
Verified truth and policy come first, then the consuming project's documented
voice and context.

## Coverage

Scenario coverage comes first: happy paths, boundaries, malformed input,
timeouts, partial failure, authorization, concurrency/idempotency, recovery,
and regression cases as applicable.

Tests must be capable of failing for a material regression in the behavior they
claim to protect. Reject tautological assertions, expectations copied from the
implementation under test, a second implementation of the same production
algorithm used as its oracle, mock-only call-wiring checks with no observable
contract, weakened assertions, test-only production branches, or unreachable
code added merely to raise coverage. Fixtures and constants may be explicit
when they represent an independently stated contract or boundary. Coverage
measures exercised lines; it never proves test integrity.

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
only when no narrower driver reaches the surface.

On an interactive user-facing change, split verification by seat. Default UI
assignment is Claude as producer and Codex as reviewer. The producer performs
the **implementer check** against `DESIGN_INTENT` (design-system fit, states,
Playwright or the platform driver). The named other-lineage reviewer
independently **QAs** the changed surface and affected journeys through
Computer Use. When that reviewer is Codex, use official app-server Computer
Use (Claude host: plugin/app-server only; Grok or Codex host: CLI via Herdr
if the daemon is missing). When Claude reviews a Codex-authored UI unit,
Claude performs Computer Use QA in Claude Code; Codex does not QA its own
unit. The sentence above about Computer Use as a *driver of last resort*
still holds for deterministic E2E. Playwright remains the deterministic web
driver; Computer Use is the QA exploration layer, not a default web driver.
Scope is every interactive
control those journeys expose — buttons, links, tabs, menus, disclosures,
fields, drag handles, scroll containers — with pointer (click, drag, scroll),
keyboard (tab order, activation, shortcuts), and applicable touch/gesture.
Cover applicable viewports including sizes where composition changes, not
only the narrowest and widest. Do not exhaust the entire product unless the
work is a full-surface redesign. Unavailable Computer Use is a declared
limitation, not a pass of interactive QA.

Check at least:

- before any concurrent browser work, allocate a task/run owner and isolate
  every mutable resource it uses: a fresh browser context/profile (prefer
  Playwright MCP `--isolated`, or a task-specific user-data directory only when
  persistence is required); a unique MCP/service endpoint when the transport
  listens on a port; non-overlapping loopback application/service endpoints;
  a per-run and, when parallel, per-worker test-data namespace/account/schema;
  and an absolute task-scoped output/report/screenshot/trace directory;
- define allocation, readiness, retention, and teardown before launch. Use
  fixture/finalizer cleanup for data and services, stop only owned process
  groups/containers/endpoints, close contexts and browsers, then verify ports
  and task-owned resources were released. Preserve failure evidence under the
  project's retention policy; delete secrets and successful disposable state;
- never attach to the operator's existing browser, default user profile, tabs,
  or active desktop. A materially necessary headed run launches a test-owned
  browser/profile and must not take over the user's current view. Computer Use
  runs only in a dedicated test desktop/session or with explicit operator
  control; if that isolated surface is unavailable, record the limitation
  rather than hijacking the user's session;
- the consuming project owns its safe port allocator/range, namespace format,
  artifact root, retention, and teardown commands. CodeFlow supplies this
  resource contract, not universal port numbers or stack-specific scripts;

- the upstream `DESIGN_INTENT`, including its valid `N/A` or `conform` path,
  before treating a design as approved;
- the project's existing design system and component library before adding a
  new pattern;
- recurring foundations or tokens, accessible primitives, reusable
  application-specific components, and their composition into views or pages;
- minimal explicit interaction state with one clear owner and framework-native
  data flow;
- no repeated one-off styling, state logic, or components when current reuse is
  evidenced; no new design system or higher-order abstraction for a one-off
  surface without such evidence;

- the approved design, its fidelity to the settled intent, and the primary user
  journeys;
- navigation, actions, guidance, validation, loading, empty, error, disabled,
  success, destructive, and recovery copy states where applicable;
- visual/verbal coherence and terminology against the project voice; localized
  variants, writing direction (LTR/RTL), and text expansion before claiming
  localization quality;
- applicable light, dark, high-contrast, system-following, manual-override,
  persistence, reduced-motion, imagery, and data-visualization behavior without
  an incorrect-mode flash;
- responsive/adaptive behavior at relevant sizes, including the intermediate
  viewports where composition actually changes;
- keyboard navigation, focus, labels, contrast, and other applicable
  accessibility requirements against the project's target; for web surfaces,
  default to WCAG 2.2 AA unless a stronger target or a different
  surface-appropriate target with recorded rationale governs;
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

Compare the rendered result with the settled intent using only applicable
dimensions—governing idea and composition, hierarchy, interaction, content,
visual/verbal coherence, type and colour roles, layout, spacing, imagery,
density, motion, states, appearance modes, and platform fit. Distinguish an
approved improvement or evidenced implementation constraint from unjustified
drift. A changed design contract or material direction creates Plan vN+1; a
reviewer's unsupported taste does not.

## Independent review

Each producer first-verifies its unit. The approved cross-lineage reviewer then
reviews the actual unit, reruns relevant gates, and checks conformance with the
chosen design. The Claude judgment primary separately reviews the integrated
diff and owns the Claude quality verdict. For a unit it authored, Codex is the
independent reviewer; the primary's integrated pass is not described as
independent review of that unit.
Every reviewer challenges the evidence rather than accepting a summary.

Classify findings by severity, order them by the materiality contract above,
and support each with a concrete trigger or reproduction plus its priority
rationale. Security approval requires checking untrusted inputs through
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
- every unit has approved cross-lineage review and the selected Claude judgment
  primary has approved the integrated design/code judgment;
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
"Failing" means an assertion-red completed check. "Missing" means the owed
check never completed anywhere. An infra-incomplete duplicate job does not
make a completed same-check missing.
