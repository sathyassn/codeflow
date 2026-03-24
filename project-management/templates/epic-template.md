---
id: "{epic-ULID}"                    # ULID PK: epic-{ulid} (auto-generated)
format_id: "{AREA}-EPC-{NNN}"        # Human-readable unique ID
title: "{Title}"
summary: "{One-line summary}"
status: draft                         # draft|planning|in_progress|blocked|complete|archived
area_type: "{AREA}"                   # FRT|BKD|INF|SHR|DOC|PLN
work_type: "{TYPE}"                   # FEAT|FIX|HTFX|RFCT|DOCS|TEST|CHOR|CICD|SPKE|PLAN
domain: "{domain}"                    # GENL|PMGT|QUAL|{custom}
is_ongoing: false
file_scope: []
priority: normal                      # low|normal|high|critical
pr_number: null
external_id: null
external_url: null
created_at: "{ISO-8601}"
updated_at: "{ISO-8601}"
---

# {format_id}: {Title}

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the epic validation command before committing:
> `codeflow validate epic <path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

{One-paragraph description of the epic's purpose and expected outcome.}

## Scope

### In Scope

- {What this epic covers}

### Out of Scope

- {What this epic explicitly does NOT cover}

## Acceptance Criteria

- [ ] {Specific, measurable criterion}
- [ ] {Specific, measurable criterion}

### PII Handling Review

- [ ] Does this epic involve code that handles PII? (Y/N)
- [ ] If Y: Direct PII check -- no hardcoded PII in source/tests/comments (emails, names, tokens, IPs)
- [ ] If Y: Code logic review -- PII-handling code follows security standards:
  - Encryption at rest and in transit
  - Proper hashing (bcrypt/argon2 for passwords, not MD5/SHA1)
  - Input sanitization and validation
  - Logging redaction (no PII in logs)
  - Access controls on PII data stores
- [ ] If Y: Reviewed against OWASP Top 10 and industry standards (GDPR, SOC 2)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| {AREA}-TSK-{NNN}-{NNN} | {Task title} | todo | normal |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Technical Notes

{Architecture decisions, constraints, or implementation guidance relevant to all tasks in this epic.}

### Autorun Batching

> For epics with autorun-eligible tasks, document execution
> strategy here.

**Execution Order:** (topological order respecting dependencies)

**Batch Groups:** (tasks that can run in parallel -- must have
non-overlapping file_scope)

| Batch | Tasks | max_workers | Notes |
|-------|-------|-------------|-------|
| 1 | (task IDs) | (n) | (dependency/scope notes) |

**Estimated Duration:** (based on task estimates and parallelism)

## Related

- {Links to related epics, PRs, or external resources}
