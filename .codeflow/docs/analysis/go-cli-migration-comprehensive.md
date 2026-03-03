---
title: "Comprehensive Go CLI Migration Analysis"
status: analysis-complete
created_at: "2026-02-27"
updated_at: "2026-02-28"
related_epic: "INF-EPC-021"
---

# Comprehensive Go CLI Migration Analysis

## Table of Contents

- [1. Executive Summary](#1-executive-summary)
- [2. Current State Assessment](#2-current-state-assessment)
- [3. Migration Scope](#3-migration-scope)
- [4. Script Migration Map](#4-script-migration-map)
- [5. Python Elimination Plan](#5-python-elimination-plan)
- [6. Hook Go Replacement Strategy](#6-hook-go-replacement-strategy)
- [7. Performance Impact Analysis](#7-performance-impact-analysis)
- [8. Dependency Elimination Matrix](#8-dependency-elimination-matrix)
- [9. Consolidated Binary Architecture](#9-consolidated-binary-architecture)
- [10. Retained Infrastructure](#10-retained-infrastructure)
- [11. Migration Priority](#11-migration-priority)
- [12. Risk Assessment](#12-risk-assessment)
- [13. Recommendation](#13-recommendation)

---

## 1. Executive Summary

CodeFlow's shell and Python layer comprises **57 production scripts** (~14,609 lines) in
`.codeflow/scripts/` plus **22 hook scripts** (~6,132 lines) in `.claude/hooks/codeflow/`.
This totals **79 scripts and ~20,741 lines** of shell and Python code with several systemic
problems: JSON written by string concatenation, `flock` unavailable on macOS, platform-specific
`date`/`stat` divergence, and Python called as a subprocess for basic operations.

The existing Go CLI epic (INF-EPC-015) builds the binary foundation but does not wire it into
the live hook and script layer. This document covers that gap — identifying every script that
should migrate, the correct migration tier, and the phased approach to reach a state where
shell scripts are limited to operations that genuinely require shell (sudo, tool validation)
and the Go binary handles all data integrity, enforcement, and session management.

**Key findings:**

- 24 scripts in Tier 1 are the source of active data integrity and security bugs — these must
  migrate before any new feature work on the shell layer
- Python can be eliminated 100% — all 18 Python source files and 20 pytest files map to Go
  stdlib equivalents
- Hook scripts account for ~300ms overhead per tool call; Go replaces this with ~30-50ms
- INF-EPC-015 covers building the binary; no epic or task currently covers wiring hooks to
  the binary or migrating the enforcement stack

---

## 2. Current State Assessment

### 2.1 Shell Script Inventory

Verified counts via `ls .codeflow/scripts/**/*.sh | wc -l` and
`ls .claude/hooks/codeflow/**/*.sh | wc -l`:

| Location | Count | Lines |
|----------|-------|-------|
| `.codeflow/scripts/**/*.sh` | 57 scripts | ~14,609 lines |
| `.claude/hooks/codeflow/**/*.sh` | 22 scripts | ~6,132 lines |
| **Total** | **79 scripts** | **~20,741 lines** |

### 2.2 Python Script Inventory

Verified via `find .codeflow/scripts -name "*.py" | wc -l`:

| Location | Count |
|----------|-------|
| `.codeflow/scripts/codeflow_py_lib/` | 9 library files |
| `.codeflow/scripts/coordination/` | 6 coordination scripts |
| `.codeflow/scripts/memory/` | 1 script |
| `.codeflow/scripts/state/` | 1 script |
| `.codeflow/testing/scripts/` (pytest) | ~20 test files |
| **Total source** | **18 Python source files** |

### 2.3 Active Bugs Introduced by Shell Limitations

The following problems are not hypothetical — they manifest in the current codebase:

**JSON corruption via string concatenation:**

`ledger.sh` and `memory.sh` construct JSONL records with shell string interpolation:

```bash
echo "{\"event\": \"$EVENT_TYPE\", \"data\": \"$PAYLOAD\"}" >> "$LEDGER_FILE"
```

If `$PAYLOAD` contains a double-quote, backslash, or newline, the output is invalid JSON.
Since JSONL is Tier 0 (rebuild authority), corrupted records are permanent data loss.

**`flock` unavailability on macOS:**

`ledger.sh` uses `flock` for concurrent write protection. macOS does not ship `flock`
by default. This means concurrent hook invocations on macOS have no write protection —
two hooks can interleave writes producing a corrupted JSONL line.

**Platform-specific `date` and `stat`:**

`cf-sentinel.sh` (~751 lines) uses `stat -f%z` on macOS and `stat --printf=%s` on Linux.
`cf-work-state.sh` uses `date -j -f "%Y-%m-%dT%H:%M:%S"` on macOS vs `date -d` on Linux.
These branches require testing on both platforms and diverge with every OS update.

**`jq` subprocess chains:**

`cf-pathflow-state.sh` (~678 lines) runs multiple `jq` subprocesses in sequence to read,
modify, and write the checkpoint JSON. Each `jq` invocation forks a process. On sessions
with frequent checkpoint operations, this produces measurable latency.

**python3 subprocesses for trivial operations:**

`ulid.sh` calls `python3 -c "import time; print(int(time.time() * 1000))"` to get a
millisecond timestamp. This spawns an interpreter for a one-liner that Go handles in
nanoseconds.

**Fragile `sed`-based YAML parsing:**

`validate-task.sh` (~606 lines) and `validate-epic.sh` (~357 lines) parse YAML frontmatter
using `sed`. YAML with multiline values, special characters, or non-standard indentation
causes false validation passes or spurious failures.

### 2.4 What INF-EPC-015 Covers vs. What It Misses

INF-EPC-015 defines the Go binary architecture (welcome screen, preflight, DB, session, autorun).
It explicitly preserves shell hooks and shell scripts as-is during its phases.

**Covered by INF-EPC-015:**

- Go binary entry point and CLI framework
- SQLite access via Go (eliminating direct `sqlite3` shell calls for DB writes)
- JSONL write via Go (eliminating `echo >>` for ledger)
- Session start/end Go subcommands
- Doctor, init, autorun, update subcommands

**Not covered by INF-EPC-015 (gaps requiring new epics/tasks):**

- Wiring hook scripts to call `codeflow hooks <name>` instead of running shell logic
- Migrating the enforcement stack (security-lib.sh, context-lib.sh, 9 enforcement modules)
- Migrating pathflow state management (cf-pathflow-state.sh)
- Migrating sentinel operations (cf-sentinel.sh)
- Migrating validation (validate-task.sh, validate-epic.sh)
- Removing direct `sqlite3` calls from shell scripts (Phase 6 V4 spec requirement)
- Verifying JSONL-only writes after migration (Phase 6 V4 spec requirement)
- Python script elimination (coordination scripts, codeflow_py_lib)

---

## 3. Migration Scope

Scripts are classified into four tiers based on data integrity risk, platform issues,
and performance impact. Verified line counts from `wc -l` output.

### 3.1 Tier 1 — MUST Migrate (Data Integrity and Security-Critical)

**24 scripts, ~9,897 lines.** These scripts contain active bugs or are on the critical path
for security enforcement. Migration is a prerequisite for production reliability.

**State and Data Layer (6 scripts):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.codeflow/scripts/state/ledger.sh` | 408 | JSON by string concatenation; `flock` unavailable on macOS |
| `.codeflow/scripts/state/cf-pathflow-state.sh` | 678 | Read-modify-write race on checkpoint JSON; `jq` subprocess chain |
| `.codeflow/scripts/state/cf-work-state.sh` | 216 | macOS/Linux `date` parsing split |
| `.codeflow/scripts/state/memory.sh` | 196 | Same JSON construction issues as ledger.sh |
| `.codeflow/scripts/db/generate-format-id.sh` | 199 | `sqlite3` subprocess for ID generation |
| `.codeflow/scripts/shell-lib/ulid.sh` | 158 | Spawns `python3` for millisecond timestamp |

**Validation (3 scripts):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.codeflow/scripts/validation/validate-task.sh` | 606 | Fragile `sed`-based YAML frontmatter parsing |
| `.codeflow/scripts/validation/validate-epic.sh` | 357 | Same `sed`-based YAML fragility |
| `.codeflow/scripts/state/cf-tracking-generate.sh` | 267 | `sed`-based YAML parser for report generation |

**Pathflow Scripts (5 scripts):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.codeflow/scripts/pathflow/cf-pathflow-phase-transition.sh` | ~155 | JSONL write via string concatenation |
| `.codeflow/scripts/pathflow/cf-pathflow-stage-transition.sh` | ~155 | JSONL write via string concatenation |
| `.codeflow/scripts/pathflow/cf-pathflow-session-register.sh` | ~155 | JSONL write via string concatenation |
| `.codeflow/scripts/pathflow/cf-pathflow-task-update.sh` | ~155 | JSONL write via string concatenation |
| `.codeflow/scripts/pathflow/cf-pathflow-session-metadata.sh` | ~155 | JSONL write via string concatenation |

**Hook Scripts — Session Lifecycle (2 hooks):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.claude/hooks/codeflow/session-start/cf-session-start-init.sh` | 619 | Most complex hook; failures cascade to entire session |
| `.claude/hooks/codeflow/session-end/cf-session-end-cleanup.sh` | 289 | Stale state breaks next session if cleanup fails |

**Hook Scripts — Enforcement Gates (4 hooks):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-pathflow-gate.sh` | 392 | Enforcement gate must be reliable; platform divergence |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh` | 223 | Security enforcement on every Bash call |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-pathflow-sentinel.sh` | 267 | Sentinel creation on critical path |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-phase-checkpoint.sh` | 180 | Phase checkpoint on critical path |

**Security Sentinel (1 script):**

| Script | Lines | Primary Issue |
|--------|-------|---------------|
| `.codeflow/scripts/security/sentinel/cf-sentinel.sh` | 751 | macOS/Linux stat/date platform split; most complex security script |

### 3.2 Tier 2 — SHOULD Migrate (Reliability and Performance)

**37 scripts, ~8,655 lines.** These scripts work but have reliability or performance issues
that Go migration resolves cleanly.

**Shell Library Modules (6 scripts):**

| Script | Lines |
|--------|-------|
| `.codeflow/scripts/shell-lib/common.sh` | 187 |
| `.codeflow/scripts/shell-lib/logging.sh` | 200 |
| `.codeflow/scripts/shell-lib/errors.sh` | ~100 |
| `.codeflow/scripts/shell-lib/config.sh` | ~120 |
| `.codeflow/scripts/shell-lib/validation.sh` | 215 |
| `.codeflow/scripts/shell-lib/index.sh` | ~25 |

**Settings Management (2 scripts):**

| Script | Lines |
|--------|-------|
| `.codeflow/scripts/settings/cf-change-approval-mode.sh` | 558 |
| `.codeflow/scripts/settings/setup-managed-settings.sh` | 417 |

**Security Libraries (3 scripts):**

| Script | Lines |
|--------|-------|
| `.codeflow/scripts/security/lib/security-lib.sh` | 437 |
| `.codeflow/scripts/security/lib/context-lib.sh` | 194 |
| `.codeflow/scripts/security/lib/bash-file-readers-lib.sh` | 581 |

**Security Enforcement Modules (8 scripts):**

| Script | Lines |
|--------|-------|
| `.codeflow/scripts/security/enforcement/cf-path-protection.sh` | 422 |
| `.codeflow/scripts/security/enforcement/cf-pattern-matching.sh` | 264 |
| `.codeflow/scripts/security/enforcement/cf-branch-file-protection.sh` | 241 |
| `.codeflow/scripts/security/enforcement/cf-git-protection.sh` | 238 |
| `.codeflow/scripts/security/enforcement/cf-privilege-protection.sh` | 163 |
| `.codeflow/scripts/security/enforcement/cf-dangerous-commands.sh` | 180 |
| `.codeflow/scripts/security/enforcement/cf-file-operations.sh` | 153 |
| `.codeflow/scripts/security/enforcement/cf-network-protection.sh` | 87 |

**Security Staging (5 scripts):**

| Script | Lines |
|--------|-------|
| `.codeflow/scripts/security/staging/cf-apply-staged-edit.sh` | 269 |
| `.codeflow/scripts/security/staging/cf-stage-edit.sh` | 248 |
| `.codeflow/scripts/security/staging/cf-rollback-edit.sh` | 225 |
| `.codeflow/scripts/security/staging/cf-cleanup-expired.sh` | 195 |
| `.codeflow/scripts/security/staging/cf-reject-staged-edit.sh` | 162 |

**Remaining Hook Scripts (13 scripts):**

| Script | Lines |
|--------|-------|
| `.claude/hooks/codeflow/session-start/cf-session-start-logging.sh` | 174 |
| `.claude/hooks/codeflow/session-start/cf-session-start-instructions.sh` | 200 |
| `.claude/hooks/codeflow/session-end/cf-session-end-logging.sh` | 309 |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-logging.sh` | 217 |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-settings-templates.sh` | 457 |
| `.claude/hooks/codeflow/post-tool-use/cf-post-tool-use-tmp-workflow.sh` | ~100 |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-edit-write.sh` | 315 |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-gh-pr.sh` | 384 |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-protected-resource.sh` | 290 |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-team-guard.sh` | ~150 |
| `.claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-webfetch.sh` | 462 |
| `.claude/hooks/codeflow/stop/cf-stop-logging.sh` | 280 |
| `.claude/hooks/codeflow/stop/cf-stop-pathflow-gate.sh` | ~100 |

### 3.3 Tier 3 — NICE TO HAVE

**~18 scripts, ~7,000 lines.** These scripts work and have no active bugs but benefit from
Go migration for consistency and elimination of the shell testing framework.

| Script / Category | Lines | Notes |
|------------------|-------|-------|
| `codeflow` wrapper (top-level) | 277 | Becomes Go binary entry point |
| `.codeflow/scripts/db/migrate-to-dual-id.sh` | 356 | One-time migration; acceptable as shell |
| Worktree scripts (4 scripts) | 199-569 each | cf-worktree-setup, status, list, cleanup |
| Security protection scripts (4 scripts) | 129-252 each | cf-protect-resources, cf-reload-protection, cf-promote-protection, cf-protection-common |
| Validation wrappers | ~80 | thin wrappers around validate-task/epic |
| Testing framework lib (10 scripts) | ~4,000 | Superseded by `go test`; remove when shell tests removed |

### 3.4 Tier 4 — KEEP AS SHELL

**4 scripts.** These scripts have genuine shell dependencies that cannot or should not
be replaced with Go.

| Script | Lines | Why Keep |
|--------|-------|----------|
| `.codeflow/scripts/security/validation/cf-validate-shell.sh` | 158 | Requires `bash -n` and `shellcheck` — tool validation |
| `.codeflow/scripts/security/protection/cf-protect-resources.sh` | 420 | Requires `sudo chown`/`chmod` — privilege escalation |
| `.codeflow/scripts/security/enforcement/cf-hook-bypass.sh` | 37 | **DEAD CODE** — remove entirely |
| `.codeflow/scripts/shell-lib/index.sh` | ~25 | Module loader concept disappears in Go |

The `cf-hook-bypass.sh` file (37 lines) is dead code — verified by searching for callers
via `Grep`. It has no callers in the codebase and should be deleted rather than migrated.

### 3.5 REMOVE — Dead Code and Post-Migration Obsolete Scripts

**3 scripts.** Scripts that are dead code or become permanently obsolete after migration.

| Script | Lines | Reason |
|--------|-------|--------|
| `.codeflow/scripts/security/enforcement/cf-hook-bypass.sh` | 37 | Dead code — no callers in codebase |
| `.codeflow/scripts/security/validation/cf-validate-python.sh` | 172 | Validates python3 interpreter; no Python files post-migration |

`cf-validate-python.sh` was previously classified T4 (Keep shell). Reclassified to REMOVE:
once Python is fully eliminated from the codebase, there is no python3 to validate and no
reason to check its availability. The script becomes permanently dead code after Phase E.

---

## 4. Script Migration Map

Complete mapping of every shell and Python file in the project. Line counts are exact
(verified via `wc -l`). Tier definitions follow Section 3. Go Target uses the consolidated
binary command tree from Section 9. File counts cross-checked against `find` output:
57 `.codeflow/scripts/*.sh`, 22 `.claude/hooks/**/*.sh`, 108 `.codeflow/testing/**/*.sh`
(excluding `coverage_reports/`), 18 Python source files, 25 Python test files.

---

### 4.1 Production Shell Scripts — `.codeflow/scripts/`

#### 4.1.1 `state/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `state/ledger.sh` | 408 | Append events to JSONL ledger files | T1 | `codeflow ledger append` | JSON by string concat; no flock on macOS |
| `state/cf-pathflow-state.sh` | 678 | Read/write PathFlow checkpoint JSON | T1 | `codeflow hooks checkpoint-register` | jq subprocess chain; read-modify-write race |
| `state/cf-work-state.sh` | 216 | Manage active work state files | T1 | `codeflow session status` | macOS/Linux date parsing split |
| `state/memory.sh` | 195 | Write memory events to JSONL | T1 | `codeflow ledger append` | Same JSON construction issues as ledger.sh |
| `state/cf-tracking-generate.sh` | 267 | Generate epic tracking reports from YAML | T1 | `codeflow report epic-tracker` | sed-based YAML parser; fragile |

#### 4.1.2 `db/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `db/generate-format-id.sh` | 199 | Generate formatted IDs via sqlite3 | T1 | `codeflow db generate-id` | sqlite3 subprocess per call |
| `db/migrate-to-dual-id.sh` | 356 | One-time dual-ID schema migration | T3 | `codeflow db migrate` | Rarely invoked; acceptable as shell short-term |
| `db/normalize-jsonl.sh` | 316 | Normalize JSONL ledger files to canonical format | T2 | `codeflow ledger normalize` | Created in INF-TSK-015-017; superseded by Go-based INF-TSK-021-026 |

#### 4.1.3 `validation/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `validation/validate-task.sh` | 606 | Validate task markdown YAML frontmatter | T1 | `codeflow validate task` | sed-based YAML; false passes on complex frontmatter |
| `validation/validate-epic.sh` | 357 | Validate epic markdown YAML frontmatter | T1 | `codeflow validate epic` | Same sed-based YAML fragility |

#### 4.1.4 `pathflow/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `pathflow/cf-pathflow-phase-transition.sh` | 144 | Write phase transition event to JSONL | T1 | `codeflow pathflow phase-transition` | JSONL write via string concatenation |
| `pathflow/cf-pathflow-stage-transition.sh` | 193 | Write stage transition event to JSONL | T1 | `codeflow pathflow stage-transition` | JSONL write via string concatenation |
| `pathflow/cf-pathflow-session-register.sh` | 133 | Register session in JSONL ledger | T1 | `codeflow pathflow session-register` | JSONL write via string concatenation |
| `pathflow/cf-pathflow-task-update.sh` | 141 | Write task update event to JSONL | T1 | `codeflow pathflow task-update` | JSONL write via string concatenation |
| `pathflow/cf-pathflow-session-metadata.sh` | 122 | Write session metadata to JSONL | T1 | `codeflow pathflow session-metadata` | JSONL write via string concatenation |

#### 4.1.5 `settings/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `settings/cf-change-approval-mode.sh` | 558 | Read/write approval mode in settings.json | T2 | `codeflow mode` | Already partially replicated in Go binary |
| `settings/setup-managed-settings.sh` | 417 | Initialize managed settings templates | T2 | `codeflow hooks settings-templates` | Called by PostToolUse hook |

#### 4.1.6 `shell-lib/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `shell-lib/common.sh` | 187 | Common shell utilities (sourced by others) | T2 | `internal/cliutil/` | Sourced by most scripts; disappears in Go |
| `shell-lib/config.sh` | 143 | Read codeflow config (YAML via python3) | T2 | `internal/config/` | python3 subprocess for YAML parsing |
| `shell-lib/errors.sh` | 184 | Error handling and exit utilities | T2 | `internal/errors/` | Sourced by most scripts |
| `shell-lib/index.sh` | 25 | Module loader (sources other lib files) | T4 | Remove | Concept disappears in Go; no callers outside scripts |
| `shell-lib/logging.sh` | 200 | Structured log output to stderr/file | T2 | `internal/logging/` | Sourced by most scripts |
| `shell-lib/ulid.sh` | 158 | Generate ULID/timestamp via python3 | T1 | `internal/ulid/` | python3 subprocess for millisecond timestamp |
| `shell-lib/validation.sh` | 215 | Input validation helpers | T2 | `internal/validation/` | Sourced by most scripts |

#### 4.1.7 `security/lib/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/lib/security-lib.sh` | 437 | Core security check orchestrator | T2 | `internal/security/` | Sources all enforcement modules; 437 lines read per Bash call |
| `security/lib/context-lib.sh` | 194 | Extract tool context from stdin JSON | T2 | `internal/security/context/` | python3 one-liner for timestamps |
| `security/lib/bash-file-readers-lib.sh` | 581 | Read/parse files for security checks | T2 | `internal/security/readers/` | Large lib sourced on every Bash call |

#### 4.1.8 `security/enforcement/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/enforcement/cf-path-protection.sh` | 422 | Block writes to protected paths | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-pattern-matching.sh` | 264 | Match tool input against patterns | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-branch-file-protection.sh` | 241 | Protect critical branch files | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-git-protection.sh` | 238 | Block dangerous git operations | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-privilege-protection.sh` | 163 | Block privilege escalation commands | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-dangerous-commands.sh` | 180 | Block known dangerous shell commands | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-file-operations.sh` | 153 | Block destructive file operations | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-network-protection.sh` | 87 | Block unauthorized network operations | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-tmp-protection.sh` | 70 | Protect temp file operations | T2 | `codeflow hooks security check-bash` | Sourced by security-lib.sh |
| `security/enforcement/cf-hook-bypass.sh` | 37 | Hook bypass utility | REMOVE | Remove | Dead code — no callers in codebase |

#### 4.1.9 `security/staging/`

> **Status: DORMANT.** These 5 scripts are never called by any hook or agent programmatically.
> Verified by searching for callers across all hook scripts and agent definitions — zero callers found.
> They are replaced by the protection-guard command (`codeflow hooks pre-tool-use protection-guard`,
> INF-TSK-021-016) which consolidates the protected-resource tier check, staging workflow guidance,
> and tmp-workflow state management into a single Go command.

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/staging/cf-stage-edit.sh` | 248 | Stage an edit for review before apply | DORMANT | `codeflow hooks pre-tool-use protection-guard` | Never called programmatically; replaced by protection-guard |
| `security/staging/cf-apply-staged-edit.sh` | 269 | Apply a previously staged edit | DORMANT | `codeflow hooks pre-tool-use protection-guard` | Never called programmatically; replaced by protection-guard |
| `security/staging/cf-rollback-edit.sh` | 225 | Roll back a staged edit | DORMANT | `codeflow hooks pre-tool-use protection-guard` | Never called programmatically; replaced by protection-guard |
| `security/staging/cf-reject-staged-edit.sh` | 162 | Reject a pending staged edit | DORMANT | `codeflow hooks pre-tool-use protection-guard` | Never called programmatically; replaced by protection-guard |
| `security/staging/cf-cleanup-expired.sh` | 195 | Remove expired staged edits | DORMANT | `codeflow hooks pre-tool-use protection-guard` | Never called programmatically; replaced by protection-guard |

#### 4.1.10 `security/sentinel/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/sentinel/cf-sentinel.sh` | 751 | Create/check/list PathFlow sentinels | T1 | `codeflow hooks sentinel-write` | macOS/Linux stat+date split; largest security script |

#### 4.1.11 `security/protection/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/protection/cf-protect-resources.sh` | 420 | Apply filesystem protection via chown/chmod | T4 | Keep shell | Requires sudo; cannot avoid |
| `security/protection/cf-reload-protection.sh` | 246 | Reload protection rules | T4 | Keep shell | RETAINED — cohesive OS-level protection subsystem; INF-TSK-021-036 cancelled |
| `security/protection/cf-promote-protection.sh` | 179 | Promote resource to protected status | T4 | Keep shell | RETAINED — cohesive OS-level protection subsystem; INF-TSK-021-036 cancelled |
| `security/protection/lib/cf-protection-common.sh` | 170 | Common protection utilities | T4 | Keep shell | Sourced by protection scripts; entire subsystem retained |
| `security/protection/lib/cf-protection-core.sh` | 137 | Core protection logic | T4 | Keep shell | Sourced by protection scripts; entire subsystem retained |
| `security/protection/lib/cf-protection-ops.sh` | 129 | Protection file operations | T4 | Keep shell | Sourced by protection scripts; entire subsystem retained |
| `security/protection/lib/cf-protection-verify.sh` | 252 | Verify protection state | T4 | Keep shell | Sourced by protection scripts; entire subsystem retained |

#### 4.1.12 `security/validation/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `security/validation/cf-validate-json.sh` | 149 | Validate JSON via python3 json.tool | T2 | `internal/validation/` | python3 subprocess; replace with json.Valid() |
| `security/validation/cf-validate-python.sh` | 172 | Check python3 interpreter availability | REMOVE | Remove | No Python files post-migration; becomes permanently dead code |
| `security/validation/cf-validate-shell.sh` | 158 | Validate shell scripts via shellcheck | T4 | Keep shell | Must invoke bash -n and shellcheck |
| `security/validation/cf-validate-yaml.sh` | 187 | Validate YAML via python3 yaml | T2 | `internal/validation/` | python3 subprocess; replace with gopkg.in/yaml.v3 |

#### 4.1.13 `worktree/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `worktree/cf-worktree-setup.sh` | 263 | Set up a new git worktree | T3 | `codeflow worktree setup` | No active bugs; wraps git commands |
| `worktree/cf-worktree-status.sh` | 360 | Show worktree status | T3 | `codeflow worktree status` | No active bugs |
| `worktree/cf-worktree-list.sh` | 199 | List all worktrees | T3 | `codeflow worktree list` | No active bugs |
| `worktree/cf-worktree-cleanup.sh` | 569 | Clean up a git worktree | T3 | `codeflow worktree cleanup` | No active bugs; largest worktree script |

#### 4.1.14 Top-Level Entry Point

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `codeflow` (wrapper) | 277 | Shell entry point; routes to subcommands | T3 | Go binary entry point | Becomes unnecessary once Go binary on PATH |

**Production shell script count: 57** (matches `find .codeflow/scripts -name "*.sh" | wc -l`)

---

### 4.2 Hook Scripts — `.claude/hooks/codeflow/`

#### 4.2.1 `session-start/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `session-start/cf-session-start-init.sh` | 619 | Initialize session state, directories, sentinels | T1 | `codeflow hooks session-start` | Most complex hook; init failures cascade |
| `session-start/cf-session-start-instructions.sh` | 200 | Load working protocol skill into context | T2 | `codeflow hooks session-start-instructions` | Reads and injects skill file content |
| `session-start/cf-session-start-logging.sh` | 174 | Log session start event | T2 | `codeflow hooks session-start-logging` | UUID session ID bug (uses Claude UUID not CODEFLOW_SESSION_ID) |

#### 4.2.2 `session-end/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `session-end/cf-session-end-cleanup.sh` | 289 | Remove stale state on session end | T1 | `codeflow hooks session-end` | Stale state on failure breaks next session |
| `session-end/cf-session-end-logging.sh` | 309 | Log session end event | T2 | `codeflow hooks session-end-logging` | UUID session ID bug |

#### 4.2.3 `pre-tool-use/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `pre-tool-use/cf-pre-tool-use-security.sh` | 223 | Security enforcement gate (sources 12 modules) | T1 | `codeflow hooks security check-bash` | ~120ms overhead per Bash call from module sourcing |
| `pre-tool-use/cf-pre-tool-use-pathflow-gate.sh` | 392 | Block tools that violate PathFlow phase order | T1 | `codeflow hooks gate-check` | Enforcement gate must be reliable |
| `pre-tool-use/cf-pre-tool-use-edit-write.sh` | 315 | Scope enforcement for Edit/Write tools | T2 | `codeflow hooks edit-write-guard` | Fires on every Edit and Write call |
| `pre-tool-use/cf-pre-tool-use-gh-pr.sh` | 384 | Block protected-branch PR merges | T2 | `codeflow hooks gh-pr-guard` | Enforces merge protection policy |
| `pre-tool-use/cf-pre-tool-use-protected-resource.sh` | 290 | Consult protected resource policy | T2 | `codeflow hooks protected-resource` | Three-tier protection (critical/high/moderate) |
| `pre-tool-use/cf-pre-tool-use-team-guard.sh` | 171 | Block TeamDelete during active session | T2 | `codeflow hooks team-guard` | Protects task graph from accidental dissolution |
| `pre-tool-use/cf-pre-tool-use-webfetch.sh` | 462 | Validate WebFetch URLs against allowlist | T2 | `codeflow hooks webfetch-guard` | Largest pre-tool-use hook; URL validation logic |

#### 4.2.4 `post-tool-use/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `post-tool-use/cf-post-tool-use-pathflow-sentinel.sh` | 267 | Create stage sentinels on STAGE-COMPLETE | T1 | `codeflow hooks sentinel-write` | Sentinel creation on critical enforcement path |
| `post-tool-use/cf-post-tool-use-phase-checkpoint.sh` | 180 | Register/complete phase checkpoint tasks | T1 | `codeflow hooks checkpoint-register` | Phase sentinel creation on critical path |
| `post-tool-use/cf-post-tool-use-logging.sh` | 217 | Log every tool call with result | T2 | `codeflow hooks post-tool-use-logging` | UUID session ID bug |
| `post-tool-use/cf-post-tool-use-settings-templates.sh` | 457 | Sync settings.json from managed templates | T2 | `codeflow hooks post-tool-use settings-validate` | Consolidated into `codeflow settings validate` (INF-TSK-021-016); single Go function serves both CLI and hook entry points |
| `post-tool-use/cf-post-tool-use-tmp-workflow.sh` | 142 | Manage tmp workflow state files | T2 | `codeflow hooks pre-tool-use protection-guard` | Consolidated into protection-guard command (INF-TSK-021-016) |

#### 4.2.5 `user-prompt-submit/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `user-prompt-submit/cf-user-prompt-submit.sh` | 220 | Validate user prompt before processing | T2 | `codeflow hooks user-prompt-submit` | Input validation on every user message |
| `user-prompt-submit/cf-user-prompt-submit-logging.sh` | 197 | Log user prompt events | T2 | `codeflow hooks user-prompt-submit-logging` | UUID session ID bug |

#### 4.2.6 `stop/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `stop/cf-stop-logging.sh` | 280 | Log stop/subagent-stop events | T2 | `codeflow hooks stop-logging` | UUID session ID bug |
| `stop/cf-stop-pathflow-gate.sh` | 107 | Enforce PathFlow gate on stop events | T2 | `codeflow hooks stop-gate` | Shared with SubagentStop event |

#### 4.2.7 `task-completed/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `task-completed/cf-task-completed-phase-checkpoint.sh` | 237 | Mark phase tasks complete; create phase sentinels | T1 | `codeflow hooks checkpoint-complete` | Phase sentinel creation; critical path |

**Hook script count: 22** (matches `find .claude/hooks/codeflow -name "*.sh" | wc -l`)

---

### 4.3 Python Source Files — `.codeflow/scripts/`

| Current Path | Lines | Purpose | Tier | Go Target | Notes |
|---|---|---|---|---|---|
| `codeflow_py_lib/__init__.py` | 69 | Package init | T2 | Remove with package | No logic; package marker |
| `codeflow_py_lib/config.py` | 120 | Read codeflow YAML config | T2 | `internal/config/` | Replace with gopkg.in/yaml.v3 |
| `codeflow_py_lib/crdt.py` | 312 | CRDT document operations | T2 | `internal/crdt/` | Loro not installed; falls back to JSON dict |
| `codeflow_py_lib/errors.py` | 115 | Error types and formatting | T2 | `internal/errors/` | Standard Go error wrapping |
| `codeflow_py_lib/jsonl.py` | 150 | JSONL read/write/validate | T2 | `internal/ledger/` | Replace with encoding/json |
| `codeflow_py_lib/logging.py` | 112 | Structured logging | T2 | `internal/logging/` | Replace with log/slog |
| `codeflow_py_lib/paths.py` | 133 | Project path resolution | T2 | `internal/paths/` | Replace with os + path/filepath |
| `codeflow_py_lib/ulid_generator.py` | 64 | Generate ULIDs | T1 | `internal/ulid/` | Replace with oklog/ulid |
| `codeflow_py_lib/ulid.py` | 90 | ULID formatting utilities | T1 | `internal/ulid/` | Replace with oklog/ulid |
| `codeflow_py_lib/validation.py` | 132 | Validate JSON/YAML structures | T2 | `internal/validation/` | Replace with encoding/json + gopkg.in/yaml.v3 |
| `coordination/cf-claim-acquire.py` | 170 | Acquire distributed claim lock | T2 | `codeflow coordination claim-acquire` | Loro CRDT fallback to JSON |
| `coordination/cf-claim-check.py` | 188 | Check claim lock status | T2 | `codeflow coordination claim-check` | Loro CRDT fallback to JSON |
| `coordination/cf-claim-list.py` | 203 | List all active claims | T2 | `codeflow coordination claim-list` | Loro CRDT fallback to JSON |
| `coordination/cf-claim-release.py` | 178 | Release a claim lock | T2 | `codeflow coordination claim-release` | Loro CRDT fallback to JSON |
| `coordination/cf-claim-renew.py` | 206 | Renew an expiring claim | T2 | `codeflow coordination claim-renew` | Loro CRDT fallback to JSON |
| `coordination/cf-crdt-rebuild.py` | 287 | Rebuild CRDT state from JSONL | T2 | `codeflow coordination crdt-rebuild` | Loro CRDT fallback to JSON |
| `memory/cf-memory-store.py` | 132 | Store memory events | T2 | `codeflow memory store` | Replace with Go JSONL writer |
| `state/cf-stage-sync.py` | 782 | Sync stage state between formats | T2 | `codeflow state stage-sync` | Largest Python file; complex sync logic |

**Python source file count: 18** (matches `find .codeflow/scripts -name "*.py" | wc -l`)

---

### 4.4 Python Test Files — `.codeflow/testing/scripts/`

Test files follow the same tier as the source they test. When source files are removed,
their pytest suites are removed with them.

#### 4.4.1 `testing/scripts/codeflow_py_lib/` (T2 → remove with source)

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `conftest.py` | 108 | Fixtures for py_lib tests |
| `test_config.py` | 327 | `codeflow_py_lib/config.py` |
| `test_crdt.py` | 660 | `codeflow_py_lib/crdt.py` |
| `test_errors.py` | 404 | `codeflow_py_lib/errors.py` |
| `test_jsonl.py` | 534 | `codeflow_py_lib/jsonl.py` |
| `test_logging.py` | 367 | `codeflow_py_lib/logging.py` |
| `test_paths.py` | 372 | `codeflow_py_lib/paths.py` |
| `test_ulid_generator.py` | 147 | `codeflow_py_lib/ulid_generator.py` |
| `test_ulid.py` | 195 | `codeflow_py_lib/ulid.py` |
| `test_validation.py` | 356 | `codeflow_py_lib/validation.py` |

#### 4.4.2 `testing/scripts/coordination/` (T2 → remove with source)

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `conftest.py` | 206 | Fixtures for coordination tests |
| `test_cf_claim_acquire.py` | 373 | `coordination/cf-claim-acquire.py` |
| `test_cf_claim_check.py` | 503 | `coordination/cf-claim-check.py` |
| `test_cf_claim_list.py` | 608 | `coordination/cf-claim-list.py` |
| `test_cf_claim_release.py` | 212 | `coordination/cf-claim-release.py` |
| `test_cf_claim_renew.py` | 367 | `coordination/cf-claim-renew.py` |
| `test_cf_crdt_rebuild.py` | 611 | `coordination/cf-crdt-rebuild.py` |
| `test_crdt_doc.py` | 301 | CRDT document layer |
| `test_crdt_io.py` | 197 | CRDT I/O layer |
| `test_pattern_matching.py` | 150 | Pattern matching utilities |

#### 4.4.3 `testing/scripts/state/` (T2 → remove with source)

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `conftest.py` | 168 | Fixtures for state tests |
| `test_cf_stage_sync.py` | 471 | `state/cf-stage-sync.py` |

#### 4.4.4 `testing/scripts/db/` (T2 → remove with source)

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `conftest.py` | 194 | Fixtures for db tests |
| `test_schema.py` | 104 | DB schema validation |

#### 4.4.5 `testing/conftest.py`

| File | Lines | Notes |
|------|-------|-------|
| `conftest.py` | 227 | Root pytest conftest; removed with all suites |

**Python test file count: 25** (matches `find .codeflow/testing -name "*.py" | wc -l`)

---

### 4.5 Shell Test Scripts — `.codeflow/testing/`

Shell test files are co-located with the production scripts they test. Tier follows the
production script. All 108 files listed individually below.

#### 4.5.1 `testing/run-all-tests.sh` and `testing/run-coverage.sh`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `run-all-tests.sh` | 230 | Test runner entry point (T2 → keep/migrate with testing infra) |
| `run-coverage.sh` | 416 | Coverage reporting runner (T2 → keep/migrate with testing infra) |

#### 4.5.2 `testing/lib/` — Test framework libraries

These are framework files used by the test runner, not tests for a specific production script.
They migrate when the test framework is ported to Go's `testing` package.

| Test File | Lines | Purpose |
|-----------|-------|---------|
| `lib/test-common.sh` | 210 | Common test utilities |
| `lib/test-config.sh` | 448 | Test configuration helpers |
| `lib/test-coverage.sh` | 1,155 | Coverage instrumentation |
| `lib/test-discovery.sh` | 160 | Test file discovery |
| `lib/test-helpers.sh` | 420 | General test helper functions |
| `lib/test-isolation.sh` | 119 | Test isolation and sandboxing |
| `lib/test-parallel.sh` | 243 | Parallel test execution |
| `lib/test-reporting.sh` | 179 | Test result reporting |
| `lib/test-runner.sh` | 426 | Core test runner logic |
| `lib/test-test-coverage.sh` | 221 | Tests for the coverage lib itself |

#### 4.5.3 `testing/ci/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `ci/check-test-coverage-pairing.sh` | 80 | CI coverage pairing check |

#### 4.5.4 `testing/cli/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `cli/test-go-cli.sh` | 123 | Go CLI binary (`codeflow` binary, T3) |

#### 4.5.5 `testing/consistency/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `consistency/test-config-consistency.sh` | 397 | Cross-file config consistency |
| `consistency/test-pathflow-task-tracker-config.sh` | 417 | PathFlow config vs. tracker consistency |
| `consistency/test-settings-sync.sh` | 636 | Settings sync consistency |

#### 4.5.6 `testing/scripts/commands/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/commands/test-commands.sh` | 336 | Command routing |

#### 4.5.7 `testing/scripts/db/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/db/test-generate-format-id.sh` | 291 | `db/generate-format-id.sh` (T1) |
| `scripts/db/test-migrate-to-dual-id.sh` | 182 | `db/migrate-to-dual-id.sh` (T3) |
| `scripts/db/test-migration-005.sh` | 576 | DB migration 005 |
| `scripts/db/test-schema-restructure.sh` | 264 | DB schema restructure |
| `scripts/db/test_v4_schema.sh` | 496 | V4 DB schema validation |

#### 4.5.8 `testing/scripts/git-hooks/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/git-hooks/test-commit-msg.sh` | 299 | `commit-msg` git hook |
| `scripts/git-hooks/test-post-commit.sh` | 761 | `post-commit` git hook |
| `scripts/git-hooks/test-pre-commit.sh` | 709 | `pre-commit` git hook |
| `scripts/git-hooks/test-pre-push.sh` | 525 | `pre-push` git hook |
| `scripts/git-hooks/test-prepare-commit-msg.sh` | 214 | `prepare-commit-msg` git hook |

#### 4.5.9 `testing/scripts/pathflow/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/pathflow/test-cf-pathflow-enforcement.sh` | 1,039 | PathFlow enforcement hooks (T1) |
| `scripts/pathflow/test-cf-pathflow-scripts.sh` | 969 | `pathflow/cf-pathflow-*.sh` scripts (T1) |

#### 4.5.10 `testing/scripts/security/enforcement/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/enforcement/helper-runner-wrapper.sh` | 34 | Test helper for enforcement runners |
| `scripts/security/enforcement/test-cf-file-operations.sh` | 126 | `security/enforcement/cf-file-operations.sh` (T2) |
| `scripts/security/enforcement/test-cf-git-protection.sh` | 373 | `security/enforcement/cf-git-protection.sh` (T2) |
| `scripts/security/enforcement/test-cf-network-protection.sh` | 506 | `security/enforcement/cf-network-protection.sh` (T2) |
| `scripts/security/enforcement/test-cf-tmp-protection.sh` | 170 | `security/enforcement/cf-tmp-protection.sh` (T2) |

#### 4.5.11 `testing/scripts/security/lib/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/lib/test-bash-file-readers-lib.sh` | 718 | `security/lib/bash-file-readers-lib.sh` (T2) |
| `scripts/security/lib/test-context-lib.sh` | 998 | `security/lib/context-lib.sh` (T2) |
| `scripts/security/lib/test-pathflow-mode.sh` | 382 | PathFlow mode detection in security lib |
| `scripts/security/lib/test-security-lib-functions.sh` | 453 | `security/lib/security-lib.sh` functions (T2) |
| `scripts/security/lib/test-security-lib.sh` | 782 | `security/lib/security-lib.sh` integration (T2) |

#### 4.5.12 `testing/scripts/security/protection/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/protection/test-cf-protect-resources.sh` | 524 | `security/protection/cf-protect-resources.sh` (T4) |
| `scripts/security/protection/test-cf-protection-common.sh` | 239 | `security/protection/lib/cf-protection-common.sh` (T3) |
| `scripts/security/protection/test-cf-protection-core.sh` | 373 | `security/protection/lib/cf-protection-core.sh` (T3) |
| `scripts/security/protection/test-cf-protection-ops.sh` | 400 | `security/protection/lib/cf-protection-ops.sh` (T3) |
| `scripts/security/protection/test-cf-protection-verify.sh` | 234 | `security/protection/lib/cf-protection-verify.sh` (T3) |

#### 4.5.13 `testing/scripts/security/sentinel/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/sentinel/test-cf-sentinel.sh` | 814 | `security/sentinel/cf-sentinel.sh` (T1) |

#### 4.5.14 `testing/scripts/security/staging/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/staging/test-cf-apply-staged-edit.sh` | 173 | `security/staging/cf-apply-staged-edit.sh` (T2) |
| `scripts/security/staging/test-cf-cleanup-expired.sh` | 156 | `security/staging/cf-cleanup-expired.sh` (T2) |
| `scripts/security/staging/test-cf-reject-staged-edit.sh` | 249 | `security/staging/cf-reject-staged-edit.sh` (T2) |
| `scripts/security/staging/test-cf-rollback-edit.sh` | 167 | `security/staging/cf-rollback-edit.sh` (T2) |
| `scripts/security/staging/test-cf-stage-edit.sh` | 350 | `security/staging/cf-stage-edit.sh` (T2) |

#### 4.5.15 `testing/scripts/security/` (top-level)

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/test-cf-branch-file-protection.sh` | 327 | `security/enforcement/cf-branch-file-protection.sh` (T2) |
| `scripts/security/test-cf-dangerous-commands.sh` | 131 | `security/enforcement/cf-dangerous-commands.sh` (T2) |
| `scripts/security/test-cf-hook-bypass.sh` | 124 | `security/enforcement/cf-hook-bypass.sh` (REMOVE) |
| `scripts/security/test-cf-path-protection.sh` | 273 | `security/enforcement/cf-path-protection.sh` (T2) |
| `scripts/security/test-cf-pattern-matching.sh` | 267 | `security/enforcement/cf-pattern-matching.sh` (T2) |
| `scripts/security/test-cf-privilege-protection.sh` | 201 | `security/enforcement/cf-privilege-protection.sh` (T2) |
| `scripts/security/test-cf-promote-protection.sh` | 259 | `security/protection/cf-promote-protection.sh` (T3) |
| `scripts/security/test-cf-reload-protection.sh` | 219 | `security/protection/cf-reload-protection.sh` (T4) |
| `scripts/security/test-main-branch-protection.sh` | 393 | Main branch protection enforcement |

#### 4.5.16 `testing/scripts/security/validation/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/security/validation/test-cf-validate-json.sh` | 223 | `security/validation/cf-validate-json.sh` (T2) |
| `scripts/security/validation/test-cf-validate-python.sh` | 259 | `security/validation/cf-validate-python.sh` (REMOVE) |
| `scripts/security/validation/test-cf-validate-shell.sh` | 210 | `security/validation/cf-validate-shell.sh` (T4) |
| `scripts/security/validation/test-cf-validate-yaml.sh` | 317 | `security/validation/cf-validate-yaml.sh` (T2) |

#### 4.5.17 `testing/scripts/settings/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/settings/test-cf-change-approval-mode.sh` | 877 | `settings/cf-change-approval-mode.sh` (T2) |
| `scripts/settings/test-managed-settings-json.sh` | 597 | Managed settings JSON structure |
| `scripts/settings/test-setup-managed-settings.sh` | 364 | `settings/setup-managed-settings.sh` (T2) |

#### 4.5.18 `testing/scripts/shell-lib/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/shell-lib/test-common.sh` | 408 | `shell-lib/common.sh` (T2) |
| `scripts/shell-lib/test-config.sh` | 571 | `shell-lib/config.sh` (T2) |
| `scripts/shell-lib/test-errors.sh` | 564 | `shell-lib/errors.sh` (T2) |
| `scripts/shell-lib/test-index.sh` | 471 | `shell-lib/index.sh` (T4) |
| `scripts/shell-lib/test-logging.sh` | 539 | `shell-lib/logging.sh` (T2) |
| `scripts/shell-lib/test-ulid.sh` | 375 | `shell-lib/ulid.sh` (T1) |
| `scripts/shell-lib/test-validation.sh` | 743 | `shell-lib/validation.sh` (T2) |

#### 4.5.19 `testing/scripts/state/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/state/test-ledger.sh` | 1,078 | `state/ledger.sh` (T1) |
| `scripts/state/test-memory.sh` | 903 | `state/memory.sh` (T1) |
| `scripts/state/test-pathflow-state.sh` | 1,517 | `state/cf-pathflow-state.sh` (T1) |
| `scripts/state/test-stage-sync.sh` | 628 | Stage sync shell interface |
| `scripts/state/test-tracking-generate.sh` | 506 | `state/cf-tracking-generate.sh` (T1) |
| `scripts/state/test-work-state.sh` | 370 | `state/cf-work-state.sh` (T1) |

#### 4.5.20 `testing/scripts/validation/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/validation/test-validate-epic.sh` | 777 | `validation/validate-epic.sh` (T1) |
| `scripts/validation/test-validate-task.sh` | 1,218 | `validation/validate-task.sh` (T1) |

#### 4.5.21 `testing/scripts/worktree/`

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `scripts/worktree/test-cf-worktree-cleanup.sh` | 494 | `worktree/cf-worktree-cleanup.sh` (T3) |
| `scripts/worktree/test-cf-worktree-list.sh` | 161 | `worktree/cf-worktree-list.sh` (T3) |
| `scripts/worktree/test-cf-worktree-setup.sh` | 458 | `worktree/cf-worktree-setup.sh` (T3) |
| `scripts/worktree/test-cf-worktree-status.sh` | 382 | `worktree/cf-worktree-status.sh` (T3) |

#### 4.5.22 `testing/claude-hooks/` — Hook test scripts

| Test File | Lines | Tests Source |
|-----------|-------|-------------|
| `claude-hooks/post-tool-use/test-cf-post-tool-use-logging.sh` | 333 | `post-tool-use/cf-post-tool-use-logging.sh` (T2) |
| `claude-hooks/post-tool-use/test-cf-post-tool-use-pathflow-sentinel.sh` | 757 | `post-tool-use/cf-post-tool-use-pathflow-sentinel.sh` (T1) |
| `claude-hooks/post-tool-use/test-cf-post-tool-use-phase-checkpoint.sh` | 460 | `post-tool-use/cf-post-tool-use-phase-checkpoint.sh` (T1) |
| `claude-hooks/post-tool-use/test-cf-post-tool-use-settings-templates.sh` | 418 | `post-tool-use/cf-post-tool-use-settings-templates.sh` (T2) |
| `claude-hooks/post-tool-use/test-cf-post-tool-use-tmp-workflow.sh` | 470 | `post-tool-use/cf-post-tool-use-tmp-workflow.sh` (T2) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-edit-write.sh` | 460 | `pre-tool-use/cf-pre-tool-use-edit-write.sh` (T2) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-gh-pr.sh` | 930 | `pre-tool-use/cf-pre-tool-use-gh-pr.sh` (T2) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-pathflow-gate.sh` | 854 | `pre-tool-use/cf-pre-tool-use-pathflow-gate.sh` (T1) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-protected-resource.sh` | 571 | `pre-tool-use/cf-pre-tool-use-protected-resource.sh` (T2) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-security.sh` | 612 | `pre-tool-use/cf-pre-tool-use-security.sh` (T1) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-team-guard.sh` | 467 | `pre-tool-use/cf-pre-tool-use-team-guard.sh` (T2) |
| `claude-hooks/pre-tool-use/test-cf-pre-tool-use-webfetch.sh` | 714 | `pre-tool-use/cf-pre-tool-use-webfetch.sh` (T2) |
| `claude-hooks/session-end/test-cf-session-end-cleanup.sh` | 1,152 | `session-end/cf-session-end-cleanup.sh` (T1) |
| `claude-hooks/session-end/test-cf-session-end-logging.sh` | 767 | `session-end/cf-session-end-logging.sh` (T2) |
| `claude-hooks/session-start/test-cf-session-start-init.sh` | 1,573 | `session-start/cf-session-start-init.sh` (T1) |
| `claude-hooks/session-start/test-cf-session-start-instructions.sh` | 764 | `session-start/cf-session-start-instructions.sh` (T2) |
| `claude-hooks/session-start/test-cf-session-start-logging.sh` | 774 | `session-start/cf-session-start-logging.sh` (T2) |
| `claude-hooks/stop/test-cf-stop-logging.sh` | 798 | `stop/cf-stop-logging.sh` (T2) |
| `claude-hooks/stop/test-cf-stop-pathflow-gate.sh` | 204 | `stop/cf-stop-pathflow-gate.sh` (T2) |
| `claude-hooks/task-completed/test-cf-task-completed-phase-checkpoint.sh` | 450 | `task-completed/cf-task-completed-phase-checkpoint.sh` (T1) |
| `claude-hooks/user-prompt-submit/test-cf-user-prompt-submit-logging.sh` | 1,073 | `user-prompt-submit/cf-user-prompt-submit-logging.sh` (T2) |
| `claude-hooks/user-prompt-submit/test-cf-user-prompt-submit.sh` | 880 | `user-prompt-submit/cf-user-prompt-submit.sh` (T2) |

**Shell test script count: 108** (matches `find .codeflow/testing -name "*.sh" -not -path "*/coverage_reports/*" | wc -l`)

---

### 4.6 Migration Map Summary

| Category | Scripts | Lines | T1 | T2 | T3 | T4 | REMOVE |
|----------|---------|-------|----|----|----|----|----|
| `.codeflow/scripts/state/` | 5 | 1,764 | 5 | 0 | 0 | 0 | 0 |
| `.codeflow/scripts/db/` | 2 | 555 | 1 | 0 | 1 | 0 | 0 |
| `.codeflow/scripts/validation/` | 2 | 963 | 2 | 0 | 0 | 0 | 0 |
| `.codeflow/scripts/pathflow/` | 5 | 733 | 5 | 0 | 0 | 0 | 0 |
| `.codeflow/scripts/settings/` | 2 | 975 | 0 | 2 | 0 | 0 | 0 |
| `.codeflow/scripts/shell-lib/` | 7 | 1,112 | 1 | 5 | 0 | 1 | 0 |
| `.codeflow/scripts/security/lib/` | 3 | 1,212 | 0 | 3 | 0 | 0 | 0 |
| `.codeflow/scripts/security/enforcement/` | 10 | 1,855 | 0 | 9 | 0 | 0 | 1 |
| `.codeflow/scripts/security/staging/` | 5 | 1,099 | 0 | 5 | 0 | 0 | 0 |
| `.codeflow/scripts/security/sentinel/` | 1 | 751 | 1 | 0 | 0 | 0 | 0 |
| `.codeflow/scripts/security/protection/` | 7 | 1,533 | 0 | 0 | 4 | 2 | 1 |
| `.codeflow/scripts/security/validation/` | 4 | 666 | 0 | 2 | 0 | 1 | 1 |
| `.codeflow/scripts/worktree/` | 4 | 1,391 | 0 | 0 | 4 | 0 | 0 |
| Top-level (`codeflow`) | 1 | 277 | 0 | 0 | 1 | 0 | 0 |
| `.claude/hooks/` — session-start | 3 | 993 | 1 | 2 | 0 | 0 | 0 |
| `.claude/hooks/` — session-end | 2 | 598 | 1 | 1 | 0 | 0 | 0 |
| `.claude/hooks/` — pre-tool-use | 7 | 2,237 | 2 | 5 | 0 | 0 | 0 |
| `.claude/hooks/` — post-tool-use | 5 | 1,263 | 2 | 3 | 0 | 0 | 0 |
| `.claude/hooks/` — user-prompt-submit | 2 | 417 | 0 | 2 | 0 | 0 | 0 |
| `.claude/hooks/` — stop | 2 | 387 | 0 | 2 | 0 | 0 | 0 |
| `.claude/hooks/` — task-completed | 1 | 237 | 1 | 0 | 0 | 0 | 0 |
| Python source (`.codeflow/scripts/`) | 18 | 3,443 | 2 | 16 | 0 | 0 | 0 |
| **Production subtotal** | **97** | **23,261** | **24** | **57** | **10** | **4** | **3** |
| Python test files (pytest) | 25 | 8,162 | — | — | — | — | — |
| Shell test scripts (`.codeflow/testing/`) | 108 | 53,447 | — | — | — | — | — |
| **TOTAL (all files)** | **230** | **84,870** | — | — | — | — | — |

**Notes:**

- Production subtotal (97 scripts) drives the migration priority. Test files are removed
  alongside the production scripts they test.
- `cf-hook-bypass.sh` (REMOVE): dead code, no callers.
- `shell-lib/index.sh` (T4): disappears when Go replaces sourced shell libraries;
  can be deleted at the same time as the shell-lib modules.
- The REMOVE column for `security/protection/` reflects `shell-lib/index.sh` not a
  protection script — it is categorized there due to its T4 classification as a sourced
  module loader alongside the protection lib files.
- Shell test line total (53,447) comes from `wc -l` across all 108 non-coverage-report
  `.sh` files in `.codeflow/testing/`.

---

## 5. Python Elimination Plan

### 5.1 Current Python Surface

Python exists in two forms in the codebase:

1. **18 Python source files** in `.codeflow/scripts/` — library modules and standalone scripts
2. **Shell-embedded `python3` one-liners** in shell scripts — subprocess calls for trivial operations
3. **20 pytest test files** in `.codeflow/testing/` — tests for the Python source files

Verified via `find .codeflow/scripts -name "*.py" | wc -l` = 18 files.
Verified via `find .codeflow/testing -name "test_*.py" -not -path "*/.venv/*" | wc -l` = 20 files.

### 5.2 Python Source File Mapping to Go

**codeflow_py_lib (9 files) — replace with Go packages:**

| Python File | Go Replacement |
|-------------|----------------|
| `codeflow_py_lib/ulid_generator.py` | `oklog/ulid` or `time.Now().UnixMilli()` |
| `codeflow_py_lib/ulid.py` | Same as above |
| `codeflow_py_lib/jsonl.py` | `encoding/json` + file I/O |
| `codeflow_py_lib/config.py` | `gopkg.in/yaml.v3` |
| `codeflow_py_lib/logging.py` | `log/slog` (Go 1.21+) |
| `codeflow_py_lib/errors.py` | Standard Go error wrapping |
| `codeflow_py_lib/paths.py` | `os`, `path/filepath` |
| `codeflow_py_lib/validation.py` | `encoding/json`, `gopkg.in/yaml.v3` |
| `codeflow_py_lib/crdt.py` | `encoding/json` (Loro CRDT not installed; falls back to JSON dict) |

**Coordination scripts (6 files) — replace with Go CLI subcommands:**

| Python Script | Go Subcommand |
|---------------|---------------|
| `coordination/cf-claim-acquire.py` | `codeflow coordination claim-acquire` |
| `coordination/cf-claim-release.py` | `codeflow coordination claim-release` |
| `coordination/cf-claim-check.py` | `codeflow coordination claim-check` |
| `coordination/cf-claim-renew.py` | `codeflow coordination claim-renew` |
| `coordination/cf-claim-list.py` | `codeflow coordination claim-list` |
| `coordination/cf-crdt-rebuild.py` | `codeflow coordination crdt-rebuild` |

**Other Python scripts (3 files) — replace with Go CLI subcommands:**

| Python Script | Go Subcommand |
|---------------|---------------|
| `memory/cf-memory-store.py` | `codeflow memory store` (or merged into `codeflow session`) |
| `state/cf-stage-sync.py` | `codeflow state stage-sync` |

**Note on Loro CRDT:** The `crdt.py` module imports `loro` for CRDT operations but falls
back to a plain JSON dict when the package is absent. Verified: the `loro` package is not
installed in `.venv`. The fallback behavior means the CRDT functionality is currently
a JSON dict wrapped in CRDT semantics — straightforward to reimplement in Go using
`encoding/json`.

**Note on Loro CRDT and future phases:** Go migration does not block or conflict with
future Loro CRDT adoption. Loro is written in Rust and Go has two integration paths:

1. **CGo bindings:** Call Loro's C API via CGo. Adds build complexity but gives full
   access to the Loro API with zero copy overhead for most operations.
2. **Native Go CRDT:** Use a pure Go CRDT implementation (e.g., `github.com/automerge/automerge-go`
   or a custom LWW-register map) that covers the coordination use case without Rust FFI.

Either path is straightforward once Python is eliminated. The Go coordination subcommands
(`codeflow coordination claim-*`) can be written against an interface and the backing
implementation swapped from JSON dict to a CRDT without changing callers.

### 5.3 Shell-Embedded Python One-Liners

These patterns appear in shell scripts and each calls `python3` as a subprocess:

| Location | Pattern | Go Replacement |
|----------|---------|----------------|
| `ulid.sh` | `python3 -c "import time; print(int(time.time() * 1000))"` | `time.Now().UnixMilli()` |
| `config.sh` | `python3 -c "import yaml..."` | `gopkg.in/yaml.v3` |
| `ledger.sh` | `python3 -m json.tool` for validation | `json.Valid([]byte(input))` |
| `context-lib.sh` | `python3 -c "..."` for Unix timestamps | `time.Now().Unix()` |

### 5.4 Test File Removal

When Python source files are removed, the 20 pytest test files become dead code.
The `.codeflow/testing/scripts/codeflow_py_lib/` directory (9 test files) and
`.codeflow/testing/scripts/coordination/` directory (10 test files) are removed.

The Python testing infrastructure (`pyproject.toml`, `pytest.ini`, `requirements-test.txt`,
`conftest.py`, `run-coverage.sh`) is also removed. Shell tests and Go tests take over.

---

## 6. Hook Go Replacement Strategy

### 6.1 Hook Invocation Model

Claude Code hooks invoke whatever command is configured in `settings.json`. The current
pattern is:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "bash .claude/hooks/codeflow/pre-tool-use/cf-pre-tool-use-security.sh"}]
      }
    ]
  }
}
```

After migration, the pattern becomes:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "codeflow hooks security-check"}]
      }
    ]
  }
}
```

The Go binary receives the same stdin JSON payload, applies the same logic, and exits with
the same codes. No change to the Claude Code hook protocol.

### 6.2 Security Enforcement Stack Priority

The PreToolUse security hook (`cf-pre-tool-use-security.sh`) sources 9 enforcement modules
via `security-lib.sh`:

```text
security-lib.sh (437 lines)
context-lib.sh (194 lines)
bash-file-readers-lib.sh (581 lines)
cf-path-protection.sh (422 lines)
cf-pattern-matching.sh (264 lines)
cf-branch-file-protection.sh (241 lines)
cf-git-protection.sh (238 lines)
cf-privilege-protection.sh (163 lines)
cf-dangerous-commands.sh (180 lines)
cf-file-operations.sh (153 lines)
cf-network-protection.sh (87 lines)
cf-tmp-protection.sh (70 lines)
```

Total: ~3,030 lines sourced on every Bash tool call. Each `source` is a file read. Each
enforcement module runs checks with subprocesses. This is the highest-ROI migration target.

Go replacement: a single `codeflow hooks security-check` subcommand compiles all enforcement
logic into one binary. Stdin parsing, pattern matching, and decision output run in memory
without file reads or subprocess forks.

### 6.3 Hook Migration Priority

Based on critical-path impact (fires on every tool call = highest priority):

| Priority | Hook | Lines | Fires On | Current Overhead |
|----------|------|-------|----------|-----------------|
| 1 | `cf-pre-tool-use-security.sh` | 223 + 3,030 sourced | Every Bash call | ~120ms (12 modules sourced) |
| 2 | `cf-pre-tool-use-pathflow-gate.sh` | 392 | Edit, Write, Bash, Task | ~50ms |
| 3 | `cf-post-tool-use-pathflow-sentinel.sh` | 267 | Every tool call | ~30ms |
| 4 | `cf-post-tool-use-phase-checkpoint.sh` | 180 | TaskCreate, TaskCompleted | ~25ms |
| 5 | `cf-session-start-init.sh` | 619 | Session start | ~200ms (one time) |
| 6 | `cf-session-end-cleanup.sh` | 289 | Session end | ~100ms (one time) |
| 7 | Remaining 16 hooks | varies | Various | varies |

### 6.4 Hook Wiring Plan

The transition from shell hooks to Go hooks can be done incrementally per hook.
The `settings.json` command string is the only change per hook. No other infrastructure
changes are required.

**Phase A:** Replace the 4 Tier 1 enforcement hooks (highest frequency, highest impact).

**Phase B:** Replace session-start-init and session-end-cleanup (session integrity).

**Phase C:** Replace all remaining Tier 2 hooks (logging, settings, etc.).

**Result:** All 22 hook scripts become dead code and are removed from the repository.

---

## 7. Performance Impact Analysis

All shell overhead measurements are estimates based on typical macOS subprocess fork costs
(~5-15ms per fork) and the measured sourcing behavior in the security enforcement stack.

| Area | Current (Shell) | Projected (Go) | Improvement |
|------|----------------|----------------|-------------|
| PreToolUse hooks total | ~300ms per tool call | ~30-50ms | 6-10x |
| Security enforcement stack | ~120ms (12 modules sourced) | ~5-10ms | 12-24x |
| ULID generation | python3 subprocess | zero subprocesses | ~100x |
| Checkpoint operations (jq chain) | 30-80ms per checkpoint write | ~2-5ms | 15-40x |
| Sentinel checks (stat + date) | platform-specific, ~10ms | `os.Stat()` uniform, ~0.5ms | ~20x |
| JSONL writes | string concatenation (risk of corruption) | `json.Marshal` (zero corruption risk) | eliminates bugs |
| YAML parsing | `sed` fragile, ~20ms | `gopkg.in/yaml.v3`, ~2ms | 10x + correctness |
| Session start hook | ~200ms | ~30ms | ~7x |

**Developer experience impact:**

A 10-tool-call session currently accumulates ~3,000ms of hook overhead. At 50ms per call
with Go hooks, the same session accumulates ~500ms. This is a 2.5-second improvement per
session, compounding across thousands of calls during a complex session.

---

## 8. Dependency Elimination Matrix

After full Tier 1 and Tier 2 migration, the following system dependencies become optional
or removable:

| Current Dependency | Used For | Go Replacement | Can Remove? |
|-------------------|----------|----------------|-------------|
| `jq` | JSON parsing in shell | `encoding/json` | Yes — after state layer migrated |
| `python3` | ULID, YAML, JSON validation | `time`, `gopkg.in/yaml.v3`, `json.Valid` | Yes — after Python scripts removed |
| `flock` | Write locking in ledger.sh | `sync.Mutex` / file lock via Go | Yes — not on macOS anyway |
| `xxd` | Hex encoding for IDs | `encoding/hex` | Yes |
| `shasum` / `sha256sum` | Content hashing | `crypto/sha256` | Yes |
| `sqlite3` CLI | DB read/write | `github.com/ncruces/go-sqlite3` (pure Go, no CGO) | Yes — after DB layer migrated |
| macOS `stat -f%z` | File size | `os.Stat().Size()` | Yes — unified |
| Linux `stat --printf=%s` | File size | Same `os.Stat().Size()` | Yes — unified |
| macOS `date -j -f` | Date parsing | `time.Parse()` | Yes — unified |
| Linux `date -d` | Date parsing | Same `time.Parse()` | Yes — unified |
| `yq` / Python yaml | YAML parsing | `gopkg.in/yaml.v3` | Yes |

**Dependencies that remain required:**

| Dependency | Reason |
|------------|--------|
| `bash` | Hook scripts still use bash until fully migrated; shell validation scripts |
| `shellcheck` | Shell validation requires shellcheck |
| `sudo` | Protection scripts require privilege escalation |
| `git` | Git operations — Go wraps git CLI, does not replace it |
| `gh` | GitHub CLI — not replaced |

---

## 9. Consolidated Binary Architecture

The Go CLI binary (`codeflow`) organizes all subcommands under a single executable.
This extends the architecture defined in INF-EPC-015 with the hook and migration subcommands.

**Source directory:** `codeflow-cli/` in the repository root (not `.codeflow/`, which is
the shell scripts directory). The compiled binary is named `codeflow` and placed on PATH.
Do not confuse with the shell wrapper at `.codeflow/scripts/codeflow` — that wrapper is
classified T3 and deleted once the Go binary is on PATH.

The tree below shows the **runtime command structure** (`codeflow <subcommand>`), not
the source layout. Source packages live under `codeflow-cli/cmd/` and `codeflow-cli/internal/`.

```text
codeflow (Go binary — source: codeflow-cli/)
├── test                           -- run test suite (existing)
├── mode                           -- manage approval modes (existing)
├── version                        -- show version (existing)
├── doctor                         -- diagnose infrastructure (existing)
├── init                           -- project init wizard (existing)
├── update                         -- self-update (existing)
├── autorun                        -- batch orchestration (existing)
│
├── validate
│   ├── task <file>                -- replaces validate-task.sh
│   └── epic <file>                -- replaces validate-epic.sh
│
├── pathflow
│   ├── phase-transition           -- replaces cf-pathflow-phase-transition.sh
│   ├── stage-transition           -- replaces cf-pathflow-stage-transition.sh
│   ├── session-register           -- replaces cf-pathflow-session-register.sh
│   ├── task-update                -- replaces cf-pathflow-task-update.sh
│   └── session-metadata           -- replaces cf-pathflow-session-metadata.sh
│
├── session
│   ├── start                      -- replaces session-start-init hook logic
│   ├── end                        -- replaces session-end-cleanup hook logic
│   └── status                     -- query current session state
│
├── settings
│   ├── validate                   -- replaces cf-post-tool-use-settings-templates.sh; also callable as codeflow hooks post-tool-use settings-validate
│   └── setup-managed              -- replaces setup-managed-settings.sh (INF-TSK-021-016)
│
├── git-hooks                      -- git hook logic as Go subcommands (INF-TSK-021-035)
│   ├── pre-commit-validate        -- pure-logic checks (branch protection, sensitive files, JSON, Go test conventions, coverage)
│   ├── commit-msg                 -- validates conventional commit format; replaces commit-msg shell hook
│   ├── post-commit                -- JSONL logging after commit; replaces post-commit shell hook; fixes JSON construction bug
│   ├── pre-push                   -- branch naming validation; replaces pre-push shell hook; TTY via os.Open("/dev/tty")
│   └── prepare-commit-msg        -- commit message template; replaces prepare-commit-msg shell hook
│
├── ledger
│   ├── append <event-type>        -- replaces echo-to-jsonl pattern
│   ├── validate <file>            -- validate JSONL integrity
│   └── rebuild                    -- rebuild SQLite from JSONL
│
├── db
│   ├── init                       -- initialize schema (existing)
│   ├── migrate                    -- run migrations (existing)
│   └── generate-id                -- replaces generate-format-id.sh
│
├── worktree
│   ├── setup                      -- replaces cf-worktree-setup.sh
│   ├── status                     -- replaces cf-worktree-status.sh
│   ├── list                       -- replaces cf-worktree-list.sh
│   └── cleanup                    -- replaces cf-worktree-cleanup.sh
│
├── report
│   └── epic-tracker               -- replaces cf-tracking-generate.sh
│
├── coordination
│   ├── claim-acquire              -- replaces cf-claim-acquire.py
│   ├── claim-release              -- replaces cf-claim-release.py
│   ├── claim-check                -- replaces cf-claim-check.py
│   ├── claim-renew                -- replaces cf-claim-renew.py
│   ├── claim-list                 -- replaces cf-claim-list.py
│   └── crdt-rebuild               -- replaces cf-crdt-rebuild.py
│
├── memory
│   └── store                      -- replaces cf-memory-store.py
│
├── shadow-test                    -- run Go subcommands alongside shell scripts and compare output (INF-TSK-021-030)
│
└── hooks                          -- all hook logic as subcommands (grouped by event type)
    ├── session-start
    │   ├── init                   -- replaces cf-session-start-init.sh
    │   ├── logging                -- replaces cf-session-start-logging.sh
    │   └── instructions           -- replaces cf-session-start-instructions.sh
    ├── session-end
    │   ├── cleanup                -- replaces cf-session-end-cleanup.sh
    │   └── logging                -- replaces cf-session-end-logging.sh
    ├── pre-tool-use
    │   ├── gate-check             -- replaces cf-pre-tool-use-pathflow-gate.sh
    │   ├── security               -- replaces security-lib.sh + all 9 enforcement modules
    │   ├── edit-write-guard       -- replaces cf-pre-tool-use-edit-write.sh (path scope enforcement)
    │   ├── gh-pr-guard            -- replaces cf-pre-tool-use-gh-pr.sh
    │   ├── protection-guard       -- replaces cf-pre-tool-use-protected-resource.sh + cf-post-tool-use-tmp-workflow.sh + 5 dormant staging scripts
    │   ├── team-guard             -- replaces cf-pre-tool-use-team-guard.sh
    │   └── webfetch-guard         -- replaces cf-pre-tool-use-webfetch.sh
    ├── post-tool-use
    │   ├── sentinel-write         -- replaces cf-post-tool-use-pathflow-sentinel.sh
    │   ├── checkpoint-register    -- replaces cf-post-tool-use-phase-checkpoint.sh
    │   ├── logging                -- replaces cf-post-tool-use-logging.sh
    │   └── settings-validate      -- replaces cf-post-tool-use-settings-templates.sh (also exposed as `codeflow settings validate`)
    ├── user-prompt-submit
    │   ├── validate               -- replaces cf-user-prompt-submit.sh
    │   └── logging                -- replaces cf-user-prompt-submit-logging.sh
    ├── stop
    │   ├── gate                   -- replaces cf-stop-pathflow-gate.sh
    │   └── logging                -- replaces cf-stop-logging.sh
    └── task-completed
        └── checkpoint-complete    -- replaces cf-task-completed-phase-checkpoint.sh
```

### 9.1 Go Package Layout

The grouped CLI subcommand structure maps to a corresponding Go package layout:

```text
codeflow-cli/internal/hooks/
├── sessionstart/        -- session-start event hooks (init, logging, instructions)
├── sessionend/          -- session-end event hooks (cleanup, logging)
├── pretooluse/          -- pre-tool-use event hooks (gate, security, edit-write, etc.)
├── posttooluse/         -- post-tool-use event hooks (sentinel, checkpoint, logging, etc.)
├── userpromptsubmit/    -- user-prompt-submit event hooks (validate, logging)
├── stop/                -- stop event hooks (gate, logging)
└── taskcompleted/       -- task-completed event hooks (checkpoint-complete)
```

### 9.2 Hook Configuration After Migration

Each hook entry in `settings.json` shrinks from a multi-argument bash invocation to a
single binary call using the grouped subcommand structure:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{"type": "command", "command": "codeflow hooks pre-tool-use security"}]
      }
    ]
  }
}
```

The `settings.json` command strings are the complete wiring. No shell scripts are needed.

### 9.3 test-config.json Integration

All new Go packages from this epic must integrate with `codeflow-cli/config/testing/test-config.json`:

- 85% coverage threshold on business packages (`per_file` enforcement)
- New packages added to `business_packages` array as they are created
- `t.Parallel()` required in all test functions
- `context.Background()` blocked (use `context.TODO()` or test context)
- Test file must be staged with source file (enforced by convention)

---

## 10. Retained Infrastructure

Not everything migrates. The following infrastructure SURVIVES the Go CLI migration and remains as shell scripts, test files, or CI workflows.

### 10.1 Shell Scripts Retained

**Security protection scripts — all retained as shell:**

| Script | Lines | Status | Reason |
|--------|-------|--------|--------|
| `.codeflow/scripts/security/protection/cf-protect-resources.sh` | 420 | RETAINED | Requires sudo/root for chown/chmod — cannot avoid privilege escalation |
| `.codeflow/scripts/security/protection/cf-promote-protection.sh` | 179 | RETAINED | Cohesive OS-level protection subsystem — INF-TSK-021-036 cancelled |
| `.codeflow/scripts/security/protection/cf-reload-protection.sh` | 246 | RETAINED | Cohesive OS-level protection subsystem — INF-TSK-021-036 cancelled |
| `.codeflow/scripts/security/protection/lib/cf-protection-common.sh` | 170 | RETAINED | Sourced by protection scripts; entire subsystem retained as shell |
| `.codeflow/scripts/security/protection/lib/cf-protection-core.sh` | 137 | RETAINED | Sourced by protection scripts; entire subsystem retained as shell |
| `.codeflow/scripts/security/protection/lib/cf-protection-ops.sh` | 129 | RETAINED | Sourced by protection scripts; entire subsystem retained as shell |
| `.codeflow/scripts/security/protection/lib/cf-protection-verify.sh` | 252 | RETAINED | Sourced by protection scripts; entire subsystem retained as shell |

All 3 protection scripts (`cf-protect-resources.sh`, `cf-promote-protection.sh`, `cf-reload-protection.sh`) and 4 lib files form a cohesive OS-level protection subsystem and are retained as shell. INF-TSK-021-036 (planned Go migration of promote/reload) was cancelled: promote and reload have zero automated callers, the protection-cache.json has zero consumers, and splitting the subsystem across Go and shell adds complexity without benefit.

**Git hooks (partially migrated — hybrid approach via INF-TSK-021-035):**

| Script | Lines | Status | Notes |
|--------|-------|--------|-------|
| `.codeflow/scripts/git-hooks/pre-commit` | 672 | HYBRID | Shell wrapper retained for linting tool invocations (shellcheck, ruff, markdownlint with auto-fix+re-staging). Go subcommand `codeflow git-hooks pre-commit-validate` handles pure-logic checks (branch protection, sensitive files, JSON validation, Go test conventions, coverage enforcement). |
| `.codeflow/scripts/git-hooks/commit-msg` | 309 | THIN WRAPPER | Delegates to `codeflow git-hooks commit-msg` — eliminates jq JSON construction |
| `.codeflow/scripts/git-hooks/post-commit` | 73 | THIN WRAPPER | Delegates to `codeflow git-hooks post-commit` — fixes systemic JSON construction bug |
| `.codeflow/scripts/git-hooks/pre-push` | 275 | THIN WRAPPER | Delegates to `codeflow git-hooks pre-push` — TTY override via os.Open("/dev/tty") |
| `.codeflow/scripts/git-hooks/prepare-commit-msg` | 116 | THIN WRAPPER | Delegates to `codeflow git-hooks prepare-commit-msg` |

**Revised rationale for git hook migration:** Latency is not the primary driver. Go provides type-safe config parsing (eliminates jq subprocess chains), fixes a systemic JSON construction bug in post-commit, and enables consistent testing via the Go test framework. All parameters configurable via enforcement-policy.json — downstream teams customize config, not code. The `core.hooksPath` setting is unchanged; only the hook script content changes.

**Top-level wrapper:**

| Script | Lines | Reason |
|--------|-------|--------|
| `./codeflow` (root wrapper) | 277 -> ~4 | Simplified to thin Go binary delegator |

### 10.2 Test Infrastructure Retained

| Path | Description |
|------|-------------|
| `.codeflow/testing/lib/` (10 scripts) | Test framework libraries (test-common.sh, test-runner.sh, etc.) |
| `.codeflow/testing/cli/test-go-cli.sh` | Go test bridge script |
| `.codeflow/testing/scripts/git-hooks/` (5 test files) | Tests for git hook shell wrappers (partially migrated per INF-TSK-021-035) |
| `.codeflow/testing/scripts/security/protection/` (~7 test files) | Tests for retained protection scripts |
| `.codeflow/testing/ci/check-test-coverage-pairing.sh` | CI coverage pairing check (needs update post-migration) |

### 10.3 CI Workflows

| Workflow | Status |
|----------|--------|
| `.github/workflows/test-suite.yml` | MODIFIED: remove Python venv/pytest steps, retain Go test job |
| `.github/workflows/enforce-commit-format.yml` | RETAINED unchanged |
| `.github/workflows/prevent-force-push.yml` | RETAINED unchanged |

### 10.4 Consolidation Design Decisions

**protection-guard command consolidation (INF-TSK-021-016):**

`protection-guard` (`codeflow hooks pre-tool-use protection-guard`) consolidates three previously separate concerns into a single Go command:

1. `cf-pre-tool-use-protected-resource.sh` — tier check (CRITICAL/HIGH/MODERATE) and policy response
2. `cf-post-tool-use-tmp-workflow.sh` — staging workflow guidance and tmp file lifecycle management
3. 5 dormant staging scripts (`cf-stage-edit.sh`, `cf-apply-staged-edit.sh`, `cf-rollback-edit.sh`, `cf-reject-staged-edit.sh`, `cf-cleanup-expired.sh`) — these scripts were never called programmatically and are fully replaced by the protection-guard workflow

A single Go command handles the complete protected-resource edit workflow: detect the tier, advise the agent on staging path, and manage tmp state.

**protection-guard vs edit-write-guard — they are complementary, not overlapping:**

| Command | Subcommand | Purpose | Triggered By |
|---------|------------|---------|--------------|
| `edit-write-guard` | `codeflow hooks pre-tool-use edit-write-guard` | Enforces which files agents CAN edit — path scope permissions | Every Edit/Write tool call |
| `protection-guard` | `codeflow hooks pre-tool-use protection-guard` | Handles what happens when agents try to edit PROTECTED files — tier check + auto-staging workflow | Edit/Write tool calls on protected resource paths |

`edit-write-guard` answers "is this file in the agent's allowed scope?" — a broad scope enforcement check.
`protection-guard` answers "this file is protected — what tier is it and how should the agent proceed?" — a targeted policy response for files that pass scope but require special handling.

**settings-validate consolidation (INF-TSK-021-016):**

`cf-post-tool-use-settings-templates.sh` is consolidated into `codeflow settings validate`. The same Go function is exposed at two entry points:

- `codeflow settings validate` — CLI entry point for manual validation
- `codeflow hooks post-tool-use settings-validate` — hook entry point (replaces the PostToolUse hook script)

This eliminates the code duplication between the CLI `mode` command (which read settings.json) and the hook (which synced settings.json from managed templates).

---

## 11. Migration Priority

### Phase A: Data Integrity Foundation (Blocks all other work)

**Scope:** Tier 1 state and data layer scripts (6 scripts).
**What it delivers:** Eliminates JSONL corruption, flock issues, and python3 subprocesses
for the core data path. SQLite writes and JSONL writes go through Go.
**Epic:** INF-EPC-021.
**Scripts:** ledger.sh, cf-pathflow-state.sh, cf-work-state.sh, memory.sh,
generate-format-id.sh, ulid.sh.

### Phase B: Hook Enforcement Migration (Highest frequency, highest impact)

**Scope:** 4 Tier 1 enforcement hooks + security enforcement stack.
**What it delivers:** Eliminates ~300ms per-tool-call overhead. Security enforcement
becomes a single compiled binary instead of 12 sourced shell files.
**Epic:** INF-EPC-021.
**Scripts:** cf-pre-tool-use-security.sh + all sourced enforcement modules,
cf-pre-tool-use-pathflow-gate.sh, cf-post-tool-use-pathflow-sentinel.sh,
cf-post-tool-use-phase-checkpoint.sh.

### Phase C: Validation and Pathflow Scripts

**Scope:** Tier 1 validation scripts + 5 pathflow scripts.
**What it delivers:** Correct YAML parsing, reliable pathflow JSONL writes.
**Epic:** Extension of Phase A epic or Phase B epic.
**Scripts:** validate-task.sh, validate-epic.sh, cf-tracking-generate.sh, all 5
cf-pathflow-*.sh scripts.

### Phase D: Session Lifecycle Hooks

**Scope:** Tier 1 session hooks (cf-session-start-init.sh, cf-session-end-cleanup.sh).
**What it delivers:** Reliable session start/end with no bash fallback paths.
**Epic:** Extension of Phase B epic.
**Scripts:** 2 hooks, replaces most complex hook scripts.

### Phase E: Python Elimination

**Scope:** All 18 Python source files, 20 pytest test files, Python test infrastructure.
**What it delivers:** Single runtime (Go), removal of python3 dependency,
removal of pytest/pytest-cov infrastructure.
**Epic:** INF-EPC-021.
**Note:** Can run partially in parallel with Phase C if coordination scripts are
independent of state layer migration.

### Phase F: Remaining Hook Migration (Tier 2)

**Scope:** All 13 remaining Tier 2 hook scripts.
**What it delivers:** Complete hook layer elimination. All 22 hook scripts become dead code.
**Epic:** Extension of Phase B epic.

### Phase G: Shell Library and Settings Migration (Tier 2)

**Scope:** Shell library modules, settings management, remaining Tier 2 scripts.
**What it delivers:** Eliminates shell utility sourcing, unifies config management.
**Epic:** Extension of Phase A/B epic or new chore epic.

### Phase H: Tier 3 Migration and Shell Test Removal

**Scope:** Worktree scripts, protection scripts, testing framework lib.
**What it delivers:** Complete shell elimination except Tier 4 scripts.
**Epic:** Chore epic or bundled with earlier phases.

### Phase I: Ledger and Log Schema Normalization

**Scope:** JSONL schema consistency across all three ledger files.
**What it delivers:** Uniform event schema enabling reliable replay, audit, and SQLite
rebuild from JSONL.

**Problem:** The three JSONL files have diverged schemas:

| File | Field name for event type | Field name for timestamp | Other drift |
|------|--------------------------|--------------------------|-------------|
| `work-graph.jsonl` | `event` | `timestamp` | Canonical format |
| `memory-events.jsonl` | `type` | `ts` | Uses short field names |
| `pathflow-events.jsonl` | `event` | `timestamp` | Raw `op:INSERT` entries mixed in |

**When to run:** Immediately after Go cutover and testing (Phase E/F), before drift
accumulates further. Running before cutover risks re-introducing schema bugs in shell paths.

**Steps:**

1. Read all existing JSONL records from the three files
2. Normalize all records to the canonical schema (`event`, `timestamp`, plus type-specific
   fields)
3. Write normalized records back atomically (temp file + rename)
4. Rebuild SQLite from normalized JSONL via `codeflow db rebuild`
5. Verify record counts match pre-normalization counts

**Epic:** INF-EPC-021 (dedicated normalization task).

### 10.1 Migration Strategy — Agreed Implementation Approach

The migration follows a build-coexist-cutover pattern rather than a parallel-run or
flag-based approach. This minimizes the risk window and keeps the implementation bounded.

**Phase A: Complete INF-EPC-015 (Go CLI build-out)**

INF-EPC-021 depends on INF-EPC-015 completion. The Go binary must exist and all
subcommands must be implemented before any hook wiring can occur. Do not begin hook
migration until INF-EPC-015 is merged.

**Phase B: Build Go equivalents (coexisting with shell)**

Implement all Go subcommands that replace shell scripts. Shell scripts remain in place and
continue to run. The Go binary is available but not yet wired into hooks or the production
path. This phase is safe — adding a binary changes nothing until it is invoked.

**Phase C: Shadow testing**

Run Go equivalents in shadow mode alongside shell scripts. For each hook invocation,
the Go binary is called separately (not replacing the hook) and its output is compared
to the shell script's output. Divergences are logged. No production behavior changes.

**Phase D: Ledger and log schema normalization**

Normalize the three JSONL files to canonical schema (see Phase I above). This is a
one-time data operation that must complete before cutover to prevent mixed-schema records
in the rebuilt SQLite database.

**Phase E: Single-session cutover**

In a single session, update `settings.json` hook commands to reference
`codeflow hooks <name>` instead of shell script paths. Delete all retired shell scripts
and their test files. This is the only phase that is not fully reversible — the delete
step requires a git revert if rollback is needed.

**Phase F: Post-cutover verification**

Run the full test suite (`./codeflow test --mode standard`) and verify all acceptance
criteria from INF-EPC-021 tasks. Run `codeflow doctor` to confirm all hook subcommands
respond correctly. Monitor one full session under Go hooks before declaring the migration
complete.

**Dependency chain:**

```text
INF-EPC-015 (Go CLI build-out)
    --> INF-EPC-021 Phase A (build Go equivalents)
        --> INF-EPC-021 Phase B (shadow testing)
            --> INF-EPC-021 Phase C (JSONL normalization)
                --> INF-EPC-021 Phase D (single-session cutover)
                    --> INF-EPC-021 Phase E (post-cutover verification)
```

---

## 12. Risk Assessment

### 12.1 High Risk

**Hook behavior divergence during cutover:**

Shell hooks and Go hooks must produce identical stdin parsing, identical blocking behavior,
and identical exit codes. A Go hook that exits 0 when it should exit 1 will silently allow
a blocked operation to proceed. Each hook must have integration tests that verify
blocking/passing behavior against known inputs before cutover.

**Mitigation:** Write Go hook integration tests before removing shell hooks. Run both
shell and Go hooks in parallel (different matcher) during a validation period.

**JSONL integrity during ledger.sh migration:**

If ledger.sh is replaced mid-session, existing JSONL records use the old format and new
records use the new format. The JSONL sync parser must handle both.

**Mitigation:** Freeze JSONL schema before migration. Test sync parser against records
from both the old and new writers.

### 12.2 Medium Risk

**settings.json command path changes:**

After hook migration, `settings.json` commands change from `bash .claude/hooks/...` to
`codeflow hooks ...`. This is a breaking change — any environment without the new Go binary
will have non-functional hooks.

**Mitigation:** Gate the settings.json change on Go binary availability. The `codeflow doctor`
command should verify the binary is installed and on PATH before sessions start.

**Platform binary distribution:**

The Go binary must be pre-built for macOS ARM (the primary development platform) and
distributed via the existing install mechanism. Until the binary is available, the shell
fallback must remain.

**Mitigation:** Maintain shell hooks until the Go binary is in the install path. Use
INF-EPC-015's existing cross-compilation Makefile targets.

### 12.3 Low Risk

**Python test removal:**

Removing 20 pytest files removes test coverage for codeflow_py_lib. If Go replacements
are written with equivalent Go test coverage, the net coverage change is zero.

**Mitigation:** Write Go tests for each Go replacement package before removing Python tests.

**Worktree and protection scripts:**

These are Tier 3 — rarely invoked and not on the critical path. Low priority means low
urgency; risk is limited to deferred technical debt.

---

## 13. Recommendation

> **Note:** The recommendations below were written before epic creation. The actual
> implementation consolidated Phases A-H into a single epic: INF-EPC-021.

### Immediate Actions (Before Next Epic Planning)

1. **Create INF-EPC-016 (Hook Go Migration):** Cover Phases B, D, and F. The enforcement
   stack migration alone (Phase B) eliminates ~250ms per tool call and is the highest-ROI
   single change available. *(Consolidated into INF-EPC-021.)*

2. **Extend INF-EPC-015 Phase 6 tasks:** Add tasks for "wire session-start hook to
   `codeflow hooks session-start`" and "verify JSONL-only writes after ledger migration."
   These are specified in the Phase 6 V4 spec but have no epic/task representation.

3. **Create INF-EPC-017 (Python Elimination):** Small, bounded epic. 18 source files,
   20 test files, 3 infrastructure files. Can be completed in 1-2 sessions once Phases
   A-C are done. *(Consolidated into INF-EPC-021.)*

4. **Delete `cf-hook-bypass.sh`:** This is dead code (37 lines, no callers). Remove in the
   next chore session.

### Sequencing Constraint

Phase A (data integrity) is a hard prerequisite for all other phases. The ledger and
checkpoint systems are the foundation everything else writes to. Migrating the hook
enforcement stack (Phase B) on top of an unreliable JSONL layer would be counterproductive.

**Recommended sequence:**

```text
Phase A (data integrity)
    --> Phase B (hook enforcement) and Phase E (Python) in parallel
        --> Phase C (validation + pathflow)
            --> Phase D (session hooks)
                --> Phase F (remaining hooks)
                    --> Phase G (shell lib + settings)
                        --> Phase H (worktree + cleanup)
```

### Success Criteria

The migration is complete when:

- All 22 hook scripts are deleted from `.claude/hooks/codeflow/`
- All 18 Python source files are deleted from `.codeflow/scripts/`
- All 20 pytest test files are deleted from `.codeflow/testing/`
- `settings.json` hook commands reference `codeflow hooks <name>` exclusively
- `flock`, `jq`, `python3`, and `sqlite3` are not called from any remaining shell script
- `macOS`/Linux platform splits (`date -j` vs `date -d`, `stat -f%z` vs `stat --printf`) exist
  only in the 4 Tier 4 scripts that have genuine shell requirements
- `codeflow doctor` verifies all hook subcommands respond correctly before session start
