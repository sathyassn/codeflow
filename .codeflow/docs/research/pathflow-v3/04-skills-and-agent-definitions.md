# 04: Skills and Agent Definitions

> The three-pattern skill model (Load at Spawn, Reference on Demand, Behavioral / Cross-Cutting), agent definition format, embed criteria, decision trees, and skill-to-agent mapping.

---

## Table of Contents

- [4.1 Skills Are Not Replaced](#41-skills-are-not-replaced)
- [4.2 The Three-Pattern Skill Model](#42-the-three-pattern-skill-model)
- [4.3 Embed Criteria](#43-embed-criteria)
- [4.4 Agent Definition Format](#44-agent-definition-format)
- [4.5 Decision Tree Pattern for Tech-Stack Selection](#45-decision-tree-pattern-for-tech-stack-selection)
- [4.6 Skill-to-Agent Mapping](#46-skill-to-agent-mapping)
- [4.7 cf-working-protocol Trimming](#47-cf-working-protocol-trimming)
- [4.8 Routing: Agent-Teams Mode vs Standalone Mode](#48-routing-agent-teams-mode-vs-standalone-mode)
- [4.9 Complete Skill Disposition Table](#49-complete-skill-disposition-table)

---

## 4.1 Skills Are Not Replaced

Skills remain in `.claude/skills/`. They are preserved as:

1. **Canonical SOPs** -- the authoritative source for operational procedures
2. **Non-agent-teams-mode invocation** -- when running without Agent Teams, skills work as before (forked to cf-general-purpose sub-agent)
3. **Content source** -- agent definition files embed or reference skill operations
4. **User-invokable commands** -- `/cf-commit`, `/cf-plan`, etc. still work via the Skill tool

Agent definitions (`.claude/agents/cf-*.md`) are a NEW artifact that sits alongside skills. In agent-teams mode, the agent definition is what a teammate actually loads. In standalone mode, the skill is invoked normally.

---

## 4.2 The Three-Pattern Skill Model

Skills relate to agent definitions in three ways, depending on how tightly the skill is coupled to a specific teammate:

```
+-------------------------------------------------------------------+
|                 THREE-PATTERN SKILL MODEL                          |
+-------------------------------------------------------------------+
|                                                                     |
|  LOAD AT SPAWN           REFERENCE ON DEMAND    BEHAVIORAL /       |
|  (Full SOP read at       (Decision tree reads   CROSS-CUTTING      |
|   spawn, kept for        skill sections when    (Lead or hook       |
|   entire session)        triggered)             system, not in      |
|                                                 any single agent)   |
|  +------------------+   +--------------------+   +--------------+  |
|  | cf-git-workflow   |   | cf-script-         |   | cf-working-  |  |
|  |  -> cf-gitops     |   |   standards        |   |   protocol   |  |
|  |                   |   |  -> cf-developer   |   |  -> team     |  |
|  | cf-memory-mgmt    |   |  -> cf-qa          |   |    lead      |  |
|  | cf-task-mgmt      |   |  -> cf-reviewer    |   |              |  |
|  | cf-db-operations   |   |                    |   | cf-security- |  |
|  |  -> cf-knowledge-  |   | cf-documentation-  |   |   management |  |
|  |     layer          |   |   standards        |   |  -> hooks    |  |
|  |                   |   |  -> cf-planner     |   |    (not an   |  |
|  +------------------+   |  -> cf-documenter   |   |     agent)   |  |
|                         |                    |   +--------------+  |
|  The agent definition   | cf-code-exploration |                    |
|  contains the full SOP. |  -> cf-developer   |   Used by the lead |
|  Read once at spawn,    |  -> cf-reviewer    |   or hook system.   |
|  no further skill       +--------------------+   Not embedded in   |
|  invocation needed.                              any single agent.  |
|                         Agent def has a                             |
|                         decision tree with                          |
|                         slots that trigger                          |
|                         skill section reads.                        |
|                                                                     |
+-------------------------------------------------------------------+
```

### Load at Spawn (formerly Tier 1: Embed)

The agent definition file contains the full SOP content. The teammate reads it once at spawn and has it for the entire session. No further skill invocation is needed.

- The agent definition contains all operations, decision trees, and enforcement rules from the skill
- The teammate does not need to invoke the skill separately -- it already has the SOPs
- Used for function teammates whose entire purpose is executing the skill's operations
- **Mechanism**: Agent def = full SOP. Teammate reads at spawn, retains for session lifetime.

### Reference on Demand (formerly Tier 2: Reference)

The agent definition contains a decision tree with slots. When a slot is triggered (e.g., "IF shell script -> Apply cf-script-standards shell section"), the teammate reads that skill section at that point. This keeps agent definitions compact while covering multiple technology stacks.

- The agent definition includes a decision tree that identifies WHEN to consult each skill section
- The teammate reads the relevant skill section only when the decision tree triggers it
- Used for role teammates that work across multiple tech stacks or quality domains
- **Mechanism**: Agent def = decision tree with reference slots. Skill sections read on demand when triggered.

### Behavioral / Cross-Cutting (formerly Tier 3: Standalone)

The skill is not owned by any single teammate. It is used by the team lead or the hook system, not embedded in any agent definition.

- cf-working-protocol is behavioral guidance for the lead -- cognitive procedures (think-and-act, decide) that shape how the lead reasons
- cf-security-management is enforced through hooks, not through a teammate -- it reacts to tool use events
- These skills may be invoked directly but are not embedded in agent definitions
- **Mechanism**: Not in any agent def. Invoked by lead behavior or hook system reactively.

---

## 4.3 Embed Criteria

A skill should use Load at Spawn in an agent definition when ALL FOUR of these conditions are met:

| # | Condition | Rationale |
|---|-----------|-----------|
| 1 | The skill defines **complete operations** (not just guidelines) | Load at Spawn gives the teammate a full procedural playbook |
| 2 | The operations are the teammate's **primary responsibility** | The teammate exists to execute this skill |
| 3 | The operations are invoked **multiple times per session** | Loading at spawn avoids repeated skill reading |
| 4 | The operations require **accumulated context** from the session | The teammate's history informs better execution |

Applying the criteria:

| Skill | Complete Ops? | Primary Responsibility? | Multiple Times? | Needs Accumulation? | Pattern |
|-------|:------------:|:----------------------:|:--------------:|:-------------------:|:----:|
| cf-git-workflow | Yes | Yes (cf-gitops) | Yes | Yes (branch state) | **Load at Spawn** |
| cf-memory-management | Yes | Yes (cf-knowledge-layer) | Yes | Yes (work state) | **Load at Spawn** |
| cf-task-management | Yes | Yes (cf-knowledge-layer) | Yes | Yes (task state) | **Load at Spawn** |
| cf-db-operations | Yes | Yes (cf-knowledge-layer) | Yes | Yes (DB context) | **Load at Spawn** |
| cf-script-standards | No (guidelines) | No (quality check) | Sometimes | No | **Reference on Demand** |
| cf-documentation-standards | No (guidelines) | No (quality check) | Sometimes | No | **Reference on Demand** |
| cf-code-exploration | No (tips) | No (technique) | Sometimes | No | **Reference on Demand** |
| cf-working-protocol | Yes | No (lead's tool) | Yes | Partially | **Behavioral / Cross-Cutting** |
| cf-security-management | Yes | No (hooks handle it) | Yes | No | **Behavioral / Cross-Cutting** |

---

## 4.4 Agent Definition Format

Agent definitions live at `.claude/agents/cf-*.md`. They are loaded by teammates at spawn time via the instruction: "Read your agent definition at `.claude/agents/{type}.md`".

### Structure

```markdown
---
name: cf-{role}
description: {What this teammate does}. {When the lead should spawn it}.
---

# {Role Name}

## Identity
Who you are, what team you're on, how to communicate.

## Constraints
Branch access, file scope, tool restrictions, memory domain.

## Standard Operating Procedures
The actual operations/procedures from the relevant skill(s).
This is the core SOP content -- what to do and how to do it.

## Communication
Who to message for what. Direct peer communication patterns.
- cf-gitops for any git operations
- cf-knowledge-layer for any WorkGraph/memory updates
- Lead for escalations and decisions

## Quality Checklist
What to verify before marking work complete.
```

### Function Teammate Example (cf-gitops)

Key sections of the agent definition:

```
Identity: You are cf-gitops, the git operations specialist.
Constraints: Branch naming conventions, commit format, never force-push to main.
SOPs: Full cf-git-workflow operations (create-branch, create-commit, sync-remote,
      create-pull-request, review-changes, ...).
Communication:
  Receive from: cf-developer, cf-qa, cf-documenter (commit requests)
  Receive from: Lead, cf-ops (PR requests)
  Report to: Lead (branch/PR status)
  Update: cf-knowledge-layer (after significant git events)
Quality: Branch naming follows convention, commit format correct, no sensitive
         files in commits, PR has description and test plan.
```

### Role Teammate Example (cf-developer)

Key sections of the agent definition:

```
Identity: You are cf-developer, implementing a specific task.
Constraints: Only modify files relevant to assigned task, follow script standards,
             do NOT commit directly -- message cf-gitops.
SOPs: Read task requirements, explore code, implement, self-test,
      message cf-gitops to commit.
Communication:
  cf-gitops: "Please commit: {type}({scope}): {description}"
  cf-knowledge-layer: "Dev stage complete for {task-id}"
  Lead: "Task #{n} complete. {summary}" or "Blocked: {reason}"
  cf-reviewer: Direct response to review feedback (rework loops)
Quality: Implementation matches requirements, no linting errors, self-tested,
         changes committed via cf-gitops.
```

---

## 4.5 Decision Tree Pattern for Tech-Stack Selection

Role teammates (especially cf-developer) need to select the right tools, standards, and patterns based on the project's technology stack. Rather than embedding all possible tech-stack knowledge, agent definitions use a **decision tree with reference slots**.

```
cf-developer Agent Definition
  |
  +-- SOPs: General development process
  |   (read task, explore code, implement, test, commit)
  |
  +-- Decision Tree: Language/Framework Standards
      |
      +-- IF shell script (.sh):
      |   Apply: cf-script-standards (shell section)
      |   Linter: ShellCheck
      |   Style: POSIX-compatible, set -euo pipefail
      |
      +-- IF Python (.py):
      |   Apply: cf-script-standards (python section)
      |   Linter: ruff
      |   Style: PEP 8, type hints
      |
      +-- IF TypeScript (.ts/.tsx):
      |   Apply: (project-specific standards)
      |   Linter: ESLint
      |   Style: Project tsconfig
      |
      +-- IF Markdown (.md):
          Apply: cf-documentation-standards
          Style: CommonMark, TOC at top
```

This pattern keeps the agent definition compact while covering multiple technology stacks. The decision tree REFERENCES the relevant skill sections rather than embedding them entirely.

> **Note**: cf-reviewer also uses this same decision tree pattern when reviewing code for standards compliance. The reviewer's decision tree triggers the appropriate cf-script-standards section based on the file type being reviewed, ensuring consistent standards enforcement across both development and review.

---

## 4.6 Skill-to-Agent Mapping

Complete mapping from existing skills to agent definitions:

| Skill | Agent Definition | Pattern | Relationship |
|-------|-----------------|:----:|-------------|
| cf-git-workflow | cf-gitops.md | Load at Spawn | Full SOP embedded in agent definition |
| cf-memory-management | cf-knowledge-layer.md | Load at Spawn | SOP embedded (part of composite) |
| cf-task-management | cf-knowledge-layer.md | Load at Spawn | SOP embedded (part of composite) |
| cf-db-operations | cf-knowledge-layer.md | Load at Spawn | SOP embedded (part of composite) |
| cf-script-standards | cf-developer.md, cf-qa.md, cf-reviewer.md | Reference on Demand | Quality checklist, decision tree slot |
| cf-documentation-standards | cf-planner.md, cf-documenter.md | Reference on Demand | Quality checklist, decision tree slot |
| cf-code-exploration | cf-developer.md, cf-reviewer.md | Reference on Demand | Technique tips |
| cf-working-protocol | Lead (CLAUDE.md) | Behavioral / Cross-Cutting | Behavioral guidance for lead |
| cf-security-management | Hook system | Behavioral / Cross-Cutting | Not an agent -- hooks handle security |
| cf-model-orchestrator | (future, deferred) | TBD | May become function teammate post-MVP |
| cf-testing-workflow | cf-qa.md | Reference on Demand | Testing procedures and patterns |

### Composite Agent Definitions

cf-knowledge-layer is a **composite** agent that embeds three skills:

```
cf-knowledge-layer.md
  |
  +-- cf-memory-management SOPs
  |   (record-progress, record-decision, begin-work, complete-work)
  |
  +-- cf-task-management SOPs
  |   (classify-work, ensure-work-registered, create-epic, create-task,
  |    update-task, query-tasks)
  |
  +-- cf-db-operations SOPs
      (query, insert, update, schema validation, migration)
```

This composite design is justified because all three skills operate on the same domain (the Knowledge Layer) and are always needed together.

---

## 4.7 cf-working-protocol Trimming

cf-working-protocol is the team lead's cognitive framework. In the PathFlow context, some of its operations are redundant with PathFlow's built-in mechanisms:

| Operation | Keep/Drop | Rationale |
|-----------|:---------:|-----------|
| think-and-act | **Keep** | Core cognitive procedure for the lead's decision-making |
| decide | **Keep** | Structured decision-making for routing, escalation, trade-offs |
| verify-work | **Drop** | Replaced by PF-5 (Work Verification) and Stop hooks |
| resources | **Drop** | Agent definitions and skill references handle resource loading |

The trimmed cf-working-protocol focuses on cognitive procedures only: how the lead thinks through problems and makes decisions. Operational concerns (verification, resource loading) are handled by PathFlow mechanisms and specific teammates.

---

## 4.8 Routing: Agent-Teams Mode vs Standalone Mode

Skills and agent definitions coexist. The routing depends on whether a team is active:

| User Action | Agent-Teams Mode | Standalone Mode |
|------------|-----------|---------------|
| `/cf-commit` | Lead tells cf-gitops to commit | Fork to cf-general-purpose with cf-git-workflow skill |
| `/cf-plan` | Lead assigns to cf-planner | Fork to cf-general-purpose with cf-task-management skill |
| `/cf-develop` | Lead assigns to cf-developer | Fork to cf-general-purpose, inline execution |
| `/cf-review` | Lead assigns to cf-reviewer | Fork to cf-general-purpose with review context |
| `/cf-test` | Lead assigns to cf-qa | Fork to cf-general-purpose with testing skill |

In agent-teams mode, slash commands become orchestration directives: the lead interprets the command and routes it to the appropriate teammate. In standalone mode, the existing skill invocation via cf-general-purpose sub-agent continues to work.

This dual-mode routing ensures backward compatibility. Nothing breaks for users who do not enable Agent Teams.

---

## 4.9 Complete Skill Disposition Table

Final disposition of every skill, showing what happens to each skill file, which agent(s) consume it, and which pattern applies:

| Skill | Disposition | Used By | Pattern |
|-------|-----------|---------|---------|
| cf-memory-management | Stays as skill file | cf-knowledge-layer | Load at Spawn |
| cf-task-management | Stays as skill file | cf-knowledge-layer | Load at Spawn |
| cf-db-operations | Stays as skill file | cf-knowledge-layer | Load at Spawn |
| cf-git-workflow | Stays as skill file | cf-gitops | Load at Spawn |
| cf-script-standards | Split into shell + python | cf-developer, cf-qa, cf-reviewer | Reference on Demand |
| cf-documentation-standards | Stays as skill file | cf-documenter, cf-planner | Reference on Demand |
| cf-code-exploration | Stays as skill file | cf-developer, cf-reviewer | Reference on Demand |
| cf-working-protocol | Stays, trimmed | Lead, any teammate | Behavioral / Cross-Cutting |
| cf-security-management | Stays as skill file | Hook system, any teammate (reactive) | Behavioral / Cross-Cutting |
| cf-model-orchestrator | Stays as skill file | Deferred (future) | TBD |
| cf-testing-workflow | Stays as skill file | cf-qa | Reference on Demand |
