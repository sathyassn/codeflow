---
id: "{task-ULID}"                     # ULID PK: task-{ulid} (auto-generated)
format_id: "{AREA}-TSK-{NNN}-{NNN}"   # Human-readable unique ID
epic_id: "{epic-ULID}"                # FK to epics(id)
epic_format_id: "{AREA}-EPC-{NNN}"    # Cross-reference to epic format_id (convenience; not yet in DB tasks table)
title: "{Title}"
description: "{One-line description}"
status: todo                           # todo|blocked|in_progress|awaiting_review|complete
area_type: "{AREA}"                    # FRT|BKD|INF|SHR|DOC|PLN
work_type: "{TYPE}"                    # FEAT|FIX|HTFX|RFCT|DOCS|TEST|CHOR|CICD|SPKE|PLAN
domain: "{domain}"                     # GENL|PMGT|QUAL|{custom}
origin: planned                        # planned|informal|auto
file_scope: []
scope_policy: soft                     # soft|hard|permissive
scope_root: null
estimate: null                         # XS|S|M|L|XL
priority: normal                       # low|normal|high|critical
assignee_id: null
autorun_eligible: false
raise_pr: true
auto_merge: false
target_branch: null
acceptance: []                         # JSON array -- required if autorun_eligible
tests: []                              # JSON array -- recommended for autorun
branch: null
pr_number: null
external_id: null
external_url: null
created_at: "{ISO-8601}"
updated_at: "{ISO-8601}"
started_at: null
completed_at: null
stage: null                            # dev|work|review|qa|done
stage_status: null                     # pending|in_progress|complete|failed
stage_history: "[]"                    # JSON array of stage transition records
---

# {format_id}: {Title}

## Description

{Detailed description of what this task accomplishes and why it is needed.}

## Approach

1. {Step-by-step implementation approach}

## Files

### To Modify

- `{path/to/file}` -- {what changes and why}

### To Create

- `{path/to/new-file}` -- {purpose}

## Acceptance Criteria

1. {Specific, measurable criterion with file:line if applicable}
2. {Specific, measurable criterion}

## Dependencies

### Blocked By

- None

### Blocks

- None

## Verification

### Automated

- [ ] {Test or script that validates the change}

### Manual

- [ ] {Human verification step}

## Notes

{Implementation hints, edge cases, known pitfalls, or references to related decisions.}
