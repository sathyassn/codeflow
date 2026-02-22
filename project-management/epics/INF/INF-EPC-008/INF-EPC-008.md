---
id: "epic-01KJ12VSN1YSWYQ03CDK8ENG78"
format_id: "INF-EPC-008"
title: "PathFlow PR Verification, Merge Protection & Validation Hardening"
summary: "Add PF6 PR verification step, configurable merge protection for protected branches, autorun integration branch convention, task/epic field validation scripts, schema updates (awaiting_review status, deprecate auto_commit), and V4 spec alignment"
status: in_progress
area_type: "INF"
work_type: "CHOR"
domain: "PMGT"
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-21"
updated_at: "2026-02-21"
---

# INF-EPC-008: PathFlow PR Verification, Merge Protection & Validation Hardening

## Summary

Close three gaps in the PathFlow pipeline and perform two cleanup items:

1. **PF6-to-PF7 gap**: Add a PR verification step (PF6-TSK-06) that polls CI, notifies the user, and optionally auto-merges to integration branches before proceeding to PF7.
2. **No merge protection**: Add a configurable PreToolUse hook that hard-blocks `gh pr merge` targeting protected branches. Human merges via GitHub UI.
3. **No field validation**: Add deterministic validation scripts for task and epic YAML frontmatter fields, executed at PF4-TSK-02, WS-PLAN, and PF6-TSK-01.
4. **Deprecate `auto_commit`**: Leave column in schema with deprecation comment, remove from templates and agent references.
5. **PF3 reorder**: Move branch creation before task registration (conditional for adhoc tasks only).

Additionally, add `awaiting_review` as a new task status, establish the `autorun/{batch-name}` integration branch convention, redefine `/cf-ship` as notification for protected branches, and update V4 specs to align with implementation.

## Scope

### In Scope

- PF6-TSK-06: verify-pr-and-sync (3 modes: interactive, autorun+auto_merge, autorun+no_merge)
- Merge protection: enforcement-policy.json config + PreToolUse hook extension
- Autorun integration branch convention (`autorun/{batch-name}`)
- Deterministic validation scripts for task and epic fields
- DB schema: `awaiting_review` status, `auto_commit` deprecation
- PF3 task reordering (conditional for adhoc tasks)
- `/cf-ship` redefinition as notification for protected branches
- Agent definition updates (cf-git-operations, cf-knowledge-layer, cf-planning, cf-security)
- Command definition updates (cf-ship, cf-autorun)
- pathflow-config.json updates (PF3, PF4, PF6 phases)
- CLAUDE.md updates (Sections 4.2, 4.3, 6)
- Template updates (task-template.md)
- V4 spec alignment (session-lifecycle, autorun, commands)

### Out of Scope

- Go CLI implementation (Phase 7)
- Frontend or API changes
- Changes to WS-REV review logic itself
- GitHub Actions CI pipeline changes
- INF-TSK-005-009 (separate epic)

## Acceptance Criteria

- [ ] PF6-TSK-06 exists in pathflow-config.json with verify-pr-and-sync operation
- [ ] cf-git-operations has verify-pr-and-sync SOP covering all 3 modes
- [ ] `gh pr merge` targeting protected branches is hard-blocked by PreToolUse hook
- [ ] enforcement-policy.json has `merge_protection` section with configurable protected_branches
- [ ] Validation scripts (`validate-task.sh`, `validate-epic.sh`) exist and pass on valid inputs
- [ ] PF4-TSK-02 in pathflow-config.json gates work execution with validation
- [ ] DB schema migration adds `awaiting_review` to tasks status CHECK constraint
- [ ] `auto_commit` column has deprecation comment in schema.sql; removed from task template
- [ ] PF3 tasks reordered: branch creation before task registration (conditional for adhoc)
- [ ] `/cf-ship` command redefined as notification for protected branches
- [ ] V4 spec files updated for PF3, PF4, PF6 changes and merge protection
- [ ] CLAUDE.md Sections 4.2, 4.3, 6 updated

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-008-001 | Verify planning artifacts and prerequisites | complete | high |
| INF-TSK-008-002 | Schema changes: awaiting_review status + deprecate auto_commit | complete | high |
| INF-TSK-008-003 | Merge protection: enforcement-policy.json + hook extension | complete | high |
| INF-TSK-008-004 | Validation scripts: validate-task.sh + validate-epic.sh | complete | high |
| INF-TSK-008-005 | PF3 reorder + PF4 validation gate in pathflow-config.json | complete | normal |
| INF-TSK-008-006 | PF6-TSK-06 verify-pr-and-sync in pathflow-config.json | complete | normal |
| INF-TSK-008-007 | Agent definition updates: cf-git-operations, cf-knowledge-layer, cf-planning, cf-security | complete | normal |
| INF-TSK-008-008 | Command definition updates: cf-ship, cf-autorun | todo | normal |
| INF-TSK-008-009 | CLAUDE.md updates: Sections 4.2, 4.3, 6 | todo | normal |
| INF-TSK-008-010 | Template updates: task-template.md | todo | low |
| INF-TSK-008-011 | V4 spec alignment: session-lifecycle, autorun, commands | todo | low |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

- **Design analysis**: See [design analysis](../../../../.codeflow/docs/analysis/inf-epc-008-design-analysis.md) for full design rationale, flow diagrams, and decision log (11 decisions: D1-D11).
- **Task ordering**: INF-TSK-008-002 (schema) must complete before INF-TSK-008-004 (validation scripts) because validation checks the new `awaiting_review` status value. INF-TSK-008-003 (merge protection) and INF-TSK-008-004 (validation) are independent and can be parallelized.
- **PF3 reorder rationale**: Branch creation triggers the pf-3 sentinel which unlocks Edit/Write. Moving it before task registration ensures cf-knowledge-layer can write task records after the gate opens.
- **SQLite CHECK constraint limitation**: SQLite does not support `ALTER TABLE ... ALTER COLUMN`. The migration must recreate the tasks table with the updated CHECK constraint, preserving all data.
- **auto_commit deprecation**: Column stays in schema for backward compatibility. Agents stop reading/writing it. Templates and docs remove it.
- **Merge protection defense-in-depth**: Hook blocks `gh pr merge` + agent instructions forbid it + GitHub branch protection rules are the final safety net.

## Related

- PLN-TSK-001-002: Planning session that produced this epic (tracked under PLN-EPC-001)
- INF-EPC-005: Project Management Standardization (schema foundation)
- INF-EPC-006: Sandbox Network Settings & Templates (enforcement patterns)
- INF-EPC-007: QA Pipeline & Test Coverage (quality gate patterns)
- pathflow-config.json: PF3, PF4, PF6 phase definitions
- enforcement-policy.json: Existing security enforcement configuration
