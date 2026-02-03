# Worktree Guide

**Purpose:** Comprehensive guide for git worktree operations, naming conventions, cleanup procedures, and memory isolation patterns. Reference when setting up parallel work or troubleshooting worktree issues.

## Standard Naming Patterns

| Use Case | Worktree Path | Branch Name | Example |
|----------|---------------|-------------|---------|
| Phase implementation | `.git-worktrees/feat-phase-{N.M}/` | `feat/phase-{N.M}` | `feat-phase-6-1` |
| Bug fixes | `.git-worktrees/fix-{slug}/` | `fix/{slug}` | `fix-auth-race-condition` |
| Exploration work | `.git-worktrees/experiment-{slug}/` | `experiment/{slug}` | `experiment-websocket` |
| Planning | `.git-worktrees/plan-{slug}/` | `plan/{slug}` | `plan-oauth-integration` |
| Documentation | `.git-worktrees/docs-{slug}/` | `docs/{slug}` | `docs-api-reference` |
| Hotfixes | `.git-worktrees/hotfix-{slug}/` | `hotfix/{slug}` | `hotfix-security-patch` |

**Path convention:** Always use `.git-worktrees/{prefix}-{slug}/` format for consistency

## Setup Script Details

The `scripts/worktree/setup.sh` script performs automatic configuration:

**What it does:**

- Copies `.env.example` to `.env` (if source file exists)
- Copies `.gitignore` to worktree (ensures proper git ignore rules)
- Installs Node.js dependencies (runs `npm install` if `package.json` exists)
- Verifies setup completion
- Reports warnings if dependencies failed to install

**Arguments:**

```bash
bash scripts/worktree/setup.sh <worktree-path> <branch-name> [agent-type]

# Example:
bash scripts/worktree/setup.sh ".git-worktrees/feat-phase-6-1" "feat/phase-6-1" "developer"
```

**When to run manually:**

- After `git worktree add` command
- Before starting work in new worktree
- If automated command setup fails

## Cleanup Operations

### List Active Worktrees

```bash
git worktree list

# Output shows:
# /path/to/main    abc123 [main]
# /path/to/.git-worktrees/feat-phase-6-1  def456 [feat/phase-6-1]
```

### Remove Worktree

**After work complete and PR merged:**

```bash
# Step 1: Remove worktree
git worktree remove .git-worktrees/{prefix}-{slug}

# Step 2: Delete branch (optional, if no longer needed)
git branch -d {prefix}/{slug}

# Example:
git worktree remove .git-worktrees/feat-phase-6-1
git branch -d feat/phase-6-1
```

**Before removing:** Consolidate important session logs to main repo (see Memory Isolation below)

### Cleanup Stale References

```bash
# Remove references to deleted worktrees
git worktree prune

# Use after manually deleting worktree directories
```

### Force Remove (if needed)

```bash
# If worktree directory already deleted or corrupted
git worktree remove --force .git-worktrees/{prefix}-{slug}
```

## Memory Isolation

### Critical Warning

**Each worktree has independent `.claude/memory/` directory**

**What this means:**

- Session logs and work agreements stay in worktree (not shared with main repo)
- Deleting worktree = losing all session history (memory not automatically transferred)

### Before Removing Worktree

**Consolidate important session logs:**

```bash
# In worktree directory
cd .git-worktrees/feat-phase-6-1/

# Copy important session logs to main repo
cp .claude/memory/sessions/2025-10-23-development-feat-phase-6-1-session-log.md \
   ../../.claude/memory/sessions/

# Copy work artifacts if needed
cp -r .claude/memory/development/phase-6-1/ \
      ../../.claude/memory/development/

# Then safe to remove worktree
cd ../..
git worktree remove .git-worktrees/feat-phase-6-1
```

## Common Workflows

### Parallel Phase Implementation

Work on multiple phases simultaneously without conflicts:

```bash
# Terminal 1: Phase 6.1
git worktree add .git-worktrees/feat-phase-6-1/ -b feat/phase-6-1
bash scripts/worktree/setup.sh ".git-worktrees/feat-phase-6-1" "feat/phase-6-1" "developer"
cd .git-worktrees/feat-phase-6-1/

# Terminal 2: Phase 6.2 (simultaneously)
git worktree add .git-worktrees/feat-phase-6-2/ -b feat/phase-6-2
bash scripts/worktree/setup.sh ".git-worktrees/feat-phase-6-2" "feat/phase-6-2" "developer"
cd .git-worktrees/feat-phase-6-2/
```

### Exploration While Feature Work Continues

Experiment with new approaches without affecting main work:

```bash
# Main repo: Continue with current feature (feat/current-feature)
# Separate terminal: Create exploration worktree
git worktree add .git-worktrees/experiment-new-approach/ -b experiment/new-approach
bash scripts/worktree/setup.sh ".git-worktrees/experiment-new-approach" "experiment/new-approach" "general"
cd .git-worktrees/experiment-new-approach/
```

### Hotfix While Feature Development Active

Handle urgent fixes without disrupting feature work:

```bash
# Currently on feat/large-feature with uncommitted work
# Create hotfix worktree from main
git worktree add .git-worktrees/hotfix-security-fix/ -b hotfix/security-fix
cd .git-worktrees/hotfix-security-fix/
# Fix, commit, push, create PR, then return to feature work (cd ../../)
```

## Troubleshooting

### Worktree Already Exists

**Error:**

```text
fatal: '.git-worktrees/feat-phase-6-1' already exists
```

**Solutions:**

**Option 1: Remove existing worktree**

```bash
git worktree remove .git-worktrees/feat-phase-6-1/
git worktree add .git-worktrees/feat-phase-6-1/ -b feat/phase-6-1
```

**Option 2: Use different slug**

```bash
git worktree add .git-worktrees/feat-phase-6-1a/ -b feat/phase-6-1a
```

### Branch Already Exists

**Error:**

```text
fatal: a branch named 'feat/phase-6-1' already exists
```

**Solutions:**

**Option 1: Use existing branch**

```bash
git worktree add .git-worktrees/feat-phase-6-1/ feat/phase-6-1
```

**Option 2: Delete old branch first**

```bash
git branch -d feat/phase-6-1  # or -D to force
git worktree add .git-worktrees/feat-phase-6-1/ -b feat/phase-6-1
```

### Worktree Directory Deleted Manually

**Problem:** Deleted `.git-worktrees/feat-phase-6-1/` directory directly

**Symptom:**

```bash
git worktree list
# Shows worktree but directory doesn't exist
```

**Solution:**

```bash
# Remove stale reference
git worktree prune

# Or force remove specific worktree
git worktree remove --force .git-worktrees/feat-phase-6-1
```

## Best Practices

**Do:**

- Use standard naming convention (`.git-worktrees/{prefix}-{slug}/`)
- Run setup script after creating worktree
- Consolidate session logs before removing worktree
- Clean up worktrees after PR merged

**Don't:**

- Manually delete worktree directories (use `git worktree remove`)
- Create too many worktrees (harder to manage)
- Merge with active work-agreement (run complete-work first)

**When to use worktrees:**

- Large parallel work (multiple phases simultaneously)
- Long-running experiments while feature work continues
- Hotfixes while feature branch has uncommitted work
- Reviewing multiple PRs simultaneously
