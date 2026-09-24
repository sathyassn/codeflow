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
- `task.depends_on` lists direct task predecessors;
- an epic or task lists the specs it consumes in `specs`; a task inherits its
  epic's specs;
- a spec does not duplicate a parent, and an epic does not duplicate a task
  roster.

Use `codeflow epic new`, `codeflow spec new --for EPC-NNN|TSK-NNN`, and
`codeflow task new --epic EPC-NNN` (or `--standalone-reason "..."`), with
`--into integration/<epic-id>-<slug>` for every task in a multi-task epic.
For a supplied task set or batch, first partition by coherent durable outcome
and direct dependencies; the request boundary is not automatically an epic or
integration boundary. A batch may therefore produce multiple epics, standalone
tasks, or both, each with its own appropriate landing route.
Create that one shared branch from the intended protected target before
allocating the tasks. The integration branch is the default for a multi-task
body; a different landing shape requires an explicit Plan vN rationale and
approval from both primary seats before allocation. Use it only for the epic's
coherent outcome, never to batch unrelated standalone tasks. The CLI refuses a
missing target, task branch, tag, object ID, or revision expression. Allocation
creates files exclusively. Parallel planners therefore serialize allocation or
use one allocator; a collision is renumbered before merge, never overwritten.

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

## From discussion to delivered work

```text
brief / discussion
        |
        v
discover facts locally and externally
        |
        +-- operator-owned outcome, scope, public behavior,
        |   authority, security, or irreversible choice unclear?
        |        `-- ask the smallest consequential question
        |
        v
cf-model-orchestrator: independent Claude + Codex discovery
        |
        v
settle and dual-approve Plan vN
        |
        v
cf-plan partitions any supplied batch, then materializes warranted records
        |
        +-- one bounded durable outcome ----------> TSK-NNN
        +-- multi-session/PR/capability outcome --> EPC-NNN + TSK-NNN...
        `-- behavior/interface must be frozen ----> SPC-NNN, linked by consumer
        |
        v
validate workgraph -> merge planning PR into each task's integration_target
        |
        v
task/<TSK-NNN>-<slug> worktree -> codeflow work start TSK-NNN
        |
        v
implement -> verify -> cross-lineage review -> integrated judgment -> ship
        |
        v
update statuses/capabilities/ADRs, merge PR, prove landing, clean resources
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

Planning records are created on a planning branch, validated, reviewed, and
merged into the task's declared `integration_target` before implementation.
That target is a protected release branch or a body-of-work `integration/`
branch, never the task branch itself; the anchored task record must declare the
same logical target. It must resolve to a real local or remote-tracking branch,
not `HEAD`, a tag, an object ID, or a revision expression.
`codeflow work start` is a read-only preflight: it checks the task branch,
merge-base anchor, parent/standalone rationale, approved specs, and completed
dependencies. Whenever a task branch stages work, pre-commit first validates
the visible graph and then applies the same anchor check; CI repeats both,
including in detached checkouts. Planning records belong on `plan/`, never on a
task branch that could authorize itself. These checks do not create branches,
worktrees, records, or status changes.

## Choose the lightest durable artifact

```text
Will the work or its rationale need to survive this session?
├─ no  -> native session task/plan; no durable record
└─ yes
   ├─ one independently reviewable outcome -> standalone task
   └─ shared outcome spans tasks, sessions, PRs, or capabilities -> epic + tasks
      └─ behavior/interface/format must be agreed before building? -> add spec
```

An epic owns a coherent outcome and boundary, not a time box. A task owns one
independently reviewable acceptance boundary and normally one producer. A
standalone task is deliberate, not an orphan escape hatch: record why no
durable parent outcome exists. During review, challenge a standalone task that
appears to be one node of a larger outcome.

A spec pins behavior, interfaces, or formats that multiple implementation
choices must obey. It is created during planning, reaches `approved` only with
no unresolved open question, and becomes `implemented` when its consuming work
ships. Later semantic change gets a new spec or an explicit superseding record;
do not rewrite history. Create an ADR only for a durable architectural decision.
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
- agreed Plan version and producer/reviewer assignment for non-trivial work;
- task-specific risk, recovery, test-data, or environment requirements;
- closeout evidence, bounded discoveries, and routed follow-ups.

Implementation discoveries follow one boundary:

```text
Does the discovery change outcome, scope, graph, owner, acceptance,
public interface, authority, or safety boundary?
├─ no  -> make the bounded implementation choice; preserve evidence
└─ yes -> stop; reconcile and dual-approve Plan vN+1; update records; continue
```

That approval follows `cf-model-orchestrator/resources/task-graph.md`,
"Mutation and settlement", including its recorded seat-loss exception.

Closeout never retroactively legitimizes a material deviation. It names the
approved plan delivered, the relevant bounded deviations, what was and was not
verified, and each follow-up's single real home. Omit empty ceremony.

## Spikes and standalone prototypes

`spike/` and `experiment/` are branch prefixes, not a reason to land throwaway
code on a protected target. `work_type: spike | experiment` records intent.

| Situation | Home | Lands on protected? |
|---|---|---|
| Changing an existing tree | `spike/<task-or-question>` branch; edit the real packages | No. Merge the decision later on `task/TSK-…`. Keep the spike branch as the primary source if needed. |
| Standalone throwaway (no production path yet) | `spikes/<task-or-question>/` **on that spike/experiment branch** | No. `spikes/` on a PR into `main` or `integration/` is a review defect unless Plan vN archives a named subset as evidence. |
| Retained comparison evidence | `docs/verification/<task>/` | Yes, as evidence, not as a product runtime. |

Do not add a mandatory `spikes/` directory on `main`. Do not host prototypes in
`cf-present` or `cf-docs-portal`. A static frame answers a composition question;
a prototype is for an interaction or logic question that paper cannot settle.

## Failure and recovery paths

- **Unclear operator-owned decision:** ask before materialization. A
  discoverable or reversible technical detail is researched and decided by the
  agents.
- **Duo seat unavailable:** record preflight evidence and reduced assurance,
  then use the documented solo flow; never imply dual approval.
- **Allocation collision:** stop, renumber on the planning branch, repair links,
  and revalidate.
- **Dangling link, cycle, orphan, draft spec, or incomplete predecessor:** fix
  the graph or sequence; do not bypass validation or `work start`.
- **Task created only on its implementation branch:** move it through a
  planning PR and merge it into the declared target before product changes.
- **Material discovery:** Plan vN+1; no after-the-fact closeout waiver.
- **CodeFlow task cancellation:** stop product edits, preserve useful work and
  evidence, and update its task to `cancelled` with the reason and resource
  disposition through a reviewed non-task planning/closeout change. Close or
  cancel other work at its declared authority. Never call cancellation
  complete or run the delivery path as though it shipped.
- **External tracker disagreement:** the declared authority wins; repair the
  link/context, not a second status mirror.
- **Already-running historical task:** preserve legacy readability and use the
  safest compatible target; do not rewrite active history merely to conform.
