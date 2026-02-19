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

## Related

- {Links to related epics, PRs, or external resources}
