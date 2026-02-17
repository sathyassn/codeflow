# CodeFlow Revamp: Architectural Analysis and Design Proposal (v2)

> From V3 sub-agents to agent-teams-only: PathFlow lifecycle, teammate architecture, DB strategy, and Claude component transformation plan.

**Task:** INF-TSK-SPKE-GENL-001
**Branch:** spike/workflow-revamp
**Date:** 2026-02-14
**Status:** Proposal v2 (corrects 9 errors from v1)
**Supersedes:** `.codeflow/docs/analysis/codeflow-revamp-proposal.md` (v1, 2026-02-13)
**Error Log:** `.codeflow/docs/analysis/errors-and-finalizations.md`

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Problem Statement](#2-problem-statement)
   - [2.1 V3 Sub-Agent Model Limitations](#21-the-v3-sub-agent-model-is-fundamentally-insufficient)
   - [2.2 Skill-Based Sentinel Enforcement](#22-skill-based-sentinel-enforcement-is-disruptive)
   - [2.3 No Structured Session Lifecycle](#23-no-structured-session-lifecycle)
   - [2.4 Self-Verification (PCV) Inadequate](#24-self-verification-pcv-is-inadequate)
   - [2.5 Over-Engineered Configurability](#25-over-engineered-configurability)
   - [2.6 Secondary Design Gaps](#26-secondary-design-gaps)
3. [Proposed Solution: Agent Teams + PathFlow](#3-proposed-solution-agent-teams--pathflow)
   - [3.1 Agent Teams Platform Capability](#31-agent-teams-the-platform-capability)
   - [3.2 PathFlow: 7-Phase Session Lifecycle](#32-pathflow-the-session-lifecycle-framework)
   - [3.3 PathFlow Sentinels Replace Skill Sentinels](#33-pathflow-sentinels-replace-skill-sentinels)
   - [3.4 WS-REV Replaces PCV](#34-ws-rev-replaces-pcv)
   - [3.5 Teammate Architecture](#35-teammate-architecture)
   - [3.6 Skills Transformation](#36-skills-transformation)
   - [3.7 Configuration Simplification](#37-configuration-simplification)
4. [Design Decisions](#4-design-decisions)
   - [4.1 Area Types: Adding PLN](#41-area-types-adding-pln)
   - [4.2 Work Types: Adding PLAN](#42-work-types-adding-plan-clarifying-spke)
   - [4.3 Work Stages: 6 Stages with 1:1 Mapping](#43-work-stages-eliminating-ws-work)
   - [4.4 Work Type Pipelines](#44-work-type-pipelines-all-10)
     - [4.4.1 Pipeline Extensibility: Custom Stages](#441-pipeline-extensibility-custom-stages)
   - [4.5 Planning Epic](#45-planning-epic-single-pln-epc-plan-genl-001)
5. [Component Transformation Plan](#5-component-transformation-plan)
   - [5.1 PathFlow Configuration File](#51-pathflow-configuration-file)
   - [5.2 Database Schema Changes](#52-database-schema-changes)
     - [5.2.8 PathFlow Config and DB Coordination](#528-pathflow-config-and-db-coordination)
   - [5.3 CLAUDE.md Changes](#53-claudemd-changes)
   - [5.4 Hook Restructuring](#54-hook-restructuring)
   - [5.5 Agent Definition Files](#55-agent-definition-files)
   - [5.6 Skills Archival](#56-skills-archival)
   - [5.7 Settings.json Changes](#57-settingsjson-changes)
6. [Enforcement Model](#6-enforcement-model)
   - [6.1 4-Layer Enforcement Chain](#61-4-layer-enforcement-chain)
   - [6.2 PathFlow-Gate Hook](#62-pathflow-gate-hook)
   - [6.3 Graceful Degradation](#63-graceful-degradation)
   - [6.4 PathFlow Enforcement Mechanism](#64-pathflow-enforcement-mechanism)
   - [6.5 STAGE-COMPLETE Protocol](#65-stage-complete-protocol)
   - [6.6 File Sentinels vs JSONL Sentinels](#66-file-sentinels-vs-jsonl-sentinels)
   - [6.7 JSONL Event Flow: Script Responsibilities](#67-jsonl-event-flow-script-responsibilities)
   - [6.8 Dual-Track State: File Sentinels + JSONL Events](#68-dual-track-state-file-sentinels--jsonl-events)
7. [End-to-End Flow Chains](#7-end-to-end-flow-chains)
   - [7.1 FEAT Flow](#71-feat-flow-full-pipeline)
   - [7.2 PLAN Flow](#72-plan-flow-design-architecture)
   - [7.3 Rework Loop](#73-rework-loop-flow)
   - [7.4 Untracked Session Flow](#74-untracked-session-flow)
8. [V4 Specification Impact](#8-v4-specification-impact)
9. [Implementation Roadmap](#9-implementation-roadmap)
   - [9.1 Phase 4 Sub-Phases](#91-phase-4-sub-phases)
   - [9.2 Three-Period DB/JSONL Strategy](#92-three-period-dbjsonl-strategy)
   - [9.3 Backward Compatibility](#93-backward-compatibility)

- [Appendix A: Cross-Reference to Research](#appendix-a-cross-reference-to-pathflow-v3-research)
- [Appendix B: Terminology](#appendix-b-terminology)
- [Appendix C: Complete Hook Disposition Table](#appendix-c-complete-hook-disposition-table)

---

## 1. Executive Summary

CodeFlow's current architecture (V3) relies on ephemeral sub-agents in a hub-and-spoke model, skill-based sentinel enforcement that blocks every tool call until a skill is invoked, a stop-time self-verification pattern that catches nothing actionable, and over-engineered configurability with four config layers, three enforcement policies, and four session types. These are not minor gaps -- they are fundamental architectural limitations that prevent CodeFlow from delivering its core promise of structured, reliable AI-native development workflows.

This proposal replaces the V3 architecture with an agent-teams-only model built on Claude Code's Agent Teams capability and guided by PathFlow, a 7-phase session lifecycle framework. The transformation touches every layer of CodeFlow: the agent model, the enforcement system, the session lifecycle, the work execution pipeline, the verification model, and the configuration surface.

**What changes:**

- **Agent model**: Hub-and-spoke sub-agents replaced by 3 persistent function teammates (cf-security, cf-knowledge-layer, cf-gitops) + 5 on-demand role teammates (cf-developer, cf-reviewer, cf-qa, cf-planner, cf-documenter) with direct peer communication
- **Mode**: Agent-teams only. No dual-mode. No standalone fallback. Simple sessions are "untracked" (lead answers directly without spawning a full team). Two orthogonal properties: interactive/autorun + tracked/untracked
- **Enforcement**: Per-skill TTL-based sentinels replaced entirely by session-scoped PathFlow file sentinels (PRIMARY, created automatically by PostToolUse hook) with JSONL events as SECONDARY fallback, enforced through a single pathflow-gate hook. SessionStart hook creates the pathflow-active flag; STAGE-COMPLETE protocol triggers work stage sentinels
- **Session lifecycle**: Ad-hoc sessions replaced by PF1-INIT through PF7-END progressive orchestration with three-mechanism coordination (instructions + tasks + hooks)
- **Verification**: Stop-time PCV self-check replaced by WS-REV stage with independent reviewer agent, earlier in pipeline, with rework capability
- **Configuration**: 4 layers / 3 policies / 4 session types collapsed to tracked vs untracked. PathFlow config in a single git-tracked JSON file
- **Skills**: All 10 procedural skills archived to `.codeflow/docs/archived/skills/`. SOPs embedded directly in agent definitions. Only cf-working-protocol retained (with verify-work replaced by WS-REV, parallelize-work replaced by native TaskCreate/teams)
- **Work stages**: Ambiguous WS-WORK split into WS-PLAN, WS-DOCS, WS-TEST with 1:1 stage-to-teammate mapping
- **Work types**: +PLAN (design/architecture/work items). SPKE clarified as POCs/prototypes only
- **Area types**: +PLN (planning area). Single ongoing epic avoids meta-for-meta recursion
- **Review**: Universal WS-REV for all 10 work types. cf-reviewer adapts criteria per work type
- **Hooks**: 28 hooks reduced to 23. Skill sentinel hooks removed. No dual-mode branching. Three new hooks: pathflow-task-guard (PathFlow task transitions), pathflow-sentinel (automatic file sentinel creation via PostToolUse), pathflow-init (flag creation at SessionStart)

**What does NOT change:**

- V4 Phases 1-3 (complete): foundation, CLI, hooks infrastructure
- Three-tier data model (JSONL -> SQLite -> Markdown)
- Dual-ID system (ULID primary keys + human-readable format_ids)
- Existing epics, tasks, and project management structure
- Security hooks (always-on, team-wide)
- Protected resource enforcement
- Git hooks and PR format enforcement

---

## 2. Problem Statement

### 2.1. The V3 Sub-Agent Model is Fundamentally Insufficient

CodeFlow V3 uses 8 sub-agents (cf-planner, cf-developer, cf-reviewer, cf-qa, cf-ops, cf-documenter, cf-support, cf-general-purpose) in a strict hub-and-spoke model where the main agent mediates all interactions.

```text
V3 Sub-Agent Model (Hub-and-Spoke)
====================================

                    +-----------+
                    | Main Agent|
                    | (mediator)|
                    +-----+-----+
                     /    |    \
                    /     |     \
          +--------+ +--------+ +--------+
          |cf-dev  | |cf-rev  | |cf-qa   |
          |(dies)  | |(dies)  | |(dies)  |
          +--------+ +--------+ +--------+
              ^           ^          ^
              |           |          |
          No peer     No peer    No peer
          comms       comms      comms
```

This architecture has five fundamental limitations that prevent the kind of session lifecycle management CodeFlow needs:

**Limitation 1: No peer communication.** When cf-developer finishes implementing code, it cannot tell cf-reviewer to start reviewing. The main agent must relay all information, consuming its context window and creating a sequential bottleneck. Every interaction is mediated, every handoff is manual.

**Limitation 2: No persistence.** Sub-agents are ephemeral -- they execute a task, return results, and die. Their context is discarded. If cf-reviewer finds issues and cf-developer needs to rework, a NEW cf-developer must be spawned, re-reading the entire codebase from scratch. The cost of context re-establishment compounds with every rework cycle.

**Limitation 3: Context bottleneck on main agent.** Because the main agent mediates everything, its context window accumulates the full conversation history of every sub-agent interaction. For a complex feature involving planning, development, review, and testing, the main agent's context fills rapidly, degrading quality in later stages.

**Limitation 4: No shared task visibility.** Sub-agents cannot see what other sub-agents are working on. There is no shared task list, no dependency graph, no way for one agent to know whether a prerequisite has been completed. The main agent is the only entity with full state awareness.

**Limitation 5: Sequential execution only.** A developer and a reviewer cannot work simultaneously. The main agent must finish one sub-agent interaction before starting another. Parallel work is impossible within the hub-and-spoke model.

**Impact on CodeFlow:** These limitations make it impossible to implement structured work pipelines where development flows to review flows to testing with rework loops, session lifecycle management where phases progress with enforcement, or any form of teammate specialization that accumulates useful context across operations.

*Source: `.codeflow/docs/research/pathflow-v3/01-agent-teams-and-pathflow.md`, Section 1.1*

### 2.2. Skill-Based Sentinel Enforcement is Disruptive

CodeFlow V3 enforces workflow ordering through skill-based sentinels: file markers created when a skill is invoked, checked by PreToolUse hooks before allowing operations. Every Edit, Write, and git operation is blocked until the corresponding skill sentinel exists.

**The mechanism:**

1. User requests work
2. Agent must invoke skill (e.g., `cf-task-management:ensure-work-registered`)
3. PostToolUse hook creates sentinel file with 600-second TTL
4. PreToolUse hook checks sentinel before allowing Edit/Write
5. If sentinel expired (>600s), agent is blocked again until skill is re-invoked

**Why this is disruptive:**

| Problem | Detail |
|---------|--------|
| **Every tool call is blocked** | The agent cannot edit a file, write a file, or run a git command without first invoking the corresponding skill. This creates constant friction in every workflow. |
| **TTL-based expiry causes false negatives** | Sentinels expire after 600 seconds. In a long session with research phases, the sentinel expires and the agent is blocked mid-work, forced to re-invoke a skill it already completed. |
| **Granular per-skill markers** | ~8-12 sentinels per workflow, each tracking a specific skill operation. The combinatorial complexity of "which sentinels are needed for which operations" is difficult to reason about and maintain. |
| **Couples enforcement to skill invocation** | The sentinel system assumes skills are always invoked through the forked sub-agent model. In an agent-teams model where teammates have SOPs embedded in their agent definitions, the invocation pattern is different. |
| **No graceful degradation** | If sentinel creation fails or hooks malfunction, the session is stuck. There is no fallback mechanism. |

**The deeper problem:** Skill-based sentinels enforce a workflow through blocking -- "you cannot proceed until you prove you did X." This is the wrong enforcement primitive for a team-based model where work flows through phases managed by a lead orchestrator. The enforcement should be phase-based ("work must be classified before coding starts"), not skill-based ("you must have invoked cf-task-management:ensure-work-registered within the last 600 seconds").

*Source: `.codeflow/docs/research/pathflow-v3/08-enforcement-model.md`, Sections 8.2-8.5*

### 2.3. No Structured Session Lifecycle

V3 sessions are ad-hoc. A session starts when the user opens Claude Code and ends when they close it. There are no phases, no progressive orchestration, no structured progression from context loading to work classification to execution to verification to completion.

**What this means in practice:**

- The agent must figure out "where am I?" at every prompt (is there active work? which task? what branch?)
- There is no mechanism to ensure work is registered before coding starts -- only the disruptive sentinel system (2.2)
- Teammate spawning has no lifecycle: all or nothing at session start vs. progressive scaling
- Session cleanup is unreliable: no structured teardown means state can leak between sessions
- The main agent carries the full burden of orchestration with no framework to guide it

**Consequences:**

- Sessions are fragile. A mid-session context compaction can cause the agent to "forget" what phase it was in
- There is no checkpoint mechanism. If a session is interrupted, there is no structured way to resume from where it left off
- The agent makes different quality-of-workflow decisions depending on its current context window state, not on a deterministic lifecycle

### 2.4. Self-Verification (PCV) is Inadequate

The current verification model uses Post-Completion Verification (PCV): a stop hook that checks whether the agent's last message contains a verification marker (the `verify-work` pattern with tier-specific sections). This has four fundamental problems:

| Problem | Detail |
|---------|--------|
| **Stop-time is too late** | By the time the agent stops, the work is "done." Catching issues at stop time means they go unaddressed -- the session is over. |
| **Advisory only** | Stop hooks in Claude Code always exit 0. They cannot block. The PCV check is purely informational; it has no enforcement power. |
| **Self-verification is weak** | The same agent that wrote the code is verifying it. This is grading your own homework. Self-review consistently misses issues that an independent reviewer would catch. |
| **Autorun incompatible** | In autorun mode (batch processing without human supervision), there is no human to see the advisory PCV output. The check is invisible and therefore useless. |

**What should replace it:** Verification should happen DURING the work pipeline (not after it), by an INDEPENDENT agent (not the author), with ENFORCEMENT power (blocks progression to next stage), and REWORK capability (routes back to the author for fixes).

*Source: `.codeflow/docs/research/pathflow-v3/08-enforcement-model.md`, Section 8.8*

### 2.5. Over-Engineered Configurability

The V3 specification accumulated layers of configuration that have no real use cases:

| Dropped Item | What It Was | Why It's Unnecessary |
|-------------|-------------|---------------------|
| 4 configuration layers | Session config, project config, user config, system config | One tracked/untracked distinction handles all real cases |
| 3 enforcement policies | strict, standard, permissive | Full enforcement (tracked) or minimal (untracked) covers every scenario. "Permissive enforcement" is an oxymoron. |
| 4 session types | interactive, autorun, structured, unstructured | 2 orthogonal properties suffice: interactive/autorun + tracked/untracked |
| Solo mode | Sessions without teammates | Not a "mode." The lead simply decides whether to create a team based on the work. |
| Session profiles | Pre-defined behavioral presets | Derived from work_type + session properties. No need for an additional abstraction layer. |

**The deeper problem:** Every configuration option that was "made up" -- invented to handle a hypothetical scenario rather than a demonstrated need -- adds cognitive overhead, maintenance burden, and test surface. Configuration should be the minimum needed to distinguish sessions that produce tracked work from sessions that don't.

*Source: `.codeflow/docs/research/pathflow-v3/12-changes-and-claude-components.md`, Section 12.4*

### 2.6. Secondary Design Gaps

Within the context of the architectural transformation above, there are also specific design gaps in the work model:

**2.6.1. WS-WORK Ambiguity.** The current V4 spec uses a generic WS-WORK stage as a catch-all for non-development primary work. For DOCS tasks, WS-WORK means "cf-documenter writes documentation." For SPKE tasks, it means "cf-planner does research." Same stage name, different semantics, different teammates, different review criteria. The knowledge layer cannot route work based on stage alone.

**2.6.2. Inconsistent Review Coverage.** The current V4 stage configuration applies WS-REV to only FEAT and DOCS work types. Bug fixes, chores, tests, hotfixes, and spikes all skip review entirely. In an AI-agent context where teammates work autonomously, every piece of work should have an independent check.

**2.6.3. Missing Work Type: PLAN.** No work type exists for pure planning and design work. When a user asks to "design the authentication architecture," the system has no appropriate classification. SPKE is specifically for throwaway POCs/prototypes. PLAN work produces durable deliverables: ADRs, architecture documents, epic/task definitions.

**2.6.4. Missing Area Type: PLN.** Planning tasks don't belong in any existing area. DOC is for project documentation. XCUT is for cross-cutting implementation. Putting planning in the area being planned creates meta-for-meta recursion.

**2.6.5. Unit Test Placement.** Ambiguity about whether unit tests are written by cf-developer in WS-DEV or by cf-qa in WS-QA. Unit tests are tightly coupled to the code; the developer writing the code is best positioned to write them. WS-QA should focus on integration testing and acceptance verification.

---

## 3. Proposed Solution: Agent Teams + PathFlow

### 3.1. Agent Teams: The Platform Capability

Claude Code's Agent Teams feature (experimental, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`) provides five capabilities that directly address the V3 sub-agent limitations:

```text
Agent Teams Model (Direct Communication)
==========================================

                    +-----------+
                    | Team Lead |
                    |(orchestr.)|
                    +-----+-----+
                     /    |    \
                    /     |     \
          +--------+ +--------+ +--------+
          |cf-dev  | |cf-rev  | |cf-qa   |
          |(alive) | |(alive) | |(alive) |
          +---+----+ +---+----+ +---+----+
              |           |          |
              +-----------+----------+
              Direct peer messaging
              Shared task list
```

| V3 Limitation | Agent Teams Capability | What It Enables |
|---------------|----------------------|-----------------|
| No peer communication | **Direct peer messaging** (SendMessage) | cf-developer sends commits to cf-gitops directly; cf-reviewer sends feedback to cf-developer; no hub-and-spoke bottleneck |
| No persistence | **Persistent teammates** | cf-gitops loads SOPs once, handles all git ops for the session; context accumulates usefully |
| No shared visibility | **Shared task list with dependencies** | Phase markers visible to all; work tasks show dependencies; every teammate sees session state |
| Sequential only | **On-demand team scaling** | Teammates spawned progressively as needed; team scales up and down with work |
| All-or-nothing lifecycle | **Individual lifecycle control** | Shut down role teammates after their stage; keep function teammates alive; recycle if context fills |

*Source: `.codeflow/docs/research/pathflow-v3/01-agent-teams-and-pathflow.md`, Sections 1.2-1.3*

**What Agent Teams does NOT solve** (CodeFlow must handle these):

| Concern | CodeFlow Solution |
|---------|-------------------|
| Cross-session persistence | WorkGraph (SQLite/JSONL) |
| Workflow enforcement (task deps are advisory only) | Hooks + PathFlow sentinels |
| Git convention enforcement | cf-gitops teammate + hooks |
| Three-tier data persistence | cf-knowledge-layer teammate |
| Security enforcement | cf-security teammate + hooks |
| Batch orchestration | Autorun system |

### 3.2. PathFlow: The Session Lifecycle Framework

PathFlow is a 7-phase logical progression framework that guides sessions from start to finish. It replaces the ad-hoc session model (Problem 2.3) with deterministic phases:

**Pre-PF1: Team Infrastructure Setup**

Before PathFlow phases begin, the team lead establishes the team infrastructure. This is NOT a PathFlow phase -- it is session bootstrapping driven by CLAUDE.md instructions and the SessionStart hook:

```text
Session opens → Claude Code starts
  |
  |-- SessionStart hook fires:
  |     Provides PathFlow instructions, checks for active work
  |
  |-- CLAUDE.md Section 4 (PathFlow Session Management):
  |     "You are the team lead. Always create a team at session start."
  |
  |-- TeamCreate("session-work")    ← ALWAYS, regardless of work complexity
  |     Creates team config + task list directory (lightweight, no teammates yet)
  |
  v
PF1-INIT begins (team infrastructure exists, zero teammates spawned so far)
```

**TeamCreate vs teammate spawning -- important distinction:**

- **TeamCreate** (team infrastructure): ALWAYS happens at session start. Lightweight -- creates a config file and task list directory. No teammates are spawned yet.
- **Teammate spawning**: Happens progressively during PathFlow phases. cf-security at PF1-INIT, cf-knowledge-layer at PF2-CONTEXT, cf-gitops at PF3-CLASSIFY. For untracked sessions, only cf-security and cf-knowledge-layer may be spawned (or none, if the lead answers directly before PF1).

**Why TeamCreate is pre-PF1, not part of PF1:**

- PF1-INIT spawns cf-security as a teammate -- the team must exist first
- PF1 creates the session record -- this requires team context for `team_name`
- The lead's decision to create a team is a CLAUDE.md behavioral instruction, not a PathFlow phase task
- A team with zero teammates is valid -- the lead can answer directly within a team context

**TeamCreate timing:** The lead creates the team immediately at session start, BEFORE evaluating the user's first message. This ensures team infrastructure exists whether the session turns out to be tracked or untracked. For simple questions, the lead answers directly within the team context without spawning any teammates or entering PathFlow phases.

```text
PF1-INIT      PF2-CONTEXT     PF3-CLASSIFY     PF4-EXECUTE    PF5-VERIFY     PF6-COMPLETE  PF7-END
Session  -->  Context    -->  Work        -->  Work      -->  Work      -->  Work     -->  Session
Start         Awareness       Classification   Execution      Verification   Completion    End
(init DB,     (load ctx,      (classify,       (DEV/REV/QA    (criteria      (PR, mark     (shutdown,
 spawn        detect work)    register,        stage          met?)          complete)     cleanup)
 cf-security)                 branch,          pipeline)
                              spawn gitops)
```

**Phase progression with PathFlow tasks:**

Each phase creates session-scoped PathFlow tasks (format: `PF{N}-TSK-{NN}`) that serve as a checklist for phase completion. These are ephemeral -- created at phase entry, disposed at session end. They are distinct from project tasks stored in the `tasks` table.

| Phase | Task ID | Description |
|-------|---------|-------------|
| PF1-INIT | PF1-TSK-01 | Initialize PathFlow session record |
| PF1-INIT | PF1-TSK-02 | Register session in DB/JSONL with `tracking_level='pending'` |
| PF1-INIT | PF1-TSK-03 | Spawn cf-security teammate |
| PF2-CONTEXT | PF2-TSK-01 | Spawn cf-knowledge-layer teammate |
| PF2-CONTEXT | PF2-TSK-02 | Query active work and session state |
| PF2-CONTEXT | PF2-TSK-03 | Load memory context for current work |
| PF2-CONTEXT | PF2-TSK-04 | Determine tracked vs untracked decision |
| PF3-CLASSIFY | PF3-TSK-01 | Classify work type and area |
| PF3-CLASSIFY | PF3-TSK-02 | Register task in WorkGraph |
| PF3-CLASSIFY | PF3-TSK-03 | Spawn cf-gitops teammate |
| PF3-CLASSIFY | PF3-TSK-04 | Create feature branch |
| PF3-CLASSIFY | PF3-TSK-05 | Activate session: set `tracking_level='tracked'`, populate `work_type`, `area_type` |
| PF4-EXECUTE | PF4-TSK-01 | Look up stage pipeline from `work_type_stages` |
| PF4-EXECUTE | PF4-TSK-02 | Execute primary stage (WS-DEV, WS-PLAN, WS-DOCS, or WS-TEST) |
| PF4-EXECUTE | PF4-TSK-03 | Execute WS-REV stage (universal review) |
| PF4-EXECUTE | PF4-TSK-04 | Execute WS-QA stage (if pipeline includes it) |
| PF5-VERIFY | PF5-TSK-01 | Verify all pipeline stages completed with pass verdict |
| PF5-VERIFY | PF5-TSK-02 | Check acceptance criteria met |
| PF6-COMPLETE | PF6-TSK-01 | Update task status to complete in WorkGraph |
| PF6-COMPLETE | PF6-TSK-02 | Create PR via cf-gitops |
| PF7-END | PF7-TSK-01 | Shutdown on-demand role teammates (SendMessage shutdown_request) |
| PF7-END | PF7-TSK-02 | Shutdown persistent function teammates (SendMessage shutdown_request) |
| PF7-END | PF7-TSK-03 | Write session summary to JSONL |
| PF7-END | PF7-TSK-04 | Clean up session state and temp files |
| PF7-END | PF7-TSK-05 | Remove pathflow-active flag (unlocks team-guard hook) |
| PF7-END | PF7-TSK-06 | TeamDelete (dissolve team infrastructure) |

**Task template source:** These task definitions come from `pathflow-config.json` `phases.{phase}.tasks` array. At phase entry, the lead creates `pathflow_tasks` rows from the config template, substituting the current `session_id`. PathFlow tasks are ephemeral -- created at phase entry, disposed at PF7-END. They are distinct from project tasks in the `tasks` table.

**Three-mechanism model** -- PathFlow coordinates through three complementary mechanisms, each addressing a different failure mode:

```text
+-------------------+     +-------------------+     +-------------------+
|   INSTRUCTIONS    |     |      TASKS        |     |      HOOKS        |
|   (Flow Logic)    |     |   (Visibility)    |     |   (Enforcement)   |
+-------------------+     +-------------------+     +-------------------+
| Agent definitions |     | PathFlow tasks    |     | PreToolUse hooks  |
| CLAUDE.md rules   |     | (PF{N}-TSK-{NN}) |     | PostToolUse hooks |
| Spawn prompts     |     | Work stage tasks  |     | PathFlow gate     |
| Embedded SOPs     |     | Dependencies      |     | Graceful degrade  |
+-------------------+     +-------------------+     +-------------------+
  "Teammates follow        "Everyone can see       "Hard guardrails
   the process because      where the session       that catch violations
   they're told to"         is and what's blocked"  even if instructions
                                                     are ignored"
```

**Progressive orchestration** -- The lead creates the next phase only when the current phase completes. This contrasts with upfront choreography where the entire task graph is defined at session start and cannot adapt to discoveries made during execution.

**Session ID lifecycle:**

The session record is created at PF1-INIT with `tracking_level='pending'` and activated at PF3-CLASSIFY:

```text
PF1-INIT:
  session_id = "session-{ulid}"
  tracking_level = 'pending'    ← session exists but tracking decision not yet made
  work_type = NULL
  area_type = NULL
  pathflow_phase = 'PF1-INIT'

PF2-CONTEXT:
  Lead evaluates: is this tracked work or a simple question?
  tracking_level still 'pending'

PF3-CLASSIFY (tracked):
  tracking_level = 'tracked'    ← activated
  work_type = 'FEAT'
  area_type = 'FRT'
  pathflow_phase = 'PF3-CLASSIFY'

PF2-CONTEXT (untracked decision):
  tracking_level = 'untracked'  ← activated early, skip PF3-PF6
  pathflow_phase = 'PF7-END'    ← jump to end
```

**Why create early (PF1, not PF3):**

1. **Audit trail**: Every hook action from PF1 onwards references a `session_id`
2. **Hook context**: pathflow-gate needs `session_id` to read current phase from JSONL
3. **Crash recovery**: If session dies at PF2, the session record exists for debugging
4. **Security context**: cf-security teammate (spawned at PF1) needs to know which session it guards

The session record lives in `.state/runtime/current-session-id` (file) and the `sessions` table (DB/JSONL).

*Source: `.codeflow/docs/research/pathflow-v3/02-system-overview.md`, Sections 2.1-2.3*

### 3.3. PathFlow Sentinels Replace Skill Sentinels

Session-scoped PathFlow sentinels replace the disruptive TTL-based skill sentinels entirely. There is no dual-mode coexistence -- skill sentinels are removed, not preserved as a fallback.

| Aspect | Skill Sentinels (V3, removed) | PathFlow Sentinels (implemented) |
|--------|-------------------------------|------------------------------|
| Naming | `{skill}:{operation}` | `pathflow-{phase}` or `pathflow-ws-{stage}` |
| Example | `cf-task-management:ensure-work-registered` | `pathflow-pf-3`, `pathflow-ws-dev` |
| TTL | 600 seconds (expires!) | None (session-scoped) |
| Created when | Skill operation invoked | Phase/stage completed (PostToolUse hook, automatic) |
| Storage | File in sentinel directory | **PRIMARY:** File sentinel (`[[ -f file ]]`). **SECONDARY:** JSONL event (for cf-knowledge-layer maturity) |
| Quantity | ~8-12 per workflow | ~3-7 per session (phases + work stages) |
| Checked by | Per-skill sentinel hooks (5 hooks) | Single pathflow-gate hook |
| Failure mode | Session blocked on expiry | Graceful degradation (advisory) |
| Read mechanism | File existence check | **PRIMARY:** File existence check (`has_sentinel()`). **SECONDARY:** JSONL tail + jq parse (fallback) |

**Key simplification:** `pathflow:PF3-CLASSIFY` (work classified and registered) implicitly covers what multiple skill sentinels would check. If PF3-CLASSIFY completed, work IS registered, work IS classified, branch IS created. No separate `ensure-work-registered`, `classify-work`, or `begin-work` sentinels needed.

**Flag-based activation:** Since the system is agent-teams only, there is no dual-mode branching or fallback to skill sentinels. A `pathflow-active` flag file is created by the SessionStart pathflow-init hook and checked by `is_pathflow_active()` in enforcement hooks (pathflow-gate, team-guard, stop). The flag contains JSON metadata (session_id, team_name, created_at, tracking_level) and is removed at session end. File-based sentinels are the PRIMARY enforcement mechanism; JSONL events serve as a SECONDARY fallback for when cf-knowledge-layer matures.

*Source: `.codeflow/docs/research/pathflow-v3/08-enforcement-model.md`, Sections 8.2-8.5*

### 3.4. WS-REV Replaces PCV

The WS-REV (Review) work stage replaces the PCV self-verification pattern (Problem 2.4):

| Aspect | PCV (V3) | WS-REV (proposed) |
|--------|----------|-------------------|
| **When** | Stop time (after all work is done) | After primary stage (before QA) |
| **Who** | Same agent (self-review) | Separate cf-reviewer agent |
| **Enforcement** | Advisory only (stop hooks exit 0) | Blocks QA stage until approved |
| **Autorun** | Invisible (no human to see it) | Automated reviewer with rework routing |
| **Rework** | Cannot trigger rework | Routes back to primary stage on failure |
| **Auditability** | Last-message text matching | Full stage_history in pathflow_stage_log |

The stop hook's PCV behavior is removed (the hook remains for phase-gate logging only). cf-working-protocol's `verify-work` operation is replaced by the WS-REV stage. The `parallelize-work` operation is replaced by Claude's native TaskCreate/team orchestration. Remaining cf-working-protocol operations stay active for the team lead: meta-awareness, think-and-act, decide, respond-organized, research-quality.

### 3.5. Teammate Architecture

**Three persistent function teammates** (session lifetime):

| Teammate | Embedded SOPs From | Spawn Phase | Shutdown Phase | Purpose |
|----------|-------------------|-------------|----------------|---------|
| cf-security | cf-security-management | PF1-INIT | PF7-END | Security checks, sandbox validation, protected resource consultation |
| cf-knowledge-layer | cf-memory-management, cf-task-management, cf-db-operations | PF2-CONTEXT | PF7-END | WorkGraph, memory, DB -- single interface to persistent storage |
| cf-gitops | cf-git-workflow | PF3-CLASSIFY | PF7-END | All git operations following conventions |

**Five on-demand role teammates** (spawned per stage, shutdown after):

| Teammate | Work Stage | Purpose |
|----------|-----------|---------|
| cf-developer | WS-DEV | Code implementation + unit tests. Also handles CICD work (cf-ops absorbed). |
| cf-planner | WS-PLAN | Design, architecture, analysis, investigation |
| cf-documenter | WS-DOCS | Documentation writing |
| cf-reviewer | WS-REV | Independent work review (adapts criteria per work type: CODE_REVIEW, DESIGN_REVIEW, DOCUMENTATION_REVIEW, TEST_REVIEW) |
| cf-qa | WS-QA | Integration testing, acceptance verification (quality gate for others' work) |
| cf-qa | WS-TEST | Primary test implementer when work type is TEST (tests ARE the deliverable) |

**cf-ops absorbed:** The cf-ops role is absorbed into cf-developer for CICD work type. CI/CD pipelines are code -- the developer writes them, the reviewer checks them, QA validates them. A separate ops teammate adds an unnecessary abstraction.

**Ad-hoc flexibility:** The predefined roster is optimized defaults, not constraints. The lead can always spawn general-purpose teammates, specialized one-offs (security-auditor, perf-analyst), Explore sub-agents for quick lookups, or multiple instances of the same role for parallel work.

**Teammate hook firing:** Teammates operate within the same session as the team lead. PostToolUse hooks fire for ALL tool calls in the session, including those from teammates. This is how the sentinel PostToolUse hook automatically creates sentinels when teammates (e.g., cf-development) send STAGE-COMPLETE messages -- the hook fires on the teammate's SendMessage call. However, `.*` regex matchers do NOT match Agent Teams tools (TeamCreate, Task, SendMessage, TeamDelete); these require explicit tool names in the matcher. Empirically validated in `.codeflow/docs/research/agent-teams-hook-findings.md`.

*Source: `.codeflow/docs/research/pathflow-v3/03-teammate-model.md`, Sections 3.1-3.9*

### 3.6. Skills Transformation

All skills are archived except cf-working-protocol. SOPs from archived skills are embedded directly into agent teammate definitions. The `.claude/skills/` directory goes from 11 skills to 1.

| Skill | Disposition | New Home |
|-------|------------|----------|
| cf-working-protocol | **RETAINED** (modified) | `.claude/skills/cf-working-protocol/` -- verify-work removed (WS-REV replaces), parallelize-work removed (native TaskCreate/teams replaces) |
| cf-git-workflow | **ARCHIVED** | SOPs embedded in `cf-gitops.md` agent definition |
| cf-memory-management | **ARCHIVED** | SOPs embedded in `cf-knowledge-layer.md` agent definition |
| cf-task-management | **ARCHIVED** | SOPs embedded in `cf-knowledge-layer.md` agent definition |
| cf-db-operations | **ARCHIVED** | SOPs embedded in `cf-knowledge-layer.md` agent definition |
| cf-security-management | **ARCHIVED** | SOPs embedded in `cf-security.md` agent definition |
| cf-script-standards | **ARCHIVED** | Relevant conventions embedded in `cf-developer.md` |
| cf-documentation-standards | **ARCHIVED** | Relevant conventions embedded in `cf-documenter.md`, `cf-planner.md` |
| cf-code-exploration | **ARCHIVED** | No dedicated embedding; general search patterns in relevant agents |
| cf-testing-workflow | **ARCHIVED** | SOPs embedded in `cf-qa.md` agent definition |

**Archive location:** `.codeflow/docs/archived/skills/` -- preserved as historical reference, not active skill definitions.

**Skill sentinel system eliminated entirely:** No TTL-based sentinels, no sentinel creation hooks, no sentinel check hooks. The agent definitions ARE the SOPs now; there is no need to verify invocation through file markers.

### 3.7. Configuration Simplification

```text
V3 Configuration (removed)           Proposed (replacement)
=========================            =======================
4 config layers (session,            Tracked / Untracked mode
  project, user, system)              (single boolean property)

3 enforcement policies               Full enforcement (tracked)
  (strict, standard, permissive)      or minimal (untracked)

4 session types (interactive,         2 orthogonal properties:
  autorun, structured,                 interactive / autorun
  unstructured)                        tracked / untracked

Solo mode                             Not a mode. Lead decides
                                       whether to create team
                                       based on work complexity.

Session profiles                      Derived from work_type
                                       + session properties.
```

**Session properties (2 orthogonal axes):**

| Property | Values | Meaning |
|----------|--------|---------|
| Interaction | interactive / autorun | Is a human present? |
| Tracking | tracked / untracked | Is work registered in WorkGraph? |

In an untracked session, the lead answers the user's question directly without spawning role teammates or progressing through PF3-CLASSIFY through PF6-COMPLETE. Persistent teammates (cf-security, cf-knowledge-layer) may still be active if the session started tracked and transitioned, but for truly simple queries the lead handles everything.

**Note on tracked-to-untracked transition:** Once a session is tracked, it stays tracked. The reverse does not happen. If a user changes their mind mid-session ("actually, never mind"), the session proceeds to PF7-END for clean shutdown rather than reverting to untracked. A `/cf-abandon` command to fast-track from any phase to PF7-END without creating a PR is deferred to the Go CLI.

**Settings cleanup:** The `_codeflow.agent_teams` and `pathflow_mode` configuration blocks in `settings.json` are removed. The `is_pathflow_active()` function checks only the flag file (created by SessionStart hook), not settings.json. The `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1` environment variable (a Claude Code platform toggle) is retained.

---

## 4. Design Decisions

### 4.1. Area Types: Adding PLN

| Code | Area | Description | Folder |
|------|------|-------------|--------|
| FRT | Frontend | UI, components, client logic | `frontend/` |
| BKD | Backend | API, services, server logic | `backend/` |
| INF | Infrastructure | CI/CD, deployment, config | `infrastructure/` |
| SHR | Shared | Common libraries, types | `shared/` |
| DOC | Documentation | Docs, guides, ADRs | `documentation/` |
| XCUT | Cross-cutting | Spans multiple areas | `cross-cutting/` |
| **PLN** | **Planning** | **Design, architecture, work item creation** | **`planning/`** |

**Rationale:** Separates "work about work" from deliverable work. Avoids meta-for-meta recursion (epics-creating-epics in the same area). Clean mapping: PLN area + PLAN work type = planning pipeline.

### 4.2. Work Types: Adding PLAN, Clarifying SPKE

| Code | Name | Branch Prefix | Commit Type | Purpose |
|------|------|---------------|-------------|---------|
| FEAT | Feature | `feat/` | `feat` | New functionality |
| FIX | Bug Fix | `fix/` | `fix` | Bug resolution |
| HTFX | Hotfix | `hotfix/` | `fix` | Urgent production fix |
| RFCT | Refactor | `refactor/` | `refactor` | Code restructuring |
| DOCS | Documentation | `docs/` | `docs` | Documentation work |
| TEST | Testing | `test/` | `test` | Test creation/updates |
| CHOR | Chore | `chore/` | `chore` | Maintenance tasks |
| CICD | CI/CD | `ci/` | `ci` | Build/deploy pipeline |
| SPKE | Spike | `spike/` | `chore` | POCs, prototypes, technical investigations |
| **PLAN** | **Planning** | **`plan/`** | **`chore`** | **Design, architecture, epic/task creation** |

**SPKE vs PLAN:**

| Aspect | SPKE (Spike) | PLAN (Planning) |
|--------|-------------|----------------|
| Purpose | Technical investigation, POCs | Design, architecture, work items |
| Produces | Throwaway code, findings | ADRs, architecture docs, epics, tasks |
| Durability | Temporary (branch may be discarded) | Permanent (deliverables merged) |
| Example | "Can we use WebSockets?" | "Design the auth system architecture" |
| Pipeline | WS-PLAN -> WS-REV | WS-PLAN -> WS-REV |

### 4.3. Work Stages: Eliminating WS-WORK

**Before (V4 current):** 4 stages with ambiguous WS-WORK

| Stage | Problem |
|-------|---------|
| WS-DEV | Clear |
| WS-WORK | Ambiguous: different teammates, different review criteria depending on work type |
| WS-REV | Applied inconsistently (only FEAT and DOCS) |
| WS-QA | Clear |

**After (proposed):** 6 stages with 1:1 stage-to-teammate mapping

| Stage Code | Assigned Teammate | Purpose |
|------------|-------------------|---------|
| WS-DEV | cf-developer | Code implementation + unit tests |
| WS-PLAN | cf-planner | Design, architecture, analysis, investigation |
| WS-DOCS | cf-documenter | Documentation writing |
| WS-TEST | cf-qa (as primary dev) | Test creation/framework work |
| WS-REV | cf-reviewer | Independent review of primary stage output (universal) |
| WS-QA | cf-qa | Integration testing, acceptance verification |

**Key properties:**

1. **1:1 stage-to-teammate mapping** -- the knowledge layer can route work based on stage alone
2. **Universal WS-REV** -- every work type gets review; cf-reviewer adapts criteria per work type
3. **Unit tests in WS-DEV** -- developer writes code and unit tests together; WS-QA handles integration
4. **WS-TEST for test-only work** -- when work type is TEST, cf-qa acts as primary developer

**WS-TEST vs WS-QA -- same teammate, different responsibility mode:**

| Aspect | WS-TEST | WS-QA |
|--------|---------|-------|
| When triggered | Work type = TEST | Work type = FEAT, FIX, RFCT, CICD |
| cf-qa role | **Primary implementer** | **Quality gate** |
| Deliverable | Tests ARE the output (new test suite, coverage expansion) | Verification of someone else's code (integration tests, acceptance) |
| Example | "Add unit tests for auth module" | "Verify login feature works end-to-end" |

The distinction is ownership: in WS-TEST, cf-qa writes tests as the primary output. In WS-QA, cf-qa validates that cf-developer's code works correctly.

### 4.4. Work Type Pipelines (All 10)

| Work Type | Pipeline | Rationale |
|-----------|----------|-----------|
| **FEAT** | WS-DEV -> WS-REV -> WS-QA | Full pipeline: code, review, integration tests |
| **FIX** | WS-DEV -> WS-REV -> WS-QA | Bug fixes need review (catches regressions) + QA |
| **HTFX** | WS-DEV -> WS-REV | Urgent: review for safety, skip QA for speed |
| **RFCT** | WS-DEV -> WS-REV -> WS-QA | Refactors change behavior paths, need thorough verification |
| **DOCS** | WS-DOCS -> WS-REV | Documentation reviewed for accuracy and completeness |
| **TEST** | WS-TEST -> WS-REV | Test suite work reviewed for coverage and correctness |
| **CHOR** | WS-DEV -> WS-REV | Maintenance reviewed for unintended side effects |
| **CICD** | WS-DEV -> WS-REV -> WS-QA | CI/CD changes need review + verification (high blast radius) |
| **SPKE** | WS-PLAN -> WS-REV | Investigation reviewed for thoroughness and conclusions |
| **PLAN** | WS-PLAN -> WS-REV | Design reviewed for completeness and feasibility |

**cf-reviewer adaptation per work type:**

| Reviewing... | Review Mode | Focus |
|-------------|-------------|-------|
| WS-DEV output | CODE_REVIEW | Code quality, security, tests present, conventions |
| WS-PLAN output | DESIGN_REVIEW | Analysis completeness, feasibility, requirements coverage |
| WS-DOCS output | DOCUMENTATION_REVIEW | Accuracy, structure, completeness, standards compliance |
| WS-TEST output | TEST_REVIEW | Coverage adequacy, test quality, edge cases, assertions |

#### 4.4.1. Pipeline Extensibility: Custom Stages

The work_type_stages routing engine is designed for extensibility. Custom stages can be added to any work type's pipeline without code changes.

**Example: Adding a dedicated security audit stage to FEAT pipeline**

Current FEAT pipeline: `WS-DEV → WS-REV → WS-QA`
Extended FEAT pipeline: `WS-DEV → WS-REV → WS-SEC → WS-QA`

**Step 1: Define the custom stage in pathflow-config.json:**

```json
"stages": {
  "WS-SEC": { "name": "Security Audit", "teammate": "cf-security-auditor", "max_rework_iterations": 2 }
}
```

**Step 2: Add the stage to the DB routing table:**

```sql
-- Add security audit stage to FEAT pipeline (between review and QA)
INSERT INTO work_type_stages VALUES
    ('FEAT', 'sec', 3, FALSE, 'cf-security-auditor');

-- Bump QA stage_order to accommodate
UPDATE work_type_stages SET stage_order = 4
    WHERE work_type_code = 'FEAT' AND stage = 'qa';
```

**Step 3: Update the CHECK constraint (if needed):**

```sql
-- Extend the stage CHECK to include custom stages
ALTER TABLE ... CHECK(stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa', 'sec'))
```

**Step 4: Create the teammate agent definition:**

Create `.claude/agents/cf-security-auditor.md` following the 5-section format (Identity, Constraints, SOPs, Communication, Quality Checklist) with security-specific review criteria.

**Step 5: Update pathflow-config.json pipeline:**

```json
"pipelines": {
  "FEAT": ["WS-DEV", "WS-REV", "WS-SEC", "WS-QA"]
}
```

**What requires NO changes:** The pathflow-gate hook, the lead's orchestration logic, the JSONL/SQLite event system, and all other teammates are unaffected. The routing engine discovers the new stage from the data.

**Default approach for Phase 4:** Security concerns are handled within cf-reviewer's CODE_REVIEW mode (OWASP top 10, injection, auth bypass). The persistent cf-security teammate is available for consultation during any stage. The custom WS-SEC stage is an extensibility path for projects with dedicated security requirements.

### 4.5. Planning Epic: Single PLN-EPC-PLAN-GENL-001

```text
project-management/epics/planning/
  PLN-EPC-PLAN-GENL-001/          # Ongoing planning epic
    PLN-EPC-PLAN-GENL-001-epic.md
    tasks/
      PLN-TSK-PLAN-GENL-001.md    # "Design auth architecture" -> produces FRT-EPC-FEAT-AUTH-001
      PLN-TSK-PLAN-GENL-002.md    # "Plan backend refactor" -> produces BKD-EPC-RFCT-CORE-001
  PLN-EPC-SPKE-GENL-001/          # Ongoing spike epic
    PLN-EPC-SPKE-GENL-001-epic.md
    tasks/
      PLN-TSK-SPKE-GENL-001.md    # "Spike: can we use WebSockets?" -> produces findings
```

**Rationale:** Planning produces epics. Having per-area planning epics creates meta-for-meta recursion. A single planning epic provides one place to see all planned/in-progress planning work. Tasks reference their target areas in descriptions.

**Naming note:** `PLN-EPC-PLAN-GENL-001` follows the mechanical `{AREA}-EPC-{TYPE}-{DOMAIN}-{NNN}` format, which produces apparent redundancy (PLN + PLAN). This is intentional and consistent with how the format works -- `DOC-EPC-DOCS-GENL-001` would exhibit the same pattern. The format_id is a structured identifier, not prose. Both PLN (area) and PLAN (work type) are new additions introduced by this proposal (Sections 4.1 and 4.2) and require V4 spec updates with corresponding DB schema changes (Section 5.2).

---

## 5. Component Transformation Plan

### 5.1. PathFlow Configuration File

**Location:** `.codeflow/config/pathflow/pathflow-config.json`

**Properties:**

- Git-tracked, versioned, reviewed
- Read-only -- agents and hooks READ it, never WRITE it (human-edited only)
- Defines: what phases exist, what stages exist, stage sequences per work type, teammate spawn rules, rework limits
- NOT duplicated in DB -- it is the source that defines valid DB values, not a copy of them

**Config vs DB separation:**

| Concern | Config (structure) | DB (execution state) |
|---------|-------------------|---------------------|
| What it stores | Phase definitions, stage pipelines, rules | Current session state, transitions, history |
| Mutability | Human-edited only | Written by agents/hooks/CLI |
| Git tracked | Yes | No (`.state/` is gitignored) |
| Example | "FEAT pipeline = dev -> review -> qa" | "Session X is at stage=review, iteration=2" |

**Full configuration structure:**

```json
{
  "version": "1.0.0",
  "phases": {
    "PF1-INIT": {
      "name": "Session Start",
      "description": "Initialize session, register in DB, spawn security teammate",
      "tasks": [
        "PF1-TSK-01: Initialize PathFlow session record",
        "PF1-TSK-02: Register session in DB/JSONL with tracking_level='pending'",
        "PF1-TSK-03: Spawn cf-security teammate"
      ]
    },
    "PF2-CONTEXT": {
      "name": "Context Awareness",
      "description": "Load previous context, determine tracked vs untracked",
      "tasks": [
        "PF2-TSK-01: Spawn cf-knowledge-layer teammate",
        "PF2-TSK-02: Query active work and session state",
        "PF2-TSK-03: Load memory context for current work",
        "PF2-TSK-04: Determine tracked vs untracked decision"
      ]
    },
    "PF3-CLASSIFY": {
      "name": "Work Classification",
      "description": "Classify work, register task, create branch, activate session",
      "tasks": [
        "PF3-TSK-01: Classify work type and area",
        "PF3-TSK-02: Register task in WorkGraph",
        "PF3-TSK-03: Spawn cf-gitops teammate",
        "PF3-TSK-04: Create feature branch",
        "PF3-TSK-05: Activate session: set tracking_level='tracked', populate work_type, area_type"
      ]
    },
    "PF4-EXECUTE": {
      "name": "Work Execution",
      "description": "Execute work through configured stage pipeline",
      "tasks": [
        "PF4-TSK-01: Look up stage pipeline from work_type_stages",
        "PF4-TSK-02: Execute primary stage (WS-DEV, WS-PLAN, WS-DOCS, or WS-TEST)",
        "PF4-TSK-03: Execute WS-REV stage (universal review)",
        "PF4-TSK-04: Execute WS-QA stage (if pipeline includes it)"
      ]
    },
    "PF5-VERIFY": {
      "name": "Work Verification",
      "description": "Verify all stages passed and acceptance criteria met",
      "tasks": [
        "PF5-TSK-01: Verify all pipeline stages completed with pass verdict",
        "PF5-TSK-02: Check acceptance criteria met"
      ]
    },
    "PF6-COMPLETE": {
      "name": "Work Completion",
      "description": "Update task status, create PR",
      "tasks": [
        "PF6-TSK-01: Update task status to complete in WorkGraph",
        "PF6-TSK-02: Create PR via cf-gitops"
      ]
    },
    "PF7-END": {
      "name": "Session End",
      "description": "Shutdown teammates, dissolve team, cleanup",
      "tasks": [
        "PF7-TSK-01: Shutdown on-demand role teammates",
        "PF7-TSK-02: Shutdown persistent function teammates",
        "PF7-TSK-03: Write session summary to JSONL",
        "PF7-TSK-04: Clean up session state and temp files",
        "PF7-TSK-05: Remove pathflow-active flag (unlocks team-guard)",
        "PF7-TSK-06: TeamDelete (dissolve team infrastructure)"
      ]
    }
  },
  "stages": {
    "WS-DEV": { "name": "Development", "teammate": "cf-developer", "max_rework_iterations": 3 },
    "WS-PLAN": { "name": "Planning", "teammate": "cf-planner", "max_rework_iterations": 3 },
    "WS-DOCS": { "name": "Documentation", "teammate": "cf-documenter", "max_rework_iterations": 3 },
    "WS-TEST": { "name": "Testing", "teammate": "cf-qa", "max_rework_iterations": 3 },
    "WS-REV": { "name": "Review", "teammate": "cf-reviewer", "max_rework_iterations": 3 },
    "WS-QA": { "name": "Quality Assurance", "teammate": "cf-qa", "max_qa_retries": 2, "rework_target": "primary" }
  },
  "pipelines": {
    "FEAT": ["WS-DEV", "WS-REV", "WS-QA"],
    "FIX": ["WS-DEV", "WS-REV", "WS-QA"],
    "HTFX": ["WS-DEV", "WS-REV"],
    "RFCT": ["WS-DEV", "WS-REV", "WS-QA"],
    "DOCS": ["WS-DOCS", "WS-REV"],
    "TEST": ["WS-TEST", "WS-REV"],
    "CHOR": ["WS-DEV", "WS-REV"],
    "CICD": ["WS-DEV", "WS-REV", "WS-QA"],
    "SPKE": ["WS-PLAN", "WS-REV"],
    "PLAN": ["WS-PLAN", "WS-REV"]
  },
  "teammates": {
    "persistent": {
      "cf-security": { "spawn_phase": "PF1-INIT", "shutdown_phase": "PF7-END", "embeds": ["cf-security-management"] },
      "cf-knowledge-layer": { "spawn_phase": "PF2-CONTEXT", "shutdown_phase": "PF7-END", "embeds": ["cf-memory-management", "cf-task-management", "cf-db-operations"] },
      "cf-gitops": { "spawn_phase": "PF3-CLASSIFY", "shutdown_phase": "PF7-END", "embeds": ["cf-git-workflow"] }
    },
    "on_demand": {
      "cf-developer": { "stage": "WS-DEV" },
      "cf-planner": { "stage": "WS-PLAN" },
      "cf-documenter": { "stage": "WS-DOCS" },
      "cf-reviewer": { "stage": "WS-REV" },
      "cf-qa": { "stage": ["WS-QA", "WS-TEST"] }
    }
  },
  "rework": {
    "max_rework_iterations": 3,
    "max_qa_retries": 2,
    "stage_timeout_minutes": 30,
    "escalation": "lead"
  }
}
```

### 5.2. Database Schema Changes

#### 5.2.1. Add PLN Area Type

```sql
INSERT OR IGNORE INTO area_types (code, name, description, scope_patterns)
VALUES ('PLN', 'Planning', 'Design, architecture, work item creation', '["project-management/**"]');

INSERT INTO area_folder_mapping (area_code, folder_name, description)
VALUES ('PLN', 'planning', 'Design, architecture, and planning work');
```

#### 5.2.2. Add PLAN Work Type

```sql
INSERT OR IGNORE INTO work_types (code, name, branch_prefix, commit_type, urgency)
VALUES ('PLAN', 'Planning', 'plan/', 'chore', 'normal');
```

#### 5.2.3. Update Stage CHECK Constraint

```sql
-- Current:
stage TEXT CHECK(stage IN ('dev', 'work', 'review', 'qa', 'done'))

-- New:
stage TEXT CHECK(stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa', 'done'))
```

Replaces `'work'` with `'plan'`, `'docs'`, `'test'`. Affects `tasks` table and `active_work` table.

#### 5.2.4. New work_type_stages Configuration Table

**Purpose:** `work_type_stages` is the **routing engine** for PF4-EXECUTE. When the lead enters PF4, it queries this table to determine:

1. **Which stages** to run for this work type
2. **In what order** (via `stage_order`)
3. **Which teammate** handles each stage (via `assigned_teammate`)
4. **Which stage is primary** (via `is_primary`) -- the stage whose output is the main deliverable

This is **data-driven, not hardcoded**. Adding a new work type or modifying a pipeline requires only INSERT/UPDATE statements -- no code changes to hooks, agent definitions, or CLAUDE.md. The `pathflow-config.json` `pipelines` section mirrors this table for quick human reference, but the DB table is the **runtime authority** that the lead queries at PF4 entry.

The `is_primary` flag identifies the rework target: if WS-REV returns `CHANGES_REQUESTED`, rework routes back to the primary stage's teammate.

```sql
CREATE TABLE work_type_stages (
    work_type_code TEXT NOT NULL REFERENCES work_types(code),
    stage TEXT NOT NULL CHECK(stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa')),
    stage_order INTEGER NOT NULL,
    is_primary BOOLEAN DEFAULT FALSE,
    assigned_teammate TEXT,
    PRIMARY KEY (work_type_code, stage)
);

-- All 10 pipelines (24 rows total)
INSERT INTO work_type_stages VALUES
    ('FEAT', 'dev',    1, TRUE,  'cf-developer'),
    ('FEAT', 'review', 2, FALSE, 'cf-reviewer'),
    ('FEAT', 'qa',     3, FALSE, 'cf-qa'),
    ('FIX',  'dev',    1, TRUE,  'cf-developer'),
    ('FIX',  'review', 2, FALSE, 'cf-reviewer'),
    ('FIX',  'qa',     3, FALSE, 'cf-qa'),
    ('HTFX', 'dev',    1, TRUE,  'cf-developer'),
    ('HTFX', 'review', 2, FALSE, 'cf-reviewer'),
    ('RFCT', 'dev',    1, TRUE,  'cf-developer'),
    ('RFCT', 'review', 2, FALSE, 'cf-reviewer'),
    ('RFCT', 'qa',     3, FALSE, 'cf-qa'),
    ('DOCS', 'docs',   1, TRUE,  'cf-documenter'),
    ('DOCS', 'review', 2, FALSE, 'cf-reviewer'),
    ('TEST', 'test',   1, TRUE,  'cf-qa'),
    ('TEST', 'review', 2, FALSE, 'cf-reviewer'),
    ('CHOR', 'dev',    1, TRUE,  'cf-developer'),
    ('CHOR', 'review', 2, FALSE, 'cf-reviewer'),
    ('CICD', 'dev',    1, TRUE,  'cf-developer'),
    ('CICD', 'review', 2, FALSE, 'cf-reviewer'),
    ('CICD', 'qa',     3, FALSE, 'cf-qa'),
    ('SPKE', 'plan',   1, TRUE,  'cf-planner'),
    ('SPKE', 'review', 2, FALSE, 'cf-reviewer'),
    ('PLAN', 'plan',   1, TRUE,  'cf-planner'),
    ('PLAN', 'review', 2, FALSE, 'cf-reviewer');
```

**Usage at PF3-CLASSIFY:** The lead queries this table to determine the stage pipeline:

```sql
SELECT stage, stage_order, assigned_teammate
FROM work_type_stages WHERE work_type_code = ? ORDER BY stage_order;
```

Stage pipeline is data-driven, not hardcoded. Adding or modifying pipelines requires only a DB change.

#### 5.2.5. PathFlow Tables (3 New Tables + Sessions Extension)

The existing V4 `sessions` table already has `pathflow_mode` and `pathflow_phase` columns. The existing `active_work` table already has `session_id`, `current_stage`, and `team_name` columns. These are extended, not duplicated.

**Session ID format:** `session-{ulid}`, stored in `.state/runtime/current-session-id`.

Three new tables reference `sessions(id)`:

```sql
-- Phase transition audit trail
CREATE TABLE pathflow_phase_log (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    phase TEXT NOT NULL,  -- PF1-INIT through PF7-END
    status TEXT NOT NULL, -- entered, completed, skipped
    timestamp TEXT NOT NULL,
    metadata TEXT DEFAULT '{}'
);

-- Stage transitions within PF4-EXECUTE
CREATE TABLE pathflow_stage_log (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    stage TEXT NOT NULL,    -- dev, plan, docs, test, review, qa
    status TEXT NOT NULL,   -- pending, in_progress, complete, failed
    agent TEXT,             -- which teammate handled it
    verdict TEXT,           -- pass, fail, changes_requested
    iteration INTEGER DEFAULT 1,
    timestamp TEXT NOT NULL
);

-- Session-scoped checklist items (NOT project tasks)
CREATE TABLE pathflow_tasks (
    id TEXT PRIMARY KEY,    -- PF{N}-TSK-{NN} format
    session_id TEXT NOT NULL REFERENCES sessions(id),
    phase TEXT NOT NULL,
    task_order INTEGER NOT NULL,
    description TEXT NOT NULL,
    status TEXT DEFAULT 'pending',
    completed_at TEXT
);
```

**Relationship chain:**

| From | To | Via | Purpose |
|------|----|-----|---------|
| pathflow_tasks | sessions | session_id FK | Phase tasks belong to session |
| pathflow_phase_log | sessions | session_id FK | Phase history belongs to session |
| pathflow_stage_log | sessions | session_id FK | Stage history belongs to session |
| active_work | sessions | session_id FK | Work belongs to session |
| active_work | tasks | task_id FK | Work linked to project task |
| tasks | epics | epic_id FK | Project task belongs to epic |

**PathFlow tasks vs Project tasks:**

| Aspect | PathFlow Tasks (pathflow_tasks) | Project Tasks (tasks) |
|--------|--------------------------------|----------------------|
| ID Format | PF{N}-TSK-{NN} | {AREA}-TSK-{TYPE}-{DOMAIN}-{NNN} + ULID PK |
| Scope | Session-local, ephemeral | Project-wide, persistent |
| Lifecycle | Created at phase entry, disposed at session end | Created at planning, persist across sessions |
| Purpose | Phase/stage checklist items | Development work items |
| FK | session_id -> sessions | epic_id -> epics |

#### 5.2.6. JSONL Event Types

During the interim period (Period 1), all PathFlow state is written to JSONL. Event types:

```json
{"id":"pf-01...","ts":"2026-...","type":"phase_transition","session_id":"session-01...","phase":"PF1-INIT","status":"entered"}
{"id":"pf-02...","ts":"2026-...","type":"phase_transition","session_id":"session-01...","phase":"PF1-INIT","status":"completed"}
{"id":"pf-03...","ts":"2026-...","type":"stage_transition","session_id":"session-01...","stage":"WS-DEV","status":"in_progress","iteration":1}
{"id":"pf-04...","ts":"2026-...","type":"pathflow_task_update","session_id":"session-01...","task_id":"PF3-TSK-01","status":"completed"}
{"id":"pf-05...","ts":"2026-...","type":"session_metadata","session_id":"session-01...","key":"work_type","value":"FEAT"}
```

The pathflow-gate hook can read these events as a SECONDARY fallback (tail + jq parse). The PRIMARY enforcement mechanism uses file-based sentinels (see Section 6.4). JSONL events serve as an audit trail and as the future migration path to SQLite (Period 2+). Performance is acceptable because events are session-scoped (~100 lines max per session).

#### 5.2.7. Seed Data Summary

| Table | Current Rows | New Rows | Change |
|-------|-------------|----------|--------|
| area_types | 6 (FRT-XCUT) | 7 (+PLN) | +1 row |
| area_folder_mapping | 6 | 7 (+PLN->planning) | +1 row |
| work_types | 9 (FEAT-SPKE) | 10 (+PLAN) | +1 row |
| work_type_stages | 0 (new table) | 24 | New table |
| tasks.stage CHECK | 5 values | 7 values | Replace 'work' with 'plan','docs','test' |
| pathflow_phase_log | 0 (new table) | Runtime | New table |
| pathflow_stage_log | 0 (new table) | Runtime | New table |
| pathflow_tasks | 0 (new table) | Runtime | New table |

#### 5.2.8. PathFlow Config and DB Coordination

The `pathflow-config.json` file and the database tables serve complementary roles with a clear separation of concerns:

```text
pathflow-config.json (STRUCTURE)          DB Tables (RUNTIME STATE)
================================          ========================
"What phases exist"                       "Which phase is session X in"
"What stages exist"                       "When did session X enter WS-DEV"
"FEAT pipeline = [DEV,REV,QA]"           "Session X is at WS-REV, iteration 2"
"max_rework_iterations = 3"               "Session X has done 2 rework cycles"
"cf-developer handles WS-DEV"             "cf-developer started at 14:30, finished at 14:45"

Human-edited, git-tracked                 Agent/hook-written, .state/ (gitignored)
Read-only for agents                      Read-write for agents
Source of truth for RULES                 Source of truth for STATE
```

**Interaction flow at PF4-EXECUTE:**

1. **Lead reads config** → `pathflow-config.json.pipelines.FEAT` = `["WS-DEV", "WS-REV", "WS-QA"]`
2. **Lead queries DB** → `SELECT * FROM work_type_stages WHERE work_type_code = 'FEAT' ORDER BY stage_order`
3. **DB returns routing** → stage order, assigned teammates, primary flag
4. **Lead spawns teammate** → `cf-developer` for WS-DEV (from DB `assigned_teammate` column)
5. **Lead writes DB** → `INSERT INTO pathflow_stage_log (session_id, stage, status, agent)` with `status='in_progress'`
6. **Teammate completes** → Lead updates `pathflow_stage_log` with `status='complete'`, `verdict='pass'`
7. **Lead checks config** → `pathflow-config.json.rework.max_rework_iterations` = 3 (for rework limit enforcement)
8. **Lead advances** → Next stage in pipeline, repeat steps 4-6

**Redundancy between config and DB is intentional:**

The `pipelines` section in `pathflow-config.json` and the `work_type_stages` DB table contain overlapping information (stage order per work type). This is by design:

| Source | Used by | Purpose |
|--------|---------|---------|
| `pathflow-config.json.pipelines` | Humans reviewing config, documentation | Quick reference for pipeline structure |
| `work_type_stages` DB table | Lead agent at PF4 entry, hooks | Runtime routing with teammate assignment |

The DB table is the **runtime authority**. The config file is the **human-readable reference**. When adding custom stages (Section 4.4.1), both must be updated. In Period 2+ (Go CLI), the CLI can validate consistency between config and DB at startup.

**Lifecycle coordination:**

| Phase | Config Read | DB Write | JSONL Write |
|-------|------------|----------|-------------|
| PF1-INIT | Phase definitions, task templates | Session record (pending) | `phase_transition: PF1-INIT entered` |
| PF3-CLASSIFY | Pipeline for classified work type | Session activation (tracked), task registration | `phase_transition: PF3-CLASSIFY completed` |
| PF4-EXECUTE | Rework limits, stage timeout | Stage log entries per transition | `stage_transition: WS-DEV in_progress` |
| PF5-VERIFY | (none) | Verdict check across stage_log | `phase_transition: PF5-VERIFY completed` |
| PF7-END | (none) | Session status = 'ended' | `phase_transition: PF7-END completed` |

### 5.3. CLAUDE.md Changes

Two new sections support PathFlow orchestration:

**Section 4: PathFlow Session Management:**

- Session mode: tracked (full PathFlow lifecycle) vs untracked (lead answers directly)
- Phase progression: PF1-INIT through PF7-END, must complete sequentially
- Team lead role: "you delegate work to teammates, you do NOT write code directly"
- Team flexibility: predefined roster is optimized defaults, not constraints
- Session boundary: one PR per session (tracked sessions)
- Team persistence rules: NEVER call Teammate cleanup during active session; persistent teammates stay alive until PF7-END

**Section 5: Teammate Coordination:**

- Function teammates (3 persistent): cf-security (PF1-INIT), cf-knowledge-layer (PF2-CONTEXT), cf-gitops (PF3-CLASSIFY)
- Role teammates (5 on-demand): cf-developer, cf-planner, cf-documenter, cf-reviewer, cf-qa -- spawned per stage, shutdown after
- Ad-hoc teammates: spawn as needed for specialized work (general-purpose, one-offs)
- Communication patterns: direct peer messaging, WorkGraph through cf-knowledge-layer, git through cf-gitops, security queries through cf-security, escalations to lead

**Skills section rewrite:** Section referencing 11 skills replaced with reference to single retained skill (cf-working-protocol) and note that all other SOPs are embedded in agent definitions under `.claude/agents/`.

**Agent definitions:** 5-section format documented (Identity, Constraints, SOPs, Communication, Quality Checklist). 8 files total: 3 persistent (cf-security, cf-knowledge-layer, cf-gitops) + 5 on-demand (cf-developer, cf-planner, cf-documenter, cf-reviewer, cf-qa).

### 5.4. Hook Restructuring

The current hook set (28 hooks) is reduced to 23. No dual-mode branching exists in any hook. Skill sentinel hooks are removed entirely. Three new hooks are added for PathFlow enforcement: pathflow-init (SessionStart), pathflow-sentinel (PostToolUse), and pathflow-task-guard (PreToolUse). See [Appendix C](#appendix-c-complete-hook-disposition-table) for the complete disposition table.

#### Hooks to REMOVE (8 total)

| Hook | Type | Reason for Removal |
|------|------|--------------------|
| cf-pre-tool-use-bash-sentinel.sh | PreToolUse | Skill sentinel system eliminated |
| cf-pre-tool-use-file-sentinel.sh | PreToolUse | Skill sentinel system eliminated |
| cf-pre-tool-use-task-sentinel.sh | PreToolUse | Skill sentinel system eliminated |
| cf-pre-tool-use-grep-sentinel.sh | PreToolUse | Skill sentinel system eliminated |
| cf-pre-tool-use-claim-validation.sh | PreToolUse | Skill sentinel system eliminated |
| cf-post-tool-use-instructions.sh | PostToolUse | Skills archived; no instruction loading needed |
| cf-post-tool-use-memory-progress.sh | PostToolUse | Skills archived; progress tracking via PathFlow tasks |
| cf-post-tool-use-skill.sh | PostToolUse | Skill sentinel creation eliminated |

#### Hooks to KEEP (10 total, unchanged)

| Hook | Type | Purpose |
|------|------|---------|
| cf-pre-tool-use-edit-write.sh | PreToolUse | File scope checks |
| cf-pre-tool-use-gh-pr.sh | PreToolUse | PR format enforcement |
| cf-pre-tool-use-protected-resource.sh | PreToolUse | Protected file access control |
| cf-pre-tool-use-security.sh | PreToolUse | Command injection, path traversal |
| cf-pre-tool-use-webfetch.sh | PreToolUse | URL validation |
| cf-post-tool-use-logging.sh | PostToolUse | Tool use audit logging |
| cf-post-tool-use-settings-templates.sh | PostToolUse | Settings file management |
| cf-post-tool-use-tmp-workflow.sh | PostToolUse | Temp file workflow tracking |
| cf-stop-logging.sh | Stop | Stop-event audit logging (mid-session decision points, PCV status, git state) |
| session/prompt logging hooks | Session/Prompt | Session and prompt logging |

#### Hooks to MODIFY (8 total)

| Hook | Type | Change |
|------|------|--------|
| cf-pre-tool-use-pathflow-gate.sh | PreToolUse | Now the ONLY gate (no dual-mode fallback). Sentinel-first checking: file sentinels (PRIMARY) via `has_sentinel()`, JSONL phase lookup (SECONDARY fallback). |
| cf-pre-tool-use-team-guard.sh | PreToolUse | Block TeamDelete and Teammate(cleanup) during active PathFlow session. Matcher: `TeamDelete\|Teammate`. Only PF7-TSK-05 (remove pathflow-active flag) unlocks team dissolution. |
| cf-pre-tool-use-read-delegation.sh | PreToolUse | Simplified: no mode-detection branching |
| cf-stop-verify-work.sh | Stop | Renamed to `cf-stop-pathflow-gate.sh`. PCV enforcement removed. Logs PathFlow phase completion status for audit trail. |
| cf-session-start-init.sh | SessionStart | Consolidated from cleanup + pathflow-init. Creates directories, stale cleanup, PathFlow flag creation (v3.0.0) |
| cf-session-start-instructions.sh | SessionStart | Updated: agent-teams model, no skill loading |
| cf-session-end-cleanup.sh | SessionEnd | Simplified: clean PathFlow state only |
| cf-user-prompt-submit.sh | UserPromptSubmit | Simplified: no mode-detection branching |

#### Hooks to CREATE (3 new)

| Hook | Type | Matcher | Purpose |
|------|------|---------|---------|
| cf-pre-tool-use-pathflow-task-guard.sh | PreToolUse | TaskUpdate | Validates PathFlow task transitions (PF{N}-TSK-{NN} status changes) |
| cf-post-tool-use-pathflow-sentinel.sh | PostToolUse | TeamCreate\|Task\|SendMessage\|Bash | Automatic sentinel creation on phase/stage transition events (see Section 6.4) |
| ~~cf-session-start-pathflow-init.sh~~ | SessionStart | (all) | ~~Consolidated into cf-session-start-init.sh~~ |

**Note on PostToolUse matcher:** The `.*` regex does NOT match Agent Teams tools (TeamCreate, Task, SendMessage, TeamDelete). Explicit tool names are required. See `.codeflow/docs/research/agent-teams-hook-findings.md` for empirical validation.

#### Net result: 28 -> 23 hooks (-5)

### 5.5. Agent Definition Files

8 files in `.claude/agents/`, each following a 5-section format: Identity, Constraints, SOPs, Communication, Quality Checklist.

| File | Category | Embedded SOPs From | Purpose |
|------|----------|-------------------|---------|
| `cf-security.md` | Function/Persistent | cf-security-management | Security checks, sandbox, protected resource consultation |
| `cf-knowledge-layer.md` | Function/Persistent | cf-memory-management, cf-task-management, cf-db-operations | WorkGraph, memory, DB operations |
| `cf-gitops.md` | Function/Persistent | cf-git-workflow | All git operations following conventions |
| `cf-developer.md` | Role/On-demand | cf-script-standards (relevant portions) | Code implementation + unit tests + CICD |
| `cf-planner.md` | Role/On-demand | cf-documentation-standards (relevant portions) | Design, architecture, analysis |
| `cf-documenter.md` | Role/On-demand | cf-documentation-standards (relevant portions) | Documentation writing |
| `cf-reviewer.md` | Role/On-demand | Multi-mode review criteria | Independent work review |
| `cf-qa.md` | Role/On-demand | cf-testing-workflow (relevant portions) | Integration testing, or primary for TEST work type |

**5-section format (example: cf-reviewer):**

```markdown
# cf-reviewer Agent Definition

## Identity
Role: Independent code/design/doc reviewer
Stage: WS-REV

## Constraints
- You MUST NOT modify source code files
- You MUST NOT run git commit/push
- You MAY read any file, run tests (read-only), produce review findings

## SOPs
- CODE_REVIEW: code quality, security, tests present, conventions
- DESIGN_REVIEW: analysis completeness, feasibility, requirements coverage
- DOCUMENTATION_REVIEW: accuracy, structure, completeness, standards
- TEST_REVIEW: coverage, quality, edge cases, assertions

## Communication
- Report findings via SendMessage to team lead
- Verdict: APPROVED | CHANGES_REQUESTED

## Quality Checklist
- All findings documented with file:line references
- Verdict justified with specific observations
```

**Loaded at spawn time** via "Read your agent definition at `.claude/agents/cf-{role}.md`" in the spawn prompt.

### 5.6. Skills Archival

**Process:**

1. Move 10 skill directories from `.claude/skills/` to `.codeflow/docs/archived/skills/`
2. Extract SOPs from each skill and embed into corresponding agent definition
3. Update CLAUDE.md skills section to reference only cf-working-protocol
4. Remove skill sentinel hooks (see Section 5.4)
5. Remove skill sentinel library code from `cf-sentinel.sh`

**Retained skill: cf-working-protocol**

| Operation | Status | Notes |
|-----------|--------|-------|
| meta-awareness | Retained | Lead cognitive procedure |
| think-and-act | Retained | Lead cognitive procedure |
| decide | Retained | Lead cognitive procedure |
| respond-organized | Retained | Lead cognitive procedure |
| research-quality | Retained | Lead cognitive procedure |
| verify-work | **Removed** | Replaced by WS-REV stage |
| parallelize-work | **Removed** | Replaced by native TaskCreate/teams |

**What moves where:**

| Archived Skill | -> Agent Definition |
|---------------|---------------------|
| cf-git-workflow | -> cf-gitops.md |
| cf-memory-management | -> cf-knowledge-layer.md |
| cf-task-management | -> cf-knowledge-layer.md |
| cf-db-operations | -> cf-knowledge-layer.md |
| cf-security-management | -> cf-security.md |
| cf-script-standards | -> cf-developer.md (relevant portions) |
| cf-documentation-standards | -> cf-documenter.md, cf-planner.md (relevant portions) |
| cf-code-exploration | -> general patterns in relevant agents |
| cf-testing-workflow | -> cf-qa.md |

### 5.7. Settings.json Changes

**Matchers to ADD:** pathflow-gate (Edit|Write|Bash), team-guard (TeamDelete|Teammate), pathflow-task-guard (TaskUpdate)

**Matchers to REMOVE:** 5 skill sentinel PreToolUse matchers + 3 PostToolUse matchers (instructions, memory-progress, skill)

**Matchers to MODIFY:** Stop hook (rename), SessionStart/End (simplify)

---

## 6. Enforcement Model

### 6.1. 4-Layer Enforcement Chain

```text
Layer 1: CLAUDE.md + Agent Definitions (behavioral)
  |-- Team lead: full PathFlow awareness, spawns teammates
  |-- cf-developer: restricted to WS-DEV stage work
  |-- cf-reviewer: restricted to WS-REV stage, read-only + report
  |-- cf-qa: restricted to WS-QA stage, test execution only
  |-- cf-security: security consultation, protected resource checks
  |-- cf-knowledge-layer: WorkGraph, memory, DB operations only
  |-- cf-gitops: git operations following conventions only

Layer 2: PathFlow DB + Gate Hook (team-level hard enforcement)
  |-- pathflow-gate: checks session active, phase correct, no backwards progression
  |-- Blocks: Edit/Write/Bash before PF4-EXECUTE
  |-- Blocks: git commit before PF4-EXECUTE
  |-- Blocks: git push/PR before PF6-COMPLETE

Layer 3: Protection Hooks (always-on, team-wide)
  |-- cf-pre-tool-use-security.sh (command injection, path traversal)
  |-- cf-pre-tool-use-protected-resource.sh (protected files)
  |-- cf-pre-tool-use-gh-pr.sh (PR format enforcement)

Layer 4: Stop Hook (session-level)
  |-- Validates PathFlow phase completion (logging-only, was PCV, now phase-gate)
```

**Layer interaction:** Layers are defense-in-depth. Layer 1 (behavioral) catches most violations through instructions. Layer 2 (PathFlow gate) catches violations that instructions missed. Layer 3 (protection) catches security and resource violations regardless of PathFlow state. Layer 4 (stop) logs session completion status for audit.

### 6.2. PathFlow-Gate Hook

The pathflow-gate hook is the single enforcement point for PathFlow phase progression. It replaces the 5 skill sentinel hooks with one hook that uses file-based sentinels as the PRIMARY check and JSONL events as a SECONDARY fallback.

**Logic (sentinel-first):**

```text
pathflow-gate hook fires on Edit|Write|Bash:
  1. Tool is Edit/Write/Bash?  No -> exit 0 (allow)
  2. Bash but not git commit/push/PR?  -> exit 0 (allow)
  3. is_pathflow_active()?  No -> exit 0 (not in PathFlow, allow all)
  4. Source cf-pathflow-state.sh library
  5. Gate check (sentinel-first):
     - Edit/Write or git commit:
         has_sentinel("pf-3")?  Yes -> ALLOW.  No -> BLOCK (exit 2)
         "PathFlow gate: Edit/Write requires PF3-CLASSIFY (branch creation)"
     - git push/PR (gh pr create):
         has_sentinel("ws-rev")?  Yes -> ALLOW.  No -> BLOCK (exit 2)
         "PathFlow gate: PR requires WS-REV completion"
  6. Fallback: existing JSONL phase lookup (secondary, for cf-knowledge-layer maturity)
```

**Block message format (stderr, exit 2):**

```text
BLOCKED: PathFlow gate - prerequisite not met
Reason: Edit/Write requires PF3-CLASSIFY (branch creation). No pathflow-pf-3 sentinel.
Complete earlier phases before this operation.
Current sentinels: pathflow-pf-1, pathflow-pf-2
```

**Primary read path:** File sentinel check (`[[ -f sentinel_file ]]`). O(1), no parsing, no dependencies.

**Secondary read path:** JSONL tail + jq parse. Preserved as fallback for when cf-knowledge-layer writes phase_transition events.

**Final read path (Period 2+):** SQLite indexed query. O(1) lookup.

### 6.3. Graceful Degradation

```text
Full enforcement (nominal):
  Instructions + PathFlow Tasks + Hooks all active
         |
         | (JSONL write fails or pathflow-gate errors)
         v
Partial enforcement:
  Instructions + PathFlow Tasks active
  pathflow-gate returns ALLOW on error
         |
         | (hooks fail entirely)
         v
Advisory only:
  Instructions + PathFlow Tasks active
  No hook enforcement, lead orchestration provides ordering
```

**Philosophy:** A session should never be BLOCKED by an enforcement system failure. No standalone fallback exists -- if PathFlow enforcement fails, behavioral enforcement (Layer 1) continues.

### 6.4. PathFlow Enforcement Mechanism

The full session lifecycle from session start to session end, showing how hooks create state and enforce progression. The diagram shows **both tracks** side by side: file sentinels (created automatically by PostToolUse hooks) and JSONL events (created by cf-knowledge-layer calling pathflow scripts).

```text
SESSION START
│
├─ SessionStart hooks fire:
│  ├─ cf-session-start-init.sh (v3.0.0): Creates directories + stale cleanup + PathFlow flag
│  │  ├─ .state/session/{SID}/
│  │  ├─ .state/sentinels/pathflow/{SID}/
│  │  └─ .state/session/{SID}/is-pathflow-active (JSON metadata)
│        └─ is_pathflow_active() now returns TRUE
│
├─ Lead calls TeamCreate
│  └─ PostToolUse (tool_name=TeamCreate)
│     ├─ FILE: Sentinel hook creates pathflow-pf-1
│     └─ JSONL: (none yet — cf-knowledge-layer not spawned)
│
├─ PF1-INIT: Lead spawns cf-security
│  └─ PostToolUse (tool_name=Task)
│     └─ name != "cf-knowledge-layer" → no sentinel
│
├─ PF2-CONTEXT: Lead spawns cf-knowledge-layer
│  └─ PostToolUse (tool_name=Task)
│     ├─ FILE: Sentinel hook creates pathflow-pf-2
│     └─ JSONL: cf-knowledge-layer calls cf-pathflow-session-register.sh
│        └─ Writes session_metadata event (tracking_level=pending)
│
├─ GATE TEST: Edit/Write attempted (BEFORE PF3)
│  └─ PreToolUse pathflow-gate
│     ├─ is_pathflow_active()? TRUE ✓
│     ├─ has_sentinel("pf-3")? FALSE ✗
│     └─ exit 2 → BLOCKED ✓
│
├─ PF3-CLASSIFY: cf-git-operations runs git checkout -b
│  └─ PostToolUse (tool_name=Bash, command matches git checkout -b)
│     ├─ FILE: Sentinel hook creates pathflow-pf-3
│     └─ JSONL: cf-knowledge-layer calls:
│        ├─ cf-pathflow-phase-transition.sh (PF3-CLASSIFY, completed)
│        ├─ cf-pathflow-session-metadata.sh (work_type, area_type)
│        ├─ cf-pathflow-session-metadata.sh (branch)
│        └─ cf-pathflow-session-metadata.sh (tracking_level=tracked)
│
├─ GATE TEST: Edit/Write now allowed
│  └─ PreToolUse pathflow-gate
│     ├─ has_sentinel("pf-3")? TRUE ✓
│     └─ exit 0 → ALLOWED ✓
│
├─ PF4-EXECUTE: Work pipeline
│  ├─ WS-DEV: cf-development works
│  │  └─ SendMessage("STAGE-COMPLETE: WS-DEV")
│  │     ├─ FILE: Sentinel hook creates pathflow-ws-dev
│  │     └─ JSONL: cf-knowledge-layer calls cf-pathflow-stage-transition.sh
│  ├─ WS-REV: cf-review works
│  │  └─ SendMessage("STAGE-COMPLETE: WS-REV")
│  │     ├─ FILE: Sentinel hook creates pathflow-ws-rev
│  │     └─ JSONL: cf-knowledge-layer calls cf-pathflow-stage-transition.sh
│  └─ WS-QA: cf-quality-assurance works
│     └─ SendMessage("STAGE-COMPLETE: WS-QA")
│        ├─ FILE: Sentinel hook creates pathflow-ws-qa
│        └─ JSONL: cf-knowledge-layer calls cf-pathflow-stage-transition.sh
│
├─ PR GATE: gh pr create attempted
│  └─ PreToolUse pathflow-gate
│     ├─ has_sentinel("ws-rev")? TRUE ✓
│     └─ exit 0 → ALLOWED ✓
│
├─ PF6-COMPLETE: cf-git-operations creates PR
│  └─ PostToolUse (tool_name=Bash, command matches gh pr create)
│     ├─ FILE: Sentinel hook creates pathflow-pf-6
│     └─ JSONL: cf-knowledge-layer calls cf-pathflow-phase-transition.sh
│
├─ TEAM-GUARD: TeamDelete attempted during active session
│  └─ PreToolUse team-guard fires
│     ├─ is_pathflow_active()? → TRUE
│     └─ exit 2 → BLOCKED (team dissolution prevented)
│
├─ PF7-END: Shutdown + cleanup
│  ├─ Lead shuts down all teammates (SendMessage type=shutdown_request)
│  ├─ Lead removes pathflow-active flag → team-guard unlocked
│  ├─ Lead calls TeamDelete → allowed (flag removed)
│  └─ SessionEnd hooks fire:
│     ├─ cf-session-end-cleanup.sh:
│     │  ├─ Removes .state/sentinels/pathflow/{SID}/ (all sentinels)
│     │  └─ Removes .state/session/{SID}/ (flag + state)
│     └─ is_pathflow_active() now returns FALSE
│
SESSION END (clean state)
```

**Key design properties:**

| Property | Mechanism |
|----------|-----------|
| Flag creation is automatic | SessionStart hook, every session |
| Sentinel creation is automatic | PostToolUse hook, on phase/stage transition events |
| Enforcement is file-based | `[[ -f file ]]` checks, no parsing, no external dependencies |
| State is session-scoped | All sentinels in `.state/sentinels/pathflow/{SID}/`, cleaned at session end |
| Idempotent | `touch` on existing sentinel is a no-op |
| Graceful degradation | If sentinel creation fails, instructions + task graph still provide ordering |

### 6.5. STAGE-COMPLETE Protocol

On-demand role teammates signal work stage completion by including `STAGE-COMPLETE: WS-{STAGE}` in their final SendMessage to the team lead. This triggers automatic sentinel creation via the PostToolUse sentinel hook.

**Message format:**

```text
STAGE-COMPLETE: WS-{STAGE}
```

Where `{STAGE}` is one of: `DEV`, `REV`, `QA`, `TEST`, `PLAN`, `DOCS`.

**Protocol participants:**

| Teammate | Message on Completion |
|----------|----------------------|
| cf-development | `"STAGE-COMPLETE: WS-DEV"` in final message to lead |
| cf-review | `"STAGE-COMPLETE: WS-REV"` in final message to lead |
| cf-quality-assurance | `"STAGE-COMPLETE: WS-QA"` or `"STAGE-COMPLETE: WS-TEST"` |
| cf-documentation | `"STAGE-COMPLETE: WS-DOCS"` in final message to lead |
| cf-planning | `"STAGE-COMPLETE: WS-PLAN"` in final message to lead |

**Detection by sentinel hook:** When the PostToolUse hook fires for `tool_name=SendMessage`, it parses `tool_input.content` for the `STAGE-COMPLETE: WS-` pattern and creates the corresponding sentinel file (e.g., `pathflow-ws-dev`, `pathflow-ws-rev`).

**How teammate tool calls are detected:** Teammates share the same session as the lead. PostToolUse hooks fire for ALL tool calls in the session, including teammate calls. When cf-development calls `SendMessage(content="STAGE-COMPLETE: WS-DEV")`, the sentinel hook fires and creates the sentinel.

### 6.6. File Sentinels vs JSONL Sentinels

File-based sentinels are the PRIMARY enforcement mechanism. JSONL events are preserved as a SECONDARY fallback. This is a deliberate design decision.

| Criterion | File Sentinels (PRIMARY) | JSONL Events (SECONDARY) |
|-----------|-------------------------|--------------------------|
| Creation | `touch file` (atomic, always works) | `jq -nc ... >> file` (needs jq, correct JSON) |
| Check | `[[ -f file ]]` (fast, no parsing) | `tail + jq select` (slow, fragile) |
| Dependencies | None | jq, correct JSON, session filtering |
| Writer | PostToolUse hook (automatic, reliable) | cf-knowledge-layer teammate (requires teammate to be alive, follow instructions) |
| Cleanup | `rm -rf dir/` (session-end already does this) | Events persist in ledger (audit trail) |

**Why file sentinels as primary:**

1. **Zero dependencies.** File existence checks require no external tools (no jq, no JSON parsing, no session ID filtering).
2. **Automatic creation.** PostToolUse hooks fire reliably for all tool calls. No dependency on teammate instructions being followed.
3. **Fast.** `[[ -f file ]]` is O(1) with no I/O beyond stat. JSONL parsing is O(n) even when bounded.
4. **Existing infrastructure.** Session-start already creates the sentinel directory. Session-end already removes it. No new cleanup logic needed.

**Why JSONL is preserved:**

1. **Audit trail.** JSONL events provide a durable, append-only record of phase transitions that survives session end.
2. **cf-knowledge-layer maturity.** When cf-knowledge-layer starts writing `phase_transition` events reliably, the JSONL-based check in pathflow-gate becomes a secondary verification mechanism.
3. **Future SQLite migration.** Period 2+ replaces JSONL reads with SQLite indexed queries. The event schema is preserved for this transition.

**Coexistence:** Both mechanisms work independently. File sentinels provide fast, reliable enforcement. JSONL events provide audit and future migration path. Neither depends on the other.

**Conflict resolution:** If file sentinel and JSONL event disagree (e.g., sentinel exists but JSONL event missing, or vice versa), file sentinel is authoritative for enforcement decisions. The pathflow-gate hook checks ONLY file sentinels via `has_sentinel()`. JSONL events are checked only as a commented-out fallback in the gate hook (preserved for when cf-knowledge-layer matures). A missing JSONL event does NOT block work; a missing file sentinel DOES.

### 6.7. JSONL Event Flow: Script Responsibilities

This section maps WHO calls WHICH script, WHEN, and what JSONL event type is produced. All JSONL scripts live in `.codeflow/scripts/pathflow/` and write to `pathflow-events.jsonl` via the `ledger.sh` library.

| Phase | Event | Script Called | Caller | JSONL Event Type |
|-------|-------|--------------|--------|------------------|
| PF1-INIT | Session registration | `cf-pathflow-session-register.sh` | cf-knowledge-layer | `session_metadata` (tracking_level=pending) |
| PF1-INIT | Phase entered | `cf-pathflow-phase-transition.sh` | cf-knowledge-layer | `phase_transition` (PF1-INIT, entered) |
| PF1-INIT | Phase completed | `cf-pathflow-phase-transition.sh` | cf-knowledge-layer | `phase_transition` (PF1-INIT, completed) |
| PF2-CONTEXT | Phase transitions | `cf-pathflow-phase-transition.sh` | cf-knowledge-layer | `phase_transition` |
| PF3-CLASSIFY | Work type set | `cf-pathflow-session-metadata.sh` | cf-knowledge-layer | `session_metadata` (work_type, area_type) |
| PF3-CLASSIFY | Branch set | `cf-pathflow-session-metadata.sh` | cf-knowledge-layer | `session_metadata` (branch) |
| PF3-CLASSIFY | Tracking activated | `cf-pathflow-session-metadata.sh` | cf-knowledge-layer | `session_metadata` (tracking_level=tracked) |
| PF4-EXECUTE | Stage transitions | `cf-pathflow-stage-transition.sh` | cf-knowledge-layer | `stage_transition` |
| PF4-EXECUTE | Task updates | `cf-pathflow-task-update.sh` | cf-knowledge-layer | `pathflow_task_update` |
| PF5-PF7 | Phase transitions | `cf-pathflow-phase-transition.sh` | cf-knowledge-layer | `phase_transition` |

**Key principle:** In Phase 4 implementation, file sentinels are PRIMARY and SUFFICIENT for enforcement. JSONL writing by cf-knowledge-layer provides an audit trail and future SQLite rebuild capability. If cf-knowledge-layer fails to write JSONL, enforcement is NOT degraded -- file sentinels still work. The JSONL scripts are called by cf-knowledge-layer via `Bash` tool calls; the scripts source `ledger.sh` for append operations with flock-based parallel safety.

### 6.8. Dual-Track State: File Sentinels + JSONL Events

File sentinels and JSONL events represent the same logical transitions through two independent mechanisms with different timing, writers, and purposes.

**Timing relationship:**

| Aspect | File Sentinels | JSONL Events |
|--------|---------------|--------------|
| **Created by** | PostToolUse hook (automatic, zero teammate involvement) | cf-knowledge-layer calling pathflow scripts (teammate-driven) |
| **Trigger** | Tool call completion (TeamCreate, Task, Bash, SendMessage) | cf-knowledge-layer processing teammate messages or lead instructions |
| **Timing** | Immediate (fires on every matching tool call) | Delayed (depends on cf-knowledge-layer receiving and processing the event) |
| **Reliability** | High (hooks fire reliably, `touch` always succeeds) | Medium (depends on teammate being alive, following instructions, jq available) |
| **Persistence** | Session-scoped (cleaned up at session end) | Permanent (append-only ledger survives session end) |
| **Purpose** | Enforcement (pathflow-gate checks these) | Audit trail + future SQLite rebuild |

**Both happen for the same logical event but through different mechanisms:**

```text
cf-development sends "STAGE-COMPLETE: WS-DEV"
    │
    ├─ PostToolUse hook fires (automatic, immediate)
    │  └─ Sentinel hook: touch .state/sentinels/pathflow/{SID}/pathflow-ws-dev
    │     └─ Enforcement: has_sentinel("ws-dev") → TRUE
    │
    └─ Lead/cf-knowledge-layer processes the message (teammate-driven, delayed)
       └─ Calls: cf-pathflow-stage-transition.sh -s {SID} -g WS-DEV -t complete
          └─ Audit: {"type":"stage_transition","stage":"WS-DEV","status":"complete"}
```

**Authoritative resolution:**

- File sentinel is authoritative for **enforcement** (PRIMARY). The pathflow-gate hook checks ONLY file sentinels.
- JSONL is authoritative for **audit/rebuild** (audit trail). JSONL events provide the durable record for post-session analysis and future SQLite migration.
- If they disagree: file sentinel wins for enforcement decisions. A missing JSONL event never blocks work.

---

## 7. End-to-End Flow Chains

### 7.1. FEAT Flow: Full Pipeline

```text
User: "Implement login form validation"

PF1-INIT: Session Start
  |-- Register session-{ulid} with tracking_level='pending'
  |-- Spawn cf-security
  |-- PF1-TSK-01..03 -> complete
  |-- JSONL: phase_transition PF1-INIT completed

PF2-CONTEXT: Context Awareness
  |-- Spawn cf-knowledge-layer, query active work -> none
  |-- TRACKED mode decision
  |-- PF2-TSK-01..03 -> complete
  |-- JSONL: phase_transition PF2-CONTEXT completed

PF3-CLASSIFY: Work Classification
  |-- Classify: FEAT/FRT, register FRT-TSK-FEAT-AUTH-001
  |-- Query pipelines.FEAT: [WS-DEV, WS-REV, WS-QA]
  |-- Spawn cf-gitops, create branch feat/frt-auth-validation
  |-- PF3-TSK-01..04 -> complete
  |-- JSONL: phase_transition PF3-CLASSIFY completed
  |   -> pathflow-gate now allows Edit/Write/Bash

PF4-EXECUTE: Work Execution

  [WS-DEV] Development Stage
  |-- JSONL: stage_transition WS-DEV in_progress iteration=1
  |-- Spawn cf-developer -> implement + unit tests -> pass
  |-- cf-developer -> cf-gitops: commit, -> cf-knowledge-layer: stage complete
  |-- Shutdown cf-developer
  |-- JSONL: stage_transition WS-DEV complete

  [WS-REV] Review Stage
  |-- JSONL: stage_transition WS-REV in_progress iteration=1
  |-- Spawn cf-reviewer (CODE_REVIEW) -> APPROVED
  |-- Shutdown cf-reviewer
  |-- JSONL: stage_transition WS-REV complete verdict=pass

  [WS-QA] Quality Assurance Stage
  |-- JSONL: stage_transition WS-QA in_progress iteration=1
  |-- Spawn cf-qa -> integration tests pass, acceptance criteria met
  |-- cf-qa -> cf-gitops: commit, -> cf-knowledge-layer: qa complete
  |-- Shutdown cf-qa
  |-- JSONL: stage_transition WS-QA complete verdict=pass

PF5-VERIFY -> PF6-COMPLETE -> PF7-END
  |-- Lead verifies all stages complete
  |-- cf-gitops: create PR, push
  |-- Shutdown all persistent teammates, TeamDelete
```

### 7.2. PLAN Flow: Design Architecture

```text
User: "Design the authentication system architecture"

PF1-INIT -> PF2-CONTEXT -> PF3-CLASSIFY
  |-- work_type=PLAN, area=PLN
  |-- Task: PLN-TSK-PLAN-GENL-001
  |-- Pipeline: [WS-PLAN, WS-REV]
  |-- Branch: plan/pln-auth-architecture

PF4-EXECUTE
  [WS-PLAN] -> cf-planner: ADR + epic + tasks
  [WS-REV] -> cf-reviewer (DESIGN_REVIEW): APPROVED

PF5-PF7: verify, PR, shutdown
  |-- Deliverables: ADR, FRT-EPC-FEAT-AUTH-001 available for FEAT work
```

### 7.3. Rework Loop Flow

```text
[WS-DEV] iteration 1 -> complete
[WS-REV] iteration 1 -> CHANGES_REQUESTED (missing input sanitization)
[WS-DEV] iteration 2 (rework) -> addresses findings -> complete
[WS-REV] iteration 2 -> APPROVED -> proceed to WS-QA

Rework limit (max_rework_iterations=3):
  If exceeded -> BLOCKED "rework limit exceeded" -> escalate to lead -> PF7-END
```

### 7.4. Untracked Session Flow

```text
User: "What's the difference between ULID and UUID?"

PF1-INIT: Session Start
  |-- Register session-{ulid} with tracking_level='pending'
  |-- Spawn cf-security (always, even for potentially untracked sessions)

PF2-CONTEXT: Context Awareness
  |-- Spawn cf-knowledge-layer, evaluate work complexity
  |-- Decision: UNTRACKED (simple question, no work to register)
  |-- Update session: tracking_level='untracked'
  |-- Lead answers directly, no role teammates spawned
  |-- Skip PF3-PF6

PF7-END -> minimal cleanup, session logged as untracked
```

**Key point:** Untracked is not a different "mode." The lead decides the work does not warrant a full team. Same system, lighter execution path.

---

## 8. V4 Specification Impact

### 8.1. Spec Sections Requiring Updates

| V4 Section | Change Type | Details |
|------------|-------------|---------|
| 04-knowledge-layer/01-work-graph.md | **Add** | PLN area, PLAN work type, work_type_stages table, PathFlow tables |
| 05-claude-components/agents/ | **Rewrite** | 8 agents, 5-section format, cf-security added, cf-ops removed |
| 05-claude-components/skills/ | **Archive** | Keep cf-working-protocol only |
| 05-claude-components/hooks/ | **Rewrite** | 28 -> 21 hooks disposition |
| 06-flows/work-lifecycle.md | **Replace** | WS-WORK -> WS-PLAN/WS-DOCS/WS-TEST, 10-pipeline table |
| 06-flows/session-lifecycle.md | **Replace** | PF-1..PF-7 -> PF1-INIT..PF7-END, remove standalone mode |
| 07-pathflow/ | **Add** | pathflow-config.json, 3-period DB strategy, PF{N}-TSK-{NN} |
| 10-implementation/ | **Update** | Sub-phases 4a-4g, skills archival |

### 8.2. Spec Sections NOT Changed

01-overview, 02-foundation, 03-cli, 04-knowledge-layer (other), 08-coordination, 09-autorun, 11-reference.

---

## 9. Implementation Roadmap

### 9.1. Phase 4 Sub-Phases

| Sub-phase | Description | Dependencies | Deliverables |
|-----------|-------------|-------------|--------------|
| **4a** | Archive 9 skills | None | `.codeflow/docs/archived/skills/` |
| **4b** | Simplify cf-working-protocol | 4a | Remove verify-work, parallelize-work |
| **4c** | Remove 5 sentinel hooks | 4a | Delete bash/file/task/grep/claim sentinel hooks |
| **4d** | Remove 3 PostToolUse hooks | 4a | Delete instructions/memory-progress/skill hooks |
| **4e** | Create 8 agent definitions | 4a, 4b | `.claude/agents/cf-*.md` (5-section format) |
| **4f** | Modify pathflow-gate for JSONL | 4c | JSONL-backed enforcement |
| **4g** | Simplify stop hook | 4b, 4f | Phase-gate logging only |

**Scope note:** Sub-phases 4a-4g are implementation ordering steps WITHIN the existing V4 Phase 4 ("Agents", Weeks 8-9). They do not create new top-level phases or alter the 1-9 phase numbering. The V4 specification already plans for agent definitions, teammate formats, and CLAUDE.md updates within Phase 4's scope. This proposal provides the specific decomposition and dependency ordering for executing that work.

```text
4a --+-- 4b --+-- 4e
     |        +-- 4g
     +-- 4c ---- 4f ---- 4g
     +-- 4d
```

### 9.2. Three-Period DB/JSONL Strategy

**Period 1 (Interim):** Shell -> JSONL only. Hooks read JSONL (tail + jq). ~100 lines/session.

**Period 2 (Final):** Go CLI -> SQLite first, JSONL second. SQLite = primary read. JSONL = rebuild authority.

**Period 3 (Transition):** Go CLI takes over writes. Shell stops. Hooks switch to SQLite.

### 9.3. Backward Compatibility

- No standalone mode to preserve -- agent-teams is the only mode
- Existing epics/tasks unaffected. Existing work types retain behavior.
- `'work'` stage code migrated to 'plan'/'docs'/'test'. NULL defaults on new columns.
- All new tables additive. Archived skills readable at `.codeflow/docs/archived/skills/`.

---

## Appendix A: Cross-Reference to PathFlow V3 Research

| Document | Relevance |
|----------|-----------|
| 01-agent-teams-and-pathflow.md | V3 limitations, Agent Teams capabilities, validated behaviors |
| 02-system-overview.md | Three-mechanism model, progressive orchestration |
| 03-teammate-model.md | Function vs role, persistence criteria, spawn timing |
| 08-enforcement-model.md | PathFlow sentinels, enforcement layers, graceful degradation |
| 12-changes-and-claude-components.md | Change inventory, CLAUDE.md, hooks, agents, schema |

## Appendix B: Terminology

| Term | Definition |
|------|-----------|
| **PathFlow** | 7-phase session lifecycle framework (PF1-INIT through PF7-END) |
| **PathFlow Task** | Session-scoped checklist item (PF{N}-TSK-{NN}), ephemeral, in `pathflow_tasks` |
| **Project Task** | Persistent work item ({AREA}-TSK-{TYPE}-{DOMAIN}-{NNN}), in `tasks` |
| **Work Stage** | Execution phase within PF4-EXECUTE (WS-DEV/PLAN/DOCS/TEST/REV/QA) |
| **Pipeline** | Ordered stage sequence per work type (in pathflow-config.json) |
| **Function Teammate** | Persistent: cf-security, cf-knowledge-layer, cf-gitops |
| **Role Teammate** | On-demand: cf-developer, cf-planner, cf-documenter, cf-reviewer, cf-qa |
| **PathFlow Sentinel** | Session-scoped, no-TTL file marker (PRIMARY) checked by pathflow-gate via `has_sentinel()`. JSONL events serve as SECONDARY fallback. Created automatically by PostToolUse hook on phase/stage transitions. |
| **Tracked Session** | Work registered in WorkGraph, full PF1-PF7 lifecycle |
| **Untracked Session** | Lead answers directly, skips PF3-PF6 |
| **Agent-Teams Only** | Single mode. No standalone/dual-mode fallback. |
| **Period 1/2/3** | DB/JSONL strategy: JSONL-only -> Go CLI dual-write -> SQLite primary |

## Appendix C: Complete Hook Disposition Table

### PreToolUse Hooks

| # | Hook | V3 Status | Disposition | Matcher | Notes |
|---|------|-----------|-------------|---------|-------|
| 1 | cf-pre-tool-use-security.sh | Active | **KEEP** | Bash | Command injection, path traversal |
| 2 | cf-pre-tool-use-protected-resource.sh | Active | **KEEP** | Edit\|Write\|Read\|Bash | Protected file access control |
| 3 | cf-pre-tool-use-edit-write.sh | Active | **KEEP** | Edit\|Write | File scope checks |
| 4 | cf-pre-tool-use-gh-pr.sh | Active | **KEEP** | Bash | PR format enforcement |
| 5 | cf-pre-tool-use-webfetch.sh | Active | **KEEP** | WebFetch | URL validation |
| 6 | cf-pre-tool-use-pathflow-gate.sh | New | **MODIFY** | Edit\|Write\|Bash | PathFlow phase enforcement. **PRIMARY:** file sentinel check (`has_sentinel()`). **SECONDARY:** JSONL/SQLite fallback. |
| 7 | cf-pre-tool-use-team-guard.sh | New | **MODIFY** | TeamDelete\|Teammate | Block team dissolution during active PathFlow session |
| 8 | cf-pre-tool-use-read-delegation.sh | Active | **MODIFY** | Read | Simplified, no mode branching |
| 9 | cf-pre-tool-use-bash-sentinel.sh | Active | **REMOVE** | Bash | Skill sentinels eliminated |
| 10 | cf-pre-tool-use-file-sentinel.sh | Active | **REMOVE** | Edit\|Write | Skill sentinels eliminated |
| 11 | cf-pre-tool-use-task-sentinel.sh | Active | **REMOVE** | TaskUpdate | Skill sentinels eliminated |
| 12 | cf-pre-tool-use-grep-sentinel.sh | Active | **REMOVE** | Grep | Skill sentinels eliminated |
| 13 | cf-pre-tool-use-claim-validation.sh | Active | **REMOVE** | TaskUpdate | Skill sentinels eliminated |

### PostToolUse Hooks

| # | Hook | V3 Status | Disposition | Matcher | Notes |
|---|------|-----------|-------------|---------|-------|
| 14 | cf-post-tool-use-logging.sh | Active | **KEEP** | * | Tool use audit logging |
| 15 | cf-post-tool-use-settings-templates.sh | Active | **KEEP** | Write | Settings file management |
| 16 | cf-post-tool-use-tmp-workflow.sh | Active | **KEEP** | Bash | Temp file workflow tracking |
| 17 | cf-post-tool-use-instructions.sh | Active | **REMOVE** | Skill | Skills archived |
| 18 | cf-post-tool-use-memory-progress.sh | Active | **REMOVE** | TaskUpdate | PathFlow tasks replace |
| 19 | cf-post-tool-use-skill.sh | Active | **REMOVE** | Skill | Sentinel creation eliminated |

### Stop Hooks

| # | Hook | V3 Status | Disposition | Notes |
|---|------|-----------|-------------|-------|
| 20 | cf-stop-verify-work.sh → `cf-stop-pathflow-gate.sh` | Active | **MODIFY** | PCV enforcement removed. Logs PathFlow phase completion status for audit trail. |
| 21 | cf-stop-logging.sh | Active | **KEEP** | Stop-event audit logging (mid-session decision points, PCV status, git state) |

### Session/Prompt Hooks

| # | Hook | V3 Status | Disposition | Notes |
|---|------|-----------|-------------|-------|
| 22 | cf-session-start-init.sh | Active | **MODIFY** | Consolidated from cleanup + pathflow-init (v3.0.0) |
| 23 | cf-session-start-instructions.sh | Active | **MODIFY** | Agent-teams model, no skill loading |
| 24 | cf-session-end-cleanup.sh | Active | **MODIFY** | Clean PathFlow state only |
| 25 | cf-user-prompt-submit.sh | Active | **MODIFY** | No mode-detection branching |

### New Hooks

| # | Hook | Disposition | Type | Matcher | Notes |
|---|------|-------------|------|---------|-------|
| 26 | cf-pre-tool-use-pathflow-task-guard.sh | **CREATE** | PreToolUse | TaskUpdate | PathFlow task transition validation |
| 27 | cf-post-tool-use-pathflow-sentinel.sh | **CREATE** | PostToolUse | TeamCreate\|Task\|SendMessage\|Bash | Automatic file sentinel creation on phase/stage transitions. `.*` does NOT match team tools — explicit names required (empirically validated). |
| ~~28~~ | ~~cf-session-start-pathflow-init.sh~~ | ~~CREATE~~ | ~~SessionStart~~ | | ~~Consolidated into cf-session-start-init.sh (row 22)~~ |

### Summary

| Action | Count |
|--------|-------|
| KEEP (unchanged) | 10 |
| MODIFY | 8 |
| REMOVE | 8 |
| CREATE | 3 |
| **Net total** | **23** (down from 28, net -5) |

---

*End of proposal v2. All 9 errors from v1 corrected. Updated 2026-02-15 with enforcement hardening: file sentinels as PRIMARY enforcement, STAGE-COMPLETE protocol, PathFlow enforcement mechanism lifecycle, SessionStart pathflow-init hook, PostToolUse sentinel hook, pathflow-active flag documentation, JSONL event flow script mapping (Section 6.7), dual-track state documentation (Section 6.8), teammate hook firing clarification (Section 3.5), and conflict resolution rule (Section 6.6).*

## Appendix D: PF7-END Cleanup Flow

Added: 2026-02-17. Resolves the chicken-and-egg problem where team-guard blocks TeamDelete while pathflow-active flag exists, but SessionEnd (which removes the flag) only fires after the session ends.

### Problem

1. **team-guard hook** blocks TeamDelete when pathflow-active flag exists (mid-session protection)
2. **SessionEnd hook** removes the flag — but only fires after the session ends
3. TeamDelete must run DURING PF7-END (before session end)
4. Result: TeamDelete is always blocked, requiring manual flag removal

### Solution

Two-part fix using the pathflow-active flag as single source of truth:

1. **team-guard.sh**: When TeamDelete requested + pf-6 sentinel exists → remove flag → allow TeamDelete
2. **session-end-cleanup.sh**: Replace team-config checking with simple pathflow-active flag check

### Flow

```text
PF7-END: Session Shutdown
══════════════════════════════════════════════════════════════════

PF7-TSK-01: Shutdown on-demand teammates
─────────────────────────────────────────
  Lead: SendMessage(type="shutdown_request", recipient="cf-development")
  Lead: SendMessage(type="shutdown_request", recipient="cf-review")
    │
    ▼
  Each teammate approves → Claude Code ends their session
    │
    ▼
  SessionEnd hook fires (for EACH teammate)
    │
    ├─ source .state/runtime/codeflow-env.sh → get CODEFLOW_SESSION_ID
    ├─ is_pathflow_active() → checks .state/session/{SID}/is-pathflow-active
    ├─ Flag EXISTS → _PATHFLOW_ACTIVE="true"
    ├─ Guard: if _PATHFLOW_ACTIVE == true → exit 0 (skip cleanup)
    │
    └─ ✅ Shared state preserved (sentinels, flag, env file intact)


PF7-TSK-02: Shutdown persistent teammates
──────────────────────────────────────────
  Lead: SendMessage(type="shutdown_request", recipient="cf-security")
  Lead: SendMessage(type="shutdown_request", recipient="cf-knowledge-layer")
  Lead: SendMessage(type="shutdown_request", recipient="cf-git-operations")
    │
    ▼
  Same as above → flag EXISTS → SessionEnd skips cleanup
    │
    └─ ✅ All teammates gone, shared state still intact


PF7-TSK-03: TeamDelete
───────────────────────
  Lead: TeamDelete()
    │
    ▼
  PreToolUse: team-guard hook fires
    │
    ├─ Tool is TeamDelete? → YES
    ├─ is_pathflow_active()? → YES (flag still exists)
    │
    ├─ PF7-END GATE: Does pathflow-pf-6 sentinel exist?
    │   File: .state/sentinels/pathflow/{SID}/pathflow-pf-6
    │   │
    │   ├─ NO  → exit 2 (BLOCK — not at PF7, mid-session protection)
    │   │
    │   └─ YES → PF7-END is legitimate
    │       │
    │       ├─ ACTION: rm -f .state/session/{SID}/is-pathflow-active
    │       └─ exit 0 (ALLOW TeamDelete)
    │
    ▼
  TeamDelete executes
    │
    ├─ Removes ~/.claude/teams/{team-name}/config.json
    └─ Removes ~/.claude/tasks/{team-name}/
    │
    └─ ✅ Team dissolved, flag already removed


Lead's session ends (user closes CLI, /exit, etc.)
───────────────────────────────────────────────────
  SessionEnd hook fires (for the LEAD)
    │
    ├─ source .state/runtime/codeflow-env.sh → get CODEFLOW_SESSION_ID
    ├─ is_pathflow_active() → checks .state/session/{SID}/is-pathflow-active
    ├─ Flag DOES NOT EXIST (removed by team-guard in TSK-03)
    ├─ _PATHFLOW_ACTIVE="false"
    │
    ├─ Guard: if _PATHFLOW_ACTIVE == true → skip
    │         else → PROCEED WITH CLEANUP
    │
    ▼
  Cleanup runs:
    ├─ 1. rm -rf .state/sentinels/pathflow/{SID}/     (all sentinels)
    ├─ 2. Clean expired skill sentinels
    ├─ 3. Flag already gone (idempotent rm -f, no-op)
    ├─ 4. Preserve active-task.json if in_progress
    ├─ 5. rm -rf .state/session/{SID}/                (session dir)
    ├─ 6. rm -f .state/runtime/codeflow-env.sh        (env file)
    └─ 7. rm -rf /tmp/claude/sessions/{SID}/          (temp files)
    │
    └─ ✅ Full cleanup complete
```

### Edge Cases

| Scenario | Outcome |
|----------|---------|
| Teammate shutdown mid-session | Flag exists → SessionEnd skips cleanup ✅ |
| Lead PF7-END (normal) | team-guard removes flag → TeamDelete → SessionEnd cleans up ✅ |
| Stale teams from previous sessions | Irrelevant — guard only checks flag, not team configs ✅ |
| Non-pathflow session | Flag never created → SessionEnd runs cleanup ✅ |
| Context overflow (orphaned flag) | session-start handles stale cleanup ✅ |
| PF6 never completed (abnormal PF7) | pf-6 sentinel missing → team-guard BLOCKS TeamDelete ✅ |
| Hard crash (no SessionEnd) | Flag persists → session-start handles stale cleanup ✅ |

### Key Insight

The pathflow-active flag is the single source of truth for session liveness. Team config files (`~/.claude/teams/`) are NOT reliable signals because:

- Stale teams accumulate from crashed/overflow sessions
- Multiple concurrent sessions share the same `~/.claude/teams/` directory
- Team config existence doesn't indicate session liveness

By having team-guard remove the flag during PF7-END (gated by pf-6 sentinel), and SessionEnd check only the flag (not team configs), the cleanup flow becomes deterministic and immune to stale state.
