---
title: "Design Analysis: CodeFlow Go CLI - Phase 6 V4 Implementation"
epic: "INF-EPC-015"
status: proposed
created_at: "2026-02-23"
updated_at: "2026-02-23"
---

# Design Analysis: CodeFlow Go CLI - Phase 6 V4 Implementation

## 1. Overview

Phase 6 builds the production CodeFlow CLI binary in Go. The Go CLI wraps Claude Code with a welcome screen, preflight checks, and commands. It is the SOLE database authority -- all database writes go through the compiled binary, eliminating LLM-driven shell-to-database data layer risks (hallucinated filenames, wrong-file event routing, schema drift). Note: The Rust CLI redesign (Epic 0) replaced the Go CLI with SurrealDB embedded via the `surrealdb` crate.

The CLI replaces:

- `surrealdb` CLI / direct shell invocations for DB writes (legacy)
- `echo ... >> file.jsonl` for JSONL appends
- `python3 ulid_generator.py` for ID generation
- Ad-hoc shell scripts for session management

The CLI preserves:

- Shell hooks (SessionStart, PreToolUse, etc.) -- these remain shell scripts
- JSONL as Tier 0 rebuild authority
- Agent definitions and instructions (markdown files)
- PathFlow phase/stage orchestration (LLM-driven)

## 2. Architecture

### Two-Tier Global/Local Pattern

| Tier | Location | Purpose |
|------|----------|---------|
| Global | `~/.local/bin/codeflow` | System-wide entry point, version management |
| Local | `codeflow-cli/` (in repo) | Project-specific build, development |

### Technology Stack

- **Language:** Pure Go (CGO-free)
- **Database:** `surrealdb` crate (Rust CLI, embedded `surrealkv://` mode) — replaces legacy Go database driver
- **CLI framework:** `spf13/cobra`
- **ULID library:** `oklog/ulid` (or equivalent)
- **Cross-compilation:** 5 platform targets via Makefile
- **Go version:** 1.21+

### Directory Structure

```text
codeflow-cli/
  cmd/
    codeflow/
      main.go              # Entry point
      main_test.go         # Integration tests
    version.go             # Version command
    uninstall.go           # Uninstall command
    db.go                  # DB subcommands
    session.go             # Session subcommands
    init.go                # Init command
    doctor.go              # Doctor command
    config.go              # Config command
    update.go              # Update command
    autorun/
      run.go               # Autorun run
      status.go            # Autorun status
      list.go              # Autorun list
  internal/
    welcome/
      welcome.go           # Welcome screen (V3 + V4)
      welcome_test.go
      welcome_bench_test.go
    preflight/
      preflight.go         # Pre-flight checks
      preflight_test.go
    doctor/
      checks.go            # Health checks (V3 13 + V4 3)
      checks_test.go
      repair.go            # Repair actions
      repair_test.go
    init/
      wizard.go            # Project init wizard
      wizard_test.go
    autorun/
      orchestrator.go      # Batch orchestration
      orchestrator_test.go
      worker.go            # Worker management
      worker_test.go
      batch.go             # Batch file parsing
      batch_test.go
    config/
      config.go            # Config management
      config_test.go
    update/
      update.go            # Self-update
      update_test.go
    db/
      connection.go        # DB singleton, PRAGMAs
      connection_test.go
      models.go            # Struct definitions
      models_test.go
      schema.sql            # Embedded schema (copied at build time)
      queries.go           # Read operations
      queries_test.go
      transactions.go      # Write operations
      transactions_test.go
      health.go            # Integrity checks
      health_test.go
      logging.go           # DB audit logging
      logging_test.go
      sync.go              # JSONL sync parser
      sync_test.go
      migrate.go           # Migration runner
      migrate_test.go
    session/
      session.go           # Session start/end
      session_test.go
    claude/                 # Claude Code integration (future)
    testutil/
      testutil.go          # Test helpers
      testutil_test.go
  testdata/
    projects/
      valid-project/       # Test fixture: valid project
      missing-config/      # Test fixture: missing .codeflow/
      corrupted-db/        # Test fixture: corrupted DB
      no-claude/           # Test fixture: no Claude Code
    configs/               # Test config fixtures
    batch/                 # Test batch file fixtures
  go.mod
  go.sum
  Makefile
  bin/                     # Build output (gitignored)
```

### Build Targets (Makefile)

| Target | Command | Purpose |
|--------|---------|---------|
| `build` | `CGO_ENABLED=0 go build -ldflags "..." ./cmd/codeflow/` | Local binary |
| `build-all` | 5x `GOOS/GOARCH` combinations | Cross-compile |
| `test` | `go test ./...` | Run all tests |
| `test-cover` | `go test -coverprofile=... ./...` + 85% threshold | Coverage gate |
| `test-race` | `go test -race ./...` | Race detector |
| `lint` | `golangci-lint run` | Linting |
| `copy-schema` | `cp .codeflow/scripts/db/schema.sql internal/db/schema.sql` | Schema embedding |
| `clean` | `rm -rf bin/` | Clean build artifacts |

### Cross-Platform Targets

| Platform | GOOS | GOARCH | Binary |
|----------|------|--------|--------|
| Linux x64 | linux | amd64 | `bin/codeflow-linux-amd64` |
| Linux ARM | linux | arm64 | `bin/codeflow-linux-arm64` |
| macOS x64 | darwin | amd64 | `bin/codeflow-darwin-amd64` |
| macOS ARM | darwin | arm64 | `bin/codeflow-darwin-arm64` |
| Windows x64 | windows | amd64 | `bin/codeflow-windows-amd64.exe` |

## 3. Design Decisions

### D1: Session ID Format

**Decision:** Go CLI is authoritative for ID generation. Use ULID-based format: `ses-{ulid}`.

Existing `ses-{timestamp}{hex}` IDs are temporary artifacts of the shell-based data layer. They will be regenerated to the canonical format during JSONL normalization (D3).

Each table uses ULID with a type prefix:

| Entity | Format | Example |
|--------|--------|---------|
| Epic | `epic-{ulid}` | `epic-01ARZ3NDEKTSV4RRFFQ69G5FAV` |
| Task | `task-{ulid}` | `task-01BRZ4PDFLUTW5SSGG70H6GBW` |
| Session | `ses-{ulid}` | `ses-01CRZ5QEGLXUW6TTHH81I7HCX` |
| Work | `work-{ulid}` | `work-01DRZ6RFHMYV07UUII92J8IDY` |
| Memory | `memory-{ulid}` | `memory-01ERZ7SGINFO18VVJJ03K9JEZ` |

**Rationale:** ULIDs are lexicographically sortable, globally unique, and encode creation time. The type prefix enables instant identification without a table lookup.

### D2: Schema Embedding

**Decision:** Build-time Makefile copy. `.codeflow/scripts/db/schema.sql` is copied to `codeflow-cli/internal/db/schema.sql` before `go build`.

Go's `//go:embed` directive only works for files within the module tree. Since schema.sql lives in `.codeflow/scripts/db/` (outside `codeflow-cli/`), the Makefile `copy-schema` target copies it into the module before build. The embedded copy is gitignored; the canonical source remains `.codeflow/scripts/db/schema.sql`.

Same approach for migration files: `cp .codeflow/scripts/db/migrations/*.sql internal/db/migrations/`.

### D3: JSONL Normalization

**Decision:** Option B -- one-time cleanup script normalizes ALL existing JSONL files to canonical format, then strict enforcement going forward.

**Canonical format requirements:**

- Top-level key `event` (not `type`, `op`, or `e`) identifies the event type
- Consistent field naming: `old_status`/`new_status` (not `from_status`/`to_status`)
- ISO-8601 timestamp in `timestamp` key (not `ts`)
- All events include `timestamp` and `event` at minimum

The 4 canonical JSONL files (per ledger.sh constants):

| File | Events |
|------|--------|
| `work-graph.jsonl` | `epic_created`, `task_created`, `task_status_changed`, `epic_status_changed`, `pr_created`, `pr_merged` |
| `memory-events.jsonl` | `memory_store`, `progress`, `decision`, `milestone`, `finding` |
| `sessions.jsonl` | `session_start`, `session_end`, `work_claimed` |
| `config.jsonl` | `config_set`, `config_updated` |

### D4: DB Schema Authority

**Decision:** Go CLI takes precedence. `schema.sql` IS the source of truth. A migration (005) brings the live DB in line with schema.sql.

**Schema drift analysis (current state):**

The schema.sql file already defines `format_id TEXT UNIQUE NOT NULL` on the `epics` table (line 113) and `current_stage`/`team_name` on `active_work` (lines 458-460). The stage CHECK constraint on `tasks` is also present (line 182-183). However, the **live database** may not have these columns if it was created before these schema.sql changes were applied and no migration added them.

Issues to fix in migration 005:

| Issue | Severity | Fix |
|-------|----------|-----|
| Live DB `epics` may lack `format_id` column | CRITICAL | Table rebuild with backfill |
| Live DB `active_work` may lack `current_stage`, `team_name` | HIGH | ALTER TABLE ADD COLUMN |
| `stage` CHECK constraint on tasks may be missing in live DB | MEDIUM | Table rebuild if constraint absent |
| Schema version gap (PRAGMA user_version may be at 2-4) | LOW | Set to 5 after migration |
| `auto_commit` column DEPRECATED but present | LOW | Leave as-is, document deprecation |

### D5: `auto_commit` Deprecation

**Decision:** Column marked DEPRECATED. Not exposed in new Go CLI commands. Existing data preserved but column scheduled for removal in a future migration.

See `inf-epc-008-design-analysis.md` (D5) for the original deprecation decision. The Go CLI continues this by not adding any CLI commands that read or write `auto_commit`.

### D6: Testing Strategy

**Decision:** Hybrid approach combining Go-native tests with the existing CodeFlow test runner.

| Layer | Tool | Coverage Target |
|-------|------|----------------|
| Go unit tests | `go test ./...` | 85% line coverage |
| Go race detection | `go test -race ./...` | No races |
| Bridge integration | `.codeflow/testing/cli/test-go-cli.sh` | Passes/fails via test-helpers.sh |
| Full suite | `./codeflow test --mode full` | Includes cli-go category |

**TDD approach:** Tests written alongside implementation, coverage enforced from task 001. Coverage = 85% LINE coverage (not structural/file coverage).

**Makefile targets:**

| Target | Purpose |
|--------|---------|
| `test` | `go test ./...` |
| `test-cover` | `go test -coverprofile=coverage.out ./...` + threshold check |
| `test-race` | `go test -race ./...` |
| `lint` | `golangci-lint run` |

**Bridge script:** `.codeflow/testing/cli/test-go-cli.sh` invokes `make test-cover` from `codeflow-cli/` and reports using `test_pass`/`test_fail`. Registered in `test-config.json` as `cli-go` category at MEDIUM priority.

### D7: JSONL Sync Parser

**Decision:** Go CLI `db sync` implements a tolerant parser that normalizes all known JSONL variants to canonical form before processing.

**Known structural patterns in existing JSONL:**

| Pattern | Example | Normalization |
|---------|---------|---------------|
| Pattern 1: `event` key | `{"event":"epic_created",...}` | Already canonical -- pass through |
| Pattern 2: `type` key | `{"type":"memory_stored",...}` | Rename `type` to `event` |
| Pattern 3: `e` key (shorthand) | `{"e":"session_start",...}` | Rename `e` to `event`, `ts` to `timestamp` |
| Pattern 4: `op`+`table` keys | `{"op":"insert","table":"sessions",...}` | Map to canonical event name |

Future events MUST use canonical form only (enforced by Go CLI validation at write time). The tolerant parser exists only for backward compatibility with pre-CLI data.

### D8: Cross-Platform Build

**Decision:** 5 binaries via Makefile. No CGO. Binary size <20MB. Startup <50ms. Version injected via `-ldflags`.

| Constraint | Target | Rationale |
|-----------|--------|-----------|
| No CGO | `CGO_ENABLED=0` | Enables cross-compilation without C toolchains |
| Binary size | <20MB per platform | Go CLI was ~15MB; Rust CLI uses SurrealDB embedded (~30-50MB total) |
| Startup time | <50ms to first output | CLI should feel instant |
| Version injection | `-ldflags "-X main.version=$(VERSION)"` | No hardcoded version strings |

## 4. Schema Drift Analysis

Analysis of mismatches between `schema.sql` (source of truth) and the live database state.

### Mismatch 1: `epics.format_id` column (CRITICAL)

**schema.sql (line 113):** `format_id TEXT UNIQUE NOT NULL`

**Live DB:** May lack this column if created before the column was added to schema.sql. The `CREATE TABLE IF NOT EXISTS` in schema.sql would not retroactively add the column to an existing table.

**Migration 005 fix:** Table rebuild (CREATE new, INSERT from old, DROP old, ALTER RENAME) with backfill. For existing rows without format_id, derive from the `external_id` field or set to a generated placeholder.

### Mismatch 2: `active_work.current_stage` + `team_name` columns (HIGH)

**schema.sql (lines 458-460):** `current_stage TEXT` with CHECK constraint, `team_name TEXT`

**Live DB:** May lack these columns. Added to schema.sql but no migration script creates them.

**Migration 005 fix:** `ALTER TABLE active_work ADD COLUMN current_stage TEXT DEFAULT NULL CHECK(...)` and `ALTER TABLE active_work ADD COLUMN team_name TEXT`.

### Mismatch 3: `tasks.stage` CHECK constraint (MEDIUM)

**schema.sql (line 182-183):** `stage TEXT DEFAULT NULL CHECK(stage IS NULL OR stage IN ('dev', 'work', 'review', 'qa', 'done'))`

**Live DB:** The column may exist (added by migration 001) but without the CHECK constraint. (Note: This was a legacy schema limitation; the Rust CLI uses SurrealDB which handles schema evolution differently.)

**Migration 005 fix:** Table rebuild to add CHECK constraint, only if constraint is missing.

### Mismatch 4: Schema version gap (LOW)

**Current PRAGMA user_version:** May be at 2, 3, or 4 depending on which migrations have been applied.

**Migration 005 fix:** Set `PRAGMA user_version = 5` unconditionally.

### Mismatch 5: `auto_commit` DEPRECATED but present (LOW)

**schema.sql (line 168):** `auto_commit BOOLEAN DEFAULT TRUE, -- DEPRECATED`

**Decision:** Leave column in place. The deprecation comment in schema.sql is sufficient. No migration needed for this item.

## 5. JSONL Normalization Plan

### Existing Structural Patterns

**Pattern 1: `event` key (canonical)**
Found in: `work-graph.jsonl` (newer entries)

```json
{"event":"epic_created","id":"epic-01KHSQPQRNQP0XTXCRHXX9YW1T","format_id":"INF-EPC-005",...,"timestamp":"2026-02-19T03:57:15Z"}
```

**Pattern 2: `type` key**
Found in: `memory-events.jsonl`

```json
{"type":"memory_stored","id":"memory-01KH7A1ZB0P2K8PYNZBW60E7K3","event_type":"milestone",...,"ts":"2026-02-11T21:34:59.431076+00:00"}
```

**Pattern 3: `e` key (shorthand via ledger.sh)**
Produced by: `ledger.sh` `append_ledger()` function

```json
{"ts":"2026-02-13T16:45:27Z","e":"session_start","sid":"ses-177137202131769e89b2d5688"}
```

**Pattern 4: `op`+`table` keys**
Found in: `memory-events.jsonl` (line 9)

```json
{"op":"INSERT","table":"memory_events","id":"memory-...","event_type":"milestone","memory_type":"complete-work","data":{...}}
```

### Canonical Target Format

After normalization, all events follow this structure:

```json
{"event":"<event_name>","timestamp":"<ISO-8601>",... event-specific fields ...}
```

**work-graph.jsonl canonical events:**

| Event | Required Fields |
|-------|----------------|
| `epic_created` | id, format_id, title, area_type, work_type, domain, status |
| `task_created` | id, format_id, epic_id, title, area_type, work_type, domain, status |
| `task_status_changed` | task_id, old_status, new_status |
| `epic_status_changed` | epic_id, old_status, new_status |
| `pr_created` | task_id, pr_number, branch, target_branch |
| `pr_merged` | task_id, pr_number |

**memory-events.jsonl canonical events:**

| Event | Required Fields |
|-------|----------------|
| `memory_store` | id, event_type, domain, data |
| `progress` | id, domain, work_id, data |
| `decision` | id, domain, data |
| `milestone` | id, domain, data |
| `finding` | id, domain, data |

**sessions.jsonl canonical events:**

| Event | Required Fields |
|-------|----------------|
| `session_start` | session_id, interaction_mode |
| `session_end` | session_id, status |
| `work_claimed` | session_id, work_id |

**config.jsonl canonical events:**

| Event | Required Fields |
|-------|----------------|
| `config_set` | key, value |
| `config_updated` | key, old_value, new_value |

### Normalization Rules

| Source Pattern | Normalization |
|---------------|---------------|
| `"e":"X"` | Rename to `"event":"X"` |
| `"ts":"T"` | Rename to `"timestamp":"T"` |
| `"type":"memory_stored"` | Rename to `"event":"memory_store"` |
| `"from_status":"X"` | Rename to `"old_status":"X"` |
| `"to_status":"X"` | Rename to `"new_status":"X"` |
| Missing `timestamp` | Add current timestamp |
| `"sid":"X"` | Rename to `"session_id":"X"` |
| `"wid":"X"` | Rename to `"work_id":"X"` |

## 6. Task Dependency Graph

```text
                           INF-TSK-015-001 (Go project init)
                          /     |      |     \         \
                         /      |      |      \         \
                        v       v      v       v         v
                   015-002  015-003  015-004  015-007  015-008
                   (CLI     (DB      (DB      (Welcome (Preflight)
                   entry)   migrate) core)    V3)       |
                     |        |       |        |        |
                     v        v       v        v        |
                   015-011  015-005  015-006  015-014   |
                   (Config) (DB CLI) (Session)(Welcome  |
                     |        |       |       V4)       |
                     v        |       v        |        |
                   015-012   |     015-009     |        |
                   (Update)  |     (Init)      |        |
                             |       |         |        |
                             v       v         v        v
                           015-013            015-010  015-016
                           (Autorun)          (Doctor  (Test
                             |                V3)      infra)
                             |                 |
                             v                 v
                           015-015
                           (Doctor V4)
                             |
                             v
                    INF-TSK-015-017 (JSONL normalize + cross-compile verify)
                    [blocked by: 002,005,006,007,008,009,010,011,012,013,014,015]
```

**Critical path:** 001 -> 004 -> 005 -> 013 -> 017

**Parallelizable after 001:** Tasks 002, 003, 004, 007, 008, 016 can all start in parallel once 001 completes.

## 7. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| SurrealDB embedded performance insufficient | Low | High | Benchmark during task 004; the Rust CLI uses `surrealdb` crate with `surrealkv://` mode |
| Binary size exceeds 20MB | Low | Medium | Profile with `go build -ldflags="-s -w"`; strip debug info |
| JSONL normalization loses data | Medium | Critical | Backup before normalize; verify event counts match before/after; idempotency test |
| Schema migration 005 corrupts existing DB | Low | Critical | Transaction-wrapped migration; backup before apply; rollback on error |
| Cross-compilation failures on Windows | Medium | Low | Windows is lowest priority target; test last |
| Test coverage drops below 85% as features accumulate | Medium | Medium | Coverage enforced by Makefile `test-cover` target; CI blocks on threshold |
| Startup time exceeds 50ms due to DB init | Medium | Medium | Lazy DB connection (only open when needed); benchmark in task 017 |
| Cobra command conflicts with existing `./codeflow` shell script | Low | Medium | Different binary name in dev (`codeflow-cli`); global install as `codeflow` only after shell script is deprecated |

### Assumptions

| # | Assumption | Verified? | Evidence |
|---|-----------|-----------|----------|
| 1 | `.codeflow/scripts/db/schema.sql` is the authoritative schema | YES | Read file; 849 lines, 37 tables, seeds, FTS |
| 2 | 4 canonical JSONL files at `.state/ledger/` | YES | Glob found: config.jsonl, sessions.jsonl, memory-events.jsonl, work-graph.jsonl |
| 3 | ledger.sh defines LEDGER_* constants | YES | Read file; LEDGER_CONFIG, LEDGER_WORK_GRAPH, LEDGER_MEMORY, LEDGER_SESSIONS |
| 4 | 4 migration files exist (001-004) | YES | Glob found 4 .sql files in migrations/ |
| 5 | `codeflow-cli/` directory does not exist yet | YES | ls returned "directory does not exist" |
| 6 | `.codeflow/testing/cli/` directory does not exist yet | YES | ls returned "directory does not exist" |
| 7 | schema.sql has format_id on epics table | YES | Line 113: `format_id TEXT UNIQUE NOT NULL` |
| 8 | schema.sql has current_stage/team_name on active_work | YES | Lines 458-460 |
| 9 | JSONL uses mixed patterns (event, type, e keys) | YES | Read first 5 lines of each file; confirmed 3+ patterns |
| 10 | Existing test suite has 1,555+ tests | YES | Per CLAUDE.md Section 7 |
