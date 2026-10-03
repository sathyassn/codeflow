# Duo model orchestration

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full contract for CAP-010. Update both in the PR that ships the work. -->

## Concept

**Two model families discover the same work independently, then one plan is
challenged once.**

`/cf-model-orchestrator` is the host-neutral entry for routed repository
work, decided by the paths a change touches: an adopter-facing path (product
code, managed instructions, hooks, policy, CI, shipped templates, watched
contracts), research or analysis that will drive such a change, and plan,
design, security or irreversible work. Other edits go direct. It selects the
smallest complete outcome mode, so research or planning-only work settles an
evidenced artifact and stops before implementation. The first stage is the
load-bearing one: the two lanes share no context edge. How the work it plans
then reaches `main` is on [how work moves to main](../delivery.md).

It is for the two primary seats and the host that runs them. It is not a
model router and not the unattended pipeline, which stays single-vendor.

## Architecture

Both seats independently discover from the same immutable brief and
repository scope, and neither sees the other's findings first: source and
documentation evidence, assumptions verified or still unresolved, edge, error
and security cases, and risks to compatibility, data, UX and operations. The
host records both outputs without collapsing disagreements. Claude then
drafts the one plan from its native session, and Codex challenges it against
its own findings; there is no second plan and no reconciliation round. The
plan names each task's responsible primary, actual binding-or-route executor,
execution mode, routing reason and provenance, and cross-lineage reviewer.
Settlement ends when both seats approve one version or the host stops for the
operator; approval of an older version does not carry forward. A task inside
an approved epic does not repeat discovery; it starts from the epic plan.

After approval, only a change of outcome, cross-task interface, dependency
graph or safety boundary creates a new plan version; a reassignment is
recorded where the assignment lives, and for an unstarted task rides in the
batched epic amendment (ADR-0076). Each actual executor first-verifies its
unit, the responsible primary inspects and accepts it, and a lineage
different from the actual author's reviews it independently. The selected
`claude-judgment-primary` owns integrated Claude quality judgment without
claiming independent review of its own unit.

Each seat is reached through its vendor's own native interactive harness, so
the host a session starts in decides the transport, not the contract:

| Host | How it reaches the other seat |
|---|---|
| Claude Code | Reaches Codex through the official plugin/app-server |
| Codex App or interactive CLI | Reaches Claude through Herdr, the named-tab terminal host for an interactive peer CLI, with tmux as the degraded host |
| Grok Build | Reaches Codex through the official `codex` CLI and local app-server daemon, and Claude through Herdr |

Grok-hosted lane canaries are in the repository at
`docs/verification/grok-host-duo-canary-2026-09-07.md`; they are not a qualified
binding. The standing pair remains the quality floor. Extra catalog families
(today Grok) are named when a routing-policy trigger fires and the family is
available; unavailable is an evidenced limitation, never a silent third vote
(architecture decision record ADR-0054).

| Rule | Behavior |
|---|---|
| Effort | Primaries default to high, use proportionate worker effort when useful, and obtain same-family xhigh reasoning on trigger mid-session rather than restarting the host (ADR-0056) |
| UI work | Claude executes and runs the implementer check, and Codex runs Computer Use QA on the app-server; if Codex produced the UI, Claude QAs independently |
| Other harnesses | Another harness, including Hermes (an outer coordinator, not a native CodeFlow host), normally delegates the repository task to one native CodeFlow host; direct coordination requires both native lanes and the full contract |
| Roles | Explicit host, peer and worker roles prevent recursive orchestration |

## Technical

The operating rules this capability enforces once a plan exists.

### Design direction

For material product, UX, interaction, or visual-direction work, the
orchestrator loads `cf-design` and records a proportionate `DESIGN_INTENT`
inside that same plan (ADR-0043, ADR-0051).

| Situation | What the orchestrator does |
|---|---|
| Cosmetic change | may collapse as not applicable |
| Bounded work in an established system | may conform to it |
| New surface | settles a direction |
| Materially open novel work | compares two or three viable directions first |
| Language, voice and appearance modes | resolved only where applicable from project evidence: localized quality needs localized evidence, mode claims need rendered preference and persistence evidence, and CodeFlow utility defaults do not become product design authority |
| After selection | further variants require one named unresolved material choice and stop when it is settled |
| References and assets | material ones retain proportionate authority, rights/privacy, transformation, and product-use provenance |
| Material feedback | names the exact reviewed version in Plan vN+1 rather than a parallel design database |

The Claude judgment role leads intent, owns real design implementation and
fidelity, and directly executes until a matching Claude design route is
scoped-qualified. Candidate design routes are limited to controlled disposable
qualification fixtures; scoped-qualified routes execute only exact evidenced
tuples and never acquire direction or fidelity-approval authority. Another
family designs only under an explicit task-specific operator override. Claude
absence alone is not one. Codex challenges feasibility and fidelity, and both
approve the exact plan. Review anchors blocking design findings in the accepted
brief, intent, accessibility target, or observed behavior rather than taste.

The design-direction eval pack covers this selection, operator precedence,
evidence-grounded design-choice review, bounded refinement, sourcing/privacy,
reviewed-version retention, distinct evidenced product voices, localization
honesty, utility/product isolation, appearance-mode behavior, accessibility,
and rendered fidelity.

### Quality floor and routing evidence

The shared quality and routing resources require reproducible evidence,
relevant unit/integration/e2e and UI tests, the project's configured coverage
gate (CodeFlow's own is `--fail-under-lines 90`), security review on its
trigger, and review that ends on evidence rather than a round count
(ADR-0076). They also block material avoidable complexity:

| Who | Duty |
|---|---|
| Both seats | review design proportionality |
| Every executor | first-verifies the smallest coherent implementation |
| The accountable primary | inspects it |
| A lineage different from the actual author's | independently reviews it |
| The directly invoked Claude judgment primary | reviews the settled design and actual integrated diff for the final quality verdict |

Substantial prose additionally loads `cf-editorial-review`: both seats protect
technical meaning and evidence, while the Claude judgment primary owns the
final contextual voice and editorial verdict.

Cross-model callers invoke both primary seats directly using the selectors,
default and escalation effort, triggers, and permitted internal routes in the
current ensemble record: `current-ensemble.json`, the managed list of primary
selectors and effort defaults. Primary seats retain their plan, integration and
approval duties; each owning primary controls its internal routes, and the
selected Claude primary owns Claude-side judgment.

A natively proven candidate may execute bounded non-design work under primary
review without becoming qualified. A scoped-qualified claim is limited to its
evidenced tuples and remains distinct from full primary promotion or an
economy/default claim.

Each run records actual model versions, applied effort and route,
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

Planning happens once, when an approved brief or spec is broken into an epic
and its tasks (ADR-0076); the flow from there to `main` is on
[how work moves to main](../delivery.md). For one obvious task, the settled
plan records `TASK_GRAPH: N/A (single task)`. For multi-task work, `/cf-plan`
turns the approved assignments and real dependencies into one acyclic Plan vN
graph that both approvals cover. Every durable task lists its direct
structural predecessors in non-executable `depends_on` frontmatter; the
metadata preserves topology and does not execute the plan.

| Rule | Behavior |
|---|---|
| Edges | ordinary completion uses bare edges, and bare active predecessors must land; only genuine pre-approved decisions use observable guards |
| Guards | select mutually exclusive branches; a later join may list every structural candidate while waiting only for active predecessors plus resolution evidence for the alternatives; missing or ambiguous guard evidence creates Plan vN+1 rather than an improvised route |
| New plan version | a change of outcome, cross-task interface, dependency graph or safety boundary forces Plan vN+1 and both approvals; an ownership change and a task's own criteria change do not (ADR-0076 narrows ADR-0040) |
| Same plan version | ordinary steps, bounded rework, extra strengthening tests, in-node implementation detail, or another safe topological order inside the same contract remain execution-ledger evidence |
| A later change of scope | a follow-up, a new or split outcome, a reassignment or another task's criteria ride in one batched epic amendment on a `plan/` branch with one other-lineage reviewer; nothing plans again per task |
| `validate --docs` | checks canonical identities and filenames, references, relationship shape, parent-or-standalone ownership, spec readiness, stable integration targets, completed acceptance criteria, and malformed, dangling, self-referential, duplicate, or cyclic topology |
| `codeflow work start` | checks the planning anchor of the task the branch carries on any work prefix (`task/`, `fix/`, `feat/`, `spike/` and the rest; not `plan/` or `integration/`): the epic's planning change for an epic task, or the record at head for a standalone task whose record arrives in its own pull request; with its parent or standalone rationale, approved specs and completed predecessors, without mutating repository state |
| CI, once per pull request | applies the same read-only merge-base check when full-tier or recognizable historical task tracking is active, proving validated planning is present on the declared stable target; the per-commit hook does not. CI shares the structural core of the check and not the start gate, so it admits a standalone task's own record that arrives complete with a valid acceptance block |
| Planning level | both report at the `git.work_planning` level: `block` by default, or `warn`, which reports the finding and lets the work continue; a declared target whose local branch is strictly behind its configured upstream anchors on that upstream, with a note, and a diverged pair is refused |
| Acceptance | the acceptance block rides in the task's own pull request as its last commit; a closeout cannot retroactively approve a material change |

#### Pull request classes

With tracking on, `codeflow ci` classifies every pull request: tracked
(`Task: TSK-NNN`, or the id the branch carries), an epic's planning-only
range (`Task: EPC-NNN`; records and `docs/plan/` only), an epic's integration
line (`Task: EPC-NNN`; a task of the epic targets it, it lands on the default
target, and it holds only merges), or an automation profile. A pull request
that names no task and no epic is refused whatever it touches; the
`Task: none` route is gone, and `git.direct_changes` is accepted and ignored.
Where tracking is inactive, the `Task:` line names the harness's tracked
unit. The range is one diff from the merge-base, and tracking is read at the
target as well as the head. Refused: an unclassified range, a mismatched
`Task:` line, a pull request that adds an epic task's record and claims it,
any record added beside a standalone task's own, and a spike that lands
anything but `docs/research/` findings and its own record. A standalone
task's own record, added in its pull request on a branch carrying its id, is
admitted through the structural checks of readiness and may arrive complete.

`task new --standalone-reason` may run on the task's own branch; `task new
--follow-up-of` runs on a `plan/` branch for an epic task. A standalone task
never uses a `plan/` branch: its follow-up is a standalone task, filed on a
task branch cut from the target with its record filled in and committed,
then claimed with `work claim`, and it lands with its work in its own pull
request (TSK-214); `epic new --integration` and
`adr new` (numbered, written `proposed`) are one command each. `init` writes
a stack default for `git.product_paths` and `update` adds it once; with
`git.breaking_watch_paths` and the embedded contract path table it decides
which ranges count as code for the pull request section rules.

#### Readiness

One readiness core judges a task for `work next`, `work claim`,
`work start`, `status`, `orient` and CI: status `todo`, no Blocker, no
`awaiting_selection`, specs approved, epic open or standalone, code
dependencies complete in the execution base, and research or decision
dependencies (`{id, kind, pin}`, the pin quoted) complete at their pinned
commit; a pin YAML reads as a number or as null is refused with the quote
remedy, and a pin left out keeps the edge unmet. `work next` lists ready,
then waiting and blocked tasks with reasons from the refs as last fetched;
`work claim` fetches, refuses a task a visible branch already carries, and
pushes `task/TSK-NNN-<slug>` as an advisory claim. `work claim` and
`work start` accept a code predecessor that is reviewed but not complete only
through `--on TSK-NNN@<sha>`, a pin a review of the predecessor names that
still equals the predecessor branch's tip: `claim` checks the pins and cuts
the branch from the pin that contains the others (incomparable pins refuse),
`start` checks each pin is an ancestor of HEAD, and CI still requires the
predecessor complete at the merge-base when the task lands. `status` shows
derived active, ready, landed and conflicting branches and epic progress, and
never calls a live integration line removable. A selection that removes
`awaiting_selection` lands only from `plan/`, and `spec new --for` links
every consumer in one change.

#### Record transitions and frozen specs

Record status moves only by legal transitions (SPC-013 R-30 to R-35).
`codeflow task status`, `epic status` and `spec status` write the status and
only the sections the transition needs: a `## Blocker` with reason, owner and
revisit for a blocked task, Closeout lines `- cancelled:` and `- scope:` for a
cancelled record, and a fenced `yaml` acceptance block on completion.
A complete task can be fixed in one PR: reopen with a reason, retain its
old block under `acceptance_superseded:`, fix it, then complete again with a
reviewed commit inside that PR. The old block and criteria are compared
with the anchored target; copied review blocks and a stale review carried
by an earlier landing merge are refused. The separate planning-reopen
path remains valid (TSK-140).
Reopening keeps the old block under `acceptance_superseded:` with its reason;
a task completed before the migration, with no block, records a Closeout line
`- reopened: <reason>` instead. Sections and blocks inside HTML comments or
enclosing fences never count. A spec is approved or superseded only in a
planning-only change, and supersession adds its successor in that change.
Approval reads the spec's `open_questions` frontmatter list and needs it
present and empty; the `## Open questions` prose is context and is not
parsed. A spec written before the field stays valid, but it is approved
only once it carries the list; null or a value that is not a list is an
error (TSK-135).

An approved spec is amended in place until it is `implemented`; after that
its text below the frontmatter is frozen and a change to it is refused, so a
changed contract is a new spec. The judge reads the spec's history, so a
later supersession or consumer reopen does not thaw it. `spec status <id>
draft` is never written: it gets the same refusal as the hand edit, naming
both routes (TSK-169).

The verbs are safe editors, not the only writers: one core judge rules on a
verb's proposal, on a hand edit (`validate --docs --since <ref>`) and on each
record a pull request changes (`codeflow ci`). No verb writes `in_progress`,
and spec `implemented` is derived from the consumers, so an approved spec
whose consumers are done is healthy and draws no warning. A Closeout item
`- acceptance: historical evidence unavailable; ...` that names the landing
merge stands in for the acceptance block of a task completed before the
migration baseline and never reopened since (SPC-013 R-101). Epic close needs
every task terminal, every criterion verified and every consumed spec
implemented or still consumed. A cancelled task never verifies a criterion:
one served only by cancelled tasks is verified, like an unserved one, in the
epic's own acceptance block with its evidence. The verb, CI and the release
judge bind that block as a task's: its reviewed commit exists with only the
epic's status and Closeout changed after it, and a waiver names a
planning-only amendment of that criterion inside the reviewed commit
(TSK-214). New records list criteria as `- AC-n` without
a checkbox. The rules apply from the `work_records_baseline` commit in
project config, which `codeflow update` records once, and by transition: an
unchanged older record keeps its exact-blob exemption. `git.work_records`
accepts `block` or `warn`, never `off`. The ledger's producerless work-graph
event types are retired.

#### Completion binding

A completion is bound to the reviewed commit (SPC-013 R-52 to R-54, R-60 to
R-62): `task status complete` and `codeflow ci` check that the block's
`reviewed` commit, named by object id, is the head or an ancestor after
which only the record's status and Closeout changed, apart from a merge from
the integration line whose tree equals the clean re-merge, and that each
waiver names the commit that changed that criterion: a planning-only
amendment on the target, or a record-only commit in the pull request's own
range before the reviewed commit; the verb also refuses uncommitted changes
outside the record. A clean task landing can carry that reviewed source onto
its line, including when only status and Closeout changed between the review
and the landed task head; unrelated line work before the landing does not
invalidate that source. Direct work and transported work use the same
binding predicate. At a batch landing each completion binds at the commit
that introduced its block, so reviewed heads land together on one candidate.
A task pull request may change its own criteria, and CI prints the change
for the reviewer; a reopened task keeps its criteria, and another task's
criteria change only in its own pull request, a planning-only change or a
checked epic line. A range touching
the adopter-facing path set needs a `(journey)` criterion or one serving the
epic's journey, and a leaf serving it says what ran or its narrower path. A
criterion tagged `(after release)` is `deferred` with owner, window and a
listed follow-up. A tag opens or closes its criterion, trailing sentence
punctuation included; a tag inside the text does not count.
`git.work_records` sets the binding and journey rules; the frozen criteria of
other records always block.

#### Release rules

A release branch (SPC-013 R-120) is one whose name matches
`git.release_branch_pattern` in the policy at the destination's default
target, or `integration/release-*` when the key is absent; the policy check
refuses a pattern that matches the default target or an epic line. On a push
to a release branch, a pull request into one, or its pull request into the
default target, pre-push, `codeflow ci` and `task status complete` judge each
change where it was introduced. A merge whose other parents lie on a verified
epic line's or the default target's first-parent chain is an import: a path
equal to the expected import's tree entry is brought, and its completions
bind where they were introduced (only a later completion from the task's own
line that binds there, and that the line landed after the earlier ones,
supersedes them; a direct completion is judged as it was made); a brought
criteria change is judged again where it landed on its line. The records rule
judges a brought record where it was introduced too: a spec approved on its
line counts where it landed there, which must have been planning-only, and a
record whose only change is a `uid` backfill landed on its line is not judged
again.

Two project-config tables, read at the default target, exist only for the
2.x to 3.0 transition: a brought criteria change landed at or before the
cutoff in `release_rule_baseline`, and a brought complete task without an
acceptance block whose record last changed at or before the cutoff in
`release_records_baseline`, each for the line the task targets and on its
first-parent chain, are listed as information. `codeflow init` and `update`
write the adoption marker `release_rules = 1` in project config and never a
table. The marker never decides whether these rules apply; once the default
target carries it, removing it or changing its value, there or in the judged
range, makes every release check refuse. A table is honoured only as a
one-time bridge for CodeFlow's own history: added in one commit and never
changed, at or before the marker's first commit on the default target, after
project config without the marker, with every cutoff from before the rule, on
its line's first-parent chain and one of CodeFlow's approved cutoffs, which
the judge compiles in; otherwise every release check refuses. A consuming
project, a fork that keeps CodeFlow's root commit included, gains no relief
for its own work. No flag, variable or policy key skips the rule or a table.
The marker's history is read from the parents each commit records.
History the check needs that it cannot read in full, cut by a shallow
boundary or missing a config object, refuses as well: adoption is never
inferred absent from it. A graft file or a replace ref, which would change
the commits a release check walks, refuses too. Anything else is direct work:
it freezes criteria, and beyond planning records it belongs to the one open
task with `role: release-integration`, whose completion binds to the release
head.

Pre-push judges a push to a release branch on everything it adds to the
default target's tip, as its pull request is. Whether the release rules apply
is read at the checkout, the pushed commit and the default target's tip,
fetched when missing; a push is refused when the destination does not answer
or any of these cannot be read. The hook asks the destination once per push
and hands that answer to its own `codeflow ci` through a hidden input; a run
given that input is advisory only, and hosted CI, the authority, never takes
it. When the default target's policy or objects cannot be read, the check
fails closed. The check states that it proves structure and binding only, and
cf-reviewer, cf-consult and cf-ship ask whether each criterion is supported
on this source and achieves the outcome.

CodeFlow's repository-only release integration workflow loads its write-token
job from the default branch after epic-line workflow completions and on a
daily schedule. Each surviving run imports all verified epic-line tips,
including landings whose pending runs were replaced. The
`release_integration` example runner checks prospective merges with this
judge and the shared reading-structure check before pushing. Conflicts and
findings leave the release branch unchanged and name the open
release-integration task from the destination's default tip plus a local
reproduction command. The workflow is not a task-PR gate and is not installed
for adopters. Its local fixtures are in `release_line_cli`; `init_e2e` checks
the conditional ship guidance.

#### Durable work and external trackers

For active CodeFlow durable work, the flow is on
[how work moves to main](../delivery.md): one planning change at the
breakdown, one pull request per task from `task/TSK-NNN-<slug>`, reviewed
heads landing in small batches with one full gate, and the operator merging
`main`.

CodeFlow is the natural authority for finite repo-local gated work. Project
organization keeps one authoritative work-item home and links, rather than
mirrors, external planning methods or trackers.

| Situation | Where authority stays |
|---|---|
| Multi-team or cross-repo work, or work driven by assignment, roadmap or service-level agreement (SLA), or already owned by an established method | the team's external tracker, for its own portfolio and product items |
| Active full or recognizable historical CodeFlow task tracking | distinct repository-execution records and planning anchors are still required; an external ticket or approved spec cannot satisfy or waive `work start`, pre-commit, or CI |
| External references | opaque IDs or URLs in epic and task `external_refs` metadata, not gate inputs; status, specs and task trees are never mirrored |
| Durable tracking inactive | the approved external method or native/session plan at earned durability, without claiming these workgraph guarantees or silently upgrading tier |
| A foreign tasks folder alone | does not activate the durable gates; malformed relevant tracking state yields a diagnostic instead of a silent opt-out |
| A host-local database | may cache or index records but is not shared team authority |

`/cf-customize` records this post-init project choice; the agent-facing
decision model lives in `cf-method/references/project-organization.md`. New
projects earn structure from accepted ownership and interface boundaries;
existing projects retain credible native layouts. Current requirements stay
living authority while specification (SPC) files freeze only warranted change
agreements. External approval never waives active CodeFlow execution gates.

### Verification strength

Normal stack tests, integration/end-to-end checks, regressions, and coverage
remain the baseline. The plan adds a stronger technique only when its evidence
fits:

| Technique | Evidence that earns it |
|---|---|
| Property or generative tests | a stable invariant, a meaningful input or state space, reproducibility and shrinking, plus a material combination risk |
| Mutation testing | targeted and time-bounded to consequential guard, decision, state, security, or recovery logic after the base suite is reliable |
| Architecture fitness check | protects a current project-owned invariant through a deterministic observable rule tied to a decision or repeated risk |

`/cf-stack` and `/cf-customize` reuse or propose the consuming project's own
reviewed commands only when earned. They do not install every technique, create
whole-repository score targets, or turn architectural taste into a gate.
CodeFlow adds neither a scheduler nor mandatory consuming-project tools.

### Selecting deterministic code analysis

CodeFlow does not impose one static application security testing (SAST)
service on every stack. During `/cf-customize`, `cf-stack` inventories
languages, trust boundaries, hosting, existing tools, and CI constraints, then
records the smallest maintained lane that provides relevant source/data-flow or
taint evidence.

| Choice | Fits |
|---|---|
| CodeQL default setup | a low-maintenance choice for an eligible GitHub-hosted repository and a supported language |
| Semgrep, Sonar, or a language-native analyzer | other stacks or governance requirements |
| Software composition analysis (SCA) and secret scanning | separate evidence; they do not substitute for SAST |

Verify the chosen analyzer rather than merely installing it: record applicable
rules, scanned-file/tool status, extraction errors, suppressions, owner,
cadence, and whether it gates locally, in CI, or through branch protection. If
no relevant lane is available, record the residual risk and disposition.
CodeFlow's own CodeQL setting is repository-specific and is not copied into
consuming projects by `init` or `update`.

### Parallelism

Independent implementation tasks use bounded, host-resource-aware parallelism:
one owner/branch/worktree per task, a single owner for shared files, and
landing on `integration/<epic>` in small batch candidates in dependency order:
the primary inspects the resolved hunks and integration seams on product
paths, asks the other lineage to review the integration effects only when it
hand-resolved a product hunk or two tasks touched one hotspot, and runs one
full gate on the candidate before the line moves (ADR-0076); unit reviews
are not repeated. Missing seats degrade
legibly to solo; mid-run failure blocks and escalates. Deterministic gates and
the human-merged PR remain authoritative.

### What pins this contract

| Pin | What it holds |
|---|---|
| Manifest parity tests | Byte mirrors of the skill across its managed copies |
| `orchestration_contract.rs` | The two-draft anti-anchoring rule, design and review roles, hard coverage floor, security lenses, always-loaded reasoning duties, host, UI, and reverse-lane contract markers |
| The hook unit and CLI tests of capability CAP-009 | Runtime adapter behavior |
