# INF-TSK-FIX-GENL-010

## Add sentinel auto-creation language to cf-quality-assurance.md

| Field | Value |
|-------|-------|
| Epic | INF-EPC-QUAL-GENL-001 (Quality Infrastructure Hardening) |
| Area | INF (Infrastructure) |
| Type | FIX |
| Domain | GENL |
| Priority | low |
| Estimate | XS |
| Status | complete |
| Branch | fix/qa-sentinel-doc |

## Description

Update cf-quality-assurance.md agent definition to include language about sentinel auto-creation by hooks. Agents must NOT create sentinels manually. Aligns with existing language in CLAUDE.md and other agent definitions.

**Scope expanded (Tier 2 decision):** Also includes:

1. PathFlow config task clarity -- add responsible_teammate and operation references to all 24 phase tasks in pathflow-config.json
2. Go CLI fallback instructions in cf-knowledge-layer.md for when codeflow CLI is unavailable
3. CLAUDE.md phase task mapping improvements

## Acceptance Criteria

1. cf-quality-assurance.md includes clear language that sentinels are auto-created by PostToolUse hooks
2. Language states agents must NOT create sentinels manually
3. Consistent with CLAUDE.md Section 4 sentinel language and other agent definitions
4. pathflow-config.json phase tasks include responsible_teammate and operation references
5. cf-knowledge-layer.md includes Go CLI fallback instructions
6. CLAUDE.md phase task mapping improved

## Progress

- 2026-02-17: Task created and work begun
- 2026-02-17: Scope expanded (Tier 2 decision) -- added pathflow-config clarity, knowledge-layer CLI fallback, CLAUDE.md improvements
- 2026-02-17: Work complete (5 commits, WS-REV approved)

## Completion

- **Completed:** 2026-02-17T14:55:26Z
- **Commits:** 5
- **Review:** WS-REV approved
- **Deliverables:**
  - cf-quality-assurance.md: sentinel auto-creation language added
  - pathflow-config.json: responsible/operation fields added to all 22 phase tasks
  - cf-knowledge-layer.md: canonical JSONL filename table + Go CLI fallback instructions
  - CLAUDE.md: PF1 timing ambiguity fixed
  - JSONL migration: orphaned workgraph-events.jsonl merged into canonical work-graph.jsonl
