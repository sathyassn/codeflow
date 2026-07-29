# Durable project organization

Load this reference when choosing a work-item home, organizing a monorepo,
coexisting with an external planning method or tracker, or closing a task after
implementation discoveries. It is a default decision model, not a demand to
replace a project structure that already has one credible authority.

## The default shape

CodeFlow's full tier writes one flat, stable-ID namespace:

```text
project-management/
├── epics/
│   └── EPC-NNN.md
├── tasks/
│   └── TSK-NNN-MMM.md
├── specs/
│   └── SPC-NNN.md
└── templates/
    ├── epic.md
    ├── task.md
    └── spec.md
```

The canonical record paths are `epics/EPC-NNN.md`,
`tasks/TSK-NNN-MMM.md`, and `specs/SPC-NNN.md` under
`project-management/`.

- `EPC-NNN` identifies a multi-session, multi-PR, or multi-capability outcome.
- `TSK-NNN-MMM` identifies task `MMM` under epic `NNN`. The parent also appears
  as `epic_id`; `depends_on` carries settled direct predecessors.
- `SPC-NNN` normally matches its epic number. Specs are hand-authored only when
  an interface, format, or behavior must be pinned before building.
- The filename equals `format_id`. Keep the stable ID in links; the title may
  evolve without moving the file.

The CLI allocates the next number visible in its checkout and creates the file
exclusively. Independent worktrees do not share uncommitted allocations, so a
parallel plan assigns one work-item allocator or serializes allocation before
fan-out. A same-ID proposal must be renumbered; it is not silently overwritten.

CodeFlow still reads historical
`epics/EPC-NNN/EPC-NNN.md` and
`epics/EPC-NNN/tasks/TSK-NNN-MMM.md` records. It never writes that shape. Move a
legacy record to the flat path when it is otherwise touched and the move will
not disrupt an active branch; do not launch a repository-wide migration merely
for neatness.

## Choose the lightest durable artifact

```text
Does the work need to survive this session?
├─ no  → native session task/plan
└─ yes
   ├─ one bounded outcome → task
   └─ >1 PR/session or multiple capabilities → epic + bounded tasks
      └─ interface/format/behavior must be frozen first? → add one spec
```

An epic describes an outcome and its boundary, not a calendar container. A task
is independently reviewable work with one acceptance boundary and normally one
producer. Create an ADR only for a Tier-3 decision; do not use an epic, spec, or
ADR to repeat the same prose at another altitude.

## One authority per work item

The system that owns an item's status, acceptance, and lifecycle is its
authority. Other systems carry an opaque link and only the context needed at
their own altitude.

| Situation | Recommended authority |
|---|---|
| Solo or small-team, repo-local, finite gated work | CodeFlow markdown in Git |
| Multi-team assignment, portfolio/roadmap, support/SLA, or cross-repo program | The team's external tracker |
| Existing spec/task method already owns the work breakdown | That method; CodeFlow consumes its settled outputs |
| High-volume granular repo queue | The project's chosen issue queue |
| Current-session execution steps | Native harness task tools |

`external_refs` is a list of opaque links or IDs, not a status mirror. A
CodeFlow task may reference its external parent; an external item may reference
the CodeFlow PR or task. Completion should not require updating two copies of
the same status.

Do not make an untracked SQLite file or another host-local database the shared
team authority. A local database is appropriate as a rebuildable index/cache or
single-host runtime store. Team work needs a Git-visible authority or a
networked system designed for shared access, concurrency, permissions, backup,
and audit.

## Single surface and monorepo use

For a single application or library, keep one repo-level project-management
namespace. Tasks point to affected paths and capabilities rather than recreating
the source tree under `project-management/`.

For a monorepo, keep the same repo-level namespace when the parts ship through a
shared integration boundary:

```text
EPC-014: account recovery
├─ TSK-014-001  shared contract/schema
├─ TSK-014-002  backend
├─ TSK-014-003  web
├─ TSK-014-004  iOS
├─ TSK-014-005  Android
└─ TSK-014-006  infrastructure + whole-flow verification
```

Put stable area facts, commands, and local rules in the nearest `AGENTS.md` or
area documentation. Keep the cross-area outcome, interfaces, dependencies, and
integration evidence at the repo-level epic/task layer. This separates durable
product coordination from code-navigation detail.

Use separate authorities only when the repository truly contains independently
governed products with different release, ownership, and tracking systems.
Record their boundaries in the root architecture/operating contract; do not
create nested CodeFlow databases or duplicate a cross-product item in each
area.

## Planning-method coexistence

Spec-driven methods may produce a constitution, brief, specification, plan, or
task tree. Treat those as inputs according to the owning project's chosen
persistence model:

1. Identify which artifact owns requirements and work-item status.
2. Check cross-artifact consistency before execution: intent, scope,
   acceptance, interfaces, dependencies, risks, and unresolved questions.
3. Run CodeFlow's independent duo reconciliation over the settled inputs.
4. Materialize only missing CodeFlow execution records. Link to authoritative
   external artifacts; do not import or paraphrase an equivalent spec/task tree.
5. Use CodeFlow for repository execution boundaries—worktrees, producer/reviewer
   assignments, gates, evidence, integration, and PR closeout.

This supports methods such as Spec Kit or BMAD without making CodeFlow depend on
their commands, phase names, or files. If their artifacts are advisory rather
than authoritative, extract the accepted outcome into CodeFlow once and leave
the source as a reference.

## What work-item templates should carry

Shared engineering, security, testing, design, and review doctrine remains in
`AGENTS.md` and the relevant skills. Repeating it in every task creates stale
checklist theater. A work item records only its concrete instantiation:

- problem/outcome, audience where relevant, scope, and non-goals;
- affected capabilities, surfaces, interfaces, and dependencies;
- testable acceptance criteria and selected verification evidence;
- the approved Plan version and producer/reviewer contract for non-trivial work;
- task-specific risk/recovery requirements where material;
- closeout evidence, bounded discoveries, and routed follow-ups.

At closeout, distinguish two cases:

```text
Discovery during implementation
├─ stays inside approved outcome/scope/interfaces/ownership
│  └─ continue; record only review-relevant deviation + evidence at closeout
└─ changes a node, edge, guard, owner, acceptance/interface, or safety boundary
   └─ stop; settle and dual-approve Plan vN+1 before continuing
```

Closeout never retroactively legitimizes a material plan change. It names the
approved plan actually delivered, what was and was not verified, and any
follow-up's real tracked home. Omit empty or inapplicable sections rather than
filling them for ceremony.
