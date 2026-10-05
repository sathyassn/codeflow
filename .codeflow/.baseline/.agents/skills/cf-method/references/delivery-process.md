# Delivery process

This is the one narrative of how work moves from a request to `main`: plan
once when a brief or spec is broken down, one PR per task with its record,
one holistic review, one full gate per batch candidate, and continuous
landing. The rule map, the stage skills and the rule files point here instead
of restating the flow. Where durable tracking is not active (the standard and
minimal tiers), the same flow runs with the harness's tracked unit in place
of the task record.

Four rules hold everywhere:

- No work goes without a task record, however small. Every assignment
  attaches to an existing task or a new one before substantive work starts,
  research, planning and review with no repository edits included. Review,
  confirmation and repairs reuse the current task; a finding, a reviewer turn
  or a status reply never creates one. Only ordinary conversation, or a
  status update about work already recorded, goes unrecorded.
- Planning happens only when a brief or spec is broken into an epic and its
  tasks.
- Review is holistic: material findings are fixed in the open task, nits are
  recorded, and no round count or other hard number decides anything.
- Only the operator adds process (section 8).

## 1. The whole flow

```text
 request or idea
       |
       | attaches to a task, existing or new, before substantive work
       v
 +-------------+   one outcome    +------------------------------+
 | how much    |----------------->| standalone task              |
 | work?       |                  | record + code in one PR      |------+
 +-------------+                  +------------------------------+      |
       | a body of work                                                 |
       v                                                                |
 +-----------+   +---------------+   +--------------------------------+ |
 | spec      |-->| spec approved |-->| breakdown, once                | |
 | why, what |   | no open       |   | one planning PR: epic + every  | |
 +-----------+   | questions     |   | task + edges, one review       | |
       :         +---------------+   +--------------------------------+ |
       : consequential open question?       |                           |
       : investigate once, both seats       v                           |
                                     task  task  task  ...              |
                                       |    |     |                     |
                                       v    v     v                     v
                               +---------------------------------------------+
                               | per task: worktree, build, ONE PR, review   |
                               +---------------------------------------------+
                                                   |
                                                   v
                               +---------------------------------------------+
                               | per batch: candidate, integration review,   |
                               | ONE full gate, the integration line moves   |
                               +---------------------------------------------+
                                                   |
                                                   v
                               +---------------------------------------------+
                               | per epic: close once, one PR to main,       |
                               | the operator merges                         |
                               +---------------------------------------------+
```

The smallest unit of work is a standalone task: it has no epic, its record is
allocated on its own branch with `codeflow task new --standalone-reason`, and
its record and code land in the same reviewed PR. That PR is its own
candidate: it lands on the integration line it targets, or, when it targets
`main`, it is the PR the operator merges.

## 2. From a spec to an epic and tasks

```text
 brief or spec (problem, requirements, non-goals)
   |
   |  approved: no open questions; intent settled with the operator
   v
 breakdown (the only planning step)
   |
   |  one planning PR, reviewed once by the other model lineage
   |  (both seats where a consequential question is open:
   |   design, security, feasibility)
   v
 epic ---------------------------------------------+
   |  one coherent outcome; criteria describe it    |
   |  as a whole a reader can hold                  |
   |                                                |
   +--> task A --+                                  |
   |             | output feeds input               |
   +--> task B <-+  (the only reason for an edge)   |
   |                                                |
   +--> task C          hotspot notes: files two    |
   |                    tasks restructure, one      |
   |                    writer at a time            |
   +------------------------------------------------+
```

- The work starts from a brief or a spec; a spec is written when a contract
  consumers rely on must be pinned. It says why and what, and it is approved
  when it has no open questions. Before it ships, it is amended in place
  through reviewed work; after it ships, it is frozen and a new record
  carries changes.
- A consequential open question (design, security, feasibility) is
  investigated once, by both seats, before approval, and the answer is
  reused. Any other question takes one seat with cited evidence.
- The breakdown is one PR that creates the epic, every task with its
  criteria, and the edges. Nothing later plans again: follow-ups, re-sizing,
  reassignment and other tasks' criteria ride in one batched epic amendment
  on a `plan/` branch, reviewed by one other-lineage seat. One amendment may
  span several epics, naming each on its `Task:` line, and carry its docs
  and the project section of `AGENTS.md` (ADR-0078).
- An epic is one outcome. When its criteria describe two outcomes, it is two
  epics on one integration line.

## 3. What a task is

A task is one outcome a user or operator can observe and verify, worth its
own review and landing, delivered in one PR.

```text
 a piece of work
       |
       v
 <one outcome a user can see?> --no--> combine it with the outcome it serves
       | yes
       v
 <a reason to split?> --yes--> split, and write the reason in the task:
       |                         - value that can ship alone
       | no                      - a contract boundary a consumer needs pinned
       v                         - a decision of the operator gates part
 ONE TASK, ONE PR                - too big for one thorough review
 (typically 3 to 8 criteria)     - a risk boundary (security, data,
                                   irreversible action)

 never a reason to create or split a task:
   a branch, a worker, a file owner, a reviewer seat, a landing slot,
   a review finding
```

The numbers are orientation. Fewer criteria with real value is fine; a broad
safety change may need more. Past about a dozen, ask whether the task is two
outcomes; a count never splits a task by itself.

### What one task PR carries

```text
 +------------------------------ one task PR ------------------------------+
 |                                                                          |
 |  task record                               acceptance block              |
 |  +---------------------------+             +---------------------------+ |
 |  | outcome: one sentence     |             | reviewed commit, review   | |
 |  | C1  when X, it does Y  ---+--proved by->| E1  test, file:line, cmd  | |
 |  | C2  when refused, says why+--proved by->| E2  refused case, message | |
 |  | C3  existing project kept +--proved by->| E3  fixture before/after  | |
 |  | depends_on: output->input |             | not verified: stated      | |
 |  | builder, reviewer         |             | verdict                   | |
 |  +---------------------------+             +---------------------------+ |
 |                                                      ^                   |
 |     code          docs          tests ---------------+ cited as evidence |
 |                                                                          |
 |  status change and any change to its own criteria ride in the same PR   |
 +--------------------------------------------------------------------------+
```

When the task changes its own criteria, CI prints the change for the
reviewer; another task's criteria are never changed from this PR.

### Criteria

| A criterion is | Not a criterion (where it goes instead) |
|---|---|
| One observable behaviour at the surface a user or operator meets, "when X, the tool shall Y" | A function, file or argv form to write: an implementation step in the description |
| A real safety or compatibility guarantee, even when it names an exact refusal or preserved file | Each refused form, fixture or canary that proves a criterion: an evidence line under it |
| A journey criterion naming the path it runs end to end | Ordering, timing, "state it once", record duties: notes in the description |
| An after-release criterion with its owner and window, marked deferred | A claim backed only by the author's own narrative; the evidence is a test, a bounded observation or an independent review |

## 4. From a task to main

```text
 PER TASK
 worktree and work start (from the line tip, or a predecessor's reviewed head)
       |
       v
 build, with the tests in the same change
       |
       v
 merge the current line in; resolve conflicts in the task
       |
       v
 ONE PR, the whole task change --> review: one holistic pass, other lineage
                                          |
                                          v
                                 <material findings?> --no--> reviewed head;
                                          | yes                nits recorded
                                          v
                                 fix in the same PR; the finder confirms
                                 (a small fix with a failing probe: rerun
                                 that probe and the affected tests)
                                          |
                                          v
                                 <repairs producing relevant evidence?>
                                   | yes              | no
                                   v                  v
                                 back to review    diagnose the stalled mechanism,
                                                   the invalid assumption or the
                                                   changed scope; split, redesign
                                                   or ask the operator

 PER BATCH
 reviewed heads (small, in dependency order)
       |
       v
 batch candidate = line tip + reviewed heads
       |
       v
 integration review: the primary inspects resolved hunks and seams
 on product paths (the other lineage only for a hand-resolved
 product hunk or a hotspot two tasks touched)
       |
       v
 <full gate, once, on the exact candidate>
       | green                          | red
       v                                v
 integration line advances        diagnose first
       |                                |
       |                 +--------------+---------------+
       |                 v                              v
       |      evidence attributes it          shared runner or
       |      to a member: drop it and        environment defect:
       |      its dependents, fix in          fixed at its owner
       |      that task's PR                            |
       |                 |                              |
       |                 +-------> regate <-------------+
       v
 PER EPIC
 <epic done?> --no--> next tasks
       | yes
       v
 epic close: prove once, one PR to main; the operator merges
```

- Builders run targeted tests and the quick gate (`codeflow test --mode
  quick`) and cite that evidence, with revision and command, in the task PR.
  The primary runs the full gate once per batch candidate and links it. A
  standalone PR is its own candidate and runs its own gate.
- Before asking for review, the builder merges the current integration line
  into the task branch and resolves any conflicts there, so conflicts surface
  in the task, not in the batch gate.
- Batches stay small and in dependency order, so a red gate drops little.
  A red gate is diagnosed before anything is dropped: a member leaves the
  batch only when evidence attributes the failure to it.
- The primary always inspects the resolved hunks and integration seams on
  product paths before the gate. The other lineage reviews integration
  effects only when the primary hand-resolved a product hunk or two tasks
  touched one hotspot; unit reviews are not repeated.
- The PR cites the full gate by its run id and revision from its durable
  home, and the review verdict lives on the PR, so the evidence survives the
  cleanup of worktrees and build directories.
- Nothing polls by default. A builder's PR carries its cited evidence and the
  primary reads hosted results once at batch assembly. A bounded wait applies
  only where an adopted policy requires hosted checks green before landing.
- The PR stays a draft until its required evidence exists.
- Review ends when every criterion not marked deferred has evidence on the
  reviewed revision, the needed checks are green, no material finding is open
  and every nit has a disposition. No round count decides it.
- The finder confirms a material fix on the affected scope. A small fix whose
  finding came with a failing probe is confirmed by rerunning that probe and
  the affected tests, with no new model turn; a judgment-dependent or widened
  fix goes back to the finder. Nits need no confirmation.
- A later task may build on a predecessor's exact reviewed head before that
  predecessor lands, named with `--on TSK-A@<sha>`. The predecessor still
  lands first; a change to it after review means rebase and recheck.
- Landing has priority: a new build starts only while no landing can proceed.

### Task states

```text
             work start              acceptance block as the PR's last commit,
                                     landed on a green batch
   todo ---------------> in progress ------------------------------------> complete
                          |  ^    ^ \                                         |
         waits on the     |  |    |  \__ review, fix, confirm (inside the PR)  |
         operator or an   v  |    |                                           |
         outside party  blocked   +------ reopened: defect found after -------+
                        (unblocked)        landing, fixed in one PR
```

## 5. When something changes midway

```text
 something changes
        |
        v
 <changes intent, behaviour, interface, safety?> --yes--> both seats weigh it, the
        | no                                              operator decides intent;
        v                                                 one epic amendment
 <inside the open task's outcome?> --yes--> handle it in the same open PR:
        | no                                 - finding: fixed, finder confirms
        v                                    - sharper criterion: CI shows the
 <affects a task already landed?> --yes-->     change, reviewer approves it
        | no                        reopen that task, fix in one PR
        v
 a new or split outcome: one batched epic amendment, one other-lineage reviewer
```

| Other situation | What happens |
|---|---|
| A nit or preference in review | Recorded with the surface and the event that revisits it; never blocks, never another pass, never a task |
| A predecessor you built on changes after review | Rebase on its new reviewed head and recheck what it touches |
| The batch gate is red | Diagnose first. When evidence attributes the failure to a member, drop it and its dependents, fix in that task's PR and regate the rest; a shared runner or environment defect is fixed at its owner and the same candidate regated |
| Repairs stop producing relevant evidence | Diagnose the stalled mechanism, the invalid assumption or the materially changed scope; split, redesign or take the intent question to the operator |
| Blocked by something outside the task | Mark it blocked; escalate an external dependency or an operator-owned choice with evidence, options and a recommendation |
| An approved spec needs a change | Before it ships: amended in place, reviewed. After: frozen, a new record carries the change |
| One decision changes several epics | One planning amendment names each epic on its `Task:` line and carries its docs and the `AGENTS.md` project section; it lands on `main` once and each integration line merges `main` (ADR-0078) |
| A reviewer seat is unavailable | Bounded recovery first; then recorded as reduced assurance naming the review that is missing; never faked, never silently waived, never turned into a finding |
| Recorded nits at epic close | Folded into the next breakdown or dropped with a reason |

## 6. Review

What makes a finding material, and what is a nit, is defined once in the
workflow discipline rules, "Review verdicts" (`.codeflow/rules/`). Confidence
is stated apart from consequence, and effort to fix never lowers materiality.
The builder gives each nit one disposition in the PR body: fix now, track once
(one line in the epic's planning notes, or the task's follow-ups when there is
no epic, with the event that revisits it), or drop with the reason.

## 7. What the process produces

```text
 stage          produces                                    reviewed by
 -----------    -----------------------------------------   ----------------------
 shape          spec, when a contract needs pinning          operator settles intent
                epic record + task records + edges          other lineage, once
                (one planning PR)
 build/review   ONE PR per task: code, tests, docs,         other lineage, one pass
                task record with status and criteria,       (+ security reviewer when
                acceptance block, PR body with evidence      hooks, guards, policy, CI,
                                                             credentials or deps change)
 land           candidate SHA, full gate output cited by    primary; other lineage when
                run id, landing merge on the line,          a product hunk was
                cleanup with merge proof                     hand-resolved or two tasks
                                                             touched one hotspot
 close          epic acceptance block, one PR to main,      operator merges main
                cleanup with merge proof
```

No longer produced: a planning PR per task change, a separate completion PR,
a closeout narrative per task, a verification file per task, the long plan
form, per-task routing rows, a closeout report per task, a file-ownership
table, unrequested estimates, and a task per finding.

## 8. Who may add process

Only the operator adds process: a rule that adds a PR, an approval, a review
pass or a record to every task needs the operator's explicit approval, and
agreement between model seats is never enough. The full rule is in the
workflow discipline rules, "Only the operator adds process".