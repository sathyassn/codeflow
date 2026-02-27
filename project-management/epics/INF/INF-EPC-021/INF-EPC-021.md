---
id: "epic-PENDING"
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

- [ ] All 22 hook scripts replaced with Go binary invocations in `settings.json`
- [ ] `codeflow ledger append` replaces all shell JSONL write operations with atomic, validated writes
- [ ] `codeflow hooks session-start-init` replaces the 619-line shell session-start-init script
- [ ] `codeflow hooks gate-check` replaces cf-pre-tool-use-pathflow-gate.sh
- [ ] `codeflow hooks security` replaces cf-pre-tool-use-security.sh and its 9 enforcement modules
- [ ] `codeflow pathflow checkpoint` replaces cf-pathflow-state.sh checkpoint engine
- [ ] All Python scripts (8) and codeflow_py_lib removed; no python3 runtime dependency
- [ ] All shell-lib scripts (6) retired after Go equivalents are wired
- [ ] Hook latency measured and documented (target: < 50ms per hook, down from ~300ms)
- [ ] Full PathFlow lifecycle test passes with Go hooks (PF1 through PF7)
- [ ] `./codeflow test` passes (existing test suite not regressed)
- [ ] `make test-cover` passes in codeflow-cli with >= 85% coverage on new packages
- [ ] No python3 invocations remain in production code paths
- [ ] CLAUDE.md, agent definitions, command definitions, and skills updated to reference Go binary
- [ ] CI workflow updated to remove Python test dependencies and add Go test coverage

### PII Handling Review

- [x] Does this epic involve code that handles PII? (N)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-021-001 | Migrate JSONL ledger writes to Go CLI | todo | critical |
| INF-TSK-021-002 | Migrate ULID generation and format ID to Go CLI | todo | critical |
| INF-TSK-021-003 | Migrate checkpoint engine to Go CLI | todo | critical |
| INF-TSK-021-004 | Migrate active task state and memory scripts to Go CLI | todo | high |
| INF-TSK-021-005 | Migrate validation scripts to Go CLI | todo | high |
| INF-TSK-021-006 | Consolidate security enforcement stack into Go binary | todo | critical |
| INF-TSK-021-007 | Migrate pathflow gate hook to Go binary | todo | critical |
| INF-TSK-021-008 | Migrate webfetch, team-guard, and gh-pr hooks to Go binary | todo | high |
| INF-TSK-021-009 | Migrate sentinel pipeline hooks to Go binary | todo | critical |
| INF-TSK-021-010 | Migrate session-start-init hook to Go binary | todo | high |
| INF-TSK-021-011 | Migrate session-end-cleanup hook to Go binary | todo | high |
| INF-TSK-021-012 | Migrate sentinel system script to Go binary | todo | high |
| INF-TSK-021-013 | Wire codeflow session start into SessionStart hook | todo | high |
| INF-TSK-021-014 | Eliminate Python scripts and codeflow_py_lib | todo | high |
| INF-TSK-021-015 | Migrate shell-lib foundation to Go binary | todo | normal |
| INF-TSK-021-016 | Migrate settings and staging scripts to Go binary | todo | normal |
| INF-TSK-021-017 | Migrate logging hooks to Go binary | todo | normal |
| INF-TSK-021-018 | Migrate edit-write, protected-resource, and user-prompt-submit hooks to Go binary | todo | normal |
| INF-TSK-021-019 | Migrate worktree and report scripts to Go binary | todo | normal |
| INF-TSK-021-020 | Migrate top-level codeflow wrapper and remove dead code | todo | normal |
| INF-TSK-021-021 | Update CLAUDE.md and agent definitions for Go CLI | todo | high |
| INF-TSK-021-022 | Update command definitions, skills, and settings.json hook entries | todo | high |
| INF-TSK-021-023 | Integration testing -- full PathFlow lifecycle with Go hooks | todo | critical |
| INF-TSK-021-024 | Performance benchmarking and hook latency verification | todo | high |
| INF-TSK-021-025 | Remove retired shell and Python infrastructure | todo | high |
| INF-TSK-021-026 | Normalize JSONL schema and rebuild SQLite | todo | high |

## Dependencies

### Blocked By

- INF-EPC-015: Go CLI Phase 6 (provides the base Go CLI binary with db, session, doctor, config, init, update, autorun commands). **Depends on: INF-EPC-015 (Go CLI Build-Out) -- the Go CLI binary must be feature-complete before migration can begin.**
- INF-EPC-013: Session end cleanup fixes (ensures hooks use correct session ID before migration)

### Blocks

- Phase 7: NX Monorepo + Claude Code wrapping (requires stable Go CLI with all hooks migrated)

## Technical Notes

### Migration Strategy

The migration follows a "strangler fig" pattern with four phases:

1. **Build** -- Create all Go equivalents while shell scripts remain active (coexist)
2. **Test** -- Run Go implementations in parallel/shadow mode to verify correctness
3. **Normalize** -- Normalize JSONL ledger/log schema before cutover (INF-TSK-021-026)
4. **Cutover** -- Single-session cutover: update settings.json hook entries to invoke Go binary, delete retired shell/Python scripts

At no point should both the old script and new Go command be active for the same hook. Each script is replaced one at a time with a Go subcommand, settings.json is updated to call the Go binary, the old script's tests are migrated to Go, and the old script is deleted.

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
  "command": "\"$CLAUDE_PROJECT_DIR\"/codeflow-cli/bin/codeflow hooks gate-check",
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
