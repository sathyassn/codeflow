# Shipped docs and artifacts: duty maps

Status: evidence for TSK-138 AC-1 to AC-4. The task reads every shipped
skill, reference, resource, rule file and template for duties stated twice
or stated away from the moment their reader needs them. One artifact
changed: the writing reference, which lacked five reply duties that its
owner states. Every other candidate is kept, with its reason, or left to
the task that owns the file under SPC-013 R-118; those are listed at the
end for planning.

Paths below are under `assets/base/`. `lifecycle` means
`claude/skills/cf-method/references/workflow-lifecycle.md`.

## Where an agent reads the reply duties

```text
agent about to report, at any tier
  rule map row "report status or summarize work"
    -> rules/writing.md            every tier; the reporting moment
  cf-method lifecycle reply rule   standard and full; the owner (R-117)
```

The minimal tier installs no skills, so the writing reference is the only
reply guidance there. At the standard and full tiers the rule map still
routes the reporting moment to it. Before this task it carried a compact
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
one.

## Candidates kept

| Candidate | Readers and moments | Decision |
|---|---|---|
| `awaiting_selection` rule in cf-method `project-organization.md` and orchestrator `task-graph.md` | a planner writing records; the orchestrator building the graph, which also needs the guard evidence and ledger detail | kept; TSK-108's `lifecycle_guidance_is_one_section_the_skills_follow` pins both homes |
| Figure proportionality in `cf-editorial-review` and orchestrator `quality/editorial.md` | an author revising prose; a reviewer grading a change | kept; each is the rule at its own moment, and CF-OUT-003 pins the author's copy |
| `utility-presentation-system.md` in `cf-present` and `cf-docs-portal` | two skills installed and read on their own | kept; each copy is tailored (Comment chrome, skins, portal limits) |
| PR template and cf-ship `pr-evidence.md` | the PR author filling a form; the author checking the rules | kept; a form and its reference |
| Root `README.md` and `docs/adoption.md` install steps | a first-time visitor; an adopter installing | kept |
| cf-plan, cf-develop, cf-ship "follow the work lifecycle" lines | each stage skill at a status change | kept; each is the pointer to the one lifecycle section |

## Left to the owning task

Each of these is a real second statement. SPC-013 R-118 gives the file to
another task, or moving the text would change requirement markers that
R-118 reserves; TSK-138 leaves SPC-013 unchanged, so none is edited here.

| Duplication | Owner and blocker | Proposed one home |
|---|---|---|
| Lifecycle reply rule and `rules/writing.md` state the same reply duties | R-117 names the lifecycle as owner; CF-OUT-001 to 004 and 007 pin the lifecycle text; eval markers are append-only (TSK-111's kit) | `rules/writing.md`, read at every tier at the reporting moment, with the lifecycle pointing to it; needs an R-117 amendment and the marker paths moved |
| cf-delegate `lane-lifecycle.md` Lifecycle section and `claude-turn-completion.md` restate the turn detection and stop-hook rules | cf-delegate line: TSK-144 is the open later editor; `delegate_doctrine_contract` and `reverse_lane_uses_hook_completion_not_pane_stability` pin both | the turn adapter, with the lane pointing to it, after TSK-144 lands |
| `docs/capabilities.md` restates the delegate adapter's byte rules | rows are per capability, updated by the task that ships it (R-118 docs row) | the adapter; the row names it |

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
