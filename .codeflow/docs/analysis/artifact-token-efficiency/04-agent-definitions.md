# Agent Definition Restructuring Plan

## Strategy: Function Agents vs Role Agents

| Agent Type | Agents | Strategy | Reduction |
|-----------|--------|----------|:---------:|
| **Function** | cf-git-operations, cf-knowledge-layer, cf-security | Extract entire Execution Steps to per-agent skills | 57-68% |
| **Role** | cf-development, cf-planning, cf-documentation, cf-review, cf-quality-assurance | Remove Working Protocol + trim Communication only; SOPs stay inline | 4-9% |

Function agents see dramatic reductions because their entire Execution Steps move to a per-agent skill loaded on-demand. Role agents see smaller reductions because their SOPs are role-specific and must stay inline.

## Duplicated Patterns Across All 8 Agents

| Pattern | Per Agent | Total (x8) | Evidence |
|---------|:---------:|:----------:|----------|
| Working Protocol section (identical table) | 12 | 96 | Lines 19-31 in every agent def |
| Breadcrumbs line (CLAUDE.md references) | 2 | 16 | Identical across all 8 |
| "MUST NOT run git commit/push" constraint | 2 | 12 | In 6 of 8 (not cf-git-operations, cf-security) |
| Stage completion protocol | 10 | 50 | In 5 role agents |
| "Delegate git ops to cf-git-operations" | 2 | 8 | In 4 agents |
| Commit request message format | 5 | 40 | All 8 agents (Communication section) |
| Progress update message format | 3 | 24 | All 8 agents |
| Escalation message format | 3 | 24 | All 8 agents |
| **Total duplicated** | **~39** | **~270** | |

## Agent Definition Target Template

The agent definition says WHAT to do and WHEN, referencing skill operations for HOW. The **workflow diagram is the SPINE**.

**Structure: Workflow Diagram (SPINE) --> Execution Steps (nodes) --> Supporting Sections**

Every workflow diagram MUST have accompanying execution steps (1:1 mapping). Complex workflows MUST be decomposed: high-level overview diagram + detailed sub-diagrams per branch/scenario (see Principle 6 in design-principles.md).

```text
---
name: cf-{role}
description: "{purpose}"
model: {opus|sonnet}
---

# cf-{role}

## Identity
    ~15 lines: Role, team, work stage, purpose, entry command

## Skills
    ~10 lines: List of skills this agent loads at spawn time
    | Skill | Purpose | Key Operations Used |
    | cf-working-protocol | Cognitive procedures | meta-awareness, think-and-act |
    | cf-team-communication | Messaging patterns | commit-request, progress-update |
    | cf-{domain} | Domain procedures | {operation-1}, {operation-2} |

## Constraints
    ~25 lines: Branch access, tool restrictions, MUST/MUST NOT

## Workflow  <-- THE SPINE
    ~50 lines: ASCII art workflow diagram with annotations

## Execution Steps
    ~80-150 lines: One subsection per workflow node

## Quality Checklist
    ~35 lines: Role-specific verification checks
```

## Per-Agent Breakdown: Function Agents

### cf-git-operations.md (466 --> ~200 lines, 57% reduction)

| Section | Lines | Content | Destination |
|---------|:-----:|---------|-------------|
| Identity | 17 | Unique role definition | STAYS |
| Working Protocol | 11 | Duplicated table | REMOVE |
| Workflow | 18 | Unique ASCII diagram | STAYS |
| Constraints | 28 | Unique branch/tool rules | STAYS |
| Execution Steps | 316 | SOPs: branch, commit, squash, PR, sync, worktree | MOVES to cf-git-workflow |
| Error Handling | 13 | Unique error table | STAYS |
| Communication | 34 | 11 unique peers + 23 common patterns | STAYS (peers) / MOVES (common) |
| Quality Checklist | 13 | Unique checklist | STAYS |
| References | 11 | Partially duplicated | TRIM |

**LOSSLESS verification:** Agent loads 200-line def + 400-500 line cf-git-workflow = 600-700 total (vs 466 currently). Agent has MORE information because the skill includes decision trees and edge cases from the archived version.

### cf-knowledge-layer.md (571 --> ~180 lines, 68% reduction)

| Section | Lines | Content | Destination |
|---------|:-----:|---------|-------------|
| Identity | 23 | Unique (mentions 3 subsumed skills) | STAYS |
| Working Protocol | 12 | Duplicated table | REMOVE |
| Workflow | 18 | Unique dual-workflow ASCII | STAYS |
| Constraints | 22 | Unique data/tool rules | STAYS |
| Execution Steps | 396 | SOPs: three-tier, ledger, memory, task, DB, PathFlow | MOVES to cf-memory-management, cf-task-management, cf-db-operations |
| Communication | 44 | 20 unique peers + 24 common patterns | STAYS (peers) / MOVES (common) |
| Quality Checklist | 11 | Unique checklist | STAYS |
| References | 13 | Partially duplicated | TRIM |

**LOSSLESS verification:** Agent loads 180-line def + 350 + 350 + 350 = 1,230 total (vs 571 currently). Agent has substantially MORE information including decision trees, validation logic, and error recovery from archived skills.

### cf-security.md (324 --> ~140 lines, 57% reduction)

| Section | Lines | Content | Destination |
|---------|:-----:|---------|-------------|
| Identity | 17 | Unique role definition | STAYS |
| Working Protocol | 11 | Duplicated table | REMOVE |
| Workflow | 19 | Unique dual-workflow ASCII | STAYS |
| Constraints | 27 | Unique security rules | STAYS |
| Execution Steps | 181 | SOPs: sandbox, protected resource, merge, permission, settings | MOVES to cf-security-operations |
| Error Handling | 12 | Unique error table | STAYS |
| Communication | 28 | 10 unique peers + 18 common patterns | STAYS (peers) / MOVES (common) |
| Quality Checklist | 12 | Unique checklist | STAYS |
| References | 11 | Partially duplicated | TRIM |

**LOSSLESS verification:** Agent loads 140-line def + 300-400 line cf-security-operations = 440-540 total (vs 324 currently). Agent has MORE information.

## Per-Agent Breakdown: Role Agents

Role agents keep their SOPs inline (role-specific). Only Working Protocol + Communication common patterns are removed.

| Agent | Current | Removed | Target | Reduction |
|-------|:-------:|:-------:|:------:|:---------:|
| cf-development | 455 | 33 (WP 13 + Comm 15 + Refs 5) | ~420 | 8% |
| cf-planning | 524 | 33 | ~490 | 6% |
| cf-documentation | 409 | 33 | ~380 | 7% |
| cf-review | 658 | 33 | ~625 | 5% |
| cf-quality-assurance | 533 | 33 | ~500 | 6% |

**What role agents KEEP inline (unchanged):** Identity, Workflow ASCII diagram, Constraints (MUST/MUST NOT), full Execution Steps (SOPs), Error Handling, unique Communication peers, Quality Checklist. These are the role-specific procedures that define the agent's expertise.

**What role agents REMOVE:**

- Working Protocol section (12 lines) -- cf-working-protocol loaded by SessionStart hook for ALL agents
- Communication common patterns (~15 lines) -- extracted to cf-team-communication skill
- Redundant references (~5 lines) -- trimmed

## Content Movement Verification: Role Agents

| Agent | Working Protocol Removed | Communication Common Removed | Total Removed | Access Preserved? |
|-------|:------------------------:|:----------------------------:|:-------------:|-------------------|
| cf-development | 13 lines | 15 lines | 28 | YES -- WP loaded by SessionStart. Comm patterns in cf-team-communication. Unique SOPs stay inline. |
| cf-planning | 13 lines | 15 lines | 28 | YES |
| cf-documentation | 13 lines | 15 lines | 28 | YES |
| cf-review | 13 lines | 15 lines | 28 | YES |
| cf-quality-assurance | 13 lines | 15 lines | 28 | YES |

## Aggregate Impact

| Agent | Current | Target | Reduction | Strategy |
|-------|:-------:|:------:|:---------:|----------|
| cf-git-operations | 466 | ~200 | 57% | Execution Steps to cf-git-workflow |
| cf-knowledge-layer | 571 | ~180 | 68% | Execution Steps to cf-memory-management, cf-task-management, cf-db-operations |
| cf-security | 324 | ~140 | 57% | Execution Steps to cf-security-operations |
| cf-development | 455 | ~420 | 8% | Working Protocol + Communication trim |
| cf-planning | 524 | ~490 | 6% | Working Protocol + Communication trim |
| cf-documentation | 409 | ~380 | 7% | Working Protocol + Communication trim |
| cf-review | 658 | ~625 | 5% | Working Protocol + Communication trim |
| cf-quality-assurance | 533 | ~500 | 6% | Working Protocol + Communication trim |
| **Total** | **3,940** | **~2,935** | **25%** | |

## Per-Agent Token Savings

**Function agents** (largest savings):

| Agent | Current Tokens | Target Tokens | Savings | Loaded Skill |
|-------|:-------------:|:------------:|:-------:|-------------|
| cf-git-operations | ~3.3K | ~1.4K | ~1.9K (58%) | cf-git-workflow (~3.5K on-demand) |
| cf-knowledge-layer | ~4.0K | ~1.3K | ~2.7K (68%) | cf-memory-management + cf-task-management + cf-db-operations (~8.4K total on-demand, loaded selectively) |
| cf-security | ~2.3K | ~1.0K | ~1.3K (57%) | cf-security-operations (~2.5K on-demand) |

**Role agents** (smaller savings):

| Agent | Current Tokens | Target Tokens | Savings |
|-------|:-------------:|:------------:|:-------:|
| cf-development | ~3.2K | ~2.9K | ~0.3K (9%) |
| cf-planning | ~3.7K | ~3.4K | ~0.3K (8%) |
| cf-documentation | ~2.9K | ~2.7K | ~0.2K (7%) |
| cf-review | ~4.6K | ~4.4K | ~0.2K (4%) |
| cf-quality-assurance | ~3.7K | ~3.5K | ~0.2K (5%) |
