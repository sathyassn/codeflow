---
description: "Run test suite and verify coverage"
argument-hint: "--mode <mode> [--coverage] [--only <target>] [--skip <target>] [--report]"
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
codeflow test --mode <mode> [--coverage] [--only <target>] [--skip <target>] [--report]
```

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
| `--coverage` | Enable coverage collection and enforcement against configured per-target thresholds | false |
| `--only <target>` | Run only the named target (name must match a target in `test-config.json`) | all targets |
| `--skip <target>` | Skip the named target and run all others | none skipped |
| `--report` | Emit a structured CTRF report for downstream consumption | false |

**Test Runner Mode Mapping:**

| Flag Combination | What Runs |
|-----------------|-----------|
| `--mode essential` | Essential subset per `test-config.json` modes |
| `--mode full` | All configured suites including slow integration |
| `--mode full --coverage` | All suites with per-target coverage enforcement |
| `--mode full --coverage --report` | Full run + coverage + structured CTRF output |

**Examples:**

```bash
# Standard QA gate (default — full run with coverage)
codeflow test --mode full --coverage

# Essential subset only (fast check during development)
codeflow test --mode essential

# Full run, single target
codeflow test --mode full --only rust-core

# Full run, skip one target
codeflow test --mode full --skip python-tools

# Full run with coverage and structured report
codeflow test --mode full --coverage --report
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
| Test runner | `codeflow test --mode full --coverage` |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF4-EXECUTE | Stage: WS-QA | Teammate: cf-quality-assurance

/cf-test invoked
    |
    v
Parse flags (--coverage, --type)
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

- Parse `--coverage` flag (boolean)
- Parse `--type` flag (unit/integration/e2e/all, default: all)
- Map to test runner mode (essential/standard/full)

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
    - Test runner mode: `standard` / `essential` / `full`
    - Coverage flag: include coverage report if requested
    - `"Run tests using: codeflow test --mode full --coverage"`
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
- Show coverage report (if `--coverage` flag used)
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
| cf-quality-assurance | test-runner-modes | Mode selection (essential/standard/full) |
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
  - coverage_pct: percentage (if `--coverage` used)
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
QA Gate: Running standard test suite
Mode: WS-QA (read-only verification)
Runner: standard (CRITICAL + HIGH priority)

## QA Verdict
Verdict: PASS
Test Results: 547/547 passed, 0 failed, 3 skipped
Runner Mode: standard

Acceptance Criteria:
  [x] OAuth2 provider configuration loads correctly
  [x] Token exchange returns valid JWT
  [x] Invalid credentials return 401

Regressions: None detected

Next: All tests passing. Proceed to PR creation with /cf-ship
```

**Example 2: Full Run with Coverage**

```bash
/cf-test --type all --coverage
```

Output:

```text
QA Gate: Running full test suite with coverage
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
QA Gate: Running standard test suite
Mode: WS-QA (read-only verification)
Runner: standard

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
- [Test runner](codeflow test --mode full --coverage) — unified CLI entry point for all suites
- [cf-develop command](cf-develop.md)
- [cf-review command](cf-review.md)
- [cf-ship command](cf-ship.md)
