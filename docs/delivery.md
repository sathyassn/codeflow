# How work moves to main

<!-- Guide layer. Reader: an operator who runs agents and wants to know what
     they do alone and what they merge, and a contributor landing a change.
     Sources: cf-method/references/delivery-process.md (TSK-184, sections 1,
     4 and 5), ADR-0076, SPC-013, docs/capabilities/CAP-010-duo-model-orchestration.md.
     The method reference carries the narrative for agents; this page is the
     one home for human readers. Other guide pages point here. -->

## Concept

**Work reaches `main` through one process: plan once, one pull request per
task, one holistic review, one full gate per batch, and one pull request into
`main` that the operator merges.**

Every piece of work attaches to a task before substantive work starts, an
existing one or a new one.

- The smallest unit is a standalone task, whose record and code land in the
  same reviewed pull request.
- A body of work starts from a brief or a spec and is broken down once into an
  epic and its tasks, in one planning pull request reviewed once by the other
  model lineage.
- Each task then goes the same way: one pull request that carries the whole
  change, one review by the other lineage, and a landing in a small batch with
  one full gate.
- When the epic is done, one pull request carries it into `main`, and only a
  person merges there.

This page is for the operator who runs agents and wants to know what to
approve and merge, and for a contributor landing a change. ADR-0076 retired
three things without lowering any quality floor:

- a plan for every task
- a review capped at a number of rounds
- a full gate for each pull request in a batch

A standalone pull request still runs its own gate as its own candidate. Where
durable tracking is inactive (the standard and minimal tiers), the same flow
runs with the harness's tracked unit in place of the task record.

## Architecture

**Four actors take a task through its stages, and the operator holds the
last boundary.**

From request to `main`, the work passes through the actors in this order:

```text
  operator   settles intent at the breakdown
      |
      v
  planning   one PR creates the epic and its tasks, reviewed once
      |
      v
  builder    one PR per task
      |
      v
  reviewer   one review of the whole change; a material finding is
      |      fixed in the same PR
      v
  primary    batch candidate, one full gate on that exact candidate
      |      green: the integration line moves
      |      red: diagnose first, drop a member only on evidence
      v
  primary    proves the epic once and raises one PR into main
      |
      v
  operator   reviews and merges it; agents never merge into main
```

The builder and reviewer steps repeat for each task, and the primary gates
each batch.

- **The builder**, an agent seat, takes the task from start to pull request.
  - It starts the task from the line tip or a predecessor's reviewed head.
  - It builds with the tests in the same change.
  - It merges the current integration line into the task branch, so conflicts
    surface in the task.
  - It opens one pull request that carries the whole task: code, tests, docs,
    the task record with its status and criteria, and the acceptance block.
  - It cites its targeted tests and the quick gate in the pull request.
- **The reviewer**, a seat of the other model lineage, reads the whole change
  once. A material finding is fixed in the same pull request and confirmed by
  its finder; a nit gets one disposition and never blocks. Review ends on
  evidence, never on a round count.
- **The primary** assembles reviewed heads into a small batch candidate in
  dependency order and inspects the resolved hunks and seams on product paths.
  - It runs the full gate once on that exact candidate.
  - The other lineage
    reviews the integration effects only when the primary hand-resolved a
    product hunk or two tasks touched one hotspot. Unit reviews are not
    repeated.
  - Green moves the integration line. Red is diagnosed first, and a
    member leaves the batch only when evidence attributes the failure to it.
  - When the last batch has landed, the primary proves the epic once on the
    integration line, closes it and raises the one pull request into `main`.
- **The operator** settles intent at the breakdown, decides any change of
  intent midway, and reviews and merges that one pull request into `main`.
  Agents never merge there.

- A standalone pull request is its own candidate. It lands on the integration
  line it targets, or, when it targets `main`, it is the pull request the
  operator merges.
- A later task may build on a predecessor before it lands, named with
  `--on TSK-NNN@<sha>`. The pin names the predecessor branch tip. Its Reviews
  section must approve that tip or an ancestor whose later commits change
  only the predecessor's own task record. Every intervening commit is
  checked, including merged commits and changes later reverted. The
  predecessor still lands first. Any other post-review change needs a new
  review, then a rebase and a recheck.
- Landing has priority: a new build starts only while no landing can proceed.

Only the operator adds process. A rule that adds a pull request, an approval,
a review pass or a record to every task needs the operator's explicit
approval, with what it protects and what it costs. Agreement between model
seats is never enough.

## Technical

**A task moves through four states, and every stage has one check that a verb
or CI runs.**

| Stage | What must hold | Checked by |
|---|---|---|
| Start | The task record exists and is anchored: in the epic's planning change, or, for a standalone task, at head on its own branch. Code predecessors are complete, or reviewed and pinned with `--on TSK-NNN@<sha>` | `codeflow work claim`, `codeflow work start`; CI applies the same read-only check once per pull request |
| Pull request | A `Task:` line names the task, or the epic for a planning-only range and for the pull request into `main`. A pull request that names neither is refused, whatever it touches | `codeflow ci` |
| Criteria | The task may change its own criteria, and CI prints the change for the reviewer. Every other record's criteria stay frozen | `codeflow ci` |
| Evidence | The builder cites targeted tests and the quick gate (`codeflow test --mode quick`) with revision and command. The pull request stays a draft until its evidence exists | The reviewer |
| Review | Every criterion not marked deferred has evidence on the reviewed revision, the needed checks are green, no material finding is open and every nit has a disposition | The other-lineage reviewer |
| Landing | Each completion is bound to its reviewed commit. A merge from the first-parent line of the target tip the run is judged against (CI's base, never a local branch), whose recorded result equals the conflict-free automatic merge of its parents, keeps the binding, so the builder can take a moved target without a new review; any other later change unbinds it. Predecessors are complete at the merge base | `codeflow ci`, `codeflow task status complete` |
| Batch gate | One full gate on the exact candidate, cited by run id and revision from its durable home under `~/.codeflow/gate-runs/`. A red gate is diagnosed before any member is dropped | The primary |
| Epic close | Every task terminal, every criterion verified, every consumed spec implemented or still consumed. The full gate runs every target (`codeflow test --all`) | `codeflow epic status`, `codeflow ci` |
| Release impact | Required only on a pull request into a protected branch or one that carries a breaking commit; elsewhere optional and checked when present | `codeflow ci` |

Nothing polls by default: the builder's pull request carries its cited
evidence, and the primary reads hosted results once at batch assembly. The
pull request cites the full gate by run id and revision, and the review
verdict lives on the pull request, so the evidence survives the cleanup of
worktrees and build directories.

### When something changes midway

| Situation | What happens |
|---|---|
| It changes intent, behaviour, an interface or a safety boundary | Both seats weigh it and the operator decides the intent; the change rides in one epic amendment on a `plan/` branch |
| It is inside the open task's outcome | Handled in the same open pull request: a finding is fixed and its finder confirms; a sharper criterion is printed by CI and approved by the reviewer |
| It affects a task already landed | That task is reopened and fixed in one pull request |
| It is a new or split outcome | One batched epic amendment with one other-lineage reviewer; nothing plans again per task |
| A predecessor built on changes after review | Rebase on its new reviewed head and recheck what it touches |
| The batch gate is red | Diagnose first. When evidence attributes the failure to a member, drop it and its dependents, fix in that task's pull request and regate the rest; a runner or environment defect is fixed at its owner and the same candidate regated |
| Repairs stop producing relevant evidence | Diagnose the stalled mechanism, the invalid assumption or the changed scope; split, redesign or take the intent question to the operator |
| Blocked by something outside the task | Mark it blocked; escalate an external dependency or an operator-owned choice with evidence, options and a recommendation |
| A reviewer seat is unavailable | Bounded recovery first; then recorded as reduced assurance naming the missing review, never faked or silently waived |
| A nit or preference in review | Recorded with the surface and the event that revisits it; it never blocks and never becomes a task |

### Related guides

| Topic | Guide |
|---|---|
| Installing CodeFlow, the hooks that check each commit and push, and the daily loop | [adoption](adoption.md) |
| The record rules in full: readiness, pull request classes, completion binding, release branches | [duo model orchestration](capabilities/CAP-010-duo-model-orchestration.md) |
| What each of the four enforcement planes catches | [enforcement planes](architecture/enforcement-planes.md) |
| An umbrella repository that holds several projects | [workspace mode](workspace-mode.md) |
| The narrative agents follow, with the same figures in fenced form | `cf-method/references/delivery-process.md` in the installed skills |
