# Part 12: Changes and Claude Components

> What's added, modified, and dropped. CLAUDE.md changes, hook changes, settings matchers, agent definitions, and schema migration.

---

## Table of Contents

- [12.1 Change Summary](#121-change-summary)
- [12.2 What's Added](#122-whats-added)
- [12.3 What's Modified](#123-whats-modified)
- [12.4 What's Dropped](#124-whats-dropped)
- [12.5 CLAUDE.md Changes](#125-claudemd-changes)
- [12.6 Hook Changes](#126-hook-changes)
- [12.7 Settings.json Changes](#127-settingsjson-changes)
- [12.8 Agent Definitions](#128-agent-definitions)
- [12.9 Schema Migration](#129-schema-migration)
- [12.10 Skills Coexistence](#1210-skills-coexistence)

---

## 12.1 Change Summary

| Category | Added | Modified | Dropped |
|----------|:-----:|:--------:|:-------:|
| PathFlow phases | 7 phases (PF-1 to PF-7) | - | 9-node outer shell |
| Work stages | WS-DEV, WS-REV, WS-QA, WS-WORK | - | WS-DEPLOY |
| Sentinels | PathFlow sentinels (session-scoped) | Existing skill sentinels preserved | Many skill sentinels redundant in agent-teams mode |
| Agent definitions | 8 new .md files | - | - |
| Hooks | 2 new (pathflow-gate, team-guard) | 2 modified (stop, post-tool-use) | 1 removed (verify-work in current form) |
| Settings | New matchers | Existing matchers updated | Over-engineered config layers |
| Schema | 5 new columns | - | - |
| Configurability | Tracked/Untracked | - | 4 layers, 3 policies, 4 session types |

---

## 12.2 What's Added

### PathFlow Phases

Seven logical phases that guide session progression. See [06-progressive-orchestration.md](06-progressive-orchestration.md).

| Phase | Name | Purpose |
|-------|------|---------|
| PF-1 | Session Start | Boot infrastructure, spawn persistent teammates |
| PF-2 | Context Awareness | Detect active work, load context |
| PF-3 | Work Classification | Classify work, register in WorkGraph, select stages |
| PF-4 | Work Execution | Run work stages (DEV, REVIEW, QA) |
| PF-5 | Work Verification | Verify acceptance criteria |
| PF-6 | Work Completion | Mark complete, create PR |
| PF-7 | Session End | Shut down team, clean up |

### Agent Definitions

New files in `.claude/agents/`:

| File | Category | Persistence | Source Skills |
|------|----------|:-----------:|---------------|
| `cf-gitops.md` | Function | Persistent | cf-git-workflow |
| `cf-knowledge-layer.md` | Function | Persistent | cf-memory-management, cf-task-management, cf-db-operations |
| `cf-developer.md` | Role | On-demand | (script standards, exploration) |
| `cf-planner.md` | Role | On-demand | cf-documentation-standards |
| `cf-reviewer.md` | Role | On-demand | cf-script-standards, (code exploration) |
| `cf-qa.md` | Role | On-demand | cf-testing-workflow, cf-script-standards |
| `cf-documenter.md` | Role | On-demand | cf-documentation-standards |
| `cf-ops.md` | Role | On-demand | (deployment procedures) |

### Work Stages

Pipeline stages within PF-4. See [07-work-stages.md](07-work-stages.md).

| Stage | Code | Used For |
|-------|------|----------|
| WS-DEV | Code development | cf-developer implements |
| WS-REV | Code/content review | cf-reviewer checks quality |
| WS-QA | Testing and quality | cf-qa writes and runs tests |
| WS-WORK | Non-code primary work | cf-documenter, cf-planner |

### PathFlow Sentinels

Session-scoped, no-TTL sentinels created on phase completion. See [08-enforcement-model.md](08-enforcement-model.md).

| Sentinel | Created When | Checked Before |
|----------|-------------|----------------|
| `pathflow:pf-1` | Session started | - |
| `pathflow:pf-2` | Context loaded | - |
| `pathflow:pf-3` | Work classified | Edit, Write, git commit |
| `pathflow:ws-dev-done` | Dev stage done | Review operations |
| `pathflow:ws-rev-done` | Review done | QA operations |
| `pathflow:ws-qa-done` | QA done | Completion operations |

### Schema Additions

Five new columns across two tables. See [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md).

```sql
-- tasks table
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;
ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL;
ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]';

-- active_work table
ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL;
ALTER TABLE active_work ADD COLUMN team_name TEXT DEFAULT NULL;
```

### Mode Detection Flag

A runtime state file at `/tmp/claude/managed/state/pathflow-active` created during PF-1 and removed during PF-7/SessionEnd. Used by all hooks to determine team vs standalone mode. Contains session metadata (session ID, timestamp, team name).

---

## 12.3 What's Modified

### CLAUDE.md

Sections added to support PathFlow orchestration. See [Section 12.5](#125-claudemd-changes).

### Stop Hook (cf-stop-verify-work.sh)

The verify-work Stop hook's PCV checking behavior is dropped entirely in agent-teams mode. In agent-teams mode, work verification is handled by the WS-REV stage during PF-4, not by a self-check at session end. The stop hook itself remains for logging purposes, but the PCV checking behavior is removed when agent-teams mode is active.

Mode detection uses the `/tmp/claude/managed/state/pathflow-active` flag (not team config file).

```
Before: Stop hook checks for PCV marker in last message (always)
After:  Stop hook checks pathflow-active flag at start:
        - Flag present (agent-teams mode): PCV check skipped entirely (WS-REV handles verification)
        - Flag absent (standalone mode): PCV check runs as before (unchanged behavior)
```

### PostToolUse Hook

Extended to detect phase marker completion and create PathFlow sentinels.

```
Before: Existing post-tool-use behaviors (logging, etc.)
After:  + If TaskUpdate completes a [PF-*] or [WS-*-DONE] task,
          create the corresponding pathflow sentinel
```

### Settings Matchers

New matcher entries for the pathflow-gate hook.

### Schema

Purely additive changes (new columns with NULL defaults). No existing columns modified. See [Section 12.9](#129-schema-migration).

---

## 12.4 What's Dropped

### Verify-Work as Stop Hook

**Removed**: The `cf-stop-verify-work.sh` hook's PCV checking behavior in agent-teams mode.

**Reason**: The WS-REV (Review) stage provides better verification than a stop-time self-check. Review happens earlier (during PF-4, not at session end), uses a separate agent (not self-review), and can trigger rework (not advisory-only). See [08-enforcement-model.md](08-enforcement-model.md), Section 8.8.

### DEPLOY Work Stage

**Removed**: WS-DEPLOY as a work stage in the pipeline.

**Reason**: PR creation is a procedural action by cf-gitops in PF-6, not a development pipeline stage. It does not need its own agent, review, or stage history entry. See [07-work-stages.md](07-work-stages.md), Section 7.8.

### Over-Engineered Configurability

**Removed**:

| Dropped Item | Replaced By |
|-------------|-------------|
| 4 configuration layers | Single tracked/untracked mode |
| 3 enforcement policies (strict/standard/permissive) | Full enforcement (tracked) or minimal (untracked) |
| 4 session types (interactive/autorun/structured/unstructured) | 2 properties: interactive/autorun + tracked/untracked |
| Solo mode | Not a mode. Lead decides whether to create team. |
| Session profiles | Derived from work_type + session properties |

**Reason**: Every configuration option that was "made up" (no real use case) is removed. What remains is the minimum needed to distinguish sessions that produce tracked work from sessions that don't. See the feedback analysis in [01-feedback-analysis.md](../pathflow-revised/01-feedback-analysis.md), Section 1.10.

### 9-Node Outer Shell

**Removed**: The M1 design's OS-1 through OS-9 phase structure.

**Replaced by**: 7-phase PF-1 through PF-7. Redundant phases merged: "Intent Identification" into PF-3, "Work Path Selection" into PF-3, "Post-Work Finalize" into PF-6, "Session Close" into PF-7.

### Context Monitoring

**Removed**: Custom hook-based self-reporting of context usage.

**Reason**: Not reliable without native Claude Code support. Context degradation is managed by keeping on-demand teammates fresh (spawned per task) and recycling persistent teammates when needed. See [01-feedback-analysis.md](../pathflow-revised/01-feedback-analysis.md), Section 1.3.

---

## 12.5 CLAUDE.md Changes

CLAUDE.md needs new sections to support PathFlow. These additions go into Sections 4-5 of the existing file.

### Section 4: PathFlow Session Management

Content to add:

```markdown
## PathFlow Session Management

### Session Mode
- **Tracked**: Work is registered in WorkGraph, PathFlow phases active,
  enforcement enabled
- **Untracked**: Questions, exploration, quick lookups. No PathFlow phases.

### Phase Progression
Sessions progress through PF-1 to PF-7 in order.
Each phase is a Claude Task marker with [PF-N] prefix.
Create next phase only when current phase completes (progressive orchestration).

### Team Lead Role
You are the orchestrator. You do NOT write code, edit files, or run commands
directly. You create tasks, assign them to teammates, and manage the workflow.
Exceptions: verification checks, user communication, team management.

### Team Flexibility
The predefined teammate roster (cf-gitops, cf-knowledge-layer, cf-developer, etc.)
provides optimized defaults, NOT constraints. You may spawn ad-hoc teammates
at any time:
- General-purpose teammates for tasks that don't fit predefined roles
- Explore sub-agents for quick read-only research
- Custom agent definitions for project-specific needs
Use your judgment. If a task needs a specialist not in the roster, spawn one.

### Session Boundary
One PR per session. After PF-6 (Work Completion), proceed to PF-7 and end.

### Team Persistence
NEVER call Teammate(operation="cleanup") during an active session.
The team's task list stores the entire PathFlow orchestration graph — phase
markers, work stages, dependencies, assignments. Dissolving the team destroys
all of this with NO recovery path.

- Individual teammate shutdown (SendMessage type="shutdown_request") is SAFE
- Team cleanup is ONLY called during PF-7 (Session End) as the final step
- Shutting down teammates does NOT affect the team or its task list
- Only Teammate(operation="cleanup") destroys the team and tasks
```

### Section 5: Teammate Coordination

Content to add:

```markdown
## Teammate Coordination

### Function Teammates (Persistent)
- **cf-knowledge-layer**: Knowledge Layer interface. Message for any WorkGraph,
  memory, or DB operation.
- **cf-gitops**: Git operations. Message for any branch, commit, PR, or
  sync operation.

### Role Teammates (On-Demand)
- Spawn per work stage. Shut down after stage completion.
- cf-developer, cf-reviewer, cf-qa, cf-planner, cf-documenter, cf-ops

### Ad-Hoc Teammates
The predefined roster is a set of optimized defaults, not a constraint.
Spawn additional teammates as needed:
- General-purpose agents for tasks outside predefined roles
- Explore sub-agents for quick read-only research during any phase
- Custom-typed agents with project-specific agent definitions
The lead decides team composition based on the work at hand.

### Communication
- Direct peer messaging between teammates (not hub-and-spoke)
- WorkGraph updates: always through cf-knowledge-layer
- Git operations: always through cf-gitops
- Escalations: to team lead
```

---

## 12.6 Hook Changes

### New Hook: cf-pre-tool-use-pathflow-gate.sh

| Property | Value |
|----------|-------|
| Location | `.claude/hooks/codeflow/pre-tool-use/` |
| Type | PreToolUse |
| Matcher | Edit, Write, Bash |
| Purpose | Check PathFlow sentinels before allowing operations |
| Behavior | Block if required sentinel missing (agent-teams mode); fall back to skill sentinels (standalone) |

### Modified Hook: PostToolUse (TaskUpdate handling)

| Property | Value |
|----------|-------|
| Change | Add phase marker detection to existing PostToolUse hook |
| Trigger | TaskUpdate with subject matching `[PF-*]` or `[WS-*-DONE]` |
| Action | Create PathFlow sentinel file |

### Modified Hook: cf-stop-verify-work.sh

| Property | Value |
|----------|-------|
| Change | Add mode detection check at the start |
| Logic | If `/tmp/claude/managed/state/pathflow-active` exists, skip PCV check entirely (WS-REV handles verification during PF-4) |
| Standalone mode | Unchanged behavior (PCV check remains) |

### Hook Priority Summary

```
PreToolUse hooks (order matters):
  1. cf-pre-tool-use-security.sh          (security checks first)
  2. cf-pre-tool-use-protected-resource.sh (protected file checks)
  3. cf-pre-tool-use-pathflow-gate.sh      (NEW: PathFlow sentinel checks)
  4. cf-pre-tool-use-edit-write.sh         (file scope checks)
  5. cf-pre-tool-use-bash-sentinel.sh      (bash command sentinel checks)
  6. cf-pre-tool-use-file-sentinel.sh      (file operation sentinel checks)

PostToolUse hooks:
  1. cf-post-tool-use-logging.sh           (audit logging)
  2. cf-post-tool-use-skill.sh             (skill sentinel creation)
  3. (EXTENDED) sentinel creation for PathFlow phase markers

Stop hooks:
  1. cf-stop-logging.sh                    (session logging)
  2. cf-stop-verify-work.sh                (MODIFIED: skip in agent-teams mode)
```

### Scripts Requiring Mode-Awareness Updates

These existing scripts need to be updated to check the `/tmp/claude/managed/state/pathflow-active` flag and branch behavior based on team vs standalone mode:

| Script | Current Behavior | Agent-Teams Mode Change |
|--------|-----------------|------------------|
| `cf-post-tool-use-skill.sh` | Creates skill sentinels | Also create PathFlow sentinels on phase markers |
| `cf-pre-tool-use-bash-sentinel.sh` | Checks skill sentinels | Check PathFlow sentinels instead |
| `cf-pre-tool-use-file-sentinel.sh` | Checks skill sentinels | Check PathFlow sentinels instead |
| `cf-pre-tool-use-task-sentinel.sh` | Checks task context | Check PathFlow phase markers |
| `cf-pre-tool-use-grep-sentinel.sh` | Checks skill sentinels | PathFlow awareness |
| `cf-session-start-cleanup.sh` | Cleans skill sentinels | Clean both types |
| `cf-session-end-cleanup.sh` | Cleans skill sentinels | Clean both types + remove pathflow-active flag |
| `cf-sentinel.sh` (library) | Supports skill sentinels | Support both PathFlow and skill sentinel types |
| `security-lib.sh` | Sentinel helpers | Mode-aware sentinel checking |
| `cf-post-tool-use-memory-progress.sh` | References sentinels | PathFlow sentinel awareness |
| `cf-stop-verify-work.sh` | PCV check always | Skip PCV in agent-teams mode |

### New Hook: Teammate Cleanup Guard (cf-pre-tool-use-team-guard.sh)

A PreToolUse hook matching `Teammate` calls intercepts `Teammate(operation="cleanup")` and **blocks it** if the `/tmp/claude/managed/state/pathflow-active` flag exists. This is a mandatory defense-in-depth guardrail against accidental team dissolution during an active PathFlow session, complementing the CLAUDE.md instruction-level rule documented in [Section 12.5](#125-claudemd-changes). Dissolving a team mid-session destroys the entire Claude Task list — all phase markers, work stages, dependencies, and assignments — with no recovery path.

| Property | Value |
|----------|-------|
| Location | `.claude/hooks/codeflow/pre-tool-use/` |
| Type | PreToolUse |
| Matcher | `Teammate` (the Claude Code tool for team management) |
| Purpose | Block the `Teammate` tool's `cleanup` operation during active PathFlow sessions |
| Logic | Parse tool input JSON from stdin; if `operation == "cleanup"` AND `pathflow-active` flag exists, exit 2 (BLOCK) |
| Standalone mode | Pass through (no flag file present, so no PathFlow session to protect) |

**Settings.json matcher**:

```json
{
  "matcher": "Teammate",
  "hooks": [
    {
      "type": "command",
      "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh"
    }
  ]
}
```

**Hook pseudocode**:

```text
on PreToolUse(Teammate, input):
  operation = parse_json(stdin, "operation")
  if operation != "cleanup":
    exit 0  # Allow all non-cleanup Teammate operations (spawnTeam, etc.)
  if not file_exists("/tmp/claude/managed/state/pathflow-active"):
    exit 0  # No active PathFlow session, allow cleanup
  # Active session + cleanup = BLOCK
  echo "BLOCKED: Cannot dissolve team during active PathFlow session."
  echo "The team's task list contains the entire session graph."
  echo "Team cleanup only happens during PF-7 (Session End)."
  exit 2
```

---

## 12.7 Settings.json Changes

New matcher entries needed in `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Edit|Write",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh"
          }
        ]
      },
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh"
          }
        ]
      },
      {
        "matcher": "TaskUpdate",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-task.sh"
          }
        ]
      },
      {
        "matcher": "Teammate",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "TaskUpdate",
        "hooks": [
          {
            "type": "command",
            "command": ".claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh"
          }
        ]
      }
    ]
  }
}
```

---

## 12.8 Agent Definitions

Agent definitions are created in `.claude/agents/`. Each file follows the standard format documented in [03-teammate-model.md](../pathflow-revised/03-teammate-model.md), Section 3.2.

**Files to create**:

| File | Size Estimate | Content Source |
|------|:------------:|----------------|
| `cf-gitops.md` | ~200 lines | cf-git-workflow SKILL.md operations |
| `cf-knowledge-layer.md` | ~300 lines | cf-memory-management + cf-task-management + cf-db-operations |
| `cf-developer.md` | ~100 lines | Role identity + communication protocol |
| `cf-planner.md` | ~80 lines | Role identity + planning SOPs |
| `cf-reviewer.md` | ~100 lines | Role identity + review checklist |
| `cf-qa.md` | ~120 lines | Role identity + testing SOPs |
| `cf-documenter.md` | ~80 lines | Role identity + doc standards |
| `cf-ops.md` | ~80 lines | Role identity + deployment SOPs |

**Agent definition structure**:

```
---
name: cf-{role}
description: {purpose}. {when spawned}.
---

# {Role Name}

## Identity
## Constraints
## Standard Operating Procedures
## Communication
## Quality Checklist
```

**Loading mechanism**: At spawn time, the lead includes in the spawn prompt: "Read and adopt your agent definition at `.claude/agents/cf-{role}.md`." The teammate reads the file and follows its instructions. See test findings in [agent-teams-test-findings.md](../agent-teams-test-findings.md), Tests 1b/1c.

---

## 12.9 Schema Migration

The schema changes are purely additive. Migration can be run as a single SQL script:

```sql
-- PathFlow v3 schema migration
-- Safe to run multiple times (IF NOT EXISTS / ADD COLUMN are idempotent)

-- Tasks table: stage tracking
ALTER TABLE tasks ADD COLUMN stage TEXT DEFAULT NULL;
ALTER TABLE tasks ADD COLUMN stage_status TEXT DEFAULT NULL;
ALTER TABLE tasks ADD COLUMN stage_history TEXT DEFAULT '[]';

-- Active work table: quick lookups
ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL;
ALTER TABLE active_work ADD COLUMN team_name TEXT DEFAULT NULL;
```

**Backward compatibility**:

- All new columns have NULL or empty defaults
- Existing queries that don't reference new columns work unchanged
- No existing columns are modified or removed
- The migration is idempotent (safe to run multiple times)

**Rollback**: Since the columns are additive and default to NULL, rollback is simply dropping the columns:

```sql
-- Rollback (if needed)
ALTER TABLE tasks DROP COLUMN stage;
ALTER TABLE tasks DROP COLUMN stage_status;
ALTER TABLE tasks DROP COLUMN stage_history;
ALTER TABLE active_work DROP COLUMN current_stage;
ALTER TABLE active_work DROP COLUMN team_name;
```

---

## 12.10 Skills Coexistence

Skills remain as the canonical SOPs. They are NOT deprecated or replaced.

```
+-------------------+          +-------------------+
|   SKILLS          |          |  AGENT DEFS       |
|   (.claude/skills)|          |  (.claude/agents) |
+-------------------+          +-------------------+
| Canonical SOPs    |--------->| Load at Spawn     |
| Standalone mode     |          | Agent-teams mode          |
| User-invokable    |          | Reference on Demand|
| Slash commands    |          | Teammate identity  |
+-------------------+          +-------------------+
```

**Skill integration tiers**:

| Tier | Name | Description |
|------|------|-------------|
| Load at Spawn | Skills embedded directly into the agent definition, loaded when the teammate is spawned |
| Reference on Demand | Skills referenced by the agent definition but read only when a specific procedure is needed |
| Behavioral / Cross-Cutting | Skills that apply broadly across agents (e.g., documentation standards, script standards) |

**Routing in team vs standalone mode**:

| Action | Agent-Teams Mode | Standalone Mode |
|--------|-----------|---------------|
| `/cf-commit` | Lead tells cf-gitops | Skill invoked directly |
| `/cf-plan` | Lead assigns cf-planner | Skill invoked directly |
| `/cf-develop` | Lead assigns cf-developer | Skill invoked directly |
| `/cf-review` | Lead assigns cf-reviewer | Skill invoked directly |

Skills provide backward compatibility for sessions that don't use Agent Teams. Agent definitions provide the agent-teams-mode equivalent with added identity, constraints, and communication protocols.

---

## Related Documents

- [02-revised-architecture.md](../pathflow-revised/02-revised-architecture.md) -- Full architecture reference
- [03-teammate-model.md](../pathflow-revised/03-teammate-model.md) -- Agent definition format
- [06-changes-from-m1.md](../pathflow-revised/06-changes-from-m1.md) -- M1 to v3 delta
- [06-progressive-orchestration.md](06-progressive-orchestration.md) -- Phase mechanics
- [08-enforcement-model.md](08-enforcement-model.md) -- Hook enforcement details
- [09-knowledge-layer-integration.md](09-knowledge-layer-integration.md) -- Schema details
