# Network Operation Patterns

Regex patterns used by enforcement hooks to detect network-bound commands. Sourced from `.codeflow/config/enforcement/enforcement-policy.json` (`network_operations` and `skills.git-workflow.operations` sections).

## Git Network Operations

Commands that access a remote git server. Require `dangerouslyDisableSandbox: true`.

| Pattern | Matches | Source |
|---------|---------|--------|
| `^git\s+(push\|pull\|fetch\|clone)` | git push, git pull, git fetch, git clone | `network_operations.git_network` |
| `^git\s+remote\s+update` | git remote update | `network_operations.git_network` |
| `^git\s+ls-remote` | git ls-remote | `network_operations.git_network` |

### Raw Regex (from enforcement-policy.json)

```text
^git\s+(push|pull|fetch|clone)
^git\s+remote\s+update
^git\s+ls-remote
```

### Block Message

`BLOCKED: Git network operation requires sandbox bypass`

### Delegation Reference

- Teammate: cf-git-operations (`sync-remote` operation)
- Security: cf-security (`sandbox-check` operation)

## GitHub CLI Operations

Commands that use the GitHub API via `gh` CLI. Require `dangerouslyDisableSandbox: true`.

| Pattern | Matches | Source |
|---------|---------|--------|
| `^gh\s+(pr\|issue\|release\|api\|workflow\|run\|repo\|gist)\s` | All gh subcommands that access GitHub API | `network_operations.github_cli` |

### Raw Regex (from enforcement-policy.json)

```text
^gh\s+(pr|issue|release|api|workflow|run|repo|gist)\s
```

### Block Message

`BLOCKED: GitHub CLI requires sandbox bypass`

### Delegation Reference

- Teammate: cf-git-operations (`create-pull-request` operation)
- Security: cf-security (`sandbox-check` operation)

## Git Workflow Skill Patterns

These patterns are used by the `git-workflow` skill in enforcement-policy.json for PreToolUse sentinel checks. They overlap with network operations but also cover local git write commands.

| Operation | Pattern | Network? |
|-----------|---------|----------|
| `create-commit` | `^git commit` | No (local) |
| `create-pull-request` | `^git push\|^gh pr create` | Yes |
| `sync-remote` | `^git\s+(pull\|fetch\|clone)` | Yes |
| `create-branch` | `^git checkout -b\|^git switch -c` | No (local) |
| `create-worktree` | `^git worktree add` | No (local) |
| `merge-branch` | `^git merge` | No (local) |
| `block-hard-reset` | `^git reset --hard` | No (local, blocked) |

## Sensitive File Patterns

Files that must never be staged or included in network operations. Used by `validate-network-safety` pre-flight checks.

From enforcement-policy.json (`edit_write.dangerous_extensions`):

| Category | Extensions |
|----------|-----------|
| Credential | `.pem`, `.key`, `.crt`, `.p12`, `.pfx`, `.keystore`, `.jks` |
| Binary | `.exe`, `.dll`, `.so`, `.dylib`, `.bin`, `.o`, `.a` |
| Archive | `.zip`, `.tar`, `.gz`, `.rar`, `.7z` |

Additional sensitive filename patterns:

```text
.env
.env.*
*credentials*
*secret*
```

## Network Domain Blocking

From enforcement-policy.json (`network.always_block_domains`):

### Blocked Domain Patterns

```text
localhost
127.0.0.1
0.0.0.0
::1
*.local
internal.*
*.internal
*.corp
*.corp.*
*.lan
*.home
*.private
```

### Blocked Private IP Ranges

```text
169.254.*
10.*
172.16.* through 172.31.*
192.168.*
```

These patterns are enforced by the WebFetch PreToolUse hook for HTTP requests. For Bash-based network tools (curl, wget), prefer using the WebFetch tool instead, which applies domain validation automatically.
