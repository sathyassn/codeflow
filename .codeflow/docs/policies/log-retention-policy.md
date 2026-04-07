---
title: "Log Retention Policy"
status: active
created_at: "2026-04-07"
updated_at: "2026-04-07"
author: cf-documentation
scope: ".state/logs/"
---

# Log Retention Policy

This document defines retention, rotation, compression, and cleanup policies for all operational log
files in CodeFlow. It covers files in `.state/logs/` only. Ledger files in `.state/ledger/` are
governed by a separate policy and are NEVER deleted.

## Table of Contents

- [1. Logs vs Ledgers](#1-logs-vs-ledgers)
- [2. Log Categories and Retention Periods](#2-log-categories-and-retention-periods)
- [3. Rotation and Compression Rules](#3-rotation-and-compression-rules)
- [4. Discontinued File Cleanup](#4-discontinued-file-cleanup)
- [5. Additional Cleanup Targets](#5-additional-cleanup-targets)
- [6. Rust CLI Interface Specification](#6-rust-cli-interface-specification)
- [7. Error Type Specification](#7-error-type-specification)
- [8. Implementation Notes](#8-implementation-notes)

---

## 1. Logs vs Ledgers

CodeFlow maintains two distinct categories of persistent data files that require different handling:

| Property | Logs (`.state/logs/`) | Ledgers (`.state/ledger/`) |
|----------|----------------------|---------------------------|
| Purpose | Operational observability — debugging, incident investigation, compliance | Rebuild authority — Tier 0 source of truth for WorkGraph and session state |
| Mutability | Rotatable and deletable after retention period | Immutable, append-only. NEVER deleted. |
| Governed by | This document | Separate ledger retention policy |
| Retention | 30 days (most categories), 90 days (git logs) | Archive after 90 days, never delete |

**Critical distinction:** Losing a log file is inconvenient but recoverable. Losing a ledger file is data loss — the WorkGraph, task history, and session records cannot be reconstructed. This policy applies ONLY to files under `.state/logs/`. If a path resolves to `.state/ledger/`, the cleanup operation MUST refuse with an error.

---

## 2. Log Categories and Retention Periods

### 2.1 Security Logs

**Location:** `.state/logs/security/`

**Subdirectories:**

| Subdirectory | File Pattern | Retention | Compression |
|-------------|-------------|-----------|-------------|
| `audit/` | `audit-{YYYY-MM-DD}.jsonl` | 30 days | Compress (gzip) after 7 days |
| `blocked/` | `blocked-{YYYY-MM-DD}.jsonl` | 30 days | Compress (gzip) after 7 days |
| `network/` | `network-{YYYY-MM-DD}.jsonl` | 30 days | Compress (gzip) after 7 days |
| `protection/` | `protection-{YYYY-MM-DD}.jsonl` | 30 days | Compress (gzip) after 7 days |

**Rotation:** Daily (date-based filenames already enforce this). Each calendar day produces one file per subdirectory.

**Rationale:** 30 days covers most incident investigation windows. Compliance and debugging queries rarely span beyond two weeks for active incidents. Compression after 7 days reduces storage while preserving accessibility.

**protection-audit.log (special case):** This plain-text file at `.state/logs/security/protection-audit.log`
uses a `[timestamp] USER=X ACTION=Y PATH=Z` format and MUST be converted to JSONL before standard retention
applies. Until conversion is complete, `codeflow log cleanup` treats this file as out-of-scope and skips it,
printing a warning:

```text
WARNING: .state/logs/security/protection-audit.log is in plain-text format.
Convert to JSONL before cleanup applies. See INF-TSK-024-018.
```

### 2.2 Activity Logs (Session, Prompts, Tool-Use)

**Location:** `.state/logs/sessions/`

**File patterns and retention:**

| File Pattern | Retention |
|-------------|-----------|
| `session-{YYYY-MM-DD}.jsonl` | 30 days |
| `prompts-{YYYY-MM-DD}.jsonl` | 30 days |
| `tool-use-{YYYY-MM-DD}.jsonl` | 30 days |

**Rotation:** Daily (date-based filenames).

**Compression:** Not applied (session logs are smaller than security logs and frequently queried for debugging
recent sessions).

**Rationale:** Operational debugging. Sessions older than 30 days are rarely investigated. Date-based filenames
make age-based cleanup deterministic without parsing file contents.

### 2.3 Git Commit Logs

**Location:** `.state/logs/git/`

**File patterns and retention:**

| File Pattern | Retention |
|-------------|-----------|
| `commits-{YYYY-MM-DD}.jsonl` | 90 days |
| `pr-events-{YYYY-MM-DD}.jsonl` | 90 days |

**Rotation:** Daily (date-based filenames).

**Compression:** Not applied.

**Rationale:** Git itself retains the full commit history permanently in the repository. The log files
provide structured, queryable commit metadata (session attribution, worktree context). 90 days covers
typical post-merge investigation windows and audit requirements while git history serves as the permanent
record.

### 2.4 PathFlow Events

**Location:** `.state/logs/pathflow-events.jsonl`

**Retention:** 30 days of entries (measured by `timestamp` field within each event).

**Archival:** Entries older than 30 days are moved to per-month archive files at:

```text
.state/logs/archive/pathflow-events-{YYYY-MM}.jsonl
```

**Rotation strategy:** Unlike date-based log files, `pathflow-events.jsonl` is a single append-only file.
Cleanup reads each entry's `timestamp` field and partitions events:

- Entries within the 30-day window remain in `pathflow-events.jsonl`.
- Entries older than 30 days are appended to the corresponding monthly archive file and removed from the active file.

**Compression:** Archive files are gzip-compressed once the month is complete (i.e., the current month's
archive is not compressed until the following month begins).

**Rationale:** Phase transitions are useful for debugging recent sessions. Historical pathflow data has
diminishing value. Monthly archives preserve the data at reduced storage cost.

### 2.5 Database Logs

**Location:** `.state/logs/db/`

**File patterns and retention:**

| File Pattern | Retention |
|-------------|-----------|
| `db-{YYYY-MM-DD}.jsonl` | 30 days |

**Rotation:** Daily (date-based filenames).

**Rationale:** Database operation logs are operational diagnostics. 30 days aligns with the general
operational window for post-incident investigation.

---

## 3. Rotation and Compression Rules

### Rotation

All log categories except PathFlow events use **date-based file rotation** — each calendar day
produces a new file. The naming convention `{prefix}-{YYYY-MM-DD}.jsonl` is enforced by the writers.
`codeflow log cleanup` does not perform active rotation; it deletes files outside the retention window.

The PathFlow events file (`pathflow-events.jsonl`) uses **content-based partitioning** by timestamp
rather than file rotation.

### Compression

Security logs (audit, blocked, network, protection) are compressed with **gzip** after 7 days. The
compressed filename appends `.gz`:

```text
.state/logs/security/audit/audit-2026-03-25.jsonl     → active (≤7 days)
.state/logs/security/audit/audit-2026-03-18.jsonl.gz  → compressed (>7 days, ≤30 days)
```

**Compression implementation:** Uses the `flate2` crate (`GzEncoder` with `Compression::default()`).
The original file is deleted after successful compression. If compression fails, the original file is
preserved and `LogCleanupError::CompressFailed` is returned.

**Reading compressed files:** Consumers that read historical security logs must handle `.gz` files via
`GzDecoder`. The cleanup command only compresses; it does not decompress.

---

## 4. Discontinued File Cleanup

The following files and directories are **discontinued** — they are no longer written by any active
component and serve no operational purpose. `codeflow log cleanup` deletes all files within these
paths on first run. No retention period applies; deletion is immediate.

| Path | Reason Discontinued |
|------|---------------------|
| `.state/logs/security/sentinel/` | Superseded by Rust CLI sentinel system after 2026-02-14. Contains 10 `sentinel-{date}.jsonl` files, last entry 2026-02-14. |
| `.state/logs/sessions/stop-events-*.jsonl` | Merged into `session-{date}.jsonl`. Separate file is redundant. |
| `.state/logs/sessions/git-operations-*.jsonl` | Superseded by `commits-{date}.jsonl` in `.state/logs/git/`. |
| `.state/logs/test-category/` | Never populated. Empty directory. |

**Cleanup behavior:** `codeflow log cleanup` removes all files within discontinued paths. If a discontinued
directory does not exist, no error is raised — the operation is idempotent. If deletion fails for any
file, `LogCleanupError::DeletionFailed` is returned with the specific path.

**Note on revived log types:** `file-changes-*.jsonl`, `tasks-*.jsonl`, and `skills-*.jsonl` are
NOT in the discontinued list. These are candidates for revival (see schema-standardization.md Section 3)
and should be treated as unknown files — `codeflow log cleanup` does not delete them.

---

## 5. Additional Cleanup Targets

### 5.1 Session `.meta` Files

**Location:** `.state/logs/sessions/` (alongside activity logs)

**File pattern:** `.meta` files (one per session, e.g., `ses-01knn099v0aamdxrg2avb92hja.meta`)

**Retention:** 30 days from the session's `started_epoch` timestamp stored in the `.meta` file.

**Cleanup:** Included in `codeflow log cleanup`. The command reads `started_epoch` from each `.meta`
file to compute age. `.meta` files whose corresponding session is older than 30 days are deleted.

**Rationale:** `.meta` files are used to compute session duration. Once the corresponding session logs
are past retention, the `.meta` files have no remaining purpose.

### 5.2 Stale `.jsonl.lock` Files

**Location:** Any `.state/logs/` subdirectory

**File pattern:** `*.jsonl.lock`

**Cleanup trigger:** Two conditions, either of which triggers cleanup:

1. **Session end:** `codeflow log cleanup` is called from the `SessionEnd` hook. Lock files whose
   associated session is no longer active are removed.
2. **Age threshold:** Any `.jsonl.lock` file older than **24 hours** is treated as stale and removed.

**Rationale:** Lock files are created during active writes to prevent concurrent modification. After a
session ends or a process crashes, lock files may be orphaned. A 24-hour TTL ensures orphaned locks
are cleaned without risk of removing in-use locks (no write operation spans more than a few seconds).

**Implementation:** Stale lock detection uses file mtime (modification time). Lock files with
`mtime < now - 24h` are deleted unconditionally.

### 5.3 `.prompt-counter-*` Files

**Location:** `.state/logs/sessions/` (alongside activity logs)

**File pattern:** `.prompt-counter-{session-id}`

**Cleanup:** `.prompt-counter-*` files are deleted alongside their parent session's activity logs.
When a session's `prompts-{date}.jsonl` entries age out of the 30-day retention window, the
corresponding `.prompt-counter-{session-id}` file is also deleted.

**Rationale:** Prompt counter files track per-session prompt counts. Once session logs are past
retention, the counter file has no remaining operational value.

---

## 6. Rust CLI Interface Specification

The cleanup subcommand is implemented as `codeflow log cleanup` using the `clap` crate for argument
parsing. It is distinct from the ledger cleanup command.

### Subcommand Definition

```rust
use clap::{Parser, Subcommand};

#[derive(Subcommand)]
pub enum LogCommands {
    /// Apply log retention policies, compress, and remove discontinued files
    Cleanup(LogCleanupArgs),
}

#[derive(Parser)]
pub struct LogCleanupArgs {
    /// Show what would be deleted or archived without making changes
    #[arg(long)]
    pub dry_run: bool,

    /// Only process the specified log category
    /// Valid values: security, session, git, pathflow, db, discontinued, meta, locks
    #[arg(long, value_name = "TYPE")]
    pub r#type: Option<String>,

    /// Override the default retention period
    /// Format: {N}d (days), e.g. "90d"
    #[arg(long, value_name = "DURATION")]
    pub older_than: Option<String>,

    /// Compress security log files older than 7 days
    #[arg(long)]
    pub compress: bool,

    /// Skip interactive confirmation and apply all policies immediately
    #[arg(long)]
    pub yes: bool,
}
```

### Command Interface

```text
codeflow log cleanup                              # Apply all retention policies (interactive confirmation)
codeflow log cleanup --dry-run                    # Show what would be deleted/archived
codeflow log cleanup --type security              # Only process security logs
codeflow log cleanup --older-than 90d             # Override retention period for this run
codeflow log cleanup --compress                   # Compress files older than 7 days
codeflow log cleanup --yes                        # Skip confirmation prompt
```

### Behavior Contract

| Flag | Behavior |
|------|----------|
| (none) | Interactive: show summary of what will change, prompt for confirmation before applying |
| `--dry-run` | Print files that would be deleted/compressed/archived. No changes made. Exit 0. |
| `--type security` | Limit scope to `.state/logs/security/` (audit, blocked, network, protection). |
| `--type session` | Limit scope to `.state/logs/sessions/` (session, prompts, tool-use, `.meta`, `.prompt-counter-*`). |
| `--type git` | Limit scope to `.state/logs/git/` (commits, pr-events). |
| `--type pathflow` | Process `pathflow-events.jsonl` only (archive old entries). |
| `--type db` | Limit scope to `.state/logs/db/`. |
| `--type discontinued` | Delete only discontinued files and directories. |
| `--type meta` | Delete only stale `.meta` files. |
| `--type locks` | Delete only stale `.jsonl.lock` files. |
| `--older-than {N}d` | Override the category's default retention period for this run. |
| `--compress` | Trigger gzip compression of security logs older than 7 days (also applies during normal cleanup). |
| `--yes` | Skip the interactive confirmation prompt. Suitable for automated/CI use. |

### Output Format

The command emits structured status to stdout:

```text
Log cleanup summary (dry-run):
  security/audit:      12 files to delete (>30d), 8 files to compress (>7d)
  security/blocked:    11 files to delete (>30d)
  security/network:    5 files to delete (>30d)
  security/protection: 11 files to delete (>30d)
  sessions:            28 files to delete (>30d), 28 .meta files to delete
  git:                 4 files to delete (>90d)
  pathflow-events:     142 entries to archive to pathflow-events-2026-01.jsonl
  discontinued:        .state/logs/security/sentinel/ (10 files)
                       .state/logs/sessions/stop-events-*.jsonl (6 files)
                       .state/logs/sessions/git-operations-*.jsonl (1 file)
                       .state/logs/test-category/ (empty directory)
  stale locks:         0 .jsonl.lock files found

Run without --dry-run to apply.
```

---

## 7. Error Type Specification

The following error enum is specified for implementation in INF-TSK-024-018 (cleanup CLI). Use
the `thiserror` crate for derivation:

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LogCleanupError {
    #[error("rotation failed for {path}: {reason}")]
    RotationFailed { path: PathBuf, reason: String },

    #[error("deletion failed for {path}: {reason}")]
    DeletionFailed { path: PathBuf, reason: String },

    #[error("compression failed for {path}: {reason}")]
    CompressFailed { path: PathBuf, reason: String },

    #[error("discontinued directory not found: {0}")]
    DiscontinuedDirNotFound(PathBuf),
}
```

**Variant usage:**

| Variant | When raised |
|---------|------------|
| `RotationFailed` | PathFlow event partitioning fails (e.g., cannot write monthly archive file) |
| `DeletionFailed` | File deletion fails (permission error, I/O error) |
| `CompressFailed` | gzip compression fails for a security log file |
| `DiscontinuedDirNotFound` | A discontinued path is missing — logged as info, not a fatal error |

**Error handling strategy:** `codeflow log cleanup` processes all categories and collects errors. It
does not stop on the first error. After processing all categories, if any errors occurred, they are
printed to stderr and the command exits with code 1. Successfully processed categories are not rolled back.

**Recommended crates:**

| Crate | Purpose |
|-------|---------|
| `flate2` | gzip compression (`GzEncoder`, `GzDecoder`) |
| `chrono` | Date parsing and age calculation (`NaiveDate`, `Duration`) |
| `clap` | CLI argument parsing (`Parser`, `Subcommand` derives) |
| `thiserror` | Error type derivation |

---

## 8. Implementation Notes

### Ledger Safety Guard

`codeflow log cleanup` MUST verify that no target path resolves to `.state/ledger/` before
performing any deletion or modification. If a resolved path begins with `.state/ledger/`, the
operation is aborted with a descriptive error:

```text
ERROR: Refusing to delete .state/ledger/sessions.jsonl — ledger files are never deleted.
This is a bug. Please report at https://github.com/sathyassn/codeflow/issues.
```

### Idempotency

All cleanup operations are idempotent. Running `codeflow log cleanup` multiple times on the same
state produces the same result. Files already deleted, already compressed, or already archived do not
cause errors.

### Atomic Writes for PathFlow Archival

When partitioning `pathflow-events.jsonl` for archival, the implementation writes to a temporary file
first, then renames atomically:

1. Write filtered entries (within retention window) to `pathflow-events.jsonl.tmp`
2. Append out-of-window entries to the monthly archive file
3. Rename `pathflow-events.jsonl.tmp` to `pathflow-events.jsonl`

If step 3 fails, the original `pathflow-events.jsonl` is preserved. If step 2 fails,
`RotationFailed` is returned and `pathflow-events.jsonl` is not modified.

### SessionEnd Hook Integration

`codeflow log cleanup --type locks --yes` is suitable for invocation from the `SessionEnd` hook to
clean up stale lock files created by the ending session. This invocation is scoped to lock files only
and uses `--yes` to skip confirmation in the automated context.
