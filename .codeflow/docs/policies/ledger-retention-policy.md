---
title: "Ledger Retention Policy"
type: policy
status: active
applies_to:
  - ".state/ledger/"
  - ".state/ledger/sessions/"
  - ".state/ledger/work-graph/"
  - ".state/ledger/memory-events/"
  - ".state/ledger/config/"
created: "2026-04-07"
last_updated: "2026-04-07"
---

# Ledger Retention Policy

This policy governs all Tier 0 JSONL ledger files in the CodeFlow three-tier data model. It defines
the NEVER-delete rule, archive thresholds, compression, per-file rotation triggers, the Rust CLI
interface for applying these policies, and the impact of parallel sessions on retention operations.

The retention policy analysis in `schema-standardization.md` Section 4 governs this work.
The downstream task INF-TSK-024-018 implements this policy as a Rust CLI subcommand. The
`RetentionError` enum defined in Section 10 is the authoritative error contract for that
implementation.

## Table of Contents

- [1. Overview](#1-overview)
- [2. Fundamental Rule: NEVER Delete Tier 0 Data](#2-fundamental-rule-never-delete-tier-0-data)
- [3. Archive Policy](#3-archive-policy)
- [4. Compression Policy](#4-compression-policy)
- [5. Per-File Rotation Thresholds](#5-per-file-rotation-thresholds)
- [6. Size-Based Rotation](#6-size-based-rotation)
- [7. CLI Interface Specification](#7-cli-interface-specification)
- [8. Parallel Session Impact](#8-parallel-session-impact)
- [9. Configuration](#9-configuration)
- [10. Error Contract](#10-error-contract)
- [11. Verification](#11-verification)

## 1. Overview

CodeFlow uses a three-tier data model:

| Tier | Location | Role |
|------|----------|------|
| 0 — JSONL ledger | `.state/ledger/` | **Rebuild authority** — immutable, append-only event log |
| 1 — SurrealDB | `.state/db/codeflow.db` | Query interface, rebuilt from Tier 0 |
| 2 — Markdown | `project-management/`, `.claude/memory/` | Human-readable derived views |

If Tier 1 is lost, the system rebuilds it from Tier 0. If Tier 0 is lost, that data is gone
permanently — no rebuild is possible. This asymmetry is the foundation of every policy in this
document.

With parallel sessions (multiple git worktrees writing concurrently, introduced in INF-EPC-023),
each worktree writes to its own fragment directory under `.state/ledger/`. Write volume grows
proportionally with session concurrency. Without retention policies, ledger storage grows without
bound.

## 2. Fundamental Rule: NEVER Delete Tier 0 Data

**Tier 0 JSONL ledger files are NEVER deleted. They are only archived.**

This rule has no exceptions. Any operation that would permanently remove a Tier 0 ledger file is a
`NeverDeleteViolation` and is rejected by the CLI with exit code 1.

### Rationale

- Tier 0 is the rebuild authority. Deleting it destroys the ability to reconstruct Tier 1 (SurrealDB)
  after corruption or migration.
- JSONL files are append-only by design. Their integrity depends on complete, contiguous records.
  Partial deletion (pruning old entries) would leave a gap in the event stream, breaking replay.
- The SurrealDB embedded database (Tier 1) is explicitly **not** git-tracked. JSONL (Tier 0) is
  git-tracked and serves as the durable record.
- Archive + compression achieves the storage goal (reducing active disk usage) without sacrificing
  the data.

### What "NEVER delete" means in practice

| Allowed | Not Allowed |
|---------|------------|
| Move to `.state/ledger/archive/` | `rm` or `unlink` on any `.jsonl` file |
| Compress with gzip into `.jsonl.gz` | Truncating a `.jsonl` file |
| Rename during rotation (atomic) | Pruning individual records from a file |
| Archive entire old fragment directories | Deleting the archive destination itself |

The CLI enforces this rule programmatically. The `RetentionError::NeverDeleteViolation` variant
(Section 10) is returned before any delete operation executes.

## 3. Archive Policy

### Trigger

A Tier 0 JSONL file is eligible for archiving when it is older than **90 days** (measured from the
file's last modification time, or from the date suffix in the fragment filename).

### Destination

Archived files move to `.state/ledger/archive/`. The archive directory is structured to mirror the
source layout:

```text
.state/ledger/archive/
├── sessions/
│   └── sessions-ses-01kmk7pky5f2kn1ajctwffmsx1.2026-01-05.jsonl.gz
├── work-graph/
│   └── work-graph-ses-01kmk7pky5f2kn1ajctwffmsx1.2026-01-05.jsonl.gz
├── memory-events/
│   └── memory-events-ses-01kmk7pky5f2kn1ajctwffmsx1.2026-01-05.jsonl.gz
└── config/
    └── config.2026-01-05.jsonl.gz
```

### Archive file naming

The archive filename uses the pattern: `{original-stem}.{YYYY-MM-DD}.jsonl.gz`

- For fragment files: `sessions-ses-01kmk7.2026-01-05.jsonl.gz`
- For monolithic files: `sessions.2026-01-05.jsonl.gz`
- Date component is the archival date (when the file was archived), not the creation date.

### Archive procedure

1. Verify the source file is not currently locked (`.jsonl.lock` file check).
2. Compress the source into the archive path using gzip (flate2 crate, see Section 4).
3. Verify the compressed archive is readable (decompress header check).
4. Remove the source file only after successful archive verification — the source removal is the one
   case where deletion is allowed, because the data still exists in the archive.

## 4. Compression Policy

### Tool

Compression uses the `flate2` Rust crate (gzip format). The implementation task INF-TSK-024-018
adds `flate2` to `codeflow-cli/core/Cargo.toml`. The `fs2` crate (advisory locking) is already
present in `codeflow-cli/core/Cargo.toml`.

### Format

All archived files use gzip compression (`.gz` suffix). The `flate2::write::GzEncoder` writer
wraps a `std::fs::File` and writes the compressed output.

### When to compress

| Trigger | Target | Action |
|---------|--------|--------|
| Age >= 90 days | Any ledger file | Archive to `.state/ledger/archive/` with gzip compression |
| Age >= 7 days, size >= 1MB | Any fragment file | Compress in-place (no move) — `--compress` flag |
| Explicit `--compress` flag | All eligible files | Compress files >= 7 days old |

Compression does not happen to files currently being written by active sessions. The parallel session
impact analysis (Section 8) explains how active files are identified.

### Compression level

Use `flate2::Compression::best()` for archived files (cold data, compression ratio matters more than
speed). Use `flate2::Compression::fast()` for in-place compression of warm files.

## 5. Per-File Rotation Thresholds

Rotation creates a dated snapshot of the current file and starts a new empty file. Archived rotation
is distinct from archiving — archived files retain the full data as a compressed historical record.

### Monolithic files (legacy, root of `.state/ledger/`)

These files accumulate events from all sessions in the main worktree. In parallel session mode
(post-INF-EPC-023), new writes go to fragment directories, but monolithic files may still accumulate
from older sessions.

| File | Size Limit | Archive Age |
|------|-----------|-------------|
| `sessions.jsonl` | 10MB | 90 days |
| `work-graph.jsonl` | 10MB | 90 days |
| `memory-events.jsonl` | 10MB | 90 days |
| `config.jsonl` | 5MB | 180 days |

`config.jsonl` uses a longer retention window (180 days) because configuration events are low-volume
and high-value for auditing configuration changes across the project lifetime.

### Fragment directories (per-worktree, post-PR #221)

Each git worktree session writes to its own fragment file. Fragments are self-contained and
session-scoped, which simplifies retention: when a session is complete and its worktree is removed,
the fragment is cold data.

| Directory | Fragment pattern | Size limit per fragment | Archive age |
|-----------|-----------------|------------------------|-------------|
| `sessions/` | `sessions-ses-{ID}.jsonl` | 5MB | 90 days |
| `work-graph/` | `work-graph-ses-{ID}.jsonl` | 10MB | 90 days |
| `memory-events/` | `memory-events-ses-{ID}.jsonl` | 5MB | 90 days |
| `config/` | `config-ses-{ID}.jsonl` | 2MB | 180 days |

Fragment files for sessions whose worktrees no longer exist (i.e., the session is complete) are
immediately eligible for archiving regardless of age, because they are guaranteed cold data.

### Additional ledger directories

These directories may exist but do not map to the four primary ledger files above. They follow the
same 90-day archive age and 10MB size limit unless otherwise noted:

| Directory | Notes |
|-----------|-------|
| `coordination-events/` | Claim lifecycle events (acquired, conflict, released) — 90 days |
| `autorun-events/` | Autorun session state changes — 90 days |
| `pathflow-events/` | Phase and stage transitions — 30 days (phase transitions have short operational value) |

## 6. Size-Based Rotation

When a monolithic ledger file exceeds its size limit (see Section 5), the CLI rotates it:

### Rotation procedure

1. Acquire advisory lock on `{filename}.jsonl.lock`.
2. Verify no active writers by checking the lock file (see Section 8).
3. Rename the current file atomically: `{filename}.jsonl` → `{filename}.{YYYY-MM-DD}.jsonl`.
   Use `std::fs::rename()`, which is atomic on POSIX systems when source and destination are on the
   same filesystem.
4. The new empty `{filename}.jsonl` is then created by the next writer through normal append-open.
5. Release the advisory lock.

### Naming convention for rotated files

Rotated (not yet archived) files use the pattern: `{original-stem}.{YYYY-MM-DD}.jsonl`

Example: `sessions.jsonl` (12MB, today is 2026-04-07) → `sessions.2026-04-07.jsonl`

The new empty `sessions.jsonl` immediately receives writes from the next appending process.

### After rotation

Rotated files sit in the ledger root and are subject to the normal archive policy:

- Age >= 90 days: archive to `.state/ledger/archive/{filename}.{date}.jsonl.gz`
- No deletion until archived and archive is verified

Fragment files are not rotated — each fragment is already session-scoped and bounded by the session's
write volume.

## 7. CLI Interface Specification

The `codeflow ledger cleanup` subcommand implements these retention policies. It uses the `clap`
crate for argument parsing.

### Subcommand signature

```text
codeflow ledger cleanup [OPTIONS]
```

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `--dry-run` | flag | off | Show what would be archived or rotated without modifying any files |
| `--type <TYPE>` | enum | all | Only process the specified ledger type |
| `--older-than <DAYS>` | u32 | 90 | Override the default 90-day archive threshold |
| `--compress` | flag | off | Compress eligible files (>= 7 days old) without archiving |
| `--archive` | flag | off | Archive files older than the threshold (implies compression) |
| `--force` | flag | off | Skip interactive confirmation prompt |
| `--verbose` | flag | off | Show detailed progress for each file processed |

### `--type` enum values

| Value | Files processed |
|-------|----------------|
| `sessions` | `sessions.jsonl` and `sessions/` fragments |
| `work-graph` | `work-graph.jsonl` and `work-graph/` fragments |
| `memory-events` | `memory-events.jsonl` and `memory-events/` fragments |
| `config` | `config.jsonl` and `config/` fragments |
| `coordination-events` | `coordination-events/` fragments |
| `autorun-events` | `autorun-events/` fragments |
| `pathflow-events` | `pathflow-events/` fragments |

### Exit codes

| Code | Meaning |
|------|---------|
| 0 | Success — all eligible files processed |
| 1 | Error — one or more files failed; see stderr for `RetentionError` details |
| 2 | Dry-run complete — no files modified (use with `--dry-run`) |

### Clap struct sketch

This is a normative specification for the implementation in INF-TSK-024-018:

```rust
#[derive(clap::Parser)]
pub struct LedgerCleanupArgs {
    /// Show what would be archived/rotated without doing it
    #[arg(long)]
    pub dry_run: bool,

    /// Only process a specific ledger type
    #[arg(long, value_enum)]
    pub r#type: Option<LedgerType>,

    /// Override default 90-day archive threshold
    #[arg(long, default_value_t = 90)]
    pub older_than: u32,

    /// Compress eligible files (>= 7 days old)
    #[arg(long)]
    pub compress: bool,

    /// Archive files older than threshold
    #[arg(long)]
    pub archive: bool,

    /// Skip interactive confirmation
    #[arg(long)]
    pub force: bool,

    /// Show detailed progress
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum LedgerType {
    Sessions,
    WorkGraph,
    MemoryEvents,
    Config,
    CoordinationEvents,
    AutorunEvents,
    PathflowEvents,
}
```

### Example invocations

```bash
# Preview all archivable files without modifying anything
codeflow ledger cleanup --dry-run --verbose

# Archive sessions and work-graph files older than 90 days
codeflow ledger cleanup --archive --type sessions
codeflow ledger cleanup --archive --type work-graph

# Compress warm files (7+ days old) without archiving
codeflow ledger cleanup --compress --force

# Override threshold: archive anything older than 30 days
codeflow ledger cleanup --archive --older-than 30 --force

# Full cleanup: compress + archive all eligible, no confirmation
codeflow ledger cleanup --compress --archive --force
```

## 8. Parallel Session Impact

### Fragment-based architecture eliminates write contention

Since PR #221, each git worktree session writes to its own fragment file:

```text
.state/ledger/work-graph/work-graph-ses-{SID}.jsonl   ← per-session fragment
.state/ledger/work-graph/work-graph.jsonl               ← monolithic (legacy/main worktree)
```

Fragment files are written by exactly one session. There is no write contention between parallel
sessions on fragment files. The cleanup CLI can read, compress, or archive any fragment that is not
currently being written by its owning session.

### Identifying active writers

A fragment is considered active (being written) if:

1. A lock file exists: `{fragment}.lock` (e.g., `work-graph-ses-01knn099.jsonl.lock`), OR
2. The session ID in the fragment filename corresponds to a worktree registered as `active` in
   `.state/worktrees/worktrees.yaml`.

The cleanup CLI checks both signals before touching any fragment file. If either signal indicates
activity, the file is skipped in the current run.

### Advisory locking for monolithic files

Monolithic files (e.g., `sessions.jsonl`, `work-graph.jsonl`) may be written by the main worktree
session. The cleanup CLI uses advisory file locking via the `fs2` Rust crate:

```rust
use fs2::FileExt;

// Acquire exclusive advisory lock before rotation
let file = std::fs::OpenOptions::new()
    .read(true)
    .write(true)
    .open(&path)?;
file.try_lock_exclusive()
    .map_err(|_| RetentionError::LockConflict {
        path: path.clone(),
        holder_pid: resolve_lock_holder(&path),
    })?;
```

Advisory locking means all participants must cooperate. The Rust CLI and any other writer that
follows this protocol will coordinate safely. Writers that do not use `fs2` advisory locking (legacy
shell scripts) may not cooperate — the lock is advisory, not mandatory.

### Atomic rename for rotation

Size-based rotation uses `std::fs::rename()` for the `{filename}.jsonl` → `{filename}.{date}.jsonl`
step. On POSIX systems (macOS, Linux), `rename()` is atomic when source and destination are on the
same filesystem. This guarantees that:

- No partial write state exists during rotation.
- A writer that has the file open before the rename continues writing to the old inode
  (its data is preserved in the rotated file).
- The new `{filename}.jsonl` (created by the next open+append) starts fresh.

### Safety constraint: run only between active sessions

The cleanup CLI should run when no active worktree sessions are writing to monolithic files. The
recommended window is after `codeflow worktree list` shows no active sessions, or during maintenance
windows. The `--dry-run` flag is safe to use at any time.

For automation (e.g., cron), use the `codeflow interactive status` command to confirm no active
sessions before running `codeflow ledger cleanup --archive --force`.

## 9. Configuration

Retention thresholds are currently hardcoded in the CLI binary. A future task (post-INF-EPC-024) may
introduce a configuration section in `.codeflow/config/` to allow per-project overrides. The policy
defaults in this document are the authoritative values until that configuration file exists.

Future configuration path (not yet implemented): `.codeflow/config/retention/retention-config.json`

Expected future schema:

```json
{
  "ledger": {
    "archive_age_days": 90,
    "compress_age_days": 7,
    "monolithic_size_limit_mb": 10,
    "config_archive_age_days": 180
  }
}
```

## 10. Error Contract

The following Rust enum defines the error contract for INF-TSK-024-018 (cleanup CLI
implementation). The policy document specifies this enum to give the implementation an unambiguous
error type surface before coding begins.

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum RetentionError {
    /// Failed to move or copy a file to the archive directory.
    #[error("Failed to archive {path}: {reason}")]
    ArchiveFailed { path: PathBuf, reason: String },

    /// Failed to compress a file using flate2/gzip.
    #[error("Failed to compress {path}: {reason}")]
    CompressFailed { path: PathBuf, reason: String },

    /// Could not acquire advisory lock on a file held by another process.
    #[error("Lock conflict on {path} (held by PID {holder_pid})")]
    LockConflict { path: PathBuf, holder_pid: u32 },

    /// A threshold value is outside the allowed range (e.g., older_than = 0).
    #[error("Invalid threshold: {field} = {value}")]
    InvalidThreshold { field: String, value: String },

    /// The CLI was asked to delete a Tier 0 ledger file. This is unconditionally refused.
    #[error("Refusing to delete Tier 0 ledger file: {0}")]
    NeverDeleteViolation(PathBuf),
}
```

### Variant usage

| Variant | When raised |
|---------|------------|
| `ArchiveFailed` | `fs::rename()` or compressed-copy to archive path fails (I/O error, permissions, cross-device) |
| `CompressFailed` | `flate2::GzEncoder` write fails, or post-compression integrity check fails |
| `LockConflict` | `fs2::FileExt::try_lock_exclusive()` returns `WouldBlock` on a monolithic file |
| `InvalidThreshold` | `--older-than 0` or a value exceeding the maximum allowed (e.g., 3650 days) |
| `NeverDeleteViolation` | Any code path that would call `fs::remove_file()` on a `.jsonl` or `.jsonl.gz` in `.state/ledger/` |

### Error propagation

The cleanup CLI propagates `RetentionError` via `anyhow::Error` at the command level. Individual
file failures are logged to stderr with the file path and error variant. The CLI continues
processing remaining files after a non-fatal error (e.g., a single `LockConflict`) and reports the
total failure count at exit.

## 11. Verification

### Verifying the NEVER-delete rule

Confirm that no code path in `codeflow ledger cleanup` calls `std::fs::remove_file()` on a file
under `.state/ledger/` except after a successful archive operation:

```bash
# After building the CLI, search for remove_file calls in the ledger module
grep -n "remove_file" codeflow-cli/core/src/ledger/
```

Every `remove_file` call must be preceded by a successful archive verification. The
`NeverDeleteViolation` variant must be reachable from any code path that would skip the archive step.

### Verifying archive completeness

After running `codeflow ledger cleanup --archive --verbose`, verify:

1. Source files that were archived no longer exist in `.state/ledger/`.
2. Corresponding `.jsonl.gz` files exist in `.state/ledger/archive/` at the correct path.
3. Archives are readable: `gzip -t .state/ledger/archive/{file}.jsonl.gz` exits 0.

### Verifying lock behavior

Run the cleanup CLI while a session is writing to a monolithic file. With `--verbose`, the CLI must
print a skip message for the locked file and return `LockConflict` in its summary, not fail
silently.

### Dry-run correctness

`--dry-run` must produce identical output to a live run without modifying any file. Verify by:

1. Running `--dry-run --verbose` and capturing output.
2. Running the same command without `--dry-run`.
3. Confirming the listed files match and no extra files appear in the live run.
