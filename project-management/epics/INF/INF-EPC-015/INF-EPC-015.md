---
id: "epic-01KJ6YG0QA7MWQQFWCZMSP7HB6"
format_id: "INF-EPC-015"
title: "CodeFlow Go CLI - Phase 6 V4 Implementation"
summary: "Build the production CodeFlow CLI binary in Go — sole SQLite authority, welcome screen, doctor, autorun, cross-platform"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
is_ongoing: false
file_scope: ["codeflow-cli/", ".codeflow/scripts/db/", ".codeflow/testing/cli/", ".codeflow/testing/test-config.json"]
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-02-23T00:00:00Z"
updated_at: "2026-02-23T00:00:00Z"
---

# INF-EPC-015: CodeFlow Go CLI - Phase 6 V4 Implementation

## Summary

Build the production CodeFlow CLI binary in Go. The Go CLI is the SOLE SQLite authority -- all database writes go through the compiled binary, eliminating LLM-driven shell-to-sqlite3 data layer risks. The CLI wraps Claude Code with a welcome screen, preflight checks, doctor diagnostics, session management, autorun orchestration, and cross-platform distribution. See `.codeflow/docs/analysis/inf-epc-015-go-cli-design.md` for full design analysis and rationale.

## Scope

### In Scope

- Go project initialization with build infrastructure (Makefile, testutil, fixtures)
- Core CLI entry point with cobra commands (version, uninstall)
- DB schema drift fix (migration 005)
- Database core package (connection, models, PRAGMA enforcement)
- Database CLI commands (init, migrate, sync, query, exec, check, version, backup)
- Session management package and CLI commands (start, end)
- Welcome screen (V3 base + V4 PathFlow enhancements)
- Preflight checks package
- Init command (project wizard)
- Doctor command (13 V3 health checks + 3 V4 checks)
- Config and Update commands
- Autorun orchestrator and worker commands
- Test infrastructure integration (bridge script, test-config.json)
- JSONL normalization script
- Cross-compilation verification (5 platforms)

### Out of Scope

- Claude Code wrapping/exec (deferred to Phase 7)
- NX monorepo setup (Phase 7)
- CRDT or multi-user collaboration features
- Embedding/vector search integration
- Web UI or dashboard
- Package manager distribution (homebrew, apt)

## Acceptance Criteria

- [ ] `go build ./cmd/codeflow/` produces working binary from `codeflow-cli/`
- [ ] `codeflow --version` outputs version from ldflags, startup < 50ms
- [ ] `codeflow db init` creates SQLite database from embedded schema
- [ ] `codeflow db sync` rebuilds SQLite from all 4 canonical JSONL files
- [ ] `codeflow db migrate` applies migration 005 fixing schema drift
- [ ] `codeflow session start` and `codeflow session end` manage session lifecycle
- [ ] `codeflow doctor` runs 16 health checks (13 V3 + 3 V4)
- [ ] `codeflow init` initializes new projects with wizard flow
- [ ] `codeflow autorun run batch.yaml` orchestrates parallel workers
- [ ] Welcome screen displays project state with V3 base and V4 PathFlow indicators
- [ ] `make build-all` produces 5 platform binaries, each < 20MB, CGO_ENABLED=0
- [ ] `make test-cover` passes with >= 85% line coverage across entire Go project
- [ ] `.codeflow/testing/cli/test-go-cli.sh` integrates with `./codeflow test`
- [ ] JSONL normalization script converts all existing events to canonical format
- [ ] All existing tests pass (`./codeflow test`)

## Tasks

| ID | Title | Status | Priority |
|----|-------|--------|----------|
| INF-TSK-015-001 | Go project initialization and build infrastructure | todo | high |
| INF-TSK-015-002 | Core CLI entry point and simple commands | todo | high |
| INF-TSK-015-003 | Fix DB schema drift (migration 005) | todo | high |
| INF-TSK-015-004 | Database core package | todo | high |
| INF-TSK-015-005 | Database CLI commands (8 subcommands) | todo | high |
| INF-TSK-015-006 | Session package and CLI commands | todo | high |
| INF-TSK-015-007 | Welcome screen package (V3 base) | todo | normal |
| INF-TSK-015-008 | Preflight checks package | todo | normal |
| INF-TSK-015-009 | Init command (project wizard) | todo | normal |
| INF-TSK-015-010 | Doctor command (13 V3 health checks) | todo | normal |
| INF-TSK-015-011 | Config command | todo | normal |
| INF-TSK-015-012 | Update command | todo | normal |
| INF-TSK-015-013 | Autorun orchestrator and worker commands | todo | high |
| INF-TSK-015-014 | Welcome screen V4 PathFlow enhancements | todo | normal |
| INF-TSK-015-015 | Doctor V4 health checks (3 new) | todo | normal |
| INF-TSK-015-016 | Test infrastructure integration (bridge script + test-config) | todo | normal |
| INF-TSK-015-017 | JSONL normalization and cross-compilation verification | todo | normal |

## Dependencies

### Blocked By

- Phases 1-5 complete (V4 Implementation: Agent Teams, PathFlow, Hooks, Commands, Sandbox)
- PLN-TSK-001-003 (this planning session)

### Blocks

- Phase 7: NX Monorepo + Claude Code wrapping

## Technical Notes

Full design analysis at `.codeflow/docs/analysis/inf-epc-015-go-cli-design.md` covers:

- D1: Session ID Format (ULID-based `ses-{ulid}`)
- D2: Schema Embedding (build-time Makefile copy)
- D3: JSONL Normalization (one-time cleanup + strict enforcement)
- D4: DB Schema Authority (Go CLI takes precedence, migration 005)
- D5: `auto_commit` Deprecation (leave column, don't expose)
- D6: Testing Strategy (hybrid Go + bridge script, 85% coverage)
- D7: JSONL Sync Parser (tolerant parser for 4 known patterns)
- D8: Cross-Platform Build (5 binaries, no CGO, <20MB, <50ms startup)

## Related

- Phase 6 V4 specification
- PLN-TSK-001-003: Planning session that produced this epic
- INF-EPC-008: PathFlow PR Verification (prerequisite Phase 5 work)
- `.codeflow/scripts/state/ledger.sh`: Current shell-based ledger operations (replaced by Go CLI)
- `.codeflow/scripts/db/schema.sql`: Authoritative schema (embedded by Go CLI)
