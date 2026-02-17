# INF-TSK-FEAT-GENL-003: Create cf-sandbox-standards on-demand skill

## Metadata

| Field | Value |
|-------|-------|
| Task ID | INF-TSK-FEAT-GENL-003 |
| Epic | INF-EPC-QUAL-GENL-001 (Quality Infrastructure Hardening) |
| Status | complete |
| Area | INF (Infrastructure) |
| Work Type | FEAT |
| Domain | GENL |
| Priority | normal |
| Estimate | M |
| Branch | feat/sandbox-skill |
| Created | 2026-02-17 |

## Description

Create a new on-demand skill at `.claude/skills/cf-sandbox-standards/` documenting network and sandbox bypass patterns, `dangerouslyDisableSandbox` usage guidelines, and approved external domains. Reference in CLAUDE.md Section 7 and relevant agent definitions.

## Scope

- `.claude/skills/cf-sandbox-standards/` (new)
- `.claude/CLAUDE.md` (reference in Section 7)
- `.claude/agents/cf-*.md` (reference in relevant agents)

## Progress

- [x] Task registered in WorkGraph
- [x] Branch created: feat/sandbox-skill
- [x] Skill SKILL.md created (4 operations: classify-operation, apply-bypass, delegate-network-op, validate-network-safety)
- [x] CLAUDE.md Section 7, 10 (sandbox bypass), Section 4 (task tracker mirroring) updated
- [x] All 8 agent definitions updated with skill references
- [x] Renamed responsible→assigned_to across 22 pathflow-config tasks
- [x] Fixed test assertion for mirror_target
- [x] WS-REV approved (round 2), 339/339 tests pass
- [x] Work complete (4 commits on feat/sandbox-skill)
