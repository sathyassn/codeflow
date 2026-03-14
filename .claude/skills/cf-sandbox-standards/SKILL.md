---
name: cf-sandbox-standards
description: Sandbox bypass rules for network operations. On-demand reference loaded by cf-git-operations, cf-development, and cf-quality-assurance when executing network-bound commands.
---

# Sandbox Standards Skill

## Type

**Procedural** - On-demand reference for sandbox bypass classification and network operation safety.

## Purpose

**Quick reference for determining when Bash commands require `dangerouslyDisableSandbox: true` and how to safely execute network-bound operations. Loaded by agents before executing commands that access the network.**

## Responsibilities

- Classify Bash commands as local-only or network-bound
- Document the `dangerouslyDisableSandbox: true` parameter for network operations
- Define delegation rules for network operations in PathFlow mode
- Specify pre-flight safety checks before network access
- NOT: Network execution (cf-git-operations handles git network ops)
- NOT: Domain validation for WebFetch (handled by WebFetch PreToolUse hook)
- NOT: Security classification (cf-security handles sandbox-check consultations)

## Decision Tree

```text
Executing a Bash command:
├── Local-only operation? → No bypass needed, proceed normally
├── Network-bound operation?
│   ├── In autorun mode ($AUTORUN_SESSION_ID)? → Pre-bypassed, proceed
│   ├── In PathFlow mode?
│   │   ├── Git network or gh CLI? → 🔧 delegate-network-op (to cf-git-operations)
│   │   └── Package manager or other? → 🔧 validate-network-safety → 🔧 apply-bypass
│   └── Outside PathFlow?
│       └── 🔧 validate-network-safety → 🔧 apply-bypass
└── Unsure? → 🔧 classify-operation first
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | classify-operation | ENF-L3 Advisory | Determine if a command needs sandbox bypass |
| 2 | apply-bypass | ENF-L3 Advisory | Set dangerouslyDisableSandbox parameter correctly |
| 3 | delegate-network-op | ENF-L3 Advisory | Route network ops to the correct handler |
| 4 | validate-network-safety | ENF-L3 Advisory | Pre-flight checks before network access |

## Operation Details

### 🔧 classify-operation

```text
When: Before executing any Bash command that might access the network
Purpose: Determine if the command requires sandbox bypass
Enforcement: ENF-L3 Advisory

Classification Table:

  | Category           | Commands                                          | Sandbox Bypass Required |
  |--------------------|---------------------------------------------------|-------------------------|
  | Local file ops     | cat, ls, diff, grep, find, mkdir, cp, mv, rm      | No                      |
  | Local git queries  | git status, git diff, git log, git branch          | No                      |
  | Local git writes   | git add, git checkout, git stash, git commit       | No                      |
  | Git network ops    | git push, git pull, git fetch, git clone           | YES                     |
  | Git network ops    | git remote update, git ls-remote                   | YES                     |
  | GitHub CLI         | gh pr, gh issue, gh release, gh api                | YES                     |
  | GitHub CLI         | gh workflow, gh run, gh repo, gh gist              | YES                     |
  | Package managers   | npm install, pip install, cargo build (fetching)   | YES                     |
  | HTTP clients       | curl, wget                                         | Handled by WebFetch hook |

Quick Test:
  Does the command contact a remote server? → YES = bypass required
  Does the command only read/write local files? → NO = bypass not required

Edge Cases:
  - git commit: local-only (writes to local .git), no bypass
  - git stash: local-only, no bypass
  - npm run / npm test: local-only (scripts), no bypass
  - npm install: network (fetches packages), bypass required
  - pip install -e .: local-only (editable install), no bypass
  - pip install package-name: network (fetches from PyPI), bypass required

Procedure:
  1. Identify the primary command being executed
  2. Check against the classification table above
  3. If the command is piped or chained (&&, ||, ;), classify each segment
  4. If ANY segment requires bypass, the entire command requires bypass
  5. Report classification: bypass required or not

Output: Classification result with bypass requirement
```

### 🔧 apply-bypass

```text
When: A command has been classified as needing sandbox bypass
Purpose: Set the dangerouslyDisableSandbox parameter correctly in the Bash tool call
Enforcement: ENF-L3 Advisory

Parameter Format:
  In the Bash tool call, set:
    "dangerouslyDisableSandbox": true

  Example tool call JSON:
    {
      "command": "git push -u origin feat/my-feature",
      "description": "Push feature branch to remote",
      "dangerouslyDisableSandbox": true
    }

Autorun Mode:
  When $AUTORUN_SESSION_ID is set, the sandbox is pre-bypassed by the CLI
  orchestrator. The dangerouslyDisableSandbox parameter is still accepted
  but has no additional effect. Detection:
    [[ -n "${AUTORUN_SESSION_ID:-}" ]]

Important Notes:
  - The parameter name is exact: dangerouslyDisableSandbox (camelCase)
  - The value must be boolean true (not string "true")
  - This parameter is set per-command, not globally
  - Each network command in a session needs its own bypass flag
  - The user will be prompted for permission when this flag is set

Procedure:
  1. Confirm the command requires bypass (via classify-operation)
  2. Check for autorun mode (bypass already active)
  3. If not autorun: include dangerouslyDisableSandbox: true in the Bash call
  4. Add a clear description explaining why bypass is needed
  5. Log the bypass usage for audit trail

Output: Correctly formatted Bash tool call with bypass parameter
```

### 🔧 delegate-network-op

```text
When: A network operation is needed and delegation rules apply
Purpose: Determine whether to delegate or self-execute the network operation
Enforcement: ENF-L3 Advisory

Delegation Decision Table:

  | Context         | Operation Type          | Action                                    |
  |-----------------|-------------------------|-------------------------------------------|
  | PathFlow mode   | git push/pull/fetch     | DELEGATE to cf-git-operations              |
  | PathFlow mode   | gh pr/issue/api/etc.    | DELEGATE to cf-git-operations              |
  | PathFlow mode   | npm install / pip install| Self-execute with bypass flag              |
  | Outside PathFlow| git network ops         | Self-execute with bypass flag              |
  | Outside PathFlow| gh CLI                  | Self-execute with bypass flag              |
  | Any context     | curl / wget             | Use WebFetch tool instead (hook-validated) |

PathFlow Delegation Rule:
  In PathFlow mode, ALL git network operations and GitHub CLI commands
  MUST be delegated to cf-git-operations via SendMessage:

    SendMessage(
      type="message",
      recipient="cf-git-operations",
      content="Please push to origin/{branch}",
      summary="Request git push"
    )

  cf-git-operations handles sandbox bypass internally for its operations.

Self-Execute Cases:
  Package managers (npm install, pip install, cargo build) are executed
  directly by the requesting agent with the bypass flag. These are not
  git operations and do not route through cf-git-operations.

Logging:
  All sandbox bypass usage should be noted in progress updates:
    SendMessage to cf-knowledge-layer:
      "DEV-UPDATE: sandbox bypass used for npm install"

Procedure:
  1. Identify operation type (git network, gh CLI, package manager, HTTP)
  2. Check if in PathFlow mode (pathflow-session-status.json exists with status not "pf-complete")
  3. If PathFlow + git/gh: delegate to cf-git-operations
  4. If PathFlow + package manager: self-execute with bypass
  5. If outside PathFlow: self-execute with bypass
  6. If HTTP: use WebFetch tool (not Bash)
  7. Log bypass usage

Output: Delegation decision with target agent or self-execute instruction
```

### 🔧 validate-network-safety

```text
When: Before executing any network-bound command (after classification, before execution)
Purpose: Pre-flight safety checks to prevent accidental data exposure or wrong targets
Enforcement: ENF-L3 Advisory

Git Network Pre-Flight Checks:

  | Check                        | How to Verify                              | Block If        |
  |------------------------------|--------------------------------------------|-----------------|
  | Correct remote               | git remote -v (expect origin → expected URL)| Unknown remote  |
  | Correct branch               | git branch --show-current                  | On main/master  |
  | No secrets staged            | git diff --staged --name-only (check for .env, *.key, *.pem) | Secrets found |
  | Clean working state          | git status --short                         | Advisory only   |

GitHub CLI Pre-Flight Checks:

  | Check                        | How to Verify                              | Block If        |
  |------------------------------|--------------------------------------------|-----------------|
  | Correct repository           | gh repo view --json nameWithOwner -q .nameWithOwner | Wrong repo |
  | Correct PR target branch     | Verify --base flag points to main          | Wrong target    |
  | No sensitive content in body | Scan PR body for credential patterns       | Secrets found   |

Package Manager Pre-Flight Checks:

  | Check                        | How to Verify                              | Block If         |
  |------------------------------|--------------------------------------------|------------------|
  | Lock file matches manifest   | Compare package.json ↔ package-lock.json   | Advisory only    |
  | No suspicious packages       | Review package names for typosquatting      | Suspicious names |
  | Expected registry            | Check .npmrc / pip.conf for custom registry | Unexpected URL   |

Sensitive File Patterns (block if staged or included):
  .env, .env.*, *.key, *.pem, *.p12, *.pfx, *credentials*, *secret*

Procedure:
  1. Identify the operation type (git, gh, package manager)
  2. Run the appropriate pre-flight checks from the tables above
  3. If any blocking condition is met: STOP and report the issue
  4. If advisory-only conditions are met: warn but proceed
  5. Confirm all checks pass before executing

Output: Pre-flight check result (pass/warn/block with details)
```

## Sandbox Network AllowedDomains

Claude Code's sandbox has **two independent layers** that can block network access:

| Layer | Control | Prompt Message |
|-------|---------|---------------|
| **Permissions** | `permissions.allow/ask/deny` + `defaultMode` | Standard permission prompt |
| **Sandbox Network** | `sandbox.network.allowedDomains` | "Network request outside of sandbox" |

These layers operate independently:

- `bypassPermissions` mode does **NOT** bypass sandbox network checks
- `dangerouslyDisableSandbox: true` bypasses **both** layers
- `sandbox.network.allowedDomains` pre-approves specific domains for the sandbox layer only
- **PermissionRequest hooks do NOT fire** for sandbox network prompts (confirmed by testing)

### AllowedDomains Configuration

```json
"sandbox": {
  "enabled": true,
  "autoAllowBashIfSandboxed": true,
  "network": {
    "allowedDomains": [
      "github.com",
      "api.github.com",
      "*.githubusercontent.com",
      "objects.githubusercontent.com",
      "registry.npmjs.org",
      "pypi.org",
      "files.pythonhosted.org"
    ]
  }
}
```

### Domain Tiers

| Tier | Domains | Used By |
|------|---------|---------|
| **Standard** | github.com, api.github.com, *.githubusercontent.com, objects.githubusercontent.com, registry.npmjs.org, pypi.org, files.pythonhosted.org | standard, autonomous, permissive templates |
| **None** | (no allowedDomains) | strict template -- prompts for every network request |

### Impact on dangerouslyDisableSandbox

With `allowedDomains` configured for common development domains, `dangerouslyDisableSandbox: true` is **rarely needed** for:

- `git push/pull/fetch/clone` to GitHub (github.com is allowed)
- `gh pr/issue/api` calls (api.github.com is allowed)
- `npm install` (registry.npmjs.org is allowed)
- `pip install` (pypi.org, files.pythonhosted.org are allowed)

`dangerouslyDisableSandbox: true` is **still needed** for:

- Custom git remotes (non-GitHub hosts)
- Private package registries
- Domains not in the allowedDomains list
- Operations that also need filesystem sandbox bypass

### Template Differences

| Template | sandbox.network | autoAllowBashIfSandboxed |
|----------|----------------|--------------------------|
| strict | Not configured (prompt for all) | false |
| standard | Standard tier domains | true |
| autonomous | Standard tier domains | true |
| permissive | Standard tier domains | true |

## Resources

Companion resources provide expanded detail beyond the operation summaries above. Load when the inline guidance is insufficient for the task at hand.

| Resource | Companion To | Contains |
|----------|-------------|----------|
| `resources/network-patterns.md` | classify-operation, validate-network-safety | Full regex patterns from enforcement-policy.json for network operation detection |
