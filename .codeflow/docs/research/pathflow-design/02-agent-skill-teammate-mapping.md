# Agent/Skill to Teammate Mapping Analysis

> Research for PathFlow Design: How V3 agents and skills should map to Claude Agent Teams teammates.

---

## Agent-by-Agent Analysis

### 1. cf-planner

| Property | Value |
|----------|-------|
| **V3 Role** | Feature planning specialist |
| **Branch Access** | Write: `plan/*` only |
| **Memory Domain** | `planning` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-git-workflow, cf-memory-management, cf-task-management, cf-documentation-standards |
| **Decision Authority** | Autonomous (task breakdown, estimates), Confirm (impl approach), Defer (architecture, scope) |
| **Tools** | Read, Write, Edit, Glob, Grep, Bash (git only) |
| **Invocation** | `/cf-plan`, Task tool from main agent |

**Teammate Recommendation: ON-DEMAND TEAMMATE**

Rationale:
- Planning is episodic -- invoked when epics/tasks need creation or refinement
- Heavy context load: needs PROJECT.md, existing epics, requirements, memory domain
- Produces artifacts (epic.md, task files, briefs) that persist via Knowledge Layer
- Not needed between planning sessions; holding it persistent wastes resources
- Estimated context consumption: MEDIUM-HIGH (~30-40% of window per session)
- Clear start/end lifecycle: invoked, produces artifacts, completes

**Blueprint**: Hybrid -- agent .md for role identity/constraints + cf-task-management SKILL.md for operations + cf-memory-management for lifecycle

---

### 2. cf-developer

| Property | Value |
|----------|-------|
| **V3 Role** | Feature implementation specialist |
| **Branch Access** | Write: `feat/*`, `fix/*`, `refactor/*` |
| **Memory Domain** | `development` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-git-workflow, cf-memory-management, cf-code-exploration, cf-script-standards |
| **Decision Authority** | Autonomous (impl details, minor refactoring), Confirm (dependency adds), Defer (architecture, scope) |
| **Tools** | Read, Write, Edit, Bash, Glob, Grep (FULL access) |
| **Invocation** | `/cf-develop`, Task tool from main agent |

**Teammate Recommendation: PERSISTENT TEAMMATE (primary work session)**

Rationale:
- Implementation is the longest-running activity; developer sessions span many tool calls
- Heavy context accumulation: code files, test patterns, implementation decisions
- Benefits from maintaining context across multiple commits within a single feature
- FULL tool access aligns with "general-purpose" teammate type
- Most likely to fill context window -- but this is productive (deep implementation work)
- Estimated context consumption: HIGH (~60-80% of window over a session)

**Blueprint**: Agent .md for role + skill references for operations (git-workflow, code-exploration inlined as needed)

---

### 3. cf-reviewer

| Property | Value |
|----------|-------|
| **V3 Role** | Code review specialist |
| **Branch Access** | Read ONLY (no write) |
| **Memory Domain** | `review` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-code-exploration |
| **Decision Authority** | Autonomous (findings categorization, verdict) |
| **Tools** | Read, Glob, Grep, Bash (read-only) -- NO Write/Edit |
| **Invocation** | `/cf-review`, Task tool from main agent |

**Teammate Recommendation: ON-DEMAND TEAMMATE (read-only type)**

Rationale:
- Reviews are discrete, bounded tasks with clear start/end
- Read-only constraint maps perfectly to "Explore" subagent type
- Low context persistence needed -- review is self-contained per PR
- Produces structured output (findings with file:line refs, verdict)
- Not needed between reviews
- Estimated context consumption: MEDIUM (~20-35% of window)
- Could potentially remain a SUB-AGENT for simple reviews

**Alternative**: For simple PRs, a sub-agent (Task tool with Explore type) suffices. For complex architectural reviews, promote to on-demand teammate.

**Blueprint**: Agent .md for role constraints + cf-code-exploration for navigation

---

### 4. cf-qa

| Property | Value |
|----------|-------|
| **V3 Role** | Test automation specialist |
| **Branch Access** | Write: `test/*`, `feat/*` (test files only) |
| **Memory Domain** | `qa` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-git-workflow, cf-memory-management, cf-testing-workflow, cf-script-standards |
| **Decision Authority** | Autonomous (test file org, scenarios, edge cases), Confirm (coverage exceptions), Defer (new frameworks) |
| **Tools** | Read, Write, Edit, Bash, Glob, Grep |
| **Invocation** | `/cf-test`, Task tool from main agent |

**Teammate Recommendation: ON-DEMAND TEAMMATE**

Rationale:
- Testing is episodic -- invoked after implementation or for coverage improvement
- Needs write access (test files only) -- so not a read-only sub-agent
- Bounded scope: write test files, run tests, report results
- Context does not accumulate as heavily as developer (focused file set)
- Clear lifecycle: create tests, run, report coverage, done
- Estimated context consumption: MEDIUM (~25-40% of window)

**Blueprint**: Agent .md for role + cf-testing-workflow SKILL.md for test operations

---

### 5. cf-ops

| Property | Value |
|----------|-------|
| **V3 Role** | DevOps and deployment specialist |
| **Branch Access** | Write: `ops/*`, `deploy/*` |
| **Memory Domain** | `ops` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-git-workflow, cf-security-management, cf-script-standards |
| **Decision Authority** | Autonomous (CI/CD config, non-prod deploys), Confirm (prod deploy, security changes), Defer (infrastructure) |
| **Tools** | Read, Write, Edit, Bash, Glob, Grep |
| **Invocation** | `/cf-ship`, `/cf-deploy`, Task tool |

**Teammate Recommendation: ON-DEMAND TEAMMATE**

Rationale:
- Deployment/shipping is episodic -- invoked at end of development cycle
- Requires security validation and audit logging -- benefits from dedicated context
- Network operations (gh pr create, git push) require special handling
- Not needed during planning or implementation phases
- Estimated context consumption: MEDIUM (~25-35% of window)

**Blueprint**: Agent .md for role + cf-security-management for protected ops + cf-git-workflow for PR/merge

---

### 6. cf-documenter

| Property | Value |
|----------|-------|
| **V3 Role** | Technical documentation specialist |
| **Branch Access** | Write: `docs/*` only |
| **Memory Domain** | `documentation` (exclusive write) |
| **Skills Used** | cf-working-protocol, cf-git-workflow, cf-memory-management, cf-documentation-standards |
| **Decision Authority** | Autonomous (structure, wording), Confirm (technical accuracy), Defer (architecture representation, new templates) |
| **Tools** | Read, Write, Edit, Glob, Grep, Bash (git only) |
| **Invocation** | `/cf-document`, Task tool |

**Teammate Recommendation: ON-DEMAND TEAMMATE (lightweight)**

Rationale:
- Documentation is episodic -- invoked after implementation or for ADRs
- Narrowly scoped: only documentation files
- Benefits from reading code context but does not modify it
- Context consumption is moderate and bounded
- Estimated context consumption: LOW-MEDIUM (~20-30% of window)

**Alternative**: For simple doc updates (README edits), a sub-agent suffices. For comprehensive ADRs or API documentation, use on-demand teammate.

**Blueprint**: Agent .md for role + cf-documentation-standards for templates

---

### 7. cf-support

| Property | Value |
|----------|-------|
| **V3 Role** | User assistance specialist |
| **Branch Access** | Read ONLY |
| **Memory Domain** | Read-only (no write domain) |
| **Skills Used** | cf-working-protocol, cf-code-exploration |
| **Decision Authority** | Autonomous (answers, references, suggestions) |
| **Tools** | Read, Glob, Grep, Bash (read-only) -- NO Write/Edit |
| **Invocation** | `/cf-help`, Task tool |

**Teammate Recommendation: SUB-AGENT (Explore type)**

Rationale:
- Support is reactive, lightweight, and bounded
- Read-only -- perfect match for Explore sub-agent type
- Does not accumulate state between invocations
- Single question-answer cycle
- Creating a teammate for a help query is excessive overhead
- Estimated context consumption: LOW (~10-20% of window)

**Blueprint**: Minimal -- agent .md role description as the sub-agent prompt + cf-code-exploration patterns

---

### 8. cf-general-purpose

| Property | Value |
|----------|-------|
| **V3 Role** | Delegated skill executor |
| **Branch Access** | Inherited from delegating agent |
| **Memory Domain** | Delegated at runtime |
| **Skills Used** | ALL skills (when delegated) |
| **Decision Authority** | Autonomous within delegated bounds only |
| **Tools** | All tools subject to delegation |
| **Invocation** | Task tool internally (NOT user-facing) |

**Teammate Recommendation: NOT A TEAMMATE -- KEEP AS SUB-AGENT PATTERN**

Rationale:
- This agent IS the sub-agent pattern -- it provides isolated context for forked skill execution
- Stateless between invocations by design
- Creating a teammate defeats the purpose (no state persistence desired)
- In Agent Teams, this maps to `Task tool` calls with appropriate sub-agent type
- The "general-purpose" concept becomes the default sub-agent spawning mechanism

**PathFlow Implication**: In Agent Teams, when a teammate needs to execute a forked skill in isolation, they use `Task tool` with the appropriate sub-agent type. The cf-general-purpose concept dissolves into the native Task tool mechanism.

---

## Skill-by-Skill Analysis

### 1. cf-working-protocol (~1,200 tokens, INLINE)

| Property | Value |
|----------|-------|
| **Context** | INLINE (always loaded) |
| **Token Size** | ~1,200 tokens |
| **Used By** | ALL agents |
| **Operations** | meta-awareness, think-and-act, decide, respond-organized, research-quality, verify-work, parallelize-work |

**Teammate Mapping: EMBEDDED IN ALL TEAMMATES (not a separate teammate)**

Rationale: This is behavioral guidance, not a service. It should be part of every teammate's blueprint as foundational instructions. At ~1,200 tokens, it fits comfortably in every agent definition.

**Blueprint Format**: Include as a section in every teammate's agent .md file. NOT a separate skill invocation.

---

### 2. cf-git-workflow (~1,100 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~1,100 tokens |
| **Used By** | cf-planner, cf-developer, cf-ops, cf-qa, cf-documenter |
| **Operations** | create-branch, create-commit, create-pull-request, create-worktree, cleanup-worktrees, sync-remote, merge-branch, review-changes, check-branch-status, rebase-interactive |
| **Dependencies** | cf-memory-management:complete-work (for commit), cf-security-management:sandbox-check (for network) |

**Teammate Mapping: SKILL LOADED BY TEAMMATES (not a separate teammate)**

Rationale: Git operations are procedures that teammates execute, not an independent agent. Each teammate that does git work should have git-workflow operations described in their blueprint.

**Blueprint Format**: Skill SKILL.md operations embedded in teammate blueprint for those that need it (developer, planner, ops, qa, documenter).

---

### 3. cf-security-management (~1,000 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~1,000 tokens |
| **Used By** | cf-ops (primary), all agents (sandbox-check for network) |
| **Operations** | stage-protected-edit, apply-protected-edit, sandbox-check, sync-settings-templates |

**Teammate Mapping: SKILL LOADED BY TEAMMATES (not a separate teammate)**

Rationale: Security operations are procedural gates, not an independent service. The sandbox-check operation is a prerequisite pattern, and the protected-edit staging is a workflow within other operations.

**Blueprint Format**: Embed security procedures in ops teammate and as pre-conditions for network operations in other teammates.

---

### 4. cf-memory-management (~1,500 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~1,500 tokens (largest skill) |
| **Used By** | ALL agents (every agent uses begin-work, record-progress, complete-work) |
| **Operations** | detect-active-work, load-work-context, search-related-work, evaluate-context, begin-work, record-work-progress, complete-work, manage-memory-lifecycle |
| **Dependencies** | cf-db-operations (for all DB operations) |

**Teammate Mapping: CRITICAL SKILL EMBEDDED IN ALL WRITING TEAMMATES**

Rationale: Memory management is the work lifecycle backbone. Every teammate that modifies state needs these operations. This is too foundational to be a separate teammate -- it's a shared protocol.

**PathFlow Implication**: In Agent Teams, memory management translates to the task system (TaskCreate, TaskUpdate, TaskList). The begin-work / record-progress / complete-work lifecycle maps to Task status transitions. The Knowledge Layer (SQLite + JSONL) becomes the shared task list.

**Blueprint Format**: Embed lifecycle operations (begin-work, record-progress, complete-work) in each writing teammate's blueprint. detect-active-work becomes part of the team lead's session start.

---

### 5. cf-documentation-standards (~600 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~600 tokens |
| **Used By** | cf-documenter (primary), cf-planner (for epic templates) |
| **Operations** | apply-standard, lint-file, validate-structure, check-links, generate-toc |

**Teammate Mapping: SKILL FOR DOCUMENTER TEAMMATE**

Rationale: Lightweight procedural skill. Embed in documenter teammate's blueprint.

**Blueprint Format**: Include in documenter and planner teammate blueprints.

---

### 6. cf-code-exploration (~500 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~500 tokens |
| **Used By** | cf-developer, cf-reviewer, cf-support, cf-qa |
| **Operations** | select-search-strategy, load-compressed-context, search-symbol, analyze-dependencies, navigate-to-definition, find-all-references |

**Teammate Mapping: SKILL FOR READING/EXPLORING TEAMMATES**

Rationale: Navigation patterns that any code-reading teammate benefits from. Small token footprint.

**Blueprint Format**: Embed in developer, reviewer, and support agent blueprints.

---

### 7. cf-script-standards (~800 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~800 tokens |
| **Used By** | cf-developer, cf-ops, cf-qa |
| **Operations** | lint-shell, lint-python, apply-shell-standards, apply-python-standards, ensure-test-coverage |

**Teammate Mapping: SKILL FOR WRITING TEAMMATES**

Rationale: Quality enforcement for script files. Include in any teammate that writes shell/Python.

**Blueprint Format**: Embed in developer and ops teammate blueprints.

---

### 8. cf-task-management (~800 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~800 tokens |
| **Used By** | cf-planner (primary), main agent (informal tasks) |
| **Operations** | understand-request, classify-work, ensure-work-registered, create-epic, update-epic, create-task, update-task, query-tasks |
| **Dependencies** | cf-db-operations (for all CRUD) |

**Teammate Mapping: REPLACES WITH AGENT TEAMS TASK SYSTEM**

Rationale: Agent Teams has a native task system (TaskCreate, TaskUpdate, TaskList, TaskGet). The V3 cf-task-management skill performs the same function with custom SQLite tables. In PathFlow, the Agent Teams task list IS the task management system.

**PathFlow Implication**: The team lead's orchestration role replaces the main agent's task routing. TaskCreate replaces create-epic + create-task. TaskUpdate replaces update-task. TaskList replaces query-tasks. classify-work becomes the team lead's internal decision-making.

**Blueprint Format**: NOT embedded as a skill. Instead, the team lead uses native Task tools.

---

### 9. cf-testing-workflow (~700 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~700 tokens |
| **Used By** | cf-qa (primary), cf-developer (run-tests) |
| **Operations** | run-tests, check-coverage, analyze-failures, suggest-tests, run-affected-tests |

**Teammate Mapping: SKILL FOR QA TEAMMATE**

Rationale: Testing procedures are QA-centric. Embed in QA teammate and optionally in developer teammate.

**Blueprint Format**: Embed in QA teammate blueprint. Developer teammate gets run-tests and run-affected-tests.

---

### 10. cf-model-orchestrator (~1,800 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~1,800 tokens (second largest) |
| **Used By** | Main agent (advanced use cases) |
| **Operations** | verify-prerequisites, construct-prompt, execute-with-model, spawn-session, send-task, poll-output, orchestrate-parallel, terminate-session, check-scope-conflict |

**Teammate Mapping: DEFERRED (v2+ feature)**

Rationale: This skill manages external AI model delegation via tmux sessions. In Agent Teams, multi-model orchestration is an advanced capability beyond initial PathFlow scope. The spec itself notes "consider deferring for CodeFlow v1."

**Blueprint Format**: Not included in initial PathFlow design.

---

### 11. cf-db-operations (~600 tokens, FORKED)

| Property | Value |
|----------|-------|
| **Context** | FORKED via cf-general-purpose |
| **Token Size** | ~600 tokens |
| **Used By** | ALL skills that persist data (cf-memory-management, cf-task-management, cf-security-management) |
| **Operations** | epic-create, epic-update, task-create, task-update, memory-store, memory-query, session-record, log-append |

**Teammate Mapping: PARTIALLY REPLACED BY AGENT TEAMS TASK SYSTEM**

Rationale: Epic/task CRUD maps to Agent Teams TaskCreate/TaskUpdate. Memory-store and memory-query map to the team's shared state (task descriptions and metadata). Session-record becomes automatic. The SQLite + JSONL three-tier model is a V3 custom solution; Agent Teams provides native task coordination.

**PathFlow Implication**: Custom SQLite DB may still be needed for memory events and detailed work tracking beyond what TaskCreate/TaskUpdate supports. But epic/task CRUD is fully replaced.

**Blueprint Format**: Reduced scope -- only memory operations if needed. Task CRUD replaced by native tools.

---

## Persistence Configuration Matrix

| Agent | Teammate Type | Persistence | Spawn Trigger | Shutdown Trigger |
|-------|---------------|-------------|---------------|------------------|
| Main Agent | **Team Lead** | PERSISTENT | Session start | Session end |
| cf-developer | **Teammate** | PERSISTENT (session) | Feature work begins | Feature complete / PR created |
| cf-planner | **Teammate** | ON-DEMAND | `/cf-plan` or planning needed | Epic/tasks created |
| cf-reviewer | **Teammate / Sub-agent** | ON-DEMAND | PR review needed | Review verdict given |
| cf-qa | **Teammate** | ON-DEMAND | Testing needed | Tests pass + coverage reported |
| cf-ops | **Teammate** | ON-DEMAND | Ship/deploy needed | PR merged / deployed |
| cf-documenter | **Teammate / Sub-agent** | ON-DEMAND | Docs needed | Documentation committed |
| cf-support | **Sub-agent** | TRANSIENT | Help question asked | Answer provided |
| cf-general-purpose | **N/A** | N/A | Dissolved into Task tool | N/A |

---

## Cross-Teammate Communication Map

```text
                    ┌──────────────┐
                    │  TEAM LEAD   │
                    │ (Orchestrator)│
                    └──────┬───────┘
                           │
           ┌───────────────┼───────────────┐
           │               │               │
    ┌──────▼──────┐ ┌──────▼──────┐ ┌──────▼──────┐
    │  cf-planner │ │cf-developer │ │   cf-ops    │
    └──────┬──────┘ └──────┬──────┘ └──────┬──────┘
           │               │               │
           │    ┌──────────▼──────────┐    │
           │    │ Shared Task List    │    │
           │    │ (Agent Teams native)│    │
           │    └──────────┬──────────┘    │
           │               │               │
    ┌──────▼──────┐ ┌──────▼──────┐ ┌──────▼──────┐
    │cf-documenter│ │  cf-qa      │ │ cf-reviewer  │
    └─────────────┘ └─────────────┘ └─────────────┘
```

### Communication Flows

| From | To | Information | Mechanism |
|------|----|-------------|-----------|
| Team Lead | cf-planner | User requirements, plan request | SendMessage |
| cf-planner | Team Lead | Epic/tasks created, ready for dev | SendMessage + TaskUpdate |
| Team Lead | cf-developer | Task assignment, implementation plan | TaskUpdate (owner) + SendMessage |
| cf-developer | Team Lead | Implementation complete, PR ready | SendMessage + TaskUpdate (completed) |
| Team Lead | cf-reviewer | Review request with PR reference | SendMessage |
| cf-reviewer | Team Lead | Review verdict, findings | SendMessage |
| Team Lead | cf-qa | Test request with scope | SendMessage |
| cf-qa | Team Lead | Test results, coverage | SendMessage + TaskUpdate |
| Team Lead | cf-ops | Ship/deploy request | SendMessage |
| cf-ops | Team Lead | Deployment status, PR URL | SendMessage + TaskUpdate |
| Team Lead | cf-documenter | Documentation request | SendMessage |
| cf-documenter | Team Lead | Docs committed | SendMessage + TaskUpdate |

### Peer-to-Peer Communication (via Team Lead mediation)

| Scenario | Flow |
|----------|------|
| Developer needs plan clarification | developer -> team lead -> planner |
| Reviewer finds must-fix issues | reviewer -> team lead -> developer |
| QA finds source bug | qa -> team lead -> developer |
| Ops needs test verification | ops -> team lead -> qa |

**Key Finding**: V3 agents communicate via Knowledge Layer (DB + files). In Agent Teams, communication is via SendMessage + Task system. The team lead mediates all cross-agent communication, replacing the V3 main agent's orchestration role.

---

## Blueprint Format Recommendation

### Recommended Approach: Agent .md as Blueprint

Each teammate's `.claude/agents/cf-{role}.md` file should serve as the blueprint, with the following structure:

```markdown
---
name: "cf-{role}"
description: "{Role description}. {When to invoke}."
---

# Section 1: Identity & Constraints
- Role, responsibilities, DO/DON'T boundaries
- Branch access permissions
- Memory domain (if applicable)

# Section 2: Operations (from relevant skills)
- Embedded operations from skills this agent uses
- Decision tree for operation selection
- Cross-skill prerequisites

# Section 3: Communication Protocol
- How to report results to team lead
- What information to include in messages
- When to use TaskUpdate vs SendMessage

# Section 4: Quality Gates
- Verification requirements before completion
- Escalation triggers
```

### Token Budget per Blueprint

| Agent Blueprint | Estimated Tokens | Components |
|----------------|------------------|------------|
| Team Lead (CLAUDE.md) | ~2,500 | Working protocol + routing + session mgmt |
| cf-developer | ~2,000 | Role + git-workflow + code-exploration + script-standards |
| cf-planner | ~1,500 | Role + task-management ops + doc-standards |
| cf-reviewer | ~1,000 | Role + code-exploration (read-only) |
| cf-qa | ~1,500 | Role + testing-workflow + git-workflow (commit) |
| cf-ops | ~1,500 | Role + security-management + git-workflow (PR/merge) |
| cf-documenter | ~1,200 | Role + doc-standards + git-workflow (commit) |
| cf-support | ~600 | Role + code-exploration (minimal) |

---

## Key Findings for PathFlow Design

### 1. Agent Teams Naturally Replaces V3 Orchestration

The V3 "main agent constrained to orchestration" + "sub-agents via Task tool" maps directly to Agent Teams' "team lead + teammates" model. The team lead IS the orchestrator. No adaptation needed.

### 2. Most Agents are ON-DEMAND, Not Persistent

Only the team lead (persistent) and cf-developer (persistent during feature work) need to stay alive. All others should be spawned when needed and shut down after completing their task. This is a key difference from V3 where agents were conceptual -- in Agent Teams they consume real resources.

### 3. Skills Dissolve into Teammate Blueprints

V3 skills are forked via cf-general-purpose into isolated contexts. In Agent Teams, there is no need for this isolation pattern -- each teammate has its own context window. Skills should be embedded directly into teammate blueprints as operational procedures.

### 4. cf-task-management is Replaced by Native Task Tools

Agent Teams provides TaskCreate, TaskUpdate, TaskList, TaskGet natively. The V3 epic/task/work-graph system with SQLite tables is redundant for coordination purposes. The team lead uses native task tools for work management.

### 5. cf-memory-management Partially Survives

Work lifecycle tracking (begin-work, record-progress, complete-work) has no native Agent Teams equivalent. If cross-session persistence is needed, the JSONL + SQLite model may still be valuable. But for within-session coordination, Task metadata suffices.

### 6. cf-general-purpose Dissolves Completely

The "stateless forked skill executor" concept is replaced by the Task tool's native sub-agent spawning. When a teammate needs isolated execution, they use Task tool directly.

### 7. Communication Model Shifts Significantly

V3 uses Knowledge Layer (DB reads/writes) for inter-agent communication. Agent Teams uses SendMessage (direct) and Task system (indirect). This is a fundamental architectural shift -- from shared state to message passing.

### 8. Branch Access Enforcement Needs Hook Equivalent

V3 enforces branch access per agent via PreToolUse hooks. In Agent Teams, each teammate's agent .md blueprint must declare branch restrictions, and the git hooks must validate against the teammate's identity.

### 9. cf-reviewer and cf-support Should Use Read-Only Sub-Agent Type

Both are read-only. Agent Teams' Explore sub-agent type (read-only tools) is a perfect match. These may not need full teammate status.

### 10. cf-model-orchestrator is Deferred

Multi-model delegation via tmux is an advanced feature. Agent Teams' native multi-agent coordination (which already runs multiple Claude instances) partially addresses this. Defer to v2+.
