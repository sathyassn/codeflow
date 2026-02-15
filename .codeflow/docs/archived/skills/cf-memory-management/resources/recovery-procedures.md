# Recovery Procedures Reference

## Context Recovery Scenarios

### Scenario 1: Session Interrupted Mid-Work

**Symptoms:**

- active_work exists with status='in_progress'
- No session_end event recorded
- Recent progress events exist

**Recovery:**

```text
cf-memory-management:detect-active-work
→ Finds interrupted work
→ Presents resume options
```

**User Options:**

1. Resume: `cf-memory-management:load-work-context` restores context
2. Abandon: Set status='paused', archive context

### Scenario 2: Stale Work Detected

**Symptoms:**

- active_work with last_progress_at > 24 hours ago
- No recent memory events

**Recovery:**

```text
cf-memory-management:detect-active-work
→ Flags stale work
→ Checks git for recent commits on branch
```

**User Options:**

1. Resume with sync from git
2. Mark as paused
3. Cleanup (if truly abandoned)

### Scenario 3: Orphaned Work (Task Deleted)

**Symptoms:**

- active_work exists but task_id invalid

**Recovery:**
Use `.codeflow/scripts/db/repair-orphans.sh`:

- Checks JSONL ledger for task history
- Recreates task record if found in ledger
- Archives work if truly orphaned

### Scenario 4: Branch Conflict

**Symptoms:**

- active_work references branch that no longer exists

**Recovery:**
Use `.codeflow/scripts/git/recover-branch.sh`:

1. Checks remote for branch
2. Checks reflog for recovery
3. Creates new branch if unrecoverable, migrates context

## JSONL Ledger Recovery

The JSONL ledger is the source of truth. If SQLite is corrupted:

```bash
.codeflow/scripts/db/rebuild-from-ledger.sh
```

This script:

1. Reads all events from JSONL ledger
2. Rebuilds SQLite tables in correct order
3. Verifies consistency

## Emergency Procedures

### Complete Data Loss

1. Check `.claude/memory/archive/` for backups
2. Check git history for committed state files
3. Run rebuild from JSONL if available
4. Start fresh if no recovery possible

### Corrupted Database

```bash
.codeflow/scripts/db/repair-database.sh
```

This script:

1. Backs up current state
2. Runs integrity check
3. Attempts recovery
4. Rebuilds from JSONL if recovery fails

### Memory Event Gaps

If memory events are missing:

```bash
.codeflow/scripts/db/reconstruct-events.sh
```

This script:

1. Checks git log for commits in timeframe
2. Checks file modification times
3. Creates synthetic events with `source: 'recovery'`

## Prevention

- JSONL ledger is append-only, never modified
- Regular backups via `cf-memory-management:manage-memory-lifecycle`
- SQLite WAL mode for crash recovery
