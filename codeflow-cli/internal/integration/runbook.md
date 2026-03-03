# Integration Test Runbook: Full PathFlow Lifecycle

This runbook documents manual verification of the Go hook implementations
for the complete PathFlow session lifecycle. Run these steps when:

- Migrating from shell hooks to Go hooks (INF-TSK-021-022 cutover)
- Verifying a Go hook change did not break the pipeline
- Onboarding new contributors who want to understand the lifecycle

## Prerequisites

- `codeflow-cli` binary built: `cd codeflow-cli && go build ./cmd/codeflow/`
- Go 1.26+ installed
- No active Claude Code session (to avoid sentinel path conflicts)

## Running the Automated Tests

The primary verification is the automated integration test suite:

```bash
cd codeflow-cli

# Run integration tests with verbose output
go test -v -timeout 300s ./internal/integration/...

# Run with race detector
go test -race -timeout 300s ./internal/integration/...

# Run with coverage
go test -v -timeout 300s -coverprofile=integration.out ./internal/integration/...
go tool cover -html=integration.out -o integration.html
```

### Expected Output

All 10 top-level test functions should report PASS:

```text
--- PASS: TestFullPathFlowLifecycle
--- PASS: TestGateEnforcementIndependent
--- PASS: TestSecurityEnforcementIndependent
--- PASS: TestSentinelPipelineIndependent
--- PASS: TestTeamGuardIndependent
--- PASS: TestLedgerRoutingIndependent
--- PASS: TestTeammateModeIndependent
--- PASS: TestValidateIndependent
--- PASS: TestGitHooksIndependent
--- PASS: TestWorktreeIndependent
PASS
ok  github.com/codeflow/codeflow-cli/internal/integration
```

## Test Coverage

The integration tests cover:

### TestFullPathFlowLifecycle

Full end-to-end lifecycle test without a real Claude Code session:

1. **Session initialization** (`hooks/session.StartInit`): Creates session directory,
   pathflow-active flag, pathflow-phase-tasks.json checkpoint, codeflow-env.sh,
   and registers session in the ledger.

2. **Phase progression** (`pathflow.Checkpoint.RegisterTask/CompleteTask`): Simulates
   PF1 through PF7 task completion. Verifies phase sentinels created when all tasks done.

3. **Gate enforcement** (`hooks/gate.GateChecker.Check`): Verifies Edit/Write blocked
   before PF3, allowed after. Git push blocked before PF5+WS-REV.

4. **Stage sentinel creation** (`hooks/sentinel.CheckAndCreateStageSentinel`): Verifies
   WS-DEV, WS-REV, WS-QA sentinels created on STAGE-COMPLETE messages.

5. **Security enforcement** (`hooks/security.Checker.Check`): Verifies dangerous commands
   blocked, safe commands allowed.

6. **Team guard** (`hooks/team.CheckTeamDelete`): Verifies TeamDelete blocked during
   active PathFlow, allowed after PF6.

7. **Session cleanup** (`hooks/session.Cleaner.EndCleanup`): Verifies pathflow-active
   flag removed, session_end event written to sessions.jsonl.

8. **JSONL schema verification**: Verifies events use Go canonical schema
   (`event`/`timestamp` fields, NOT shell schema `type`/`ts`).

### TestGateEnforcementIndependent

Table-driven tests covering all gate types:

| Scenario | Expected |
|----------|----------|
| Edit before pf-3 | BLOCKED |
| Edit after pf-3 | ALLOWED |
| Write before pf-3 | BLOCKED |
| Write after pf-3 | ALLOWED |
| git commit before pf-3 | BLOCKED |
| git commit after pf-3 | ALLOWED |
| git push without pf-5 | BLOCKED |
| git push with pf-5 but no ws-rev | BLOCKED |
| git push with pf-5 AND ws-rev | ALLOWED |
| gh pr create without pf-5 | BLOCKED |
| gh pr create with pf-5 AND ws-rev | ALLOWED |
| cf-development spawn before pf-3 | BLOCKED |
| cf-review spawn before pf-3 | BLOCKED |
| cf-quality-assurance spawn after pf-3 | ALLOWED |
| cf-knowledge-layer spawn (function teammate) | ALLOWED (ungated) |
| SendMessage tool | ALLOWED (ungated) |
| Read tool | ALLOWED (ungated) |
| Glob tool | ALLOWED (ungated) |
| "git push" in commit message string | ALLOWED (not a real push command) |

### TestSecurityEnforcementIndependent

Table-driven security checks:

| Command | Expected |
|---------|----------|
| `rm -rf /` | BLOCKED |
| `rm -rf /*` | BLOCKED |
| `sudo apt-get install` | BLOCKED |
| `git push --force main` | BLOCKED |
| `git push --force-with-lease` | BLOCKED |
| `ls -la` | ALLOWED |
| `cat file.txt` | ALLOWED |
| `go test ./...` | ALLOWED |
| `echo hello` | ALLOWED |
| `grep -r pattern ./src` | ALLOWED |

### TestSentinelPipelineIndependent

Table-driven sentinel creation tests:

| Scenario | Expected |
|----------|----------|
| STAGE-COMPLETE: WS-DEV | ws-dev sentinel created |
| STAGE-COMPLETE: WS-TEST | ws-test sentinel created |
| STAGE-COMPLETE: WS-DOCS | ws-docs sentinel created |
| No STAGE-COMPLETE pattern | no sentinel created |
| WS-REV without prior primary stage | BLOCKED |
| WS-REV after ws-dev | ws-rev sentinel created |
| WS-QA without ws-dev or ws-test | BLOCKED |
| WS-QA after ws-test | ws-qa sentinel created |
| Non-SendMessage tool | no sentinel (passthrough) |
| Cross-phase registration without prior sentinel | ErrCrossPhaseBlock |
| Checkpoint register/complete full lifecycle | phase sentinel created |

### TestTeamGuardIndependent

| Scenario | Expected |
|----------|----------|
| TeamDelete with no pathflow-active | ALLOWED |
| TeamDelete with pathflow-active, no pf-6 | BLOCKED |
| TeamDelete with pathflow-active AND pf-6 | ALLOWED |
| HandlePostTeamDelete | removes pathflow-active flag |

### TestLedgerRoutingIndependent

| Event Type | Expected File |
|------------|---------------|
| session_end | sessions.jsonl |
| session_start | sessions.jsonl |
| session_progress | sessions.jsonl |
| task_created | work-graph.jsonl |
| epic_created | work-graph.jsonl |
| begin_work | work-graph.jsonl |
| memory_store | memory-events.jsonl |
| config_set | config.jsonl |
| phase_transition | pathflow-events.jsonl |
| stage_transition | pathflow-events.jsonl |
| session_register | pathflow-events.jsonl |
| Misrouted event | ErrMisroutedEvent |

### TestTeammateModeIndependent

| Scenario | Expected |
|----------|----------|
| Lead PID alive | StartInit returns IsTeammate=true, skips checkpoint init |
| No lead PID | StartInit returns IsTeammate=false, creates full session |

### TestValidateIndependent

Table-driven validation checks for task and epic validation subcommands:

| Scenario | Expected |
|----------|----------|
| Valid task markdown with all required fields | VALID (no error) |
| Task markdown missing required field | INVALID (validation error) |
| Valid epic markdown with all required fields | VALID (no error) |
| Epic markdown missing required field | INVALID (validation error) |

### TestGitHooksIndependent

Table-driven commit message validation checks:

| Commit Message | Expected |
|----------------|----------|
| `feat: add new feature` | VALID |
| `fix: correct bug` | VALID |
| `docs: update readme` | VALID |
| `test: add coverage` | VALID |
| `chore: update deps` | VALID |
| `not a conventional commit` | INVALID |
| `feat(scope): scoped type rejected` | INVALID |

### TestWorktreeIndependent

Table-driven worktree manager checks:

| Scenario | Expected |
|----------|----------|
| List worktrees (empty) | returns empty list, no error |
| List with filter (no matches) | returns empty list, no error |
| Get status of non-existent worktree | ErrNotFound |

## Schema Divergence Reference

The Go implementation uses a different JSONL schema than the shell hooks:

| Field | Shell (legacy) | Go (canonical) |
|-------|---------------|----------------|
| Event type field name | `type` | `event` |
| Timestamp field name | `ts` | `timestamp` |
| Session register event type | `session_metadata` | `session_register` |

Integration tests verify Go canonical schema exclusively. Historical shell
events will be normalized by INF-TSK-021-026.

## Troubleshooting

| Symptom | Cause | Fix |
|---------|-------|-----|
| `ErrCrossPhaseBlock` during test | Phase sentinel missing when registering next-phase task | Verify `CompleteTask` calls create the sentinel file |
| Test fails with "session dir not found" | Session ID not propagated to test setup | Check mock SessionStarter returns the same ID as pathflow-active flag |
| Race condition in parallel tests | Shared temp directory between parallel subtests | Ensure each subtest creates its own `t.TempDir()` |
| `sessions.jsonl` missing session_end event | EndCleanup not finding session ID | Check `codeflow-env.sh` contains correct CODEFLOW_SESSION_ID |
| Coverage below 85% | New code paths not exercised | Add test cases for missing branches |
