# Agent Permissions Reference

## Permission Model

Permissions are enforced by cf-db-operations at the operation level. Agents cannot bypass permissions by accessing the database directly.

## Epic Permissions

| Operation | cf-planner | cf-developer | cf-reviewer | cf-qa | Main Agent |
|-----------|------------|--------------|-------------|-------|------------|
| epic-create | Yes | No | No | No | No |
| epic-update | Yes | No | No | No | No |
| epic-read | Yes | Yes | Yes | Yes | Yes |

Only cf-planner can create or modify epics.

## Task Permissions

| Field | cf-planner | cf-developer | cf-reviewer | cf-qa | Main Agent |
|-------|------------|--------------|-------------|-------|------------|
| Create (formal) | Yes | No | No | No | No |
| Create (informal) | No | No | No | No | Yes |
| status | Yes | Yes | Yes | Yes | No |
| assignee_id | Yes | No | No | No | No |
| priority | Yes | No | No | No | No |
| estimate | Yes | No | No | No | No |
| file_scope | Yes | Yes* | No | No | No |
| branch | Yes | Yes | No | No | No |
| pr_number | Yes | Yes | Yes | No | No |
| autorun_eligible | Yes | No | No | No | No |
| auto_commit | Yes | No | No | No | No |
| auto_merge | Yes | No | No | No | No |
| target_branch | Yes | No | No | No | No |
| acceptance | Yes | No | No | No | No |
| tests | Yes | No | No | No | No |

*cf-developer can only expand scope when scope_policy is 'soft' or 'permissive'

## Memory Permissions

| Operation | Rule |
|-----------|------|
| memory-store | Can only write to own domain |
| memory-query | All agents can read all domains |

### Domain Ownership

| Agent | Domain |
|-------|--------|
| cf-planner | planning |
| cf-developer | development |
| cf-reviewer | review |
| cf-qa | qa |
| cf-documenter | documentation |
| Main Agent | ops |

### Valid Domains

The following domains are valid for memory_events:

- `planning` - Planning and task breakdown
- `development` - Code implementation work
- `review` - Code review activities
- `qa` - Testing and quality assurance
- `ops` - Operational activities
- `documentation` - Documentation work

## Log Permissions

| Operation | Agents | System |
|-----------|--------|--------|
| log-append | No | Yes |
| log-read | Yes | Yes |

Only system hooks can append to logs. All agents can read logs.

## Autorun Permissions

| Operation | cf-planner | cf-developer | System |
|-----------|------------|--------------|--------|
| autorun-session-create | Yes | No | Yes |
| autorun-session-update | Yes | No | Yes |
| autorun-worker-create | No | No | Yes |
| autorun-worker-update | No | No | Yes |
| autorun-task-run-record | No | No | Yes |

Autorun sessions are primarily managed by the system (Go CLI orchestrator).

## Permission Enforcement

Permissions are enforced by:

1. **Operation validation** - Each db operation checks caller identity
2. **Field-level checks** - Update operations validate per-field permissions
3. **Domain ownership** - Memory writes validated against agent's domain
4. **Audit logging** - All violations logged to security_logs

## Validation Errors

| Error | Description |
|-------|-------------|
| PERM_001 | Agent not authorized for operation |
| PERM_002 | Cannot modify other agent's domain |
| PERM_003 | Scope expansion denied by policy |
| PERM_004 | Epic modification restricted to cf-planner |
| PERM_005 | Task field modification not allowed |
| PERM_006 | Autorun configuration requires cf-planner |

## Escalation

When permission denied:

1. Error returned with escalation hint
2. Attempt logged in security_logs
3. For critical ops, suggests user intervention

### Escalation Paths

| From | To | Reason |
|------|-----|--------|
| cf-developer | cf-planner | Protected edits, epic changes, priority changes |
| cf-developer | cf-planner | Autorun configuration changes |
| cf-reviewer | cf-planner | Task reassignment |
| Any Agent | User | Unknown network domains, protected file approval |

## Status Transitions

### Epic Status (cf-planner only)

```text
draft → planning → in_progress → blocked ↔ in_progress → complete → archived
```

### Task Status

| Transition | Allowed Agents |
|------------|----------------|
| todo → in_progress | cf-planner, cf-developer |
| in_progress → blocked | cf-planner, cf-developer, cf-reviewer |
| blocked → in_progress | cf-planner, cf-developer |
| in_progress → complete | cf-planner, cf-developer, cf-reviewer, cf-qa |
| complete → todo | cf-planner only (reopen) |
