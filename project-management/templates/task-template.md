---
id: "{task-ULID}"                     # ULID PK: task-{ulid} (auto-generated)
format_id: "{AREA}-TSK-{NNN}-{NNN}"   # Human-readable unique ID
epic_id: "{epic-ULID}"                # FK to epics(id)
epic_format_id: "{AREA}-EPC-{NNN}"    # Cross-reference to epic format_id (convenience; not yet in DB tasks table)
title: "{Title}"
description: "{One-line description}"
status: todo                           # todo|blocked|in_progress|complete|cancelled
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
tests: []                              # JSON array -- REQUIRED for code-producing work types (FEAT, FIX, RFCT, HTFX, CHOR, CICD, TEST)
                                       # Format: JSON array of test file paths relative to .codeflow/testing/
                                       # Example: ["scripts/validation/test-validate-task.sh"]
branch: null
pr_number: null
external_id: null
external_url: null
dependencies: []
created_at: "{ISO-8601}"
updated_at: "{ISO-8601}"
started_at: null
completed_at: null
stage: null                            # dev|plan|docs|test|review|qa|done
stage_status: null                     # pending|in_progress|complete|failed
stage_history: "[]"                    # JSON array of stage transition records
---

# {format_id}: {Title}

> **MANDATORY VALIDATION:** Files created from this template MUST be validated against the task validation command before committing:
> `codeflow validate task <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Description

{Detailed description of what this task accomplishes and why it is needed.}

## Pre-Work Analysis

> **MANDATORY at task start.** Before implementation, review
> recent commits and related work that may affect this task's
> scope, approach, or acceptance criteria. Update the task
> description and criteria if needed.

- [ ] Reviewed git log for recent changes to files in scope
- [ ] Checked if related tasks/PRs modified shared code
- [ ] Verified acceptance criteria still accurate
- [ ] Updated approach if implementation landscape changed

## Approach

1. {Step-by-step implementation approach}

## Standards & Practices

Apply the relevant standards skill for each language/tool used:

| Language/Tool | Standards Skill | Key Requirements |
|--------------|----------------|-----------------|
| Rust | cf-rust-standards | No unsafe, thiserror/anyhow, clippy clean |
| Shell/Bash | cf-shell-standards | shellcheck clean, error handling |
| Python | cf-python-standards | Type hints, pytest |
| Go | cf-go-standards | gofmt, golint, go vet |
| SurrealDB | cf-surrealdb-standards | DEFINE OVERWRITE, embedded mode |
| Markdown | cf-markdown-standards | Lint rules, templates |

Read the applicable skill BEFORE starting implementation.

## Files

### To Modify

- `{path/to/file}` -- {what changes and why}

### To Create

- `{path/to/new-file}` -- {purpose}

### To Read

- (files to read for context but not modify)

## Concurrency Considerations

> Identify any shared-state operations in this task and document
> the locking/safety strategy. Mark N/A if no shared state.

| Shared Resource | Access Pattern | Safety Mechanism |
|----------------|---------------|-----------------|
| (e.g., state.loro) | (read-modify-write) | (locked_binary_rmw via file_lock.rs) |
| (e.g., codeflow.db) | (concurrent queries) | (SurrealDB transactions) |
| (e.g., config.json) | (read-only at runtime) | (N/A -- immutable during execution) |

Mechanisms available in codebase:

- `file_lock.rs` -> `locked_binary_rmw` for CRDT state (state.loro)
- SurrealDB embedded -> transactions for DB operations
- Atomic temp+rename for config/state file writes
- `WorktreeRegistry` -> `locked_register_with_limit` for worktree ops

## Acceptance Criteria

1. {Specific, measurable criterion with file:line if applicable}
2. {Specific, measurable criterion}
3. All modified script/code files (.sh, .py) have corresponding test files created/updated and registered in test-config.json

### PII Handling Review

- [ ] Does this task involve code that handles PII? (Y/N)
- [ ] If Y: Direct PII check -- no hardcoded PII in source/tests/comments (emails, names, tokens, IPs)
- [ ] If Y: Code logic review -- PII-handling code follows security standards:
  - Encryption at rest and in transit
  - Proper hashing (bcrypt/argon2 for passwords, not MD5/SHA1)
  - Input sanitization and validation
  - Logging redaction (no PII in logs)
  - Access controls on PII data stores
- [ ] If Y: Reviewed against OWASP Top 10 and industry standards (GDPR, SOC 2)

### Criteria Status

<!-- Pipeline column mapping:
     FEAT/FIX/RFCT/CICD/HTFX/CHOR: DEV | REV | QA
     DOCS:                          DOCS | REV
     TEST:                          TEST | REV | QA
     PLAN/SPKE:                     PLAN | REV
     Replace columns to match task work_type before use. -->

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DEV -> WS-REV -> WS-QA (default for FEAT/FIX/RFCT/CICD/HTFX/CHOR; replace with pipeline-appropriate stages per comment above)

| # | Criterion | DEV | REV | QA | Notes |
|---|-----------|-----|-----|-----|-------|
| 1 | {criterion text} | -- | -- | -- | |
| 2 | {criterion text} | -- | -- | -- | |
| 3 | {criterion text} | -- | -- | -- | |

## Dependencies

### Blocked By

- None

### Blocks

- None

## Verification

### Automated

- [ ] {Test or script that validates the change}
- [ ] Test coverage validation passes (bash .codeflow/testing/lib/test-coverage.sh --audit)

### Manual

- [ ] {Human verification step}

## Stage Reports

<!-- Pipeline stage report mapping:
     FEAT/FIX/RFCT/CICD/HTFX/CHOR: DEV Report | REV Report | QA Report
     DOCS:                          DOCS Report | REV Report
     TEST:                          TEST Report | REV Report | QA Report
     PLAN/SPKE:                     PLAN Report | REV Report
     Replace subsection headings and agent references to match task work_type.
     Omit QA Report for DOCS and PLAN/SPKE pipelines. -->

### DEV Report

> Populated by cf-development before STAGE-COMPLETE: WS-DEV
> (For DOCS pipeline: DOCS Report, populated by cf-documentation. For PLAN/SPKE: PLAN Report, populated by cf-planning. For TEST: TEST Report, populated by cf-quality-assurance.)

**Implementation Summary:**
{What was built/changed, key design decisions}

**Files Changed:**

| File | Action | Lines | Description |
|------|--------|-------|-------------|
| {path} | created/modified | {n} | {what changed} |

**Test Results:**

- Tests run: {command}
- Result: {passed}/{total} passed, {failed} failed
- Coverage: {n}% (threshold: {n}%)

**Deviations from Approach:**
{Any deviations from the planned approach and why, or "None"}

### REV Report

> Populated by cf-review before STAGE-COMPLETE: WS-REV

**Review Mode:** {CODE_REVIEW | DESIGN_REVIEW | DOCUMENTATION_REVIEW | TEST_REVIEW}
**Verdict:** {APPROVED | CHANGES_REQUESTED}
**Reviewer iterations:** {n} (initial + {n-1} rework cycles)

#### Dimensional Assessment

<!-- Mark N/A for dimensions not applicable to the review mode. See applicability matrix.
     Base dimensions (all modes): Functional Correctness, Security, Standards Compliance, PII Check, Scope Compliance
     CODE adds: Concurrency Safety, Error Handling, Resource Management, Test Quality, API Design
     DESIGN adds: API Design, Problem Statement, Architecture Soundness, Trade-off Analysis
     DOCS adds: Accuracy, Completeness, Examples Tested
     TEST adds: Concurrency Safety, Error Handling, Resource Management, Test Quality, Test Independence, Edge Cases -->

| Dimension | Verdict | Key Evidence |
|-----------|---------|-------------|
| Functional Correctness | {PASS/FAIL} | {brief evidence or file:line} |
| Security | {PASS/FAIL} | {brief evidence} |
| Concurrency Safety | {PASS/FAIL/N/A} | {brief evidence} |
| Error Handling | {PASS/FAIL/N/A} | {brief evidence} |
| Resource Management | {PASS/FAIL/N/A} | {brief evidence} |
| Test Quality | {PASS/FAIL/N/A} | {brief evidence} |
| Standards Compliance | {PASS/FAIL} | {brief evidence} |
| API Design | {PASS/FAIL/N/A} | {brief evidence} |
| PII Check | {PASS/FAIL} | {brief evidence} |
| Scope Compliance | {PASS/FAIL} | {brief evidence} |
| Problem Statement | {PASS/FAIL/N/A} | {DESIGN only} |
| Architecture Soundness | {PASS/FAIL/N/A} | {DESIGN only} |
| Trade-off Analysis | {PASS/FAIL/N/A} | {DESIGN only} |
| Accuracy | {PASS/FAIL/N/A} | {DOCS only} |
| Completeness | {PASS/FAIL/N/A} | {DOCS only} |
| Examples Tested | {PASS/FAIL/N/A} | {DOCS only} |
| Test Independence | {PASS/FAIL/N/A} | {TEST only} |
| Edge Cases | {PASS/FAIL/N/A} | {TEST only} |

#### Findings Log

| # | Severity | Finding | File:Line | Iteration | Resolution |
|---|----------|---------|-----------|-----------|------------|
| 1 | {CRITICAL/MAJOR/MINOR/NOTE} | {description} | {file:line} | {1/2/3} | {RESOLVED/OPEN} |

#### Rework History

| Iteration | Trigger | Changes Requested | Changes Made | Re-review Verdict |
|-----------|---------|-------------------|-------------|-------------------|
| 1 | Initial review | {n} findings | N/A | {APPROVED/CHANGES_REQUESTED} |

### QA Report

> Populated by cf-quality-assurance before STAGE-COMPLETE: WS-QA

**Verdict:** {PASS | FAIL}
**Runner Mode:** {essential | standard | full}

#### Test Execution

| Suite | Passed | Failed | Skipped | Duration |
|-------|--------|--------|---------|----------|
| {suite name} | {n} | {n} | {n} | {time} |

**Coverage:** {n}% (threshold: {n}%)

#### Acceptance Verification

| # | Criterion | Method | Result | Evidence |
|---|-----------|--------|--------|----------|
| 1 | {criterion} | {test/inspection/both} | {PASS/FAIL} | {test name or file:line} |

#### Regressions

{None detected | List with details}

#### QA Retry History (if applicable)

| Retry | Trigger | Failures | Fix Applied | Re-test Result |
|-------|---------|----------|-------------|----------------|
| 1 | Initial QA | {n} failures | N/A | {PASS/FAIL} |

## Notes

> **Standard Requirements (all tasks):**
>
> **Testing:** MANDATORY 85%+ per-file code coverage via
> `cargo llvm-cov` (Rust) or equivalent. Tests MUST be in
> the same file as implementation (`#[cfg(test)] mod tests`
> for Rust). Exceptions require documented technical
> justification consulted with user.
>
> **No Unsafe:** Zero `unsafe` blocks in production code.
> The only existing unsafe (libc::kill in autorun abort) is
> grandfathered -- do NOT add more.
>
> **Code Quality:** Follow DRY -- extract shared utilities,
> no copy-paste. Modular architecture with clear boundaries.
> Consider declarative/reactive patterns where appropriate.
> Question duplication before implementing.
>
> **Concurrency:** For any shared-state operations, consider
> sequential/parallel/concurrent access scenarios. Plan for
> race conditions. Use appropriate locking mechanisms
> (file_lock.rs, locked_binary_rmw, SurrealDB transactions).
> Document strategy in Concurrency Considerations section.
>
> **Pipeline:** Follow work type pipeline stages per CLAUDE.md
> Section 6. Do NOT skip WS-REV or WS-QA for code tasks.
>
> **Status Updates:** After completion, update BOTH:
> 1. Task markdown frontmatter (status, stage, stage_status,
>    started_at, completed_at, updated_at)
> 2. Epic markdown task table row (status column)
> If this is the final task, update epic status to complete.
>
> **Validation:** Run `codeflow validate task <path>` on the
> task file before committing.

{Implementation hints, edge cases, known pitfalls, or references to related decisions.}
