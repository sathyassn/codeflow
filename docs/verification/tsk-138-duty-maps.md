# Shipped docs and artifacts: duty maps

Status: evidence for TSK-138 AC-1 to AC-4. The task reads every shipped
skill, reference, resource, rule file and template for duties stated twice
or stated away from the moment their reader needs them. One artifact
changed: the writing reference, which lacked five reply duties that its
owner states. A duplicate that serves two read moments stays and carries a
parity guard; one that does not is consolidated by the task that owns the
file. Two guards are added here; one consolidation is filed as TSK-163.

Paths below are under `assets/base/`. `lifecycle` means
`claude/skills/cf-method/references/workflow-lifecycle.md`.

## Where an agent reads the reply duties

```text
agent about to report or explain, at any tier
  rule-map.toml:209-227, 270-288 (writing, present, report and explain rows)
    -> rules/writing.md              every tier; the reporting moment
agent about to build an accepted change, standard and full
  rule-map.toml:326-330 (build row)
    -> cf-method lifecycle           the reply rule's owner (R-117)
```

The minimal tier installs no skills, so the writing reference is the only
reply guidance there. At the standard and full tiers the rule map still
routes the reporting moment to it, and the lifecycle is read when a
change is built. Before this task it carried a compact
form that dropped five duties the owner states.

## Writing reference (`rules/writing.md`)

Reader: any agent, at every tier, about to write a reply, report, summary,
document, record, commit message or pull request body. Reached from the
rule map's "Outcomes first, in words" and "Show complex things" always
rules and its "report status" and "explain" rows.

Job: the one writing reference installed at every tier; the lifecycle
reply rule owns the reply duties (SPC-013 R-117) and this file states them
where the agent reads them before it reports.

| Duty | Home after TSK-138 | How |
|---|---|---|
| Outcomes first, in words; IDs follow | Replies and status | kept |
| Titles name the subject in words | Replies and status | kept |
| Report order: result, what changes it, what the reader must do | Replies and status | kept |
| Steps, gates, counts and tooling last, only where they explain | Replies and status | added from the owner |
| An order, not headings; a design discussion leads in prose | Replies and status | design clause added from the owner |
| Labels only in compared reports; forced labels are a defect | Replies and status | defect clause added from the owner (was missing) |
| Running report on long work opens with the result | Replies and status | added from the owner (was missing) |
| Summary anchors the reader; detail that does not orient comes after | Replies and status | ordering clause added from the owner |
| A summary that buries the anchor fails | Replies and status | added from the owner (was missing) |
| NEED YOUR ATTENTION once, after the opening; verbs; options and a recommendation | Replies and status | kept |
| Attention items are only the operator's, including a hard gate; other work keeps moving | Replies and status | added from the owner (was missing) |
| No heading when nothing is owed; a manufactured ask is a defect | Replies and status | defect clause added from the owner (was missing) |
| The heading never appears in a PR body, document, commit, outbound draft or payload | Replies and status | "outbound draft" added from the owner |
| A project may rename or drop the heading | Replies and status | kept |
| A simple answer stays simple | Replies and status; Copy guide, Replies | kept; the copy guide test pins its copy |
| Durations are agentic estimates | Replies and status, pointer to `workflow-discipline.md` | kept |
| Exact links, never guessed | Replies and status | kept |
| Figures by surface; `cf-present`; inline HTML; ASCII; never Mermaid | Figures by surface | kept |
| Shape the deliverable; editorial precedence | Shape the deliverable | kept |
| Written content policy: dashes, plain voice, no emoji or attribution | Written content policy | kept |
| Copy guide: ten sections with sourced examples | Copy guide | kept |

Nothing was removed. The owner, the lifecycle reply rule, is unchanged.
The new test `reply_duties_read_when_reporting_match_their_owner` pins 21
duties on both sides and fails, naming the duty, when either side drops
one. A pin whose clause ends the owner's sentence carries its full stop,
so a qualifier appended after it also fails; a negative control proves
it. The review record is the Closeout of TSK-138.

## Candidates kept

| Candidate | Readers and moments | Decision |
|---|---|---|
| Reply duties in the lifecycle and `rules/writing.md` | an agent reporting, at every tier; an agent building, at standard and full | kept with the parity guard this task adds; a single home would need an R-117 amendment and moving the CF-OUT-001 to 004 and 007 marker paths, for no reader gain |
| `awaiting_selection` rule in cf-method `project-organization.md` and orchestrator `task-graph.md` | a planner writing records; the orchestrator building the graph, which also needs the guard evidence and ledger detail | kept; TSK-108's `lifecycle_guidance_is_one_section_the_skills_follow` pins both homes |
| Figure proportionality in the lifecycle, orchestrator `quality/editorial.md` and `cf-editorial-review` | an author shaping a reply; a reviewer grading a change; an editor revising prose | kept with a parity guard this task adds; the three copies state the same six duties, each in its reader's voice (the author "never adds", the reviewer grades "findings", the editor calls it "a defect"), so `figure_duties_match_for_author_reviewer_and_editor` pins each file's own wording; CF-OUT-003 still pins the author's copy |
| `utility-presentation-system.md` in `cf-present` and `cf-docs-portal` | two skills installed and read on their own | kept; each copy is tailored (Comment chrome, skins, portal limits) |
| PR template and cf-ship `pr-evidence.md` | the PR author filling a form; the author checking the rules | kept; a form and its reference |
| Root `README.md` and `docs/adoption.md` install steps | a first-time visitor; an adopter installing | kept |
| `docs/capabilities.md` and the delegate adapter's byte rules | an adopter reading the catalog; an agent driving the lane | kept; the catalog states the contract at its altitude, not the mechanics, and each task updates its own rows at ship (R-118 docs row) |
| cf-plan, cf-develop, cf-ship "follow the work lifecycle" lines | each stage skill at a status change | pointers to the one lifecycle section, not duplicates |

## Consolidated by the owning task

| Duplication | Why it moves | Owner |
|---|---|---|
| cf-delegate `lane-lifecycle.md` Lifecycle section and `claude-turn-completion.md` | one reader (an agent on a non-Claude host) at successive moments of one task, and the copies drifted: the lane runs the sibling Stop-hook preflight "Before delivery", the adapter "Before launching"; separate pins at `delegate_doctrine_contract.rs:265` and `:272` lock both wordings | TSK-163, filed by planning PR 719: one home in the adapter, before launch, and one guard; after TSK-144 on the R-118 cf-delegate row |

## Measured totals

Measured as the record's baseline was: the byte sum of tracked `.md` files
under `assets/base/agents/skills/` (AC-2 reports the figure; it is not a
gate).

| Basis | Baseline `b2b14569c` | Task base `e199da079` | After TSK-138 |
|---|---|---|---|
| `agents/skills/**/*.md` | 413,152 bytes, 45 files | 427,765 bytes, 76 files | 427,765 bytes, 76 files |
| `claude/skills/**/*.md` | 91,577 bytes, 7 files | 101,402 bytes, 11 files | 101,402 bytes, 11 files |
| `rules/*.md` | not present | 34,258 bytes, 4 files | 34,930 bytes, 4 files |

The rise from the baseline to the task base came from the splits and
restorations of TSK-129, TSK-131 and TSK-150. TSK-138 adds 672 bytes to
`rules/writing.md` for the five duties the minimal tier lacked.
`codeflow doctor` reports every skill within its guideline, the kernel at
8,366 of 10,240 bytes and the per-task reading chain at 134,554 of 153,600
bytes, unchanged by this task.
