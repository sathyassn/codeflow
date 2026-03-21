---
description: "Clean up sessions, worktrees, and temporary files"
argument-hint: "[--sessions] [--worktrees] [--all] [--dry-run]"
---

# /cf-cleanup Command

## Working Protocol

**Skill:** `.claude/skills/cf-working-protocol/SKILL.md`

Apply cognitive operations throughout execution:

- 🔧 meta-awareness: Continuous state and context awareness
- 🔧 think-and-act: Before tool calls and teammate dispatch (PAC-5 for destructive operations)
- 🔧 decide: At decision points (cleanup scope, safety checks, what to preserve)
- 🔧 respond-organized: When presenting cleanup results and summaries

**Note:** cf-working-protocol loaded at SessionStart, applies to all execution.

---

## 1. Purpose & Usage

**Purpose:** Clean up stale sessions, merged worktrees, old logs, and PathFlow state by dispatching cleanup work to cf-git-operations, with user confirmation before destructive operations.

**Usage:**

```text
/cf-cleanup [--sessions] [--worktrees] [--all] [--dry-run]
```

**Use When:**

- After merging a PR and completing a PathFlow session (PF7-END)
- Worktrees from merged PRs are no longer needed
- Stale session state needs cleanup (completed or orphaned sessions)
- Periodic maintenance of project state between sessions

**Do Not Use When:**

- Active PathFlow session is in progress (wait for PF7-END or use `--dry-run`)
- Worktrees have uncommitted changes (commit or stash first)
- Cleaning up code or refactoring (use `/cf-develop`)
- Checking session status (use `/cf-stack`)

### Pipeline Position

```text
Phase: PF7-END
Pipeline: /cf-ship --> /cf-cleanup
                        ^ you are here
Previous: /cf-ship (PR merged)
Next: Session ends. Start new session for new work.
```

---

## 2. Arguments & Flags

**Arguments:**

No positional arguments. Cleanup scope is controlled entirely by flags.

**Flags:**

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--sessions` | `-s` | Clean up completed and stale session state | false |
| `--worktrees` | `-w` | Remove worktrees for merged branches | false |
| `--all` | `-a` | Clean up all categories (sessions + worktrees + logs + state) | false |
| `--dry-run` | `-d` | Show what would be cleaned without executing | false |

**Behavior When No Flags:**

- Equivalent to `--all --dry-run`: show everything that can be cleaned without executing

**Cleanup Categories:**

| Category | What Gets Cleaned | Flag |
|----------|------------------|------|
| Sessions | Completed/stale entries in `.state/runtime/`, `.state/sentinels/` | `--sessions` |
| Worktrees | `.git-worktrees/` entries for merged branches | `--worktrees` |
| Logs | Old log files beyond retention period | `--all` |
| PathFlow state | Stale sentinels, orphaned flags, expired runtime files | `--all` |

**Examples:**

```bash
# Dry run: show what can be cleaned (default)
/cf-cleanup

# Clean up worktrees only
/cf-cleanup --worktrees

# Clean up sessions only
/cf-cleanup --sessions

# Full cleanup of everything
/cf-cleanup --all

# Preview full cleanup without executing
/cf-cleanup --all --dry-run
```

---

## 3. Prerequisites

**Required State:**

- [ ] No active PathFlow session (PF7-END complete or no session started)
  - Exception: `--dry-run` can run during active sessions for preview
- [ ] cf-git-operations teammate available (for worktree cleanup)
- [ ] cf-knowledge-layer teammate available (for session state queries)

**Stage Availability:**

- PF7-END (standard flow: final cleanup as last step before team dissolution)
- No active PathFlow (maintenance cleanup between sessions)
- Active session with `--dry-run` only (preview, no modifications)

**Required Infrastructure:**

| Component | Purpose |
|-----------|---------|
| cf-git-operations | Worktree removal, branch cleanup, git prune |
| cf-knowledge-layer | Session state queries, stale session identification |

---

## 4. Workflow Definition

### 4.1 Workflow Diagram

```text
Phase: PF7-END | Type: Session Cleanup

/cf-cleanup invoked
    |
    v
Parse flags (--sessions, --worktrees, --all, --dry-run)
    |
    v
No flags? ---YES---> Default to --all --dry-run
    |
    NO
    |
    v
Active PathFlow? ---YES---> --dry-run only?
    |                           |
    NO                          v
    |                   YES --> PROCEED (preview only)
    |                   NO ---> ERROR: "Active session"
    |                           "Complete PF7-END first"
    v
Analyze cleanup targets --------+-------------------+
    |                           |                   |
    v                           v                   v
  SESSIONS                  WORKTREES           LOGS/STATE
    |                           |                   |
    v                           v                   v
Query completed/          List worktrees        Scan for stale
stale sessions            for merged PRs        sentinels, flags,
from DB                       |                 old logs
[cf-knowledge-layer]     [cf-git-operations]
    |                         |                     |
    +----------+--------------+---------------------+
               |
               v
Is --dry-run? ---YES---> Report summary, STOP
               |
               NO
               |
               v
Confirm with user
(show items to delete)
    |
    v
User confirms? ---NO---> CANCELLED
    |
    YES
    |
    v
Execute cleanup                                     [cf-git-operations]
(delegate to teammates)                             [cf-knowledge-layer]
    |
    v
Report results
    |
    v
Next: Session ends. Start new session for new work.
```

### 4.2 Execution Steps

**Step 1: Parse Flags**

- Parse flags: `--sessions`, `--worktrees`, `--all`, `--dry-run`
- If no flags: default to `--all --dry-run`
- If `--all`: enable all categories (sessions, worktrees, logs, state)

**Step 2: Check Active Session**

- Check for active PathFlow session:
  - Flag file: `/tmp/claude/managed/state/pathflow-active`
  - Or query cf-knowledge-layer: `"LEAD: check-active-session"`
- If active session and not `--dry-run`: block with error
- If active session and `--dry-run`: allow preview only

**Step 3: Analyze Cleanup Targets**

- **Sessions (if `--sessions` or `--all`):**
  - Send to cf-knowledge-layer:
    - `"LEAD: query-stale-sessions -- status=completed,abandoned,orphaned"`
  - Identify: completed sessions older than retention period, orphaned sessions without end events
  - Collect: session IDs, branches, ages

- **Worktrees (if `--worktrees` or `--all`):**
  - Use `codeflow worktree cleanup --dry-run` to preview stale worktrees
  - Use `codeflow worktree prune --dry-run` to preview registry/filesystem inconsistencies
  - Send to cf-git-operations:
    - `"List all worktrees and identify which have merged branches"`
  - Identify: worktrees whose branches have been merged to main
  - Safety check: skip worktrees with uncommitted changes or unpushed commits

- **Logs and State (if `--all`):**
  - Scan for stale PathFlow state:
    - Sentinels in `.state/sentinels/` without active sessions
    - Orphaned `pathflow-active` flag in `/tmp/claude/managed/state/`
    - Stale runtime files in `.state/runtime/`
  - Scan for old logs beyond retention period

**Step 4: Dry Run Report (if `--dry-run`)**

- Present cleanup summary grouped by category:
  - Sessions: count, session IDs, ages
  - Worktrees: paths, branches, merge status
  - Logs/State: file counts, total size
- Stop execution after report

**Step 5: Confirm with User**

- Present cleanup summary showing exactly what will be removed
- Require explicit confirmation before proceeding
- If user declines: cancel with no changes made

**Step 6: Execute Cleanup**

- **Sessions:**
  - Send to cf-knowledge-layer:
    - `"LEAD: cleanup-sessions -- ids=[{session-ids}]"`
  - Archives session records, removes runtime state

- **Worktrees:**
  - Run `codeflow worktree cleanup` to remove stale worktrees (or `--force` for orphaned too)
  - Run `codeflow worktree prune` to reconcile registry with filesystem
  - Send to cf-git-operations for branch-level cleanup:
    - `"Clean up worktrees: [{paths}]. Remove merged branches."`
  - cf-git-operations executes cleanup-worktrees SOP:
    - `git worktree remove {path}` for each clean worktree
    - `git worktree prune` for stale references
    - `git branch -d {branch}` for merged branches

- **Logs and State:**
  - Send to cf-knowledge-layer:
    - `"LEAD: cleanup-state -- sentinels=stale, flags=orphaned, logs=old"`
  - Removes stale sentinels, orphaned flags, old log entries

**Step 7: Report Results**

- Show cleanup results grouped by category:
  - Sessions: count cleaned, count skipped (with reasons)
  - Worktrees: count removed, count skipped (uncommitted changes)
  - Logs/State: files removed, space reclaimed
- Show any items that could not be cleaned (with reasons)

---

## 5. Skills Integration

| Teammate/Skill | Operation | Purpose |
|----------------|-----------|---------|
| cf-working-protocol | think-and-act, decide | Cognitive procedures throughout |
| cf-git-operations | cleanup-worktrees | Remove stale worktrees and prune git refs |
| cf-git-operations | check-branch-status | Identify merged vs unmerged branches |
| cf-git-operations | review-changes | Verify no uncommitted work in worktrees |
| cf-knowledge-layer | query-stale-sessions | Identify completed/orphaned sessions |
| cf-knowledge-layer | cleanup-sessions | Archive and remove session records |
| cf-knowledge-layer | cleanup-state | Remove stale sentinels and runtime flags |

---

## 6. Hooks Integration

| Hook | When | Purpose |
|------|------|---------|
| SessionStart | Session start | Load cf-working-protocol |
| PreToolUse:pathflow-gate | Before tool calls | Verify PathFlow phase consistency |
| PreToolUse:team-guard | Before TeamDelete | Prevent team dissolution while pathflow-active |
| PostToolUse:logging | After tool calls | Log cleanup operations |
| Stop:pathflow-gate | Session stop | Verify work state consistency |

**PathFlow Gate Interaction:**

`/cf-cleanup` operates at PF7-END or outside an active PathFlow session. The pathflow-gate hook allows cleanup operations when:

- `pathflow-active` flag does not exist (no active session)
- Session is at PF7-END phase (final cleanup)
- `--dry-run` mode (read-only, no modifications)

The team-guard hook prevents `TeamDelete` while the pathflow-active flag exists. During PF7-END, the pathflow-active flag is removed as part of the cleanup sequence, which then allows team dissolution.

---

## 7. Memory Integration

### 7.1 Session Cleanup

**Stale Session Identification:**

- Sessions with status='completed' older than retention period
- Sessions with status='abandoned' (started but never completed)
- Orphaned sessions: runtime state exists but no matching DB record

**Cleanup Actions:**

- Archive session events to JSONL (preserves rebuild authority)
- Remove ephemeral runtime files (active-task.json, current-session-id)
- Update DB records (mark as archived)

### 7.2 PathFlow State Cleanup

- Remove stale sentinels from `.state/sentinels/`
- Remove orphaned `pathflow-active` flag from `/tmp/claude/managed/state/`
- Clean expired entries from `.state/runtime/`

### 7.3 Three-Tier Data Model

| Tier | Location | Purpose |
|------|----------|---------|
| 0 | `.state/ledger/` | Event log (NEVER deleted -- rebuild authority) |
| 1 | `.state/db/codeflow.db` | Active state (archived records marked, not deleted) |
| 2 | `.state/runtime/`, `.state/sentinels/` | Ephemeral state (removed during cleanup) |

**Important:** Tier 0 (JSONL) is the rebuild authority and is NEVER deleted during cleanup. Only ephemeral runtime state and stale DB records are cleaned. This ensures full session history is always recoverable.

---

## 8. Error Handling

| Error | Cause | Recovery |
|-------|-------|----------|
| Active PathFlow session | Cleanup during active work | Complete PF7-END first, or use `--dry-run` to preview |
| Worktree has uncommitted changes | Unsaved work in worktree | Commit or stash changes, then retry |
| Worktree has unpushed commits | Commits not synced to remote | Push commits via cf-git-operations, then retry |
| cf-git-operations not available | Teammate not spawned | Spawn cf-git-operations for worktree cleanup |
| Permission denied | File system permissions issue | Check file ownership, retry with appropriate permissions |
| User declined confirmation | Cleanup cancelled by user | No action taken; re-run to try again |

**Recovery Procedures:**

```text
ON "Active PathFlow session" error:
  1. Check session state: /cf-stack
  2. If work is complete: proceed through PF7-END
  3. If work is in progress: complete work first
  4. Alternatively: use --dry-run to preview cleanup

ON "Worktree has uncommitted changes":
  1. Navigate to worktree directory
  2. Commit changes via cf-git-operations or stash them
  3. Or: push changes, create PR, and merge
  4. Retry /cf-cleanup --worktrees

ON "Worktree has unpushed commits":
  1. Send to cf-git-operations: "Push commits in worktree {path}"
  2. After push succeeds: retry /cf-cleanup --worktrees
```

---

## 9. Examples

**Example 1: Preview All Cleanup (Default)**

```bash
/cf-cleanup
```

Output:

```text
DRY RUN: Analyzing cleanup targets

Sessions (2 candidates):
  - session-abc123 (completed, 3 days ago)
  - session-def456 (abandoned, 7 days ago)

Worktrees (1 candidate):
  - .git-worktrees/feat-oauth2/ (branch merged, clean)

PathFlow state (3 items):
  - .state/sentinels/pathflow-pf-3 (orphaned)
  - .state/runtime/active-task.json (stale)
  - /tmp/claude/managed/state/pathflow-active (orphaned)

Total: 2 sessions, 1 worktree, 3 state files
Run without --dry-run to execute cleanup.
```

**Example 2: Clean Up Worktrees Only**

```bash
/cf-cleanup --worktrees
```

Output:

```text
Worktree cleanup:

Candidates:
  - .git-worktrees/feat-oauth2/ (branch merged, clean) --> REMOVE
  - .git-worktrees/fix-validation/ (uncommitted changes) --> SKIP

Confirm removal of 1 worktree? [y/N]: y

Cleaned:
  - Removed: .git-worktrees/feat-oauth2/
  - Pruned: stale worktree references
  - Deleted branch: feat/oauth2-providers (merged)

Skipped:
  - .git-worktrees/fix-validation/ (has uncommitted changes)
```

**Example 3: Full Cleanup**

```bash
/cf-cleanup --all
```

Output:

```text
Full cleanup analysis:

Sessions (2):
  - session-abc123 (completed, 3 days ago) --> ARCHIVE
  - session-def456 (abandoned, 7 days ago) --> ARCHIVE

Worktrees (1):
  - .git-worktrees/feat-oauth2/ (merged, clean) --> REMOVE

PathFlow state (3):
  - .state/sentinels/pathflow-pf-3 (orphaned) --> REMOVE
  - .state/runtime/active-task.json (stale) --> REMOVE
  - /tmp/claude/managed/state/pathflow-active (orphaned) --> REMOVE

Confirm cleanup of 2 sessions, 1 worktree, 3 state files? [y/N]: y

Results:
  Sessions: 2 archived
  Worktrees: 1 removed, 0 skipped
  State: 3 files removed
  JSONL ledger: preserved (not modified)

Cleanup complete.
```

**Example 4: Cleanup Blocked by Active Session**

```bash
/cf-cleanup --all
```

Output:

```text
Cannot clean up: active PathFlow session detected.

Current session: session-ghi789 at PF4-EXECUTE
Active work: feat/search-feature

Options:
  1. Complete current work and proceed through PF7-END
  2. Use --dry-run to preview: /cf-cleanup --all --dry-run
  3. Check session state: /cf-stack
```

**Example 5: Sessions Cleanup Only**

```bash
/cf-cleanup --sessions
```

Output:

```text
Session cleanup:

Candidates:
  - session-abc123 (completed, 3 days ago) --> ARCHIVE
  - session-def456 (abandoned, 7 days ago) --> ARCHIVE
  - session-xyz789 (in progress, today) --> SKIP (active)

Confirm archival of 2 sessions? [y/N]: y

Cleaned:
  Sessions archived: 2
  Runtime files removed: 4
  Sentinels removed: 6

Skipped:
  - session-xyz789 (active session)
```

---

## 10. References

- [cf-git-operations agent](../agents/cf-git-operations.md)
- [cf-knowledge-layer agent](../agents/cf-knowledge-layer.md)
- [cf-working-protocol skill](../skills/cf-working-protocol/SKILL.md)
- [PathFlow configuration](../../.codeflow/config/pathflow/pathflow-config.json)
- [cf-ship command](cf-ship.md)
- [cf-stack command](cf-stack.md)
- [cf-doctor command](cf-doctor.md)
