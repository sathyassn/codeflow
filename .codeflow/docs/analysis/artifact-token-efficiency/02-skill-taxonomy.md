# Skill Taxonomy

## Scope Enforcement

Each skill's YAML `description` field includes "Used by:" and optionally "Not used by:" directives. Agents check this at discovery time before loading.

```text
Example YAML frontmatter:
---
name: cf-git-workflow
description: "Git operations procedures (branch, commit, PR, sync). Used by: cf-git-operations. Not used by: team-lead."
---
```

## Complete Taxonomy (15 Skills)

```text
Skill Taxonomy:

Lead-Facing (loaded by team lead):
    +-- cf-working-protocol       (EXISTING, always-loaded by SessionStart hook)
    +-- cf-pathflow-protocol      (NEW, on-demand: phases, stages, enforcement, recovery)

Agent-Specific (loaded by one agent):
    +-- cf-git-workflow           (RESTORE from archive + update from agent def)
    +-- cf-memory-management      (RESTORE from archive + update from agent def)
    +-- cf-db-operations          (RESTORE from archive + update from agent def)
    +-- cf-security-operations    (RESTORE from archive + update from agent def)

Shared Agent (loaded by multiple agents):
    +-- cf-team-communication     (NEW, extract from all 8 agent defs)
    +-- cf-task-management        (RESTORE from archive + update from agent def)
    +-- cf-code-exploration       (RESTORE from archive + update from agent defs)
    +-- cf-documentation-standards (RESTORE from archive + update from agent def)

Standards (EXISTING, unchanged):
    +-- cf-shell-standards
    +-- cf-python-standards
    +-- cf-go-standards
    +-- cf-markdown-standards
    +-- cf-sandbox-standards
```

**Total: 15 active skills** (currently 6 active + 10 archived; target 15 active + 1 archived)

Note: The 3 knowledge domain skills (cf-memory-management, cf-task-management, cf-db-operations) are kept as separate functional domains rather than consolidated. Each has a distinct concern: work lifecycle, task/epic CRUD, and persistence operations. cf-knowledge-layer references all three; cf-planning also references cf-task-management for work decomposition. One skill per functional domain -- if an agent needs multiple domains, it references multiple skills.

## Skill Detail Table

| Skill | Category | Used By (in YAML description) | Source | Loading | Est. Lines |
|-------|----------|-------------------------------|--------|---------|:----------:|
| **cf-working-protocol** | Lead-facing | team-lead | EXISTING | Always (SessionStart hook) | 410 |
| **cf-pathflow-protocol** (NEW) | Lead-facing + Agents | team-lead, cf-development, cf-planning, cf-documentation, cf-quality-assurance, cf-review | Extract from CLAUDE.md S4/S5/S6/S7/S11 + agent defs | On-demand | ~500-600 |
| **cf-git-workflow** (RESTORE+) | Agent-specific | cf-git-operations | Restore archive + update from agent def | On-demand | ~400-500 |
| **cf-memory-management** (RESTORE+) | Agent-specific | cf-knowledge-layer | Restore archive + update from agent def | On-demand | ~350-400 |
| **cf-task-management** (RESTORE+) | Shared agent | cf-knowledge-layer, cf-planning | Restore archive + update from agent def | On-demand | ~350-400 |
| **cf-db-operations** (RESTORE+) | Agent-specific | cf-knowledge-layer | Restore archive + update from agent def | On-demand | ~350-400 |
| **cf-security-operations** (RESTORE+) | Agent-specific | cf-security | Restore archive + update from agent def | On-demand | ~300-400 |
| **cf-team-communication** (NEW) | Shared agent | all agents | Extract from all 8 agent defs | On-demand | ~150-200 |
| **cf-code-exploration** (RESTORE+) | Shared agent | cf-planning, cf-development, cf-review | Restore archive + update from agent defs | On-demand | ~200-250 |
| **cf-documentation-standards** (RESTORE+) | Shared agent | cf-documentation | Restore archive + update from agent def | On-demand | ~200-250 |
| **cf-shell-standards** | Standards | cf-development, cf-git-operations | EXISTING | On-demand | (unchanged) |
| **cf-python-standards** | Standards | cf-development, cf-quality-assurance | EXISTING | On-demand | (unchanged) |
| **cf-go-standards** | Standards | cf-development | EXISTING | On-demand | (unchanged) |
| **cf-markdown-standards** | Standards | cf-planning, cf-documentation | EXISTING | On-demand | (unchanged) |
| **cf-sandbox-standards** | Standards | cf-git-operations, cf-development, cf-quality-assurance | EXISTING | On-demand | (unchanged) |

## Skill-to-Agent Access Matrix

This matrix proves that every agent retains access to all content it currently uses, showing the loading mechanism for each skill-agent pair.

| Skill | team-lead | cf-git-ops | cf-knowledge | cf-security | cf-dev | cf-plan | cf-doc | cf-review | cf-qa |
|-------|:---------:|:----------:|:------------:|:-----------:|:------:|:-------:|:------:|:---------:|:-----:|
| cf-working-protocol | SS | SS | SS | SS | SS | SS | SS | SS | SS |
| cf-pathflow-protocol | OD | -- | -- | -- | SP | SP | SP | SP | SP |
| cf-git-workflow | -- | SP | -- | -- | -- | -- | -- | -- | -- |
| cf-memory-management | -- | -- | SP | -- | -- | -- | -- | -- | -- |
| cf-task-management | -- | -- | SP | -- | -- | SP | -- | -- | -- |
| cf-db-operations | -- | -- | SP | -- | -- | -- | -- | -- | -- |
| cf-security-operations | -- | -- | -- | SP | -- | -- | -- | -- | -- |
| cf-team-communication | OD | SP | SP | SP | SP | SP | SP | SP | SP |
| cf-code-exploration | -- | -- | -- | -- | SP | SP | -- | SP | -- |
| cf-documentation-standards | -- | -- | -- | -- | -- | SP | SP | -- | -- |
| cf-shell-standards | -- | OD | -- | -- | OD | -- | -- | -- | OD |
| cf-python-standards | -- | -- | -- | -- | OD | -- | -- | -- | OD |
| cf-go-standards | -- | -- | -- | -- | OD | -- | -- | -- | -- |
| cf-markdown-standards | -- | -- | -- | -- | -- | OD | OD | -- | -- |
| cf-sandbox-standards | -- | OD | OD | -- | OD | -- | -- | -- | OD |

**Legend:** SS = SessionStart (always-loaded), SP = Spawn-loaded (in agent def YAML + spawn prompt), OD = On-demand (agent loads when needed), -- = Not applicable.

**Verification rules:**

1. Every SS cell means the skill is available to ALL agents automatically (loaded by SessionStart hook before any agent spawns).
2. Every SP cell means the agent's YAML `description` field includes this skill name, AND the spawn prompt instructs the agent to read referenced skills.
3. Every OD cell means the agent loads the skill when it encounters a task requiring those standards (same mechanism as today).
4. No agent loses access to ANY content it currently has. Function agents gain access to richer skill content that includes decision trees and error recovery from archived skill versions.

## Effective Knowledge Per Agent

For each agent, what they can access before and after restructuring:

| Agent | Current Knowledge Sources | Restructured Knowledge Sources | Net Change |
|-------|--------------------------|-------------------------------|------------|
| **team-lead** | CLAUDE.md (1,424 lines) + cf-working-protocol (410 lines) = 1,834 always-loaded | CLAUDE.md (~400 lines) + cf-working-protocol (410 lines) = ~810 always-loaded. cf-pathflow-protocol (~550 lines) + cf-team-communication (~175 lines) loaded on-demand = ~725 on-demand. Total accessible: ~1,535 | SAME content, ~7K fewer always-loaded tokens |
| **cf-git-operations** | Agent def (466 lines) = 466 at spawn | Agent def (~200 lines) + cf-git-workflow (~450 lines) + cf-team-communication (~175 lines) = ~825 at spawn | MORE content (+359 lines). Gains decision trees from archive. |
| **cf-knowledge-layer** | Agent def (571 lines) = 571 at spawn | Agent def (~180 lines) + cf-memory-management (~375 lines) + cf-task-management (~375 lines) + cf-db-operations (~375 lines) + cf-team-communication (~175 lines) = ~1,480 at spawn | MORE content (+909 lines). Gains full archived decision trees. |
| **cf-security** | Agent def (324 lines) = 324 at spawn | Agent def (~140 lines) + cf-security-operations (~350 lines) + cf-team-communication (~175 lines) = ~665 at spawn | MORE content (+341 lines). Gains complete security decision trees. |
| **cf-development** | Agent def (455 lines) = 455 at spawn | Agent def (~427 lines) + cf-team-communication (~175 lines) = ~602+ at spawn | MORE content (+147 lines). Gains shared communication patterns. |
| **cf-planning** | Agent def (524 lines) = 524 at spawn | Agent def (~496 lines) + cf-team-communication (~175 lines) + cf-task-management (~375 lines) = ~1,046+ at spawn | MORE content (+522 lines). Gains full task management skill. |
| **cf-documentation** | Agent def (409 lines) = 409 at spawn | Agent def (~381 lines) + cf-team-communication (~175 lines) + cf-documentation-standards (~250 lines) = ~806+ at spawn | MORE content (+397 lines). Gains documentation standards from archive. |
| **cf-review** | Agent def (657 lines) = 657 at spawn | Agent def (~629 lines) + cf-team-communication (~175 lines) + cf-code-exploration (~250 lines) = ~1,054+ at spawn | MORE content (+397 lines). Gains code exploration patterns. |
| **cf-quality-assurance** | Agent def (533 lines) = 533 at spawn | Agent def (~505 lines) + cf-team-communication (~175 lines) = ~680+ at spawn | MORE content (+147 lines). Gains shared communication patterns. |

## Loading Mechanisms

| Mechanism | When Content Loads | Token Cost | Example |
|-----------|-------------------|------------|---------|
| **Always-loaded** | Session start (automatic) | Every session | CLAUDE.md, cf-working-protocol |
| **Spawn-loaded** | Teammate creation (automatic via spawn prompt) | Once per teammate lifetime | Agent definition + referenced skills in spawn prompt |
| **On-demand** | Agent reads skill when needed | Once per invocation | cf-shell-standards, cf-sandbox-standards |

**Key principle:** Moving content from an always-loaded artifact (CLAUDE.md) to a spawn-loaded skill (cf-pathflow-protocol) does NOT reduce an agent's access -- it changes WHEN the content loads. The team lead loads cf-pathflow-protocol at PF4-EXECUTE start instead of loading it at session start. The effective knowledge is identical; the timing is optimized.

**Agent definition + skill loading pattern:**

```text
Current (embedded):
    Agent spawned --> Reads agent def (contains full SOPs inline)
    Result: Agent has all procedures (570 lines for cf-knowledge-layer)

Restructured (skill-referenced):
    Agent spawned --> Reads agent def (identity + constraints + references)
                  --> Reads referenced skill(s) (full SOPs)
    Result: Agent has all procedures (180 + 350 + 350 + 350 = 1,030 lines for cf-knowledge-layer)
    Net change: Agent has MORE content (procedures + decision trees from skill format)
```
