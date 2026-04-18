---
description: "Run test suite and verify coverage"
argument-hint: "--mode <mode> [--only <target>] [--skip <target>]"
---

# /cf-test Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch
- 🔧 decide: At decision points (test scope, verdict assessment)
- 🔧 respond-organized: When presenting QA results

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Run the quality gate (WS-QA) by dispatching test execution and acceptance verification to the cf-quality-assurance teammate, producing a PASS or FAIL verdict.

**Usage:**

```text
codeflow test --mode <mode> [--only <target>] [--skip <target>]
```

Coverage is driven entirely by `.codeflow/config/testing/test-config.json` per-target rules; there is no CLI flag to toggle it.

**Use When:**

- WS-REV stage approved and pipeline includes WS-QA (FEAT, FIX, RFCT, CICD)
- Need to verify implementation against acceptance criteria
- Want to run the full test suite with coverage analysis
- Verifying no regressions before PR creation

**Do Not Use When:**

- Writing tests as the primary deliverable (team lead spawns cf-quality-assurance in WS-TEST mode directly)
- Running a quick self-test during development (cf-development handles its own test runs)
- Code hasn't been reviewed yet (complete `/cf-review` first)

### Pipeline Position

```text
Phase: PF4-EXECUTE | Stage: WS-QA
Pipeline: /cf-review --> /cf-test --> /cf-ship --> /cf-cleanup
                          ^ you are here
Previous: /cf-review (APPROVED verdict)
Next: /cf-ship (on PASS) or back to /cf-develop (on FAIL)
```

---

## 2. Arguments & Flags

**Arguments:**

This command takes no positional arguments. Test scope is determined from the current session context (changed files, work type, acceptance criteria).

**Flags:**

| Flag | Description | Default |
|------|-------------|---------|
| `--mode essential\|full` | Test depth: `essential` runs the critical subset; `full` runs all suites including slow integration | required |
| `--only <target>` | Run only the named target (name must match a target in `test-config.json`) | all targets |
| `--skip <target>` | Skip the named target and run all others | none skipped |
| `--only-tag <tags>` | Run only targets tagged with these priority tags (comma-separated: `critical`, `high`, `medium`, `low`). Composes with `--only` (AND). Targets without tags are excluded when this flag is non-empty. | all tags |
| `--skip-tag <tags>` | Skip targets tagged with these priority tags (comma-separated). Composes with `--skip` (AND). Wins over `--only-tag` on conflict. | none skipped |

Coverage collection, per-target thresholds, and report format are configured per-target in `.codeflow/config/testing/test-config.json`. There is no CLI flag to toggle coverage — the engine reads `coverage.format`/`coverage.rules`/`report.format` and applies them automatically. A `full` mode run always produces the canonical 5-section PR body; the `report show` subcommand renders saved reports in human or JSON form.

**Test Runner Mode Mapping:**

| Flag Combination | What Runs |
|-----------------|-----------|
| `--mode essential` | Essential subset per `test-config.json` modes |
| `--mode full` | All configured suites with coverage enforcement + PR-body rendering |

**`structural-check` Subcommand:**

The `structural-check` subcommand runs the bidirectional structural integrity check on every target that declares a `structural` block in the config. It verifies that every source file maps to a test file and vice versa. Exits 1 when any finding is reported.

```text
codeflow test structural-check [--format human|json] [--only <target>]
```

| Flag | Description | Default |
|------|-------------|---------|
| `--format human\|json` | Output format | `human` |
| `--only <target>` | Limit to a single target by name | all targets with structural blocks |

Use `structural-check` as a pre-commit validation step to catch missing test files or orphaned test files before running the full suite.

**Examples:**

```bash
# Standard QA gate (full run with coverage per test-config.json)
codeflow test --mode full

# Essential subset only (fast check during development)
codeflow test --mode essential

# Full run, single target
codeflow test --mode full --only rust-core

# Full run, skip one target
codeflow test --mode full --skip python-tools

# Run only critical and high priority targets
codeflow test --mode full --only-tag critical,high

# Run all targets except low priority
codeflow test --mode full --skip-tag low

# Run only high priority targets within a specific target
codeflow test --mode full --only rust-core --only-tag high

# Structural integrity check (all targets with structural blocks)
codeflow test structural-check

# Structural check for a single target, JSON output
codeflow test structural-check --only rust-core --format json

# Inspect the saved report after a run (human table or JSON)
codeflow test report show --format human
codeflow test report show --format json
```

---

## 3. Prerequisites

**Required State:**

- [ ] PathFlow session initialized (PF1 through PF3 complete)
- [ ] WS-REV stage completed with APPROVED verdict
- [ ] Pipeline includes WS-QA (FEAT, FIX, RFCT, CICD work types)
- [ ] Changes committed (tests run against committed state)
- [ ] cf-knowledge-layer teammate available (for acceptance criteria and verdict recording)

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-quality-assurance | Test execution and verdict delivery |
| cf-knowledge-layer | Acceptance criteria lookup, verdict recording |
| Test runner | `codeflow test --mode full` |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF4-EXECUTE | Stage: WS-QA | Teammate: cf-quality-assurance

/cf-test invoked
    |
    v
Parse flags (--mode, --only, --skip)
    |
    v
Verify WS-REV approved                              [cf-knowledge-layer]
    |
    v
Load acceptance criteria from WorkGraph              [cf-knowledge-layer]
    |
    v
Determine test scope from changed files
    |
    v
Spawn cf-quality-assurance teammate                  [cf-quality-assurance]
(WS-QA mode: read-only verification)
    |
    v
Teammate runs test suite                             [cf-quality-assurance]
    |
    v
Teammate verifies acceptance criteria                [cf-quality-assurance]
    |
    v
Teammate checks for regressions                     [cf-quality-assurance]
    |
    v
Teammate delivers verdict                           [cf-quality-assurance]
    |
    v
Process verdict --------+-------------------+
    |                    |
    v                    v
  PASS                 FAIL
    |                    |
    v                    v
Record verdict       Send failure details
in WorkGraph         to cf-development               [cf-knowledge-layer]
    |                    |
    v                    v
Present results      Route back to WS-DEV
                     for targeted fixes
    |                    |
    v                    v
Next: /cf-ship       Next: /cf-develop
(PF6-COMPLETE)       (rework, max 2 retries)
```

### 4.2 Execution Steps

**Step 1: Parse Flags**

- Parse `--mode`, `--only`, `--skip` flags (coverage is driven by `test-config.json`, not a CLI flag)
- Map to test runner mode (essential / full)

**Step 2: Verify Review Approval**

- Query cf-knowledge-layer:
  - `"LEAD: query-stage-status -- stage=WS-REV, session=current"`
- Verify WS-REV verdict is APPROVED
- If not approved: error with guidance to complete review first

**Step 3: Load Acceptance Criteria**

- Query cf-knowledge-layer:
  - `"LEAD: query-acceptance-criteria -- task=current"`
- Extract testable criteria for verification checklist
- Include any task-specific test requirements

**Step 4: Determine Test Scope**

- Identify changed files from the current branch diff (via cf-git-operations or session context)
- Map changed files to relevant test categories
- Determine targeted tests (in addition to full suite)

**Step 5: Assign to cf-quality-assurance Teammate**

- Ensure cf-quality-assurance is spawned (check team roster)
- If not alive: spawn via Task tool with instruction to read `.claude/agents/cf-quality-assurance.md`
- Assign task via SendMessage:
  - recipient: `"cf-quality-assurance"`
  - content: QA gate assignment with full context:
    - Mode: WS-QA (read-only verification, source code is NOT to be modified)
    - Acceptance criteria (specific, testable items)
    - Changed files list (for targeted testing)
    - Test runner mode: `essential` / `full` (coverage enforcement is always applied in `full` mode per test-config.json — no separate flag to toggle)
    - `"Run tests using: codeflow test --mode full"`
    - `"Deliver verdict using standard QA verdict format. If FAIL, send specific failure details to cf-development."`
- Wait for teammate completion message

**Step 6: Process Verdict**

- Receive verdict from cf-quality-assurance:
  - `"QA: {PASS|FAIL} -- {passed}/{total} passed. {summary}"`

- **If PASS:**
  - Record verdict in WorkGraph via cf-knowledge-layer
  - Present pass results to user
  - Indicate next step: proceed to PF5-VERIFY

- **If FAIL:**
  - cf-quality-assurance sends failure details to cf-development:
    - `"QA-FAIL: {n} failures. {specific issues with file paths}"`
  - Record verdict in WorkGraph
  - Route work back to WS-DEV for targeted fixes
  - Track QA retry count (max 2)

**Step 7: Present Results**

- Show QA verdict (PASS or FAIL)
- Show test results: passed/total, failed count, skipped count
- Show acceptance criteria checklist (met/unmet)
- Show coverage report (always included in `--mode full` via test-config.json)
- Show next steps:
  - Pass: "All tests passing. Proceed to PR creation with `/cf-ship`"
  - Fail: "Failures routed to cf-development for fixes. Re-run `/cf-test` after fixes (retry {n}/2)"

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-quality-assurance | qa-quality-gate | Test execution and verdict delivery |
| cf-quality-assurance | verdict-format | Structured PASS/FAIL report |
| cf-quality-assurance | retry-behavior | Rework routing on FAIL |
| cf-quality-assurance | test-runner-modes | Mode selection (essential / full) |
| cf-knowledge-layer | query-stage-status | Verify WS-REV approved |
| cf-knowledge-layer | query-acceptance-criteria | Load testable criteria |
| cf-knowledge-layer | record-verdict | Store QA verdict in WorkGraph |
| cf-git-operations | Diff retrieval | Identify changed files for test scope |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before tool calls | Verify PathFlow phase consistency |
| PostToolUse:logging | After tool calls | Log QA operations |
| SubagentStop | cf-quality-assurance returns | Validate results, log session |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**Note:** cf-quality-assurance operates in WS-QA mode (read-only for source code). The edit-write hook allows test execution commands via Bash but blocks any source file modifications by the QA teammate. The teammate can only Read source files and execute test runners.

---

## 7. Memory Integration

### 7.1 QA Verdict Recording

**On Verdict Delivery:**

- Send to cf-knowledge-layer:
  - `"QA-COMPLETE: task={id}, verdict={PASS|FAIL}, passed={n}, failed={n}, total={n}"`
- Records QA event in WorkGraph:
  - stage: WS-QA
  - verdict: PASS or FAIL
  - test_passed: count
  - test_failed: count
  - test_total: count
  - coverage_pct: percentage (always populated for `--mode full` runs)
  - retry: retry iteration number

### 7.2 Retry Tracking

- Each FAIL verdict increments the QA retry counter
- Maximum QA retries: 2 (from `pathflow-config.json`)
- Retry flow: FAIL -> cf-development fixes -> re-run `/cf-test`
- If limit reached: escalate to team lead for decision

### 7.3 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | QA events log |
| 1 | `.state/db/codeflow.db` | Verdict state, retry tracking |
| 2 | `.claude/memory/` | Derived QA summaries |

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| WS-REV not approved | Review stage not completed or verdict was CHANGES_REQUESTED | Complete `/cf-review` first |
| No acceptance criteria | Task missing testable criteria | Query cf-knowledge-layer for task details; escalate if missing |
| codeflow binary not found | `codeflow` not in PATH | Check project setup; run `/cf-doctor` |
| Test infrastructure broken | Test helpers or fixtures unavailable | cf-quality-assurance escalates to team lead |
| QA retry limit exceeded | 2 FAIL cycles without achieving PASS | Escalate to team lead for decision |
| cf-quality-assurance not available | Teammate spawn failure | Retry spawn with fresh context |
| Flaky test detected | Non-deterministic test results | cf-quality-assurance re-runs to confirm; report as NOTE |

**Recovery Procedures:**

```text
ON "WS-REV not approved" error:
  1. Check WS-REV status via cf-knowledge-layer
  2. If not yet reviewed: run /cf-review first
  3. If CHANGES_REQUESTED: complete rework first, then re-review

ON "QA retry limit exceeded":
  1. Present 2-retry history to user
  2. Options:
     a. Override: accept with known failures documented
     b. Fix: developer investigates root cause with team lead
     c. End session: mark task blocked, proceed to PF7-END
  3. Record decision in WorkGraph

ON "Flaky test detected":
  1. cf-quality-assurance re-runs the specific test 3 times
  2. If inconsistent results: mark as flaky in verdict (NOTE severity)
  3. Flaky tests do not block PASS verdict unless they cover acceptance criteria
```

---

## 9. Examples

**Example 1: Standard QA Gate**

```bash
/cf-test
```

Output:

```text
QA Gate: Running essential test suite
Mode: WS-QA (read-only verification)
Runner: essential (critical subset per test-config.json)

## QA Verdict
Verdict: PASS
Test Results: 547/547 passed, 0 failed, 3 skipped
Runner Mode: essential

Acceptance Criteria:
  [x] OAuth2 provider configuration loads correctly
  [x] Token exchange returns valid JWT
  [x] Invalid credentials return 401

Regressions: None detected

Next: All tests passing. Proceed to PR creation with /cf-ship
```

**Example 2: Full Run (coverage enforced automatically via test-config.json)**

```bash
/cf-test --mode full
```

Output:

```text
QA Gate: Running full test suite (coverage via test-config.json)
Mode: WS-QA (read-only verification)
Runner: full (all priorities)

## QA Verdict
Verdict: PASS
Test Results: 1,555/1,555 passed, 0 failed, 0 skipped
Runner Mode: full
Coverage: 87% (target: 80%)

Acceptance Criteria:
  [x] All 5 criteria met

Next: All tests passing. Proceed to PR creation with /cf-ship
```

**Example 3: QA Failure with Rework**

```bash
/cf-test
```

Output:

```text
QA Gate: Running essential test suite
Mode: WS-QA (read-only verification)
Runner: essential

## QA Verdict
Verdict: FAIL
Test Results: 544/547 passed, 3 failed, 0 skipped

Acceptance Criteria:
  [x] OAuth2 provider configuration loads correctly
  [ ] Token exchange returns valid JWT -- FAILED
  [x] Invalid credentials return 401

Test Failures:
  | Test | Expected | Actual |
  |------|----------|--------|
  | test_token_exchange_success | JWT token | ConnectionError |
  | test_token_refresh | Refreshed token | TimeoutError |
  | test_token_expiry | Expired response | AssertionError |

Regressions: None (failures are in new code)

Required Fixes:
  1. Token exchange connection handling (src/auth/token-exchange.py:34)
  2. Missing timeout configuration (src/auth/token-exchange.py:56)

Failure details sent to cf-development.
Next: Fix issues, then re-run /cf-test (retry 1/2)
```

---

## 10. References

- [cf-quality-assurance agent](../agents/cf-quality-assurance.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-development agent](../agents/cf-development.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [Test runner](codeflow test --mode full) — unified CLI entry point for all suites
- [cf-develop command](cf-develop.md)
- [cf-review command](cf-review.md)
- [cf-ship command](cf-ship.md)
