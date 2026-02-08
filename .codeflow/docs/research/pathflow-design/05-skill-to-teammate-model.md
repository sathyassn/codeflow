# Skill-to-Teammate Conversion Model

> Design specification for converting CodeFlow V3 skills and agents into Agent Teams teammates with configurable persistence.
> Designer: teammate-designer | Date: 2026-02-06

---

## 1. Overview and Design Philosophy

### Core Principle: Skills Are Instructions, Not Services

In V3, skills are forked into isolated sub-agent contexts via `cf-general-purpose`. Each skill invocation creates a transient execution environment, loads the SKILL.md, performs operations, and returns results. The agent itself decides when to invoke which skill.

In Agent Teams, teammates ARE the persistent execution environments. There is no need for the intermediate "fork a sub-agent to run a skill" pattern. Instead, skills become **behavioral instructions embedded in teammate blueprints**. The teammate reads its blueprint at spawn time and carries those instructions throughout its lifecycle.

### Design Philosophy

1. **Teammates are role-defined, not skill-defined.** A teammate is "the developer" -- not "the git-workflow executor." Skills dissolve into the behavioral instructions of the teammates that use them.

2. **The team lead orchestrates, never implements.** Enforced via hooks (not built-in delegate mode, which is interactive-only). The lead uses TaskCreate, TaskUpdate, SendMessage, and TeammateTool exclusively.

3. **Persistence follows usage frequency.** Teammates that are needed across many sequential tasks persist. Teammates needed for one bounded task are spawned on-demand and shut down after.

4. **Context is the scarce resource.** Every design decision is evaluated against context window consumption. Teammates are recycled when context is exhausted, not when they are conceptually "done."

5. **Communication is message-based, not state-based.** V3 agents communicated via shared database (Knowledge Layer). Agent Teams teammates communicate via SendMessage and the shared Task list. This is a fundamental shift from shared-state coordination to message-passing coordination.

---

## 2. Blueprint Specification

### 2.1 Blueprint Location and Format

Blueprints live at `.claude/agents/cf-{role}.md` and follow the standard Claude Code agent definition format.

```
.claude/agents/
  cf-developer.md       # Implementation specialist
  cf-planner.md         # Planning specialist
  cf-reviewer.md        # Code review specialist
  cf-qa.md              # Test automation specialist
  cf-ops.md             # DevOps/deployment specialist
  cf-documenter.md      # Documentation specialist
  cf-support.md         # User assistance (sub-agent prompt, not a full blueprint)
```

### 2.2 Blueprint Structure

Every blueprint follows this canonical structure:

```markdown
---
name: "cf-{role}"
description: "{One-line description}. {When the lead should spawn this teammate}."
---

# {Role Name}

## Identity

You are **cf-{role}**, a {role description} on the CodeFlow team.
Your team lead coordinates your work through the task system and messages.

## Constraints

### Branch Access
- Write: {branch patterns this teammate may commit to}
- Read: all branches

### File Scope
- Write: {file patterns this teammate may modify}
- Read: all files (subject to read delegation thresholds)

### Tool Restrictions
- {Any tools this teammate must NOT use, or special usage rules}

### Memory Domain
- Write domain: `{domain}` (exclusive -- no other teammate writes here)
- Read: all domains

## Operations

### {Operation Group 1} (from cf-{skill})
{Embedded operational procedures from the relevant skill}

### {Operation Group 2} (from cf-{skill})
{Embedded operational procedures from the relevant skill}

## Working Protocol

### Before Starting Work
1. Read the task description fully (use TaskGet if needed)
2. Assess scope and identify affected files
3. Verify you are on the correct branch

### During Work
- Record progress on significant milestones (update task description or send message to lead)
- Stay within your file scope and branch access constraints
- If blocked, message the team lead immediately

### Completing Work
1. Verify all changes are correct and complete
2. Run relevant tests if applicable
3. Mark your task as completed via TaskUpdate
4. Send a summary message to the team lead with:
   - What was done (files changed, decisions made)
   - Any follow-up work identified
   - Any concerns or risks

## Quality Gates

### Before Marking Task Complete
- [ ] All changes are within declared file scope
- [ ] Changes compile/parse without errors
- [ ] {Role-specific verification requirements}
- [ ] Summary sent to team lead

## Communication Protocol

### Reporting to Lead
- Use SendMessage for completion reports, blockers, and questions
- Use TaskUpdate to mark task status transitions
- Include file paths and line numbers in technical reports

### Peer Coordination
- All peer communication goes through the team lead
- Never message another teammate directly unless instructed by the lead
```

### 2.3 Token Budget Guidelines

| Blueprint | Target Tokens | Components |
|-----------|:------------:|------------|
| cf-developer | ~2,000 | Identity + git ops + code exploration + script standards + quality gates |
| cf-planner | ~1,500 | Identity + documentation standards + planning procedures |
| cf-reviewer | ~1,000 | Identity + code exploration + review checklist |
| cf-qa | ~1,500 | Identity + testing workflow + git ops (commit only) |
| cf-ops | ~1,500 | Identity + security procedures + git ops (PR/merge) |
| cf-documenter | ~1,200 | Identity + documentation standards + git ops (commit only) |
| cf-support | ~600 | Identity + code exploration (minimal, sub-agent prompt only) |

### 2.4 Example Blueprint: cf-developer

```markdown
---
name: "cf-developer"
description: "Implementation specialist. Spawn for feature development, bug fixes, and refactoring work."
---

# Developer

## Identity

You are **cf-developer**, an implementation specialist on the CodeFlow team.
Your team lead coordinates your work through the task system and messages.
You focus on writing correct, tested, minimal code that solves the assigned task.

## Constraints

### Branch Access
- Write: `feat/*`, `fix/*`, `refactor/*`
- Read: all branches

### File Scope
- Write: source code, test files, configuration files
- Protected (requires lead approval): `.claude/**`, `.codeflow/config/**`, `.codeflow/scripts/security/**`
- Read: all files

### Tool Restrictions
- Do NOT use `gh pr create`, `gh pr merge`, or `git push` -- these are ops-scope operations
- Do NOT modify epic or planning documents -- these are planner-scope

### Memory Domain
- Write domain: `development`
- Read: all domains

## Operations

### Git Operations
- **create-branch**: Create feature/fix/refactor branches following naming convention
  - Pattern: `{type}/{area-code}-{short-description}` (e.g., `feat/frt-login-button`)
- **create-commit**: Stage and commit changes with conventional commit format
  - Format: `{type}({scope}): {description}`
  - Verify all staged files are within your branch access scope
- **review-changes**: Use `git diff` to review your own changes before committing

### Code Exploration
- Use Glob for file discovery, Grep for content search, Read for file contents
- For large codebases, search incrementally: start narrow, widen if needed
- When reading unfamiliar code, identify the entry point first

### Script Standards (Quality)
- Shell scripts: use `set -euo pipefail`, quote variables, use `local` in functions
- Python scripts: use type hints, handle errors explicitly
- All scripts must be executable (`chmod +x`)

## Working Protocol

### Before Starting Work
1. Read the task description fully
2. Identify affected files and understand existing code
3. Create or switch to the correct branch
4. Verify no other teammate has conflicting file claims

### During Work
- Commit frequently at logical checkpoints
- Run tests after significant changes
- Message the lead if scope expands beyond the original task

### Completing Work
1. Run all relevant tests and verify they pass
2. Review your own diff (`git diff`) for correctness
3. Commit all changes
4. Mark task as completed via TaskUpdate
5. Send summary to team lead

## Quality Gates

### Before Marking Task Complete
- [ ] All changes compile/parse without errors
- [ ] Tests pass (existing and new)
- [ ] Changes are within declared file scope
- [ ] Commit messages follow conventional format
- [ ] No TODO/FIXME introduced without a linked task
- [ ] Summary sent to team lead
```

---

## 3. Persistence Model

### 3.1 Persistence Types

| Type | Lifecycle | Context Window | Resource Cost | Use When |
|------|-----------|---------------|---------------|----------|
| **PERSISTENT** | Spawned once, receives sequential tasks until session end or context exhaustion | Accumulates across tasks (high) | High (continuous API usage) | Core work role needed across many sequential tasks |
| **ON-DEMAND** | Spawned for a specific task, shut down after completion | Fresh per task (clean) | Medium (spawn + task + shutdown) | Episodic work with clear boundaries |
| **SUB-AGENT** | Task tool call, returns result, no persistent identity | Minimal (single call) | Low (one-shot) | Read-only queries, quick checks, exploration |
| **DISSOLVED** | Not a teammate -- skill absorbed into another blueprint | Zero (no separate entity) | Zero | Behavioral directives, replaced by native tools |

### 3.2 Default Persistence Configuration

| Teammate | Default Persistence | Rationale |
|----------|:------------------:|-----------|
| Team Lead | PERSISTENT | Session orchestrator, always present |
| cf-developer | PERSISTENT | Feature work spans many tool calls; context accumulation is productive |
| cf-planner | ON-DEMAND | Planning is episodic; produces artifacts and completes |
| cf-reviewer | ON-DEMAND (or SUB-AGENT for simple PRs) | Reviews are bounded per-PR tasks |
| cf-qa | ON-DEMAND | Testing is episodic; run tests, report, done |
| cf-ops | ON-DEMAND | Shipping/deployment is episodic, end-of-cycle |
| cf-documenter | ON-DEMAND (or SUB-AGENT for simple updates) | Documentation is bounded per-deliverable |
| cf-support | SUB-AGENT | Single question-answer cycle, read-only |

### 3.3 Configuration Schema

The persistence configuration is stored in the team lead's session context (not a separate config file, since it is runtime behavior):

```json
{
  "_pathflow_persistence": {
    "defaults": {
      "cf-developer": "persistent",
      "cf-planner": "on-demand",
      "cf-reviewer": "on-demand",
      "cf-qa": "on-demand",
      "cf-ops": "on-demand",
      "cf-documenter": "on-demand",
      "cf-support": "sub-agent"
    },
    "overrides": {},
    "context_thresholds": {
      "warning": 0.70,
      "recycle": 0.85
    }
  }
}
```

### 3.4 Per-Session Overrides

The user (or team lead) can override defaults for specific session types:

| Override Scenario | Change | Rationale |
|-------------------|--------|-----------|
| Intensive planning session | `cf-planner: persistent` | Many sequential planning tasks benefit from accumulated context |
| Multi-PR review session | `cf-reviewer: persistent` | Reviewing multiple related PRs benefits from cross-PR context |
| Large refactor | `cf-developer: persistent` + `cf-qa: persistent` | Tight dev-test loop needs both alive |
| Quick fix | All non-developer: `sub-agent` | Minimize overhead for a simple change |

The lead applies overrides by noting them at session start. There is no declarative override file -- the lead's instructions to itself (via PathFlow awareness) drive the decisions.

### 3.5 Auto-Recycling Strategy

When a teammate's context window reaches the threshold:

1. **At 70% (warning):** PostToolUse hook on the teammate detects threshold via context window data from the hook environment. Teammate sends a warning message to the lead: "Context at 70%. Consider winding down current task or recycling."

2. **At 85% (recycle recommendation):** Teammate sends urgent message. Lead should:
   - Allow teammate to finish current task if close to completion
   - OR have teammate send a handoff summary, then shut it down
   - Spawn a fresh replacement with the handoff summary in its spawn prompt

3. **Context exhaustion (system-level):** If context is fully consumed, the teammate's responses degrade. The system may compress prior messages automatically. The lead should preemptively recycle before this point.

**Recycling procedure:**
```
1. Lead sends message: "Prepare for recycling. Summarize your current state."
2. Teammate responds with:
   - Current task status
   - Files modified and why
   - Decisions made and rationale
   - Remaining work
3. Lead sends shutdown_request
4. Lead spawns replacement with handoff summary in spawn prompt
5. Lead assigns remaining work to replacement
```

---

## 4. Spawn Prompt Builder Pattern

### 4.1 Spawn Prompt Structure

When the team lead spawns a teammate, the spawn prompt is constructed from four sections:

```
[1. Team Preamble]     -- Standard across all teammates (~200 tokens)
[2. Blueprint Load]    -- Instruct teammate to read its agent definition
[3. Task Assignment]   -- Current specific task
[4. Constraints]       -- Session-specific constraints
```

### 4.2 Section Details

#### Section 1: Team Preamble (standardized)

```
You are a teammate on the CodeFlow team managed by PathFlow.

Team: {team_name}
Lead: team-lead
Your name: cf-{role}

Communication:
- Send messages to the team lead using SendMessage when you complete tasks, encounter blockers, or have questions.
- Use TaskUpdate to mark tasks in_progress when starting and completed when done.
- All peer communication goes through the team lead.

Context awareness:
- Monitor your context usage. If you notice responses degrading or feel you are losing earlier context, message the lead proactively.
```

#### Section 2: Blueprint Load (two strategies)

**Strategy A: Instruct-to-Read (recommended for large blueprints)**

```
Read your agent definition file at `.claude/agents/cf-{role}.md` before starting work.
It contains your role identity, constraints, operations, and quality gates.
Follow those instructions for the duration of your session.
```

Pro: Saves spawn prompt tokens. Con: Costs one Read tool call at start.

**Strategy B: Inline (for small blueprints or critical instructions)**

```
{Full blueprint content inlined here}
```

Pro: Immediate availability, no tool call needed. Con: Larger spawn prompt.

**Recommendation:** Use Strategy A (instruct-to-read) as the default. The cost of one Read tool call is negligible compared to the spawn prompt token savings. Use Strategy B only for sub-agents (cf-support) where the blueprint is tiny (<600 tokens) and the agent's lifecycle is a single task.

#### Section 3: Task Assignment

```
Your current task:

Task ID: {task_id}
Subject: {task_subject}
Description: {task_description}

Start by marking this task as in_progress via TaskUpdate, then proceed with the work.
```

#### Section 4: Constraints (session-specific)

```
Session constraints:
- Branch: {current branch or branch to create}
- File scope: {any additional restrictions for this specific task}
- Coordination: {any teammates to be aware of, concurrent work notes}
```

### 4.3 Complete Spawn Prompt Example

```
You are a teammate on the CodeFlow team managed by PathFlow.

Team: feature-login-redesign
Lead: team-lead
Your name: cf-developer

Communication:
- Send messages to the team lead using SendMessage when you complete tasks, encounter blockers, or have questions.
- Use TaskUpdate to mark tasks in_progress when starting and completed when done.
- All peer communication goes through the team lead.

Context awareness:
- Monitor your context usage. If you notice responses degrading or feel you are losing earlier context, message the lead proactively.

Read your agent definition file at `.claude/agents/cf-developer.md` before starting work.
It contains your role identity, constraints, operations, and quality gates.
Follow those instructions for the duration of your session.

Your current task:

Task ID: 3
Subject: Implement login form validation
Description: Add client-side validation to the login form. Validate email format
and password minimum length (8 chars). Show inline error messages below each field.
Files: src/components/LoginForm.tsx, src/utils/validation.ts

Start by marking this task as in_progress via TaskUpdate, then proceed with the work.

Session constraints:
- Branch: feat/frt-login-redesign (already created, switch to it)
- File scope: src/components/Login*.tsx, src/utils/validation.ts, src/components/__tests__/Login*.test.tsx
- Coordination: cf-qa will run tests after you complete. Ensure tests are runnable.
```

### 4.4 Spawn Prompt Token Budgets

| Section | Target Tokens | Notes |
|---------|:------------:|-------|
| Team preamble | ~200 | Fixed across all spawns |
| Blueprint load instruction | ~50 | Strategy A (instruct-to-read) |
| Blueprint inlined | ~600-2,000 | Strategy B (inline), varies by role |
| Task assignment | ~100-300 | Varies by task complexity |
| Constraints | ~50-150 | Session-specific |
| **Total (Strategy A)** | **~400-700** | Lean prompt, teammate reads blueprint |
| **Total (Strategy B)** | **~1,000-2,700** | Heavier prompt, no read needed |

---

## 5. Complete Skill Dissolution/Transformation Map

### 5.1 Transformation Summary

| # | Skill | Tokens | Transformation | Target |
|---|-------|:------:|:--------------:|--------|
| 1 | cf-working-protocol | ~1,200 | **DISSOLVE** | Behavioral directives embedded in all blueprints |
| 2 | cf-task-management | ~800 | **DISSOLVE** | Replaced by native TaskCreate/TaskUpdate/TaskList |
| 3 | cf-memory-management | ~1,500 | **TRANSFORM** | Work lifecycle operations in writing teammates + lead session management |
| 4 | cf-git-workflow | ~1,100 | **TRANSFORM** | Git operation procedures shared across multiple blueprints |
| 5 | cf-code-exploration | ~500 | **DISSOLVE** | Replaced by native Glob/Grep/Read; patterns inlined as tips |
| 6 | cf-script-standards | ~800 | **TRANSFORM** | Quality gate procedures in developer/ops/qa blueprints |
| 7 | cf-documentation-standards | ~600 | **TRANSFORM** | Quality gate procedures in documenter/planner blueprints |
| 8 | cf-security-management | ~1,000 | **TRANSFORM** | Enforcement operations via hooks + ops blueprint procedures |
| 9 | cf-db-operations | ~600 | **PARTIAL DISSOLVE** | Task CRUD replaced by native tools; memory/session ops may persist for cross-session tracking |
| 10 | cf-model-orchestrator | ~1,800 | **DEFER** | Replaced by native Agent Teams multi-agent; advanced multi-model deferred to v2 |
| 11 | cf-testing-workflow | ~700 | **TRANSFORM** | Testing procedures in qa blueprint + run-tests in developer blueprint |

### 5.2 Detailed Transformation Notes

#### cf-working-protocol (DISSOLVE)

The 7 operations become:

| Operation | Transformation |
|-----------|---------------|
| meta-awareness | Team lead session start procedure (not a teammate concern) |
| think-and-act | Condensed into "Before Starting Work" section of each blueprint |
| decide | Condensed into "During Work" guidance in each blueprint |
| respond-organized | Condensed into "Communication Protocol" section of each blueprint |
| research-quality | Embedded as quality gate: "Verify claims before reporting" |
| verify-work | Embedded as quality gate: "Before Marking Task Complete" checklist |
| parallelize-work | Team lead's orchestration responsibility (spawn multiple teammates) |

**Net effect:** ~300 tokens of distilled behavioral directives appear in every blueprint's Working Protocol and Quality Gates sections, replacing the full ~1,200 token skill.

#### cf-task-management (DISSOLVE)

| Operation | Replacement |
|-----------|------------|
| understand-request | Team lead's natural language processing |
| classify-work | Team lead's routing decision (which teammate to spawn/assign) |
| ensure-work-registered | TaskCreate by the team lead |
| create-epic | TaskCreate with epic-level description |
| update-epic | TaskUpdate on the epic task |
| create-task | TaskCreate with task-level description |
| update-task | TaskUpdate by any teammate |
| query-tasks | TaskList by any teammate |

**Net effect:** Completely replaced by native Agent Teams task tools. Zero tokens in blueprints.

#### cf-memory-management (TRANSFORM)

| Operation | Transformation |
|-----------|---------------|
| detect-active-work | Team lead's session start: check for in-progress tasks in TaskList |
| load-work-context | Team lead reads task descriptions and metadata at session start |
| search-related-work | Team lead queries TaskList before creating new tasks |
| evaluate-context | Each teammate's "Before Starting Work" protocol |
| begin-work | Teammate marks task as in_progress via TaskUpdate |
| record-work-progress | Teammate sends progress messages to lead; lead updates task metadata |
| complete-work | Teammate marks task as completed via TaskUpdate + sends summary |
| manage-memory-lifecycle | Team lead's periodic maintenance (archive old tasks, clean state) |

**Net effect:** Work lifecycle becomes task status transitions + message-based reporting. ~200 tokens of lifecycle procedures per writing teammate blueprint. Cross-session persistence (JSONL/SQLite) deferred to a separate "session persistence" design.

#### cf-git-workflow (TRANSFORM)

Operations are distributed across blueprints based on who uses them:

| Operation | Which Blueprints |
|-----------|-----------------|
| create-branch | cf-developer, cf-planner, cf-qa, cf-documenter |
| create-commit | cf-developer, cf-planner, cf-qa, cf-documenter |
| create-pull-request | cf-ops only |
| create-worktree | cf-developer (advanced use) |
| cleanup-worktrees | cf-ops |
| sync-remote | cf-ops only |
| merge-branch | cf-ops only |
| review-changes | cf-developer (self-review), cf-reviewer |
| check-branch-status | All writing teammates |
| rebase-interactive | cf-developer (advanced use) |

**Net effect:** ~200-400 tokens of git procedures per blueprint, covering only the operations that blueprint needs. Network operations (push, PR, merge) are exclusively in cf-ops.

#### cf-security-management (TRANSFORM)

| Operation | Transformation |
|-----------|---------------|
| sandbox-check | PreToolUse hook enforcement (remains in hook system, not in blueprints) |
| validate-protected-resource | PreToolUse hook enforcement |
| check-permissions | PreToolUse hook enforcement |
| request-permission | Stop hook + team lead escalation via SendMessage |
| stage-protected-edit | cf-ops blueprint procedure for protected file changes |
| apply-protected-edit | cf-ops blueprint procedure |
| sync-settings-templates | cf-ops blueprint procedure |

**Net effect:** Most security operations remain in the hook system (unchanged). ~150 tokens of security-specific procedures in cf-ops blueprint. Other blueprints reference constraints in their "File Scope" section.

#### cf-db-operations (PARTIAL DISSOLVE)

| Operation | Transformation |
|-----------|---------------|
| epic-create | TaskCreate (native) |
| epic-update | TaskUpdate (native) |
| task-create | TaskCreate (native) |
| task-update | TaskUpdate (native) |
| memory-store | Deferred -- may use shared file or task metadata |
| memory-query | Deferred -- may use task metadata or shared file reads |
| session-record | Hook system (automatic, no blueprint involvement) |
| log-append | Hook system (automatic, no blueprint involvement) |

**Net effect:** Task CRUD eliminated. Memory operations deferred. Session/log operations unchanged (hooks). Zero tokens in blueprints.

#### cf-code-exploration (DISSOLVE)

| Operation | Transformation |
|-----------|---------------|
| select-search-strategy | Inline tip in developer/reviewer blueprints: "Search narrow first, widen if needed" |
| load-compressed-context | Not needed -- teammates have their own context windows |
| search-symbol | Native Grep tool |
| analyze-dependencies | Native Grep + Glob tools |
| navigate-to-definition | Native Grep tool |
| find-all-references | Native Grep tool |

**Net effect:** ~50 tokens of search tips in relevant blueprints. All operations are native tool usage.

#### cf-testing-workflow (TRANSFORM)

| Operation | Which Blueprints |
|-----------|-----------------|
| run-tests | cf-qa (primary), cf-developer (self-test) |
| check-coverage | cf-qa only |
| analyze-failures | cf-qa only |
| suggest-tests | cf-qa only |
| run-affected-tests | cf-developer (run tests for changed files) |

**Net effect:** ~300 tokens of testing procedures in cf-qa blueprint. ~100 tokens of run-tests guidance in cf-developer blueprint.

---

## 6. Team Composition Templates

### 6.1 Feature Development (most common)

```
Team Lead (PERSISTENT)
  |-- cf-developer (PERSISTENT) -- assigned feature implementation tasks sequentially
  |-- cf-planner (ON-DEMAND) -- spawned if planning is needed before development
  |-- cf-reviewer (ON-DEMAND) -- spawned after PR is ready for review
  |-- cf-qa (ON-DEMAND) -- spawned after implementation for test writing/execution
  |-- cf-ops (ON-DEMAND) -- spawned for PR creation and merge
```

**Typical flow:**
1. Lead creates tasks from user request
2. Lead spawns cf-developer (persistent) and assigns first task
3. Developer works through tasks sequentially
4. When implementation complete, lead spawns cf-qa (on-demand) for testing
5. QA writes/runs tests, reports results, shuts down
6. Lead spawns cf-ops (on-demand) for PR creation
7. Ops pushes, creates PR, shuts down
8. Session complete

**Simplified flow (small features):**
1. Lead creates single task
2. Lead spawns cf-developer (persistent)
3. Developer implements, tests, commits
4. Lead handles PR creation directly (or spawns cf-ops)

### 6.2 Bug Fix

```
Team Lead (PERSISTENT)
  |-- cf-developer (PERSISTENT) -- assigned bug fix task
  |-- cf-qa (ON-DEMAND) -- spawned to verify fix and add regression test
```

**Typical flow:**
1. Lead creates task for bug investigation and fix
2. Lead spawns cf-developer
3. Developer investigates, fixes, writes regression test
4. If additional test coverage needed, lead spawns cf-qa
5. Lead handles commit and PR

### 6.3 Planning Session

```
Team Lead (PERSISTENT)
  |-- cf-planner (PERSISTENT*) -- override to persistent for heavy planning
  |-- cf-developer (ON-DEMAND) -- spawned for feasibility checks if needed
```

*Override: cf-planner is promoted to PERSISTENT because planning sessions involve multiple sequential planning tasks.

**Typical flow:**
1. Lead creates planning tasks (break down requirements, create epics)
2. Lead spawns cf-planner (persistent for this session)
3. Planner creates epic structure, task breakdown, estimates
4. If feasibility questions arise, lead spawns cf-developer briefly
5. Planner finalizes plan artifacts

### 6.4 Review Session

```
Team Lead (PERSISTENT)
  |-- cf-reviewer (PERSISTENT*) -- override to persistent for multi-PR review
  |-- cf-developer (ON-DEMAND) -- spawned to fix must-fix issues from review
```

*Override: cf-reviewer is promoted to PERSISTENT for sessions reviewing multiple PRs.

**Typical flow:**
1. Lead creates review tasks (one per PR or per review scope)
2. Lead spawns cf-reviewer (persistent for this session)
3. Reviewer analyzes code, produces findings
4. If must-fix issues found, lead spawns cf-developer to address them
5. Reviewer re-reviews fixes

### 6.5 Research/Exploration

```
Team Lead (PERSISTENT)
  |-- (SUB-AGENT calls only -- no teammates spawned)
```

**Typical flow:**
1. Lead uses Task tool with `subagent_type='Explore'` for each research question
2. Sub-agents return results and terminate
3. Lead synthesizes findings
4. No persistent teammates needed

### 6.6 Full Pipeline (rare, complex projects)

```
Team Lead (PERSISTENT)
  |-- cf-planner (ON-DEMAND) -- phase 1: planning
  |-- cf-developer (PERSISTENT) -- phase 2: implementation
  |-- cf-qa (ON-DEMAND) -- phase 3: testing
  |-- cf-reviewer (ON-DEMAND) -- phase 4: review
  |-- cf-ops (ON-DEMAND) -- phase 5: ship
  |-- cf-documenter (ON-DEMAND) -- phase 6: documentation
```

**Note:** Phases are sequential. Only 1-2 teammates are active at any time. The lead orchestrates the pipeline, spawning each phase's teammate when the previous completes.

---

## 7. Context Management Strategy

### 7.1 Context Budget Recommendations

| Teammate | Estimated Context per Task | Max Tasks Before Recycle | Notes |
|----------|:------------------------:|:------------------------:|-------|
| cf-developer | 15-25% per task | 3-4 tasks | Heavy file reads + edits accumulate fast |
| cf-planner | 20-30% per planning cycle | 2-3 cycles | Epic creation reads many existing docs |
| cf-reviewer | 20-35% per PR | 2-3 PRs | Code reading is context-intensive |
| cf-qa | 15-25% per test cycle | 3-4 cycles | Test writing is moderately context-heavy |
| cf-ops | 10-20% per operation | 4-5 operations | Git/PR operations are relatively light |
| cf-documenter | 15-25% per doc task | 3-4 tasks | Doc writing is moderately context-heavy |

### 7.2 Context Monitoring Mechanism

Since there is no built-in API for teammates to query their own context usage, we use a **hook-based self-reporting** approach:

**PostToolUse Hook (teammate context monitor):**

The hook reads context window data from the hook environment (available via the `context_window` JSON that StatusLine already processes):

```
PostToolUse:
  1. Extract context_window.current_usage from hook input
  2. Calculate usage percentage: (input_tokens + cache_creation) / context_window_size
  3. If usage > warning_threshold (70%):
     - Write warning to shared state file: /tmp/claude/{team}/context-warnings/{teammate}.json
     - The teammate's next response should include a context warning message to lead
  4. If usage > recycle_threshold (85%):
     - Write urgent marker
     - Teammate should finish current subtask and send handoff summary
```

**Important caveat:** Context window data availability in the hook environment for teammates (not just the lead) needs to be verified during implementation. If unavailable, fall back to heuristic monitoring (count tool calls as a proxy for context consumption).

### 7.3 Recycle vs. Continue Decision Matrix

| Condition | Decision | Action |
|-----------|----------|--------|
| Context < 70%, has pending tasks | **Continue** | Assign next task |
| Context 70-85%, current task almost done | **Continue then recycle** | Finish task, send handoff, shutdown |
| Context 70-85%, new large task incoming | **Recycle now** | Send handoff, shutdown, spawn replacement |
| Context > 85% | **Recycle immediately** | Send handoff summary, shutdown, spawn replacement |
| Task complete, no more tasks of this type | **Shutdown** | Clean shutdown, no replacement needed |
| Teammate unresponsive or degraded | **Force recycle** | Shutdown, spawn replacement with last known state |

### 7.4 Handoff Summary Format

When a teammate is recycled, its handoff summary to the lead should include:

```
## Handoff Summary

### Completed Work
- {List of completed tasks and their outcomes}

### Current State
- Branch: {current branch}
- Last commit: {commit hash and message}
- Modified files: {list}

### In-Progress Work
- Task: {current task subject}
- Status: {what's done, what remains}
- Key decisions made: {list}

### Remaining Work
- {What the replacement should pick up}
- {Any known issues or blockers}
```

The lead includes this handoff in the replacement's spawn prompt (Section 4 Constraints):

```
Session constraints:
- You are replacing a previous cf-developer who was recycled due to context limits.
- Handoff summary: {handoff content}
- Continue from where they left off.
```

### 7.5 What Happens When a Teammate Hits Context Limits Mid-Task

**Graceful degradation procedure:**

1. Teammate notices degradation (repeated tool calls, forgetting earlier context)
2. Teammate sends message to lead: "Experiencing context pressure. Requesting recycle."
3. Lead instructs teammate: "Summarize your current state for handoff."
4. Teammate sends handoff summary
5. Lead sends shutdown_request
6. Lead spawns replacement with handoff summary
7. Replacement continues the task

**If teammate becomes unresponsive:**

1. Lead detects no response after reasonable wait
2. Lead sends shutdown_request (may not get response)
3. Lead reconstructs state from:
   - Task description and status
   - Git diff on the working branch
   - Last messages from the teammate
4. Lead spawns replacement with reconstructed context

---

## 8. Migration Path from V3 Skills to Teammate Blueprints

### 8.1 Migration Phases

#### Phase 1: Create Blueprint Files

Create `.claude/agents/cf-{role}.md` for each teammate role, following the blueprint structure defined in Section 2. Source content from:

- V3 agent definitions (role, constraints, branch access)
- Relevant skill SKILL.md files (operations to embed)
- V3 skill enforcement levels (map to quality gates)

**Order of creation:**
1. cf-developer (most critical, used in most sessions)
2. cf-ops (handles git push/PR, needed for shipping)
3. cf-qa (testing, needed after development)
4. cf-planner (planning, needed for epic sessions)
5. cf-reviewer (review, needed for PR review)
6. cf-documenter (documentation, needed for docs)
7. cf-support (minimal, sub-agent prompt)

#### Phase 2: Adapt Hooks for Agent Teams

Existing hooks need adaptation:

| Hook | Adaptation Needed |
|------|-------------------|
| PreToolUse sentinel hooks | Add teammate identity detection (check `CLAUDE_CODE_TEAM_NAME` env var) |
| PostToolUse progress hooks | Add context monitoring for teammates |
| Read delegation hook | Adjust delegation target from `cf-general-purpose` to `Task(Explore)` or `offset/limit` |
| Stop hook (PCV) | Adapt for teammate shutdown vs. session end |
| Session start hooks | Add team initialization (spawn persistent teammates) |

#### Phase 3: Update Settings Templates

Add Agent Teams configuration to all settings templates:

```json
{
  "env": {
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }
}
```

Ensure hook arrays include teammate-aware hooks.

#### Phase 4: Team Lead Configuration

Update CLAUDE.md (or create a separate PathFlow instruction file) with:

- Team composition templates (Section 6)
- Spawn prompt builder patterns (Section 4)
- Context management procedures (Section 7)
- Routing logic (which user commands map to which teammate types)

#### Phase 5: Deprecate V3 Skill Infrastructure

Once teammate blueprints are operational:

1. Remove `cf-general-purpose` agent definition (dissolved)
2. Remove `cf-model-orchestrator` skill (deferred)
3. Archive SKILL.md files (content has been absorbed into blueprints)
4. Update CLAUDE.md to remove skill invocation instructions
5. Update hooks to remove skill sentinel references that are replaced by task status

### 8.2 Compatibility Strategy

During migration, support both modes:

- **V3 mode (no team):** Skills work as before via forked sub-agents. No behavioral change.
- **Agent Teams mode (team active):** Skills are replaced by teammate blueprints. Hooks detect team mode via `CLAUDE_CODE_TEAM_NAME` env var and adjust behavior.

This allows incremental migration without breaking the existing V3 workflow.

### 8.3 Validation Criteria

The migration is complete when:

- [ ] All 7 teammate blueprints exist in `.claude/agents/`
- [ ] Team lead can spawn, assign tasks to, and shut down each teammate type
- [ ] Each teammate correctly reads its blueprint and follows constraints
- [ ] Hook system correctly enforces constraints in team mode
- [ ] Context monitoring detects and reports threshold breaches
- [ ] Teammate recycling produces clean handoffs
- [ ] All V3 session lifecycle events have Agent Teams equivalents
- [ ] A complete feature development session runs end-to-end using teammates

---

## Appendix A: Open Questions

### A1: Delegate Mode Programmatic Access

Delegate mode is interactive-only (Shift+Tab). If Anthropic adds a settings key or env var for delegate mode, the hook-based enforcement becomes redundant. Monitor Claude Code releases for this capability.

### A2: Cross-Session Teammate Persistence

Agent Teams teammates do not persist across sessions. The V3 Knowledge Layer (JSONL + SQLite) provides cross-session memory. PathFlow needs a strategy for cross-session context that does not depend on teammate persistence. Options:
- Continue using JSONL/SQLite for cross-session memory (separate from task system)
- Use MEMORY.md auto-memory for lightweight cross-session context
- Use task metadata for structured cross-session state

### A3: Teammate-Specific Permissions

Currently, teammates inherit the lead's permissions from settings.json. There is no per-teammate permission customization at spawn time. If a teammate should have restricted tool access (e.g., cf-reviewer cannot use Edit), this must be enforced via:
- Blueprint instructions (soft enforcement -- teammate follows instructions)
- PreToolUse hooks that check teammate identity (hard enforcement)

Hook-based enforcement is more reliable but requires the hook to identify which teammate is executing.

### A4: Context Window Data Availability in Teammate Hooks

The feasibility findings note that StatusLine context window data is available to hooks, but this was verified for the lead's hooks only. Whether teammate hooks receive the same `context_window` JSON needs implementation-time verification.

### A5: Maximum Concurrent Teammates

Claude Code documentation does not specify a hard limit on concurrent teammates. The practical limit is determined by API rate limits and total context budget across all active agents. Recommendation: cap at 3 concurrent teammates (lead + 2 active teammates) for initial implementation to manage costs.

---

## Appendix B: Decision Log

| Decision | Choice | Alternatives Considered | Rationale |
|----------|--------|------------------------|-----------|
| Blueprint format | Markdown in `.claude/agents/` | JSON config, YAML, settings.json entries | Markdown is human-readable, matches Claude Code agent definition convention |
| Blueprint loading | Instruct-to-read (Strategy A) | Inline in spawn prompt (Strategy B) | Saves spawn prompt tokens; one Read call is negligible overhead |
| Persistence default | cf-developer persistent, all others on-demand | All persistent, all on-demand | Matches usage patterns; developer has highest sequential task volume |
| Skill dissolution | Embed in blueprints, not centralized | Central skill registry, skill-as-service teammate | Skills are behavioral instructions, not services; embedding avoids indirection |
| Communication model | Hub-and-spoke via team lead | Direct peer-to-peer, shared state database | Matches Agent Teams design; lead mediates all coordination |
| Context monitoring | Hook-based self-reporting | Periodic polling, external monitoring | Only mechanism available; no built-in context API for teammates |
| Security enforcement | Hooks (unchanged from V3) | Blueprint-only soft enforcement | Hooks provide hard enforcement independent of teammate compliance |
| Network operations | cf-ops exclusive | Any teammate with sandbox-check | Centralizes risky operations; easier to audit and control |
