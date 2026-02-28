---
id: "epic-01KJHQCJSFC7W6MT0Q1GYEKQMH"
format_id: "INF-EPC-021"
title: "Go CLI Integration & Script Retirement"
summary: "Wire the existing Go CLI binary into the live CodeFlow workflow, migrate shell/Python scripts to Go subcommands, and retire legacy infrastructure"
status: planning
area_type: "INF"
work_type: "RFCT"
domain: "GENL"
is_ongoing: false
file_scope: ["codeflow-cli/", ".codeflow/scripts/", ".claude/hooks/codeflow/", ".claude/settings.json", ".codeflow/scripts/codeflow_py_lib/"]
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-27T00:00:00Z"
updated_at: "2026-02-27T00:00:00Z"
---

# INF-EPC-021: Go CLI Integration & Script Retirement

## Summary

Wire the existing Go CLI binary (`codeflow-cli/`) into the live CodeFlow workflow and retire the shell/Python scripts it replaces. The Go CLI already exists with db, session, doctor, config, init, update, and autorun commands built during INF-EPC-015. This epic is about INTEGRATION: adding new Go subcommands for hook operations, ledger writes, checkpoint management, and utility functions; updating `settings.json` to invoke the Go binary instead of shell scripts; and removing retired shell/Python infrastructure. The migration eliminates ~24,000 lines of shell, ~3,000 lines of Python, removes the python3 runtime dependency, fixes platform-specific bugs (macOS/Linux date, stat, flock), and reduces hook latency from ~300ms to ~10ms per tool call.

## Scope

### In Scope

- New Go subcommands: `codeflow ledger append`, `codeflow hooks *`, `codeflow pathflow *`, `codeflow sentinel *`, `codeflow validate *`, `codeflow internal *`
- Migrating all 22 hook scripts from shell to Go binary invocations
- Migrating state management scripts (ledger.sh, cf-pathflow-state.sh, cf-work-state.sh, memory.sh)
- Migrating security enforcement stack to consolidated Go binary
- Migrating Python scripts (claim, memory-store, stage-sync, crdt-rebuild) and codeflow_py_lib
- Migrating shell-lib utilities (common.sh, logging.sh, errors.sh, config.sh, validation.sh, ulid.sh)
- Migrating settings scripts, staging scripts, validation scripts
- Updating CLAUDE.md, agent definitions, command definitions, skills, and settings.json
- Test migration from shell test suite to Go tests
- Removing retired shell scripts, Python scripts, and associated test files
- Performance benchmarking of Go hooks vs shell hooks

### Out of Scope

- Rewriting the Go CLI core (already built in INF-EPC-015)
- Claude Code wrapping/exec (Phase 7)
- NX monorepo setup
- Package manager distribution
- Protection scripts requiring sudo (`cf-protect-resources.sh`, `cf-promote-protection.sh`, `cf-reload-protection.sh`) -- these stay as shell
- Web UI or dashboard
- CRDT implementation in Go (Loro integration deferred)

## Acceptance Criteria

### Phase B (Build) -- Tasks 001-014

- [ ] All Go subcommands created with full functionality matching shell/Python equivalents
- [ ] `codeflow ledger append` provides atomic, validated JSONL writes
- [ ] `codeflow hooks session-start init` implements all session initialization steps
- [ ] `codeflow hooks pre-tool-use gate-check` implements PathFlow phase enforcement
- [ ] `codeflow hooks pre-tool-use security` consolidates all 9 enforcement modules
- [ ] `codeflow pathflow checkpoint` implements checkpoint engine operations
- [ ] All Go subcommands for Python scripts (claim, memory-store, stage-sync, crdt-rebuild) created
- [ ] `make test-cover` passes in codeflow-cli with >= 85% coverage on all new business packages
- [ ] Go code follows idiomatic Go conventions per cf-go-standards across all packages
- [ ] `./codeflow test` passes (existing test suite not regressed)
- [ ] Shell/Python scripts remain UNCHANGED and operational during Phase B

### Phase C (Shadow Testing) -- Task 030

- [ ] Shadow test harness runs Go subcommands alongside shell scripts
- [ ] Output divergence detection and logging for each migrated hook
- [ ] Zero divergences observed across shadow testing cycle

### Phase D (JSONL Normalization) -- Task 026

- [ ] JSONL schema normalized before cutover

### Phase E (Single-Session Cutover) -- Tasks 015-024, 026-028 (025 cancelled, absorbed into 022)

- [ ] All 22 hook scripts replaced with Go binary invocations in `settings.json`
- [ ] All Python scripts (8) and codeflow_py_lib removed; no python3 runtime dependency
- [ ] All shell-lib scripts (6) retired after Go equivalents wired
- [ ] CLAUDE.md, agent definitions, command definitions, and skills updated to reference Go binary
- [ ] CI workflow updated to remove Python test dependencies and add Go test coverage

### Phase F (Post-Cutover Verification) -- Task 031

- [ ] Full PathFlow lifecycle test passes with Go hooks (PF1 through PF7)
- [ ] Hook latency measured and documented (target: < 50ms per hook, down from ~300ms)
- [ ] `codeflow doctor` reports clean state post-cutover
- [ ] One full session completed successfully with all Go hooks active

### PII Handling Review

- [x] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-021-001 | Build Go CLI for JSONL ledger writes | complete | critical |
| INF-TSK-021-002 | Build Go CLI for ULID generation and format ID | todo | critical |
| INF-TSK-021-003 | Build Go CLI for checkpoint engine | todo | critical |
| INF-TSK-021-004 | Build Go CLI for active task state and memory operations | todo | high |
| INF-TSK-021-005 | Build Go CLI for validation scripts | todo | high |
| INF-TSK-021-006 | Build Go CLI for security enforcement stack | todo | critical |
| INF-TSK-021-007 | Build Go CLI for pathflow gate hook | todo | critical |
| INF-TSK-021-008 | Build Go CLI for webfetch, team-guard, and gh-pr hooks | todo | high |
| INF-TSK-021-009 | Build Go CLI for sentinel pipeline hooks | todo | critical |
| INF-TSK-021-010 | Build Go CLI for session-start-init hook | todo | high |
| INF-TSK-021-011 | Build Go CLI for session-end-cleanup hook | todo | high |
| INF-TSK-021-012 | Build Go CLI for sentinel system script | todo | high |
| INF-TSK-021-013 | Wire codeflow session start into SessionStart hook | todo | high |
| INF-TSK-021-014 | Build Go equivalents for Python scripts and codeflow_py_lib | todo | high |
| INF-TSK-021-015 | Build Go CLI for shell-lib foundation | todo | normal |
| INF-TSK-021-016 | Build Go CLI for settings and staging scripts | todo | normal |
| INF-TSK-021-017 | Build Go CLI for logging hooks | todo | normal |
| INF-TSK-021-018 | Build Go CLI for edit-write, protected-resource, and user-prompt-submit hooks | todo | normal |
| INF-TSK-021-019 | Build Go CLI for worktree and report scripts | todo | normal |
| INF-TSK-021-020 | Build Go CLI for DB migration script | todo | normal |
| INF-TSK-021-021 | Update CLAUDE.md and agent definitions for Go CLI | todo | high |
| INF-TSK-021-022 | Single-session cutover: wire Go hooks, retire shell/Python scripts, update settings | todo | critical |
| INF-TSK-021-023 | Integration testing -- full PathFlow lifecycle with Go hooks | todo | critical |
| INF-TSK-021-024 | Performance benchmarking and hook latency verification | todo | high |
| INF-TSK-021-025 | SUPERSEDED -- Remove retired shell and Python infrastructure (absorbed into INF-TSK-021-022) | cancelled | high |
| INF-TSK-021-026 | Normalize JSONL schema and rebuild SQLite | todo | high |
| INF-TSK-021-027 | Update git hooks and CI workflows for post-migration compatibility | todo | normal |
| INF-TSK-021-028 | Update test-config.json business_packages with migration packages | todo | high |
| INF-TSK-021-029 | Align INF-EPC-021 task criteria with build-coexist-cutover strategy | complete | high |
| INF-TSK-021-030 | Shadow testing -- run Go alongside shell and verify output parity | todo | high |
| INF-TSK-021-031 | Post-cutover verification -- full lifecycle test with Go hooks | todo | high |
| INF-TSK-021-032 | Fix replace/migrate language in INF-EPC-021 build tasks | complete | high |

## Dependencies

### Blocked By

- INF-EPC-015: Go CLI Phase 6 (provides the base Go CLI binary with db, session, doctor, config, init, update, autorun commands). **Depends on: INF-EPC-015 (Go CLI Build-Out) -- the Go CLI binary must be feature-complete before migration can begin.**
- INF-EPC-013: Session end cleanup fixes (ensures hooks use correct session ID before migration)

### Blocks

- Phase 7: NX Monorepo + Claude Code wrapping (requires stable Go CLI with all hooks migrated)

## Technical Notes

### Migration Strategy

The migration follows the **Build-Coexist-Cutover** strategy (documented in `.codeflow/docs/analysis/go-cli-migration-comprehensive.md` Section 10.1) with six phases:

1. **Phase A (Prerequisites)** -- INF-EPC-015 (Go CLI binary built), INF-EPC-013 (session ID bugs fixed)
2. **Phase B (Build)** -- Tasks 001-014: Create all Go subcommands with full functionality and >= 85% test coverage. Shell/Python scripts remain UNCHANGED and active. No settings.json modifications. Go code is built and tested but not wired in.
3. **Phase C (Shadow Testing)** -- Task 030: Run Go subcommands alongside shell scripts, compare output, log divergences. Validates behavioral parity before cutover.
4. **Phase D (JSONL Normalization)** -- Task 026: Normalize JSONL ledger/log schema before cutover to ensure Go binary reads/writes the correct format.
5. **Phase E (Single-Session Cutover)** -- Tasks 015-024, 026-028 (025 cancelled, absorbed into 022): In a single session, update all settings.json hook entries from shell to Go binary, delete retired shell/Python scripts, update documentation and CI. This is the atomic switchover.
6. **Phase F (Post-Cutover Verification)** -- Task 031: Full PathFlow lifecycle test (PF1-PF7) with Go hooks, `codeflow doctor` health check, performance benchmarking.

**Key principle:** During Phase B (Build), shell scripts are NOT modified. The Go code is built and tested in isolation. Cutover criteria (settings.json changes, script deletion, documentation updates) are deferred to Phase E. This ensures the live system is never in a half-migrated state.

### New Go Packages Required

```text
codeflow-cli/internal/
  hooks/           # Hook command implementations
    security/      # Consolidated security enforcement
    gate/          # PathFlow gate check
    sentinel/      # Sentinel pipeline (create, checkpoint)
    session/       # Session start/end hooks
    logging/       # Hook event logging
    webfetch/      # URL validation
    team/          # Team guard
    edit/          # Edit/write scope enforcement
    ghpr/          # GitHub PR hook
    resource/      # Protected resource enforcement
    prompt/        # User prompt submit hooks
  ledger/          # JSONL ledger operations (append, read, validate)
  pathflow/        # PathFlow state management (checkpoint, transitions)
  sentinel/        # Sentinel system (create, check, list)
  validate/        # Epic/task markdown validation
  workstate/       # Active task state management
  shellutil/       # Shell-lib equivalents (logging, config, errors)
```

### Hook Invocation Pattern

Current (shell):

```json
{
  "type": "command",
  "command": "bash \"$CLAUDE_PROJECT_DIR\"/.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh",
  "timeout": 10000
}
```

Target (Go):

```json
{
  "type": "command",
  "command": "\"$CLAUDE_PROJECT_DIR\"/codeflow-cli/bin/codeflow hooks pre-tool-use gate-check",
  "timeout": 5000
}
```

### Key Constraints

- No backward compatibility concerns -- optimize for best end state
- Shell scripts requiring sudo (protection scripts) STAY as shell
- Dead code (`cf-hook-bypass.sh`, `shell-lib/index.sh`) is removed, not migrated
- All hook scripts read stdin JSON -- Go binary must accept the same stdin format
- Go binary must handle `CLAUDE_PROJECT_DIR`, `CODEFLOW_SESSION_ID`, and other env vars
- Settings.json timeout values can be reduced (Go is faster) but not eliminated
- Platform-specific bugs (macOS/Linux) are fixed by Go's stdlib (no more `date`/`stat`/`flock` splits)

### Test Migration Strategy

- Shell tests in `.codeflow/testing/` are migrated to Go tests in `codeflow-cli/internal/`
- Each Go package gets `*_test.go` files with table-driven tests
- Integration tests use `testutil.TempProject()` for isolated test environments
- Shell test files are removed after Go equivalents pass
- `test-config.json` entries for retired tests are removed
- CI workflow (`test-suite.yml`) updated to run `make test-cover` instead of shell tests for migrated components

### Analysis Document

Full migration analysis at `.codeflow/docs/analysis/go-cli-migration-comprehensive.md` covers:

- Complete script inventory with line counts and migration priority
- Hook-by-hook migration mapping
- Python elimination strategy
- Performance targets and measurement methodology

## Related

- INF-EPC-015: Go CLI Phase 6 V4 Implementation (base Go CLI binary)
- INF-EPC-013: Session end cleanup fixes (prerequisite fixes)
- `.codeflow/docs/analysis/go-cli-migration-comprehensive.md`: Migration analysis document
- PLN-TSK-001-NNN: Planning session that produced this epic
