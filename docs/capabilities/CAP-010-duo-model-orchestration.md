# Duo model orchestration

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full contract for CAP-010. Update both in the PR that ships the work. -->

## Concept

**Two model families draft the same work independently, and only then meet.**

```cf-stage
CLAUDE SEAT | own research · risks · complete plan @accent
CODEX SEAT | own research · risks · complete plan
->
PLAN vN | reconciled, both seats approve the assignments
->
EXECUTE | each unit first-verified by its actual executor
->
CROSS LINEAGE REVIEW | a lineage different from the author @positive
caption: neither seat sees the other's draft before both exist; Claude leads design
```

`/cf-model-orchestrator` is the host-neutral default for every non-trivial
repository task: research, analysis, planning, design, implementation,
debugging, security, substantive documentation, review, or verification. It
selects the smallest complete outcome mode, so research/planning-only work
settles an evidenced artifact and stops before implementation. The first stage
is the load-bearing one: the two lanes share no context edge, so the second
seat is never reduced to critiquing a plan the first already supplied.

## Architecture

The plan is the structure. Both seats independently research, analyze risks,
and draft complete plans from the same immutable brief before either sees the
other's conclusions. This is an anti-anchoring requirement: Codex must not be
reduced to critiquing a plan Claude has already supplied. After both drafts
exist, Claude leads design. The host reconciles a versioned plan whose task
rows name the responsible primary, actual binding-or-route executor, execution
mode, routing reason and provenance, available usage evidence with freshness or
an explicitly unknown value, and cross-lineage reviewer.

Both seats approve those assignments before implementation; changing ownership,
scope, lineage, isolation, or a named reviewer invalidates the approvals, while
a permitted primary-owned executor change inside that boundary does not. Each
actual executor first-verifies its unit, the responsible primary inspects and
accepts it, and a lineage different from the actual author's reviews it
independently. The selected `claude-judgment-primary` owns integrated Claude quality judgment
without claiming independent review of its own unit.

Each seat is reached through its vendor's own native interactive harness, so
the host a session starts in decides the transport, not the contract:

| Host | How it reaches the other seat |
|---|---|
| Claude Code | Reaches Codex through the official plugin/app-server |
| Codex App or interactive CLI | Reaches Claude through Herdr, the named-tab terminal host for an interactive peer CLI, with tmux as the degraded host |
| Grok Build | Reaches Codex through the official `codex` CLI and local app-server daemon, and Claude through Herdr plus schema-v2 |

Grok-hosted lane canaries are in
`docs/verification/grok-host-duo-canary-2026-09-07.md`; they are not a
qualified binding. The standing pair remains the quality floor. Extra
catalog families (today Grok) are named when a routing-policy trigger fires
and the family is available; unavailable is an evidenced limitation, never a
silent third vote (ADR-0054). Primaries default to high, use proportionate
worker effort when useful, and obtain same-family xhigh reasoning on trigger
mid-session rather than restarting the host
(ADR-0056). Default UI assignment is Claude execution and implementer check plus Codex
Computer Use QA on the app-server; if Codex produced the UI, Claude QAs
independently. Another harness, including Hermes (an outer coordinator, not a
native CodeFlow host), normally delegates the repository task to one native
CodeFlow host; direct coordination requires both
native lanes and the full contract. Explicit host/peer/worker roles prevent recursive orchestration.

## Technical

### Design direction

For material product, UX, interaction, or visual-direction work, the
orchestrator loads `cf-design` and records a proportionate `DESIGN_INTENT`
inside that same plan. Cosmetic changes may collapse as not applicable,
bounded established-system work may conform, new surfaces settle a direction,
and materially open novel work compares two or three viable directions first.
Language/voice and appearance modes are resolved only where applicable from
project evidence: localized quality needs localized evidence, mode claims need
rendered preference and persistence evidence, and CodeFlow utility defaults do
not become product design authority. After selection, further variants require
one named unresolved material choice and stop when it is settled. Material
references and assets retain proportionate authority, rights/privacy,
transformation, and product-use provenance, while material feedback names the
exact reviewed version in Plan vN+1 rather than a parallel design database.
The Claude judgment role leads intent, owns real design implementation and
fidelity, and directly executes until a matching Claude design route is
scoped-qualified. Candidate design routes are limited to controlled disposable
qualification fixtures; scoped-qualified routes execute only exact evidenced
tuples and never acquire direction or fidelity-approval authority. Another
family designs only under an explicit task-specific operator override. Claude
absence alone is not one. Codex challenges feasibility and fidelity, and both
approve the exact plan. Review anchors blocking design
findings in the accepted brief, intent, accessibility target, or observed
behavior rather than taste. The design-direction eval pack covers this
selection, operator precedence, evidence-grounded design-choice review,
bounded refinement, sourcing/privacy, reviewed-version retention, distinct
evidenced product voices, localization honesty, utility/product isolation,
appearance-mode behavior, accessibility, and rendered fidelity
(ADR-0043, ADR-0051).

### Quality floor and routing evidence

The shared quality and routing resources require reproducible
evidence, relevant unit/integration/e2e and UI tests, an 80% production-code
coverage floor where measurable (90% normal target), security review, and
bounded rework. It also blocks material avoidable complexity: both seats review
design proportionality, every executor first-verifies the smallest coherent
implementation, the accountable primary inspects it, a lineage different from
the actual author's independently reviews it, and the directly
invoked Claude judgment primary reviews the settled design and actual
integrated diff for the final quality verdict. Substantial prose additionally
loads `cf-editorial-review`: both seats protect technical meaning and evidence,
while the Claude judgment primary owns the final contextual voice and editorial
verdict. Cross-model
callers invoke both primary seats directly using the selectors, default and
escalation effort, triggers, and permitted internal routes in the current
ensemble record. Primary seats retain their plan, integration and approval
duties; each owning primary controls its internal routes, and the selected
Claude primary owns Claude-side judgment. A natively proven candidate may
execute bounded non-design work under primary review without becoming
qualified. A scoped-qualified claim is limited to its evidenced tuples and
remains distinct from full primary promotion or an economy/default claim. Each
run records actual model versions, applied effort and route,
requested-versus-observed provenance, and scoped usage evidence rather than
inferring availability, application, quota, or savings.

### Whole-flow and UI isolation

For a material changed journey, the end-to-end plan maps the affected entry,
in-project components, persistence/queue, external seam, infrastructure/runtime
wiring, observable result, and recovery path. One faithful vertical run crosses
every applicable changed boundary; disconnected unit/integration passes and a
mocked changed service are not whole-flow proof. Parallel UI tasks allocate
task-owned isolated browser state, applicable listening/application endpoints,
namespaced test data, run-scoped artifacts, and teardown evidence without
attaching to the operator's browser or active desktop. The project supplies its
own allocator/ranges, namespace, artifact, retention, and cleanup commands
during customization (ADR-0044).

### Task graphs and durable records

For multi-task work, both approvals cover one acyclic Plan vN graph. Ordinary
completion uses bare edges; only genuine pre-approved decisions use observable
guards. Task frontmatter keeps non-executable structural `depends_on` data.
`validate --docs` checks canonical identities and filenames, relationship
shape, parent-or-standalone ownership, spec readiness, stable integration
targets, completed acceptance criteria, and malformed, dangling,
self-referential, duplicate, or cyclic topology. Explicit `codeflow work start`
always checks the assigned CodeFlow task branch's planning anchor. Pre-commit
and detached CI apply that same read-only merge-base check when full-tier or
recognizable historical CodeFlow task tracking is active. It proves validated
planning is present on the declared stable target. Material graph or cross-task
contract changes force Plan vN+1; in-node implementation detail does not.
Review-relevant bounded discoveries persist at task closeout; closeout cannot
retroactively approve a
material change. Project organization keeps one authoritative work-item home
and links, rather than mirrors, external planning methods or trackers. A foreign
tasks folder alone does not activate those durable gates; malformed relevant
tracking state yields a diagnostic instead of a silent opt-out. New projects
earn structure from accepted ownership and interface boundaries; existing
projects retain credible native layouts. Current requirements stay living
authority while SPC files freeze only warranted change agreements. External
approval never waives active CodeFlow execution gates.
Verification planning selects property tests, targeted mutation testing, or
project-owned architecture fitness checks only when the risk and oracle
evidence earn them. CodeFlow adds neither a scheduler nor mandatory
consuming-project tools.

### Parallelism

Independent implementation tasks use bounded, host-resource-aware parallelism:
one owner/branch/worktree per task, a single owner for shared files, serialized
landing through `codeflow integrate` to `integration/<epic>`, affected gates
after each landing, and aggregate gates plus review on the combined diff.
Missing seats degrade legibly to solo; mid-run failure blocks and escalates.

### What pins this contract

The unattended Claude workflow is explicitly single-vendor and rejects the old
`duo` preset semantics. Manifest parity tests pin byte mirrors, while
`orchestration_contract.rs` pins the two-draft anti-anchoring rule, design and
review roles, hard coverage floor, security lenses, always-loaded reasoning
duties, host, UI, and reverse-lane contract markers. Runtime adapter behavior
is exercised by the CAP-009 hook unit and CLI tests. No engine model router is
added; deterministic gates and the human-merged PR remain authoritative.
