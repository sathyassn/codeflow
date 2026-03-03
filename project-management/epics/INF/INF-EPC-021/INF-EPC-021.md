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
- Protection scripts (`cf-protect-resources.sh`, `cf-promote-protection.sh`, `cf-reload-protection.sh` and `lib/`) -- all stay as shell. They form a cohesive OS-level protection subsystem; splitting across Go and shell would complicate maintenance without meaningful benefit. INF-TSK-021-036 (planned Go migration) has been cancelled.
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

### Phase D (JSONL Normalization + Build Completion) -- Tasks 026, 028, 034, 035, 037

- [ ] JSONL schema normalized before cutover (Task 026)
- [ ] test-config.json business_packages updated with all migration packages (Task 028)
- [x] Go session-start instructions hook built and tested (Task 034) — COMPLETE
- [x] Go git-hooks package built: commit-msg, post-commit, pre-push, prepare-commit-msg, pre-commit-validate (Task 035) — COMPLETE (Go-only; shell cutover deferred to Phase E via Task 022)
- [ ] Go pathflow subcommands built: phase-transition, stage-transition, session-register, task-update, session-metadata (Task 037) — required before Phase E deletes cf-pathflow-*.sh scripts

### Phase E (Single-Session Cutover) -- Tasks 021, 022 (parallel; 025 cancelled, absorbed into 022)

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
| INF-TSK-021-002 | Build Go CLI for ULID generation and format ID | complete | critical |
| INF-TSK-021-003 | Build Go CLI for checkpoint engine | complete | critical |
| INF-TSK-021-004 | Build Go CLI for active task state and memory operations | complete | high |
| INF-TSK-021-005 | Build Go CLI for validation scripts | complete | high |
| INF-TSK-021-006 | Build Go CLI for security enforcement stack | complete | critical |
| INF-TSK-021-007 | Build Go CLI for pathflow gate hook | complete | critical |
| INF-TSK-021-008 | Build Go CLI for webfetch, team-guard, and gh-pr hooks | complete | high |
| INF-TSK-021-009 | Build Go CLI for sentinel pipeline hooks | complete | critical |
| INF-TSK-021-010 | Build Go CLI for session-start-init hook | complete | high |
| INF-TSK-021-011 | Build Go CLI for session-end-cleanup hook | complete | high |
| INF-TSK-021-012 | Build Go CLI for sentinel system script | complete | high |
| INF-TSK-021-013 | Wire codeflow session start into SessionStart hook | complete | high |
| INF-TSK-021-014 | Build Go equivalents for Python scripts and codeflow_py_lib | complete | high |
| INF-TSK-021-015 | Build Go CLI for shell-lib foundation | complete | normal |
| INF-TSK-021-016 | Build Go CLI for settings validation and protection guard | complete | normal |
| INF-TSK-021-017 | Build Go CLI for logging hooks | complete | normal |
| INF-TSK-021-018 | Build Go CLI for edit-write, protected-resource, and user-prompt-submit hooks | complete | normal |
| INF-TSK-021-019 | Build Go CLI for worktree and report scripts | complete | normal |
| INF-TSK-021-020 | Build Go CLI workgraph commands | complete | normal |
| INF-TSK-021-021 | Update CLAUDE.md and agent definitions for Go CLI | todo | high |
| INF-TSK-021-022 | Single-session cutover: wire Go hooks, retire shell/Python scripts, update settings | todo | critical |
| INF-TSK-021-023 | Integration testing -- full PathFlow lifecycle with Go hooks | complete | critical |
| INF-TSK-021-024 | Performance benchmarking and hook latency verification | todo | high |
| INF-TSK-021-025 | SUPERSEDED -- Remove retired shell and Python infrastructure (absorbed into INF-TSK-021-022) | cancelled | high |
| INF-TSK-021-026 | Normalize JSONL schema and rebuild SQLite | todo | high |
| INF-TSK-021-027 | Update git hooks and CI workflows for post-migration compatibility | todo | normal |
| INF-TSK-021-028 | Update test-config.json business_packages with migration packages | complete | high |
| INF-TSK-021-029 | Align INF-EPC-021 task criteria with build-coexist-cutover strategy | complete | high |
| INF-TSK-021-030 | Shadow testing -- run Go alongside shell and verify output parity | complete | high |
| INF-TSK-021-031 | Post-cutover verification -- full lifecycle test with Go hooks | todo | high |
| INF-TSK-021-032 | Fix replace/migrate language in INF-EPC-021 build tasks | complete | high |
| INF-TSK-021-033 | Align INF-EPC-021 tasks with revised Go CLI migration decisions | complete | high |
| INF-TSK-021-034 | Build Go CLI for session-start instructions hook | complete | high |
| INF-TSK-021-035 | Build Go git-hooks subcommands (Go-only; cutover deferred to 022) | complete | normal |
| INF-TSK-021-036 | Migrate protection management scripts to Go | cancelled | low |
| INF-TSK-021-037 | Build Go CLI for pathflow transition and registration scripts | complete | high |
| INF-TSK-021-038 | Update INF-EPC-021 task files: corrections and gap-filling | complete | normal |

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
2. **Phase B (Build)** -- Tasks 001-015, 017-020, 033-035 (036 cancelled): Create all Go subcommands with full functionality and >= 85% test coverage. Shell/Python scripts remain UNCHANGED and active. No settings.json modifications. Go code is built and tested but not wired in.
3. **Phase C (Shadow Testing)** -- Task 030: Run Go subcommands alongside shell scripts, compare output, log divergences. Validates behavioral parity before cutover.
4. **Phase D (JSONL Normalization + Build Completion)** -- Task 026: Normalize JSONL ledger/log schema before cutover to ensure Go binary reads/writes the correct format. Task 028: Register all migration packages in test-config.json business_packages so cutover verification (`./codeflow test`) can enforce 85% coverage. Task 034: **COMPLETE** — Go session-start instructions hook built. Task 035: **COMPLETE** — Go `internal/githooks/` package built and tested (85.1% coverage); shell scripts NOT yet replaced (thin wrapper creation deferred to Phase E/Task 022). Task 037: Build Go pathflow subcommands (phase-transition, stage-transition, session-register, task-update, session-metadata) — must complete before Phase E cutover deletes cf-pathflow-*.sh scripts.
5. **Phase E (Single-Session Cutover)** -- Tasks 022 + 021 (parallel): Task 022 handles hook wiring, script deletion, git hook cutover, and test rewriting. Task 021 handles CLAUDE.md and agent/command/skill documentation updates. Both run simultaneously. Prerequisites: 023 (integration tests pass), 026 (JSONL normalized), 028 (business_packages registered), 030 (shadow tests pass).
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
  cliutil/         # Shell-lib equivalents (logging, config, errors)
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
- All three protection scripts STAY as shell (`cf-protect-resources.sh`, `cf-promote-protection.sh`, `cf-reload-protection.sh` + `lib/`): they form a cohesive OS-level protection subsystem. INF-TSK-021-036 (planned Go migration of promote/reload) has been cancelled.
- Dead code (`cf-hook-bypass.sh`, `shell-lib/index.sh`) is removed, not migrated
- All hook scripts read stdin JSON -- Go binary must accept the same stdin format
- Go binary must handle `CLAUDE_PROJECT_DIR`, `CODEFLOW_SESSION_ID`, and other env vars
- Settings.json timeout values can be reduced (Go is faster) but not eliminated
- Platform-specific bugs (macOS/Linux) are fixed by Go's stdlib (no more `date`/`stat`/`flock` splits)

### Scope Revisions (Post-Initial Planning)

**INF-TSK-021-016 scope revised (INF-TSK-021-033):**

- Original scope: settings scripts (cf-change-approval-mode.sh, setup-managed-settings.sh) + staging scripts
- Revised scope: settings validation (`codeflow settings validate` / `codeflow hooks post-tool-use settings-validate`) + protection-guard command (`codeflow hooks pre-tool-use protection-guard`)
- 5 dormant staging scripts in `.codeflow/scripts/security/staging/` are replaced by the protection-guard command; they were never called programmatically
- `cf-post-tool-use-settings-templates.sh` consolidated into `codeflow settings validate` -- single Go function serves both CLI and hook entry points
- `cf-post-tool-use-tmp-workflow.sh` consolidated into protection-guard command
- Approval mode shell script (`cf-change-approval-mode.sh`) dropped from scope; the `/cf-approval-mode` slash command stays as-is

**New tasks added (Phase B build phase):**

- INF-TSK-021-034: Session-start instructions hook (`cf-session-start-instructions.sh`) now has a build task; previously omitted from Phase B
- INF-TSK-021-035: **COMPLETE (reduced scope).** Builds Go `internal/githooks/` package (5 subcommands: commit-msg, post-commit, pre-push, prepare-commit-msg, pre-commit-validate). Shell scripts in `.codeflow/scripts/git-hooks/` are NOT modified — cutover to thin wrappers is deferred to INF-TSK-021-022 (atomic cutover). QA FAIL verdict is expected: 111/279 shell test failures are due to shell tests still asserting old shell behavior; Go tests pass at 85.1% coverage. Thin shell wrapper creation, shell test rewrites, and consistency test behavioral updates all transfer to INF-TSK-021-022.
- INF-TSK-021-036: **CANCELLED.** Protection management scripts (`cf-promote-protection.sh`, `cf-reload-protection.sh`) will stay as shell. They form a cohesive OS-level protection subsystem alongside `cf-protect-resources.sh` and `lib/`; splitting promote/reload into Go while leaving protect-resources in shell would complicate maintenance without meaningful benefit.

**Retained scripts count revised:**

- Git hooks: previously 5 permanently retained (shell) → 1 hybrid (pre-commit: shell wrapper + Go logic) + 4 thin wrappers (minimal shell delegation)
- Protection: all 3 scripts retained as shell (`cf-protect-resources.sh`, `cf-promote-protection.sh`, `cf-reload-protection.sh` + `lib/`) — INF-TSK-021-036 cancelled

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
