# Security Tiers Reference

## Overview

CodeFlow security uses multiple protection tiers. Understanding these helps diagnose permission errors and choose the right workflow.

## Security Protection Tiers (SEC-*)

| Tier | Name | Mechanism | Bypassable? | Override |
|------|------|-----------|-------------|----------|
| **SEC-OS** | OS Foundation | File ownership + immutable flags | No | Requires sudo |
| **SEC-L0** | Enterprise Settings | System-level settings | No | Admin only |
| **SEC-L1** | Server-Side | GitHub Actions | No | N/A |
| **SEC-L2** | Blocking Hooks | PreToolUse/Stop hooks | No | Skill invocation |
| **SEC-L3** | Project Settings | .claude/settings.json | Yes | Agent can modify |
| **SEC-L4** | Git Hooks | Client-side hooks | Yes | --no-verify (blocked) |
| **SEC-L5** | Working Protocol | Documentation | Yes | Advisory only |

## Tier Details

### SEC-OS: OS Foundation

**Purpose:** Kernel-level protection that cannot be bypassed by any user-space process.

**Files Protected:**

```text
.claude/hooks/codeflow/            # All Claude hooks
.claude/settings.json              # Project settings
.claude/settings.local.json        # Local overrides
.codeflow/config/                  # Configuration files
.codeflow/scripts/security/        # Security scripts
```

**How to Edit:** Use `handle-protected-resource` staging workflow.

### SEC-L2: Blocking Hooks

**Purpose:** Client-side blocking enforcement via Claude Code hooks.

**Mechanisms:**

- **Sentinel files**: File-based tokens proving skill invocation
- **Regex validation**: Pattern matching for dangerous commands
- **Path checks**: Protected resource enforcement

**Exit Codes:**

| Code | Meaning | Effect |
|------|---------|--------|
| 0 | Success | Allow operation |
| 1 | Error | Log error, continue |
| 2 | Block | Block operation |

### SEC-L3: Project Settings

**Purpose:** Permission configuration in `.claude/settings.json`.

**Limitation:** Can be modified by agent (bypassable) - but protected by SEC-OS.

**Contents:**

- Permission allow/deny lists
- Hook configuration
- Approval mode settings

## Error to Tier Mapping

| Error Message | Security Tier | Resolution |
|---------------|---------------|------------|
| "Permission denied" | SEC-OS | `handle-protected-resource` |
| "Operation not permitted" | Sandbox | `sandbox-check` with bypass |
| Hook block message | SEC-L2 | Follow hook guidance |
| "Access denied" | SEC-L3 | Inform user (by design) |

## Threat Model

| Threat | Protection | Security Layer |
|--------|------------|----------------|
| AI modifies security hooks | Kernel immutable flags | SEC-OS |
| AI bypasses git hooks | Block --no-verify flag | SEC-L2 |
| AI force pushes | Block --force flag | SEC-L2 |
| AI escalates privileges | Block sudo, su, pkexec | SEC-L2 |
| AI modifies protected configs | Protected path enforcement | SEC-L2 |
| AI skips skill procedures | Sentinel enforcement | SEC-L2 |

## Settings Hierarchy

```text
PERMISSIONS (most specific wins):
  User Settings → Project Settings → Local Overrides
  (Local overrides project, project overrides user)

HOOKS (merged and deduplicated):
  User + Project + Local → Combined (no duplicates)
  (All hooks run, duplicates removed)

ENTERPRISE (overrides everything):
  Enterprise Settings → BLOCKS any conflicting setting below
  (Cannot be overridden by user/project/local)
```

**Loading Order** (bottom-up, later overrides earlier for permissions):

1. User Settings (`~/.claude/settings.json`)
2. Project Settings (`.claude/settings.json`)
3. Local Overrides (`.claude/settings.local.json`)
4. Enterprise Managed (always enforced, cannot be overridden)

## Network Domain Classification

### Always Allowed

- github.com, api.github.com
- registry.npmjs.org
- pypi.org

### Requires Approval

- api.* (external APIs)
- *.cloudfront.net (CDNs)

### Always Blocked

- localhost, 127.0.0.1
- *.local
- 10.*, 192.168.* (private networks)

## Approval Modes

| Mode | Description | Network | Protected Files |
|------|-------------|---------|-----------------|
| strict | Most restrictive | Block untrusted | Always stage |
| standard | Default | Ask for untrusted | Always stage |
| autonomous | Agent-driven | Ask for untrusted | Always stage |
| permissive | Least restrictive | Ask for untrusted | Always stage |

Note: Protected files always require staging regardless of mode.

## Agent DB Permissions

For agent permissions related to database operations (epic/task CRUD, memory domains), see `cf-db-operations/resources/agent-permissions.md`.
