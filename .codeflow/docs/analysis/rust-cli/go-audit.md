---
title: "Go CLI Audit for Rust Redesign"
type: analysis
epic: INF-EPC-022
task: INF-TSK-022-001
status: complete
created: "2026-03-07"
author: cf-planning
---

# Go CLI Audit for Rust Redesign

## 1. Executive Summary

The CodeFlow Go CLI (`codeflow-cli/`) comprises 29 internal packages totaling ~21,500 source lines (excluding tests) plus a `cmd/codeflow/` dispatch layer with 26 top-level subcommands across 30 source files. The codebase contains significant DRY violations concentrated in the data layer (`db/sync.go`, `ledger/normalize.go`, `workgraph/workgraph.go`), duplicate sentinel implementations, and pervasive Go-specific workarounds that Rust idioms eliminate entirely. This audit identifies 12 major DRY violations, 8 coupling hotspots, 10 Go-specific workaround categories, and proposes a Go-to-Rust module mapping with 7 consolidation opportunities.

## 2. Package Inventory

### 2.1 Source Line Counts (Non-Test)

| Package | Source Lines | Test Lines | Test Ratio | Internal Deps |
|---------|-------------|------------|------------|---------------|
| autorun | 983 | 1,505 | 1.53x | db, ulid |
| benchmark | 0 (test-only) | 332 | -- | -- |
| claim | 625 | 598 | 0.96x | idgen, ledger |
| claude | 2 (doc-only) | 0 | -- | -- |
| cliutil | 303 | 831 | 2.74x | -- |
| config | 412 | 733 | 1.78x | -- |
| db | 2,257 | 5,512 | 2.44x | -- |
| doctor | 1,133 | 1,377 | 1.22x | db |
| githooks | 1,217 | 2,316 | 1.90x | -- |
| hooks (11 sub-pkgs) | 6,531 | 12,098 | 1.85x | pathflow, security(internal) |
| idgen | 164 | 441 | 2.69x | -- |
| initialize | 559 | 565 | 1.01x | db, preflight |
| integration | 0 (test-only) | 1,669 | -- | -- |
| ledger | 792 | 2,364 | 2.98x | -- |
| pathflow | 904 | 2,254 | 2.49x | ledger |
| preflight | 264 | 481 | 1.82x | -- |
| report | 461 | 536 | 1.16x | -- |
| sentinel | 170 | 608 | 3.58x | -- |
| session | 323 | 1,029 | 3.19x | db, ulid |
| settings | 716 | 1,264 | 1.77x | -- |
| shadowtest | 1,163 | 1,178 | 1.01x | -- |
| testutil | 117 | 361 | 3.09x | -- |
| update | 314 | 759 | 2.42x | -- |
| validate | 700 | 1,367 | 1.95x | -- |
| verification | 10 (doc-only) | 398 | -- | -- |
| welcome | 649 | 850 | 1.31x | -- |
| workgraph | 1,082 | 1,959 | 1.81x | db, idgen, ledger |
| workstate | 125 | 500 | 4.00x | -- |
| worktree | 546 | 921 | 1.69x | -- |
| **Total** | **~21,470** | **~43,230** | **2.01x** | |

### 2.2 Package Classification

| Category | Packages | Combined Lines |
|----------|----------|----------------|
| Data layer | db, ledger, workgraph, workstate | 4,256 |
| Hook enforcement | hooks/* (11 sub-packages) | 6,531 |
| PathFlow lifecycle | pathflow, sentinel, session | 1,397 |
| Git operations | githooks | 1,217 |
| CLI infrastructure | cliutil, config, settings, welcome | 2,080 |
| Domain logic | claim, autorun, doctor, report | 3,202 |
| Maintenance/testing | shadowtest, testutil, benchmark, integration, verification | 1,290 |
| Setup/update | initialize, preflight, update, validate | 1,837 |
| Minimal/stub | claude, idgen, worktree | 712 |

## 3. DRY Violations

### 3.1 Critical: Event Normalization Duplication

**Files:** `db/sync.go:149-244` and `ledger/normalize.go:249-361`

The most significant DRY violation in the codebase. Both files implement identical event normalization logic with parallel functions:

| Function in `db/sync.go` | Function in `ledger/normalize.go` | Purpose |
|--------------------------|-----------------------------------|---------|
| `normalizeEvent()` :149 | `normalizeRecord()` :249 | Top-level event normalization |
| `normalizeEventName()` :207 | `normalizeEventName()` :312 | Event name canonicalization |
| `mapOpToEvent()` :217 | `mapOpToEventName()` :323 | Legacy op-to-event mapping |
| `renameField()` :244 | `renameFieldIfPresent()` :351 | Field rename in map |

The `ledger/normalize.go:248` comment explicitly acknowledges: "consistent with db/sync.go:normalizeEvent()".

**Rust opportunity:** Single `normalize` module with `#[derive(Serialize, Deserialize)]` event types. The normalization becomes a `From<LegacyEvent>` trait implementation, eliminating the duplication entirely.

### 3.2 Critical: Map Helper Duplication

**Files:** `db/sync.go:570-595`, `workgraph/workgraph.go:103-139`

Both packages implement identical untyped map accessor helpers:

| Helper | `db/sync.go` | `workgraph/workgraph.go` |
|--------|-------------|--------------------------|
| `getString(m, key)` | :570 | :103 |
| `getStringDefault(m, key, def)` | :577 | :111 |
| `getBool(m, key)` | :584 | :119 |
| `getJSONString(m, key)` | :595 | (not present) |
| `getJSONArray(m, key)` | (not present) | :139 |

**Rust opportunity:** Typed deserialization via `serde` eliminates all map accessor helpers. Event payloads become structs with `Option<String>` fields. No map accessors needed.

### 3.3 Critical: Sentinel Duplication

**Files:** `pathflow/sentinel.go` (73 lines) and `sentinel/sentinel.go` (157 lines)

Two independent sentinel implementations with different API styles:

| Aspect | `pathflow/sentinel.go` | `sentinel/sentinel.go` |
|--------|----------------------|----------------------|
| API style | Package-level functions | Manager struct with methods |
| Scope handling | Hardcoded "pathflow" prefix | Configurable scope + session |
| Create/Exists/List | Direct file operations | Manager-mediated operations |
| Shared constant | `sentinelPrefix = "pathflow-"` | `sentinelPrefix = "pathflow-"` |

**Rust opportunity:** Single `sentinel` module with a `SentinelManager` struct parameterized by scope. The `pathflow` module uses the same manager with a `"pathflow"` scope configuration.

### 3.4 High: JSONL File Constants Duplication

**Files:** `ledger/routing.go:15-18` and `db/sync.go:17-20` (cross-referenced by `routing_crosscheck_test.go`)

Both files define the same JSONL filename constants:

```
FileWorkGraph    = "work-graph.jsonl"
FileMemoryEvents = "memory-events.jsonl"
FileSessions     = "sessions.jsonl"
FileConfig       = "config.jsonl"
```

A crosscheck test (`routing_crosscheck_test.go`) exists specifically to verify these stay in sync -- evidence of acknowledged duplication.

**Rust opportunity:** Single `ledger::files` module with `pub const` values. No duplication possible.

### 3.5 High: nullString/nullInt64 Helper Duplication

**Files:** `autorun/orchestrator.go:509,517` and `workgraph/create.go:297`

Both packages implement `nullString` helpers that convert Go values to SQL-nullable types, with different return types:

- `autorun/orchestrator.go:509`: `func nullString(s string) sql.NullString` -- converts string to `sql.NullString` (empty string → null)
- `autorun/orchestrator.go:517`: `func nullInt64(n int64) sql.NullInt64` -- converts int64 to `sql.NullInt64` (zero → null)
- `workgraph/create.go:297`: `func nullString(s string) any` -- returns `nil` for empty strings, else the string (untyped nullable)

**Rust opportunity:** `Option<String>` and `Option<i64>` replace all `sql.Null*` types. No conversion helpers needed. SurrealDB via the `surrealdb` crate handles `Option<T>` with SurrealQL natively.

### 3.6 High: Timestamp Parsing Duplication

**Files:** `db/crdt.go:364` and `claim/claim.go:141`

Both implement RFC3339 timestamp parsing with fallback logic:

- `db/crdt.go:364`: `parseTimestamp(s string) (time.Time, error)` -- parses RFC3339 with fallback
- `claim/claim.go:141`: `parseExpiry(s string) (time.Time, error)` -- similar pattern, local to claim

**Rust opportunity:** `chrono::DateTime::parse_from_rfc3339()` provides standard parsing. A single `util::parse_timestamp()` function in a shared module.

### 3.7 High: Verdict Struct Duplication

**Files:** 8 separate `Verdict` struct definitions across hooks sub-packages:

| Package | File | Fields |
|---------|------|--------|
| `hooks/gate` | `gate.go:60` | Allow, GateType, Reason |
| `hooks/sentinel` | `stage.go:21` | Allow, Reason |
| `hooks/team` | `guard.go:15` | Allow, Reason |
| `hooks/edit` | `edit.go:12` | Allow, Message |
| `hooks/ghpr` | `ghpr.go:14` | Allow, Reason |
| `hooks/resource` | `resource.go:29` | Allow, Tier, Path, Message |
| `hooks/webfetch` | `webfetch.go:13` | Allow, Reason, AlwaysBlocked, Domain |
| `hooks/security` | `checker.go:6` | Allow, Category, Reason, Pattern, Module |

Each hook sub-package defines its own `Verdict` struct with overlapping fields. Go's package-scoped types make this the natural pattern, but it creates 8 nearly-identical types.

**Rust opportunity:** A single `hook::Verdict` enum with variant-specific data:
```rust
enum HookVerdict {
    Allow,
    Block { reason: String, category: BlockCategory },
    Warn { message: String },
}
```

### 3.8 Moderate: hookInput Struct Duplication

**Files:** 9 definitions across hook sub-packages:

| Package | File:Line | Struct Name | Fields |
|---------|----------|-------------|--------|
| `hooks/gate` | `gate.go:133` | `HookInput` (exported) | ToolName, ToolInput |
| `hooks/security` | `context.go:11` | `HookInput` (exported) | ToolName, ToolInput |
| `hooks/edit` | `edit.go:36` | `hookInput` | ToolName, ToolInput |
| `hooks/resource` | `resource.go:37` | `hookInput` | ToolName, ToolInput |
| `hooks/webfetch` | `webfetch.go:50` | `hookInput` | ToolName, ToolInput |
| `hooks/team` | `guard.go:24` | `hookInput` | ToolName, ToolInput |
| `hooks/session` | `start.go:77` | `hookInput` | ToolName, ToolInput |
| `hooks/ghpr` | `ghpr.go:31` | `hookInput` | ToolName, ToolInput |
| `hooks/sentinel` | `stage.go:31` | `postToolUseInput` | ToolName, ToolInput |

Nine hook packages define their own input struct with identical fields (`ToolName string`, `ToolInput json.RawMessage`). Two are exported (`HookInput` in gate and security), six are unexported (`hookInput`), and one uses a different name (`postToolUseInput` in sentinel). Go's internal package visibility makes sharing across packages awkward without a shared sub-package.

**Rust opportunity:** Single `hook::HookInput` struct in a shared `hook` module, used by all hook handlers.

### 3.9 Moderate: Protected Branch Matching

**Files:** `githooks/prepush.go:129-146` and `hooks/edit/edit.go:159-172`

Both implement branch protection pattern matching (exact match + glob wildcard):

- `githooks/prepush.go:129`: `isProtectedBranch()` + `matchBranchPattern()` with `/*` suffix handling
- `hooks/edit/edit.go:159`: `isProtectedBranch()` with `filepath.Match()` for `*` patterns

Slightly different implementations of the same concept.

**Rust opportunity:** Single `enforcement::is_protected_branch()` function in a shared module.

### 3.10 Moderate: Session JSONL Event Writing

**Files:** `session/session.go:262-285` and `ledger/writer.go`

The session package (`session/session.go:262`) implements `writeJSONLEvent()` which writes directly to the sessions JSONL file, bypassing the `ledger.Writer` that provides file locking (`syscall.Flock` at `writer.go:99`) and event routing (`routing.go`).

**Rust opportunity:** All JSONL writes go through a single `ledger::Writer` with a `write()` method. The session module uses the same writer, ensuring consistent locking and routing.

### 3.11 Moderate: EnforcementPolicy Struct Duplication

**Files:** `githooks/config.go:11-15` and `hooks/security/` (via separate `EnforcementPolicy` type)

The `githooks` package defines its own `EnforcementPolicy` struct subset focused on git format rules, while `hooks/security` defines a fuller policy type. Both parse the same `enforcement-policy.json` file independently.

**Rust opportunity:** Single `enforcement::Policy` struct with all fields. Each consumer uses only the fields it needs (zero-cost in Rust -- unused fields don't incur runtime cost).

### 3.12 Low: Config File Loading Pattern

**Files:** `githooks/config.go:43-56`, `cliutil/config.go:65-78`, `hooks/resource/resource.go:53-69`

Multiple packages implement the same JSON-file-loading pattern: `os.ReadFile()` -> `json.Unmarshal()` into a local struct. Each has its own error wrapping.

**Rust opportunity:** Generic `config::load<T: DeserializeOwned>(path)` function.

## 4. Coupling Analysis

### 4.1 Import Dependency Graph

```
                    +-----------+
                    |    db     |  (0 internal deps -- foundation)
                    +-----+-----+
                          |
          +-------+-------+--------+--------+
          |       |       |        |        |
       session  workgraph autorun  doctor  initialize
          |       |                          |
          |    +--+--+                    preflight
          |    | idgen|
          |    | ledger|
          |    +------+
          |
       +--+--+
       |claim|-----> idgen, ledger
       +-----+

       pathflow -----> ledger
       hooks/session -> db, pathflow, session  (3 deps)
       hooks/team ----> pathflow
       hooks/resource -> hooks/security
       hooks/sentinel -> (none -- standalone)
       hooks/gate -----> (none -- standalone)
```

### 4.2 Coupling Hotspots

| Package | Internal Import Count | Risk |
|---------|----------------------|------|
| `hooks/session` (start.go) | 3 (db, pathflow, session) | MODERATE -- significant but manageable coupling |
| `workgraph` | 3 (db, idgen, ledger) | MODERATE -- data layer coupling |
| `session` | 2 (db, ulid) | LOW -- clean dependency |
| `claim` | 2 (idgen, ledger) | LOW -- clean dependency |
| `autorun` | 2 (db, ulid) | LOW -- clean dependency |
| `initialize` | 2 (db, preflight) | LOW -- clean dependency |
| `pathflow` | 1 (ledger) | LOW |
| `doctor` | 1 (db) | LOW |

### 4.3 Shared Globals and Singletons

| Pattern | Location | Risk |
|---------|----------|------|
| `db.Get()` singleton via `sync.Once` | `db/connection.go:46-68` | HIGH -- hidden global state, complicates testing |
| `cliutil.logger` singleton via `sync.Once` | `cliutil/log.go:17-19` | LOW -- logging is inherently global |
| Package-level compiled regexes | `hooks/gate/gate.go:47-57`, `hooks/sentinel/stage.go:15` | LOW -- immutable, safe |
| `githooks.DefaultPolicy()` hardcoded fallback | `githooks/config.go:60-115` | MODERATE -- policy should be injected |

### 4.4 Circular Dependency Risk

No actual circular dependencies exist in the current codebase. Go's compiler enforces this. However, the following packages have conceptual coupling that could become circular if boundaries shift:

- `session` <-> `workstate`: Both manage session runtime state -- `session` handles lifecycle (start/end) while `workstate` manages `active-task.json`. No package currently imports both, but they operate on the same `.state/runtime/` directory. A Rust redesign should merge these into a single `codeflow-session` crate (see Section 6.2).
- `db/sync.go` <-> `ledger/normalize.go`: Duplicated normalization logic suggests these should share code, but Go package boundaries prevent it without creating a third package. Rust modules solve this naturally.

## 5. Go-Specific Workarounds

### 5.1 Untyped JSON Handling -- `map[string]any`

**Prevalence:** 612 occurrences across 61 files

The most pervasive Go workaround. Used for:

- JSONL event parsing (`db/sync.go`, `ledger/normalize.go`, `workgraph/workgraph.go`)
- Hook stdin payloads (`hooks/*/`)
- Config file processing (`config/config.go`)
- Shadow test normalization (`shadowtest/harness.go`)

**Key file:line examples:**
- `db/sync.go:570`: `getString(m map[string]any, key string) string`
- `workgraph/workgraph.go:103`: `getString(m map[string]any, key string) string`
- `config/config.go:205`: `switch existing.(type)` -- type assertion cascade
- `ledger/event.go:1-93`: Custom `MarshalJSON`/`UnmarshalJSON` for flat JSONL format

**Rust replacement:** `serde` derives on typed structs. Each event type gets its own struct with `#[serde(tag = "event")]` for discriminated unions. Zero runtime type assertions needed.

### 5.2 String-Typed Enum Maps

**Prevalence:** 15+ instances of `map[string]bool` used as enum validation sets

| Location | Variable | Values |
|----------|----------|--------|
| `pathflow/transitions.go:35-58` | `validPhases` | PF1-INIT through PF7-END |
| `pathflow/transitions.go` | `validStages` | WS-DEV, WS-REV, etc. |
| `pathflow/transitions.go` | `validPhaseStatuses` | entered, completed, skipped |
| `pathflow/transitions.go` | `validStageStatuses` | in_progress, completed, etc. |
| `pathflow/transitions.go` | `validStageVerdicts` | approved, changes_requested, fail |
| `pathflow/transitions.go` | `validInteractionModes` | interactive, autorun |
| `pathflow/transitions.go` | `validTaskStatuses` | pending, in_progress, etc. |
| `workgraph/workgraph.go:35-58` | `ValidEpicStatuses` | todo, in_progress, etc. |
| `workgraph/workgraph.go` | `ValidTaskStatuses` | todo, in_progress, etc. |
| `workgraph/workgraph.go` | `ValidAreaTypes` | INF, PLN, DOC, etc. |
| `workgraph/workgraph.go` | `ValidWorkTypes` | FEAT, FIX, RFCT, etc. |
| `workgraph/workgraph.go` | `ValidDomains` | GENL, HOOK, DATA, etc. |
| `ledger/schema.go` | `requiredFields` map | event_type -> []string |

**Rust replacement:** `enum` types with `#[derive(Serialize, Deserialize)]`:
```rust
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase { Pf1Init, Pf2Context, Pf3Classify, ... }

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WorkType { Feat, Fix, Rfct, Cicd, ... }
```

### 5.3 sql.NullString / sql.NullInt64

**Prevalence:** All 10 model structs in `db/models.go:1-186`

Every database model uses `sql.NullString` and `sql.NullInt64` for nullable columns:

```go
// db/models.go
type Task struct {
    ID           string
    FormatID     sql.NullString  // Rust: Option<String>
    EpicID       sql.NullString
    Title        string
    Status       string
    Stage        sql.NullString
    StageStatus  sql.NullString
    ...
}
```

Each nullable field requires explicit `.Valid` checks and `.String` access throughout the codebase.

**Rust replacement:** `Option<String>` and `Option<i64>`. The `surrealdb` crate handles optional fields natively via SurrealQL `option<T>` types. Zero boilerplate.

### 5.4 Manual JSON Marshaling

**Prevalence:** 4 custom marshal implementations, 630 `fmt.Errorf` with `%w` wrapping

- `ledger/event.go:1-93`: Custom `MarshalJSON`/`UnmarshalJSON` to produce flat JSONL (merges `Data map[string]any` into top-level fields)
- `db/sync.go:570-595`: Manual map[string]any field extraction
- `workgraph/workgraph.go:103-139`: Duplicate manual field extraction

**Rust replacement:** `#[serde(flatten)]` attribute handles the flat JSONL merge:
```rust
#[derive(Serialize, Deserialize)]
struct Event {
    event: String,
    timestamp: String,
    session_id: String,
    #[serde(flatten)]
    data: HashMap<String, Value>,
}
```

### 5.5 fmt.Errorf Wrapping Chains

**Prevalence:** 630 occurrences across the codebase (source + test files)

Every error in the codebase follows the `fmt.Errorf("context: %w", err)` pattern. This creates verbose, repetitive error wrapping code.

**Example from `db/connection.go:88`:**
```go
return nil, fmt.Errorf("db: opening database %s: %w", path, err)
```

**Rust replacement:** `thiserror` crate with derive macros:
```rust
#[derive(Debug, thiserror::Error)]
enum DbError {
    #[error("opening database {path}")]
    Open { path: String, #[source] source: surrealdb::Error },
    ...
}
```

### 5.6 interface{} Type Assertions

**Prevalence:** 33 occurrences across 5 files

Used primarily in:
- `config/config.go:205`: `switch existing.(type)` for config value type coercion
- `shadowtest/harness.go:214,267`: `event["event"].(string)` for JSONL field access
- `db/queries.go:40`: `[]byte` to `string` conversion in generic query results

**Rust replacement:** Enum types and pattern matching. Config values become a `ConfigValue` enum with typed variants.

### 5.7 syscall.Flock File Locking

**Prevalence:** 2 independent implementations

- `ledger/writer.go:99`: `syscall.Flock(fd, syscall.LOCK_EX)` for JSONL write locking
- `pathflow/checkpoint.go:427`: `syscall.Flock(fd, syscall.LOCK_EX)` for checkpoint file locking

Both use Unix-specific `syscall.Flock` which is not portable to Windows.

**Rust replacement:** `fs2` crate provides cross-platform file locking via `file.lock_exclusive()`. Or `fd-lock` for a minimal alternative.

### 5.8 Cobra Command Tree Boilerplate

**Prevalence:** 26 top-level commands, each requiring manual registration

`cmd/codeflow/main.go:44-69` manually calls `rootCmd.AddCommand()` for each subcommand. Each subcommand file (30 files) creates a `*cobra.Command` with manually bound flags.

**Rust replacement:** `clap` with derive macros:
```rust
#[derive(Parser)]
#[command(name = "codeflow")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Hooks(HooksArgs),
    Pathflow(PathflowArgs),
    ...
}
```

### 5.9 Singleton Database Pattern

**Location:** `db/connection.go:46-68`

```go
var (
    instance *DB
    once     sync.Once
    mu       sync.Mutex
)

func Get(path string) *DB {
    mu.Lock()
    defer mu.Unlock()
    once.Do(func() { ... })
    return instance
}
```

Uses `sync.Once` + `sync.Mutex` for lazy singleton initialization. `Reset()` at `:72` allows test teardown but requires recreating `sync.Once`.

**Rust replacement:** `once_cell::sync::Lazy` or `std::sync::OnceLock`. Or better: pass `&Database` via dependency injection (Rust's borrow checker makes this natural and safe).

### 5.10 Error Sentinel Variables

**Prevalence:** 30+ sentinel error variables across packages

Every package defines `var Err* = errors.New(...)`:
- `session/session.go:20-28`: 3 sentinel errors
- `db/connection.go:18-27`: 3 sentinel errors
- `update/update.go:17-27`: 4 sentinel errors
- `initialize/wizard.go:20-37`: 6 sentinel errors
- `cliutil/validate.go:12-14`: 2 sentinel errors

**Rust replacement:** `thiserror` enum variants. Each module's errors become an enum:
```rust
#[derive(Debug, thiserror::Error)]
enum SessionError {
    #[error("claude ID must not be empty")]
    EmptyClaudeId,
    #[error("no active session")]
    NoActiveSession,
    #[error("session already ended")]
    AlreadyEnded,
}
```

## 6. Go-to-Rust Module Mapping

### 6.1 Proposed Rust Crate Structure

```
codeflow-rs/                          # Rust workspace (coexists with codeflow-cli/ during migration)
  Cargo.toml                    # workspace root
  crates/
    codeflow-core/              # Shared types, errors, config
      src/
        lib.rs
        config.rs               # <-- config + cliutil/config.go + enforcement policy
        error.rs                # <-- cliutil/errors.go + all sentinel errors
        logging.rs              # <-- cliutil/log.go
        validate.rs             # <-- cliutil/validate.go
        types/
          mod.rs
          phase.rs              # <-- pathflow/transitions.go enums
          stage.rs              # <-- pathflow/transitions.go enums
          work.rs               # <-- workgraph enums (status, area, work_type)
          event.rs              # <-- ledger/event.go + ledger/schema.go

    codeflow-db/                # Database layer
      src/
        lib.rs
        connection.rs           # <-- db/connection.go
        schema.rs               # <-- db/schema.sql (embedded)
        migrate.rs              # <-- db/migrate.go
        models.rs               # <-- db/models.go (with Option<T> instead of sql.Null*)
        queries.rs              # <-- db/queries.go (typed via surrealdb SurrealQL)
        sync.rs                 # <-- db/sync.go + ledger/normalize.go (MERGED)
        crdt.rs                 # <-- db/crdt.go

    codeflow-ledger/            # JSONL event system
      src/
        lib.rs
        writer.rs               # <-- ledger/writer.go + session/writeJSONLEvent (MERGED)
        routing.rs              # <-- ledger/routing.go (canonical file constants)
        normalize.rs            # <-- DELETED (merged into codeflow-db::sync)

    codeflow-pathflow/          # PathFlow lifecycle
      src/
        lib.rs
        checkpoint.rs           # <-- pathflow/checkpoint.go
        sentinel.rs             # <-- pathflow/sentinel.go + sentinel/sentinel.go (MERGED)
        transitions.rs          # <-- pathflow/transitions.go

    codeflow-session/           # Session management
      src/
        lib.rs
        session.rs              # <-- session/session.go
        workstate.rs            # <-- workstate/activetask.go + workstate/memory.go (MERGED)

    codeflow-workgraph/         # Work item management
      src/
        lib.rs
        service.rs              # <-- workgraph/workgraph.go
        create.rs               # <-- workgraph/create.go
        claim.rs                # <-- claim/claim.go

    codeflow-hooks/             # Hook enforcement
      src/
        lib.rs
        verdict.rs              # <-- UNIFIED Verdict type (replaces 8 duplicates)
        input.rs                # <-- UNIFIED HookInput (replaces 9 duplicates)
        security/
          mod.rs                # <-- hooks/security/checker.go
          modules.rs            # <-- hooks/security/*.go (8 modules)
        gate.rs                 # <-- hooks/gate/gate.go
        sentinel.rs             # <-- hooks/sentinel/stage.go
        team.rs                 # <-- hooks/team/guard.go
        edit.rs                 # <-- hooks/edit/edit.go
        resource.rs             # <-- hooks/resource/resource.go
        webfetch.rs             # <-- hooks/webfetch/webfetch.go
        session.rs              # <-- hooks/session/*.go
        logging.rs              # <-- hooks/logging/*.go
        prompt.rs               # <-- hooks/prompt/*.go

    codeflow-githooks/          # Git hook enforcement
      src/
        lib.rs
        commitmsg.rs            # <-- githooks/commitmsg.go
        prepush.rs              # <-- githooks/prepush.go
        config.rs               # <-- githooks/config.go (uses codeflow-core::config)
        precommit.rs            # <-- githooks/precommitvalidate.go
        postcommit.rs           # <-- githooks/postcommit.go
        preparecommitmsg.rs     # <-- githooks/preparecommitmsg.go

    codeflow-cli/               # CLI binary (Clap dispatch)
      src/
        main.rs                 # <-- cmd/codeflow/main.go
        commands/
          mod.rs
          hooks.rs              # <-- cmd/codeflow/hooks.go (7 PreToolUse + 4 PostToolUse + ...)
          pathflow.rs           # <-- cmd/codeflow/pathflow.go
          session.rs            # <-- cmd/codeflow/session.go
          db.rs                 # <-- cmd/codeflow/db.go
          workgraph.rs          # <-- cmd/codeflow/workgraph.go
          validate.rs           # <-- cmd/codeflow/validate.go
          ... (remaining subcommands)

    codeflow-autorun/           # Autorun orchestrator
      src/
        lib.rs
        orchestrator.rs         # <-- autorun/orchestrator.go
        worker.rs               # <-- autorun/worker.go + worker_real.go
        batch.rs                # <-- autorun/batch.go

    codeflow-tools/             # Utility commands
      src/
        lib.rs
        doctor.rs               # <-- doctor/checks.go
        init.rs                 # <-- initialize/wizard.go
        preflight.rs            # <-- preflight/preflight.go
        update.rs               # <-- update/update.go
        report.rs               # <-- report/
        welcome.rs              # <-- welcome/
        shadowtest.rs           # <-- shadowtest/ (dev-only, behind feature flag)
        settings.rs             # <-- settings/validate.go
        worktree.rs             # <-- worktree/worktree.go
```

### 6.2 Consolidation Decisions

| Consolidation | Go Source | Rust Target | Rationale |
|--------------|-----------|-------------|-----------|
| Normalization merge | `db/sync.go` + `ledger/normalize.go` | `codeflow-db::sync` | Eliminate the largest DRY violation |
| Sentinel merge | `pathflow/sentinel.go` + `sentinel/sentinel.go` | `codeflow-pathflow::sentinel` | Unified sentinel API with scope parameter |
| Session + workstate merge | `session/` + `workstate/` | `codeflow-session` | Both manage session runtime state |
| Verdict unification | 8 `Verdict` structs | `codeflow-hooks::verdict` | Single enum type |
| HookInput unification | 9 input structs | `codeflow-hooks::input` | Single shared struct |
| File constants merge | `ledger/routing.go` + `db/sync.go` constants | `codeflow-ledger::routing` | Single source of truth |
| Config loading unification | 3 config loaders | `codeflow-core::config` | Generic `load<T>()` function |

### 6.3 Split Decisions

| Split | Go Source | Rust Target | Rationale |
|-------|-----------|-------------|-----------|
| hooks sub-packages | `hooks/` (11 sub-pkgs, 6,531 lines) | `codeflow-hooks/` crate with modules | Already well-structured; keep module boundaries |
| autorun | Part of `cmd/autorun/` | `codeflow-autorun` crate | Independent binary with own dependency set |
| claim extraction | `claim/` embedded in main binary | `codeflow-workgraph::claim` module | Logically part of work item management |

### 6.4 Packages with No Direct Rust Equivalent

| Go Package | Reason | Rust Handling |
|------------|--------|---------------|
| `claude` | Doc-only stub (2 lines) | Remove -- no content |
| `benchmark` | Test-only (benchmarks) | `#[bench]` or `criterion` in relevant crates |
| `integration` | Test-only (integration tests) | `tests/` directory in workspace root |
| `verification` | Test-only + doc | `tests/` directory |
| `testutil` | Test helpers | `dev-dependencies` helper crate or `#[cfg(test)]` modules |
| `idgen` | ULID generation (37 lines) | `ulid` crate dependency; `format_id` logic moves to `codeflow-core::types` |

## 7. CLI Subcommand Mapping

### 7.1 Top-Level Commands

| # | Command | Go Handler File | Test File | Clap Subcommand |
|---|---------|----------------|-----------|-----------------|
| 1 | `version` | `cmd/codeflow/version.go` | `version_test.go` | `Commands::Version` |
| 2 | `uninstall` | `cmd/codeflow/uninstall.go` | `uninstall_test.go` | `Commands::Uninstall` |
| 3 | `db` | `cmd/codeflow/db.go` | `db_test.go` | `Commands::Db(DbArgs)` |
| 4 | `session` | `cmd/codeflow/session.go` | `session_test.go` | `Commands::Session(SessionArgs)` |
| 5 | `init` | `cmd/codeflow/init.go` | `init_test.go` | `Commands::Init` |
| 6 | `doctor` | `cmd/codeflow/doctor.go` | `doctor_test.go` | `Commands::Doctor` |
| 7 | `config` | `cmd/codeflow/config.go` | `config_test.go` | `Commands::Config(ConfigArgs)` |
| 8 | `update` | `cmd/codeflow/update.go` | `update_test.go` | `Commands::Update` |
| 9 | `autorun` | `cmd/autorun/` (separate package) | (in cmd/autorun/) | `Commands::Autorun(AutorunArgs)` |
| 10 | `ledger` | `cmd/codeflow/ledger.go` | `ledger_test.go` | `Commands::Ledger(LedgerArgs)` |
| 11 | `welcome` | `cmd/codeflow/main.go:89` | `main_test.go` | `Commands::Welcome` |
| 12 | `internal` | `cmd/codeflow/internal.go` | `internal_test.go` | `Commands::Internal(InternalArgs)` |
| 13 | `pathflow` | `cmd/codeflow/pathflow.go` | `pathflow_test.go` | `Commands::Pathflow(PathflowArgs)` |
| 14 | `state` | `cmd/codeflow/state.go` | `state_test.go` | `Commands::State(StateArgs)` |
| 15 | `validate` | `cmd/codeflow/validate.go` | `validate_test.go` | `Commands::Validate(ValidateArgs)` |
| 16 | `hooks` | `cmd/codeflow/hooks.go` | `hooks_test.go` | `Commands::Hooks(HooksArgs)` |
| 17 | `sentinel` | `cmd/codeflow/sentinel.go` | `sentinel_test.go` | `Commands::Sentinel(SentinelArgs)` |
| 18 | `coordination` | `cmd/codeflow/coordination.go` | `coordination_test.go` | `Commands::Coordination(CoordinationArgs)` |
| 19 | `settings` | `cmd/codeflow/settings.go` | `settings_test.go` | `Commands::Settings(SettingsArgs)` |
| 20 | `worktree` | `cmd/codeflow/worktree.go` | `worktree_test.go` | `Commands::Worktree(WorktreeArgs)` |
| 21 | `report` | `cmd/codeflow/report.go` | `report_test.go` | `Commands::Report(ReportArgs)` |
| 22 | `workgraph` | `cmd/codeflow/workgraph.go` | `workgraph_test.go` | `Commands::Workgraph(WorkgraphArgs)` |
| 23 | `git-hooks` | `cmd/codeflow/githooks.go` | `githooks_test.go` | `Commands::GitHooks(GitHooksArgs)` |
| 24 | `shadow-test` | `cmd/codeflow/shadowtest.go` | `shadowtest_test.go` | `Commands::ShadowTest(ShadowTestArgs)` |
| 25 | `normalize` | `cmd/codeflow/normalize.go` | `normalize_test.go` | `Commands::Normalize(NormalizeArgs)` |
| 26 | `test` | `cmd/codeflow/test.go` | `test_test.go` | `Commands::Test(TestArgs)` |

### 7.2 Nested Subcommands (hooks)

| Parent | Sub | Sub-Sub | Handler Function |
|--------|-----|---------|-----------------|
| `hooks` | `pre-tool-use` | `security` | `runPreToolUseSecurity` in `hooks.go` |
| `hooks` | `pre-tool-use` | `gate-check` | `runPreToolUseGateCheck` in `hooks.go` |
| `hooks` | `pre-tool-use` | `edit-write-guard` | `hooks_editwrite.go` |
| `hooks` | `pre-tool-use` | `gh-pr-guard` | `runPreToolUseGhPrGuard` in `hooks.go` |
| `hooks` | `pre-tool-use` | `protection-guard` | `runPreToolUseProtectionGuard` in `hooks.go` |
| `hooks` | `pre-tool-use` | `team-guard` | `runPreToolUseTeamGuard` in `hooks.go` |
| `hooks` | `pre-tool-use` | `webfetch-guard` | `runPreToolUseWebfetchGuard` in `hooks.go` |
| `hooks` | `post-tool-use` | `sentinel-write` | `runPostToolUseSentinelWrite` in `hooks.go` |
| `hooks` | `post-tool-use` | `settings-validate` | `runPostToolUseSettingsValidate` in `hooks.go` |
| `hooks` | `post-tool-use` | `checkpoint-register` | `runPostToolUseCheckpointRegister` in `hooks.go` |
| `hooks` | `post-tool-use` | `logging` | `hooks_logging.go` |
| `hooks` | `task-completed` | `checkpoint-complete` | `runTaskCompletedCheckpointComplete` in `hooks.go` |
| `hooks` | `session-start` | `init` | `runSessionStartInit` in `hooks.go` |
| `hooks` | `session-start` | `instructions` | `runSessionStartInstructions` in `hooks.go` |
| `hooks` | `session-start` | `logging` | `hooks_logging.go` |
| `hooks` | `session-end` | `cleanup` | `runSessionEndCleanup` in `hooks.go` |
| `hooks` | `session-end` | `logging` | `hooks_logging.go` |
| `hooks` | `stop` | `logging` | `hooks_logging.go` |
| `hooks` | `user-prompt-submit` | `validate` | `hooks_prompt_validate.go` |
| `hooks` | `user-prompt-submit` | `logging` | `hooks_logging.go` |

### 7.3 Nested Subcommands (pathflow)

| Parent | Sub | Handler Function |
|--------|-----|-----------------|
| `pathflow` | `phase-transition` | `pathflow.go` |
| `pathflow` | `stage-transition` | `pathflow.go` |
| `pathflow` | `session-register` | `pathflow.go` |
| `pathflow` | `session-metadata` | `pathflow.go` |
| `pathflow` | `task-update` | `pathflow.go` |

## 8. Recommendations

### 8.1 Priority Order for Rust Implementation

1. **Phase 0A (this audit):** Complete -- provides the roadmap.
2. **Phase 0B (codeflow-core):** Shared types, enums, error hierarchy. Everything depends on this.
3. **Phase 0C (codeflow-db + codeflow-ledger):** Data layer with unified normalization. Eliminates the largest DRY violations.
4. **Phase 0D (codeflow-pathflow + codeflow-session):** Lifecycle management with merged sentinel implementation.
5. **Phase 0E (codeflow-hooks):** Hook enforcement with unified Verdict type. Largest package by line count.
6. **Phase 0F (codeflow-workgraph):** Work item management including claim.
7. **Phase 0G (codeflow-cli):** Clap dispatch layer connecting all crates.
8. **Phase 0H (codeflow-autorun + codeflow-tools):** Remaining commands.
9. **Phase 0I (codeflow-githooks):** Git hook enforcement (can run in parallel with 0H).

### 8.2 Key Architectural Decisions for Rust

| Decision | Recommendation | Rationale |
|----------|---------------|-----------|
| Workspace layout | Cargo workspace with 10 crates | Mirrors Go package boundaries with consolidations applied |
| Error handling | `thiserror` for library crates, `anyhow` for CLI binary | Clean error hierarchy in libraries, convenience in binary |
| JSON handling | `serde` + `serde_json` throughout | Eliminates all `map[string]any` and manual marshaling |
| Database | `surrealdb` crate (embedded `surrealkv://`) | Replaces legacy Go database driver with SurrealDB embedded |
| CLI framework | `clap` with derive macros | Direct replacement for Cobra with less boilerplate |
| File locking | `fs2` crate | Cross-platform replacement for `syscall.Flock` |
| ID generation | `ulid` crate | Direct replacement for `oklog/ulid` |
| Async model | Synchronous (no async runtime) | CLI tool with synchronous SurrealDB embedded; async adds complexity without benefit |
| Testing | Built-in `#[test]` + `assert_cmd` for CLI integration | Replaces `testing.T` + `testutil.TempProject` |

### 8.3 Risk Areas

| Risk | Mitigation |
|------|-----------|
| Shadow test parity during migration | Keep Go binary as reference; run shadow tests comparing Rust output to Go output |
| Hook latency regression | Rust cold-start is ~0ms (no runtime init); benchmark each hook independently |
| SurrealDB migration compatibility | Port schema from SQL to SurrealQL; `surrealdb` crate uses embedded `surrealkv://` mode |
| JSONL format compatibility | Use same serde serialization format; run normalization comparison tests |
| Behavioral parity in 20+ hooks | Port shadow test registry (`shadowtest/registry.go`) to Rust integration tests |
