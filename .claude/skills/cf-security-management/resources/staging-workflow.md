# Staging Workflow Reference

## Overview

Protected files cannot be edited directly. Changes must go through a staging workflow:

1. Stage edits to temporary location
2. User reviews changes
3. Apply with backup on approval

## Workflow Steps

### Step 1: Validate Protection

```text
cf-security-management:validate-protected-resource
→ Returns: protected=true, tier=core, workflow=staging
```

### Step 2: Stage Edit

The staging system creates:

```text
/tmp/claude/managed/protected-edits/
├── {filename}.staged          # Modified version
├── {filename}.original        # Backup of original
└── {filename}.metadata.json   # Edit metadata
```

Staging is handled by `.codeflow/scripts/security/stage-edit.sh`.

### Step 3: User Review

System presents:

- File path and protection tier
- Diff between original and staged
- Approve/Reject options

### Step 4: Apply or Reject

#### On Approve

`.codeflow/scripts/security/apply-staged-edit.sh`:

1. Creates timestamped backup
2. Validates staged file (syntax check)
3. Applies changes
4. Preserves permissions
5. Logs to security_log
6. Cleans up staged files

#### On Reject

`.codeflow/scripts/security/reject-staged-edit.sh`:

1. Logs rejection
2. Cleans up staged files
3. Notifies agent

## Validation by File Type

| Type | Validation Script |
|------|-------------------|
| .sh | `.codeflow/scripts/security/validate-shell.sh` |
| .py | `.codeflow/scripts/security/validate-python.sh` |
| .json | `.codeflow/scripts/security/validate-json.sh` |
| .yaml/.yml | `.codeflow/scripts/security/validate-yaml.sh` |

## Edge Cases

### Staged Edit Expires

- TTL: 1 hour
- On expiry: Auto-cleanup via `.codeflow/scripts/security/cleanup-expired.sh`
- Must re-stage if needed

### Concurrent Edits

- Only one staged edit per file allowed
- Second attempt blocked until first resolved

### Original Modified During Staging

- Detected via checksum comparison
- Apply blocked, must re-stage with fresh original

## Rollback

If issues discovered after apply:

```bash
.codeflow/scripts/security/rollback-edit.sh "{path}"
```

This script:

1. Finds most recent backup
2. Restores from backup
3. Logs rollback to security_log

## Security Logging

All staging operations logged to security_log via `cf-db-operations:log-append`:

- `protected_edit_staged`
- `protected_edit_approved`
- `protected_edit_rejected`
- `protected_edit_rollback`
