# Outer Shell PathFlow: Generic Session Lifecycle Pathway

> The foundational pathway template that wraps every Claude Code session, regardless of work type. All inner pathways (development, review, research, etc.) plug in at a defined junction within this outer shell.

**Date**: 2026-02-06
**Status**: Design
**Depends On**: 01-v3-session-lifecycle-analysis.md, 02-agent-skill-teammate-mapping.md, 03-feasibility-findings.md, agent-teams-workflow-graph-sentinel-integration.md

---

## 1. Overview and Design Philosophy

### What the Outer Shell Is

The Outer Shell is the **generic session lifecycle pathway** that every Claude Code session traverses. It is not a workflow for any particular kind of work -- it is the wrapper that:

1. Boots the session and establishes context
2. Initializes the Agent Team and shared infrastructure
3. Determines what kind of work is needed and registers it
4. Hands off to an **Inner Pathway** (development, review, research, etc.)
5. Verifies work quality when the inner pathway completes
6. Tears down the session cleanly

### Design Principles

**P1 -- Separation of Lifecycle from Workflow.** The outer shell handles "how sessions live and die." Inner pathways handle "how work gets done." These are orthogonal concerns. A session can host any inner pathway; an inner pathway can run in any session.

**P2 -- Mandatory Skeleton, Advisory Flesh.** The outer shell has a small number of hard-enforced gates (sentinel-based, ENF-L1) and a larger set of advisory steps (ENF-L3). The mandatory skeleton prevents sessions from producing untracked or unverified work. The advisory steps improve quality without blocking progress.

**P3 -- Session Types as Configuration, Not Structure.** The outer shell does not branch into different structures for interactive vs autorun sessions. Instead, each node has configurable behavior that adapts based on session type. The graph topology stays the same; the enforcement level and interaction mode change.

**P4 -- Progressive Enforcement.** A quick ad-hoc fix and a multi-epic feature traverse the same outer shell. The difference is enforcement intensity: ad-hoc work may skip checkpoints (with audit trail), while structured work requires every gate to pass. This is controlled by a `session_profile` configuration.

**P5 -- Team Lead Owns the Outer Shell.** The team lead (orchestrator) is the only agent that traverses the outer shell directly. Teammates are spawned and managed within specific nodes. The outer shell is the team lead's workflow; inner pathways coordinate the team.

### Visual Overview

```
 ┌─────────────────────────────────────────────────────────────────────┐
 │                        OUTER SHELL PATHWAY                          │
 │                                                                     │
 │  SESSION    CONTEXT      TEAM       WORK          INNER            │
 │  BOOT  ──▶ LOAD    ──▶  INIT  ──▶  REGISTER ──▶  PATHWAY  ──┐    │
 │  (OS-1)    (OS-2)       (OS-3)     (OS-4)        (OS-5)     │    │
 │                                                               │    │
 │                              ┌────────────────────────────────┘    │
 │                              │                                      │
 │  SESSION    SESSION     WORK        POST-WORK                      │
 │  END    ◀── CLOSE  ◀── VERIFY  ◀── FINALIZE ◀───────────────      │
 │  (OS-9)    (OS-8)      (OS-7)      (OS-6)                         │
 │                                                                     │
 └─────────────────────────────────────────────────────────────────────┘
```

---

## 2. Complete Node-by-Node Specification

### Summary Table

| ID | Name | Type | Enforcement | Sentinel Creates | Sentinel Requires | Agent | Skippable |
|----|------|------|-------------|-----------------|-------------------|-------|-----------|
| OS-1 | Session Boot | waypoint | System (automatic) | `pathflow:shell:boot` | None | System/Lead | No |
| OS-2 | Context Load | waypoint | ENF-L3 Advisory | `pathflow:shell:context` | `pathflow:shell:boot` | Lead | No |
| OS-3 | Team Init | waypoint | ENF-L1 Blocking | `pathflow:shell:team-init` | `pathflow:shell:context` | Lead | Conditional |
| OS-4 | Work Registration | checkpoint | ENF-L1 Blocking | `pathflow:shell:work-reg` | `pathflow:shell:team-init` | Lead | No |
| OS-5 | Inner Pathway | dynamic | Delegated to inner | `pathflow:shell:inner-complete` | `pathflow:shell:work-reg` | Team | No |
| OS-6 | Post-Work Finalize | waypoint | ENF-L1 Blocking | `pathflow:shell:finalize` | `pathflow:shell:inner-complete` | Lead | No |
| OS-7 | Work Verification | checkpoint | ENF-L2 Stop | `pathflow:shell:verified` | `pathflow:shell:finalize` | Lead | No |
| OS-8 | Session Close | waypoint | System (automatic) | `pathflow:shell:close` | `pathflow:shell:verified` | System/Lead | No |
| OS-9 | Session End | waypoint | System (automatic) | None (terminal) | `pathflow:shell:close` | System | No |

### Detailed Node Descriptions

---

#### OS-1: Session Boot

**Type**: waypoint (automatic pass-through)
**Enforcement**: System -- fires automatically via SessionStart hooks
**Agent**: System hooks, then team lead

**Operations**:
1. SessionStart hooks fire in order:
   - `cf-session-start-cleanup.sh` -- Clean expired sentinels, remove stale state
   - `cf-session-start-instructions.sh` -- Display behavioral instructions, verify CRDT state
   - `cf-session-start-logging.sh` -- Initialize session log (JSONL), create session record
2. Generate `session-{ulid}` identifier
3. Insert session record into sessions table (SQLite Tier 1) and JSONL ledger (Tier 0)
4. Create the `pathflow:shell:boot` sentinel

**Sentinel Creates**: `pathflow:shell:boot` (TTL: session lifetime)
**Sentinel Requires**: None (entry point)
**Success Transition**: OS-2 (Context Load)
**Failure Transition**: Session abort (hooks failed -- log and exit)

**Configurable Aspects**:
- Cleanup aggressiveness (how old sentinels must be before removal)
- Instruction display verbosity (controlled by session profile)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Full instructions displayed |
| Autorun | Minimal instructions, batch context injected |
| Structured work | Full instructions + active epic context |
| Unstructured work | Minimal instructions |

---

#### OS-2: Context Load

**Type**: waypoint (automatic pass-through)
**Enforcement**: ENF-L3 Advisory (recommended but not blocking)
**Agent**: Team lead

**Operations**:
1. `cf-memory-management:detect-active-work` -- Query active_work table for in-progress items
2. Decision tree:
   - No active work found: Continue to OS-3 (fresh session)
   - Single active work: Offer to resume (interactive) or auto-resume (autorun)
   - Multiple active work: Present selection (interactive) or use batch target (autorun)
3. If resuming: `cf-memory-management:load-work-context` -- Load work details, recent progress events
4. `cf-working-protocol:meta-awareness` -- Assess current context state, acknowledge limitations
5. First `UserPromptSubmit` hooks fire:
   - `cf-user-prompt-submit-context.sh` -- Branch awareness, inject workflow instructions
   - `cf-user-prompt-submit-logging.sh` -- Log user prompt content
6. Create `pathflow:shell:context` sentinel

**Sentinel Creates**: `pathflow:shell:context` (TTL: session lifetime)
**Sentinel Requires**: `pathflow:shell:boot`
**Success Transition**: OS-3 (Team Init)
**Failure Transition**: Continue anyway (advisory) -- log warning

**Configurable Aspects**:
- Resume behavior: `auto-resume-single`, `prompt-always`, `never-resume`
- Meta-awareness display level: `full`, `minimal`, `silent`
- Active work detection: `enabled`, `disabled` (for batch/autorun)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Full resume prompt, meta-awareness display |
| Autorun | Auto-resume if matching task, skip meta-awareness display |
| Structured work | Auto-resume matching epic/task |
| Unstructured work | Skip resume (fresh context preferred) |

---

#### OS-3: Team Init

**Type**: waypoint (automatic for team sessions, skip for solo)
**Enforcement**: ENF-L1 Blocking (when team mode is active)
**Agent**: Team lead

**Operations**:
1. Determine team strategy based on work complexity and session profile:
   - `solo` -- No team needed (single-agent session, lead does everything)
   - `minimal` -- Lead + 1 specialist (simple feature or fix)
   - `standard` -- Lead + developer + reviewer (standard development)
   - `full` -- Lead + planner + developer + reviewer + qa (complex feature)
2. If `solo`: Create minimal sentinel and skip to OS-4
3. If team mode:
   a. `Teammate.spawnTeam(team_name)` -- Create team and task list
   b. Spawn persistent teammates based on strategy (cf-developer always for dev work)
   c. Verify team creation succeeded
   d. Read team config to confirm member roster
4. Create `pathflow:shell:team-init` sentinel

**Sentinel Creates**: `pathflow:shell:team-init` (TTL: session lifetime)
**Sentinel Requires**: `pathflow:shell:context`
**Success Transition**: OS-4 (Work Registration)
**Failure Transition**: Retry team spawn (max 2 retries), then fallback to solo mode

**Configurable Aspects**:
- Team strategy: `solo`, `minimal`, `standard`, `full`, `auto-detect`
- Max teammates: 1-5 (resource constraint)
- Context warning threshold: percentage (default 0.7)
- Persistent teammates: which roles stay alive vs on-demand

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Auto-detect team strategy from first prompt |
| Autorun | Team strategy specified in batch config |
| Structured work | Standard or full team based on epic complexity |
| Unstructured work | Solo or minimal team |

**Skippable**: Yes -- when session profile is `solo` or when Agent Teams is disabled. Creates sentinel with `skipped: true` metadata. Audit log records the skip.

---

#### OS-4: Work Registration

**Type**: checkpoint (requires verification to proceed)
**Enforcement**: ENF-L1 Blocking -- `cf-task-management:ensure-work-registered` sentinel
**Agent**: Team lead

**Operations**:
1. Analyze user request or batch configuration to determine work type
2. `cf-task-management:understand-request` -- Parse and classify the request
3. `cf-task-management:classify-work` -- Determine area/type/domain codes
4. `cf-task-management:ensure-work-registered` -- Guarantee task_id NOT NULL:
   - For structured work: Link to existing epic/task
   - For unstructured work: Create ongoing epic (GENL domain, lazy) + task
   - For autorun: Link to batch-specified task ID
5. `cf-memory-management:begin-work` -- Register active_work with task_id
6. Select inner pathway based on work type (see Section 5)
7. Gate check: Verify task_id is registered and work context is loaded
8. Create `pathflow:shell:work-reg` sentinel

**Sentinel Creates**: `pathflow:shell:work-reg` (TTL: session lifetime)
**Sentinel Requires**: `pathflow:shell:team-init`
**Success Transition**: OS-5 (Inner Pathway)
**Failure Transition**: Re-attempt classification (max 2 retries), then prompt user for clarification

**Checkpoint Configuration**:
- `reopenable`: true (work scope can be re-evaluated mid-session)
- `max_reopens`: 3
- `on_reopen`: Delete sentinel, re-enter classification

**Configurable Aspects**:
- Work classification mode: `auto-detect`, `user-specified`, `batch-defined`
- Inner pathway selection: `auto`, `manual`, `batch-override`
- Ongoing epic creation: `enabled`, `disabled` (strict mode requires pre-existing epic)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Analyze user prompt, confirm classification with user |
| Autorun | Use batch-specified task_id and pathway, no confirmation |
| Structured work | Link to epic/task from resume or user specification |
| Unstructured work | Create ongoing epic + task automatically |

---

#### OS-5: Inner Pathway (Plug-In Point)

**Type**: dynamic (inserted at runtime based on work type)
**Enforcement**: Delegated to inner pathway's own node enforcement
**Agent**: Team (all teammates participate as defined by inner pathway)

**Operations**:
1. Load inner pathway template from `.codeflow/config/pathways/{type}.json`
2. Instantiate inner pathway task graph:
   - Create Agent Teams tasks for each inner node (TaskCreate)
   - Wire dependencies between inner nodes (TaskUpdate with blockedBy)
   - Assign first available task to appropriate teammate
3. Inner pathway executes (see original PathFlow design doc for inner pathway details)
4. Monitor inner pathway progress via TaskList polling
5. When inner pathway's terminal node completes, create `pathflow:shell:inner-complete` sentinel

**Sentinel Creates**: `pathflow:shell:inner-complete` (TTL: 3600s)
**Sentinel Requires**: `pathflow:shell:work-reg`
**Success Transition**: OS-6 (Post-Work Finalize)
**Failure Transition**: Inner pathway handles its own failures; if terminal failure, escalate to lead for pathway adjustment

**Inner Pathway Selection Logic** (from OS-4 classification):

| Work Type | Branch Pattern | Inner Pathway | Command Trigger |
|-----------|---------------|---------------|-----------------|
| Feature development | `feat/*` | `development.json` | `/cf-develop` |
| Bug fix | `fix/*` | `bugfix.json` | `/cf-develop` (fix mode) |
| Hotfix | `hotfix/*` | `hotfix.json` | `/cf-ship` (hotfix mode) |
| Refactoring | `refactor/*` | `refactoring.json` | `/cf-develop` (refactor mode) |
| Planning | `plan/*` | `planning.json` | `/cf-plan` |
| Code review | (no branch) | `review.json` | `/cf-review` |
| Testing | `test/*` | `testing.json` | `/cf-test` |
| Documentation | `docs/*` | `documentation.json` | `/cf-document` |
| Research | (no branch) | `research.json` | (ad-hoc) |

**Configurable Aspects**:
- Inner pathway template override (user can specify non-default)
- Dynamic node insertion allowed (configurable per inner pathway)
- Max inner pathway duration (timeout for autorun sessions)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Inner pathway runs with interactive checkpoints |
| Autorun | Inner pathway runs without interactive gates; acceptance criteria auto-evaluated |
| Structured work | Full inner pathway with all checkpoints enforced |
| Unstructured work | Minimal inner pathway (often `research.json` or `bugfix.json`) |

---

#### OS-6: Post-Work Finalize

**Type**: waypoint (mandatory pass-through)
**Enforcement**: ENF-L1 Blocking
**Agent**: Team lead

**Operations**:
1. `cf-memory-management:complete-work` -- Archive work context, create commit sentinel
2. `cf-git-workflow:create-commit` (if not already committed by inner pathway) -- Requires complete-work sentinel (TTL 600s)
3. `cf-security-management:sandbox-check` -- Gate for network operations
4. `cf-git-workflow:sync-remote` -- Push to remote (requires sandbox-check sentinel)
5. `cf-git-workflow:create-pull-request` (if applicable) -- Requires sandbox-check sentinel
6. `cf-memory-management:record-work-progress` -- Final progress event with summary
7. Shut down on-demand teammates (SendMessage shutdown_request)
8. Create `pathflow:shell:finalize` sentinel

**Sentinel Creates**: `pathflow:shell:finalize` (TTL: 600s)
**Sentinel Requires**: `pathflow:shell:inner-complete`
**Success Transition**: OS-7 (Work Verification)
**Failure Transition**: Retry failed operations individually (commit, push, PR are independent after prerequisite)

**Configurable Aspects**:
- Auto-PR creation: `always`, `never`, `prompt` (interactive), `if-remote-branch` (autorun)
- Remote sync: `always`, `never`, `prompt`
- Teammate shutdown timing: `immediate`, `after-verification`, `manual`

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Prompt before push and PR creation |
| Autorun | Auto-push, auto-PR with batch-defined base branch |
| Structured work | PR required, linked to epic/task |
| Unstructured work | PR optional, commit always required |

---

#### OS-7: Work Verification

**Type**: checkpoint (must pass to proceed)
**Enforcement**: ENF-L2 Stop -- `cf-stop-verify-work.sh` blocks session end if incomplete
**Agent**: Team lead

**Operations**:
1. Determine PCV (Post-Completion Verification) tier:
   - Tier 1 (Simple): `verify-work` marker + TIER indicator
   - Tier 2 (Standard): Tier 1 + ARTIFACTS section + VERIFICATION section
   - Tier 3 (Complex): Tier 2 + ADVERSARIAL section
2. `cf-working-protocol:verify-work` -- Execute verification at determined tier
3. For autorun: Evaluate acceptance criteria from batch config instead of user confirmation
4. Verification output includes:
   - Files modified / created / deleted
   - Tests run and results
   - PR URL (if created)
   - Work summary
5. Gate check: Stop hook validates PCV output
6. Create `pathflow:shell:verified` sentinel

**Sentinel Creates**: `pathflow:shell:verified` (TTL: 300s)
**Sentinel Requires**: `pathflow:shell:finalize`
**Success Transition**: OS-8 (Session Close)
**Failure Transition**: Block session stop, return to work (checkpoint re-opens OS-6 if needed)

**Checkpoint Configuration**:
- `reopenable`: true (verification can fail and be re-attempted)
- `max_reopens`: 3 (after 3 failures, force-allow to prevent deadlock)
- `on_reopen`: Re-enter finalize or inner pathway depending on failure type

**Configurable Aspects**:
- PCV tier override: `auto-detect`, `force-tier-1`, `force-tier-2`, `force-tier-3`
- Autorun acceptance criteria: defined in batch config
- Max retry before force-allow: 1-5 (default 3)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Display verification output, user confirms |
| Autorun | Evaluate acceptance criteria programmatically |
| Structured work | Tier 2 or 3 verification required |
| Unstructured work | Tier 1 verification sufficient |

---

#### OS-8: Session Close

**Type**: waypoint (automatic)
**Enforcement**: System -- fires automatically
**Agent**: System hooks, then team lead

**Operations**:
1. Shut down all remaining teammates:
   - SendMessage shutdown_request to each active teammate
   - Wait for shutdown confirmations (timeout: 30s per teammate)
   - Force cleanup if teammates do not respond
2. `Teammate.cleanup()` -- Remove team and task directories
3. SessionEnd hooks fire:
   - `cf-session-end-cleanup.sh` -- Archive session state, final cleanup, release claims
   - `cf-session-end-logging.sh` -- Finalize session log, write duration, close JSONL
4. Create `pathflow:shell:close` sentinel

**Sentinel Creates**: `pathflow:shell:close` (TTL: 60s -- just long enough for OS-9)
**Sentinel Requires**: `pathflow:shell:verified`
**Success Transition**: OS-9 (Session End)
**Failure Transition**: Force session end (log errors, do not block)

**Configurable Aspects**:
- Teammate shutdown timeout: seconds (default 30)
- Cleanup aggressiveness: `full` (remove everything), `preserve-state` (keep for resume)

**Session Type Variations**:
| Session Type | Behavior |
|:-------------|:---------|
| Manual interactive | Graceful teammate shutdown with confirmations |
| Autorun | Immediate teammate shutdown, no confirmations |
| Structured work | Preserve state for session resume |
| Unstructured work | Full cleanup |

---

#### OS-9: Session End

**Type**: waypoint (terminal node)
**Enforcement**: System (automatic)
**Agent**: System

**Operations**:
1. Update sessions table: `ended_at`, `duration`, `status = 'completed'`
2. Append `session_end` event to JSONL ledger
3. Generate context summary for potential future resume
4. Clean expired pathway sentinels from `/tmp/claude/managed/sentinels/pathflow:*`
5. Status transition: `active -> completed`

**Sentinel Creates**: None (terminal node)
**Sentinel Requires**: `pathflow:shell:close`
**Success Transition**: None (session ends)
**Failure Transition**: Log error and exit (must never block)

---

## 3. Outer Shell JSON Pathway Template

```json
{
  "pathway_id": "outer-shell",
  "description": "Generic session lifecycle wrapper for all session types",
  "version": "1.0.0",
  "applicable_commands": ["*"],
  "applicable_branches": ["*"],
  "is_outer_shell": true,

  "session_profiles": {
    "interactive": {
      "description": "Manual interactive session with user present",
      "team_strategy": "auto-detect",
      "resume_behavior": "prompt-always",
      "verification_tier": "auto-detect",
      "auto_pr": "prompt",
      "auto_push": "prompt",
      "meta_awareness": "full",
      "checkpoint_interaction": "interactive"
    },
    "autorun": {
      "description": "Automated batch execution with acceptance criteria",
      "team_strategy": "batch-defined",
      "resume_behavior": "auto-resume-matching",
      "verification_tier": "auto-detect",
      "auto_pr": "always",
      "auto_push": "always",
      "meta_awareness": "silent",
      "checkpoint_interaction": "auto-evaluate"
    },
    "structured": {
      "description": "Epic-driven, task-tracked development",
      "team_strategy": "standard",
      "resume_behavior": "auto-resume-matching",
      "verification_tier": "force-tier-2",
      "auto_pr": "always",
      "auto_push": "always",
      "meta_awareness": "minimal",
      "checkpoint_interaction": "interactive"
    },
    "unstructured": {
      "description": "Ad-hoc exploration, quick fixes, research",
      "team_strategy": "solo",
      "resume_behavior": "never-resume",
      "verification_tier": "force-tier-1",
      "auto_pr": "never",
      "auto_push": "prompt",
      "meta_awareness": "minimal",
      "checkpoint_interaction": "auto-evaluate"
    }
  },

  "nodes": [
    {
      "id": "OS-1",
      "name": "Session Boot",
      "type": "waypoint",
      "label": "Boot session, fire SessionStart hooks, create session record",
      "agent_role": "system",
      "enforcement": "system",
      "sentinel_name": "pathflow:shell:boot",
      "sentinel_ttl": "session",
      "operations": [
        "hook:session-start-cleanup",
        "hook:session-start-instructions",
        "hook:session-start-logging",
        "system:create-session-record"
      ],
      "blockedBy": [],
      "acceptance": "Session record created, session-{ulid} assigned"
    },
    {
      "id": "OS-2",
      "name": "Context Load",
      "type": "waypoint",
      "label": "Detect active work, load context, run meta-awareness",
      "agent_role": "team-lead",
      "enforcement": "ENF-L3",
      "sentinel_name": "pathflow:shell:context",
      "sentinel_ttl": "session",
      "operations": [
        "cf-memory-management:detect-active-work",
        "cf-memory-management:load-work-context",
        "cf-working-protocol:meta-awareness",
        "hook:user-prompt-submit-context",
        "hook:user-prompt-submit-logging"
      ],
      "blockedBy": ["OS-1"],
      "acceptance": "Context assessed, active work detected or fresh start confirmed"
    },
    {
      "id": "OS-3",
      "name": "Team Init",
      "type": "waypoint",
      "label": "Determine team strategy, spawn teammates, verify team",
      "agent_role": "team-lead",
      "enforcement": "ENF-L1",
      "sentinel_name": "pathflow:shell:team-init",
      "sentinel_ttl": "session",
      "operations": [
        "pathflow:determine-team-strategy",
        "agent-teams:spawn-team",
        "agent-teams:spawn-teammates",
        "agent-teams:verify-team"
      ],
      "blockedBy": ["OS-2"],
      "acceptance": "Team spawned and verified, or solo mode confirmed",
      "skippable": {
        "condition": "session_profile.team_strategy == 'solo' OR agent_teams_disabled",
        "sentinel_metadata": { "skipped": true },
        "audit": true
      }
    },
    {
      "id": "OS-4",
      "name": "Work Registration",
      "type": "checkpoint",
      "label": "Classify work, register task, select inner pathway",
      "agent_role": "team-lead",
      "enforcement": "ENF-L1",
      "sentinel_name": "pathflow:shell:work-reg",
      "sentinel_ttl": "session",
      "operations": [
        "cf-task-management:understand-request",
        "cf-task-management:classify-work",
        "cf-task-management:ensure-work-registered",
        "cf-memory-management:begin-work",
        "pathflow:select-inner-pathway"
      ],
      "blockedBy": ["OS-3"],
      "acceptance": "task_id NOT NULL, inner pathway selected, active_work registered",
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "delete_sentinel_and_reblock"
      }
    },
    {
      "id": "OS-5",
      "name": "Inner Pathway",
      "type": "dynamic",
      "label": "Execute work via plugged-in inner pathway",
      "agent_role": "team",
      "enforcement": "delegated",
      "sentinel_name": "pathflow:shell:inner-complete",
      "sentinel_ttl": 3600,
      "operations": [
        "pathflow:load-inner-template",
        "pathflow:instantiate-inner-graph",
        "pathflow:execute-inner-pathway",
        "pathflow:monitor-inner-progress"
      ],
      "blockedBy": ["OS-4"],
      "acceptance": "Inner pathway terminal node completed",
      "dynamic_config": {
        "inner_pathway_source": "OS-4:selected_pathway",
        "inner_pathway_dir": ".codeflow/config/pathways/",
        "max_duration_seconds": null,
        "autorun_max_duration_seconds": 1800
      }
    },
    {
      "id": "OS-6",
      "name": "Post-Work Finalize",
      "type": "waypoint",
      "label": "Complete work lifecycle, commit, push, create PR, shutdown on-demand teammates",
      "agent_role": "team-lead",
      "enforcement": "ENF-L1",
      "sentinel_name": "pathflow:shell:finalize",
      "sentinel_ttl": 600,
      "operations": [
        "cf-memory-management:complete-work",
        "cf-git-workflow:create-commit",
        "cf-security-management:sandbox-check",
        "cf-git-workflow:sync-remote",
        "cf-git-workflow:create-pull-request",
        "cf-memory-management:record-work-progress",
        "agent-teams:shutdown-on-demand-teammates"
      ],
      "blockedBy": ["OS-5"],
      "acceptance": "Work committed, remote synced, PR created (if applicable)"
    },
    {
      "id": "OS-7",
      "name": "Work Verification",
      "type": "checkpoint",
      "label": "Post-Completion Verification at appropriate tier",
      "agent_role": "team-lead",
      "enforcement": "ENF-L2",
      "sentinel_name": "pathflow:shell:verified",
      "sentinel_ttl": 300,
      "operations": [
        "pathflow:determine-pcv-tier",
        "cf-working-protocol:verify-work"
      ],
      "blockedBy": ["OS-6"],
      "acceptance": "PCV output generated and validated at required tier",
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "re-enter-finalize-or-inner",
        "force_allow_after_max": true
      }
    },
    {
      "id": "OS-8",
      "name": "Session Close",
      "type": "waypoint",
      "label": "Shutdown all teammates, cleanup team, fire SessionEnd hooks",
      "agent_role": "system",
      "enforcement": "system",
      "sentinel_name": "pathflow:shell:close",
      "sentinel_ttl": 60,
      "operations": [
        "agent-teams:shutdown-all-teammates",
        "agent-teams:cleanup-team",
        "hook:session-end-cleanup",
        "hook:session-end-logging"
      ],
      "blockedBy": ["OS-7"],
      "acceptance": "All teammates shut down, session state archived"
    },
    {
      "id": "OS-9",
      "name": "Session End",
      "type": "waypoint",
      "label": "Finalize session record, clean pathway sentinels",
      "agent_role": "system",
      "enforcement": "system",
      "sentinel_name": null,
      "sentinel_ttl": null,
      "operations": [
        "system:finalize-session-record",
        "system:clean-pathway-sentinels",
        "system:generate-context-summary"
      ],
      "blockedBy": ["OS-8"],
      "acceptance": "Session record status=completed, JSONL closed"
    }
  ],

  "dynamic_insertion_rules": {
    "allowed": true,
    "insertion_points": ["after:OS-4", "after:OS-5", "after:OS-6"],
    "max_inserted_nodes": 3,
    "require_lead_approval": true,
    "examples": [
      {
        "scenario": "Security review needed before finalize",
        "insert_after": "OS-5",
        "node": {
          "id": "OS-5a",
          "name": "Security Review",
          "type": "work",
          "agent_role": "cf-reviewer",
          "enforcement": "ENF-L1"
        }
      },
      {
        "scenario": "Scope change after work registration",
        "insert_after": "OS-4",
        "node": {
          "id": "OS-4a",
          "name": "Scope Re-Evaluation",
          "type": "checkpoint",
          "agent_role": "team-lead",
          "enforcement": "ENF-L1"
        }
      }
    ]
  }
}
```

---

## 4. Session Type Variations

### 4.1 Manual Interactive Session

The default mode. A human user invokes Claude Code and works interactively.

**Key Characteristics**:
- User is present and can answer questions
- Checkpoints require interactive confirmation
- Team strategy auto-detected from first prompt
- PR creation and push require user confirmation
- Full meta-awareness and verification display

**Outer Shell Traversal**:
```
OS-1 (auto) → OS-2 (with resume prompt) → OS-3 (auto-detect team) → OS-4 (classify from prompt)
    → OS-5 (inner pathway with interactive checkpoints) → OS-6 (confirm push/PR)
    → OS-7 (display verification, user confirms) → OS-8 (graceful shutdown) → OS-9 (auto)
```

**Node-Level Differences**:
| Node | Interactive Behavior |
|------|---------------------|
| OS-2 | Full resume prompt if active work found |
| OS-3 | Team spawned after first prompt analysis |
| OS-4 | Classification confirmed with user before proceeding |
| OS-5 | Inner pathway checkpoints wait for user input |
| OS-6 | Push and PR creation require user approval |
| OS-7 | Verification output displayed, user confirms quality |
| OS-8 | Teammates get graceful shutdown with confirmation wait |

### 4.2 Autorun Session

Automated batch execution via CLI. No user present. Acceptance criteria drive decisions.

**Key Characteristics**:
- No interactive prompts; all decisions from batch config
- Acceptance criteria replace user confirmation at every checkpoint
- Team strategy and inner pathway specified in batch config
- Auto-push and auto-PR at the end
- Timeout enforcement on all nodes

**Outer Shell Traversal**:
```
OS-1 (auto) → OS-2 (auto-resume matching task) → OS-3 (batch-defined team) → OS-4 (batch task_id)
    → OS-5 (inner pathway, auto-evaluate checkpoints, max_duration enforced)
    → OS-6 (auto-push, auto-PR) → OS-7 (evaluate acceptance criteria) → OS-8 (immediate shutdown) → OS-9 (auto)
```

**Batch Configuration Integration**:
```json
{
  "session_profile": "autorun",
  "task_id": "FRT-TSK-FIX-AUTH-042",
  "inner_pathway": "development",
  "team_strategy": "standard",
  "acceptance_criteria": [
    "All tests pass",
    "No linting errors",
    "PR created against main branch"
  ],
  "max_duration_seconds": 1800,
  "auto_pr_base": "main"
}
```

**Node-Level Differences**:
| Node | Autorun Behavior |
|------|-----------------|
| OS-2 | Auto-resume if matching task_id found, skip meta-awareness display |
| OS-3 | Spawn team per batch config, no auto-detect |
| OS-4 | Use batch task_id directly, skip classification |
| OS-5 | Inner checkpoints auto-evaluated against acceptance criteria; timeout enforced |
| OS-6 | Auto-commit, auto-push, auto-PR with batch base branch |
| OS-7 | Programmatic acceptance criteria evaluation |
| OS-8 | Immediate teammate termination, no confirmation wait |

### 4.3 Structured Work

Epic-driven development with task tracking. Work is planned before execution.

**Key Characteristics**:
- Epic and task pre-exist (from `/cf-plan` or prior session)
- Inner pathway is typically `development.json` or `planning.json`
- All checkpoints enforced at standard or full level
- PR always required, linked to task/epic
- Verification at Tier 2 or Tier 3

**Outer Shell Traversal**:
```
OS-1 (auto) → OS-2 (resume matching epic/task) → OS-3 (standard or full team) → OS-4 (link to epic)
    → OS-5 (full inner pathway with all checkpoints) → OS-6 (required PR, linked to epic)
    → OS-7 (Tier 2+ verification) → OS-8 (preserve state for resume) → OS-9 (auto)
```

### 4.4 Unstructured Work

Ad-hoc exploration, quick fixes, help requests. Minimal ceremony.

**Key Characteristics**:
- No pre-existing epic or task (created lazily as ongoing/GENL)
- Solo mode preferred (no teammates)
- Inner pathway is lightweight (`bugfix.json` or `research.json`)
- Verification at Tier 1
- PR optional

**Outer Shell Traversal**:
```
OS-1 (auto) → OS-2 (skip resume) → OS-3 (solo, skipped) → OS-4 (create ongoing epic + task)
    → OS-5 (minimal inner pathway) → OS-6 (commit, optional push/PR)
    → OS-7 (Tier 1 verification) → OS-8 (full cleanup) → OS-9 (auto)
```

---

## 5. Inner Pathway Plug-In Mechanism

### Architecture

The outer shell connects to inner pathways at the OS-4 -> OS-5 junction. This is a clean boundary:

```
OUTER SHELL                    INNER PATHWAY
┌──────────┐                   ┌──────────────────────────┐
│ OS-4     │                   │ development.json         │
│ Work Reg │─── selects ─────▶│                          │
│          │                   │  init → plan → [ckpt]   │
│ output:  │                   │    → impl → [ckpt]      │
│ pathway= │                   │    → review ─┐          │
│ "dev"    │                   │    → testing ─┤→ final   │
└──────────┘                   └──────────────────────────┘
                                       │
                                       │ on terminal node completion
                                       ▼
                               ┌──────────┐
                               │ OS-6     │
                               │ Finalize │
                               └──────────┘
```

### Plug-In Protocol

**Step 1: Selection** (at OS-4)
The team lead determines the inner pathway based on:
1. Explicit command: `/cf-develop` selects `development.json`
2. Branch pattern: `feat/*` maps to `development.json`
3. Work classification: `type=FIX` maps to `bugfix.json`
4. Batch config: `inner_pathway: "development"` overrides auto-detection
5. User specification: "I want to do a code review" -> `review.json`

Priority order: batch config > explicit command > user specification > branch pattern > auto-detection.

**Step 2: Instantiation** (at OS-5 entry)
1. Load template from `.codeflow/config/pathways/{selected}.json`
2. Validate template version compatibility
3. For each node in the inner pathway template:
   - Create Agent Teams task (TaskCreate with subject, description)
   - Set blockedBy relationships (TaskUpdate)
   - Do NOT assign owners yet (wait for node readiness)
4. Store instantiated graph in active pathway state file

**Step 3: Execution** (during OS-5)
1. Team lead monitors TaskList for ready tasks (unblocked, no owner)
2. When inner node is ready:
   - Assign to appropriate teammate based on `agent_role` (TaskUpdate with owner)
   - Teammate executes work per node acceptance criteria
   - On completion: pathway engine creates inner pathway sentinel
   - Task marked completed (TaskUpdate)
3. For checkpoint nodes in the inner pathway:
   - Team lead evaluates acceptance criteria
   - Approved: sentinel created, downstream unblocked
   - Rejected: checkpoint re-opened, predecessor may be re-assigned

**Step 4: Completion** (OS-5 exit)
1. Inner pathway terminal node completes
2. Pathway engine creates `pathflow:shell:inner-complete` sentinel
3. Control returns to outer shell at OS-6

### Inner Pathway Contract

Every inner pathway template must conform to this contract:

```json
{
  "required_fields": {
    "pathway_id": "string (unique identifier)",
    "version": "semver string",
    "nodes": "array of node objects",
    "nodes[0].id": "must be entry point",
    "nodes[-1]": "must be terminal node (no outgoing edges)"
  },
  "entry_point": "First node in nodes array, blockedBy must be empty",
  "terminal_node": "Last node in nodes array, no other node has it as blockedBy",
  "sentinel_namespace": "All sentinel names prefixed with pathflow:{pathway_id}:",
  "agent_roles": "Must be from: team-lead, cf-planner, cf-developer, cf-reviewer, cf-qa, cf-ops, cf-documenter"
}
```

### Composition: Nested Inner Pathways

For complex workflows, inner pathways can reference other pathways as sub-graphs. This is a future extension:

```json
{
  "id": "full-feature",
  "type": "composite",
  "sub_pathways": [
    { "pathway_id": "planning", "connects_to": "development" },
    { "pathway_id": "development", "connects_to": "review-and-test" },
    { "pathway_id": "review-and-test", "connects_to": null }
  ]
}
```

This is noted for future design and is NOT part of the initial implementation.

---

## 6. Dynamic Modification Rules

### 6.1 Node Skipping

Any outer shell node marked as `skippable` can be bypassed under defined conditions.

**Rules**:
- Only the team lead can skip a node
- Skip conditions are evaluated at node entry
- Skipped nodes create their sentinel with `"skipped": true` metadata
- All skips are logged to the audit trail (JSONL ledger)
- Downstream nodes treat skipped sentinels the same as completed sentinels

**Currently Skippable Nodes**:
| Node | Skip Condition |
|------|---------------|
| OS-3 (Team Init) | `session_profile.team_strategy == 'solo'` or Agent Teams disabled |
| OS-6 (Post-Work Finalize) | Inner pathway already committed + pushed (avoids double-commit) |

All other nodes are mandatory and cannot be skipped.

### 6.2 Checkpoint Re-Opening

Checkpoint nodes (OS-4, OS-7) can be re-opened when their acceptance criteria are not met.

**Rules**:
- `max_reopens` limits re-opening count (default 3)
- On re-open:
  1. Checkpoint sentinel is **deleted**
  2. Downstream nodes are re-blocked (their sentinels are also deleted)
  3. The checkpoint's predecessor may be re-assigned for rework
  4. Reopen count is incremented
  5. Audit log entry created with reason
- After `max_reopens` exceeded:
  - For OS-7 (Work Verification): Force-allow to prevent deadlock (session must end)
  - For OS-4 (Work Registration): Prompt user for manual classification

**Re-Open Flow Example** (OS-7 fails):
```
OS-7 fails (verification insufficient)
    │
    ├── reopen_count < max_reopens?
    │   ├── Yes → Delete pathflow:shell:verified
    │   │         Delete pathflow:shell:finalize
    │   │         Re-enter OS-6 (complete missing work, re-commit)
    │   │         Then re-enter OS-7 (re-verify)
    │   │
    │   └── No → Force-allow (create sentinel with "forced: true" metadata)
    │            Log warning to audit trail
    │            Proceed to OS-8
```

### 6.3 Dynamic Node Insertion

New nodes can be inserted into the outer shell at runtime.

**Allowed Insertion Points**: `after:OS-4`, `after:OS-5`, `after:OS-6`
**Maximum Inserted Nodes**: 3

**Rules**:
- Only the team lead can insert nodes
- Inserted nodes must specify: id, name, type, agent_role, enforcement
- Inserted nodes are wired into the dependency graph:
  - blockedBy: the node they are inserted after
  - The node that originally followed the insertion point now also depends on the inserted node
- Inserted nodes are tracked in the active pathway state (`inserted_nodes` array)
- Inserted nodes can be any type except `waypoint` (system waypoints are not dynamic)

**Insertion Example**:
```
Before: OS-5 (Inner Pathway) → OS-6 (Post-Work Finalize)

Insert "Security Review" after OS-5:

After:  OS-5 (Inner Pathway) → OS-5a (Security Review) → OS-6 (Post-Work Finalize)
```

### 6.4 Short vs Long Session Adaptation

The outer shell adapts its behavior for sessions of different lengths:

**Short Sessions** (< 5 minutes, < 10 tool calls):
- OS-2: Skip meta-awareness display
- OS-3: Default to solo mode
- OS-4: Lightweight classification (auto-create ongoing task)
- OS-5: Minimal inner pathway
- OS-6: Commit only (no push/PR unless requested)
- OS-7: Tier 1 verification

**Long Sessions** (> 30 minutes, > 50 tool calls):
- OS-2: Full context load with memory review
- OS-3: Full team with context monitoring
- OS-5: Full inner pathway with all checkpoints
- OS-6: Complete finalization with PR
- OS-7: Tier 2 or 3 verification
- Additional: Periodic heartbeat checks, claim renewal, context warnings

Detection happens progressively -- the outer shell starts in "short session" mode and escalates as the session grows. This is tracked via the session state and monitored by PostToolUse hooks.

---

## 7. Sentinel Enforcement Map

### Outer Shell Sentinel Chain

```
pathflow:shell:boot
    │
    ▼ (requires boot)
pathflow:shell:context
    │
    ▼ (requires context)
pathflow:shell:team-init
    │
    ▼ (requires team-init)
pathflow:shell:work-reg
    │
    ▼ (requires work-reg)
pathflow:shell:inner-complete
    │
    ▼ (requires inner-complete)
pathflow:shell:finalize
    │
    ▼ (requires finalize)
pathflow:shell:verified
    │
    ▼ (requires verified)
pathflow:shell:close
```

### Sentinel Details

| Sentinel Name | Created By | Created At | TTL | Enforcement | File Path |
|--------------|-----------|-----------|-----|-------------|-----------|
| `pathflow:shell:boot` | System (SessionStart hooks) | OS-1 exit | session | System | `/tmp/claude/managed/sentinels/pathflow-shell-boot-{session_ulid}` |
| `pathflow:shell:context` | Team lead | OS-2 exit | session | ENF-L3 | `/tmp/claude/managed/sentinels/pathflow-shell-context-{session_ulid}` |
| `pathflow:shell:team-init` | Team lead | OS-3 exit | session | ENF-L1 | `/tmp/claude/managed/sentinels/pathflow-shell-team-init-{session_ulid}` |
| `pathflow:shell:work-reg` | Team lead | OS-4 exit | session | ENF-L1 | `/tmp/claude/managed/sentinels/pathflow-shell-work-reg-{session_ulid}` |
| `pathflow:shell:inner-complete` | Pathway engine | OS-5 exit | 3600s | Delegated | `/tmp/claude/managed/sentinels/pathflow-shell-inner-complete-{session_ulid}` |
| `pathflow:shell:finalize` | Team lead | OS-6 exit | 600s | ENF-L1 | `/tmp/claude/managed/sentinels/pathflow-shell-finalize-{session_ulid}` |
| `pathflow:shell:verified` | Team lead | OS-7 exit | 300s | ENF-L2 | `/tmp/claude/managed/sentinels/pathflow-shell-verified-{session_ulid}` |
| `pathflow:shell:close` | System (SessionEnd hooks) | OS-8 exit | 60s | System | `/tmp/claude/managed/sentinels/pathflow-shell-close-{session_ulid}` |

### Interaction with Existing Skill Sentinels

Outer shell pathway sentinels operate at a **different layer** from existing skill sentinels. Both must be satisfied:

```
Tool Invocation
    │
    ├── Layer 0: settings.json deny list (unchanged)
    │
    ├── Layer 1a: Skill sentinels (existing, e.g., complete-work before commit)
    │   └── Checked by: cf-pre-tool-use-bash-sentinel.sh, cf-pre-tool-use-file-sentinel.sh
    │
    ├── Layer 1b: Pathway sentinels (new, e.g., work-reg before any Edit/Write)
    │   └── Checked by: cf-pre-tool-use-pathflow-gate.sh (NEW)
    │
    ├── Layer 2: Stop hook (PCV verification, unchanged)
    │   └── cf-stop-verify-work.sh
    │
    └── Layer 3: Advisory hooks (unchanged)
        └── cf-post-tool-use-*.sh
```

**Key Rule**: Skill sentinels and pathway sentinels are complementary. A commit requires BOTH `complete-work` (skill sentinel) AND `pathflow:shell:inner-complete` (pathway sentinel, verifying the inner pathway finished). Neither replaces the other.

### Sentinel File Format

Each pathway sentinel is a JSON file:

```json
{
  "sentinel": "pathflow:shell:work-reg",
  "type": "pathway",
  "pathway_id": "outer-shell",
  "node_id": "OS-4",
  "session_id": "session-01JKABCDEF1234",
  "team_name": "cf-E-AUTH-001",
  "task_id": "FRT-TSK-FIX-AUTH-042",
  "created": 1707200000,
  "expires": null,
  "completed_by": "team-lead",
  "skipped": false,
  "forced": false,
  "reopen_count": 0,
  "metadata": {
    "inner_pathway_selected": "development",
    "work_classification": { "area": "FRT", "type": "FIX", "domain": "AUTH" }
  }
}
```

---

## 8. Team Actions at Each Node

### Team Action Summary

| Node | Team Lead Action | Teammate Actions | Team State After |
|------|-----------------|-----------------|-----------------|
| OS-1 | None (system) | None (no team yet) | No team |
| OS-2 | Load context, assess work | None (no team yet) | No team |
| OS-3 | Determine strategy, spawn team | Teammates boot, load blueprints | Team active |
| OS-4 | Classify work, select pathway | None (lead-only checkpoint) | Team idle, awaiting work |
| OS-5 | Assign tasks, monitor progress, manage checkpoints | Execute assigned work nodes | Team working |
| OS-6 | Finalize work, commit, push, PR, shutdown on-demand | On-demand teammates shut down | Persistent teammates only |
| OS-7 | Run PCV verification | None (lead-only checkpoint) | Persistent teammates idle |
| OS-8 | Shutdown all remaining teammates | Acknowledge shutdown, exit | Team dissolved |
| OS-9 | None (system) | None (team gone) | No team |

### Detailed Team Actions Per Node

#### OS-1: Session Boot
- **Team Lead**: Passive. System hooks handle everything.
- **Teammates**: Do not exist yet.

#### OS-2: Context Load
- **Team Lead**: Runs active work detection and meta-awareness. Decides whether to resume previous work. This is solo -- the team lead needs context before deciding what team to build.
- **Teammates**: Do not exist yet.
- **Design Decision**: Team init comes AFTER context load because the team strategy depends on understanding what work is needed.

#### OS-3: Team Init
- **Team Lead**:
  1. Evaluates work complexity from OS-2 context
  2. Selects team strategy: solo / minimal / standard / full
  3. If not solo: `Teammate.spawnTeam(team_name)` creates team + task list
  4. Spawns teammates per strategy:
     - `minimal`: cf-developer only
     - `standard`: cf-developer + cf-reviewer
     - `full`: cf-planner + cf-developer + cf-reviewer + cf-qa
  5. Verifies team by reading config file
- **Teammates**: Boot, load blueprints from `.claude/agents/cf-{role}.md`, report ready.

#### OS-4: Work Registration
- **Team Lead**:
  1. Analyzes user request
  2. Classifies work (area, type, domain)
  3. Registers task in work graph (ensure task_id NOT NULL)
  4. Selects inner pathway based on classification
  5. Communicates inner pathway selection to team (if team exists)
- **Teammates**: Idle. Waiting for work assignment. May receive a message from lead about upcoming work.

#### OS-5: Inner Pathway Execution
- **Team Lead**:
  1. Instantiates inner pathway as Agent Teams tasks
  2. Assigns first ready task to appropriate teammate (based on `agent_role`)
  3. Monitors TaskList for progress
  4. Manages inner pathway checkpoints (approve/reject)
  5. Handles dynamic node insertion if needed
  6. Re-assigns work if teammate encounters blockers
  7. Responds to teammate messages (questions, escalations)
- **Teammates**:
  1. Check TaskList for assigned/claimable tasks
  2. Execute work per task description and acceptance criteria
  3. Report progress via SendMessage
  4. Mark tasks completed via TaskUpdate
  5. Claim next available task

#### OS-6: Post-Work Finalize
- **Team Lead**:
  1. Runs work completion lifecycle (complete-work, commit, push, PR)
  2. Sends shutdown_request to on-demand teammates (planner, reviewer, qa, documenter)
  3. Waits for shutdown confirmations
  4. Records final progress event
- **On-Demand Teammates**: Receive shutdown request, approve, exit.
- **Persistent Teammates** (cf-developer): Remain alive through OS-7 in case rework is needed.

#### OS-7: Work Verification
- **Team Lead**:
  1. Determines PCV tier
  2. Executes verification protocol
  3. If verification fails and re-open needed: may re-activate persistent teammates for rework
- **Persistent Teammates**: Idle but available for rework if verification fails.

#### OS-8: Session Close
- **Team Lead**:
  1. Sends shutdown_request to all remaining teammates
  2. Waits for confirmations (timeout: 30s each)
  3. Calls `Teammate.cleanup()` to remove team directories
- **All Teammates**: Receive shutdown, approve, exit. If unresponsive, force-terminated by cleanup.

#### OS-9: Session End
- **Team Lead**: Passive. System hooks handle session record finalization.
- **Teammates**: Gone.

---

## 9. Open Design Questions

### Q1: Should OS-3 (Team Init) be deferred until after OS-4 (Work Registration)?

**Argument for current ordering** (OS-3 before OS-4): The team strategy can be initially estimated from context load (OS-2), and having the team ready before work registration allows parallel setup. The planner teammate (if spawned) could assist with work classification.

**Argument for deferral** (OS-4 before OS-3): Work classification determines team strategy more precisely. Why spawn a full team for what turns out to be a simple fix? This would reduce resource waste.

**Current decision**: Keep OS-3 before OS-4, but allow the team strategy to be refined at OS-4. If OS-4 determines a simpler pathway than expected, surplus teammates can be shut down. The `auto-detect` strategy evaluates lazily.

### Q2: How should the outer shell handle session crashes?

If a session terminates unexpectedly (crash, network loss, context exhaustion):
- Sentinels in `/tmp/claude/managed/sentinels/` persist (they are files, not memory)
- Next session's OS-1 cleanup hook removes expired sentinels
- OS-2 detects the crashed session's active work
- Resume path re-enters at OS-4 or OS-5 depending on how far the previous session progressed

**Unresolved**: Should the outer shell create a "crash recovery" variant of the pathway that skips already-completed nodes? This requires checking which pathway sentinels survived the crash.

### Q3: Multi-pathway sessions

Can a single session traverse the outer shell multiple times (e.g., user does a fix, then does a review, then does documentation)? Options:
- **Single traversal**: Each session = one outer shell pass. New work = new session.
- **Multi-traversal**: OS-7 can loop back to OS-4 for a new inner pathway. The outer shell becomes a loop.
- **Nested sessions**: The outer shell instantiates a new "virtual session" for each piece of work within the same Claude Code session.

**Current decision**: Defer to implementation. Start with single traversal. Multi-traversal is a natural extension if OS-7 success allows re-entering OS-4 instead of proceeding to OS-8.

### Q4: Sentinel TTL for session-lifetime sentinels

Several sentinels have `"sentinel_ttl": "session"` meaning they live for the entire session. Since sessions can vary from 5 minutes to 2+ hours, a fixed TTL does not work. Options:
- Use a very long TTL (e.g., 14400s = 4 hours) as an upper bound
- Use "no expiry" and rely on cleanup hooks to remove them
- Use a moderate TTL (3600s) with heartbeat renewal from the lead

**Current decision**: Use 14400s (4 hours) as default for session-lifetime sentinels. Cleanup hooks remove them on next session start if the session ended without proper cleanup.

---

## 10. Implementation Priority

The outer shell should be implemented in this order:

1. **Sentinel foundation**: Pathway sentinel create/validate/delete functions in security lib
2. **Node OS-1 and OS-9**: System waypoints (SessionStart/SessionEnd hooks already exist)
3. **Node OS-4**: Work Registration checkpoint (builds on existing task-sentinel enforcement)
4. **Node OS-7**: Work Verification checkpoint (builds on existing stop-verify-work hook)
5. **Node OS-3**: Team Init (requires Agent Teams integration)
6. **Node OS-5**: Inner Pathway plug-in (requires inner pathway templates)
7. **Nodes OS-2, OS-6, OS-8**: Remaining waypoints
8. **Dynamic modification**: Node insertion, checkpoint re-opening, skipping
9. **Session profiles**: Configuration-driven behavior adaptation
10. **Enforcement hook**: `cf-pre-tool-use-pathflow-gate.sh`
