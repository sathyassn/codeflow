---
name: cf-model-orchestrator
description: Provides multi-model delegation via T1 (one-shot) and T3 (persistent tmux sessions). Enables parallel execution with external AI models (GPT-4, Gemini, local). Use when delegating tasks to external models.
context: fork
agent: cf-general-purpose
---

# Model Orchestrator Skill

## Type

**Procedural** - Provides step-by-step procedures for orchestrating external model CLIs through non-interactive commands (T1) or persistent tmux sessions (T3).

## Purpose

**Centralizes multi-model orchestration, enabling Claude to delegate tasks to external AI models with consistent patterns, output parsing, and resource management.**

## Scope Clarification

```text
┌─────────────────────────────────────────────────────────────────────┐
│ CF-MODEL-ORCHESTRATOR: External Model Delegation                     │
│                                                                       │
│ What this skill DOES:                                                │
│   - Delegate tasks to EXTERNAL models (GPT-4, Gemini, Ollama)       │
│   - Manage T1 (one-shot) and T3 (persistent tmux) execution         │
│   - Coordinate parallel work with external models                    │
│   - Track sessions in active_work table                             │
│                                                                       │
│ What this skill does NOT do:                                         │
│   - Handle /cf-autorun (that's the Go CLI orchestrator)             │
│   - Spawn additional Claude instances (that's autorun)              │
│   - Use autorun_sessions/autorun_workers tables (that's autorun)    │
└─────────────────────────────────────────────────────────────────────┘

CF-AUTORUN (Go CLI) vs CF-MODEL-ORCHESTRATOR (This Skill):

| Aspect | CF-Autorun | This Skill |
|--------|-----------|------------|
| Controller | Go CLI | Claude (this agent) |
| Workers | Claude instances | External models |
| DB Tables | autorun_* | active_work |
| tmux naming | ar-{session}-{worker} | model-{ulid} |
| Use case | Batch Claude tasks | Delegate to GPT/Gemini |
```

## Responsibilities

- Non-interactive model execution (T1 - one-shot commands)
- Persistent session management (T3 - tmux-based)
- Output capture and parsing across model CLIs
- Parallel execution coordination with scope conflict detection
- Session tracking via DB and cleanup
- Quality validation of model responses
- Recording model decisions in memory events
- NOT: Direct code execution (that's the external model)
- NOT: Worktree management (that's cf-git-workflow)
- NOT: Work lifecycle (that's cf-memory-management)

## Decision Tree

```text
First time using?
└── 🔧 verify-prerequisites (check CLIs installed)

Delegating a task?
├── Build prompt first → 🔧 construct-prompt
│
├── One-shot task (T1)? (read-only, no work tracking needed)
│   ├── Single model → 🔧 execute-with-model
│   ├── Multiple models → 🔧 execute-multi-model
│   ├── After execution → 🔧 validate-output
│   └── Record decision? → cf-memory-management:record-work-progress
│
└── Persistent session (T3)? (requires work tracking)
    ├── Has task_id? → cf-memory-management:begin-work FIRST
    │   └── No task_id? → cf-task-management:ensure-work-registered
    ├── Start session → 🔧 spawn-session
    │   └── Need isolation? → cf-git-workflow:create-worktree first
    ├── Send task → 🔧 send-task
    ├── Check progress → 🔧 poll-output
    │   └── Output received? → cf-memory-management:record-work-progress
    ├── Parallel work → 🔧 orchestrate-parallel
    └── End session with changes?
        ├── cf-memory-management:complete-work FIRST
        └── 🔧 terminate-session

Session management?
├── List active → 🔧 list-sessions (queries DB)
├── Check conflicts → 🔧 check-scope-conflict
└── Cleanup stale → 🔧 cleanup-stale-sessions
```

## Work Tracking Integration

```text
T1 (One-shot) - Typically read-only analysis:
  - No begin-work required (quick, no modifications)
  - Record decisions via record-work-progress if significant
  - No complete-work needed

T3 (Persistent) - File modifications expected:
  - MUST have task_id (ensures tracking)
  - MUST call begin-work before session work
  - SHOULD call record-work-progress for milestones
  - MUST call complete-work before terminating if changes made
  - Session linked to work_id for continuity
```

## Autorun Context Awareness

```text
If running INSIDE an autorun worker (detected via $AUTORUN_SESSION_ID):

  Environment Variables Available:
    AUTORUN_SESSION_ID    - Parent autorun session
    AUTORUN_WORKER_ID     - This worker's ID
    AUTORUN_TASK_ID       - Task being worked on
    AUTORUN_ACCEPTANCE    - Acceptance criteria

  Behavior Modifications:
    - T1 operations: Proceed normally
    - T3 operations: WARN about nested sessions
      (spawning tmux inside autorun tmux can be complex)
    - Prefer T1 for delegation when in autorun context
    - Record delegations in parent task's memory events

  Check for autorun context:
    if [[ -n "${AUTORUN_SESSION_ID:-}" ]]; then
      # Running inside autorun worker
      AUTORUN_MODE=true
    fi
```

## Execution Tiers

| Tier | Name | Use Case | Mechanism |
|------|------|----------|-----------|
| T1 | One-shot | Quick tasks, analysis | CLI command with --full-auto |
| T3 | Persistent | Multi-turn, long-running | tmux session |

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | verify-prerequisites | None | Check model CLIs available |
| 2 | construct-prompt | ENF-L3 Advisory | Build context-rich prompt |
| 3 | select-model | ENF-L3 Advisory | Choose appropriate model |
| 4 | execute-with-model | ENF-L1 Sentinel | T1: One-shot execution |
| 5 | execute-multi-model | ENF-L1 Sentinel | T1: Multi-model consensus |
| 6 | validate-output | None | Output quality check |
| 7 | spawn-session | ENF-L1 Sentinel | T3: Create persistent session |
| 8 | send-task | ENF-L1 Sentinel | T3: Send task to session |
| 9 | poll-output | None | T3: Check session output |
| 10 | orchestrate-parallel | ENF-L1 Sentinel | T3: Run parallel sessions |
| 11 | terminate-session | ENF-L1 Sentinel | T3: Clean up session |
| 12 | list-sessions | None | List active sessions |
| 13 | check-scope-conflict | None | Detect scope overlap |
| 14 | cleanup-stale-sessions | ENF-L1 Sentinel | Remove dead sessions |

## Operation Details

### 🔧 verify-prerequisites

```text
When: Before any model orchestration
Purpose: Verify required CLIs are installed
Enforcement: None

Checks:
  | Model | CLI | Verification |
  |-------|-----|--------------|
  | GPT-4/Codex | codex | codex --version |
  | Gemini | gemini | gemini --version |
  | Ollama | ollama | ollama list |

  Note: Claude CLI is NOT listed - use autorun for Claude delegation

Procedure:
  1. Check each model CLI availability
  2. Check tmux for T3 support
  3. Detect if running in autorun context ($AUTORUN_SESSION_ID)
  4. If in autorun: WARN about T3 limitations
  5. Report available/missing

Output:
  available_models: [{name, version}...]
  missing: [{name, install_command}...]
  t3_available: true | false (tmux present)
  in_autorun_context: true | false
```

### 🔧 construct-prompt

```text
When: BEFORE any task delegation (T1 or T3)
Purpose: Build structured prompt with project context
Enforcement: ENF-L3 Advisory

CRITICAL: External models lack project context

Components:
  | Component | Source | Purpose |
  |-----------|--------|---------|
  | Role | Task requirements | Set model persona |
  | Task | User request | What to do |
  | Context | PROJECT.md, files | Project understanding |
  | Skills | Relevant skills | Available procedures |
  | Output format | Requirement | Expected result format |

Procedure:
  1. Load prompt template
  2. Substitute placeholders
  3. Include working protocol (inline)
  4. Include relevant skill references
  5. Write to temp file

Output:
  prompt_file: /tmp/claude/task-prompt-{timestamp}.md
  context_tokens: {estimated}
  files_included: [{paths}...]

Cross-skill: cf-working-protocol (for procedures)
```

### 🔧 select-model

```text
When: Choosing which model to use for task
Purpose: Match task requirements to model capabilities
Enforcement: ENF-L3 Advisory

Selection Criteria:
  | Factor | GPT-4/Codex | Gemini | Claude | Ollama |
  |--------|-------------|--------|--------|--------|
  | Code generation | Excellent | Good | Excellent | Varies |
  | Long context | 8K-128K | 1M | 200K | Model-dependent |
  | Speed | Medium | Fast | Medium | Local/fast |
  | Cost | High | Medium | High | Free |
  | Privacy | Cloud | Cloud | Cloud | Local |

Procedure:
  1. Assess task requirements
  2. Match against available models
  3. Consider cost/speed tradeoffs
  4. Recommend model

Output:
  recommended_model: {model name}
  reason: {selection rationale}
  alternatives: [{other options}]
```

### 🔧 execute-with-model (T1)

```text
When: Simple one-shot delegation
Enforcement: ENF-L1 Sentinel
Prerequisite: construct-prompt (this skill)

Procedure:
  1. Verify model CLI available
  2. Load constructed prompt
  3. Execute model CLI:
     - Codex: codex exec --full-auto "{prompt}"
     - Gemini: gemini -p "{prompt}" --yolo
  4. Capture output
  5. Parse/clean response
  6. Return to Claude for review

Output:
  model: {model used}
  output: {model response}
  tokens_used: {count}
  duration: {seconds}
  exit_code: {status}

On Failure:
  - CLI not found: BLOCK with install instructions
  - Timeout: WARN with partial output
  - Error: Return error details

Cross-skill: cf-security-management:sandbox-check (for network)
```

### 🔧 execute-multi-model

```text
When: Need consensus from multiple models
Enforcement: ENF-L1 Sentinel
Prerequisite: construct-prompt (this skill)

Procedure:
  1. Execute same task on multiple models
  2. Collect all outputs
  3. Compare results
  4. Identify consensus/differences

Output:
  results: [{model, output, duration}...]
  consensus: {areas of agreement}
  differences: {areas of disagreement}
```

### 🔧 validate-output

```text
When: After receiving model output
Purpose: Quality check before using output
Enforcement: None

Validation Checks:
  | Check | Criteria | Action if Failed |
  |-------|----------|------------------|
  | Completeness | Output addresses task | Request clarification |
  | Format | Matches requested format | Reformat |
  | Quality | No obvious errors | Flag for review |
  | Relevance | On-topic, useful | Discard/retry |

Procedure:
  1. Parse output structure
  2. Check against success criteria
  3. Flag any issues
  4. If valid and contains decisions:
     - Record decision via cf-db-operations:memory-store
     - event_type='decision', include rationale
  5. Return validation result

Output:
  valid: true | false
  issues: [{type, description}...]
  usable: true | false
  decision_recorded: true | false (if decision stored)

Cross-skill: cf-db-operations:memory-store (decision recording)
```

### 🔧 spawn-session (T3)

```text
When: Complex multi-turn work needs persistent session
Enforcement: ENF-L1 Sentinel
Prerequisites:
  - cf-memory-management:begin-work (if modifications expected)
  - cf-git-workflow:create-worktree (for isolation, optional)

CRITICAL: T3 sessions that modify files require work tracking

Autorun Context Check:
  If $AUTORUN_SESSION_ID is set:
    - WARN: "Running inside autorun worker - T3 nesting not recommended"
    - Suggest: "Use T1 (one-shot) for model delegation in autorun"
    - If user confirms: Proceed with caution

Procedure:
  1. Check for autorun context - warn if nested
  2. Verify work_id exists (from begin-work)
  3. Run check-scope-conflict via cf-memory-management:search-related-work
  4. If conflicts: WARN and suggest coordination
  5. Create worktree if needed (via cf-git-workflow)
  6. Generate session name: model-{ulid} (NOT ar-* which is autorun)
  7. Spawn tmux session:
     tmux new-session -d -s model-{ulid} -c {path}
  8. Initialize model in session
  9. Register session in DB via cf-db-operations:active-work-update
     - Link session_id to work_id
     - type: 'model_session' (distinguishes from autorun)
     - Store model, scope, worktree path

Output:
  session_id: model-{ulid}
  work_id: {linked work ID}
  task_id: {linked task ID}
  worktree_path: {path if created}
  model: {model type}
  status: 'active'
  in_autorun_context: true | false

Cross-skill:
  - cf-memory-management:begin-work (prerequisite)
  - cf-memory-management:search-related-work (conflict check)
  - cf-git-workflow:create-worktree (isolation)
  - cf-db-operations:active-work-update (session tracking)
```

### 🔧 send-task (T3)

```text
When: Sending work to persistent session
Enforcement: ENF-L1 Sentinel
Prerequisite: spawn-session (this skill)

Procedure:
  1. Validate session exists and is active
  2. Validate work_id is linked to session
  3. Load/construct task prompt
  4. Send to tmux session:
     tmux send-keys -t {session-id} "{prompt}" Enter
  5. Record task send event:
     cf-db-operations:memory-store (event_type='progress')
  6. Mark task as pending

Output:
  task_sent: true
  session_id: {session}
  work_id: {linked work}
  timestamp: {sent time}

Cross-skill: cf-db-operations:memory-store (event tracking)
```

### 🔧 poll-output (T3)

```text
When: Checking session progress
Enforcement: None

Procedure:
  1. Capture tmux buffer:
     tmux capture-pane -t {session-id} -p
  2. Parse for completion markers
  3. Extract output if complete
  4. If complete with significant output:
     - Record via cf-memory-management:record-work-progress
     - Store output summary in memory_events

Output:
  status: 'running' | 'complete' | 'error'
  output: {if complete}
  progress: {estimated percentage}
  recorded: true | false (if milestone recorded)

Cross-skill: cf-memory-management:record-work-progress (milestone tracking)
```

### 🔧 orchestrate-parallel (T3)

```text
When: Running same task on multiple models in parallel
Enforcement: ENF-L1 Sentinel

Procedure:
  1. Spawn sessions for each model
  2. Send identical tasks
  3. Poll all sessions
  4. Collect outputs when complete
  5. Compare results

Output:
  sessions: [{id, model, status}...]
  results: [{model, output, duration}...]
  comparison: {summary of differences}
```

### 🔧 terminate-session (T3)

```text
When: Session work complete
Enforcement: ENF-L1 Sentinel
Prerequisite: cf-memory-management:complete-work (if changes made)

CRITICAL: If session made file changes, complete-work MUST be called first

Procedure:
  1. Check for uncommitted changes in worktree
  2. If changes exist:
     - Verify complete-work was called (check sentinel)
     - If not: BLOCK with "call complete-work before terminating"
  3. Kill tmux session:
     tmux kill-session -t {session-id}
  4. Update session status in DB:
     cf-db-operations:active-work-update (status='completed')
  5. Clean up worktree if requested (via cf-git-workflow)
  6. Record termination event:
     cf-db-operations:memory-store (event_type='milestone')

Output:
  terminated: true
  work_id: {completed work ID}
  uncommitted_changes: true | false
  worktree_cleaned: true | false

Cross-skill:
  - cf-memory-management:complete-work (prerequisite if changes)
  - cf-git-workflow:cleanup-worktrees (optional)
  - cf-db-operations:active-work-update (session status)
  - cf-db-operations:memory-store (completion event)
```

### 🔧 list-sessions

```text
When: Checking active sessions
Enforcement: None

Procedure:
  1. Query active sessions via cf-db-operations:active-work-query
     - Filter by: type='model_session', status='active'
  2. Verify each session is alive (tmux has-session -t {id})
  3. For stale entries (tmux dead but DB active):
     - Mark for cleanup
  4. Return session list with work linkage

Output:
  sessions: [{id, model, work_id, task_id, status, started_at, last_activity}...]
  stale: [{id, reason}...]

Cross-skill: cf-db-operations:active-work-query
```

### 🔧 check-scope-conflict

```text
When: Before spawning new session
Purpose: Detect overlap with existing parallel work
Enforcement: None

Procedure:
  1. List active sessions
  2. Get scope of each session
  3. Check proposed scope against existing
  4. Warn if overlaps found

Output:
  conflicts: [{session_id, scope, overlap}...]
  safe: true | false

Cross-skill: cf-memory-management:search-related-work
```

### 🔧 cleanup-stale-sessions

```text
When: Maintenance or on error recovery
Enforcement: ENF-L1 Sentinel (cleanup uses tmux kill-session, gated by terminate-session sentinel)

Procedure:
  1. Query sessions via cf-db-operations:active-work-query
  2. Check tmux for each (tmux has-session -t {id})
  3. For dead sessions:
     - Update status via cf-db-operations:active-work-update (status='terminated')
     - Record cleanup event via cf-db-operations:memory-store
     - Clean up worktree if exists (via cf-git-workflow)
  4. Report cleaned

Output:
  cleaned: [{session_id, work_id, reason}...]
  remaining: [{active sessions}...]

Cross-skill:
  - cf-db-operations:active-work-query (find sessions)
  - cf-db-operations:active-work-update (mark terminated)
  - cf-db-operations:memory-store (cleanup event)
  - cf-git-workflow:cleanup-worktrees (optional)
```

## Session Storage

Sessions are tracked in the work graph database via `active_work` table:

```text
Database Storage (Primary - Tier 1):
  Table: active_work
  Fields:
    - id: work-{ulid}
    - task_id: {linked task}
    - type: 'model_session'
    - session_id: {tmux session name}
    - model: codex | gemini | ollama | claude
    - worktree_path: {path if isolated}
    - scope: [{file patterns}]
    - status: active | paused | completed | terminated
    - started_at: timestamp
    - last_activity: timestamp

JSONL Ledger (Tier 0 - Audit):
  All session events appended to .state/memory/sessions.jsonl
  Events: session_start, task_sent, output_received, session_end

Legacy YAML (Deprecated):
  .codeflow/state/model-sessions.yaml - migrating to DB
```

Query active sessions:

```text
cf-db-operations:active-work-query(type='model_session', status='active')
```

**Important:** This skill uses `active_work` table, NOT `autorun_*` tables:

| Table | Used By | Purpose |
|-------|---------|---------|
| `active_work` | This skill | Track model delegation sessions |
| `autorun_sessions` | Go CLI only | Track /cf-autorun batch sessions |
| `autorun_workers` | Go CLI only | Track Claude workers in autorun |
| `autorun_task_runs` | Go CLI only | Track task execution in autorun |

## Model Configuration

```json
{
  "models": {
    "codex": {
      "cli": "codex",
      "command": "exec --full-auto",
      "max_tokens": 128000
    },
    "gemini": {
      "cli": "gemini",
      "command": "-p --yolo",
      "max_tokens": 1000000
    },
    "ollama-llama": {
      "cli": "ollama",
      "command": "run llama2",
      "max_tokens": 4096
    }
  }
}
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [prompt-template.md](resources/prompt-template.md) | Delegation prompt template | Before any delegation |
| [model-comparison.md](resources/model-comparison.md) | Model capability comparison | When selecting model |
| [session-management.md](resources/session-management.md) | T3 session details | When using persistent sessions |

**Note:** Resource files above are planned but not yet created. The operation details
in this SKILL.md contain sufficient inline guidance for each operation. Resource files
will be created when the model orchestrator is actively used in production.
