# V3 Session Lifecycle Analysis

> Deep analysis of the CodeFlow Specification V3 session lifecycle, skill operation sequences, hook firing order, and cross-skill dependency chains. Source material: `/codeflow-specification-v3/` repository.

---

## 1. Session Start Sequence (Ordered Steps)

The session start is a deterministic sequence of hooks followed by skill-driven operations:

### Step 1: SessionStart Hooks Fire (automatic, ordered)

| Order | Hook | Purpose | Blocking? |
|:-----:|------|---------|-----------|
| 1 | `cf-session-start-cleanup.sh` | Clean expired sentinels from `/tmp/claude/managed/sentinels/`, remove stale state | No |
| 2 | `cf-session-start-instructions.sh` | Display behavioral instructions, verify CRDT state, check offline status | No |
| 3 | `cf-session-start-logging.sh` | Initialize session log (JSONL), create session record | No |

### Step 2: Session Record Created

- Generate `session-{ulid}` identifier
- Insert into `sessions` table (SQLite Tier 1)
- Append `session_start` event to `sessions.jsonl` (Tier 0)
- Log to JSONL ledger

### Step 3: Active Work Detection

- Skill: `cf-memory-management:detect-active-work` (ENF-L3 Advisory)
- Query: `SELECT id, topic, branch, task_id FROM active_work WHERE status = 'in_progress'`
- For each active work: fetch recent progress events (last 5)
- Decision:
  - No active work found -> Continue to new work
  - Single active work -> Offer to resume
  - Multiple active work -> Present selection menu

### Step 4: Meta-Awareness (cf-working-protocol)

- Operation: `cf-working-protocol:meta-awareness`
- Display meta-awareness indicator
- Assess current context state
- Acknowledge knowledge limitations
- Check for active work context

### Step 5 (if resuming): Load Work Context

- Skill: `cf-memory-management:load-work-context`
- Query work details joining `active_work`, `tasks`, `epics`
- Load recent progress events
- Update session with loaded context
- Display context summary

### Step 6: UserPromptSubmit Hook (on first prompt)

| Order | Hook | Purpose |
|:-----:|------|---------|
| 1 | `cf-user-prompt-submit-context.sh` | Branch awareness, inject workflow instructions, task reminder |
| 2 | `cf-user-prompt-submit-logging.sh` | Log user prompt content |

---

## 2. Session End Sequence (Ordered Steps)

### Step 1: Stop Hook Fires (blocking)

| Order | Hook | Purpose | Blocking? |
|:-----:|------|---------|-----------|
| 1 | `cf-stop-verify-work.sh` | Post-Completion Verification (PCV) enforcement | **Yes** |
| 2 | `cf-stop-logging.sh` | Log stop event | No |

The Stop hook is unique:
- Returns JSON: `{"decision": "allow|block", "continue": true, "reason": "..."}`
- If `decision: "block"` -> session continues, agent must complete verification
- After max retries -> allows stop to prevent deadlock
- **Timeouts default to BLOCK** (conservative - err on side of more work)

### PCV Verification Tiers

| Tier | Requirements |
|------|-------------|
| 1 (Simple) | `verify-work` marker + TIER indicator |
| 2 (Standard) | Tier 1 + ARTIFACTS section + VERIFICATION section |
| 3 (Complex) | Tier 2 + ADVERSARIAL section |

### Step 2: SessionEnd Hooks Fire (automatic, ordered)

| Order | Hook | Purpose |
|:-----:|------|---------|
| 1 | `cf-session-end-cleanup.sh` | Archive session state, final cleanup, release claims |
| 2 | `cf-session-end-logging.sh` | Finalize session log, write duration, close JSONL |

### Step 3: Session Record Finalized

- Update `sessions` table: `ended_at`, `duration`, `status = 'completed'`
- Append `session_end` event to JSONL ledger
- Generate context summary for potential future resume
- Status transitions: `active -> completed -> archived`

---

## 3. Skill Operation Sequences (Per Skill)

### 3.1 cf-working-protocol (INLINE - always loaded, ~1,200 tokens)

| # | Operation | Enforcement | When |
|---|-----------|-------------|------|
| 1 | meta-awareness | ENF-L3 Advisory | Session start, periodically |
| 2 | think-and-act | ENF-L1 (PAC-5) + ENF-L3 | Before significant actions |
| 3 | decide | ENF-L3 Advisory | Decision points |
| 4 | respond-organized | ENF-L3 Advisory | User communication |
| 5 | research-quality | ENF-L2 Stop | Before making claims |
| 6 | verify-work | ENF-L2 Stop | Before agent completion |
| 7 | parallelize-work | ENF-L1 + ENF-L3 | Complex multi-part tasks |

**Key**: This is the only INLINE skill. It is always in context. All other skills are FORKED.

### 3.2 cf-memory-management (FORKED, ~1,500 tokens)

Full work lifecycle sequence:

```
detect-active-work  (session start)
       |
load-work-context   (if resuming)
       |
search-related-work (before creating worktree / starting conflicting work)
       |
evaluate-context    (before making changes)
       |
begin-work          (starting work - registers active_work, task_id NOT NULL)
       |
record-work-progress (repeatedly during work - milestones, decisions, file mods)
       |
complete-work       (before commit - archives context, creates sentinel)
       |
manage-memory-lifecycle (periodic maintenance / /cf-cleanup)
```

**Critical chain**: `begin-work` -> [work happens] -> `complete-work` -> `cf-git-workflow:create-commit`

### 3.3 cf-git-workflow (FORKED, ~1,100 tokens)

| # | Operation | Enforcement | Prerequisite |
|---|-----------|-------------|-------------|
| 1 | create-branch | ENF-L1 Sentinel | None |
| 2 | create-commit | ENF-L1 Sentinel | **cf-memory-management:complete-work** |
| 3 | create-pull-request | ENF-L1 Sentinel | **cf-security-management:sandbox-check** |
| 4 | create-worktree | ENF-L1 Sentinel | cf-memory-management:search-related-work |
| 5 | cleanup-worktrees | ENF-L1 Sentinel | None |
| 6 | sync-remote | ENF-L1 Sentinel | **cf-security-management:sandbox-check** |
| 7 | merge-branch | ENF-L1 Sentinel | cf-security-management:sandbox-check |
| 8 | review-changes | None | None (read-only) |
| 9 | check-branch-status | None | None (read-only) |
| 10 | rebase-interactive | ENF-L1 Sentinel | cf-memory-management:search-related-work |

**Typical development flow**:
```
create-branch -> [development work] -> create-commit -> sync-remote -> create-pull-request
```

### 3.4 cf-security-management (FORKED, ~1,000 tokens)

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | sandbox-check | ENF-L1 Sentinel | Gate for all network operations |
| 2 | validate-protected-resource | ENF-L1 Sentinel | Check if resource is protected |
| 3 | check-permissions | ENF-L1 Sentinel | Verify permission for operation |
| 4 | request-permission | ENF-L2 Stop | Request elevated permission from user |

**Internal procedures** (not separate operations):
- stage-protected-edit -> copy to `/tmp/claude/managed/protected-edits/`
- apply-protected-edit -> validate + backup + apply
- sync-settings-templates -> propagate hooks to all permission templates

### 3.5 cf-task-management (FORKED, ~800 tokens)

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | understand-request | ENF-L3 Advisory | Parse user request |
| 2 | classify-work | ENF-L3 Advisory | Determine area/type/domain |
| 3 | ensure-work-registered | ENF-L1 Sentinel | **Guarantee task_id NOT NULL** |
| 4 | create-epic | ENF-L1 Sentinel | Insert epic into work graph |
| 5 | update-epic | ENF-L1 Sentinel | Modify epic status |
| 6 | create-task | ENF-L1 Sentinel | Create task linked to epic |
| 7 | update-task | ENF-L1 Sentinel | Modify task status |
| 8 | query-tasks | None | Query by criteria (read-only) |

**Informal work flow**:
```
classify-work -> ensure-work-registered -> (creates epic if needed) -> (creates task) -> returns task_id
```

### 3.6 cf-db-operations (FORKED, ~600 tokens)

Foundation skill. All database CRUD goes through this:

| # | Operation | Agent Permissions |
|---|-----------|------------------|
| 1 | epic-create | cf-planner only |
| 2 | epic-update | cf-planner only |
| 3 | task-create | cf-planner, Main Agent (informal) |
| 4 | task-update | All agents (status), cf-planner (all) |
| 5 | memory-store | All (own domain only) |
| 6 | memory-query | All (read-only) |
| 7 | session-record | System (hooks) |
| 8 | log-append | System (hooks) |

Every write syncs across three tiers: JSONL (Tier 0) -> SQLite (Tier 1) -> Markdown (Tier 2).

### 3.7 cf-documentation-standards (FORKED, ~600 tokens)

| # | Operation | Enforcement |
|---|-----------|-------------|
| 1 | apply-standard | ENF-L1 Sentinel |
| 2 | lint-file | ENF-L1 Sentinel |
| 3 | validate-structure | ENF-L1 Sentinel |
| 4 | check-links | None |
| 5 | generate-toc | None |

### 3.8 cf-script-standards (FORKED, ~800 tokens)

| # | Operation | Enforcement |
|---|-----------|-------------|
| 1 | lint-shell | ENF-L1 Sentinel |
| 2 | lint-python | ENF-L1 Sentinel |
| 3 | apply-shell-standards | ENF-L1 Sentinel |
| 4 | apply-python-standards | ENF-L1 Sentinel |
| 5 | ensure-test-coverage | ENF-L3 Advisory |

### 3.9 cf-code-exploration (FORKED, ~500 tokens)

| # | Operation | Enforcement |
|---|-----------|-------------|
| 1 | select-search-strategy | ENF-L3 Advisory |
| 2 | load-compressed-context | ENF-L3 Advisory |
| 3 | search-symbol | ENF-L3 Advisory |
| 4 | analyze-dependencies | ENF-L3 Advisory |
| 5 | navigate-to-definition | ENF-L1 Sentinel |
| 6 | find-all-references | ENF-L1 Sentinel |

### 3.10 cf-testing-workflow (FORKED, ~700 tokens)

| # | Operation | Enforcement |
|---|-----------|-------------|
| 1 | run-tests | ENF-L1 Sentinel |
| 2 | check-coverage | None |
| 3 | analyze-failures | None |
| 4 | suggest-tests | ENF-L3 Advisory |
| 5 | run-affected-tests | None |

### 3.11 cf-model-orchestrator (FORKED, ~1,800 tokens)

Advanced skill. Deferred for v1. Handles delegation to external AI models via tmux sessions.

---

## 4. Cross-Skill Dependency Map

### Sentinel Prerequisite Chains (Critical Path)

These are enforced by PreToolUse hooks and cannot be bypassed:

```
git commit  requires:  cf-memory-management:complete-work  (sentinel, TTL 600s)
                       which requires: work must be registered via begin-work

gh pr create  requires:  cf-git-workflow:create-pull-request  (sentinel)
              which requires:  cf-security-management:sandbox-check  (sentinel)

git push  requires:  cf-security-management:sandbox-check  (sentinel, TTL 600s)

Edit/Write to protected paths  requires:  cf-security-management:stage-protected-edit -> apply-protected-edit
```

### Complete Dependency Graph

```
cf-working-protocol  (INLINE, no dependencies)
    |
    +-- All other skills depend on its procedures (think-and-act, verify-work)

cf-db-operations  (Foundation - no skill dependencies)
    |
    +-- cf-memory-management  (depends on cf-db-operations for all CRUD)
    |       |
    |       +-- cf-git-workflow:create-commit  (requires complete-work sentinel)
    |       +-- cf-git-workflow:create-worktree  (requires search-related-work)
    |       +-- cf-git-workflow:rebase-interactive  (requires search-related-work)
    |
    +-- cf-task-management  (depends on cf-db-operations for CRUD)
    |       |
    |       +-- cf-memory-management:begin-work  (requires task_id from ensure-work-registered)
    |
    +-- cf-security-management  (depends on cf-db-operations for config queries)
            |
            +-- cf-git-workflow:create-pull-request  (requires sandbox-check)
            +-- cf-git-workflow:sync-remote  (requires sandbox-check)
            +-- cf-git-workflow:merge-branch  (requires sandbox-check)

cf-git-workflow  (depends on cf-memory-management, cf-security-management)
    |
    +-- cf-testing-workflow  (depends on git branch context)

cf-script-standards  (no dependencies)
cf-documentation-standards  (no dependencies)
cf-code-exploration  (no dependencies, directory-only)
```

### Sentinel Registry (Tool -> Skill -> Operation mapping)

| ID | Tool | Pattern | Required Skill | Required Operation | TTL |
|----|------|---------|----------------|--------------------|-----|
| bash-git-commit | Bash | `^git\s+commit` | cf-git-workflow | create-commit | 600s |
| bash-git-push | Bash | `^git\s+push` | cf-security-management | sandbox-check | 300s |
| bash-gh-pr-create | Bash | `^gh\s+pr\s+create` | cf-git-workflow | create-pull-request | 600s |
| bash-gh-pr-merge | Bash | `^gh\s+pr\s+merge` | cf-git-workflow | merge-branch | 300s |
| edit-protected-config | Edit | `^\.codeflow/config/` | cf-security-management | modify-protected-resource | 300s |
| write-protected-config | Write | `^\.codeflow/config/` | cf-security-management | modify-protected-resource | 300s |
| edit-hooks | Edit | `^\.claude/hooks/` | cf-security-management | modify-security-hooks | 300s |
| bash-sandbox-bypass | Bash | `dangerouslyDisableSandbox` | cf-security-management | approve-sandbox-bypass | 120s |
| task-spawn-agent | Task | `.*` | cf-task-management | validate-delegation | 600s |

---

## 5. Mandatory vs Optional Operations

### Mandatory at Session Start

| Operation | Trigger | Enforcement |
|-----------|---------|-------------|
| SessionStart hooks (3) | Automatic | System |
| Session record creation | Automatic | System |
| detect-active-work | UserPromptSubmit hook | ENF-L3 Advisory |
| meta-awareness | cf-working-protocol (inline) | ENF-L3 Advisory |

### Mandatory During Work

| Operation | Trigger | Enforcement |
|-----------|---------|-------------|
| ensure-work-registered | Before any Edit/Write/Bash | ENF-L1 Sentinel (task-sentinel hook) |
| begin-work | Before modifications | ENF-L1 Sentinel |
| complete-work | Before git commit | ENF-L1 Sentinel (bash-sentinel hook) |
| sandbox-check | Before network ops | ENF-L1 Sentinel |
| verify-work | Before stopping | ENF-L2 Stop hook |

### Mandatory at Session End

| Operation | Trigger | Enforcement |
|-----------|---------|-------------|
| Stop hook (PCV) | Agent signals completion | ENF-L2 (blocking) |
| SessionEnd hooks (2) | Automatic | System |
| Session record finalization | Automatic | System |

### Optional / Advisory Operations

| Operation | Trigger | Enforcement |
|-----------|---------|-------------|
| record-work-progress | After milestones | ENF-L3 Advisory (PostToolUse reminder) |
| evaluate-context | Before changes | ENF-L3 Advisory |
| search-related-work | Before worktree | ENF-L1 (only when creating worktree) |
| decide | Decision points | ENF-L3 Advisory |
| respond-organized | User communication | ENF-L3 Advisory |
| research-quality | Making claims | ENF-L2 Stop |

---

## 6. Recurring Operations (Things That Repeat During a Session)

### Per User Prompt

```
UserPromptSubmit hooks fire:
  1. cf-user-prompt-submit-context.sh  (inject workflow instructions)
  2. cf-user-prompt-submit-logging.sh  (log prompt)
```

### Per Tool Call (repeated for every tool invocation)

```
PreToolUse hooks fire:  (can BLOCK)
  1-11 hooks depending on tool type
     |
  [Tool executes]
     |
PostToolUse hooks fire:  (advisory only)
  1-6 hooks depending on tool type
```

### Per Significant Action

- `cf-working-protocol:think-and-act` - Before any significant action (PAC-5 for protected ops)
- `cf-memory-management:record-work-progress` - After milestones, decisions, file batch changes

### Heartbeat Pattern (Claim Renewal)

Occurs in `cf-post-tool-use-memory-progress.sh`:
- **Operation-based**: Every 10 tool operations (Edit, Write, Bash)
- **Time-based**: Every 5 minutes (fallback)
- **Buffer-based**: When claim `expires_at < NOW() + 120 seconds`
- Renews claims via `cf-claim-renew.py`, extends by CLAIM_TTL (600s)

---

## 7. Complete Typical Development Session Flow

```
SESSION START
  |
  [SessionStart hooks: cleanup -> instructions -> logging]
  |
  [Session record created: session-{ulid}]
  |
  [detect-active-work: query active_work table]
  |
  +-- Resume? -> load-work-context
  |
USER PROMPT: "Fix the login button"
  |
  [UserPromptSubmit hooks: context -> logging]
  |
  [cf-working-protocol:meta-awareness]
  |
  [cf-working-protocol:think-and-act]
  |
  [cf-task-management:classify-work]
  |   area=FRT, type=FIX, domain=AUTH
  |
  [cf-task-management:ensure-work-registered]
  |   Creates/finds epic, creates task: FRT-TSK-FIX-AUTH-042
  |
  [cf-git-workflow:create-branch]
  |   Creates: fix/auth-login-button
  |
  [cf-memory-management:begin-work]
  |   Registers active_work with task_id (NOT NULL)
  |   Creates work agreement markdown
  |
  [cf-code-exploration:select-search-strategy]
  |   (optional, if exploring code)
  |
WORK LOOP (repeated):
  |
  [PreToolUse hooks validate every Edit/Write/Bash]
  |   security -> protected-resource -> edit-write -> sentinels
  |
  [Edit/Write tools execute code changes]
  |
  [PostToolUse hooks fire after every tool]
  |   skill sentinels -> instructions -> memory-progress -> logging
  |
  [cf-memory-management:record-work-progress] (periodically)
  |
  [Heartbeat: claim renewal every ~10 ops or 5 min]
  |
COMPLETION:
  |
  [cf-testing-workflow:run-tests]
  |
  [cf-memory-management:complete-work]
  |   Archives context, creates sentinel for commit
  |
  [cf-git-workflow:create-commit]
  |   Requires complete-work sentinel (TTL 600s)
  |   Commit format: type(scope): description
  |
  [cf-security-management:sandbox-check]
  |   Creates sentinel for network operations
  |
  [cf-git-workflow:sync-remote]
  |   git push -u origin fix/auth-login-button
  |
  [cf-git-workflow:create-pull-request]
  |   Requires sandbox-check sentinel
  |
  [cf-working-protocol:verify-work]
  |   PCV tier determination + verification output
  |
STOP:
  |
  [Stop hook: cf-stop-verify-work.sh -> blocks if PCV incomplete]
  [Stop hook: cf-stop-logging.sh -> log stop event]
  |
SESSION END:
  |
  [SessionEnd hooks: cleanup -> logging]
  |
  [Session record finalized: status=completed, duration calculated]
```

---

## 8. Hook Firing Order Summary

### Complete Hook Execution Sequence

| Phase | Hook Type | Hook Count | Blocking? |
|-------|-----------|:----------:|-----------|
| Session Start | SessionStart | 3 | No |
| Each Prompt | UserPromptSubmit | 2 | No |
| Before Each Tool | PreToolUse | up to 11 | **Yes** (exit 2) |
| After Each Tool | PostToolUse | up to 6 | No |
| Agent Completion | Stop | 2 | **Yes** (JSON decision) |
| Session End | SessionEnd | 2 | No |

### Total hooks: 26 (21 core + 5 logging)

### Hook Chain Semantics

- **First block wins**: If any PreToolUse hook exits with code 2, chain stops
- **Errors are logged but don't block**: Non-2 exit codes continue chain
- **Timeouts are logged but don't block**: Timed-out hooks continue chain
- **Order matters**: Hooks execute in array order from settings.json
- **Stop hook is special**: Timeouts/errors default to BLOCK (conservative)

---

## 9. Key Findings for PathFlow Design

### Finding 1: Two Enforcement Levels Define the Skeleton

The session has two categories of operations:
- **ENF-L1 Sentinel** (blocking, cannot skip): These form the mandatory skeleton. They are enforced by PreToolUse hooks that check for sentinel files in `/tmp/claude/managed/sentinels/`.
- **ENF-L3 Advisory** (non-blocking, recommended): These are guidance that can be skipped but shouldn't be.

**PathFlow implication**: The mandatory path is defined by sentinel requirements. Everything else is advisory.

### Finding 2: The Critical Chain is: Task -> Work -> Complete -> Commit -> Push -> PR

Every development session must traverse this chain:
1. `cf-task-management:ensure-work-registered` (task_id NOT NULL)
2. `cf-memory-management:begin-work` (register active_work)
3. `cf-memory-management:complete-work` (create commit sentinel)
4. `cf-git-workflow:create-commit` (requires complete-work sentinel)
5. `cf-security-management:sandbox-check` (create network sentinel)
6. `cf-git-workflow:sync-remote` (requires sandbox-check sentinel)
7. `cf-git-workflow:create-pull-request` (requires sandbox-check sentinel)

### Finding 3: Sentinel TTL Creates Temporal Coupling

Sentinels expire (default 600s = 10 min). This means:
- `complete-work` must be invoked within 10 minutes of `create-commit`
- `sandbox-check` must be invoked within 10 minutes of network operations
- If a sentinel expires, the prerequisite skill must be re-invoked

**PathFlow implication**: Path segments have time constraints.

### Finding 4: Three Parallel Skill Groups

Skills can be grouped by when they're needed:
- **Always present**: cf-working-protocol (INLINE)
- **Session bookends**: cf-memory-management (detect/begin/complete), cf-db-operations (session records)
- **During work**: cf-task-management, cf-git-workflow, cf-security-management, cf-code-exploration, cf-testing-workflow
- **Quality gates**: cf-script-standards, cf-documentation-standards
- **Advanced**: cf-model-orchestrator (deferred)

### Finding 5: Hook Lifecycle Wraps Everything

Every tool call is sandwiched by hooks:
- 11 possible PreToolUse hooks (blocking)
- 6 possible PostToolUse hooks (advisory)

The hook system is the primary enforcement mechanism. Skills are invoked by the agent; hooks enforce that skills were invoked.

### Finding 6: Three-Tier Data Model Flows Through Everything

Every write operation flows: JSONL (Tier 0, append-only authority) -> SQLite (Tier 1, operational) -> Markdown (Tier 2, presentation). This is enforced by `cf-db-operations`.

### Finding 7: Agent Handoffs Are Knowledge Layer Mediated

Agents never communicate directly. All context sharing happens through the Knowledge Layer (database). The handoff chain is:
```
cf-planner -> [Knowledge Layer] -> cf-developer -> [Knowledge Layer] -> cf-reviewer -> [Knowledge Layer] -> cf-qa -> [Knowledge Layer] -> cf-ops
```

### Finding 8: Command Routing Determines Agent

Each `/cf-*` command maps to exactly one agent type. The main agent orchestrates routing:
- `/cf-plan` -> cf-planner
- `/cf-develop` -> cf-developer
- `/cf-review` -> cf-reviewer
- `/cf-test` -> cf-qa
- `/cf-ship` -> cf-ops
- `/cf-deploy` -> cf-ops
- `/cf-document` -> cf-documenter
- `/cf-help` -> cf-support

### Finding 9: Session State Machine

```
active -> paused -> completed -> archived
  |                     ^
  +---> crashed --------+  (resume & complete)
```

Sessions can be paused (context compaction), crashed (unexpected termination), or completed normally. All states eventually reach `archived`.

### Finding 10: Work Claim System Enables Multi-Agent Safety

The CRDT-based claim system (Loro) prevents file conflicts:
- Claims have TTL (600s) with heartbeat renewal
- Fencing tokens provide conflict detection
- Claims are renewed every 10 tool operations or 5 minutes
- CRDT auto-merges on git sync

---

## 10. Operation Count Summary

| Skill | Operations | Sentinel-Enforced | Advisory |
|-------|:----------:|:-----------------:|:--------:|
| cf-working-protocol | 7 | 1 (PAC-5) | 6 |
| cf-git-workflow | 10 | 7 | 0 (+3 read-only) |
| cf-security-management | 4 | 3 | 0 (+1 ENF-L2) |
| cf-memory-management | 8 | 4 | 4 |
| cf-task-management | 8 | 5 | 2 (+1 read-only) |
| cf-db-operations | 8 | 7 | 0 (+1 read-only) |
| cf-documentation-standards | 5 | 3 | 0 (+2 no enforcement) |
| cf-script-standards | 5 | 4 | 1 |
| cf-code-exploration | 6 | 2 | 4 |
| cf-testing-workflow | 5 | 1 | 1 (+3 no enforcement) |
| cf-model-orchestrator | 12 | 7 | 0 (+5 no enforcement) |
| **TOTAL** | **78** | **44** | **18** (+16 none) |

---

## 11. Enforcement Level Reference

| Level | Name | Mechanism | Can Block? |
|-------|------|-----------|------------|
| ENF-L1 | Sentinel | PreToolUse hooks check for sentinel files | **Yes** (exit 2) |
| ENF-L2 | Stop | Stop hook verifies completion | **Yes** (JSON block) |
| ENF-L3 | Advisory | PostToolUse hooks suggest actions | No |
| SEC-OS | OS Foundation | Kernel immutable flags | Yes (kernel) |
| SEC-L0 | Enterprise | System-level Claude Code settings | Yes |
| SEC-L2 | Blocking Hooks | PreToolUse/Stop | Yes |
| SEC-L3 | Project Settings | .claude/settings.json | No (bypassable) |
| SEC-L4 | Git Hooks | pre-commit/commit-msg | No (--no-verify blocked at SEC-L2) |
