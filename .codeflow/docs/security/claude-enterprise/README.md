# Enterprise Managed Settings

**System-Level AI Agent Restrictions**

## What This Is

Enterprise managed settings are **system-level** Claude Code configurations that:

- Apply to ALL projects on your machine
- CANNOT be overridden by project settings
- Require admin/sudo access to modify
- Provide bulletproof protection against agent manipulation

This is **Layer 0** of our security architecture (L0-L5 + OS Foundation) - the foundation that makes all other layers effective.

---

## Why You Need This

### Without Enterprise Settings

AI agents can potentially:

- Bypass commit validation hooks (`git commit --no-verify`)
- Disable hooks entirely (`git config core.hooksPath /dev/null`)
- Modify their own permission settings (`.claude/settings.json`)
- Escalate privileges (`sudo` commands)
- Access secret files (`.env`, `.key` files)

### With Enterprise Settings

Agents **cannot** bypass validation (L2 PreToolUse hooks block `--no-verify` via regex)
Agents **cannot** disable hooks (L0 blocks `git config core.hooksPath`)
Agents **cannot** escalate privileges (L0 blocks `sudo`)
Agents **cannot** access secrets (L0 blocks sensitive file patterns)
Server-side validation (L1) provides independent verification

**Note:** L0 uses PREFIX matching only. Flag blocking (`--no-verify`, `--force`) requires L2 regex hooks.

**Result:** Bulletproof protection even if agent is compromised or buggy.

---

## For CodeFlow Projects

This directory contains enterprise managed settings for **CodeFlow project maintainers**.

**File:** `managed-settings.json`

**Restrictions:** Minimal - blocks only critical bypasses and security violations

**Allows:**

- Editing source code and configuration
- Updating scripts and hooks
- Normal development work

**Blocks:**

- Git hook bypasses (`--no-verify`, `-n`, all variants)
- Hook disabling attempts (`core.hooksPath`)
- Force push to protected branches (`main`, `master`, `release`, `production`)
- Privilege escalation (`sudo`, `su`, `doas`, `pkexec`)
- Secrets access (`.env`, `.key`, `.pem`, credentials, SSH keys)
- Catastrophic commands (`rm -rf /`, `mkfs`, `dd`)
- Security infrastructure modification (settings.json, hooks, workflows)

### What's Protected

**CRITICAL (Never Allow):**

- `git commit --no-verify` / `-n` - Bypasses validation
- `git push --no-verify` - Bypasses server checks
- `git rebase/cherry-pick/merge/am --no-verify` - Bypasses validation
- `git config core.hooksPath` - Disables hooks
- `git push --force` to main/master/release/production - Rewrites protected history
- `sudo` / `su` / `doas` / `pkexec` - Escalates privileges
- Reading `.env`, `.key`, `.pem`, credentials - Exposes secrets
- `rm -rf /`, `mkfs`, `dd` - Catastrophic operations
- Writing `.claude/settings.json` - Weakens security config
- Deleting security files (`rm`, `rm -rf`, `mv`, `git rm`, `unlink`, `truncate`)
- Script bypass (`bash -c`, `sh -c`, `eval` with delete commands)
- Disabling sandbox (`allowUnsandboxedCommands` enforced false)

**Allowed:**

- Reading hooks and workflows - Understanding code
- Editing documentation - Normal workflow
- Running build commands - Development work
- Normal git operations - With hooks running
- Force push to feature branches - Normal development workflow

---

## Installation

### Helper Script (Recommended)

```bash
# Dry run (shows what would happen)
.codeflow/scripts/settings/setup-managed-settings.sh --dry-run

# Install
.codeflow/scripts/settings/setup-managed-settings.sh
```

The helper script:

- Detects your operating system automatically
- Validates settings file exists
- Creates target directory with correct permissions
- Installs to appropriate location (macOS, Linux, or Windows)
- Provides verification instructions

### Manual Install (macOS)

```bash
# Review settings first
cat .codeflow/docs/security/claude-enterprise/managed-settings.json

# Install
sudo mkdir -p "/Library/Application Support/ClaudeCode"
sudo cp .codeflow/docs/security/claude-enterprise/managed-settings.json "/Library/Application Support/ClaudeCode/managed-settings.json"
sudo chmod 644 "/Library/Application Support/ClaudeCode/managed-settings.json"

# Verify
sudo cat "/Library/Application Support/ClaudeCode/managed-settings.json"

# Restart Claude Code
```

### Manual Install (Linux)

```bash
# Review settings first
cat .codeflow/docs/security/claude-enterprise/managed-settings.json

# Install
sudo mkdir -p /etc/claude-code
sudo cp .codeflow/docs/security/claude-enterprise/managed-settings.json /etc/claude-code/managed-settings.json
sudo chmod 644 /etc/claude-code/managed-settings.json

# Verify
sudo cat /etc/claude-code/managed-settings.json

# Restart Claude Code
```

### Quick Install (Windows)

**PowerShell (Run as Administrator):**

```powershell
# Review settings first
Get-Content .codeflow\docs\security\claude-enterprise\managed-settings.json

# Install
New-Item -ItemType Directory -Force -Path "C:\ProgramData\ClaudeCode"
Copy-Item .codeflow\docs\security\claude-enterprise\managed-settings.json "C:\ProgramData\ClaudeCode\managed-settings.json"

# Verify
Get-Content "C:\ProgramData\ClaudeCode\managed-settings.json"

# Restart Claude Code
```

---

## Verification

After installation and Claude Code restart:

### Test 1: Bypass Attempt Should Fail

```bash
git commit --no-verify -m "test"
```

**Expected:**

```text
Permission denied: Bash(git commit --no-verify:*)
   blocked by enterprise managed settings
```

If you see this error, L0 is active!

### Test 2: Normal Commit Should Work

```bash
git commit -m "feat: test normal commit"
```

**Expected:**

```text
Commit succeeds (with hook validation)
```

### Test 3: Hook Disable Should Fail

```bash
git config core.hooksPath /dev/null
```

**Expected:**

```text
Permission denied: Bash(git config core.hooksPath:*)
   blocked by enterprise managed settings
```

### Test 4: Sudo Should Fail

```bash
sudo ls
```

**Expected (in Claude Code):**

```text
Permission denied: Bash(sudo:*)
   blocked by enterprise managed settings
```

**Note:** This test is for agent-invoked commands. Human terminal still allows sudo.

### Test 5: Force Push to Protected Branch Should Fail

```bash
git push --force origin main
```

**Expected:**

```text
Permission denied: Bash(git push --force origin main:*)
   blocked by enterprise managed settings
```

### Test 6: Force Push to Feature Branch Should Work

```bash
git push --force origin feat/my-feature
```

**Expected:**

```text
Push succeeds (feature branches allow force push)
```

---

## File Location by OS

**macOS:**

```text
/Library/Application Support/ClaudeCode/managed-settings.json
```

**Linux:**

```text
/etc/claude-code/managed-settings.json
```

**Windows:**

```text
C:\ProgramData\ClaudeCode\managed-settings.json
```

**Permissions:** 644 (readable by all, writable only by root/admin)

---

## Customization

### Safe to Modify

You can customize:

- Secret file patterns (add `.custom-secret` patterns)
- Dangerous command patterns (add organization-specific risks)
- Comments and documentation within the file
- `_version` field (when making changes to track configuration versions)
- Branch-specific force push entries (add new protected branches)

### NEVER Remove

**Do NOT remove these protections:**

- `Bash(git commit --no-verify:*)` - Bypasses all validation
- `Bash(git push --no-verify:*)` - Bypasses server checks
- `Bash(git config core.hooksPath:*)` - Disables hooks entirely
- `Bash(sudo:*)` - Escalates privileges
- Branch-specific force push deny entries for main/master/release/production

Removing these compromises the entire security architecture.

### Template Versioning Protection

**IMPORTANT:** When this enterprise settings file is used as a template for consumer projects,
the `_version` field helps track configuration evolution. If you maintain settings templates
(like the 4 approval mode templates in `.claude/settings-templates/`):

- **ALWAYS synchronize** `_version` across all templates when making changes
- Use semantic versioning (X.Y.Z format)
- Increment version for any security-relevant changes

### Adding Restrictions

To add a new restriction:

```json
{
  "permissions": {
    "deny": [
      "// Existing restrictions...",

      "// Organization-specific restrictions",
      "Bash(kubectl delete:*)",
      "Bash(terraform destroy:*)",
      "Read(**/*production.env)"
    ]
  }
}
```

**Note:** Use `"// Comment text"` as string elements for comments within the deny array.
JSON does not support traditional comments, so we use string prefixes that won't match any tool patterns.

Then re-install:

```bash
sudo cp .codeflow/docs/security/claude-enterprise/managed-settings.json "/Library/Application Support/ClaudeCode/managed-settings.json"
# Restart Claude Code
```

---

## Troubleshooting

### Issue: Restrictions Not Applying

**Solutions:**

1. Verify file location is correct for your OS
2. Check file permissions: `sudo ls -la "/Library/Application Support/ClaudeCode/"`
3. Validate JSON syntax: `jq . managed-settings.json`
4. Ensure Claude Code was restarted completely
5. Check Claude Code version supports enterprise settings

### Issue: Locked Out of Legitimate Work

**Solutions:**

1. This version is designed for maintainers - should not block normal work
2. Check if restriction is too broad (pattern matching issue)
3. Use project settings (`.claude/settings.json`) for workflow preferences
4. Use local overrides (`.claude/settings.local.json`) for personal needs
5. If truly blocked, temporarily remove enterprise settings (requires sudo)

### Issue: Agent Still Bypassing

**Solutions:**

1. Verify file exists: `sudo cat "/Library/Application Support/ClaudeCode/managed-settings.json"`
2. Ensure Claude Code restarted after installation
3. Check Claude Code is using the file (restart again)
4. Verify agent is using Claude Code (not direct terminal)
5. Check Layer 1 (server-side) as backup protection

---

## Understanding Protections

### Pattern Syntax

**Bash Commands:**

```json
"Bash(git commit --no-verify:*)"
```

- Matches: `git commit --no-verify`
- With any additional arguments after

**File Patterns:**

```json
"Read(**/.env)"
```

- Matches: `.env` anywhere in tree
- `**` is recursive glob

### Branch-Specific Force Push Protection

L0 uses PREFIX matching, so force push entries are branch-specific:

```json
"Bash(git push --force origin main:*)"
"Bash(git push -f origin main:*)"
```

This blocks force push to `main` but allows force push to feature branches.
L2 PreToolUse hooks provide additional wildcard coverage (e.g., `release/*`).

### Why Minimal Restrictions

This maintainer version has **minimal restrictions** because:

1. **Trust:** Maintainers are trusted to modify template safely
2. **Flexibility:** Need to edit hooks, settings, scripts
3. **Critical Only:** Only block truly dangerous operations
4. **Server Backup:** L1 (server-side) validates anyway
5. **Branch Freedom:** Feature branches allow force push for rebasing workflows

---

## Maintenance

### Updating Settings

```bash
# 1. Edit local copy
vim .codeflow/docs/security/claude-enterprise/managed-settings.json

# 2. Test changes (validate JSON)
jq . .codeflow/docs/security/claude-enterprise/managed-settings.json

# 3. Re-install
sudo cp .codeflow/docs/security/claude-enterprise/managed-settings.json "/Library/Application Support/ClaudeCode/managed-settings.json"

# 4. Restart Claude Code

# 5. Test restrictions work
git commit --no-verify -m "test"
```

### Backup Before Changes

```bash
# Backup current settings
sudo cp "/Library/Application Support/ClaudeCode/managed-settings.json" \
  ~/backups/managed-settings-$(date +%Y%m%d).json
```

### Removing (If Needed)

```bash
# Only do this if critically blocked
sudo rm "/Library/Application Support/ClaudeCode/managed-settings.json"

# Restart Claude Code

# Warning: Removes all L0 protection!
```

---

## For Organizations

### Deployment Strategy

**Option 1: Manual Installation**

- Document installation steps
- Require during onboarding
- Verify installation checklist

**Option 2: Configuration Management**

```bash
# Ansible example
- name: Install Claude Code enterprise settings
  copy:
    src: managed-settings.json
    dest: /Library/Application Support/ClaudeCode/managed-settings.json
    owner: root
    mode: '0644'
  become: yes
```

**Option 3: Mobile Device Management (MDM)**

```bash
# Jamf example
jamf policy -trigger install-claude-settings
```

### Verification Script

```bash
#!/bin/bash
# verify-enterprise-settings.sh

if [ "$(uname)" = "Darwin" ]; then
  SETTINGS_PATH="/Library/Application Support/ClaudeCode/managed-settings.json"
elif [ "$(uname)" = "Linux" ]; then
  SETTINGS_PATH="/etc/claude-code/managed-settings.json"
fi

if [ ! -f "$SETTINGS_PATH" ]; then
  echo "Enterprise settings not found: $SETTINGS_PATH"
  exit 1
fi

echo "Enterprise settings installed: $SETTINGS_PATH"
exit 0
```

---

## Related Documentation

- **Security Architecture:** See `.codeflow/docs/security/` directory
- **Settings Templates:** `.claude/settings-templates/`
- **Security Scripts:** `.codeflow/scripts/security/`

---

**Status:** Production-ready for CodeFlow project maintainers
**Version:** 3.6.0
**Last Updated:** 2026-02-27

## Changelog

### v3.6.0 (2026-02-27)

- Added 36 branch-specific force push deny entries (main, master, release, production)
- Force push now allowed on feature branches (L0 no longer blanket blocks)
- Added `--delete` protection for main/master branches
- Added `--no-verify` commit/push protection
- Updated architecture notes to document branch-specific approach
- L2 hooks provide wildcard coverage (release/*) that L0 PREFIX matching cannot
- Migrated from workflow project to CodeFlow `.codeflow/docs/security/`

### v3.5.0 (2026-01-09)

- Changed sandbox from allowUnsandboxedCommands:false to excludedCommands:[git,gh] for selective network bypass

### v3.0.0 (2025-12-13)

- **BREAKING:** Fixed path syntax - all paths now use `/` prefix for project-relative
- Added comprehensive path syntax documentation in settings metadata
- Added architecture notes explaining L0 vs L2 protection layers

### v2.2.0 (2025-12-13)

- Added comprehensive file deletion protection (rm, rm -f, rm -r, rm -rf variants)
- Added move command protection (mv = effective deletion)
- Added git rm protection (tracked file deletion)
- Added unlink, rmdir, truncate protection
- Added script execution bypass prevention (bash -c, sh -c, eval patterns)

### v2.1.0 (2025-12-13)

- Added security infrastructure protection (settings.json, hooks, workflows)
- Fixed JSON syntax (object-style comments converted to string comments)
- Added comprehensive git command coverage (rebase, cherry-pick, merge, am)
- Added force push prevention patterns
- Added privilege escalation tools (doas, pkexec)

### v2.0.0 (2025-12-13)

- Post-incident hardening with expanded bypass patterns

### v1.0.0 (2025-10-12)

- Initial release with basic protections
