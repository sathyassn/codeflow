---
id: "epic-01KJD6Q4H6MPF58Q7C8JQJCP8V"
format_id: "INF-EPC-019"
title: "Claude Artifact Token Efficiency Restructuring"
summary: "Restructure CLAUDE.md, agent definitions, commands, and skills to reduce token overhead by ~28%, with always-loaded content reduced by ~54%."
status: draft
area_type: "INF"
work_type: "RFCT"
domain: "GENL"
is_ongoing: false
file_scope:
  - ".claude/CLAUDE.md"
  - ".claude/agents/"
  - ".claude/commands/"
  - ".claude/skills/"
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-26T15:00:00Z"
updated_at: "2026-02-26T15:00:00Z"
---

# INF-EPC-019: Claude Artifact Token Efficiency Restructuring

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation script before committing:
> `bash .codeflow/scripts/validation/validate-epic.sh <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory. Script is at `.codeflow/scripts/validation/validate-epic.sh`.

## Summary

Restructure CodeFlow's Claude artifacts (CLAUDE.md, 8 agent definitions, 14 commands, and skills) to align with V4 specification token efficiency targets. The primary goal is reducing always-loaded token overhead from ~13K to ~6K tokens (54% reduction) by:

1. Extracting procedural HOW content from CLAUDE.md into the new `cf-pathflow-protocol` skill
2. Restoring and updating 3 knowledge-layer skills (cf-memory-management, cf-task-management, cf-db-operations) from archive, merging improvements from the current agent def
3. Restoring and updating agent-specific skills (cf-git-workflow, cf-security-operations) from archive with agent def improvements
4. Creating a shared `cf-team-communication` skill to deduplicate messaging patterns across all 8 agent definitions
5. Restoring and updating shared skills (cf-code-exploration, cf-documentation-standards) from archive with agent def improvements
6. Slimming command files (remove working protocol copies, replace Mermaid with ASCII diagrams)

Skills are organized by domain with scope enforcement via YAML `description` field ("Used by:" / "Not used by:"). Archived skills are restored as separate skills (not consolidated), each updated to reflect the latest agent definition improvements.

### Reference Package

Analysis package (8 files): `.codeflow/docs/analysis/artifact-token-efficiency/`

| File | What It Is | When to Use |
|------|-----------|-------------|
| [README.md](.codeflow/docs/analysis/artifact-token-efficiency/README.md) | Executive summary, package navigation | Start here for orientation |
| [01-design-principles.md](.codeflow/docs/analysis/artifact-token-efficiency/01-design-principles.md) | 6 design principles + SPINE mandate + anti-patterns | **MANDATORY first read** before any task |
| [02-skill-taxonomy.md](.codeflow/docs/analysis/artifact-token-efficiency/02-skill-taxonomy.md) | 14-skill tree, access matrix, effective knowledge per agent | When understanding skill organization |
| [03-claude-md.md](.codeflow/docs/analysis/artifact-token-efficiency/03-claude-md.md) | CLAUDE.md section audit, STAY/MOVE classification | For CLAUDE.md restructuring tasks (008, 013) |
| [04-agent-definitions.md](.codeflow/docs/analysis/artifact-token-efficiency/04-agent-definitions.md) | All 8 agent audits, function vs role strategy | For agent definition tasks (009, 010) |
| [05-commands.md](.codeflow/docs/analysis/artifact-token-efficiency/05-commands.md) | 14-command audit, per-command targets | For command tasks (011, 012) |
| [06-skills-detail.md](.codeflow/docs/analysis/artifact-token-efficiency/06-skills-detail.md) | Per-skill source extraction tables and specs | For skill tasks (001-007) |
| [07-implementation-order.md](.codeflow/docs/analysis/artifact-token-efficiency/07-implementation-order.md) | 5-phase execution plan, dependency graph | For planning execution order |

## Scope

### In Scope

- Create 2 new skills: cf-pathflow-protocol, cf-team-communication
- Restore + update 5 archived skills with agent def improvements: cf-git-workflow, cf-memory-management, cf-task-management, cf-db-operations, cf-security-operations
- Restore + update 2 archived skills with agent def improvements: cf-code-exploration, cf-documentation-standards
- Restructure CLAUDE.md to ~400 lines (extract HOW content to cf-pathflow-protocol)
- Slim 3 function agent definitions (extract Execution Steps to per-agent skills)
- Slim 5 role agent definitions (remove Working Protocol, trim Communication)
- Slim 14 commands (remove working protocol copies, add ASCII diagrams, no Mermaid)
- Update capabilities inventory in CLAUDE.md Section 8
- Validation task: run full test suite and PathFlow E2E

### Out of Scope

- Changing PathFlow phase logic or enforcement behavior
- Modifying hook scripts or their configuration
- Changing the data model (JSONL, SurrealDB, markdown tiers)
- Creating new agent definitions or commands
- Modifying test infrastructure or test runner
- Go CLI integration changes
- Modifying existing on-demand standards skills (cf-shell-standards, cf-python-standards, cf-go-standards, cf-sandbox-standards)

## Acceptance Criteria

- [ ] CLAUDE.md reduced to ~400 lines (from 1,424 lines)
- [ ] Always-loaded token overhead reduced from ~13K to ~6K tokens
- [ ] 9 new/restored skills created and functional (cf-pathflow-protocol, cf-git-workflow, cf-memory-management, cf-task-management, cf-db-operations, cf-security-operations, cf-team-communication, cf-code-exploration, cf-documentation-standards)
- [ ] All skills have "Used by:" scope in YAML description field
- [ ] 3 function agent definitions slimmed (Execution Steps replaced with skill references)
- [ ] 5 role agent definitions slimmed (Working Protocol removed, Communication trimmed)
- [ ] All 14 commands slimmed with ASCII diagrams (no Mermaid) and minimal working protocol references
- [ ] Capabilities inventory in CLAUDE.md updated to reflect 14 active skills
- [ ] Full test suite passes after restructuring
- [ ] PathFlow E2E validates restructured artifacts work end-to-end

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-019-001 | Create cf-pathflow-protocol skill | todo | high |
| INF-TSK-019-002 | Restore + update cf-git-workflow skill | todo | high |
| INF-TSK-019-003 | Restore + update cf-memory-management skill | todo | high |
| INF-TSK-019-015 | Restore + update cf-task-management skill | todo | high |
| INF-TSK-019-016 | Restore + update cf-db-operations skill | todo | high |
| INF-TSK-019-004 | Restore + update cf-security-operations skill | todo | normal |
| INF-TSK-019-005 | Create cf-team-communication skill | todo | normal |
| INF-TSK-019-006 | Restore + update cf-code-exploration skill | todo | normal |
| INF-TSK-019-007 | Restore + update cf-documentation-standards skill | todo | normal |
| INF-TSK-019-008 | Restructure CLAUDE.md to awareness-only content | todo | high |
| INF-TSK-019-009 | Slim agent definitions batch 1 (function agents: cf-git-operations, cf-knowledge-layer, cf-security) and cf-development | todo | normal |
| INF-TSK-019-010 | Slim agent definitions batch 2 (role agents: cf-planning, cf-documentation, cf-review, cf-quality-assurance) | todo | normal |
| INF-TSK-019-011 | Slim commands batch 1 (cf-develop, cf-plan, cf-review, cf-test, cf-document, cf-deploy, cf-ship) | todo | normal |
| INF-TSK-019-012 | Slim commands batch 2 (cf-resume, cf-autorun, cf-cleanup, cf-doctor, cf-stack, cf-help, cf-approval-mode) | todo | normal |
| INF-TSK-019-013 | Update CLAUDE.md capabilities inventory | todo | normal |
| INF-TSK-019-017 | Update pathflow-config.json with skill operation references | todo | normal |
| INF-TSK-019-014 | Validation: full test suite and PathFlow E2E | todo | high |

## Dependencies

### Blocked By

- Phase 5 (Commands Implementation) should be complete before starting
- Phase 6 (Autorun) should be complete before starting

### Blocks

- Phase 9 (E2E Testing) benefits from restructured artifacts

## Technical Notes

### Execution Order

Tasks are ordered bottom-up: build referenced content first (skills), then restructure referencing artifacts (agents, commands, CLAUDE.md).

```text
Phase A: Create/Restore/Update Skills (Tasks 1-7 + 015/016, parallelizable)
    |
    +-- INF-TSK-019-001: cf-pathflow-protocol (NEW, from CLAUDE.md S4/S5/S6/S7/S11)
    +-- INF-TSK-019-002: cf-git-workflow (RESTORE archive + UPDATE from cf-git-operations)
    +-- INF-TSK-019-003: cf-memory-management (RESTORE archive + UPDATE from cf-knowledge-layer)
    +-- INF-TSK-019-015: cf-task-management (RESTORE archive + UPDATE from cf-knowledge-layer)
    +-- INF-TSK-019-016: cf-db-operations (RESTORE archive + UPDATE from cf-knowledge-layer)
    +-- INF-TSK-019-004: cf-security-operations (RESTORE archive + UPDATE from cf-security)
    +-- INF-TSK-019-005: cf-team-communication (NEW, EXTRACT from all 8 agent defs)
    +-- INF-TSK-019-006: cf-code-exploration (RESTORE archive + UPDATE from agent defs)
    +-- INF-TSK-019-007: cf-documentation-standards (RESTORE archive + UPDATE from agent def)
    |
    v
Phase B: Slim Agent Definitions (Tasks 9-10, batched)
    |   Skills exist -> agents can now reference them
    |
    +-- INF-TSK-019-009: Slim agents batch 1 (function agents + cf-development)
    |       (depends on Tasks 2-5 + 015/016 existing: per-agent skills + cf-team-communication)
    |
    +-- INF-TSK-019-010: Slim agents batch 2 (role agents)
    |       (depends on Task 5 existing: cf-team-communication)
    |
    v
Phase C: Slim Commands (Tasks 11-12, batched)
    |   Skills + slimmed agents exist -> commands can reference both
    |
    +-- INF-TSK-019-011: Slim commands batch 1
    +-- INF-TSK-019-012: Slim commands batch 2
    |
    v
Phase D: Restructure CLAUDE.md (Task 8)
    |   Skills, agents, commands all restructured -> CLAUDE.md references everything above
    |
    +-- INF-TSK-019-008: Restructure CLAUDE.md to awareness-only content
    |       (depends on all skills, agents, and commands being restructured)
    |
    v
Phase E: Finalize (Tasks 13, 017, 014, sequential)
    |
    +-- INF-TSK-019-013: Update capabilities inventory in CLAUDE.md
    |       (depends on all Phase A-D tasks being complete)
    +-- INF-TSK-019-017: Update pathflow-config.json with skill operation references
    |       (depends on all Phase A-D tasks + 013 being complete)
    +-- INF-TSK-019-014: Full validation
            (depends on 013 and 017 being complete)
```

### Skill Taxonomy (9 new/restored)

```text
Lead-Facing:
    +-- cf-working-protocol        (EXISTING, unchanged, always-loaded)
    +-- cf-pathflow-protocol       (NEW: phases, stages, enforcement, recovery)

Agent-Specific (one agent each):
    +-- cf-git-workflow            (RESTORE + UPDATE: git ops procedures)
    +-- cf-memory-management       (RESTORE + UPDATE: memory lifecycle, work detection)
    +-- cf-task-management         (RESTORE + UPDATE: work classification, epic/task CRUD)
    +-- cf-db-operations           (RESTORE + UPDATE: three-tier persistence, queries)
    +-- cf-security-operations     (RESTORE + UPDATE: security ops procedures)

Shared Agent (multiple agents):
    +-- cf-team-communication      (NEW: peer messaging, commit requests, escalation)
    +-- cf-code-exploration         (RESTORE + UPDATE: search patterns, code comprehension)
    +-- cf-documentation-standards  (RESTORE + MERGE cf-markdown-standards: doc structure, style, quality, templates)

Standards (EXISTING, unchanged):
    +-- cf-shell-standards, cf-python-standards, cf-go-standards
    +-- cf-sandbox-standards
```

### Risk Mitigations

| Risk | Mitigation |
|------|-----------|
| Restructured CLAUDE.md loses critical instructions | Section-by-section extraction plan in analysis document; each task validates test suite passes |
| Function agents fail without Execution Steps | Per-agent skills (cf-git-workflow, cf-memory-management + cf-task-management + cf-db-operations, cf-security-operations) contain the same procedures, loaded on-demand |
| Communication patterns lost after extraction | cf-team-communication skill + agent-specific peer lists in trimmed Communication sections |
| Commands break without inline procedures | Commands reference skills; ASCII diagrams replace Mermaid |
| Token savings lower than projected | Savings targets are estimates; functional correctness is the hard requirement |

## Related

- Analysis package: `.codeflow/docs/analysis/artifact-token-efficiency/` (8 files -- see Reference Package section above)
- V4 Spec (main agent): `codeflow-specification-v4/05-claude-components/main-agent/01-claude-md-spec.md`
- V4 Spec (skills framework): `codeflow-specification-v4/11-reference/frameworks/cf-skills-framework.md`
- V4 Spec (main agent framework): `codeflow-specification-v4/11-reference/frameworks/cf-main-agent-framework.md`
- Planning task: `project-management/epics/PLN/PLN-EPC-001/tasks/PLN-TSK-001-004.md`
