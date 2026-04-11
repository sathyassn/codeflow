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

## Deliverables

> **MANDATORY:** Every task must specify what is being delivered, where it integrates, and the expected outcome. Vague deliverables like "update code" are insufficient.

| Deliverable | Type | Location | Integration Point |
|-------------|------|----------|-------------------|
| {what is delivered} | {feature/fix/config/doc/test} | {file path or system} | {what consumes/uses this} |

**Expected Outcome:** {What behavior/capability exists after this task is complete that did not exist before}
**Deployment:** {How this reaches its consumers -- PR merge, config reload, manual step, etc.}

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

> **Chain-coverage requirement:** Acceptance criteria MUST cover all links in the implementation chain. A criterion that verifies only creation but not content, or only happy-path but not error handling, is incomplete. For every acceptance criterion, ask: does this verify the FULL delivery chain?
>
> **The 8-link delivery chain (verify ALL that apply):**
>
> | Link | What to Verify |
> |------|---------------|
> | 1. Creation | File/function/config entry exists at correct path |
> | 2. Content | Required fields/logic/text present and correct |
> | 3. Error handling | Failure modes produce expected output/exit codes |
> | 4. Integration | Component wired into caller/consumer/config correctly |
> | 5. Testing | Test file exists, is registered, and exercises the behavior |
> | 6. Coverage | 85%+ per-file coverage threshold met |
> | 7. Standards | Lint/format/style checks pass (shellcheck, ruff, clippy, markdownlint) |
> | 8. Verification | Observable behavior confirmed by running the code, not just reading it |

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
     FEAT/FIX/RFCT/CICD/HTFX/CHOR: DEV | SEC | REV | QA
     DOCS:                          DOCS | REV
     TEST:                          TEST | REV | QA
     PLAN/SPKE:                     PLAN | REV
     Replace columns to match task work_type before use. -->

> Legend: -- Not evaluated | DONE Implemented | PASS Verified passing | FAIL Verified failing | PARTIAL Partially met | N/A Not applicable
> Pipeline: WS-DEV -> WS-SEC -> WS-REV -> WS-QA (default for FEAT/FIX/RFCT/CICD/HTFX/CHOR; replace with pipeline-appropriate stages per comment above)

| # | Criterion | DEV | SEC | REV | QA | Notes |
|---|-----------|-----|-----|-----|-----|-------|
| 1 | {criterion text -- copy VERBATIM from YAML frontmatter acceptance array, do NOT paraphrase} | -- | -- | -- | -- | |
| 2 | {criterion text -- copy VERBATIM from YAML frontmatter acceptance array, do NOT paraphrase} | -- | -- | -- | -- | |
| 3 | {criterion text -- copy VERBATIM from YAML frontmatter acceptance array, do NOT paraphrase} | -- | -- | -- | -- | |

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
     FEAT/FIX/RFCT/CICD/HTFX/CHOR: DEV Report | SEC Report | REV Report | QA Report
     DOCS:                          DOCS Report | REV Report
     TEST:                          TEST Report | REV Report | QA Report
     PLAN/SPKE:                     PLAN Report | REV Report
     Replace subsection headings and agent references to match task work_type.
     Omit QA Report for DOCS and PLAN/SPKE pipelines.

     FINDING RESOLUTION POLICY: All stage findings must be resolved before STAGE-COMPLETE.
     There are no non-blocking observations — every identified issue is a finding that
     requires resolution or explicit team lead escalation. Severity indicates fix priority,
     not whether a fix is required. Do not use phrases like "non-blocking observation",
     "note for future work", "acceptable gap", or "informational only". -->

### DEV Report

> Populated by cf-development before STAGE-COMPLETE: WS-DEV
> (For DOCS pipeline: DOCS Report, populated by cf-documentation. For PLAN/SPKE: PLAN Report, populated by cf-planning. For TEST: TEST Report, populated by cf-quality-assurance.)

**Implementation Summary:**
{What was built/changed, key design decisions}

**Files Changed:**

| File | Action | Lines | Description |
|------|--------|-------|-------------|
| {path} | created/modified | {n} | {what changed} |

**Test Stats (draft — verified by WS-QA)**

#### 1. Pass Status

| Suite | Passed | Failed | Skipped |
|-------|--------|--------|---------|
| cargo test --workspace | {n} | 0 | 0 |

#### 2. Workspace Coverage

| Crate | Coverage | Threshold | Status |
|-------|----------|-----------|--------|
| codeflow-core | {n}% | {n}% | PASS/FAIL |
| codeflow-cli | {n}% | {n}% | PASS/FAIL |

#### 3. Modified File Coverage

| File | Coverage | Threshold | Status |
|------|----------|-----------|--------|
| {path} | {n}% | 85% | PASS/FAIL |

#### Code Path Audit

<!-- MANDATORY: Trace every code path through changed/new code. cf-review will independently
     verify this trace. Missing paths → findings at review. ALL findings are BLOCKING
     regardless of severity — zero tolerance. -->

| Entry Point | Path Type | Path Description | Outcome | Verified |
|-------------|-----------|------------------|---------|----------|
| {caller or trigger} | Success | {conditions → branches → result} | {return value/side effect} | Yes |
| {caller or trigger} | Error | {conditions → error → propagation} | {user-visible result} | Yes |
| {caller or trigger} | Edge | {boundary condition → behavior} | {result} | Yes |

**Unhandled paths identified and fixed:** {count}
**Silent failure check:** {result}
**Resource cleanup verification:** {result}
**Integration chain:** {upstream/downstream verification result}

**Deviations from Approach:**
{Any deviations from the planned approach and why, or "None"}

### SEC Report

> Populated by cf-security before STAGE-COMPLETE: WS-SEC

**Verdict:** {PASS | FAIL}
**Scope:** {files scanned}

#### OWASP Checklist

| # | Category | Result | Evidence |
|---|----------|--------|----------|
| A01 | Broken Access Control | {PASS/FAIL/N/A} | {file:line or justification} |
| A02 | Cryptographic Failures | {PASS/FAIL/N/A} | {evidence} |
| A03 | Injection | {PASS/FAIL/N/A} | {evidence} |
| A04 | Insecure Design | {PASS/FAIL/N/A} | {evidence} |
| A05 | Security Misconfiguration | {PASS/FAIL/N/A} | {evidence} |
| A06 | Vulnerable Components | {PASS/FAIL/N/A} | {evidence} |
| A07 | Authentication Failures | {PASS/FAIL/N/A} | {evidence} |
| A08 | Data Integrity Failures | {PASS/FAIL/N/A} | {evidence} |
| A09 | Logging & Monitoring | {PASS/FAIL/N/A} | {evidence} |
| A10 | SSRF | {PASS/FAIL/N/A} | {evidence} |

#### Findings

| # | Severity | Category | Finding | File:Line | Resolution |
|---|----------|----------|---------|-----------|------------|
| 1 | {CRITICAL/HIGH/MEDIUM} | {category} | {description} | {file:line} | {OPEN/RESOLVED} |

#### Security Code Path Audit

| Security Control | Paths Traced | Bypass Found | Finding |
|-----------------|-------------|-------------|---------|
| {validation/access check/etc.} | {n} | {Yes: description / No} | {--/finding ref #} |

**Error path security:** {All error paths maintain security controls / Gaps found: {description}}
**TOCTOU check:** {No race conditions found / Found: {description}}
**Defense-in-depth:** {Multiple layers verified / Gaps: {description}}

#### Security Red Team Assessment

**Attack chains attempted:** {n}
**Exploitable chains found:** {n}

| # | Entry Point | Attack Type | Exploit Chain | Blocked By | Bypass Found | Severity |
|---|------------|-------------|--------------|-----------|-------------|----------|
| 1 | {input source} | {injection/traversal/bypass/etc.} | {step → step → target} | {control or "NONE"} | {Yes: detail / No} | {CRITICAL/HIGH/MEDIUM} |

**Blast radius:** {contained to function / extends to session / extends to system}

**Confidence Score:** {0-100} -- {brief rationale}

### REV Report

> Populated by cf-review before STAGE-COMPLETE: WS-REV

**Review Mode:** {CODE_REVIEW | DESIGN_REVIEW | DOCUMENTATION_REVIEW | TEST_REVIEW}
**Verdict:** {APPROVED | CHANGES_REQUESTED}
**Reviewer iterations:** {n} (initial + {n-1} rework cycles)

#### Dimensional Assessment

<!-- Mark N/A for dimensions not applicable to the review mode. See applicability matrix.
     Base dimensions (all modes): Functional Correctness, Security, Standards Compliance, PII Check, Scope Compliance
     CODE adds: Concurrency Safety, Error Handling, Code Path Completeness, Red Team Resilience, Resource Management, Test Quality, API Design
     DESIGN adds: API Design, Problem Statement, Architecture Soundness, Trade-off Analysis
     DOCS adds: Accuracy, Completeness, Examples Tested
     TEST adds: Concurrency Safety, Error Handling, Code Path Completeness, Red Team Resilience, Resource Management, Test Quality, Test Independence, Edge Cases -->

| Dimension | Verdict | Key Evidence |
|-----------|---------|-------------|
| Functional Correctness | {PASS/FAIL} | {brief evidence or file:line} |
| Security | {PASS/FAIL} | {brief evidence} |
| Concurrency Safety | {PASS/FAIL/N/A} | {brief evidence} |
| Error Handling | {PASS/FAIL/N/A} | {brief evidence} |
| Code Path Completeness | {PASS/FAIL/N/A} | {paths traced, gaps found} |
| Red Team Resilience | {PASS/FAIL/N/A} | {scenarios tested, breaks found} |
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

#### Code Path Audit

<!-- MANDATORY for CODE_REVIEW and TEST_REVIEW. Independently trace all paths;
     cross-reference against DEV audit. Gaps → findings. ALL findings BLOCKING — zero tolerance. -->

**Paths independently traced:** {n}
**Paths matching developer's audit:** {n}
**Gaps found in developer's audit:** {n}

| Entry Point | Path | Reviewer Finding | Dev Audit Match | Severity |
|-------------|------|-----------------|-----------------|----------|
| {caller} | {success/error/edge} | {correct / gap: description} | {Yes/Missing/Mismatch} | {--/CRITICAL/MAJOR} |

**Error propagation chain verified:** {Yes/No — details}
**Silent failure check:** {result}

#### Red Team Assessment

<!-- MANDATORY for CODE_REVIEW and TEST_REVIEW. N/A for DESIGN_REVIEW and DOCUMENTATION_REVIEW.
     ALL findings BLOCKING regardless of severity — zero tolerance. -->

**Adversarial scenarios tested:** {n}
**Scenarios that broke implementation:** {n}

| # | Category | Scenario | Setup → Action → Result | Impact | Severity |
|---|----------|----------|------------------------|--------|----------|
| 1 | {Input/State/Sequence/Assumption} | {description} | {concrete steps} | {data loss/crash/wrong result/etc.} | {CRITICAL/MAJOR/MINOR} |

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
> All three Test Stats sections are REQUIRED. A QA Report missing any section is incomplete and the verdict is automatically FAIL.

**Verdict:** {PASS | FAIL}
**Runner Mode:** {essential | standard | full}

#### 1. Overall Test Pass Status

- Command: `cargo test --workspace --no-fail-fast`
- Result: {n} passed, 0 failed, 0 skipped
- New tests added: {n}
- Runs: {n} consecutive clean runs (minimum 2)

| Suite | Passed | Failed | Skipped | Duration |
|-------|--------|--------|---------|----------|
| {suite name} | {n} | {n} | {n} | {time} |

#### 2. Overall Coverage

> Run `cargo llvm-cov --manifest-path codeflow-cli/Cargo.toml --workspace` to generate.

- Workspace: {n}%
- CLI crate: {n}%
- Core crate: {n}%

##### Exempted Files (below 85%)

All project-wide coverage exceptions from test-config.json conventions.exceptions[].

| File | Coverage | Configured Threshold | Reason |
|------|----------|---------------------|--------|
| {path} | {n}% | {n}% | {reason from codeflow-cli/config/testing/test-config.json} |

#### 3. Modified File Coverage

> Per-file coverage for files modified in this PR only. Each must be >= 85%.

| File | Coverage | Threshold | Status |
|------|----------|-----------|--------|
| {path} | {n}% | 85% | PASS/FAIL |

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

### Confidence Score

> **Hard gate at PF5-VERIFY.** The team lead MUST NOT mark PF5-TSK-02 complete unless ALL pipeline stages report a confidence score of 95 or higher. A score below 95 from any stage is a rework trigger regardless of verdict.

| Stage | Agent | Score (0-100) | Rationale |
|-------|-------|--------------|-----------|
| WS-DEV | cf-development | {n} | {brief rationale} |
| WS-REV | cf-review | {n} | {brief rationale} |
| WS-QA | cf-quality-assurance | {n} | {brief rationale -- omit if pipeline has no WS-QA} |

**Scoring guide:** 95-100 = every acceptance criterion verifiably met with evidence; 80-94 = criteria met but some evidence thin or untested path exists; below 80 = known gaps remain. Round down when uncertain.

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
