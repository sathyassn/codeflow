---
id: "epic-01KNS2FKJYJ8C6C1GV1J6Z6VQX"
format_id: "INF-EPC-045"
title: "QA Report: project-wide coverage exceptions"
summary: "Surface all project-wide coverage exceptions from test-config.json in QA Report and CI output"
status: complete
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: normal
pr_number: null
external_id: null
external_url: null
created_at: "2026-04-09T12:12:00Z"
updated_at: "2026-04-09T12:12:00Z"
---

# INF-EPC-045: QA Report: project-wide coverage exceptions

## Summary

Surface all project-wide coverage exceptions configured in test-config.json in the QA Report output and the task template's Exempted Files table. The Exempted Files table should enumerate all configured exceptions, not just those triggered during a specific run.

## Scope

### In Scope

- QA Report Exempted Files table in `project-management/templates/task-template.md`
- WS-QA instructions in `.claude/agents/cf-quality-assurance.md`
- PF5-VERIFY test stats gate and PR body template in `.claude/CLAUDE.md`
- TestValidator Rust code if changes needed to enumerate all exceptions

### Out of Scope

- Changes to test-config.json exception list itself
- Coverage threshold changes

## Acceptance Criteria

- [ ] QA Report Exempted Files table shows ALL configured exceptions from test-config.json (not just triggered ones)
- [ ] Each row includes: file path, coverage %, configured threshold, reason
- [ ] CLAUDE.md, cf-quality-assurance.md, and task-template.md updated consistently
- [ ] codeflow test --mode full --coverage passes with 0 failures

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-045-001 | QA Report: show all project-wide coverage exceptions from test-config.json | complete | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

Documentation-first epic. Primary changes are to markdown files and agent definitions. Rust test_validator changes only if the current implementation does not already enumerate all configured exceptions.

### Autorun Batching

Not autorun-eligible (autorun_eligible: false).

## Related

- test-config.json: `codeflow-cli/config/testing/test-config.json`
- TestValidator: `codeflow-cli/core/src/test_validator/`
