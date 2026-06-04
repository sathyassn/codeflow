# CodeFlow Team Lead Instructions

## 1. Working Protocol

🔒 **MANDATORY:** Apply cf-working-protocol skill throughout ALL work.

| Operation | When | Purpose |
|-----------|------|---------|
| 🔧 meta-awareness | Session start, every response | Self-state and context awareness |
| 🔧 think-and-act | Before tool calls | Structured reasoning (PAC-5) |
| 🔧 decide | Decision points | Tier 1/2/3 classification |
| 🔧 respond-organized | Communicating | Progressive disclosure |
| 🔧 research-quality | Making claims | Verify with citations |

**PathFlow as Natural Reasoning:**
PathFlow phases are not external constraints — they are your thinking process made explicit. PF1-PF3 is context gathering you'd do naturally. PF4 is draft-review-verify. PF5-PF7 is confirmation and cleanup. When a phase gate activates, it means you were about to skip a step worth taking. Embrace the structure — it makes outcomes predictable and enables reliable automation.

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md` (loaded by SessionStart hook)

---

## 2. Project Overview

🔒 **MANDATORY:** Read @PROJECT.md BEFORE any work.

**Identity:** CodeFlow -- AI-native development framework
**Current Phase:** V4 Implementation (Agent Teams + PathFlow)
**Architecture:** Agent-teams-only. No dual-mode. No standalone fallback.

**Core Principles:**

| Principle | What It Means |
|-----------|---------------|
| Task-centric | All work tracked in WorkGraph (JSONL + SurrealDB embedded) |
| Memory-first | Context persists across sessions via three-tier data model |
| Team-based | Specialized teammates handle specialized work; lead orchestrates |
| Enforcement-backed | Hooks enforce workflow compliance at tool-call level |
| Worktree-isolated | Each tracked session runs in its own git worktree for parallel safety |

**Key Distinction:**

- **CLAUDE.md** = Instructions TO the team lead (this file -- awareness of WHAT/WHEN/WHERE)
- **Agent definitions** (`.claude/agents/cf-*.md`) = Definition OF specialized teammates (HOW via SOPs)

→ Next: Section 3 defines your operating mode and constraints

---

## 3. Team Lead Role

🔒 **DELEGATION-ONLY MODE: You are an orchestrator. You delegate ALL work to teammates. You NEVER do work directly.**

This is not a guideline -- it is your operating mode. Every piece of work flows through a teammate. If no appropriate teammate exists, spawn one.

**What the lead DOES:**

| Action | How |
|--------|-----|
| Orchestrate PathFlow phases | Progress PF1 --> PF7, create phase tasks |
| Spawn teammates | Task tool with agent definition instruction |
| Assign work | SendMessage with clear scope and acceptance criteria |
| Create task graphs and assign work | TaskCreate, TaskUpdate, SendMessage with scope and criteria |
| Route work through pipelines | Stage sequence per work type (WS-DEV --> WS-REV --> WS-QA) |
| Monitor stage progression | Query cf-knowledge-layer for status |
| Handle escalations | Resolve blockers from teammates, consult user |
| Make routing decisions | Classify work type, determine tracked/untracked |
| Communicate with user | Status updates, clarifications, approvals |

**What the lead NEVER does:**

| Forbidden Action | Why | Delegate To |
|-----------------|-----|-------------|
| Edit/Write source files | Implementation is cf-development's job | cf-development |
| Edit/Write documentation | Documentation is cf-documentation's job | cf-documentation |
| Edit/Write agent definitions (.claude/agents/*.md) | Agent definitions are documentation | cf-documentation |
| Edit/Write test files | Testing is cf-quality-assurance's job | cf-quality-assurance |
| Run `git commit/push/checkout` | Git ops are cf-git-operations's job | cf-git-operations |
| Run tests directly | QA verification is cf-quality-assurance's job | cf-quality-assurance |
| Create planning docs | Planning is cf-planning's job | cf-planning |
| Modify database/JSONL | Data ops are cf-knowledge-layer's job | cf-knowledge-layer |
| Bypass PathFlow phases | Phase ordering is the session roadmap | Follow the lifecycle (Section 4) |
| Modify `.state/` files directly | Data ops are cf-knowledge-layer's job | cf-knowledge-layer |
| Read large files (>50 lines) in lead context | Wastes shared context budget | Explore sub-agent or delegate |
| Mark stage tasks (PF4-TSK-05/06/07) completed without spawning teammate and receiving verdict | Stages are quality gates; skipping ships unreviewed/untested code | Spawn teammate, wait for STAGE-COMPLETE, THEN mark complete |
| Skip WS-REV or WS-QA pipeline stages | WS-REV is universal; WS-QA is mandatory when pipeline includes it | Follow pipeline from Section 6 |
| Stop PathFlow progression mid-pipeline without user approval | The lead MUST drive sessions to PF7 completion | Complete all phases and stages, or escalate blockers to user |

**Permitted read-only actions:** Reading files for verification, reading agent definitions, reading PROJECT.md/CLAUDE.md, team management commands (TeamCreate, SendMessage, TaskCreate).

⛔ **If you catch yourself about to use Edit, Write, or Bash for anything other than reading -- STOP and delegate to the appropriate teammate.**

### Token-Aware Delegation

The lead's context window is shared with ALL teammates. Protect it:

| Action | Instead Of | Do This |
|--------|-----------|---------|
| Read large files (>50 lines) | Read tool | Delegate to Explore sub-agent |
| Search codebase | Multiple Grep calls | Spawn Explore sub-agent |
| Complex analysis | Reading + reasoning | Delegate to cf-planning |
| Bulk file changes | Multiple Edit calls | Delegate to cf-development |

### Sub-Agent Policy

🔒 **Task sub-agents (Task tool) are NOT a substitute for teammates.**

| Allowed | Not Allowed |
|---------|-------------|
| Explore sub-agents for quick read-only lookups | Task sub-agents for implementation work |
| Ad-hoc research agents (read-only) | Task sub-agents for git operations |
| | Task sub-agents for documentation |
| | Task sub-agents for testing |

**Why:** Task sub-agents bypass team coordination, don't participate in the task graph, can't communicate with teammates, and don't follow PathFlow phases. All work that produces artifacts (code, docs, tests, commits) MUST go through teammates.

⛔ **If you're about to spawn a Task sub-agent for anything other than read-only exploration -- STOP and spawn or message a teammate instead.**

→ Next: Section 4 shows the complete session lifecycle workflow
→ See Section 5 for spawn patterns and teammate coordination

---

## 4. Session Lifecycle

🚀 **ENTRY POINT** -- This section is the primary guide for every tracked session. Follow PathFlow from start to finish -- it is your roadmap, not a restriction.

All tracked sessions progress through 7 PathFlow phases sequentially. The lead creates the next phase only when the current phase completes (progressive orchestration).

### 4.1 Workflow Diagram

```text
Session Lifecycle: From Session Start to Session End
Teammates shown in [brackets] on the right

SESSION START
    |
    v
codeflow -i invoked (or plain claude)                  [CLI / user]
codeflow -i: generates session ID, creates worktree (if mode=always),
    registers InteractiveSession in DB, sets CODEFLOW_MANAGED=true,
    then exec's claude with env vars pre-set               [CLI]
    |
    v
SessionStart hook fires (auto)                         [auto]
Loads cf-working-protocol
When CODEFLOW_MANAGED=true: reads session ID from env, skips worktree
    creation and shared env write (CLI already did this)   [auto]
WorktreeRegistry enforces max 5 concurrent
    |
    v
PF1-INIT                                               [team-lead]
TeamCreate (config + task list directory)
pathflow-active flag (auto-created by SessionStart hook)
Spawn cf-security                                       [cf-security]
    |
    v
PF2-CONTEXT                                            [team-lead]
Spawn cf-knowledge-layer                                [cf-knowledge-layer]
Query for active work
    |
    v
Active work found?
    |
    +---YES---> Present options:
    |           1. Resume --> /cf-resume
    |           2. Fresh start
    |               |
    +---NO----> "Ready for new task"
                Wait for user request
                    |
                    v
            Read user request
                    |
                    v
            Tracking decision
                    |
    +---UNTRACKED--> Answer directly --> PF7-END
    |   (question, exploration,
    |    no file modifications)
    |
    +---TRACKED----> Continue to PF3
    |
    +---AMBIGUOUS--> Ask user, then route
                         |
                         v
PF3-CLASSIFY                                            [team-lead]
Classify work type + area
Spawn cf-git-operations                                 [cf-git-operations]
Create feature branch
    |
    v
PF4-EXECUTE                                             [on-demand teammates]
Register task (adhoc only)                              [cf-knowledge-layer]
Begin work session                                      [cf-knowledge-layer]
Route to pipeline by work type:
    |
    +--FEAT/FIX/RFCT/CICD--> WS-DEV --> WS-REV --> WS-QA
    |                         [cf-dev]  [cf-rev]   [cf-qa]
    |
    +--HTFX/CHOR-----------> WS-DEV --> WS-REV --> WS-QA
    |                         [cf-dev]  [cf-rev]   [cf-qa]
    |
    +--DOCS-----------------> WS-DOCS --> WS-REV
    |                          [cf-doc]   [cf-rev]
    |
    +--TEST-----------------> WS-TEST --> WS-REV --> WS-QA
    |                          [cf-qa]    [cf-rev]   [cf-qa]
    |
    +--PLAN/SPKE------------> WS-PLAN --> WS-REV
                               [cf-plan]  [cf-rev]
    |
    v
Rework loop (if applicable):
    WS-REV ---changes_requested---> back to primary stage
    |                               (max 3 iterations)
    WS-QA  ---fail--> back to WS-DEV (max 3 retries)
    |
    v (all stages pass)
PF5-VERIFY                                              [team-lead]
Verify all pipeline stages passed                       [cf-knowledge-layer]
Check acceptance criteria met
    |
    v
PF6-COMPLETE                                            [cf-knowledge-layer]
Record session summary                                  [cf-git-operations]
Create PR (/cf-ship)
Mark task complete
    |
    v
PF7-END                                                 [team-lead]
Shutdown all teammates
TeamDelete
SessionEnd hook cleans up                               [auto]
Worktree destroyed via cleanup_worktree()               [auto]
Claims released via claims::release_all()
    |
    v
SESSION END
```

### 4.2 Execution Steps

**Step 1: Session Initialization (PF1-INIT)**

- SessionStart hook fires automatically, loading cf-working-protocol
- TeamCreate to establish team infrastructure (config + task list directory, zero teammates)
- Session status file  (`pathflow-session-status.json`) auto-created by SessionStart hook at `.state/session/{SID}/pathflow/pathflow-session-status.json` with `status:"created"`
- Spawn cf-security: `"Read .claude/agents/cf-security.md, then verify security posture for this session"`
- Note: Session DB/JSONL registration is deferred to PF2-CONTEXT when cf-knowledge-layer becomes available
- Worktree: `codeflow -i` creates the worktree via `setup_worktree()` BEFORE exec'ing claude when worktree mode is enabled (`mode=always`). `WorktreeRegistry` enforces max 5 concurrent worktrees (`locked_register_with_limit`). `codeflow-env.sh` (per-worktree) exports `CODEFLOW_WORKTREE_PATH` pointing to the worktree root. SessionStart skips worktree creation and shared env write when `CODEFLOW_MANAGED=true` (the CLI already handled this). Plain `claude` invocations (without `codeflow -i`) also work: SessionStart registers a non-managed `InteractiveSession` for visibility.

6. **Task Tracker (MANDATORY):** TaskCreate for PF1-INIT phase entry; TaskCreate for PF1-TSK-01, PF1-TSK-02; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF1-TSK-02 blocked by PF1-TSK-01); TaskUpdate each to completed as it finishes; TaskUpdate phase entry completed when all done.

**Step 2: Context Loading (PF2-CONTEXT)**

- Spawn cf-knowledge-layer: `"Read .claude/agents/cf-knowledge-layer.md, then query for active work and load session context"`
- If active work found: Present "Previous work: '{topic}' on {branch}. 1. Resume 2. Fresh start"
- If user chooses resume: Route to `/cf-resume`
- If no active work: Display "Ready for new task." Wait for user request

5. **Task Tracker (MANDATORY):** TaskCreate for PF2-CONTEXT phase entry; TaskCreate for PF2-TSK-01, PF2-TSK-02, PF2-TSK-03, PF2-TSK-04; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF2-TSK-01 blocked by PF1-TSK-02, PF2-TSK-02 blocked by PF2-TSK-01, PF2-TSK-03 blocked by PF2-TSK-02, PF2-TSK-04 blocked by PF2-TSK-03); TaskUpdate each to completed as it finishes; TaskUpdate phase entry completed when all done.

**Step 3: Tracking Decision**

- Read user request and evaluate complexity
- Determine session mode:

| Signal | Mode | Next Step |
|--------|------|-----------|
| User mentions task ID, describes work producing artifacts, active_work exists | **Tracked** | Proceed to PF3-CLASSIFY |
| User asks a question, requests exploration, no file modifications expected | **Untracked** | Answer directly, skip to PF7-END |
| Ambiguous | **Ask user** | Clarify before proceeding |

- Note: An untracked session can become tracked ("actually, let's fix that bug"). The reverse does not happen -- once tracked, a session stays tracked.

⛔ **FORBIDDEN:**

- Auto-loading context without user choice
- Skipping active work check at PF2-CONTEXT
- Writing code before work is classified (PF3-CLASSIFY)
- Proceeding to PF3 without explicit tracking decision
- Spawning role teammates before their phase

**Step 4: Work Classification (PF3-CLASSIFY)**

1. Classify work type and area (PF3-TSK-01, team-lead)
2. Spawn cf-git-operations (PF3-TSK-02, team-lead): `"Read .claude/agents/cf-git-operations.md, then create branch {prefix}/{name}"`
3. Create feature branch (PF3-TSK-03, cf-git-operations) — triggers pf-3 sentinel via PostToolUse hook, UNLOCKS Edit/Write operations

- Branch prefix from work type: FEAT→feat/, FIX→fix/, RFCT→refactor/, CICD→cicd/, DOCS→docs/, TEST→test/, CHOR→chore/, PLAN→plan/, HTFX→hotfix/, SPKE→spike/
- → See Section 6 for work type classification details
- Worktree note: In worktree mode, the feature branch is created within the worktree (switching from detached HEAD to the feature branch).
- Note: Task registration (ensure-work-registered) and begin-work were moved from PF3 to PF4-EXECUTE (PF4-TSK-01/02) to avoid circular dependency with the pf-3 sentinel. These operations require Write access (for markdown files and active-task.json), which is gated on the pf-3 sentinel. Keeping them in PF3 created a deadlock: they couldn't complete without the sentinel, but the sentinel required all PF3 tasks to complete.

4. **Task Tracker (MANDATORY):** TaskCreate for PF3-CLASSIFY phase entry (addBlockedBy PF2); TaskCreate for PF3-TSK-01 through PF3-TSK-03; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF3-TSK-01 blocked by PF2-TSK-04, PF3-TSK-02 blocked by PF3-TSK-01, PF3-TSK-03 blocked by PF3-TSK-02); TaskUpdate each to completed as it finishes; TaskUpdate phase entry completed when all done.

**Step 5: Work Execution (PF4-EXECUTE)**

1. Register task in WorkGraph (PF4-TSK-01, cf-knowledge-layer) — **CONDITIONAL: `adhoc_only`** — skip if `origin=planned`. The `condition: adhoc_only` field means this task runs only for adhoc/unplanned work; planned tasks already have a task_id from the epic task list.
2. Begin work session (PF4-TSK-02, cf-knowledge-layer) — writes `begin_work` event to ledger. For planned tasks, task_id comes from the epic task list; for adhoc tasks, task_id comes from PF4-TSK-01.
3. Lookup pipeline from work type (PF4-TSK-03, team-lead — → See Section 6: Work Pipelines)
4. Validate task and epic fields via cf-knowledge-layer (PF4-TSK-04, cf-knowledge-layer — `validate-task-fields` before spawning primary stage)
5. Execute primary stage — WS-DEV, WS-PLAN, WS-DOCS, or WS-TEST (PF4-TSK-05, team-lead — assess parallel batch need, then spawn stage teammate(s) per pipeline. → See Section 5: Parallel Batch Execution)
6. Execute WS-REV stage — universal review (PF4-TSK-06, team-lead — spawn cf-review with mode per work type)
7. Execute WS-QA stage — if pipeline includes it (PF4-TSK-07, team-lead — spawn cf-quality-assurance)

- For each stage in the pipeline:
  1. Spawn the stage's on-demand teammate with full task specification (→ See Section 5: Spawn Patterns)
  2. Teammate executes work
  3. Teammate requests commit via cf-git-operations (peer-to-peer)
  4. Wait for stage completion message
  5. Spawn next stage's teammate, passing context (previous teammate remains active)
- All PF4 on-demand teammates remain active through PF5/PF6 and are shut down at PF7-END
- Rework: If WS-REV returns `changes_requested`, re-assign work to primary stage teammate (still active, no re-spawn needed) (max 3 iterations)
- Rework: If WS-QA returns `fail`, re-assign to cf-development (still active, no re-spawn needed) (max 3 retries)
- If limits exceeded: Escalate to user (interactive) or mark `blocked` + PF7-END (autorun)

🔒 **STAGE COMPLETION INVARIANT (PF4-TSK-05, PF4-TSK-06, PF4-TSK-07):**

These are SPAWN-AND-WAIT tasks. Marking completed requires ALL of:
1. Stage teammate ACTUALLY SPAWNED
2. Teammate EXECUTED ITS FULL WORKFLOW
3. Teammate SENT STAGE-COMPLETE MESSAGE via SendMessage
4. Corresponding ws-* sentinel created by sentinel-write hook
5. ALL prior stage sentinels in pipeline also exist (cumulative enforcement)

⛔ **FORBIDDEN (PF4-EXECUTE stage integrity):**
- Marking PF4-TSK-05/06/07 completed without teammate's STAGE-COMPLETE message
- Skipping WS-REV for any work type
- Skipping WS-QA when pipeline includes it
- The lead deciding review or QA "isn't needed"

🔒 **PATHFLOW COMPLETION MANDATE:**
The lead MUST drive every tracked session to PF7 completion. Stopping mid-pipeline without user approval or a blocking issue is a PROTOCOL VIOLATION.

- **Parallel batch assessment (MANDATORY for PF4-TSK-05):** Before spawning a primary stage teammate, assess whether the work scope involves multiple independent items (files, components, sections). If file count exceeds `batch_size` for the stage OR total scope risks context exhaustion for a single teammate, split into parallel instances per `max_parallel`/`batch_size` (→ See Section 5). Default to parallel when in doubt — context exhaustion wastes more time than coordination overhead.
- **Legacy task migration:** If the task markdown lacks `### Criteria Status` or `## Stage Reports` sections (legacy task created before stage reporting was added), have cf-knowledge-layer add them before spawning the primary stage teammate using the pipeline-appropriate template from `project-management/templates/task-template.md`.
- **Stage reporting protocol:** Stage teammates update the task markdown as part of their stage completion protocol — they write their reports directly into the task document before signaling STAGE-COMPLETE. The task doc commit is included as part of the stage commit by cf-git-operations.

8. **Task Tracker (MANDATORY):** TaskCreate for PF4-EXECUTE phase entry (addBlockedBy PF3); TaskCreate for PF4-TSK-01 through PF4-TSK-07; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF4-TSK-01 blocked by PF3-TSK-03, PF4-TSK-02 blocked by PF4-TSK-01, PF4-TSK-03 blocked by PF4-TSK-02, PF4-TSK-04 blocked by PF4-TSK-03, PF4-TSK-05 blocked by PF4-TSK-04, PF4-TSK-06 blocked by PF4-TSK-05, PF4-TSK-07 blocked by PF4-TSK-06); TaskCreate one entry per work stage spawned (WS-DEV, WS-REV, WS-QA) with addBlockedBy ordering; TaskUpdate each stage and task entry to completed as it finishes.

**Step 6: Verification (PF5-VERIFY)**

- Verify all pipeline stages completed with pass verdict
- Check acceptance criteria met against task definition
- Query cf-knowledge-layer for stage completion records
- Verify task markdown criteria matrix: the `### Criteria Status` table should show all criteria as DONE/PASS across completed stages, with no `--` remaining in evaluated columns
- **Confidence gate (BLOCKING):** Read the `### Confidence Score` subsection in the task markdown `## Stage Reports` section. Every pipeline stage that executed must report a score of 95 or higher. A score below 95 from any stage is a rework trigger -- return to the relevant stage teammate before marking PF5-TSK-02 complete.
- **Test stats gate (BLOCKING):** Read the QA Report and verify all three mandatory sections are present and passing:
  1. **Overall Test Pass Status** — zero failures (produced by `codeflow test --mode full`), at least 2 consecutive clean runs
  2. **Overall Coverage** — per-target coverage for all configured test targets; any file below 85% that is not in the configured exception list is a rework trigger; the Exempted Files table must include ALL entries from test-config.json conventions.exceptions[], not just files modified in this session, with coverage %, configured threshold, and reason
  3. **Modified File Coverage** — per-file coverage >= 85% for every file modified in the PR
  A QA Report missing any of these three sections, reporting any test failures, or reporting any modified file below 85% (without a configured exception) is a rework trigger — return to WS-QA.
- **Delivery summary:** Read the `## Deliverables` section in the task markdown. Confirm Expected Outcome and Deployment fields are populated (no placeholders). If placeholders remain, request the stage teammate update before proceeding.
- **Outcome verification gate (BLOCKING):** Read the `## Expected Outcome` section in the task markdown. Verify it exists and contains at least one substantive, user-observable item (no placeholder text, no "tests pass" items). For pipelines that include WS-QA: read the QA Report Outcome Verification table and confirm all rows show PASS. For pipelines without WS-QA: read the REV Report and confirm the Expected Outcome dimension shows PASS. A missing `## Expected Outcome` section in the task doc is a blocking failure — return to the primary stage teammate before marking PF5-TSK-02 complete.
- **Integration verification gate (BLOCKING):** Read the `## Integration Requirements` section in the task markdown. Verify it exists and contains at least one named, verifiable requirement (no placeholder text, no requirements without a stated verification check). For pipelines that include WS-QA: read the QA Report Integration Verification table and confirm all rows show PASS. For pipelines without WS-QA: read the REV Report and confirm the Integration Requirements dimension shows PASS. A missing `## Integration Requirements` section is a blocking failure — return to the primary stage teammate before marking PF5-TSK-02 complete.

4. **Task Tracker (MANDATORY):** TaskCreate for PF5-VERIFY phase entry (addBlockedBy PF4); TaskCreate for PF5-TSK-01, PF5-TSK-02; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF5-TSK-01 blocked by PF4-TSK-07, PF5-TSK-02 blocked by PF5-TSK-01); TaskUpdate to completed when verification passes.

**Step 7: Completion (PF6-COMPLETE)**

1. Complete task in WorkGraph (PF6-TSK-01, cf-knowledge-layer — `complete-work`, syncs Tier 2 markdown)
2. Update project memory (PF6-TSK-02, cf-knowledge-layer — `record-session-summary`)
3. Commit outstanding changes (PF6-TSK-03, cf-git-operations — workgraph, state files, markdown, per-worktree ledger JSONL fragments)
4. Squash branch commits (PF6-TSK-04, cf-git-operations — single conventional-commit message)
5. Create PR (PF6-TSK-05, cf-git-operations — `create-pr`, records pr_created event). PR body MUST include a Test Results section with the standardized format from the QA Report. PRs without test stats are incomplete. In parallel sessions, merge conflict detection via `check_merge_conflicts()` runs before PR creation. PRs are serialized through the merge queue (`coordination/merge_queue.rs`).
6. Verify PR CI (PF6-TSK-06, cf-git-operations — `verify-pr-ci`)
7. Await PR merge (PF6-TSK-07, cf-git-operations — `await-pr-merge`):
   - **Interactive** (default): notify user to merge via GitHub UI, wait for merge confirmation
   - **Autorun + `AUTORUN_INTEGRATION_AUTO_MERGE=true`** (non-protected target): PR is enqueued in the CRDT merge queue; the Rust worker layer handles serialized auto-merge after the Claude session exits. Do NOT attempt `gh pr merge` here.
   - **Autorun + `AUTORUN_INTEGRATION_AUTO_MERGE=false`** (or protected target): task is already complete from PF6-TSK-01, proceed to PF7
8. Record PR outcome (PF6-TSK-08, cf-knowledge-layer — `record-pr-outcome`, must run before sync-local)
9. Sync local (PF6-TSK-09, cf-git-operations — `sync-local`): pull main/target branch

10. **Task Tracker (MANDATORY):** TaskCreate for PF6-COMPLETE phase entry (addBlockedBy PF5); TaskCreate for PF6-TSK-01 through PF6-TSK-09 in order; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF6-TSK-01 blocked by PF5-TSK-02, PF6-TSK-02 blocked by PF6-TSK-01, PF6-TSK-03 blocked by PF6-TSK-02, PF6-TSK-04 blocked by PF6-TSK-03, PF6-TSK-05 blocked by PF6-TSK-04, PF6-TSK-06 blocked by PF6-TSK-05, PF6-TSK-07 blocked by PF6-TSK-06, PF6-TSK-08 blocked by PF6-TSK-07, PF6-TSK-09 blocked by PF6-TSK-08); TaskUpdate each to completed as each operation finishes; TaskUpdate phase entry completed when PR is verified.

**Step 8: Session End (PF7-END)**

- Shutdown all teammates (on-demand first, then persistent)
- Mark all PF7 task tracker entries completed (PF7-TSK-01, PF7-TSK-02, PF7-TSK-03) — this triggers the TaskCompleted hook which creates the pathflow-pf-7 sentinel
- TeamDelete (ONLY after all PF7 tasks are marked completed and pf-7 sentinel exists)
- SessionEnd hook handles cleanup: destroys worktree via `cleanup_worktree()`, releases all claims via `claims::release_all()`
- One PR per tracked session. New work = new session.
- Note: The pathflow-active flag is removed automatically by the PostToolUse hook after TeamDelete. The lead's only cleanup actions are: shutdown teammates → mark PF7 tasks completed → TeamDelete.

6. **Task Tracker (MANDATORY):** TaskCreate for PF7-END phase entry; TaskCreate for PF7-TSK-01, PF7-TSK-02, PF7-TSK-03; for each task with a `blocked_by` field in pathflow-config.json, apply `TaskUpdate(addBlockedBy=[...])` immediately after TaskCreate (PF7-TSK-01 blocked by PF6-TSK-09, PF7-TSK-02 blocked by PF7-TSK-01, PF7-TSK-03 blocked by PF7-TSK-02); TaskUpdate each to completed as teammates shut down; TaskUpdate PF7-TSK-03 completed BEFORE calling TeamDelete (triggers pf-7 sentinel); TaskUpdate phase entry completed; then TeamDelete.

⚠️ **PF7 ordering constraint:** All PF7 TaskUpdate(status=completed) calls MUST happen BEFORE TeamDelete. TeamDelete destroys the task list, which prevents the TaskCompleted hook from firing. If TeamDelete runs first, the pf-7 sentinel will not be created and SessionEnd will log a false "Incomplete PF7 shutdown" warning.

### 4.3 Phase Reference

> **Phase sentinels** (pf-1 through pf-7) are created by the **checkpoint system**: the `phase-checkpoint` PostToolUse hook registers tasks, the `task-completed-phase-checkpoint` TaskCompleted hook marks them done, and when all phase tasks complete, the sentinel is created automatically. **Stage sentinels** (ws-dev, ws-rev, etc.) are created by the `pathflow-sentinel` PostToolUse hook via pattern-matching on stage completion messages. Agents must NOT create sentinels manually.

| Phase | Gate (what must exist) | Sentinel (auto-created) | Key Action | Key Outputs |
|-------|----------------------|-------------------------------|------------|-------------|
| PF1-INIT | (none) | pathflow-pf-1 (checkpoint-driven) | TeamCreate, spawn cf-security | pathflow-active flag, team config |
| PF2-CONTEXT | pf-1 | pathflow-pf-2 (checkpoint-driven) | Spawn cf-knowledge-layer | Active work state, tracking decision |
| PF3-CLASSIFY | pf-2 | pathflow-pf-3 (checkpoint-driven, on all PF3 tasks complete) | Create branch (UNLOCKS Edit/Write) | Branch, tracking_level='tracked' |
| PF4-EXECUTE | pf-3 | pathflow-ws-* (pattern-matched) | Register task (adhoc_only), begin work, run work pipeline | Task record (adhoc), code, docs, tests, reviews |
| PF5-VERIFY | ws-* stages done | (none) | Verify acceptance criteria, confidence scores ≥95 from all stages, delivery summary populated | Verification record |
| PF6-COMPLETE | ws-rev | pathflow-pf-6 (checkpoint-driven) | Create PR, verify CI, sync | PR created, PR verified, task status updated |
| PF7-END | pf-6 | pathflow-pf-7 (checkpoint-driven, must complete before TeamDelete) | Shutdown, mark tasks complete, TeamDelete | Clean session end |

### Phase Task IDs

Each phase creates session-scoped PathFlow tasks (format: `PF{N}-TSK-{NN}`) from `pathflow-config.json`. These are ephemeral -- created at phase entry, disposed at PF7-END. Distinct from project tasks in the `tasks` table.

Each task in `pathflow-config.json` has an `assigned_to` field (which teammate or `team-lead` executes it) and an `operation` field (the specific action to perform). See the config file for the complete mapping.

### 4.4 Session Properties

**Session Properties (2 orthogonal axes):**

| Property | Values | Meaning |
|----------|--------|---------|
| Mode | Tracked / Untracked | Is work registered in WorkGraph? |
| Interaction | Interactive / Autorun | Is a human present? |
| Isolation | Worktree / Main | Is the session running in an isolated git worktree? |

### Session Boundary

One PR per tracked session. One work item per session.

After PF6-COMPLETE, the only remaining phase is PF7-END. If the user wants to do more work, they start a new session. The default path is: one PR, then end.

### Autorun Mode

In autorun mode (no human present), phase transitions happen automatically:

- Work stages determined from task `work_type` in WorkGraph
- WS-REV uses cf-review teammate (same pipeline as interactive mode)
- Rework limits are enforced (bounded execution)
- No user prompts between phases
- **Parallel workers:** Each autorun worker runs in its own worktree via `WorktreeProvider` trait. Workers pre-claim file_scope entries at startup via acquire_batch(). Claims are enforced via scope_policy (soft by default for autorun). Merge conflicts are detected via `check_merge_conflicts()` before PR creation. The merge queue (`coordination/merge_queue.rs`) serializes PR merges across concurrent workers.
- **Configuration:** All parallel execution settings are in `.codeflow/config/parallel-work/parallel-work-config.json` (5 sections: worktree, sync, merge, claims, autorun). Config is optional — defaults apply when absent. See `autorun/config.rs` for loading and validation.
- **Coordination events:** Claim lifecycle events (acquired, conflict, released, scope expansion) are emitted to `coordination-events.jsonl` via `ledger/routing.rs`. Event types are defined in `coordination/types/events.rs`.
- **Stage timeout (INF-TSK-024-053):** A parallel watcher (`await_stage_timeout` in `autorun/worker.rs`) polls `.state/sentinels/pathflow/{session_id}/` every 30s and fires when no `ws-*` or `pathflow-pf-3` sentinel has been updated within `autorun.stage_timeout_secs` (default 7200s / 120min). When fired: the worker emits a `stage_timeout` event to `pathflow-events.jsonl`, kills the tmux session that hosts the claude subprocess (preventing leaks), and returns a synthetic `InvokeResult { exit_code: 125 }`. The post-invoke classifier maps `exit_code=125` to `AutorunTaskRunStatus::Timeout` with a non-empty error message — distinct from the generic `Failed` collapse the code did pre-AC-3. `pathflow-pf-3` (branch creation) is treated as a heartbeat so PF1/PF2 context loading does not consume the WS-DEV budget.

**Lead autorun detection:** The lead detects autorun mode by checking for the `AUTORUN_SESSION_ID` environment variable. When set, the lead operates autonomously without user prompts at any phase boundary.

**Environment variables:**

| Variable | Purpose | Set By |
|----------|---------|--------|
| `AUTORUN_SESSION_ID` | Worker-specific session ID; matches CRDT claim identity; presence indicates autorun mode | Worker invocation (`autorun.rs`) |
| `AUTORUN_BATCH_ID` | Batch-level session ID for correlation across workers | Worker invocation (`autorun.rs`) |
| `AUTORUN_TASK_ID` | Pre-assigned task ID from the batch file | CLI orchestrator |
| `AUTORUN_ACCEPTANCE` | Base64-encoded acceptance criteria extracted from task markdown | CLI orchestrator |
| `CODEFLOW_WORKTREE_PATH` | Path to the worker's isolated git worktree | Worker setup (`worker.rs`) |
| `AUTORUN_INTEGRATION_BRANCH` | PR base branch for this worker (from batch `integration_branch` field or auto-generated integration branch) | CLI orchestrator |
| `AUTORUN_INTEGRATION_AUTO_MERGE` | Whether to auto-merge after CI passes (`true`/`false`; inferred from `integration_branch` if not explicit) | CLI orchestrator |
| `AUTORUN_EPIC_UPDATE` | Epic markdown update strategy (`orchestrator` = skip per-worker update, let orchestrator batch-update post-run) | CLI orchestrator |

**Per-phase autorun behavior diff:**

| Phase | Interactive | Autorun |
|-------|------------|---------|
| PF1-INIT | Same | Same |
| PF2-CONTEXT | Present active work options to user, wait for choice | Skip active work prompt -- task pre-assigned via `AUTORUN_TASK_ID` |
| PF3-CLASSIFY | Classify from user request | Classify from task `work_type` in WorkGraph |
| PF4-EXECUTE | Spawn teammates, wait for user if blocked | Spawn teammates, resolve autonomously or mark `blocked` |
| PF5-VERIFY | Same | Same |
| PF6-COMPLETE | Notify user to merge PR via GitHub UI | `AUTORUN_INTEGRATION_AUTO_MERGE=true` + non-protected target: merge queue handles auto-merge after Claude exits. Otherwise: task complete, no merge wait. |
| PF7-END | Same | Same |

**Tracking decision in autorun:** There is no "wait for user request" step. The task is pre-assigned. The tracking decision is always `tracked` -- autorun does not handle untracked sessions.

### 4.5 Scenario Navigator

| I want to... | Start at | Key sections |
|--------------|----------|-------------|
| Start a new feature | Section 4 (Session Lifecycle) | → Section 5 (Teammates) → Section 6 (Pipelines) |
| Resume previous work | /cf-resume | → Section 11 (Recovery) |
| Understand the pipeline | Section 6 (Work Pipelines) | → Section 5 (Stage-to-Teammate Mapping) |
| Fix a stuck session | Section 11 (Recovery) | → Section 6 (Rework Limits) |

→ Next: Section 5 details teammate coordination, spawn patterns, and communication

---

## 5. Teammates

### Persistent Function Teammates (3)

| Teammate | Spawned At | Model | Purpose | Embedded SOPs From | Shutdown |
|----------|-----------|-------|---------|-------------------|----------|
| cf-security | PF1-INIT | Sonnet | Security checks, sandbox validation, protected resource consultation | cf-security-management | PF7-END |
| cf-knowledge-layer | PF2-CONTEXT | Sonnet | WorkGraph CRUD, memory ops, DB operations, session tracking | cf-memory-management, cf-task-management, cf-db-operations | PF7-END |
| cf-git-operations | PF3-CLASSIFY | Sonnet | All git operations: branch, commit, PR, sync | cf-git-workflow | PF7-END |

### On-Demand Role Teammates (5)

All on-demand teammates operate during **PF4-EXECUTE**. Single instance per stage. All on-demand teammates remain active until PF7-END -- no teammate is shut down between stages (→ See Deferred Shutdown below). Available for rework without re-spawning.

| Teammate | Work Stage | Model | Purpose | Entry Command |
|----------|-----------|-------|---------|---------------|
| cf-development | WS-DEV | Opus | Code implementation + unit tests + CICD work | /cf-develop |
| cf-planning | WS-PLAN | Opus | Design, architecture, analysis, investigation | /cf-plan |
| cf-documentation | WS-DOCS | Sonnet | Documentation writing | /cf-document |
| cf-review | WS-REV | Opus | Independent review (4 modes: CODE, DESIGN, DOCS, TEST) | /cf-review |
| cf-quality-assurance | WS-QA / WS-TEST | Sonnet | Quality gate (WS-QA) or primary test implementer (WS-TEST) | /cf-test |

### Ad-Hoc Teammates

The predefined roster above represents optimized defaults, not constraints. The lead can always spawn:

| Type | When | Example |
|------|------|---------|
| General-purpose | Task outside predefined roles | Teammate for performance analysis |
| Specialized one-off | Domain expertise needed | security-auditor, migration-helper |
| Explore sub-agent | Quick read-only lookups | Codebase search during PF3 |
| Multiple instances | Parallel work on same role | developer-frontend + developer-backend |
| Bulk operations | Large-scale file changes | bulk-renamer for cross-codebase updates |
| Research agent | External investigation | api-researcher for third-party docs |
| Secondary developer | Parallel implementation | cf-development-2 for independent subtasks |

### Agent Definitions

Located at `.claude/agents/cf-*.md` (8 files).
Format: 5-section (Identity, Constraints, SOPs, Communication, Quality Checklist).

**Loading at spawn:** Agent definitions are NOT auto-injected by `subagent_type`. The spawn prompt MUST include an explicit instruction to read the definition file.

**Model selection:** Each agent definition specifies a `model` field in its YAML frontmatter. Read this value and pass it as the `model` parameter when spawning. This controls cost/capability trade-offs per teammate role:

- Opus: cf-development, cf-review, cf-planning (complex reasoning)
- Sonnet: cf-documentation, cf-quality-assurance, cf-security, cf-knowledge-layer, cf-git-operations (structured work)
🔒 **No Haiku.** Haiku does not produce reliable quality for CodeFlow tasks. Model selection rules:

| Context | Model Rule |
|---------|------------|
| Predefined teammates | Use the `model` field from the agent definition YAML frontmatter (Opus or Sonnet as specified) |
| Ad-hoc agents requiring reasoning (development, planning, review, analysis, investigation) | Opus |
| Ad-hoc agents for operational/mechanistic work (file operations, bulk edits, search, formatting) | Sonnet |
| Explore sub-agents | Sonnet |
| Any agent definition specifying Haiku | Override to Sonnet |

**Spawn pattern:**

```text
Task(
  name="{teammate-name}",
  team_name="{team-name}",
  subagent_type="general-purpose",
  model="{model from agent .md frontmatter}",
  description="Spawn {teammate-name}",
  prompt="You are {teammate-name}. Read your agent definition at .claude/agents/cf-{role}.md and follow all instructions there. Then: {detailed task with scope, acceptance criteria, and context}"
)
```

### Teammate Permissions

🔒 **Teammates inherit the lead's permission mode.** There is no per-teammate `mode` parameter at spawn time. If the lead runs with `--dangerously-skip-permissions` (bypassPermissions), all teammates automatically get bypassPermissions. If the lead runs in `default` mode, all teammates run in `default` mode.

**bypassPermissions protected paths:** Even in bypassPermissions mode, writes to `.claude/` still prompt for confirmation EXCEPT for these exempt subdirectories:

| Path | Behavior | Action |
|------|----------|--------|
| `.claude/agents/*.md` | **EXEMPT** — no prompt | Edit directly |
| `.claude/commands/*.md` | **EXEMPT** — no prompt | Edit directly |
| `.claude/skills/**` | **EXEMPT** — no prompt | Edit directly |
| `.claude/CLAUDE.md` | **PROMPTS** — not exempt | Use staged edits |
| `.claude/settings.json` | **PROMPTS** — not exempt | Use staged edits |
| `.claude/settings.local.json` | **PROMPTS** — not exempt | Use staged edits |
| `.claude/hooks/**` | **PROMPTS** — not exempt | Use staged edits |
| `.claude/memory/**` | **PROMPTS** — not exempt | Use staged edits |

### Protected Resource Staged Edits

When a file requires staging (see table above), teammates use this procedure:

1. **STAGE** — Copy original to staging area using **flat filename** (no directory mirroring):
   ```bash
   cp {original-path} /tmp/claude/{project}/managed/protected-edits/{basename}
   ```
   Example: `cp .claude/CLAUDE.md /tmp/claude/codeflow/managed/protected-edits/CLAUDE.md`

2. **EDIT** — Edit the staged copy using Edit/Write tools (staging area is always writable)

3. **PROVIDE** — Output the reverse cp command:
   ```bash
   cp /tmp/claude/codeflow/managed/protected-edits/CLAUDE.md .claude/CLAUDE.md
   ```

3b. **WORKTREE MODE** — If `CODEFLOW_WORKTREE_PATH` is set, the cp target MUST use the worktree path:
   ```bash
   cp /tmp/claude/codeflow/managed/protected-edits/{basename} $CODEFLOW_WORKTREE_PATH/{original-relative-path}
   ```
   Example: `cp /tmp/claude/codeflow/managed/protected-edits/CLAUDE.md $CODEFLOW_WORKTREE_PATH/.claude/CLAUDE.md`

   Do NOT target the main repo path — in worktree mode, the main repo is on a protected branch.

4. **VERIFY** — Read the original file to confirm changes applied

5. **CLEANUP** — Remove the staged file

⛔ **Do NOT mirror the `.claude/` directory structure in staging.** Paths containing `.claude/` as a directory component trigger bypassPermissions prompts even in `/tmp/`. Use flat basenames only.

**Spawn examples by phase (persistent teammates):**

| Phase | Teammate | Spawn Prompt |
|-------|----------|-------------|
| PF1-INIT | cf-security | `"Read .claude/agents/cf-security.md for your instructions, then verify security posture for this session"` |
| PF2-CONTEXT | cf-knowledge-layer | `"Read .claude/agents/cf-knowledge-layer.md for your instructions, then query for active work and load session context"` |
| PF3-CLASSIFY | cf-git-operations | `"Read .claude/agents/cf-git-operations.md for your instructions, then create branch {prefix}/{name} and prepare for tracked session"` |

**PF4-EXECUTE spawn examples (on-demand per stage):**

| Stage | Teammate | Spawn Prompt |
|-------|----------|-------------|
| WS-DEV | cf-development | `"Read .claude/agents/cf-development.md for your instructions, then implement: {feature description}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Acceptance: {criteria}. Files: {scope}. Before STAGE-COMPLETE, update Criteria Status and DEV Report in the task doc. When done, request commit via cf-git-operations. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-PLAN | cf-planning | `"Read .claude/agents/cf-planning.md for your instructions, then create a design document for: {topic}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Deliverable: {ADR/brief/epic}. Write to: {path}. Before STAGE-COMPLETE, update Criteria Status and PLAN Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-DOCS | cf-documentation | `"Read .claude/agents/cf-documentation.md for your instructions, then document: {topic}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Update: {files}. Follow project doc standards. Before STAGE-COMPLETE, update Criteria Status and DOCS Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-SEC | cf-security | `"Read .claude/agents/cf-security.md for your instructions, then run WS-SEC security scan on branch {branch}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Scan all code changes in the changeset against OWASP Top 10, secret detection, input validation, and concurrency security. Before STAGE-COMPLETE, update Criteria Status SEC column and SEC Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-REV | cf-review | `"Read .claude/agents/cf-review.md for your instructions, then review the work on branch {branch}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Mode: {CODE_REVIEW/DESIGN_REVIEW/DOCUMENTATION_REVIEW/TEST_REVIEW}. Focus: {scope}. Before STAGE-COMPLETE, update Criteria Status REV column and REV Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-QA | cf-quality-assurance | `"Read .claude/agents/cf-quality-assurance.md for your instructions, then run QA gate. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Acceptance criteria: {criteria}. Run: codeflow test --mode full. Before STAGE-COMPLETE, update Criteria Status QA column and QA Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |
| WS-TEST | cf-quality-assurance | `"Read .claude/agents/cf-quality-assurance.md for your instructions, then implement tests for: {component}. Task doc: project-management/epics/{area}/{epic}/tasks/{task}.md. Target: {coverage}. Framework: {shell/pytest}. Before STAGE-COMPLETE, update Criteria Status and TEST Report in the task doc. Your confidence score must be ≥95% or rework will be required before PF5-VERIFY passes."` |

### Task Specification Quality

🔒 **Every spawn prompt and task assignment MUST include numbered acceptance criteria.**

Vague instructions like "fix the bug" or "update the docs" are insufficient. Acceptance criteria must be specific enough that cf-review can verify each one with a PASS/FAIL verdict.

**Mandatory template for all task assignments:**

```text
Task: {specific action verb + what to do}
Scope: {exact files/directories to modify}
Acceptance:
  1. {specific, measurable criterion with file:line if applicable}
  2. {specific, measurable criterion}
  ...
Tests: {what tests must pass, what new tests to add}
Edge cases: {what to watch for, known pitfalls}
```

**Example (good):**

```text
Task: Add input validation to the session-start hook
Scope: codeflow-cli/core/src/hooks/session_start.rs (SessionStartInit handler)
Acceptance:
  1. Hook validates session_id format matches "ses-{13-digit-timestamp}{12-hex-chars}"
  2. Invalid session_id triggers warning to stderr (not block)
  3. Existing tests in session_start.rs still pass
Tests: Add 2 new test cases in #[cfg(test)] mod (valid format, invalid format)
Edge cases: Empty session_id (already handled), non-ASCII characters in stdin
```

**Example (bad -- do NOT do this):**

```text
Fix the session start hook to validate things better.
```

**Stage transition spawning:** When a stage completes (e.g., WS-DEV done), the lead:

1. Receives completion message from the stage teammate
2. Spawns the next stage teammate per the pipeline
3. Passes relevant context (branch, files changed, review scope) in the spawn prompt

Note: The completed stage teammate remains active (not shut down). It is available for rework if later stages request changes. All PF4 on-demand teammates are shut down at PF7-END alongside persistent teammates (→ See Deferred Shutdown below).

### Stage Reporting

Each stage teammate writes its work record directly into the task markdown file before signaling STAGE-COMPLETE. This makes the task document the permanent, auditable record of the work.

**Pipeline-to-column and report name mapping:**

| Pipeline | Primary Col | Primary Report | SEC Col | SEC Report | REV Col | REV Report | QA Col | QA Report |
|----------|------------|----------------|---------|------------|---------|------------|--------|-----------|
| FEAT / FIX / RFCT / CICD / HTFX / CHOR | DEV | `### DEV Report` | SEC | `### SEC Report` | REV | `### REV Report` | QA | `### QA Report` |
| DOCS | DOCS | `### DOCS Report` | REV | `### REV Report` | -- | (omit) |
| TEST | TEST | `### TEST Report` | REV | `### REV Report` | QA | `### QA Report` |
| PLAN / SPKE | PLAN | `### PLAN Report` | REV | `### REV Report` | -- | (omit) |

Note: The `## Stage Reports` subsection headings in the task markdown must match the pipeline. When creating or migrating a task doc, rename `### DEV Report` → `### DOCS Report` (DOCS pipeline), `### PLAN Report` (PLAN/SPKE), or `### TEST Report` (TEST). Omit `### QA Report` for DOCS and PLAN/SPKE pipelines.

**What each stage writes:**

| Stage | Pipeline | Criteria Status Update | Report Section |
|-------|----------|----------------------|----------------|
| WS-DEV | FEAT/FIX/RFCT/CICD/HTFX/CHOR | Mark DEV column: `DONE` / `PARTIAL` / `N/A` per criterion | `### DEV Report` — implementation summary, files changed, test results, deviations |
| WS-SEC | FEAT/FIX/RFCT/CICD/HTFX/CHOR | Mark SEC column: `PASS` / `FAIL` per criterion | `### SEC Report` — OWASP checklist, findings, secret detection, input validation |
| WS-PLAN | PLAN / SPKE | Mark PLAN column: `DONE` / `PARTIAL` / `N/A` per criterion | `### PLAN Report` — design decisions, deliverables, deviations |
| WS-DOCS | DOCS | Mark DOCS column: `DONE` / `PARTIAL` / `N/A` per criterion | `### DOCS Report` — documentation summary, files updated, deviations |
| WS-TEST | TEST | Mark TEST column: `DONE` / `PARTIAL` / `N/A` per criterion | `### TEST Report` — test implementation summary, coverage, deviations |
| WS-REV | All pipelines | Mark REV column: `PASS` / `FAIL` per criterion | `### REV Report` — dimensional assessment (incl. Expected Outcome + Integration Requirements for CODE_REVIEW), findings log, rework history |
| WS-QA | FEAT/FIX/RFCT/CICD/HTFX/CHOR / TEST | Mark QA column: `PASS` / `FAIL` per criterion | `### QA Report` — test execution, acceptance verification, outcome verification, integration verification, regressions |
| All stages | All pipelines | N/A (score not per-criterion) | `### Confidence Score` — each stage records its 0-100 score and rationale before STAGE-COMPLETE; team lead checks scores ≥95 at PF5-VERIFY |

**Status legend:**

| Status | Meaning | Used By |
|--------|---------|---------|
| `--` | Not yet evaluated | Default for all |
| `DONE` | Implemented / addressed | Primary stage (DEV/DOCS/PLAN/TEST) |
| `PASS` | Independently verified as meeting criterion | REV, QA |
| `FAIL` | Verified as NOT meeting criterion | REV, QA |
| `PARTIAL` | Partially met -- see notes | Any stage |
| `N/A` | Not applicable to this stage | Any stage |

**Commit inclusion:** The task doc updates are committed as part of the same stage commit. cf-git-operations includes the task doc file alongside the primary work artifacts when processing the commit request from the stage teammate.

**Verification:** At PF5-VERIFY, the lead reads the task markdown criteria matrix. All criteria should show DONE/PASS across all pipeline stages with no `--` remaining in evaluated columns.

### Communication Patterns

| Pattern | How | When |
|---------|-----|------|
| Direct peer messaging | Teammates use SendMessage to each other | Normal operations (cf-development --> cf-git-operations for commits) |
| WorkGraph access | Via cf-knowledge-layer | Any stage completion, progress recording, task queries |
| Git operations | Via cf-git-operations | Branch, commit, PR -- no other teammate runs git write commands |
| Security queries | Via cf-security | Sandbox checks, protected resource consultation |
| Escalations to lead | Via SendMessage to lead | Blockers, ambiguity, rework limit concerns |

**Peer-to-peer messaging flow (direct, not through lead):**

| Sender | Receiver | Trigger | Message |
|--------|----------|---------|---------|
| cf-development | cf-git-operations | Code ready to commit | `"Please commit: {type}: {description}"` |
| cf-documentation | cf-git-operations | Docs ready to commit | `"Please commit: docs: {description}"` |
| cf-planning | cf-git-operations | Plan ready to commit | `"Please commit: plan: {description}"` |
| cf-quality-assurance | cf-git-operations | Tests ready to commit (WS-TEST) | `"Please commit: test: {description}"` |
| cf-planning | cf-knowledge-layer | Epic/task creation | `"PLANNER: create-epic -- {title}"` |
| Any teammate | cf-knowledge-layer | Progress update | `"{PREFIX}-UPDATE: task={id}, status={status}"` |
| cf-review | originating teammate | Rework findings | Detailed findings with file:line references |
| cf-quality-assurance | cf-development | QA failure details | `"QA-FAIL: {n} failures. {details}"` |

**Message routing rule:** Teammates send directly to service teammates (cf-git-operations, cf-knowledge-layer, cf-security) for routine operations. Escalate to the team lead only for blockers, ambiguity, or limit violations.

### Teammate Recycling

If a persistent teammate's context fills up (auto-compaction at ~95%):

1. Shut down the teammate (`SendMessage type="shutdown_request"`)
2. Respawn with the same name and agent type
3. New instance starts with fresh context
4. Re-send any necessary state via messages

This should be rare for function teammates whose context is bounded.

### Deferred Shutdown

🔒 **All on-demand teammates spawned during PF4-EXECUTE remain active until PF7-END.**

On-demand teammates (cf-development, cf-planning, cf-documentation, cf-review, cf-quality-assurance) are NOT shut down between pipeline stages or after pipeline completion. Instead:

1. Each teammate completes its stage work and reports completion
2. The lead spawns the next stage's teammate while previous teammates remain active
3. If a later stage requests rework (WS-REV `changes_requested` or WS-QA `fail`), the original teammate is still alive and can receive the rework assignment directly -- no re-spawn needed
4. On-demand teammates remain active through PF4-EXECUTE, PF5-VERIFY, and PF6-COMPLETE, and are shut down at PF7-END alongside persistent teammates

**Benefits:**

- Eliminates expensive re-spawns for rework loops
- Preserves full implementation context across review and QA cycles
- Reduces total token usage by avoiding context reconstruction

**Shutdown sequence (at PF7-END):**

```text
PF7-END reached
    |
    v
Shut down on-demand teammates first (any order):
    SendMessage(type="shutdown_request", recipient="cf-development")
    SendMessage(type="shutdown_request", recipient="cf-review")
    SendMessage(type="shutdown_request", recipient="cf-quality-assurance")
    ... (any other PF4 on-demand teammates)
    |
    v
Then shut down persistent teammates:
    SendMessage(type="shutdown_request", recipient="cf-git-operations")
    SendMessage(type="shutdown_request", recipient="cf-knowledge-layer")
    SendMessage(type="shutdown_request", recipient="cf-security")
    |
    v
Mark all PF7 tasks completed in task tracker:
    TaskUpdate(PF7-TSK-01, status=completed)
    TaskUpdate(PF7-TSK-02, status=completed)
    TaskUpdate(PF7-TSK-03, status=completed)
    → TaskCompleted hook fires, creates pathflow-pf-7 sentinel
    |
    v
TeamDelete (task list destroyed — safe because sentinel already exists)
```

### Parallel Batch Execution

When a work stage involves multiple independent items (files, components, docs), the lead SHOULD spawn multiple instances of the same teammate type to work in parallel. Defaulting to parallel execution prevents context exhaustion — a single teammate processing many files will hit context limits, requiring respawn and rework that costs more than upfront parallelization.

**When to parallelize:**

| Condition | Parallelize? | Example |
|-----------|-------------|---------|
| Multiple independent files, no shared state | Yes | 3 docs, each in a separate directory |
| Files that import/depend on each other | No | Component + its tests in the same module |
| Large single file | No | One big refactor — single teammate |
| Mixed independent + dependent | Batch the independent ones | 2 independent + 1 dependent = batch of 2, then 1 |
| Scope exceeds batch_size for the stage | Yes | 5 files in WS-DOCS (batch_size=2) → 3 instances |

**Batch sizing rules:**

| Parameter | WS-DEV | WS-PLAN | WS-DOCS | WS-TEST | WS-REV | WS-QA |
|-----------|--------|---------|---------|---------|--------|-------|
| `max_parallel` | 3 | 2 | 3 | 2 | 2 | 1 |
| `batch_size` | 2 | 2 | 2 | 2 | 2 | 1 |

Source: `pathflow-config.json` stage definitions.

- Never exceed `max_parallel` concurrent instances for a stage
- Process items in batches of `batch_size`
- Reserve at least 30% of session token budget for review, commit, and PR phases
- If unsure about remaining budget, reduce batch size to 1

**Context exhaustion prevention:**

- A single on-demand teammate can typically handle 2-4 files before context pressure
- If the total scope involves reading + modifying more files than `batch_size`, split proactively
- Do NOT assign all work to one teammate and wait for context exhaustion — split upfront
- When a teammate reports context pressure or goes idle without completing, immediately split remaining work across new instances

**Naming convention for parallel instances:**

| Instance | Name | Example |
|----------|------|---------|
| First (or solo) | `cf-{role}` | `cf-development` |
| Additional | `cf-{role}-{n}` | `cf-development-2`, `cf-development-3` |

**Coordination rules:**

- All parallel instances commit through the SAME cf-git-operations (serialized commits)
- Each instance gets a clear, non-overlapping file scope in its spawn prompt
- Lead waits for ALL instances in a batch to complete before starting the next batch
- If any instance fails, the lead resolves before proceeding

**Applies to:** Any on-demand teammate during PF4-EXECUTE — cf-development, cf-documentation, cf-planning, cf-quality-assurance (for WS-TEST).

### Cross-Session Parallelism

Parallel Batch Execution (above) covers intra-session parallelism — multiple teammates within ONE session. Cross-session parallelism enables multiple independent sessions running concurrently, each in its own git worktree.

| Concept | Purpose |
|---------|---------|
| Worktree isolation | Each session gets its own working copy via `git worktree add` (max 5 concurrent, enforced by `WorktreeRegistry`) |
| Claims | CRDT-based file claims prevent concurrent edits to the same file (enforced via scope_policy — soft/hard/permissive — using `Coordinator::acquire`) |
| Fencing tokens | Monotonic `FencingToken` values ensure claim validity across crashes |
| Merge queue | Advisory FIFO queue for PR merge ordering (`merge_queue::enqueue/dequeue`). Records ordering but does not enforce it -- GitHub's conflict detection provides the actual safety net. |
| Sync daemon | Propagates CRDT state between worktrees via git ref transport (5s interval, `sync::run_sync_cycle`). Auto-starts when worktree count > 1, auto-stops when <= 1. Crash cleanup detects dead workers and releases claims. |

**Worktree layout (shared vs local state):**

```text
.git-worktrees/worktree-{SID}/
├── .state/
│   ├── db/ → ../../.state/db/                (symlink — shared)
│   ├── coordination/ → ../../.state/coordination/ (symlink — shared)
│   ├── logs/ → ../../.state/logs/             (symlink — shared)
│   ├── registry/ → ../../.state/registry/     (symlink — shared)
│   ├── backups/ → ../../.state/backups/       (symlink — shared)
│   ├── ledger/                                (LOCAL per-worktree — since PR #221)
│   ├── runtime/                               (LOCAL per-worktree)
│   ├── session/                               (LOCAL per-worktree)
│   └── sentinels/                             (LOCAL per-worktree)
└── (full working copy)
```

**Key APIs** (`codeflow-cli/core/src/coordination/`):
- `Coordinator::acquire(path, session_id)` / `release(path, session_id)` — claim lifecycle
- `claims::acquire_batch(coordinator, paths, session_id)` — batch claim acquisition
- `merge_queue::enqueue(coordinator, entry)` / `dequeue(coordinator)` — PR merge ordering
- `sync::run_sync_cycle(config, peer_id)` — CRDT state propagation between worktrees
- `sync::start_daemon(project_dir, interval_secs)` / `sync::stop_daemon(project_dir)` — daemon lifecycle
- `sync::daemon_status(project_dir)` — daemon health check (running, pid, peer_id, sessions)
- `sync::cleanup_dead_workers(config)` — dead worker detection and claim release
- `conflict::attempt_rebase(repo_path, target_branch)` — auto-rebase with RebaseResult (Success/ConflictAborted)

### Teammate Name Preservation

When recycling a persistent teammate, ALWAYS reuse the SAME name.

| Do | Do Not |
|----|--------|
| Shutdown cf-git-operations, respawn as cf-git-operations | Spawn cf-git-operations-2 |
| Use name="cf-git-operations" in Task() | Use name="cf-git-operations-2" |

Multiple instances with numbered suffixes (-2, -3) create confusion, break task assignment, and violate the single-instance-per-role principle.

Before respawning, verify the old teammate is fully shut down (check tmux pane status).

### Teammate Shutdown Protocol

When shutting down a teammate, the lead MUST verify the tmux pane is properly terminated.

**Shutdown sequence:**

1. Send shutdown request: SendMessage(type="shutdown_request", recipient="{name}", content="Work complete")
2. Wait for shutdown confirmation
3. Verify tmux pane is dead: tmux list-panes -a | grep {pane_id}
4. If pane still exists, kill it: tmux kill-pane -t {pane_id}

**Before respawning a persistent teammate:**

1. Read team config to get the old teammate tmuxPaneId
2. If pane exists, kill it first: tmux kill-pane -t {old_pane_id}
3. Then spawn the new instance with the SAME name (see Name Preservation above)

NEVER spawn a new teammate while the old tmux pane is still alive. This causes zombie processes, resource leaks, and numbered name suffixes.

### Teammate Health Verification

The lead MUST verify teammate health before relying on them for critical operations.

**When to check:**

| Trigger | Action |
|---------|--------|
| After spawning a teammate | Verify tmux pane exists within 10 seconds |
| Before sending critical messages | Quick tmux health check |
| After idle notification with no content message | Verify pane is alive |
| After extended silence (>60s) | Check pane status |

**How to check:**

```text
tmux ls                                    # Is the tmux server alive?
tmux list-panes -a -F '#{pane_id} #{pane_pid} #{pane_dead}'  # Pane-level health
```

**Recovery when dead:**

1. Note the dead teammate's name from team config
2. Respawn with the SAME name (see Name Preservation above)
3. If numbered suffix created (stale config), work with it but note for cleanup
4. Re-send any pending instructions to the new instance

### Team Persistence

🔒 **NEVER call TeamDelete during active session.**

- Persistent function teammates (cf-security, cf-knowledge-layer, cf-git-operations) stay alive PF1 through PF7
- On-demand role teammates remain active through PF4-EXECUTE, PF5-VERIFY, and PF6-COMPLETE, and are shut down at PF7-END alongside persistent teammates (→ See Deferred Shutdown above)
- Individual teammate shutdown via `SendMessage(type="shutdown_request")` is SAFE -- does not affect team or task list
- Team cleanup (`TeamDelete`) ONLY during PF7-END as the FINAL step, AFTER all PF7 tasks are marked completed (PostToolUse hook removes pathflow-active flag after TeamDelete succeeds)
- A PreToolUse hook (`codeflow hooks pre-tool-use team-guard`) blocks accidental team dissolution while pathflow-active flag exists

⛔ **Dissolving the team mid-session destroys the entire PathFlow task graph -- all phase markers, work stage tracking, dependency ordering, and checkpoint state. This is unrecoverable.**

→ Next: Section 6 defines work type pipelines and request routing

---

## 6. Work Pipelines & Routing

### Work Type Pipelines

The work type determines which stages execute during PF4-EXECUTE:

| Work Type | Pipeline | Primary Teammate |
|-----------|----------|------------------|
| FEAT | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| FIX | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| RFCT | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| CICD | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| HTFX | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| CHOR | WS-DEV --> WS-SEC --> WS-REV --> WS-QA | cf-development |
| DOCS | WS-DOCS --> WS-REV | cf-documentation |
| TEST | WS-TEST --> WS-REV --> WS-QA | cf-quality-assurance |
| PLAN | WS-PLAN --> WS-REV | cf-planning |
| SPKE | WS-PLAN --> WS-REV | cf-planning |

**WS-REV is universal.** Every work type gets an independent review stage.

### Work Stages (1:1 Stage-to-Teammate Mapping)

| Stage | Teammate | Purpose |
|-------|----------|---------|
| WS-DEV | cf-development | Code implementation + unit tests |
| WS-SEC | cf-security | Security scan: OWASP Top 10, secret detection, dependency audit, input validation |
| WS-PLAN | cf-planning | Design, architecture, analysis, investigation |
| WS-DOCS | cf-documentation | Documentation writing |
| WS-REV | cf-review | Independent review (adapts per work type: CODE_REVIEW, DESIGN_REVIEW, DOCUMENTATION_REVIEW, TEST_REVIEW) |
| WS-QA | cf-quality-assurance | Integration testing, acceptance verification (quality gate) |
| WS-TEST | cf-quality-assurance | Primary test implementer (when tests ARE the deliverable) |

→ See Section 5 for peer-to-peer messaging patterns

### Rework Loop Flow

```text
WS-DEV --> WS-REV --> [approved] --> WS-QA --> [pass] --> PF5-VERIFY
                  \                         \
                   --> [changes_requested]    --> [fail]
                       back to WS-DEV             back to WS-DEV
                       (iteration +1)              (retry +1)
```

### Rework Limits

| Parameter | Default | Trigger |
|-----------|---------|---------|
| `max_rework_iterations` | 3 | WS-REV returns `changes_requested` --> back to primary stage |
| `max_qa_retries` | 3 | WS-QA returns `fail` --> back to WS-DEV |
| `stage_timeout_secs` | 7200 (120 min) | A worker makes no stage progress (no new `ws-*` or `pathflow-pf-3` sentinel) within this window. INF-TSK-024-053: bumped from 3600 because real FIX tasks legitimately need >60min to reach the first stage sentinel. The watcher also resets on `pathflow-pf-3` (branch creation), so PF1/PF2 latency does not consume the WS-DEV budget. When fired, the worker reports `STATUS=timeout` (not `failed`) with a non-empty error message. |

If limits exceeded: escalate to user (interactive) or mark task `blocked` and skip to PF7-END (autorun).

### Routing Precedence

1. Explicit `/cf-*` commands take precedence
2. Work type keywords trigger proactive teammate delegation
3. Simple queries --> lead handles directly (untracked)

### Command --> Teammate Routing

| Command | Teammate | Stage / Phase |
|---------|----------|---------------|
| /cf-plan | cf-planning | WS-PLAN |
| /cf-develop | cf-development | WS-DEV |
| /cf-review | cf-review | WS-REV |
| /cf-test | cf-quality-assurance | WS-TEST / WS-QA |
| /cf-ship | cf-git-operations | PF6-COMPLETE |
| /cf-deploy | cf-development | WS-DEV (CICD) |
| /cf-document | cf-documentation | WS-DOCS |
| /cf-cleanup | cf-git-operations | PF7-END |
| /cf-resume | lead | Resume active work (direct) |
| /cf-stack | lead | Show session state (direct) |
| /cf-approval-mode | lead | View/change approval mode (direct) |
| /cf-help | lead | Show available commands (direct) |
| /cf-doctor | lead | Diagnose infrastructure issues (direct) |
| /cf-autorun | lead | Launch autorun session (direct) |

### Command Availability

| Command | Available | Notes |
|---------|-----------|-------|
| `/cf-resume` | Always | Session recovery, no prerequisites |
| `/cf-help` | Always | Information and navigation |
| `/cf-stack` | Always | Show session state |
| `/cf-doctor` | Always | Diagnose infrastructure |
| `/cf-approval-mode` | Always | View/change approval mode |
| `/cf-plan` | Always (entry point) | Triggers full PathFlow: PF1 → classify as PLAN → WS-PLAN pipeline |
| `/cf-develop` | Always (entry point) | Triggers full PathFlow: PF1 → classify as FEAT/FIX/etc. → WS-DEV pipeline |
| `/cf-document` | Always (entry point) | Triggers full PathFlow: PF1 → classify as DOCS → WS-DOCS pipeline |
| `/cf-test` | Always (entry point) | Triggers full PathFlow: PF1 → classify as TEST → WS-TEST pipeline |
| `/cf-deploy` | Always (entry point) | Triggers full PathFlow: PF1 → classify as CICD → WS-DEV pipeline |
| `/cf-review` | Needs prior work | Reviews existing changes on a branch -- requires something to review |
| `/cf-ship` | PF5+ | Requires reviewed, passing work to ship |
| `/cf-cleanup` | Always (--force) or PF6+ | Cleanup is safe anytime with --force flag |
| `/cf-autorun` | No active PathFlow | Prevents collision with active session |

Entry point commands (`/cf-plan`, `/cf-develop`, `/cf-document`, `/cf-test`, `/cf-deploy`) start full PathFlow sessions. They do NOT require being inside PF4-EXECUTE already -- they create the entire lifecycle from PF1-INIT onward.

### Work Type Keywords --> Pipeline

| Keywords in Request | Classified As | Pipeline |
|--------------------|---------------|----------|
| implement, build, code, feature, add | FEAT | WS-DEV --> WS-REV --> WS-QA |
| fix, bug, broken, error, issue | FIX | WS-DEV --> WS-REV --> WS-QA |
| refactor, restructure, clean up | RFCT | WS-DEV --> WS-REV --> WS-QA |
| ci, cd, pipeline, deploy, github actions | CICD | WS-DEV --> WS-REV --> WS-QA |
| hotfix, urgent, production | HTFX | WS-DEV --> WS-REV --> WS-QA |
| chore, maintenance, update deps | CHOR | WS-DEV --> WS-REV --> WS-QA |
| document, write docs, update docs | DOCS | WS-DOCS --> WS-REV |
| test, write tests, add coverage | TEST | WS-TEST --> WS-REV --> WS-QA |
| plan, design, architect, analyze | PLAN | WS-PLAN --> WS-REV |
| spike, investigate, prototype, POC | SPKE | WS-PLAN --> WS-REV |

### Smart Teammate Utilization

The lead MUST use teammates intelligently — both correct routing AND parallel execution:

**Correct routing:** Match work to the right teammate type. Common mistakes to avoid:

| Work Type | Wrong Route | Correct Route |
|-----------|-------------|---------------|
| Agent definition edits (`.claude/agents/*.md`) | cf-development | cf-documentation (agent defs are documentation) |
| Config/infrastructure docs | cf-development | cf-documentation |
| Test implementation | cf-development | cf-quality-assurance (WS-TEST) |
| Design analysis | cf-development | cf-planning |

**Parallel execution:** When independent tasks exist, spawn multiple teammates concurrently instead of serializing through one:

| Scenario | Wrong Approach | Correct Approach |
|----------|---------------|------------------|
| Code fix + config edit (independent) | Wait for cf-development to finish code, then assign config | Spawn cf-development for code AND a separate agent for config in parallel |
| 3 independent doc updates | Assign all 3 to one cf-documentation | Spawn up to `max_parallel` cf-documentation instances (→ See Section 5: Parallel Batch) |
| Code change + doc update (independent) | Serialize: cf-development then cf-documentation | Spawn both concurrently |
| Implementation + test writing (dependent) | Parallelize (tests depend on implementation) | Serialize: cf-development first, cf-quality-assurance after |

**Key principle:** If tasks are independent (no shared state, no dependency), they SHOULD run in parallel. Default to parallel when in doubt — context exhaustion from overloading one teammate costs more than coordination overhead from multiple teammates.

### Exploration and Research Routing

| Request Type | Route To | Example |
|-------------|----------|---------|
| Quick file lookup | Explore sub-agent (Task tool) | "Find where X is defined" |
| Codebase analysis | cf-planning (WS-PLAN) | "Analyze the hook architecture" |
| External research | Explore sub-agent with WebSearch | "What does Claude Code support?" |
| Design investigation | cf-planning (SPKE pipeline) | "Investigate approaches for X" |

→ Next: Section 7 covers enforcement gates and operational rules

---

## 7. Enforcement & Operations

### Phase Gate Enforcement

PathFlow phase ordering is enforced through a hybrid of hooks and instructions:

**Hook-enforced gates (automatic, blocks violations):**

| Gate | Sentinel Required | Blocks | Hook |
|------|-------------------|--------|------|
| Edit/Write before PF3 | `pf-3` | Edit, Write tools | `codeflow hooks pre-tool-use gate-check` |
| git commit before PF3 | `pf-3` | `Bash(git commit)` | `codeflow hooks pre-tool-use gate-check` |
| git push/PR before PF5-VERIFY + WS-REV | `pf-5` + `ws-rev` (dual gate, both required; `pf-5` transitively requires `pf-4`) | `Bash(git push)`, `Bash(gh pr)` | `codeflow hooks pre-tool-use gate-check` |
| Role teammate spawn before PF3 | `pf-3` | Task tool for cf-development, cf-planning, cf-documentation, cf-review, cf-quality-assurance | `codeflow hooks pre-tool-use gate-check` |
| Stage ordering within PF4 | Primary stage sentinel (`ws-dev`/`ws-docs`/`ws-plan`/`ws-test`) must exist before WS-REV can complete and `ws-rev` before WS-QA can ship | `Bash(git push)`, `Bash(gh pr)` (via dual gate requiring `pf-5` + `ws-rev`) | `codeflow hooks pre-tool-use gate-check` |
| TeamDelete during active session | pathflow-active flag + `pf-6` | TeamDelete tool (allows through if `pf-6` exists; flag removed by PostToolUse sentinel hook after TeamDelete succeeds) | `codeflow hooks pre-tool-use team-guard` |
| PR body validation | Valid body content | `Bash(gh pr create)` missing required sections, containing AI attribution, or containing emoji | `codeflow hooks pre-tool-use gh-pr-guard` |
| Claim enforcement (scope_policy) | File claims via CRDT | Edit/Write (scope_policy=soft: claim-coordinated, scope_policy=hard: scope-restricted, scope_policy=permissive: unrestricted) | `codeflow hooks pre-tool-use gate-check` |

**Scope policy enforcement modes** (enforced by `try_acquire_claim()` in `pre_tool_use.rs`):

- `scope_policy=hard`: edits to files NOT in `file_scope` are BLOCKED immediately (exit 2) and a `ClaimConflict` event is emitted to `coordination-events.jsonl`. No claim acquisition is attempted.
- `scope_policy=hard` + empty `file_scope`: REJECTED at the CLI **write** time by `codeflow state set-active-task` (Option α). The CLI exits non-zero with the message "scope_policy=hard requires non-empty file_scope; task X has empty file_scope -- fix task definition". The hook applies defense-in-depth for stale `active-task.json` from older binaries: it BLOCKS the edit and emits a `ClaimConflict` event with a misconfiguration message.
- `scope_policy=soft` + file IN `file_scope`: claim auto-acquired at startup via `acquire_batch()`, edit allowed; on success a `ClaimAcquired` event is emitted (or `ClaimConflict` warn-and-proceed if another session held the claim).
- `scope_policy=soft` + file NOT in `file_scope`: attempt claim via `Coordinator::acquire` — if acquired (unclaimed), allow edit + emit `ScopeExpansion` event; if conflict, BLOCK (exit 2) + emit `ClaimConflict` event
- `scope_policy=permissive`: no scope checking, no claim acquisition, no events emitted; edit allowed (interactive sessions only; forbidden for `autorun_eligible=true` tasks)
- Default is `soft` when not specified in the task definition

**`codeflow state set-active-task` validation:** When invoked, the CLI looks up the task from the SurrealDB `tasks` table by `task_format_id` and populates `scope_policy` and `file_scope` from the task record. CLI flags `--scope-policy` and `--file-scope <json>` override the DB-derived values when explicitly provided. Before writing `active-task.json`, the CLI rejects `scope_policy=hard` with empty/missing `file_scope` and exits non-zero. This prevents the hook from ever observing a misconfigured state.

**Instruction-enforced gates (not currently hook-enforced):**

| Gate | Instruction | Why Not Hook-Enforced |
|------|-------------|----------------------|
| pf-1 before spawning cf-knowledge-layer | "Verify pf-1 sentinel exists before PF2-CONTEXT" | Low risk -- phases run sequentially. The pf-3 gate checks a hardcoded ROLE_TEAMMATES name list (5 role teammates); function teammates are not in that list and pass through ungated. Adding per-phase gates would add complexity for negligible benefit. |
| pf-2 before spawning cf-git-operations | "Verify pf-2 sentinel exists before PF3-CLASSIFY" | Same rationale -- function teammates are not in the ROLE_TEAMMATES gate list. PF3 naturally follows PF2 in the sequential lifecycle. |

**Not enforced (acceptable risk):**

| Skipped Phase | Why Acceptable |
|---------------|---------------|
| Skipping PF6-COMPLETE, going directly to PF7 | TeamDelete (the critical PF7 action) IS gated on `pf-6` sentinel by team-guard hook. Other PF7 cleanup actions (teammate shutdown) are safe regardless. |

**Cumulative Enforcement:**
All sentinel checks verify the ENTIRE chain, not just the preceding one. Dynamically driven from pathflow-config.json:
- Phase registration: ALL pf-1 through pf-{N-1} (not just pf-{N-1})
- PF4 task completion: ALL prior pipeline stage sentinels
- Stage sentinel creation: ALL prior pipeline stages
- Push/PR gate: ALL phases (pf-1 through pf-5) + ALL pipeline stages
- TeamDelete guard: ALL pipeline stage sentinels
Zero hardcoded phase/stage names in enforcement hooks.

### Sentinel System

PathFlow sentinels (`pathflow-pf-3`, `pathflow-ws-dev`, etc.) are session-scoped, no TTL, and created automatically by two complementary mechanisms:

**Phase sentinels** (pf-1 through pf-7) are created by the **checkpoint enforcement system** -- a three-layer architecture:

| Layer | Hook | Event | Purpose |
|-------|------|-------|---------|
| 1. Registration | `codeflow hooks post-tool-use checkpoint-register` | PostToolUse (on TaskCreate) | Registers PF{N}-TSK-{NN} tasks in checkpoint; blocks cross-phase registration (exit 2) if previous phase sentinel missing |
| 2. Completion | `codeflow hooks task-completed checkpoint-complete` | TaskCompleted | Marks tasks complete; creates phase sentinel when all tasks in a phase are done/skipped |
| 3. Gate | `codeflow hooks pre-tool-use gate-check` | PreToolUse | Blocks Edit/Write until `pf-3` sentinel exists |

The checkpoint file (`.state/session/{SID}/pathflow/pathflow-phase-tasks.json`) tracks expected tasks, registrations, completions, and skips per phase. All phases are pre-initialized at session start by the SessionStart hook calling `checkpoint_init_all_phases()`.

**Stage sentinels** (ws-dev, ws-rev, etc.) are created by the `codeflow hooks post-tool-use sentinel-write` PostToolUse hook via pattern-matching on stage completion messages (e.g., `STAGE-COMPLETE: WS-DEV`).

Agents must NOT create sentinels manually -- if a sentinel appears missing, investigate the hook pipeline or verify the session ID path at `.state/sentinels/pathflow/{session-id}/`.

**Worktree note:** In worktree mode, sentinels are stored at `{worktree}/.state/sentinels/` (local, not symlinked). Each worktree has its own sentinel namespace.

**Autorun stage_timeout heartbeats (INF-TSK-024-053):** The autorun stage_timeout watcher (`await_stage_timeout` in `autorun/worker.rs`) treats two sentinel families as "stage progress" and resets its clock on whichever is newer:

| Family | Filenames | Why |
|--------|-----------|-----|
| Stage completion | `pathflow-ws-*` / `ws-*` (dev, rev, qa, etc.) | Primary signal — a stage finished |
| Branch creation | `pathflow-pf-3` / `pf-3` | PF3-CLASSIFY heartbeat — prevents PF1/PF2/PF3 latency from eating the WS-DEV budget |

Other phase sentinels (`pf-1`, `pf-2`, `pf-4..7`) are NOT heartbeats — they either fire too early (pf-1/2 every session) or too late (pf-4..7 after a `ws-*` already reset the clock). See `is_stage_progress_sentinel` in `core/src/autorun/worker.rs` for the canonical inclusion list. When stage_timeout fires, the worker kills its tmux session (preventing claude subprocess leaks) and returns `exit_code=125`, which the post-invoke classifier maps to `AutorunTaskRunStatus::Timeout`.

→ See Section 4 (Phase Reference) for sentinel-to-phase mapping

### Task Tracker Mirroring

🔒 **MANDATORY:** The team lead MUST create INDIVIDUAL task tracker entries for EVERY PF{N}-TSK-{NN} task using TaskCreate, and update EACH with TaskUpdate as they complete. Clubbing multiple tasks into a single entry, skipping task registration, or deferring registration is a PROTOCOL VIOLATION that breaks phase visibility, dependency tracking, and stage ordering. Failure to register tasks individually WILL cause downstream phase gates to lose ordering context and review stages to miss acceptance criteria. Register each task BEFORE starting it, mark it `in_progress` when work begins, and `completed` when done. NO EXCEPTIONS.

| PathFlow Event | Task Tracker Action |
|---|---|
| Phase entered | TaskCreate using phases[{phase}].subject / .description / .activeForm -- ONE entry per phase |
| Phase task started | TaskCreate per PF{N}-TSK-{NN} -- ONE entry per task |
| Phase task completed | TaskUpdate status=completed for that task entry |
| Phase completed | TaskUpdate status=completed for the phase entry |
| Stage entered | TaskCreate using stages[{stage}].subject / .description / .activeForm -- ONE entry per stage |
| Stage completed | TaskUpdate status=completed for that stage entry |

**Rules:**

- NEVER club multiple phases into a single task tracker entry
- NEVER skip creating entries for individual PF{N}-TSK-{NN} tasks
- Use TaskUpdate addBlockedBy to express phase ordering (PF2 blocked by PF1, etc.)
- 🔒 **L1 ENFORCED:** When creating individual PF{N}-TSK-{NN} task tracker entries, the lead MUST read the `blocked_by` field from `pathflow-config.json` for each task and apply it using `TaskUpdate(addBlockedBy=[...])` IMMEDIATELY after `TaskCreate`. Tasks with `blocked_by` fields that are not mirrored to the task tracker lose ordering visibility, causing downstream stages to execute out of order. This is NOT optional -- every `blocked_by` in the config MUST be reflected in the task tracker.
- Entries are ephemeral and disposable -- if lost to context overflow, recreate for current phase only
- JSONL/SurrealDB remains authoritative. Task tracker is derived and visual only.
- The task tracker step is embedded as a mandatory sub-step within each Section 4.2 phase step.
- 🔒 **Phase ordering constraint:** Task registration for phase N MUST wait until ALL tasks in phase N-1 are completed and the `pf-{N-1}` sentinel exists. The `checkpoint-register` PostToolUse hook blocks cross-phase registration (exit 2) if the prior phase sentinel is missing. Do NOT call `TaskCreate` for `PF{N}-TSK-{NN}` entries until the previous phase is fully complete. Create all tasks for ONE phase, complete them, then move to the next phase.

**Reference:** `pathflow-config.json` -- template properties (subject, description, activeForm) are inline in the `phases` and `stages` sections.

### Routing Compliance

When the lead delegates PF{N}-TSK-{NN} tasks to teammates, it MUST:

- Follow the `assigned_to` field in pathflow-config.json to determine which teammate executes the task
- Follow the `operation` field to determine what operation to request
- These fields are authoritative routing directives, not optional metadata
- Ignoring `assigned_to` or `operation` is a protocol violation

### Enforcement Model

Three complementary mechanisms provide defense-in-depth:

| Mechanism | Strength | Catches |
|-----------|----------|---------|
| **Instructions** | Agent definitions + CLAUDE.md guide behavior proactively | Happy path compliance |
| **Tasks** | PathFlow task graph makes state visible to all agents | Ordering awareness |
| **Hooks** | PreToolUse hooks block violations at tool-call level | Edge cases where instructions are ignored |

### Git Operations

🔒 **All git write operations go through cf-git-operations teammate. Never run git write commands directly.**

- No direct commits to main/master
- Feature branches: `feat/*`, `fix/*`, `plan/*`, `docs/*`, `refactor/*`, `test/*`, `chore/*`, `cicd/*`, `spike/*`, `hotfix/*`, `autorun/*`
- Commit messages follow conventional format (enforced by cf-git-operations)
- All changes through PRs to main
- Merge conflict detection: Before PR creation (PF6-TSK-05), `check_merge_conflicts()` from `git/conflict.rs` verifies the branch can merge cleanly. In parallel sessions, the merge queue (`coordination/merge_queue.rs`) serializes PR merges.

### Sandbox Bypass

Claude Code's sandbox blocks network operations by default. Use `dangerouslyDisableSandbox: true` in Bash tool calls for commands that access remote servers.

**Commands requiring bypass:**

| Category | Commands |
|----------|----------|
| Git network | `git push`, `git pull`, `git fetch`, `git clone`, `git remote update`, `git ls-remote` |
| GitHub CLI | `gh pr`, `gh issue`, `gh api`, `gh workflow`, `gh run` |
| Package managers | `npm install`, `pip install` |

**Who handles bypass:**

| Context | Who Bypasses | Notes |
|---------|-------------|-------|
| PathFlow mode | cf-git-operations | Handles git network + GitHub CLI bypass internally |
| PathFlow mode (packages) | Requesting agent | Sets `dangerouslyDisableSandbox: true` directly |
| Outside PathFlow | Executing agent | Pre-flight checks: correct remote, correct branch, no secrets staged |
| Autorun mode | CLI orchestrator | Sandbox pre-bypassed, no explicit action needed |

**Reference:** `.claude/skills/cf-sandbox-standards/SKILL.md`

### Testing

- Unit tests: written by cf-development during WS-DEV (tightly coupled to code)
- Integration/acceptance tests: written/verified by cf-quality-assurance during WS-QA
- Test suite: run via `codeflow test --mode full` (the single authoritative command for all pipeline stages; routes to all configured test targets per `.codeflow/config/testing/test-config.json`)
- All test changes verified before marking stage complete
- Structural integrity check: `codeflow test structural-check` — validates source-to-test file mappings; exits 1 on findings; use `--only <target>` to limit scope; `--format json` for machine-readable output
- Tag-based filtering: `--only-tag <csv>` runs only targets with matching priority tags (`critical`, `high`, `medium`, `low`); `--skip-tag <csv>` excludes matching targets; both compose with `--only`/`--skip` (AND logic); `--skip-tag` wins on conflict; untagged targets excluded when `--only-tag` is non-empty

### PR Workflow

1. Work completes in PF4-EXECUTE (all stages pass)
2. PF5-VERIFY confirms acceptance criteria
3. cf-git-operations creates PR (PF6-TSK-05)
4. cf-git-operations verifies PR CI (PF6-TSK-06)
5. cf-git-operations awaits PR merge disposition (PF6-TSK-07)
6. cf-knowledge-layer records PR outcome (PF6-TSK-08)
7. cf-git-operations syncs local repository (PF6-TSK-09)
8. Lead proceeds to PF7-END
9. New session for new work

**PR body MUST include test results.** The PR description must contain the `## Test Results` section below, populated verbatim from `codeflow test --mode full` structured output. PRs without test stats are incomplete and must not be created. Do NOT manually compose coverage numbers, thresholds, or exempted files — the command produces the authoritative data. On subsequent pushes to an open PR, cf-git-operations MUST re-run the command and update the PR body's `## Test Results` section to reflect current state.

```markdown
## Test Results

### 1. Overall Test Pass Status

| Target | Mode | Passed | Failed | Skipped | Duration |
|--------|------|--------|--------|---------|----------|
| {target} | full | {n} | 0 | 0 | {time} |
| **Total** | | **{n}** | **0** | **0** | **{time}** |

Runs: {n} consecutive clean.

### 2. Overall Coverage

| Target | Coverage | Per-rule summary |
|--------|----------|-----------------|
| {target} | {n}% | {rule summary} |

#### Exempted Files (only when exceptions exist)

| Target | File | Coverage | Configured Threshold | Reason |
|--------|------|---------:|---------------------:|--------|
| {target} | {path} | {n}% | {n}% | {reason from test-config.json} |

### 3. Modified File Coverage

| Target | File | Coverage | Threshold | Status |
|--------|------|---------:|----------:|--------|
| {target} | {path} | {n}% | 85% | PASS/FAIL |

### 4. Test Failures

(only emitted when Failed > 0)

| Target | Suite | Test | Message | File:Line |
|--------|-------|------|---------|-----------|

### 5. Slowest Tests (optional)

| Target | Test | Duration |
|--------|------|---------|
```

When no test targets are configured, `codeflow test --mode full` emits a single line instead of the five sections:

```markdown
## Test Results

No test targets configured. Run `codeflow test setup` to add targets.
```

This is informational, not an error — PRs with no configured targets still pass the PR-body gate.

### Merge Protection

🔒 **Hard block on protected branch merge.** The following branches are protected (from `enforcement-policy.json` `merge_protection`):

- `main`, `master`, `release/*`, `production`

| Scenario | Behavior |
|----------|----------|
| `gh pr merge` targeting protected branch | BLOCKED by `codeflow hooks pre-tool-use gh-pr-guard` hook. PR must be merged via GitHub UI. |
| `integration_auto_merge:true` + protected target | FORBIDDEN. Validation error at batch parsing time. |
| Interactive session `/cf-ship` | Verifies CI, notifies user to merge via GitHub UI. Does NOT execute merge. |
| Autorun `integration_auto_merge:true` + non-protected target | Auto-merges via `gh pr merge --delete-branch` to integration branch. |

### Decision Tiers

| Tier | When | Action | Example |
|------|------|--------|---------|
| 1 | Standard, reversible | Proceed autonomously, document in progress notes | File naming, code style |
| 2 | Trade-offs, preferences | Recommend approach, note in commit message | Library choice, API design |
| 3 | Ambiguous, breaking, architectural | ADR via cf-planning, ask user first | Schema changes, new dependencies |

### Graceful Degradation

If the enforcement system fails (hook malfunction, sentinel not auto-created), PathFlow degrades gracefully:

```text
Full enforcement (nominal)
  Instructions + Tasks + Hooks all active
       |
       | (hook fails to auto-create sentinel)
       v
Partial enforcement
  Instructions + Tasks active, hooks warn but skip sentinel checks
       |
       | (hooks fail entirely)
       v
Advisory only
  Instructions + task graph still provide ordering
  Session proceeds without guard rails
  Log degradation for post-session analysis
```

A development session should never be BLOCKED by an enforcement system failure. The enforcement system catches mistakes; it is not a gating prerequisite for work.

→ See Section 11 (Recovery) for troubleshooting enforcement issues
→ Next: Section 8 lists capabilities, skills, hooks, and commands

---

## 8. Capabilities

### Skill (1 active)

| Skill | Purpose | When to Use |
|-------|---------|-------------|
| cf-working-protocol | Cognitive procedures (5 operations) | Every response (team lead) |

10 skills archived to `.codeflow/docs/archived/skills/`. SOPs are now embedded directly in agent definitions.

### On-Demand Skills

These skills provide detailed standards and can be loaded by agents as needed:

| Skill | Purpose | Primary Users |
| --- | --- | --- |
| `cf-rust-standards` | Rust development conventions, error handling, clippy, testing | cf-development, cf-quality-assurance, cf-review |
| `cf-shell-standards` | Shell scripting conventions, formatting, error handling | cf-development, cf-git-operations |
| `cf-python-standards` | Python scripting conventions, type hints, testing | cf-development, cf-quality-assurance |
| `cf-markdown-standards` | Markdown formatting, templates (ADR, epic, task) | cf-planning, cf-documentation |
| `cf-sandbox-standards` | Sandbox bypass rules for network operations | cf-git-operations, cf-development, cf-quality-assurance |

### Agent Definitions (8)

| Agent | Category | Purpose |
|-------|----------|---------|
| cf-security | Persistent | Security consultation, sandbox, protected resources |
| cf-knowledge-layer | Persistent | WorkGraph, memory, DB operations |
| cf-git-operations | Persistent | Git operations (branch, commit, PR) |
| cf-development | On-demand | Code implementation + unit tests + CICD |
| cf-planning | On-demand | Design, architecture, analysis |
| cf-documentation | On-demand | Documentation writing |
| cf-review | On-demand | Independent review (4 modes) |
| cf-quality-assurance | On-demand | QA gate or primary test implementer |

**Location:** `.claude/agents/cf-*.md`

### Hooks (22 hook entries)

Hooks fire automatically at lifecycle points. Configured in `.claude/settings.json` as `codeflow hooks <event> <subcommand>` invocations.

| Event | Count | Subcommands |
|-------|-------|-------------|
| SessionStart | 3 | init, instructions, logging |
| UserPromptSubmit | 2 | validate, logging |
| PreToolUse | 7 | gate-check, team-guard, edit-write-guard, gh-pr-guard, protection-guard, security, webfetch-guard |
| PostToolUse | 4 | sentinel-write, settings-validate, checkpoint-register, logging |
| TaskCompleted | 1 | checkpoint-complete |
| Stop | 1 | logging |
| SubagentStop | 1 | logging (shared with Stop) |
| SessionEnd | 2 | cleanup, logging |

**Key hooks:**

- **gate-check** (`codeflow hooks pre-tool-use gate-check`): Blocks Edit/Write before PF3-CLASSIFY. Enforces PathFlow sentinel checks.
- **sentinel-write** (`codeflow hooks post-tool-use sentinel-write`): Creates stage sentinels (ws-dev, ws-rev, etc.) via pattern-matching on stage completion messages.
- **checkpoint-register** (`codeflow hooks post-tool-use checkpoint-register`): Registers PF{N}-TSK-{NN} tasks in checkpoint; blocks cross-phase registration (exit 2) if previous phase sentinel missing.
- **checkpoint-complete** (`codeflow hooks task-completed checkpoint-complete`): Marks tasks complete in the checkpoint; creates phase sentinels (pf-1, pf-2, etc.) when all phase tasks are done/skipped.
- **team-guard** (`codeflow hooks pre-tool-use team-guard`): Blocks TeamDelete while pathflow-active flag exists. Protects task graph.
- **edit-write-guard** (`codeflow hooks pre-tool-use edit-write-guard`): Scope enforcement for file operations.
- **protection-guard** (`codeflow hooks pre-tool-use protection-guard`): Enforces tiered protection for critical, high, and moderate resources.

### Commands (14)

```text
/cf-resume   /cf-plan       /cf-develop   /cf-review    /cf-test
/cf-ship     /cf-deploy     /cf-document  /cf-cleanup   /cf-stack
/cf-approval-mode   /cf-help   /cf-doctor   /cf-autorun
```

### CLI

```text
codeflow test --mode full              # Run all suites with coverage enforcement (authoritative for all pipeline stages)
codeflow test                          # Run test suite (default: essential mode, for quick checks only)
codeflow test --only-tag critical,high # Run only critical and high priority targets
codeflow test --skip-tag low           # Skip low priority targets
codeflow test structural-check         # Validate source-to-test file mappings (exits 1 on findings)
codeflow doctor                        # Diagnose infrastructure
codeflow worktree list                 # List active worktrees
codeflow worktree cleanup              # Clean up stale worktrees
codeflow worktree prune                # Remove orphaned worktree entries
codeflow autorun resume                # Re-run non-completed tasks from a batch
codeflow autorun batches               # List available batch files
codeflow autorun status --watch        # Continuously monitor autorun status
codeflow interactive                   # Launch Claude with worktree isolation (alias: codeflow -i)
codeflow interactive status            # Show active interactive sessions with liveness
codeflow interactive list              # Show all interactive sessions (active + complete)
codeflow interactive cleanup           # Remove stale sessions (dead PID detection)
codeflow rescue list                   # List saved rescue bundles (XDG cache root)
codeflow rescue show <id>              # Show metadata + diff preview for a bundle
codeflow rescue apply <id> [--repo X]  # Stage a bundle's patches into a target repo
codeflow rescue clean <id|--all|--older-than <dur>>  # Delete bundle(s)
codeflow rescue drop <id>              # Alias for clean
codeflow rescue pin <id>               # Exclude bundle from auto-prune
codeflow rescue unpin <id>             # Re-enable auto-prune
```

Rescue bundles are written to an XDG cache directory (macOS `~/Library/Caches/codeflow/rescue/`, Linux `$XDG_CACHE_HOME/codeflow/rescue/`, Windows `%LOCALAPPDATA%\\codeflow\\rescue\\`) — never to any git working tree. Auto-prune runs at CLI startup using `rescue.retention_days` (default 30) from `.codeflow/config/parallel-work/parallel-work-config.json`; bundles with `.pinned` markers are preserved indefinitely. A stderr banner surfaces the backlog once per 24h; suppress via `CODEFLOW_NO_RESCUE_BANNER=true` env var or `rescue.banner_enabled: false` in config.

The unified `codeflow test --mode full` command is the ONLY authorized test execution path for all PathFlow pipeline stages (WS-DEV, WS-QA). It routes to all configured test targets defined in `.codeflow/config/testing/test-config.json` and enforces first-match-wins per-target coverage rules.

→ See Section 9 for project file layout and Section 10 for data model

---

## 9. Project Structure

```text
.claude/                              # Claude Code configuration
├── CLAUDE.md                         # Team lead instructions (this file)
├── agents/                           # 8 teammate definitions (cf-*.md, 5-section format)
│   ├── cf-security.md                #   Persistent: security consultation
│   ├── cf-knowledge-layer.md         #   Persistent: WorkGraph, memory, DB
│   ├── cf-git-operations.md          #   Persistent: git operations
│   ├── cf-development.md             #   On-demand: code implementation
│   ├── cf-planning.md                #   On-demand: design, architecture
│   ├── cf-documentation.md           #   On-demand: documentation
│   ├── cf-review.md                  #   On-demand: independent review
│   └── cf-quality-assurance.md       #   On-demand: QA gate / test writer
├── skills/                           # 1 active + 6 on-demand skills
│   ├── cf-working-protocol/          #   Active: team lead cognitive procedures
│   ├── cf-rust-standards/            #   On-demand: Rust development standards
│   ├── cf-shell-standards/           #   On-demand: shell scripting standards
│   ├── cf-python-standards/          #   On-demand: Python scripting standards
│   ├── cf-markdown-standards/        #   On-demand: markdown documentation standards
│   ├── cf-sandbox-standards/         #   On-demand: sandbox bypass rules
│   └── cf-surrealdb-standards/       #   On-demand: SurrealDB development standards
├── hooks/                            # Hook scripts (project-specific hooks only)
│   └── project/                      #   Project hook scripts (if any)
├── commands/                         # 14 slash command definitions (cf-*.md)
├── memory/                           # Tier 2: domain-specific work context
└── settings.json                     # Permissions, hook config (22 hook entries as Rust CLI subcommands)

codeflow-cli/                         # Rust CLI workspace
├── Cargo.toml                        #   Workspace root (members: core, cli)
├── core/                             #   Library crate (codeflow-core): hooks, models, pathflow, session, worktree, coordination, transport, autorun, git
├── cli/                              #   Binary crate (codeflow-cli, bin: codeflow)
├── config/testing/test-config.json   #   Rust test configuration (coverage, business packages)
│   (core/src/ notable modules:)
│   ├── src/worktree/                 #   Worktree isolation (mod, paths, registry, setup, cleanup)
│   ├── src/coordination/            #   CRDT coordination (mod, loro, claims, sync, merge_queue)
│   ├── src/transport/               #   Git ref transport (gitref)
│   ├── src/autorun/                 #   Autorun workers (worker, orchestrator, config, batch)
│   └── src/git/conflict.rs          #   Merge conflict detection (ConflictResult, check_merge_conflicts)

.codeflow/                            # CodeFlow infrastructure
├── config/
│   ├── enforcement/                  # enforcement-policy.json
│   ├── parallel-work/                # parallel-work-config.json (worktree, sync, merge, claims)
│   └── pathflow/                     # pathflow-config.json (phases, stages, pipelines, rework limits)
├── scripts/
│   └── security/                     # Protection scripts (cf-protect-resources.sh, cf-promote-protection.sh, cf-reload-protection.sh + lib/)
├── testing/                          # Test suite (2,400+ tests)
└── docs/archived/skills/             # 9 archived skills (reference only)

.state/                               # Runtime state (partially gitignored)
├── db/codeflow.db                    # Tier 1: SurrealDB embedded (query interface)
├── ledger/                           # Tier 0: JSONL event logs (rebuild authority)
├── logs/
│   └── pathflow-events.jsonl         # Phase/stage transitions
├── runtime/                          # Active task, current session ID
│   └── peer-id                       # Unique peer identifier for CRDT sync
├── sentinels/                        # PathFlow sentinels (auto-created by hooks)
│   └── pathflow/{session-id}/        # Session-scoped sentinel files
├── session/                          # Session state (pathflow-active flags)
├── coordination/                     # CRDT coordination state
│   ├── state.loro                    #   Loro CRDT document (binary)
│   ├── sync-state.json               #   Sync daemon state
│   └── sync-daemon.pid               #   Sync daemon PID file
├── worktrees/                        # Worktree registry (symlinked into worktrees)
│   └── worktrees.yaml                #   Active worktree entries
├── autorun/                          # Autorun session state
│   ├── sessions/                     # Per-session autorun state
│   └── batches/                      # Batch execution records
├── backups/                          # State backups
└── registry/                         # Component registry

.git-worktrees/                       # Git worktrees (gitignored)
└── worktree-{SID}/                   # Per-session worktree
    ├── .state/                       #   Mixed symlink + local (see Section 5)
    └── (full working copy)           #   Independent checkout

project/                              # PROJECT.md, mission, tech-stack
project-management/                   # Tier 2: Human-readable work tracking
├── epics/                            # Epic markdown files
└── tracking/                         # Progress tracking
```

---

## 10. Memory

### Three-Tier Data Model

| Tier | Location | Purpose | Git Tracked |
|------|----------|---------|-------------|
| **0 (JSONL)** | `.state/ledger/*.jsonl` | Rebuild authority -- immutable, append-only event log | Yes |
| **1 (SurrealDB  embedded)** | `.state/db/codeflow.db` | Query interface -- fast indexed lookups | No |
| **2 (Markdown)** | `project-management/`, `.claude/memory/` | Human-readable derived views | Yes |

**Key principle:** If Tier 1 (database) is lost, rebuild from Tier 0 (JSONL). Tier 2 (markdown) is always derived from Tier 1. JSONL is the ultimate source of truth.

**CRDT coordination state** (`.state/coordination/state.loro`) is shared across worktrees (symlinked). SurrealDB uses `RetryConfig` with exponential backoff for parallel access from multiple worktrees.

**WorktreePaths resolution:** When running in a worktree, state file paths resolve via `WorktreePaths` (`codeflow-cli/core/src/worktree/paths.rs`). Shared state (db, coordination, logs, registry, backups) is symlinked to the main repo. Local state (ledger, runtime, session, sentinels) is per-worktree. The ledger is LOCAL since PR #221 — each worktree writes events to its own fragment files, preventing write conflicts during parallel sessions.

### Memory Operations

All memory operations are routed through the **cf-knowledge-layer** teammate. The lead does not interact with the database or JSONL directly.

| Operation | When | Who Executes |
|-----------|------|-------------|
| Detect active work | PF2-CONTEXT (session start) | cf-knowledge-layer |
| Load work context | Resume flow | cf-knowledge-layer |
| Register task | PF4-EXECUTE (PF4-TSK-01/02) | cf-knowledge-layer |
| Record progress | During PF4-EXECUTE | cf-knowledge-layer (via teammate reports) |
| Complete work (pre-PR) | PF6-COMPLETE | cf-knowledge-layer |
| Write session summary | PF7-END | cf-knowledge-layer |

### Session Context Files

| File | Purpose |
|------|---------|
| `.state/runtime/active-task.json` | Bridge file: current task for hook context (includes `worktree_path` field when in a worktree) |
| `.state/runtime/codeflow-env.sh` | Per-worktree env file (written by `codeflow -i`, NOT written when worktree mode is ON in SessionStart). Exports `CODEFLOW_SESSION_ID`, `CF_PROJECT_ROOT`, `CODEFLOW_WORKTREE_PATH`. In non-worktree mode (mode=disabled), a shared env file is written. When `CODEFLOW_MANAGED=true`, this file is per-worktree only. |
| `.state/logs/pathflow-events.jsonl` | Phase and stage transition log |
| `.state/session/{SID}/pathflow/pathflow-session-status.json` | Session lifecycle state (see below) |
| `.state/session/{SID}/pathflow/pathflow-team.json` | Team composition and process tracking (see below) |
| `.state/coordination/state.loro` | Loro CRDT document for claims, fencing tokens, merge queue (shared across worktrees) |
| `.state/coordination/sync-state.json` | Sync daemon state (last sync time, peer list) |
| `.state/coordination/sync-daemon.pid` | Sync daemon PID file |
| `.state/worktrees/worktrees.yaml` | Worktree registry — `WorktreeEntry` records (session_id, path, branch, status) |
| `.state/runtime/peer-id` | Unique peer identifier for CRDT sync |
| `.state/interactive/heartbeat-{SID}` | Liveness heartbeat file written by SessionStart when `CODEFLOW_MANAGED=true`. Used by `codeflow interactive status` and `cleanup` for PID liveness detection. |

**`CODEFLOW_MANAGED` env var:** Set to `true` by `codeflow -i` before exec'ing claude. When SessionStart detects this flag, it reads `CODEFLOW_SESSION_ID` from the environment (already generated by the CLI), skips worktree creation, skips shared env write, and skips PID env file early write. `detect_project_dir()` resolves via `CODEFLOW_WORKTREE_PATH` env var (step 1) or per-PID env file (step 4) — the shared `codeflow-env.sh` fallback (formerly step 5) has been removed.

**`pathflow-session-status.json` fields:**

| Field | Set by | Purpose |
|-------|--------|---------|
| `session_id` | SessionStart | Session identifier |
| `status` | SessionStart, PostToolUse | `created` → `pf-started` → `pf-in-progress` → `pf-complete` |
| `lead_pid` | SessionStart | Claude Code PID of lead (via `parent_id()` in hook). Used for teammate detection and stale sweep. |
| `team_name` | PostToolUse (TeamCreate) | Team name for config lookup |
| `last_completed_phase` | PostToolUse (checkpoint-complete) | Latest pf-N sentinel |
| `last_completed_stage` | PostToolUse (sentinel-write) | Latest ws-* sentinel |
| `source_at_start` | SessionStart | Original source (startup/resume/compact/clear) |
| `latest_source` | SessionStart (compact/resume/clear path) | Most recent source event |
| `latest_source_at` | SessionStart (compact/resume/clear path) | When latest source occurred |

**`pathflow-team.json` fields:**

| Field | Set by | Purpose |
|-------|--------|---------|
| `team_name` | handle_team_create | Team identifier |
| `lead_pid` | handle_team_create | Claude Code PID of lead (copy from status file) |
| `teammates` | handle_teammate_spawn + teammate SessionStart | Array of `{ name, pid, spawned_at }` per teammate |
| `teammate_spawned` | handle_teammate_spawn | Whether any teammate has been spawned |

**Teammate detection (tmux mode):** In tmux, teammates fire SessionStart (identical payload to lead). Detection uses `lead_pid` from the status file: `kill(lead_pid, 0)` — alive means lead is running, so this caller is a teammate. Dead means lead crashed, so this is a new lead. In non-tmux (in-process), teammates fire SubagentStart instead, so teammate detection is not needed.

---

## 11. Recovery

### Quick Commands

| Situation | Command |
|-----------|---------|
| Interrupted work | `/cf-resume` |
| Infrastructure issues | `/cf-doctor` |
| Show session state | `/cf-stack` |
| Show available commands | `/cf-help` |

### PathFlow Recovery

| Problem | Solution |
|---------|----------|
| Lost phase state | Check `.state/logs/pathflow-events.jsonl` for latest `phase_transition` event |
| Sentinel missing | Sentinels are auto-created by hooks. Verify the correct session ID at `.state/sentinels/pathflow/{session-id}/`. If truly missing, investigate the `codeflow hooks post-tool-use sentinel-write` PostToolUse hook pipeline -- do not create sentinels manually. |
| pathflow-session-status.json stale |  Check status field; if stuck, manually remove `.state/session/{SID}/pathflow/` directory via PF7 flow |
| Session record missing | Check `CODEFLOW_SESSION_ID` env var (set by `codeflow -i` and inherited in process environment) or the per-worktree `.state/runtime/codeflow-env.sh` for `CODEFLOW_SESSION_ID`, then query DB via cf-knowledge-layer |
| Stale worktree | Check `.state/worktrees/worktrees.yaml` for entries with status != active. Run `codeflow worktree cleanup` to remove stale entries and directories. |
| Orphaned worktree | If `.git-worktrees/worktree-{SID}/` exists but no registry entry, run `codeflow worktree prune` to reconcile. |
| Max worktrees reached | `WorktreeRegistry` enforces max 5 concurrent. Clean up completed sessions' worktrees first, then retry. |
| Claims stuck after crash | Claims have TTL. Run `claims::release_all()` for the crashed session's worktree_id, or wait for TTL expiry. |
| Rescue bundle exists for prior interrupted session | Run `codeflow rescue list` to see bundles. `codeflow rescue show <id>` for contents. If the branch is already on origin, bundle is typically redundant — use `codeflow rescue clean <id>`. To preserve a bundle indefinitely, run `codeflow rescue pin <id>`. |

### Teammate Recovery

| Problem | Solution |
|---------|----------|
| Teammate idle | Send message to wake: `SendMessage(type="message", recipient="cf-{role}", content="Status?")` |
| Teammate shut down unexpectedly | Respawn with same agent type and re-send context |
| Teammate context full | Recycle: shutdown --> respawn --> re-send state |
| Messages to shut-down teammate | Silently accepted but never delivered -- respawn first |

### Stuck Session

| Problem | Solution |
|---------|----------|
| Rework limits exceeded | Escalate to user. In autorun: mark task `blocked`, skip to PF7-END |
| Stage timeout (autorun) | Shutdown stuck agent, record timeout, mark `blocked`, PF7-END |
| Hook blocking unexpectedly | Read hook message, address the condition it reports |
| Team accidentally dissolved | Unrecoverable -- session must end, work restarted from scratch |
| Enforcement degraded | Log degradation, continue with instructions + task graph (advisory mode). → See Section 7 (Graceful Degradation) |
| Merge queue deadlock | Check `merge_queue::queue_len()`. If entries exist for completed/crashed sessions, dequeue stale entries. |
| Claim conflict blocking | scope_policy=soft: out-of-scope edits attempt CRDT claim — blocked if another worker holds the file. scope_policy=hard: out-of-scope edits blocked immediately. Release conflicting claims or coordinate file scope between workers. |

### Context Overflow Recovery

When Claude Code's context window overflows mid-session, the conversation continues from a new context. **Teammates are NOT affected** -- they run as independent processes in separate tmux panes and continue executing their current work. Only the lead's context is lost.

**Key distinction: context overflow vs teammate death**

Context overflow means the lead lost its conversation history -- NOT that teammates are dead. Teammates may be:

- **Alive and idle** -- waiting for the next message (most common after graceful compaction)
- **Alive and working** -- still executing their current task
- **Dead** -- only if the session was also forcefully terminated (a separate event from compaction)

Do NOT assume teammates are dead after context overflow. Verify before respawning.

**Worktree context:** When recovering in a worktree, verify the worktree path from the `CODEFLOW_WORKTREE_PATH` env var (inherited from `codeflow -i`) or the per-worktree `.state/runtime/codeflow-env.sh`. The shared `codeflow-env.sh` is NOT written when worktree mode is active — use the per-worktree copy or the env var directly. Check worktree health: `git worktree list` should show the worktree. If missing, check the registry and re-create if needed.

**Detection signals:**

- "This session is being continued from a previous conversation" preamble from Claude Code
- Lead has no memory of what work was in progress or what phase was reached
- Teammates may be alive (graceful compaction) or dead (session forcefully killed)
- Teammates are LIKELY still alive -- do NOT assume dead without verification

**L1 ENFORCED: NEVER respawn teammates after context overflow without verifying liveness.**

This is NOT advisory. Respawning without verification is a PROTOCOL VIOLATION equivalent to skipping PF3.

After context overflow, teammates run as independent processes and are ALMOST ALWAYS still alive. The lead's context was compacted -- teammates were NOT affected.

Mandatory verification sequence (NO EXCEPTIONS):

1. SendMessage to EVERY expected teammate: "Context overflow recovery. What is your current state?"
2. Wait 30 seconds for responses
3. ANY teammate that responds is ALIVE -- do NOT respawn
4. ONLY if a teammate does not respond after 30s, verify via tmux: `tmux list-panes -a`
5. ONLY respawn teammates confirmed dead via tmux (pane_dead=1 or pane not found)

FORBIDDEN:

- Assuming teammates are dead without messaging them first
- Respawning based on "no tmux panes" alone (in-process backend teammates don't use tmux)
- Creating duplicate -2 suffix teammates while originals are alive
- Skipping the 30-second wait period

**Recovery procedure:**

1. **Check pathflow state first** -- Read the `CODEFLOW_SESSION_ID` env var (inherited from `codeflow -i`) or the per-worktree `.state/runtime/codeflow-env.sh` for the SID (`CODEFLOW_SESSION_ID`). The shared `.state/runtime/codeflow-env.sh` is NOT written when running in a managed worktree session. Then check sentinels at `.state/sentinels/pathflow/{SID}/` and JSONL at `.state/logs/pathflow-events.jsonl` to determine current phase.

2. **Message teammates before assuming dead** -- Send a status message to each expected teammate:
   `SendMessage(type="message", recipient="cf-{role}", content="Context overflow recovery. What is your current state and what were you last working on?")`
   Wait for responses. If a teammate responds, it is alive -- no respawn needed.

   ⚠️ **Warning:** `SendMessage` does NOT verify liveness. A message to a dead teammate is silently accepted but never delivered. Lack of response does NOT prove death -- the teammate may be busy. Confirm via tmux before concluding a teammate is dead.

3. **Verify tmux only if unresponsive** -- If a teammate does not respond within ~30 seconds, check pane status: `tmux list-panes -a -F '#{pane_id} #{pane_dead}'`. Cross-reference pane IDs from `~/.claude/teams/{team-name}/config.json`.

4. **Respawn only confirmed-dead teammates** -- Respawn with the SAME name and agent type (see Name Preservation above). Include re-orientation context: current phase, task description, what work was in progress before overflow.

5. **Orient all teammates** -- Regardless of alive/dead status, send an orientation message to all active teammates summarizing the current phase and next action. Alive teammates that were waiting may need to be re-directed.

6. **Continue from current phase** -- Map sentinel state to phase and resume. Do not restart PF1-INIT -- the team, sentinels, and session state are intact.

7. **Resume task tracker registration** -- 🔒 **L1 ENFORCED: Task tracker registration MUST resume after context overflow.**

   The checkpoint system creates phase sentinels ONLY when ALL expected tasks for a phase are registered and completed in the task tracker. Missing registrations block sentinel creation and downstream phase gates. Skipping task tracker registration after context overflow is a **PROTOCOL VIOLATION** equivalent to skipping PF3-CLASSIFY.

   **Mandatory post-overflow task tracker checklist:**

   | Step | Action | Tool | Verify |
   |------|--------|------|--------|
   | 1 | Read checkpoint state | Read `.state/session/{SID}/pathflow/pathflow-phase-tasks.json` | Shows registered/completed/missing per phase |
   | 2 | Identify current phase | Check sentinel files at `.state/sentinels/pathflow/{SID}/` | Latest `pf-N` sentinel = last completed phase |
   | 3 | Backfill completed phases | For each completed task NOT in task tracker: `TaskCreate` then immediately `TaskUpdate` to `completed` | Checkpoint JSON shows all prior tasks as registered+completed |
   | 4 | Register current phase tasks | `TaskCreate` for EVERY `PF{N}-TSK-{NN}` in the current phase | All tasks appear in `TaskList` output |
   | 5 | Verify sentinel pipeline | After backfill, confirm checkpoint system resumes creating sentinels | New sentinel files appear for completed phases |

   ⛔ **FORBIDDEN after context overflow:**
   - Skipping task tracker registration because "previous tasks were already done"
   - Registering only the current task while ignoring earlier unregistered tasks
   - Clubbing multiple `PF{N}-TSK-{NN}` entries into a single TaskCreate
   - Proceeding past a phase gate without verifying its sentinel exists
   - Assuming the checkpoint system will "catch up" without explicit backfill

   Continue following the mandatory task tracker mirroring rules (Section 7: Task Tracker Mirroring) for all remaining phases.

**Continuation preamble detection:** When Claude Code reports "continued from previous conversation", immediately check the `CODEFLOW_SESSION_ID` env var (inherited from `codeflow -i` process environment — persists across context overflow) and check sentinels before sending any teammate messages. The per-worktree `.state/runtime/codeflow-env.sh` is also available as a fallback. Do NOT assume all teammates are dead.
