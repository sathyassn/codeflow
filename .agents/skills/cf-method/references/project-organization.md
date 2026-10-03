# Durable project organization

Load this reference when deciding whether work needs durable records, breaking
an outcome into epics/specs/tasks, organizing a monorepo, coexisting with an
external tracker, or handling discoveries during implementation. It is an
opinionated default for durable projects, not a reason to replace an existing
credible authority.

## Earn the project shape

For a new project, start with the accepted product, users, change horizon, and
delivery constraints. Map responsibilities and the interfaces between them;
then decide which boundaries deserve a native unit or an explicit owner:

- **Source and build:** use the language's real package/workspace boundaries.
  Share code only for actual consumers and a maintained contract, not to make
  every directory look symmetrical.
- **Runtime and release:** separate a process, deployment, or independently
  versioned artifact when scaling, failure recovery, operations, or release
  cadence requires it. A package is not automatically a service, and a
  separate release version alone does not establish independent governance.
- **Data and trust:** identify who can write a schema, publish a contract, hold
  a credential, or cross a trust boundary. A folder name is not access control;
  enforce the boundary in the applicable runtime, policy, and tests.
- **Verification:** keep native tests with their units and place cross-system
  journeys/contracts where their real integration can be exercised. Do not
  create a tools, contracts, design-system, or deployment tree without an
  accepted use and a named authority for its contents.

For an existing project, first locate its manifests, deployable units, data
owners, local instructions, CI, and accepted knowledge and work authorities.
Keep a coherent native layout and vocabulary; do not move source or create
parallel docs/tasks for visual conformity. Record the boundary map in the
project-owned architecture and nearest local instructions. Propose a migration
only for an evidenced conflict or operational cost, with its owner and path to
reconcile it. `cf-customize` performs this post-init project-specific review;
the human adoption guide covers choices that must be made before `init`.
Unlike later root-discovering commands, `init` and `update` use the process
current directory. Standard docs and full work records have fixed operating
destinations; a prose link does not relocate a parsed record. Recorded tier
only rises, and recognizable historical tasks may keep tracking active below
full. Default init preserves existing content mechanically, but skipped files,
combined instructions, and `.new` update conflicts need semantic
reconciliation. Foreign content at a parsed record home may still fail an
explicit `validate --docs` even when automatic task gates are inactive.

For example, an order product may have customer and operator web apps using an
actual shared design system, one order API owning order data and migrations,
and an asynchronous receipt worker with its own failure and scaling lifecycle.
An API/event contract and cross-system test are warranted if both runtimes
consume them. That does not prescribe `apps/`, `services/`, `contracts/`, or a
second data directory: each native toolchain and accepted authority chooses
its home. The worker does not gain direct order-table access merely because it
is in the same repository. No mobile app, SDK, or extra deployable is implied.

## Authority and layout

When CodeFlow durable tracking is active, Git-tracked Markdown is the shared
repository-execution workgraph authority at these fixed paths:

```text
project-management/
├── epics/
│   └── EPC-NNN.md
├── specs/
│   └── SPC-NNN.md
├── tasks/
│   └── TSK-NNN.md
└── templates/
    ├── epic.md
    ├── spec.md
    └── task.md
```

`EPC-NNN`, `SPC-NNN`, and `TSK-NNN` are independent, repo-wide sequences. A
record has one `id`, equal to its filename. Canonical paths are
`epics/EPC-NNN.md`, `specs/SPC-NNN.md`, and `tasks/TSK-NNN.md`.
Relationships, not encoded numbers, form the graph:

- `task.epic_id` points to one epic, or is null with a non-empty
  `standalone_reason`;
- `task.depends_on` lists direct task predecessors, typed as the
  [work lifecycle](#the-work-lifecycle) states;
- `specs` links follow the spec and epic rule there;
- a spec does not duplicate a parent, and an epic does not duplicate a task
  roster.

The [work lifecycle](#the-work-lifecycle) names the allocation verbs and the
planning routes; `cf-method`, "Managing a body of work", owns the landing
shape. The CLI refuses a missing target, task branch, tag, object ID, or
revision expression. Allocation creates files exclusively. Parallel planners
therefore serialize allocation or use one allocator; a collision is renumbered
before merge, never overwritten.

CodeFlow reads historical dual-identity and `TSK-NNN-NNN` records, including
the old nested epic/task layout. New records use the flat shape and one ID.
Migrate a historical record only when it is already being changed and no active
branch depends on its path; never create a cleanup campaign solely for visual
uniformity.

The flat layout is intentionally the default. If a repository eventually has
enough records that navigation measurably suffers, it may introduce documented
range directories without changing IDs or relationships. Do not pre-shard a
small workgraph.

`.git/codeflow/ledger` is local operational evidence, not shared planning
authority. A future local SQLite index may accelerate queries, but must remain
rebuildable from Git Markdown and must never become a second source of truth.

## The work lifecycle

This is the one statement of how a durable work item moves and which verbs
move it. The stage skills (`cf-plan`, `cf-develop`, `cf-ship`,
`cf-customize`) and the orchestrator's task graph follow it and do not restate
it.

```text
brief / discussion
        |
        v
discover facts locally and externally; findings -> docs/research/
        |
        +-- operator-owned outcome, scope, public behavior,
        |   authority, security, or irreversible choice unclear?
        |        `-- ask the smallest consequential question
        |
        v
cf-model-orchestrator: independent Claude + Codex discovery
        |
        v
one plan, drafted by Claude, challenged by Codex, approved once
        |
        v
cf-plan partitions any supplied batch, then materializes warranted records
        |
        +-- one outcome --------------------------> TSK-NNN (standalone):
        |                                            record + code in ONE PR
        +-- multi-session/PR/capability outcome --> EPC-NNN + TSK-NNN...:
        |                                            ONE planning PR, merged
        |                                            into the integration target
        `-- behavior/interface must be frozen ----> SPC-NNN, linked by consumer
        |
        v
codeflow work next -> codeflow work claim TSK-NNN
        |
        v
task/<TSK-NNN>-<slug> worktree -> codeflow work start TSK-NNN
        |
        v
implement -> verify -> merge the line in -> cross-lineage review ->
integrated judgment -> ship
        |
        v
task status complete as the PR's last commit, capabilities/ADRs synced,
batch candidate gated once, merge, prove landing, clean resources
```

`cf-model-orchestrator` owns independent discovery, reconciliation, design and
plan review, and approval. `cf-plan` owns planning discipline and
materialization: it gathers repository context, asks only necessary
operator-owned questions, chooses the lightest durable artifact set, and writes
the agreed epic/spec/task/ADR records. Invoking `cf-plan` directly for
non-trivial work routes through the orchestrator before materialization.

Agents do not ask the operator to rediscover facts available in the repository,
tools, or authoritative sources. They do ask when different reasonable choices
would change the accepted outcome, public behavior, authority, material
security posture, irreversible action, or another decision the operator owns.
State the evidence, viable options, consequences, and recommendation.

### Discovery, specs and the standalone test

- **Research.** Findings live under `docs/research/`, one file per question
  with its sources. A spec, ADR or epic that draws on them lists them in an
  `inputs` list. A record may cite a research file; a research file never
  carries status. A spike is a task with `work_type: spike` on a `spike/`
  branch: `work start` runs for it, and its pull request lands findings (files
  under `docs/research/` and its own record), never product code.
- **Spec and epic.** Spec and epic are many to many, with one owner per link.
  An epic lists the specs it consumes, in full or in part, in `specs`; a task
  lists its own and inherits its epic's. A spec keeps no consumer list: its
  consumers are read from their `specs` lists, so the two sides never
  disagree. An epic with no spec is valid; its own criteria carry acceptance.
- **Standalone.** A task may stand alone when its outcome is one reviewable
  pull request. It records a `standalone_reason` and may list capabilities
  and specs directly; its record and code land in that one PR. Anything
  larger is an epic with tasks. Challenge a standalone task that looks like
  one node of a larger outcome.

### Planning records

Records are allocated only by the CLI, and allocation creates files
exclusively. An epic and its tasks are allocated on a `plan/` branch of their
target; a standalone task's record is allocated on its own task branch:

- `codeflow epic new "<title>" --integration` allocates an epic and cuts and
  pushes its `integration/EPC-NNN-<slug>` branch from the protected target.
- `codeflow spec new --for EPC-NNN --for TSK-NNN "<title>"` allocates a spec
  and writes its id into every named consumer's `specs` list.
- `codeflow task new --epic EPC-NNN --into <target> "<title>"` allocates a
  task; `--standalone-reason "<why>"` replaces `--epic` for a standalone task,
  and `codeflow task new --follow-up-of TSK-NNN "<title>"` files a follow-up
  that inherits its epic and target. An epic task's follow-up is filed on a
  `plan/` branch and lands in the epic's batched amendment. A standalone task
  never uses a `plan/` branch, and the command refuses one; its follow-up is
  a standalone task too. Cut a task branch from the target, file the
  follow-up there, fill in and commit its record, then run
  `codeflow work claim <TSK-NNN>` (it reads the committed record). Claim
  renames that branch to `task/TSK-NNN-<slug>` and pushes it; it creates no
  worktree. The record lands with its work in that task's pull request.
- `codeflow adr new "<title>"` allocates a decision record.

The epic's one planning change is validated with `codeflow validate --docs`,
reviewed once, and merged into each task's declared `integration_target`
before implementation; it anchors every task of the epic, and an epic task
never authorizes its own record from its task branch. A standalone task's
record and code land in one independently reviewed PR, where CI admits that
one record and no other. Later follow-ups of epic tasks, re-sizing and other
tasks' criteria ride in one batched epic amendment on a `plan/` branch; a
task's own criteria amendment and status ride in its own PR. The target is a protected
release branch or a body-of-work `integration/` branch, never a task branch,
and it resolves to a real local or remote-tracking branch, never `HEAD`, a
tag, an object ID, or a revision expression.

### Dependencies

An entry of `depends_on` is either a bare task id, a code dependency, or
`{id: TSK-NNN, kind: research | decision, pin: "<commit sha>"}`, with the pin
quoted. The kind is never inferred from the predecessor's `work_type`.

- A **code dependency** is met when the predecessor is `complete` and its
  accepted change is in this task's execution base, by merge or an explicit
  reviewed port. A predecessor complete only on another line is not here yet,
  and an unfetched line is unknown, never met. Before that, a task may be
  claimed and started on the predecessor's exact reviewed head, named with
  `--on TSK-NNN@<sha>`: the tool checks the pin structurally (an ancestor of
  HEAD, on the predecessor's branch, still its tip), which is not
  authentication of the review. CI still requires the predecessor complete at
  the merge base when the task lands, so the stack lands in order.
- A **research or decision dependency** is met when the predecessor is
  `complete` at the pinned commit on its target. The planner writes the pin
  when it is known; an entry without a pin is unmet.
- **Guarded plans:** only the selected branch of a decision is written into
  `depends_on`. Until the selection is approved on the target, the join task
  carries `awaiting_selection: <plan or decision path>`, is `blocked` with the
  reason "awaiting selection" and that path as its revisit event, and may
  have an empty `depends_on`. The selection lands only by the batched epic
  amendment that removes `awaiting_selection`, writes the selected
  dependencies and unblocks the join. A cancelled, unselected alternative
  never blocks a join.

### Picking up and starting work

- `codeflow work next --epic EPC-NNN --json` lists ready tasks first, then
  waiting and blocked ones with their reasons, and names the fetched snapshot
  it judged; both flags are optional.
- `codeflow work claim TSK-NNN` checks readiness against the fetched target
  tip, creates `task/TSK-NNN-<slug>` from it (or from a named reviewed pin,
  `--on`) and pushes it; on a branch that already holds a new standalone
  record, it renames that branch instead. The pushed branch is an advisory
  claim, never a lock; two branches carrying one id show as a conflict in
  `codeflow status`.
- Before product edits, `codeflow work start TSK-NNN` runs on the task branch.
  It is a read-only preflight: it checks the task branch, the anchor (at the
  merge-base for an epic task, at head for a standalone task's own record),
  parent or standalone rationale, approved specs, and met dependencies.
  A task branch is not an integration target and cannot authorize its own
  epic planning record. The target must resolve to a real local or
  remote-tracking branch, never HEAD, a tag, object ID or revision expression.
  `codeflow work start`, `codeflow ci` and pre-commit enforce these same
  target and anchor checks.
  Whenever a task branch stages work, pre-commit validates the visible graph
  and applies the same check; CI repeats both, including in detached
  checkouts. None of them creates branches, worktrees, records, or status
  changes.

### Status and closeout

`codeflow task status` is the safe way to change a task's status, and the
change rides in the task's PR. It is not the only writer: a hand edit in a
reviewed pull request is judged by the same rules, and any transition this
table does not list is refused, whoever writes it:

| Transition | Command | Carries |
|---|---|---|
| todo to blocked | `codeflow task status TSK-NNN blocked --reason <why> --owner <who> --revisit <event>` | the Blocker section |
| blocked to todo | `codeflow task status TSK-NNN todo` | the blocker cleared |
| todo to complete | `codeflow task status TSK-NNN complete --acceptance <file>` | an acceptance block naming the reviewed code commit, as the task PR's last commit (`cf-ship`) |
| todo or blocked to cancelled | `codeflow task status TSK-NNN cancelled --reason <why> --scope <where it went>` | the reason and scope disposition in the Closeout |
| complete to todo (reopen) | `codeflow task status TSK-NNN todo --reason <why>` | the old acceptance block, kept and marked superseded |

`in_progress` stays readable on older records, but no verb writes it: a
visible task branch shows the work in progress. The acceptance block is the
completion record; each follow-up gets one real home, filed with
`--follow-up-of` in the batched epic amendment, or as its own standalone task
when it follows a standalone task.

A spec moves by `codeflow spec status SPC-NNN approved`, which needs no open
question, or `codeflow spec status SPC-NNN superseded --by SPC-NNN` when a new
revision replaces it. `implemented` is derived when every consumer is
complete; nobody writes it. An epic closes with `codeflow epic status EPC-NNN
complete --acceptance <file>` once every task is terminal and every criterion
is verified. A cancelled task never verifies a criterion: one that only
cancelled tasks serve, like one no task serves, is verified in the epic's own
acceptance block with its evidence. That block binds as a task's does: it
names the reviewed commit, after which only the epic's status and Closeout
change, and a waiver names the planning amendment of that criterion that
the reviewed commit contains. When it does not bind, correct the block and
have it reviewed: an open epic reruns `codeflow epic status EPC-NNN complete
--acceptance <file>`, and an epic the pull request already completes gets
the corrected block in its Closeout in that pull request, since an epic is
never reopened. `cancelled` and `archived` are its other
terminal acts. A
multi-task epic lands in gated batch candidates on its integration branch and
reaches the protected branch as one reviewed body (cf-method, "Managing a body
of work").

## Choose the lightest durable artifact

```text
Will the work or its rationale need to survive this session?
├─ no  -> native session task/plan; no durable record
└─ yes
   ├─ one reviewable pull request -> standalone task (the standalone test)
   └─ shared outcome spans tasks, sessions, PRs, or capabilities -> epic + tasks
      └─ behavior/interface/format must be agreed before building? -> add spec
```

An epic owns a coherent outcome and boundary, not a time box. A task owns one
independently reviewable acceptance boundary and normally one producer. A
standalone task is deliberate, not an orphan escape hatch; the standalone test
in the [work lifecycle](#the-work-lifecycle) decides it.

A spec pins behavior, interfaces, or formats that multiple implementation
choices must obey. It is created during planning, reaches `approved` only with
no unresolved open question, and is derived `implemented` once every consumer
is complete. While it is approved and not yet `implemented`, a change to it is
Plan vN+1, amended in place through a reviewed planning change: each change of
meaning gets a dated note naming its resolution (an editorial change needs
none), and the superseded decision stays visible as history. That change names
each consumer bound to a changed requirement and its disposition: unaffected,
criteria amended in the same change, or reopened. An `implemented` spec is
frozen, so a later change gets a new spec or an explicit superseding record; do
not rewrite history. Create an ADR only for a durable architectural decision.
Do not repeat the same prose at several altitudes.

Maintained requirements and executable interface schemas remain current in
their declared project-owned homes. An SPC is an optional frozen agreement for
one change, not a second living product manual or the only place to write
requirements. If an existing requirement or contract already settles the
change, link it and omit the SPC. When an SPC is needed, identify the source and
revision in its body and pin only the implementation delta; its `specs` link
from the consumer contains an SPC ID, not a URL or another method's spec ID.
The SPC template has no parsed `external_refs` field. On later change, update
the living authority and allocate new warranted work; do not edit the frozen
SPC to make history appear current.

## One authority per work item

The authority owns status, acceptance, and lifecycle for its work item. Other
systems keep an opaque link and only the context needed at their own altitude.
Distinguish a portfolio/product item from a repository-execution task: they can
be related without becoming two copies of the same task tree.

| Situation | Recommended authority |
|---|---|
| Repo-local, finite, gated work | CodeFlow Markdown in Git |
| Multi-team assignment, roadmap, support/SLA, or cross-repo program | The team's networked tracker |
| Existing spec/task method already owns the breakdown | That method; CodeFlow links to its settled outputs |
| High-volume issue queue | The project's chosen issue tracker |
| Current-session execution detail | Native harness task tools |

Epic/task template `external_refs` holds opaque links or IDs as preserved
metadata, not a parsed gate input and never mirrored status. An external system may
own portfolio state while CodeFlow owns repository execution boundaries;
record that split once. Do not require two status updates to call
one item complete.

Active full-tier or recognizable historical CodeFlow task tracking still
requires its distinct Git task records and planning anchors for implementation;
an external ticket, approved external spec, or `external_refs` link cannot
satisfy or waive `work start`, pre-commit, or CI. Make the smallest honest
CodeFlow execution record for the gated repository outcome, referring to the
external authority instead of copying its spec or task decomposition. Do not
import or paraphrase an equivalent authoritative tree. When durable CodeFlow
tracking is not active, retain the approved external method or
native/session plan at earned durability; do not fabricate CodeFlow workgraph
guarantees or silently upgrade the tier. Close or cancel each item at its
declared authority.

Linear, Jira, or another board may receive a **one-way projection** through the
project's issue-tracker MCP or CLI. For CodeFlow-owned execution tasks,
`codeflow status` is the repository completion signal. Missing or failed
remote updates never block `work start`, review, or ship. Never poll the remote
to decide whether a
CodeFlow task is done. Intake may start on the board; when active tracking
owns the repository execution, allocate its CodeFlow anchor and keep the
remote item as a link plus optional comment.

| Event in CodeFlow | Allowed remote action | Not allowed |
|---|---|---|
| Plan lands (`epic`/`task new`) | Create or attach the remote item; store its ID/URL in `external_refs` | Copy remote workflow state into `status:` |
| Task starts, blocks, completes, or cancels | Comment, or a projection field if the team wants a board view | Treat Linear/Jira `Done` as CodeFlow complete |
| Human merge / ship | Comment with the PR URL; optional remote close only if that item was intake | Dual-write two sources of truth |

On disagreement, the declared authority wins; repair the link, not a second
status mirror. `cf-customize` records which MCP/CLI to use and which events to
project.

Spec Kit, BMAD, or another planning method may supply a constitution, brief,
specification, plan, or task tree. Identify its authority, check intent/scope/
acceptance/interfaces/dependencies/risks for consistency, run the independent
duo reconciliation. Materialize only the CodeFlow execution records required
by active tracking or its assigned authority, not a copy of the external tree.
If durable tracking is active, create the distinct repository-execution anchor
the gates require; do not call the external work tree an anchor.

## Single surface and monorepos

Keep one repo-level namespace for a single application or for a monorepo whose
parts share an integration boundary. Plan around coherent outcomes and direct
prerequisites, not one epic per team or one task per folder. Tasks identify
affected areas, paths, and capabilities instead of recreating the source tree
under `project-management/`.

```text
EPC-014  account recovery outcome
├── SPC-009  recovery API and event contract
├── TSK-061  shared schema
├── TSK-062  backend              depends_on: [TSK-061]
├── TSK-063  web                  depends_on: [TSK-061]
├── TSK-064  iOS                  depends_on: [TSK-061]
├── TSK-065  Android              depends_on: [TSK-061]
└── TSK-066  infrastructure + whole-flow evidence
             depends_on: [TSK-062, TSK-063, TSK-064, TSK-065]
```

Stable area facts, commands, and local rules live in the nearest `AGENTS.md` or
area documentation. The root epic/task layer owns the cross-area outcome,
contracts, dependencies, and integration evidence. Use separate authorities
only for genuinely independently governed products with different releases,
ownership, and tracking systems; document the boundary at the root.

## What records carry

Shared engineering, security, testing, design, and review doctrine stays in
`AGENTS.md` and the relevant skills. Repeating it in every task creates stale
checklist theater. A record instantiates only what is specific:

- outcome, audience where relevant, scope, and non-goals;
- affected capabilities, surfaces, interfaces, and direct dependencies;
- testable acceptance criteria and selected evidence;
- producer and reviewer for non-trivial work;
- task-specific risk, recovery, test-data, or environment requirements, in
  the description;
- the acceptance block with evidence per criterion, and routed follow-ups.

Implementation discoveries follow one boundary:

```text
Does the discovery change outcome, scope, graph, owner, acceptance,
public interface, authority, or safety boundary?
├─ no  -> make the bounded implementation choice; preserve evidence
└─ yes -> stop; a new plan version or the batched epic amendment, as the
          task-graph mutation rules say; update records; continue
```

Completion never retroactively legitimizes a material deviation. The
acceptance block and PR body name what was and was not verified, the relevant
bounded deviations, and each follow-up's single real home. Omit empty
ceremony.

## Spikes and standalone prototypes

`spike/` and `experiment/` are branch prefixes, not a reason to land throwaway
code on a protected target. `work_type: spike | experiment` records intent.

| Situation | Home | Lands on protected? |
|---|---|---|
| Changing an existing tree | `spike/<task-or-question>` branch; edit the real packages | Findings only: the spike task's PR lands files under `docs/research/` and its record, never product code. Build the decision later on `task/TSK-…`. |
| Standalone throwaway (no production path yet) | `spikes/<task-or-question>/` **on that spike/experiment branch** | No. `spikes/` on a PR into `main` or `integration/` is a review defect unless the approved plan archives a named subset as evidence. |
| Retained comparison evidence | `docs/verification/<task>/` | Yes, as evidence, not as a product runtime. |

Do not add a mandatory `spikes/` directory on `main`. Do not host prototypes in
`cf-present` or `cf-docs-portal`. A static frame answers a composition question;
a prototype is for an interaction or logic question that paper cannot settle.

## Failure and recovery paths

- **Unclear operator-owned decision:** ask before materialization. A
  discoverable or reversible technical detail is researched and decided by the
  agents.
- **Duo seat unavailable:** record preflight evidence and reduced assurance,
  then use the documented solo flow; never imply approval by both seats.
- **Allocation collision:** stop, renumber on the planning branch, repair links,
  and revalidate.
- **Dangling link, cycle, orphan, draft spec, or incomplete predecessor:** fix
  the graph or sequence; do not bypass validation or `work start`.
- **Epic task created only on its implementation branch:** move it into the
  epic's batched amendment and merge that into the declared target before
  product changes. A standalone task's own record belongs on its branch.
- **Material discovery:** a new plan version under the task-graph rules; no
  after-the-fact waiver.
- **CodeFlow task cancellation:** stop product edits, preserve useful work and
  evidence, and cancel the task with its reason and scope disposition (the
  [work lifecycle](#the-work-lifecycle) command) through a reviewed change.
  Close or
  cancel other work at its declared authority. Never call cancellation
  complete or run the delivery path as though it shipped.
- **External tracker disagreement:** the declared authority wins; repair the
  link/context, not a second status mirror.
- **Already-running historical task:** preserve legacy readability and use the
  safest compatible target; do not rewrite active history merely to conform.
