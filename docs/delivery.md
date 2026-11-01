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
  dependency order and inspects each landing merge.
  - It runs the full gate once on that exact candidate.
  - A clean merge does not reopen unit review; a
    hand-resolved product hunk returns its task to a whole-unit review at its
    new head.
  - Green moves the integration line. Red is diagnosed first, and a
    member leaves the batch only when evidence attributes the failure to it.
  - When the last batch has landed, the primary proves the epic once on the
    integration line, closes it and raises the one pull request into `main`.
- **The operator** settles intent at the breakdown, decides any change of
  intent midway, and reviews and merges that one pull request into `main`.
  Agents never merge there.

- A standalone pull request is its own candidate. It lands on the integration
  line it targets, or, when it targets `main`, it is the pull request the
  operator merges. Its required checks count only when they ran on the
  current tip of `main`; under ADR-0081 the `main` ruleset requires an
  up-to-date branch, so a pull request that is behind is updated first. A
  clean update leaves a completed task's acceptance block standing, also
  when the pull request reopened its task; a hand-resolved update
  invalidates it (SPC-013 R-60, R-119).
- A task can be blocked, completed, reopened and completed again, and its
  epic closed after it, with ordinary commands that `task status`, the task
  pull request's `codeflow ci` and the line's pull request into `main` all
  accept:

  ```text
  task branch    review -> task status blocked -> task status todo
                 -> task status complete            (task PR, lands)
  fix branch     task status todo --reason -> fix -> review
                 -> task status complete            (fix PR, lands)
                 [a clean merge of the line after the review is allowed]
  planning PR    epic status complete               (closeout, lands)
  line -> main   codeflow ci accepts the whole sequence
  ```

  The Blocker that `task status` writes and removes is not reviewed scope.
  Each reopen appends the block it supersedes to one chronological
  archive, so the record reads the same against the line and against
  `main`. Archived blocks stay byte for byte (only a validated redaction
  rewords one), and the blocks the range archives are every completion
  its history made, in order, none missing (each completion is an
  occurrence, so an identical block completed again after a reopen
  counts twice), each with real provenance (a
  reviewed commit and waiver commits it contains). An archived
  completion is not judged again: reopening is how a completion that
  failed its binding is repaired. A reopening pull
  request takes the same clean target merge after its review as any task
  pull request, and the same per-commit walk: a post-review edit to any
  other path, even one reverted later, needs a new review. A waiver names
  an amendment the reviewed commit contains, on the target as in the
  branch, so no merge after the review, whichever way its parents run,
  brings one in. A completion's span ends at the completion, and the epic's Closeout rides its own
  planning pull request, since a completion commit changes only its own
  record. A reopen of a landed completion keeps its criteria, so its
  waiver names a planning commit on the target; a reopen of a completion
  that never landed is still the task's own pull request and keeps the
  in-branch waiver route, as does a task pull request on a branch whose
  name carries no task id.
- A later task may build on a predecessor before it lands, named with
  `--on TSK-NNN@<sha>`. The pin names the predecessor branch tip. Its Reviews
  section must approve that tip or an ancestor whose later commits change
  only the predecessor's own task record, and in it only its status and
  Closeout. Every intervening commit is checked, so a change later reverted
  still needs a new review, and a merge after the review is refused: the
  predecessor's own acceptance binding refuses a merge unless it is a clean
  re-merge of its target, and that re-merge changes more than the record,
  so this check refuses it too. The
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
| Pull request | A `Task:` line names the task (`Task: TSK-NNN`), or the epic for the breakdown PR and the PR into `main` (`Task: EPC-NNN`); a planning amendment names every epic it changes (`Task: EPC-001, EPC-002`). A pull request that names neither is refused, whatever it touches | `codeflow ci` |
| Criteria | The task may change its own criteria, and CI prints the change for the reviewer. Other records' criteria stay frozen except for completed standalone wording corrections (ADR-0080) and record-only redactions (ADR-0084). Both preserve every criterion and tag; see [acceptance waivers and redactions](#acceptance-waivers-and-redactions) | `codeflow ci` |
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

Where work stands is read, never stored. `codeflow status --epic EPC-NNN`
(or `--task`, `--line`, and `--json` for the versioned facts) derives each
task's stage, location, place in the order, lane, window and issues from
the records on each target's tip, the fetched refs, the forge and the
coordination ledger, with the readiness core that `work next` uses; see
[the CLI reference](cli.md#remember-notes).

### Completed work imported from another line

An ordinary merge can bring completed work from `main` or a verified epic
line. CI and pre-push bind an unchanged imported completion at its home,
so later receiving-line commits do not require that source task to reopen.
A task branch can carry the same imports while delivering its own work.
A receiver's first push still proves its own task landings.

- The guarded history reader checks the source line, record identity, exact
  contents, ancestry and home landing. Equal record text alone is not proof.
  With a remote configured, the home must be published at
  the selected fetch remote. The gate uses the configured upstream; a
  retired line can use the default target's fetch remote (normally
  `origin`). A local branch ahead of the remote-tracking ref supplies no
  authority.
  Local homes are usable only in a repository with no remotes. The local
  gate trusts local remote-tracking refs and remoteless clones; git-guard
  and hosted CI are the backstop.
- After a home line lands on the default target and is deleted, the exact
  record's first-parent landing on the published default target supplies
  the proof. The landing and its source span must still pass the guarded
  checks; a missing home without that landing remains unavailable.
- A source task that merged its home after review retains its binding only
  when that merge was clean and came from the home line. A task reopened
  before its first landing keeps the criteria accepted at that landing.
- Criteria amendments, reopenings and records brought unchanged from the
  target stay attributed to its history. A task PR does not claim those
  records as its own additions.
- Receiving-line record edits and competing criteria introductions still
  need their own judgment. A source landing with an unreviewed resolution,
  an unavailable home without a proven default-target landing, or an
  overlaid history supplies no import proof.
  The receiver's own completion still binds all its product work. A line
  merge that adds product changes outside verified source history and
  reviewed task heads refuses even when no task completes in that range.
  A line push after landing work whose completion is still pending waits
  until the completion record lands, even if the work was reviewed earlier.

When `work.import_authority` refuses, fetch the named home and its full
history, or the default target's history if the home has been retired.
Repair invalid source acceptance through that task's pull request on its
home, then merge the reviewed result. A refusal naming target history does
not ask the receiver to reopen an unrelated task. Pass the intended
`--branch` and `--base` when checking a detached or historical range; naming
the same base by ref or commit id does not change the proof.

### Acceptance waivers and redactions

A completed task can have personal or customer names removed from its
criteria wording and acceptance evidence without losing its results.
`codeflow task redact TSK-NNN --replacement <file> --review <reference>`
reads an edited copy outside the repository and writes the record with its
original SHA-256 digest, source commit and separate redaction review.

- The pull request changes only that task record, on a `plan/` branch from
  its integration target, and names the task on its `Task:` line.
- Status, criterion identifiers and tags, every result, `reviewed`, original
  review provenance and verdict remain unchanged. Superseded blocks can be
  redacted too, and unrelated commits already on the line do not prevent it.
- A reviewer confirms unchanged meaning. The original text remains in Git
  history; the command removes it from the current record only.

A waiver is different: `AC-N: waived | <full amendment commit SHA>` under
`acceptance.criteria` records a changed criterion result. The amendment
changes that criterion and planning records only, and the completion
contains it. A task may name an amendment on its target. Before its first
landing, its own record-only amendment may also be after the range's
merge-base and strictly before `reviewed` in ancestry, never equal to either
endpoint. A range that reopens a completion that never landed keeps that
own-range allowance, whatever caused the reopen; a range that reopens a
landed completion does not. A waiver refused as not on the target says what
that requires: a receiving line the run pins holds the commit (the pull
request's base in `codeflow ci`, `--base` in `task status`). The refusal also
names the route: amend the criterion through a planning pull request into
the target, merge the target into the branch before the review, and name
that planning commit.

The installed `cf-method/references/project-organization.md` section
"Acceptance waivers and redactions" owns the field definitions and workflow.
ADR-0084 records the redaction exception to the frozen-record rule.

### When something changes midway

| Situation | What happens |
|---|---|
| It changes intent, behaviour, an interface or a safety boundary | Both seats weigh it and the operator decides the intent; the change rides in one epic amendment on a `plan/` branch |
| It is inside the open task's outcome | Handled in the same open pull request: a finding is fixed and its finder confirms; a sharper criterion is printed by CI and approved by the reviewer |
| It affects a task already landed | That task is reopened and fixed in one pull request, unless it qualifies for a wording correction or record-only redaction |
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
