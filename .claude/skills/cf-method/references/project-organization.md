# Durable project organization

Load this reference when deciding whether work needs durable records, breaking
an outcome into epics/specs/tasks, organizing a monorepo, coexisting with an
external tracker, or handling discoveries during implementation. It is an
opinionated default for durable projects, not a reason to replace an existing
credible authority.

## Authority and layout

Git-tracked Markdown is the shared workgraph authority:

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

## One authority per work item

The authority owns status, acceptance, and lifecycle. Other systems keep an
opaque link and only the context needed at their own altitude.

| Situation | Recommended authority |
|---|---|
| Repo-local, finite, gated work | CodeFlow Markdown in Git |
| Multi-team assignment, roadmap, support/SLA, or cross-repo program | The team's networked tracker |
| Existing spec/task method already owns the breakdown | That method; CodeFlow links to its settled outputs |
| High-volume issue queue | The project's chosen issue tracker |
| Current-session execution detail | Native harness task tools |

`external_refs` contains opaque links or IDs, never mirrored status. An external
system may own portfolio state while CodeFlow owns repository execution
boundaries; record that split once. Do not require two status updates to call
one item complete.

Spec Kit, BMAD, or another planning method may supply a constitution, brief,
specification, plan, or task tree. Identify its authority, check intent/scope/
acceptance/interfaces/dependencies/risks for consistency, run the independent
duo reconciliation, then materialize only missing CodeFlow execution records.
Do not import or paraphrase an equivalent authoritative tree.

## Single surface and monorepos

Keep one repo-level namespace for a single application or for a monorepo whose
parts share an integration boundary. Tasks identify affected areas, paths, and
capabilities instead of recreating the source tree under
`project-management/`.

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

Closeout never retroactively legitimizes a material deviation. It names the
approved plan delivered, the relevant bounded deviations, what was and was not
verified, and each follow-up's single real home. Omit empty ceremony.

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
- **Cancellation:** stop product edits, preserve useful work and evidence, and
  update the authoritative task to `cancelled` with the reason and resource
  disposition through a reviewed non-task planning/closeout change. Never call
  cancellation complete or run the delivery path as though it shipped.
- **External tracker disagreement:** the declared authority wins; repair the
  link/context, not a second status mirror.
- **Already-running historical task:** preserve legacy readability and use the
  safest compatible target; do not rewrite active history merely to conform.
