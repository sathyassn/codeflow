# Part 3: Revised Teammate Model

> Function teammates (persistent SOPs) + role teammates (on-demand) + agent definition format.

---

## Table of Contents

- [3.1 Complete Teammate Roster](#31-complete-teammate-roster)
- [3.2 Agent Definition Format](#32-agent-definition-format)
- [3.3 Skills Coexistence](#33-skills-coexistence)

---

## 3.1 Complete Teammate Roster

| Teammate | Category | Persistence | Source Skills | Purpose |
|----------|----------|:-----------:|:-------------|---------|
| **Team Lead** | Orchestrator | PERSISTENT | cf-working-protocol (embedded) | PathFlow management, task assignment, decisions |
| **cf-gitops** | Function | PERSISTENT | cf-git-workflow | All git operations following conventions |
| **cf-knowledge** | Function | PERSISTENT | cf-memory-management, cf-task-management, cf-db-operations | WorkGraph CRUD, memory ops, session tracking |
| **cf-developer** | Role | ON-DEMAND | (code exploration tips, script standards) | Implementation work |
| **cf-planner** | Role | ON-DEMAND | cf-documentation-standards (for plan docs) | Planning, epic/task breakdown |
| **cf-reviewer** | Role | ON-DEMAND | (code exploration tips) | Code review |
| **cf-qa** | Role | ON-DEMAND | cf-testing-workflow, cf-script-standards (for test quality) | Test writing and execution |
| **cf-documenter** | Role | ON-DEMAND | cf-documentation-standards | Technical documentation |
| **cf-ops** | Role | ON-DEMAND | (deployment procedures) | DevOps, CI/CD, deployment |

**cf-ops IS for DevOps** (not git operations). cf-ops handles deployment pipelines, CI/CD configuration, infrastructure. Git operations (branching, commits, PRs) are cf-gitops territory.

### Function Teammates: Why These Two?

**cf-gitops**: Git operations are procedural, convention-heavy, and happen repeatedly throughout a session. Loading the SOP once and reusing it for every branch, commit, and PR is efficient. The teammate accumulates useful state: what branches were created, what's been pushed, what PRs are open.

**cf-knowledge**: The Knowledge Layer (DB, JSONL, WorkGraph) is accessed throughout the entire session for work tracking, progress recording, and context loading. Having a single persistent interface prevents every teammate from needing to understand DB operations.

### Why NOT Other Function Teammates?

| Candidate | Verdict | Reasoning |
|-----------|---------|-----------|
| cf-security | No | Security enforcement is in hooks (automatic). No need for a dedicated teammate. |
| cf-doc-standards | No | Quality standards are embedded in relevant role teammates' blueprints. |
| cf-script-standards | No | Same as above — embedded in cf-developer and cf-qa blueprints. |
| cf-model-orchestrator | Maybe (future) | External model delegation could be a persistent function if used heavily. Defer to post-MVP. |

---

## 3.2 Agent Definition Format

Agent definitions live at `.claude/agents/cf-*.md`. They are loaded by teammates at spawn time via "Read your agent definition at `.claude/agents/cf-{role}.md`".

**Structure**:
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
This is the core SOP content — what to do and how to do it.

## Communication
Who to message for what. Direct peer communication patterns.
- cf-gitops for any git operations
- cf-knowledge for any WorkGraph/memory updates
- Lead for escalations and decisions

## Quality Checklist
What to verify before marking work complete.
```

### Function Teammate Example (cf-gitops)

```markdown
---
name: cf-gitops
description: Persistent function teammate for all git operations.
  Spawned at session start. Handles branching, commits, PRs, sync.
---

# cf-gitops

## Identity
You are cf-gitops, the git operations specialist for this team.
You follow CodeFlow's git-workflow SOPs exactly.

## Constraints
- Branch naming: {area}/{project-id}-{description} (e.g., feat/frt-auth-validation)
- Commit format: type(scope): description
- Never force-push to main/master
- All changes through PRs

## Standard Operating Procedures
[Full cf-git-workflow operations embedded here]
- create-branch
- create-commit
- sync-remote
- create-pull-request
- review-changes
- ...

## Communication
- Receive commit requests from: cf-developer, cf-qa, cf-documenter
- Receive PR requests from: Lead, cf-ops
- Report to Lead: branch created, PR status, sync status
- Update cf-knowledge: after significant git events

## Quality Checklist
- [ ] Branch naming follows convention
- [ ] Commit message follows format
- [ ] No sensitive files in commits
- [ ] PR has description and test plan
```

### Role Teammate Example (cf-developer)

```markdown
---
name: cf-developer
description: On-demand role teammate for implementation work.
  Spawned per development task. Shut down after task completion.
---

# cf-developer

## Identity
You are cf-developer, implementing a specific task for this team.
You write code, self-test, and hand off to cf-gitops for commits.

## Constraints
- Only modify files relevant to your assigned task
- Follow script standards (ShellCheck for .sh, ruff for .py)
- Do NOT commit directly — message cf-gitops

## Standard Operating Procedures
- Read and understand the task requirements
- Explore relevant code (use Glob, Grep, Read)
- Implement the solution
- Self-test your changes
- Message cf-gitops to commit

## Communication
- cf-gitops: "Please commit: {type}({scope}): {description}"
- cf-knowledge: "Dev stage complete for {task-id}"
- Lead: "Task #{n} complete. {summary}" or "Blocked: {reason}"
- cf-reviewer: Direct response to review feedback (when in rework loop)

## Quality Checklist
- [ ] Implementation matches task requirements
- [ ] No linting errors
- [ ] Self-tested for basic functionality
- [ ] Changes committed via cf-gitops
```

---

## 3.3 Skills Coexistence

Skills remain in `.claude/skills/`. They serve as:
1. **Canonical SOPs** — the authoritative source for operational procedures
2. **Non-team-mode invocation** — when running without Agent Teams, skills work as before
3. **Content source** — agent .md files embed/reference skill operations
4. **User-invokable commands** — `/cf-commit`, `/cf-plan` etc. still work

### Routing in Team Mode vs Non-Team Mode

| Action | Team Mode | Non-Team Mode |
|--------|-----------|---------------|
| `/cf-commit` | Lead tells cf-gitops to commit | Fork to cf-general-purpose with cf-git-workflow skill |
| `/cf-plan` | Lead assigns to cf-planner | Fork to cf-general-purpose with cf-task-management skill |
| `/cf-develop` | Lead assigns to cf-developer | Fork to cf-general-purpose, inline execution |
| `/cf-review` | Lead assigns to cf-reviewer | Fork to cf-general-purpose with review context |

### Skill-to-Agent Mapping

| Skill | Agent Definition | Relationship |
|-------|-----------------|-------------|
| cf-git-workflow | cf-gitops.md | Full SOP embedded in agent def |
| cf-memory-management | cf-knowledge.md | SOP embedded (part of composite) |
| cf-task-management | cf-knowledge.md | SOP embedded (part of composite) |
| cf-db-operations | cf-knowledge.md | SOP embedded (part of composite) |
| cf-documentation-standards | cf-planner.md, cf-documenter.md | Referenced in quality checklist |
| cf-script-standards | cf-developer.md, cf-qa.md | Referenced in quality checklist |
| cf-working-protocol | Lead instructions | Embedded in CLAUDE.md / spawn prompt |
| cf-security-management | Hook system | Not an agent — hooks handle this |
| cf-code-exploration | cf-developer.md, cf-reviewer.md | Tips referenced |
| cf-model-orchestrator | (future, maybe) | Deferred |
