---
name: cf-security-management
description: Provides security controls for sandbox bypass, protected resource staging, settings template sync, and permission error diagnosis. Use before network operations, when editing protected files, or when encountering permission errors.
context: fork
agent: cf-general-purpose
---

# Security Management Skill

## Type

**Procedural** - Provides step-by-step technical procedures for security-related operations including sandbox bypass, protected resource editing, managed tmp protection, and settings template management.

## Purpose

**Provide systematic security operation guidance for sandbox bypass, protected resource handling, managed tmp structure, and settings template synchronization.**

## Responsibilities

- Sandbox bypass classification and execution
- Protected resource detection and tmp-file workflow
- Managed tmp structure enforcement
- Settings template synchronization
- Security-related error diagnosis
- NOT: Authentication (that's external systems)
- NOT: Audit logging (that's cf-db-operations:log-append)
- NOT: Agent DB permissions (that's cf-db-operations)

## Decision Tree

```text
START: What security-related situation are you handling?
    │
    ├─ About to execute a command?
    │   └─→ USE 🔧 sandbox-check
    │       ├─ Local file/git read operations? ────→ Sandbox OK
    │       ├─ git add/checkout/stash? ────────────→ Sandbox OK
    │       ├─ git commit (any format)? ───────────→ Sandbox OK
    │       ├─ git push/pull/fetch/clone? ─────────→ BYPASS REQUIRED
    │       └─ GitHub CLI (gh pr, gh issue)? ──────→ BYPASS REQUIRED
    │
    ├─ Got a permission error?
    │   └─→ USE 🔧 diagnose-permission-error
    │       ├─ "Permission denied" ────────────────→ OS-level protection
    │       │   └─→ USE 🔧 handle-protected-resource
    │       ├─ "Operation not permitted" ──────────→ Sandbox restriction
    │       │   └─→ USE 🔧 sandbox-check (with bypass)
    │       ├─ "Managed Tmp Protection" ───────────→ Protected by design
    │       │   └─ Cannot delete /tmp/claude/managed/ folders
    │       └─ "Access denied" + settings path ────→ Project policy block
    │           └─ Intentional - inform user
    │
    ├─ Editing a file?
    │   └─→ TRY DIRECT EDIT FIRST (most files are NOT protected)
    │       │
    │       ├─ Edit succeeded? ──────────────────→ Done (no staging needed)
    │       │
    │       └─ Got "Permission denied" or hook block?
    │           └─→ USE 🔧 handle-protected-resource
    │               ├─ Stage to /tmp/claude/managed/protected-edits/
    │               ├─ Edit the tmp file
    │               ├─ Provide copy command to user
    │               └─ If settings file → also USE 🔧 sync-settings-templates
    │
    └─ Modified settings templates?
        └─→ USE 🔧 sync-settings-templates
            ├─ Verify hooks section identical across all templates
            ├─ Verify _version identical across all templates
            ├─ Provide copy commands to user
            └─ Verify settings files match after user copies
```

**Cross-skill Integration:**

For `git push`, `git pull`, `git fetch`, `gh pr create`:

1. **FIRST:** This skill → 🔧 sandbox-check
2. **THEN:** `cf-git-workflow` → appropriate operation

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | sandbox-check | ENF-L1 Sentinel | Sandbox bypass classification and execution |
| 2 | handle-protected-resource | ENF-L1 Sentinel | Protected resource tmp-file staging workflow |
| 3 | diagnose-permission-error | None | Permission error diagnosis |
| 4 | sync-settings-templates | ENF-L2 Stop | Settings template synchronization |

## Operation Details

### 🔧 sandbox-check

```text
When: Before operations that may require sandbox bypass
Enforcement: ENF-L1 Sentinel
Lifecycle: BEFORE_NETWORK

Purpose: Proactively determine sandbox bypass requirement to avoid failed attempts

Autorun Mode Behavior:
  If $AUTORUN_SESSION_ID is set:
    - Claude started with --dangerously-skip-permissions
    - Sandbox is ALREADY BYPASSED by the Go CLI orchestrator
    - No need to use dangerouslyDisableSandbox: true
    - Network operations work without explicit bypass

  Detection:
    if [[ -n "${AUTORUN_SESSION_ID:-}" ]]; then
      # Sandbox already disabled, no bypass needed
      SANDBOX_BYPASSED=true
    fi

Classification Table:
  | Operation Category | Sandbox OK? | Action |
  |--------------------|-------------|--------|
  | Local file reads | Yes | Normal execution |
  | Local git queries (status, diff, log, branch) | Yes | Normal execution |
  | git add, checkout, stash | Yes | Normal execution |
  | git commit | Yes | Normal execution |
  | git push, pull, fetch, clone | No* | Requires bypass |
  | GitHub CLI (gh pr, gh issue, etc.) | No* | Requires bypass |

  *In autorun mode: Sandbox already bypassed, no action needed

Procedure:
  1. Check for autorun context ($AUTORUN_SESSION_ID)
  2. If in autorun: Sandbox already bypassed, proceed normally
  3. If interactive: Identify operation type from the table above
  4. For operations needing bypass in interactive mode:
     Bash({
       command: "git push -u origin branch-name",
       dangerouslyDisableSandbox: true,
       description: "Push to remote"
     })
  5. Inform user when bypassing (interactive only):
     "Using sandbox bypass for network operation."

Cross-skill: cf-git-workflow (operations use this as prerequisite)

Output:
  operation: {operation type}
  requires_bypass: true | false
  sentinel_created: true (if bypass used)
  autorun_mode: true | false
  sandbox_pre_bypassed: true | false (if autorun)

📚 Resource: [protected-paths.md](resources/protected-paths.md)
   Load when: Determining if an operation needs sandbox bypass
```

### 🔧 handle-protected-resource

```text
When: Agent is blocked from editing a protected file (OS or hook block)
Enforcement: ENF-L1 Sentinel

IMPORTANT: TRY DIRECT EDIT FIRST - This is a FALLBACK workflow

Protected vs Not Protected:
  | File Type | Protected? | Action |
  |-----------|------------|--------|
  | .claude/hooks/*.sh | YES | Use this workflow |
  | .claude/settings.json | YES | Use this workflow |
  | .claude/settings.local.json | YES | Use this workflow |
  | .codeflow/scripts/security/** | YES | Use this workflow |
  | .claude/skills/**/*.md | NO | Edit directly |
  | .claude/commands/**/*.md | NO | Edit directly |
  | .claude/agents/**/*.md | NO | Edit directly |

Procedure:
  1. DETECT blocked path and error type

  2. STAGE to /tmp/claude/managed/protected-edits/{relative-path}
     mkdir -p /tmp/claude/managed/protected-edits/{parent-dirs}
     cp {original} /tmp/claude/managed/protected-edits/{relative-path}

  3. EDIT the tmp file
     Use Edit tool on /tmp/claude/managed/protected-edits/ path
     All edits allowed in managed area

  4. PROVIDE combined apply command to user:
     cp /tmp/claude/managed/protected-edits/{path} {dest} && \
     chmod +x {dest}  # if executable

  5. VERIFY after user confirms execution
     Read the original file
     Confirm changes match expected

  6. CLEANUP (targeted only)
     rm /tmp/claude/managed/protected-edits/{specific-file}
     DO NOT delete /tmp/claude/managed/ folders

Output:
  staged_path: /tmp/claude/managed/protected-edits/{path}
  apply_command: {combined command}
  verified: true | false

📚 Resources:
   [protected-paths.md](resources/protected-paths.md) - Load when: Checking if file is protected
   [staging-workflow.md](resources/staging-workflow.md) - Load when: Detailed staging procedures needed
```

### 🔧 diagnose-permission-error

```text
When: Agent encounters unexpected permission error
Enforcement: None (reactive diagnostic)

Decision Tree:
  Error: "Permission denied"
    → Cause: OS-level protection
    → Action: Use handle-protected-resource operation

  Error: "Operation not permitted"
    → Cause: Sandbox restriction
    → Action: Use sandbox-check, need dangerouslyDisableSandbox

  Error: Hook block message with protected path
    → Cause: PreToolUse hook protection
    → Action: Use handle-protected-resource operation

  Error: "Managed Tmp Protection" block
    → Cause: Protected by design
    → Action: Cannot delete managed folders, inform user

  Error: "Access denied" / settings pattern match
    → Cause: Permissions deny list in settings.json
    → Action: Cannot override - inform user this is by design

Procedure:
  1. Identify error type from message content
  2. Check if path is protected (see resources/protected-paths.md)
  3. Recommend appropriate action based on decision tree
  4. If settings-denied: Explain this is intentional project policy

Output:
  error_type: os_protection | sandbox | hook_block | managed_tmp | settings_deny
  cause: {explanation}
  recommended_action: {operation to use}

📚 Resources:
   [protected-paths.md](resources/protected-paths.md) - Load when: Checking protection status
   [permission-matrix.md](resources/permission-matrix.md) - Load when: Understanding security tier details
```

### 🔧 sync-settings-templates

```text
When: After modifying any settings template
Enforcement: ENF-L2 Stop

Template Location: .claude/settings-templates/
  - strict.json - Minimal permissions, read-only focus
  - standard.json - Balanced, ask for git commit/push/PR
  - autonomous.json - Full autonomy within safe boundaries
  - permissive.json - Maximum permissions

Protection Status:
  | File | Protected? | Edit Method |
  |------|------------|-------------|
  | .claude/settings-templates/*.json | NO | Edit directly |
  | .claude/settings.json | YES | Copy from template |
  | .claude/settings.local.json | YES | Copy from template |

Sync Rules:
  | Section | Sync Rule |
  |---------|-----------|
  | _version | MUST be identical across ALL templates + settings |
  | hooks | MUST be identical across ALL templates |
  | sandbox | Can differ per template |
  | permissions | Can differ per template |

Procedure:
  1. MODIFY templates in .claude/settings-templates/
     - For hooks section: Update ALL templates identically
     - For _version: Update ALL templates identically
     - For permissions/sandbox: Update appropriately per template

  2. VERIFY hooks section is identical across all templates

  3. VERIFY _version is identical across all templates

  4. PROVIDE copy commands to user:
     cp .claude/settings-templates/strict.json .claude/settings.json && \
     cp .claude/settings-templates/autonomous.json .claude/settings.local.json

  5. WAIT for user to run copy command

  6. VERIFY settings files match templates after copy

Default Template Mapping:
  | Template | Target | Purpose |
  |----------|--------|---------|
  | strict.json | .claude/settings.json | Project settings (committed) |
  | autonomous.json | .claude/settings.local.json | Local settings (gitignored) |

Output:
  templates_synced: [list of templates updated]
  version: {new version}
  copy_command: {combined command}

📚 Resource: [staging-workflow.md](resources/staging-workflow.md)
   Load when: Understanding template sync procedures or copy commands
```

## Managed Tmp Structure

```text
/tmp/claude/
├── managed/                       # Protected container
│   ├── protected-edits/           # For protected resource workflow
│   │   └── .claude/hooks/...      # Staged files mirror project structure
│   └── state/                     # For state tracking scripts
│       └── verify-work-retry-*    # State files
└── (unmanaged scratch space)      # Ad-hoc files, no special protection
```

**Protection Rules:**

| Path | Folder Protection | Contents Protection |
|------|-------------------|---------------------|
| `/tmp/claude/managed/` | No delete/rename | N/A (container) |
| `managed/protected-edits/` | No delete/rename | Full CRUD allowed |
| `managed/state/` | No delete/rename | Create/Edit yes, Delete blocked |

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [protected-paths.md](resources/protected-paths.md) | Full list of protected patterns | When checking protection |
| [permission-matrix.md](resources/permission-matrix.md) | Security tier details | When understanding protection levels |
| [staging-workflow.md](resources/staging-workflow.md) | Detailed staging procedures | When editing protected files |
