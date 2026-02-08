# Part 7: Work Stages

> Two categories of work stages, conditional routing, rework loops, and stage configuration per work type.

---

## Table of Contents

- [7.1 What Work Stages Are](#71-what-work-stages-are)
- [7.2 Code Work Stages: DEV, REVIEW, QA](#72-code-work-stages-dev-review-qa)
- [7.3 Non-Code Work Stages: WORK, REVIEW](#73-non-code-work-stages-work-review)
- [7.4 Stage Configuration per Work Type](#74-stage-configuration-per-work-type)
- [7.5 Optional Stage Selection](#75-optional-stage-selection)
- [7.6 Conditional Routing: Rework Loops](#76-conditional-routing-rework-loops)
- [7.7 Rework Limits for Autorun](#77-rework-limits-for-autorun)
- [7.8 DEPLOY Stage Removal](#78-deploy-stage-removal)
- [7.9 Stage Transitions in the Knowledge Layer](#79-stage-transitions-in-the-knowledge-layer)

---

## 7.1 What Work Stages Are

Work stages are the development pipeline steps that happen inside PF-4 (Work Execution). They define HOW work progresses from implementation through quality gates to completion.

Work stages are NOT PathFlow phases. They are a sub-structure within the PF-4 phase:

```
PathFlow Phases:     PF-1 -> PF-2 -> PF-3 -> [ PF-4 ] -> PF-5 -> PF-6 -> PF-7
                                                  |
Work Stages (PF-4):                    WS-DEV -> WS-REV -> WS-QA
```

**Two categories**:

| Category | Stages | Used For |
|----------|--------|----------|
| **Code** | WS-DEV -> WS-REV -> WS-QA | Feature, bug fix, refactor, test tasks |
| **Non-code** | WS-WORK -> WS-REV | Documentation, planning, research tasks |

---

## 7.2 Code Work Stages: DEV, REVIEW, QA

For code-producing tasks, the standard pipeline is:

```
WS-DEV (Development)
  |
  | cf-developer spawned on-demand
  | Implements the solution
  | Self-tests basic functionality
  | Messages cf-gitops to commit
  | Messages cf-knowledge-layer: "dev stage complete"
  |
  v
WS-REV (Review)
  |
  | cf-reviewer spawned on-demand
  | Reviews code for correctness, style, security
  | Documents findings
  | Verdict: APPROVED or CHANGES_REQUESTED
  |
  |---> If CHANGES_REQUESTED: route back to WS-DEV (rework loop)
  |
  v
WS-QA (Quality Assurance)
  |
  | cf-qa spawned on-demand
  | Writes tests (unit, integration as appropriate)
  | Runs tests, reports coverage
  | Verdict: PASS or FAIL
  |
  |---> If FAIL: route back to WS-DEV (rework loop)
  |
  v
(PF-4 complete -> proceed to PF-5)
```

**Key design decision**: WS-REV replaces the old PCV (Post-Completion Verification) from the `cf-stop-verify-work.sh` hook. Instead of a stop-time advisory check, review is now a first-class stage in the pipeline with a dedicated reviewer agent. See [Section 7.8](#78-deploy-stage-removal) and [08-enforcement-model.md](08-enforcement-model.md) for details.

**Stage responsibilities**:

| Stage | Agent | Inputs | Outputs |
|-------|-------|--------|---------|
| WS-DEV | cf-developer | Task requirements, acceptance criteria | Implemented code, committed via cf-gitops |
| WS-REV | cf-reviewer | Committed code, task context | Review verdict (approved/changes_requested), findings |
| WS-QA | cf-qa | Committed code, acceptance criteria | Test results (pass/fail), coverage report |

---

## 7.3 Non-Code Work Stages: WORK, REVIEW

For non-code tasks (documentation, planning, research), the pipeline is simpler:

```
WS-WORK (Primary Work)
  |
  | cf-documenter / cf-planner spawned on-demand
  | Performs the primary work (writing docs, creating plans)
  | Messages cf-knowledge-layer: "work stage complete"
  |
  v
WS-REV (Review)
  |
  | cf-reviewer spawned on-demand
  | Reviews output for accuracy, completeness
  | Verdict: APPROVED or CHANGES_REQUESTED
  |
  |---> If CHANGES_REQUESTED: route back to WS-WORK
  |
  v
(PF-4 complete -> proceed to PF-5)
```

**WS-WORK vs WS-DEV**: WS-WORK is a generic primary work stage. WS-DEV specifically implies code development. The distinction matters for:

- Which agent is spawned (cf-documenter vs cf-developer)
- What review criteria apply (accuracy vs code correctness)
- What the Knowledge Layer records in `stage_history`

---

## 7.4 Stage Configuration per Work Type

The lead determines which stages are needed at PF-3 (Work Classification), based on the WorkGraph task's `work_type`:

| Work Type | Stages | Role Agents | Notes |
|-----------|--------|-------------|-------|
| Feature (FEAT) | DEV -> REV -> QA | cf-developer, cf-reviewer, cf-qa | Full pipeline |
| Bug fix (FIX) | DEV -> QA | cf-developer, cf-qa | Review optional for small fixes |
| Hotfix (HTFX) | DEV | cf-developer | Minimal. Post-merge review flagged. |
| Refactor (RFCT) | DEV -> REV -> QA | cf-developer, cf-reviewer, cf-qa | Full pipeline (refactors need careful review) |
| Docs (DOCS) | WORK -> REV | cf-documenter, cf-reviewer | No QA for documentation |
| Test (TEST) | DEV | cf-qa (as developer) | QA teammate IS the developer for test tasks |
| Planning (SPKE) | WORK | cf-planner | Planning-only, no review needed |
| Research | WORK | lead (with explore sub-agents) | No review or QA |

**The lead has discretion**: These are defaults, not hard rules. The lead can add or skip stages based on task size, complexity, and risk. A trivial one-line bug fix might skip review. A complex planning task might add review.

---

## 7.5 Optional Stage Selection

At PF-3, the lead decides which stages to include. This decision is recorded in the WorkGraph via cf-knowledge-layer and determines which Claude Tasks are created inside PF-4.

**Decision factors**:

| Factor | Fewer Stages | More Stages |
|--------|-------------|-------------|
| Task size | XS, S | L, XL |
| Task risk | Low (docs, cosmetic) | High (auth, data, security) |
| Work type | Research, planning | Feature, refactor |
| User preference | Quick fix requested | Thorough review requested |
| Autorun mode | Depends on task config | Depends on task config |

**Recording the decision**: cf-knowledge-layer stores the selected stages in the WorkGraph task's metadata when work is registered at PF-3. This enables cross-session recovery: if the session crashes mid-pipeline, a resuming session can reconstruct the expected stage sequence.

---

## 7.6 Conditional Routing: Rework Loops

When a review or QA stage produces a negative verdict, the work routes back to the development stage for rework.

### Review Rework Loop

```
                    +---------------------------+
                    |                           |
                    v                           |
  WS-DEV -----> WS-REV -----> WS-QA       REWORK
    ^               |                       (new)
    |               |                         |
    |               +--- CHANGES_REQUESTED ---+
    |                                         |
    +------------- (new WS-DEV tasks) --------+
```

**Mechanics when cf-reviewer returns `changes_requested`**:

1. cf-reviewer messages cf-developer directly with findings
2. cf-reviewer messages cf-knowledge-layer: "Review complete, verdict: changes_requested"
3. cf-knowledge-layer updates WorkGraph: adds review entry to `stage_history`
4. cf-reviewer messages lead: "Review failed. Findings sent to developer."
5. Lead creates new Claude Tasks:
   - `#N+1` Rework: address review findings (assigned: cf-developer, blocked by review task)
   - `#N+2` Commit rework (assigned: cf-gitops, blocked by #N+1)
   - `#N+3` [WS-REV] Re-review (assigned: cf-reviewer, blocked by #N+2)

### QA Rework Loop

```
                    +---------------------------+
                    |                           |
                    v                           |
  WS-DEV -----> WS-REV -----> WS-QA       REWORK
    ^                            |          (new)
    |                            |            |
    |                            +--- FAIL ---+
    |                                         |
    +------------- (new WS-DEV tasks) --------+
```

**Mechanics when cf-qa returns `fail`**:

1. cf-qa messages cf-developer directly with failing test details
2. cf-qa messages cf-knowledge-layer: "QA complete, verdict: fail"
3. cf-knowledge-layer updates WorkGraph: adds QA entry to `stage_history`
4. cf-qa messages lead: "QA failed. Details sent to developer."
5. Lead creates new Claude Tasks:
   - `#N+1` Fix: address test failures (assigned: cf-developer)
   - `#N+2` Commit fix (assigned: cf-gitops, blocked by #N+1)
   - `#N+3` [WS-QA] Re-test (assigned: cf-qa, blocked by #N+2)
   - Optionally: `#N+4` [WS-REV] Re-review the fix (if the fix was substantial)

---

## 7.7 Rework Limits for Autorun

In autorun mode (no human in the loop), rework loops must be bounded to prevent infinite cycling.

**Limits**:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `max_rework_iterations` | 3 | Maximum DEV->REV->DEV cycles before escalation |
| `max_qa_retries` | 2 | Maximum DEV->QA->DEV cycles before escalation |
| `stage_timeout_minutes` | 30 | Maximum time for any single stage |

**Escalation behavior** (when limits reached):

1. Lead marks the WorkGraph task as `blocked` with reason "rework limit exceeded"
2. Lead records a JSONL event: `{ type: "rework_limit", task_id: "...", iterations: N }`
3. In autorun: skip to PF-7 (Session End) with task status `blocked`
4. In interactive: present findings to user for decision

**Stage timeouts** prevent indefinite hangs. If a stage exceeds its timeout:

1. Lead sends a shutdown request to the stuck agent
2. Lead records timeout event in WorkGraph
3. Proceeds to escalation behavior above

---

## 7.8 DEPLOY Stage Removal

**DEPLOY (WS-DEPLOY) is removed as a work stage.** PR creation is now a completion action in PF-6, not a development stage.

**Rationale**:

- Deployment (PR creation, remote sync) is a procedural operation handled by cf-gitops
- It does not involve variable-context work like DEV/REV/QA
- It does not need a separate role agent
- Treating it as a "stage" added unnecessary complexity to the stage pipeline
- cf-gitops already handles all git operations; making it a "stage" was redundant

**What happens instead**:

```
PF-4: Work Execution
  WS-DEV -> WS-REV -> WS-QA
  (stages complete)

PF-5: Work Verification
  Lead verifies acceptance criteria

PF-6: Work Completion
  cf-knowledge-layer marks task complete in WorkGraph
  cf-gitops creates PR                              <-- PR creation here, not as a stage
  cf-gitops syncs remote if needed
```

---

## 7.9 Stage Transitions in the Knowledge Layer

Every stage transition is recorded in the WorkGraph for auditability and cross-session recovery. See [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) for the full data model.

**Summary of what gets updated**:

| Event | tasks.stage | tasks.stage_status | active_work.current_stage | JSONL Event |
|-------|------------|-------------------|--------------------------|-------------|
| Enter DEV | `dev` | `in_progress` | `dev` | `stage_transition` |
| Complete DEV | `dev` | `complete` | `dev` | `stage_complete` |
| Enter REV | `review` | `in_progress` | `review` | `stage_transition` |
| REV approved | `review` | `complete` | `review` | `stage_complete` |
| REV changes_requested | `review` | `complete` | `review` | `stage_complete` (verdict recorded) |
| Re-enter DEV (rework) | `dev` | `in_progress` | `dev` | `stage_transition` (rework=true) |
| Enter QA | `qa` | `in_progress` | `qa` | `stage_transition` |
| QA pass | `qa` | `complete` | `qa` | `stage_complete` |
| QA fail | `qa` | `complete` | `qa` | `stage_complete` (verdict recorded) |
| Work complete | `done` | `complete` | `null` | `work_complete` |

---

## Related Documents

- [06-progressive-orchestration.md](06-progressive-orchestration.md) -- Phase progression that contains work stages
- [08-enforcement-model.md](08-enforcement-model.md) -- How stages are enforced via sentinels
- [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) -- Schema for stage tracking
- [11-use-cases.md](11-use-cases.md) -- Full walkthrough with stage routing
- [12-changes-and-claude-components.md](12-changes-and-claude-components.md) -- DEPLOY removal details
