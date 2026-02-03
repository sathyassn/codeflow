---
name: cf-git-workflow
description: Version control: branching, commits, PRs, and worktree management.
context: fork
---

# Git Workflow Skill

## Type

**Procedural** - Step-by-step procedures for version control operations.

## Purpose

**Provide git operation guidance for branch management, commit creation, and pull request workflows.**

## Responsibilities

- Branch verification and creation
- Commit message formatting (conventional commits)
- PR creation and management
- Worktree setup and cleanup

## Decision Tree

```text
START: What git operation?
    │
    ├─ On protected branch (main/master)?
    │   └─ BEFORE any edits → 🔧 create-feature-branch
    │
    ├─ Starting work? → 🔧 branch-verification
    │   └─ If on main → 🔧 create-feature-branch
    │
    ├─ Committing? → 🔧 create-commit
    │   └─ Use printf for multi-line (NOT heredoc)
    │
    ├─ Syncing remote? → 🔧 sync-remote
    │   └─ REQUIRES dangerouslyDisableSandbox: true
    │
    └─ Creating PR? → 🔧 create-pull-request
        └─ REQUIRES dangerouslyDisableSandbox: true
```

## Operations

### 🔧 branch-verification

**When:** Before starting any work

**Purpose:** Ensure on correct branch, not on protected main/master

**Procedure:**

```bash
CURRENT_BRANCH=$(git branch --show-current)
case "$CURRENT_BRANCH" in
  main|master) echo "🔒 On protected - create feature branch!" ;;
  feat/*|fix/*|docs/*|plan/*) echo "✅ Valid branch" ;;
esac
```

If on main/master: IMMEDIATELY invoke 🔧 create-feature-branch

### 🔧 create-feature-branch

**When:** Starting new work, need to leave protected branch

**Purpose:** Create properly named feature branch

**Procedure:**

```bash
# Determine prefix from work type
# feat/, fix/, docs/, plan/, chore/, refactor/, test/

git checkout -b ${PREFIX}/${SLUG}
# Example: git checkout -b feat/user-authentication
```

**Branch naming:** lowercase, hyphens only, 3-5 words max

### 🔧 create-commit

**When:** Work complete, ready to commit

**Purpose:** Create properly formatted conventional commit

**Format Requirements:**

- Subject: `type: description` (max 50 chars)
- Body: max 3 bullets, 72 chars each
- NO AI attribution (Claude, Co-Authored-By, etc.)

**Procedure:**

```bash
# Stage files
git add <specific-files>

# Show what will be committed
git status
git diff --cached --stat

# Commit with printf (NOT heredoc)
git commit -m "$(printf 'type: subject\n\n- bullet 1\n- bullet 2')"
```

**Valid types:** feat, fix, docs, refactor, test, chore, perf, style, build, ci

### 🔧 sync-remote

**When:** Need to pull/fetch from remote

**Purpose:** Sync with remote repository

**REQUIRES:** `dangerouslyDisableSandbox: true`

```javascript
Bash({
  command: "git pull origin main",
  dangerouslyDisableSandbox: true,
  description: "Pull latest from remote"
})
```

### 🔧 create-pull-request

**When:** Feature complete, ready for review

**Purpose:** Create PR following format requirements

**REQUIRES:** `dangerouslyDisableSandbox: true`

**PR Requirements:**

- Title: `type: description` (max 50 chars)
- Body MUST include `## Summary` and `## Testing` sections
- NO AI attribution

**Procedure:**

```javascript
// Push branch
Bash({
  command: "git push -u origin $(git branch --show-current)",
  dangerouslyDisableSandbox: true,
  description: "Push branch to remote"
})

// Create PR
Bash({
  command: "gh pr create --title \"type: desc\" --body \"## Summary\n...\n\n## Testing\n...\"",
  dangerouslyDisableSandbox: true,
  description: "Create pull request"
})
```

## Resources

For branch patterns: `resources/branch-patterns.md`
For commit format: `resources/commit-format.md`
For PR format: `resources/pr-format-guide.md`
