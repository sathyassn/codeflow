---
name: cf-git-workflow
description: Provides git operations for branching, commits, PRs, worktrees, and remote sync. Enforces branch naming conventions and commit message formats. Use before any code changes and after work completion.
context: fork
agent: cf-general-purpose
---

# Git Workflow Skill

## Type

**Procedural** - Provides step-by-step git operations with validation.

## Purpose

**Ensures git operations follow project conventions, preventing commit failures and maintaining repository integrity.**

## Responsibilities

- Create branches with proper naming conventions
- Create conventional commits with required format
- Manage pull request creation and format
- Create and cleanup worktrees for parallel work
- Sync with remote repositories safely
- Merge branches with conflict detection
- NOT: Code review (that's cf-reviewer agent)
- NOT: Work tracking (that's cf-memory-management)

## Decision Tree

```text
Starting new work?
└── 🔧 create-branch (prefix matches work type)

Ready to commit?
├── Has complete-work sentinel? → 🔧 create-commit
└── No sentinel? → cf-memory-management:complete-work first

Submitting for review?
├── Has sandbox-check sentinel? → 🔧 create-pull-request
└── No sentinel? → cf-security-management:sandbox-check first

Need parallel isolation?
└── 🔧 create-worktree (check scope conflicts)

Syncing with remote?
└── 🔧 sync-remote (requires sandbox-check)

Checking current state?
├── Branch info → 🔧 check-branch-status
└── Changes summary → 🔧 review-changes
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | create-branch | ENF-L1 Sentinel | Create feature/fix/etc branch |
| 2 | create-commit | ENF-L1 Sentinel | Create conventional commit |
| 3 | create-pull-request | ENF-L1 Sentinel | Create PR with required format |
| 4 | create-worktree | ENF-L1 Sentinel | Create isolated worktree |
| 5 | cleanup-worktrees | ENF-L1 Sentinel | Remove stale worktrees |
| 6 | sync-remote | ENF-L1 Sentinel | Push/pull operations |
| 7 | merge-branch | ENF-L1 Sentinel | Merge with strategy |
| 8 | review-changes | None | Display diff summary |
| 9 | check-branch-status | None | Branch state check |
| 10 | rebase-interactive | ENF-L1 Sentinel | Interactive rebase |

## Operation Details

### 🔧 create-branch

```text
When: Starting new work item
Enforcement: ENF-L1 Sentinel
Prerequisite: None

Procedure:
  1. Validate branch prefix matches work type:
     | Work Type | Prefix | Alternative |
     |-----------|--------|-------------|
     | FEAT | feat/ | feature/ |
     | FIX | fix/ | bugfix/ |
     | RFCT | refactor/ | - |
     | DOCS | docs/ | - |
     | TEST | test/ | - |
     | HTFX | hotfix/ | - |
     | CHOR | chore/ | - |
     | CICD | ci/ | build/ |
     | SPKE | spike/ | experiment/ |
     | PERF | perf/ | - |
     | STYLE | style/ | - |
     | REVERT | revert/ | - |
     | PLAN | plan/ | - |
     | RELEASE | release/ | - |
     | MERGE | merge/ | - |

  2. Generate branch name: {prefix}{task-slug}
     Example: feat/auth-jwt-tokens

  3. Create branch from base:
     git checkout -b {branch-name}

  4. Update active work registry

Output:
  branch: {created branch name}
  base: {parent branch}
  task_id: {associated task}

Cross-skill: cf-task-management (for task association)
Hook: PreToolUse/pre-tool-use-bash-sentinel.sh

📚 Resource: [branch-naming-guide.md](resources/branch-naming-guide.md)
   Load when: Unsure of branch type or naming convention
```

### 🔧 create-commit

```text
When: Committing changes (after complete-work)
Enforcement: ENF-L1 Sentinel
Prerequisite: cf-memory-management:complete-work (creates sentinel)

CRITICAL: Sentinel from complete-work required before commit

Autorun Mode Behavior:
  If $AUTORUN_SESSION_ID is set:
    - Check task's auto_commit setting (default: TRUE in autorun)
    - If auto_commit=TRUE: Proceed with commit automatically
    - If auto_commit=FALSE: Skip commit, let user handle
    - Commit message should reference task: "feat(task-id): description"

Procedure:
  1. Verify complete-work sentinel exists (TTL: 600s)

  2. In autorun: Check task.auto_commit setting
     - Query via cf-db-operations or $AUTORUN_AUTO_COMMIT env var

  3. Stage memory files if modified:
     - .state/memory/*.jsonl (if exists)
     - project-management/epics/**/*.md (task markdown updates)

  4. Validate commit message format:
     type(scope): description

     Valid types (16 total):
     | Type | Purpose |
     |------|---------|
     | feat | New feature or capability |
     | fix | Bug fix |
     | bugfix | Bug fix (alias) |
     | hotfix | Urgent production fix |
     | docs | Documentation only |
     | refactor | Code restructuring |
     | test | Add/update tests |
     | chore | Maintenance, deps |
     | style | Formatting only |
     | perf | Performance improvement |
     | build | Build system changes |
     | ci | CI/CD pipeline changes |
     | revert | Revert previous changes |
     | merge | Merge conflict resolution |
     | plan | Planning documents |
     | refine | Process improvements |

     Scope: optional component/area (use task_id in autorun)
     Description: imperative mood, no period, max 50 chars

  5. Check for staged changes

  6. Execute commit:
     git commit -m "{message}"

  7. Record in memory via cf-db-operations:memory-store

Memory File Staging:
  | File Pattern | Stage If | Purpose |
  |--------------|----------|---------|
  | .state/memory/*.jsonl | Modified | Memory ledger sync |
  | project-management/epics/**/*.md | Modified | Task status updates |
  | .claude/memory/** | Modified | Session context |

Output:
  commit_sha: {short sha}
  message: {commit message}
  files_changed: {count}
  memory_files_staged: [{list}]
  autorun_auto_commit: true | false (if in autorun)

On Failure:
  - If no sentinel: BLOCK with "complete-work required first"
  - If sentinel expired: BLOCK with "sentinel expired, re-run complete-work"
  - If no staged changes: WARN "nothing to commit"

Cross-skill: cf-memory-management:complete-work (prerequisite)
Hook: PreToolUse/pre-tool-use-bash-sentinel.sh

📚 Resources:
   [commit-format.md](resources/commit-format.md) - Load when: Unsure of commit type or format requirements
   [commit-examples.md](resources/commit-examples.md) - Load when: Need example commit messages
```

### 🔧 create-pull-request

```text
When: Submitting work for review
Enforcement: ENF-L1 Sentinel
Prerequisite: cf-security-management:sandbox-check (for network)

Autorun Mode Behavior:
  If $AUTORUN_SESSION_ID is set:
    - Check task's raise_pr setting (default: TRUE)
    - Use $AUTORUN_TARGET_BRANCH for base branch (default: main)
    - Check task's auto_merge setting (default: FALSE)
    - Include acceptance criteria checklist in PR body
    - Output "AUTORUN_COMPLETE: PR #<number>" for orchestrator detection

Procedure:
  1. Verify sandbox-check sentinel exists

  2. In autorun: Check task settings
     - raise_pr: Should PR be created? (default TRUE)
     - target_branch: Base branch from $AUTORUN_TARGET_BRANCH
     - auto_merge: Request auto-merge if TRUE

  3. Validate PR format:
     - Title: type(scope): description (≤50 chars)
     - Body: ## Summary + ## Test Plan sections
     - In autorun: Add ## Acceptance Criteria checklist
     - No AI attribution patterns

  4. Check for uncommitted changes (warn if present)

  5. Execute PR creation:
     gh pr create --title "{title}" --body "{body}" --base {target_branch}

  6. If auto_merge=TRUE:
     gh pr merge --auto --squash {pr_number}

  7. Record PR URL in memory via cf-db-operations:memory-store

  8. In autorun: Output completion marker
     echo "AUTORUN_COMPLETE: PR #{number}"

Output:
  pr_url: {GitHub PR URL}
  pr_number: {number}
  title: {PR title}
  target_branch: {base branch}
  auto_merge_requested: true | false

On Failure:
  - If format invalid: BLOCK with format guidance
  - If AI attribution detected: BLOCK with removal instructions
  - If network blocked: Guide to sandbox-check

Cross-skill: cf-security-management:sandbox-check (prerequisite)
Hook: PreToolUse/pre-tool-use-gh-pr.sh

📚 Resource: [pr-format-guide.md](resources/pr-format-guide.md)
   Load when: Unsure of PR title/body format or required sections
```

### 🔧 create-worktree

```text
When: Parallel work isolation needed
Enforcement: ENF-L1 Sentinel
Prerequisite: cf-memory-management:search-related-work (conflict check)

Procedure:
  1. Run search-related-work for scope conflicts
  2. If conflicts found: WARN with conflict details
  3. Create worktree:
     git worktree add ../worktrees/{branch} -b {branch}
  4. Register worktree in work registry
  5. Set up shared state symlink

Output:
  worktree_path: {path to worktree}
  branch: {worktree branch}
  conflicts: [{any scope conflicts}]

Cross-skill: cf-memory-management:search-related-work

📚 Resource: [worktree-guide.md](resources/worktree-guide.md)
   Load when: Setting up worktree, troubleshooting conflicts, or managing memory isolation
```

### 🔧 cleanup-worktrees

```text
When: Worktrees no longer needed
Enforcement: ENF-L1 Sentinel

Procedure:
  1. List worktrees with status
  2. For each stale worktree:
     - Check for uncommitted changes
     - Check for unpushed commits
     - Prompt if changes exist
  3. Remove worktree:
     git worktree remove {path}
  4. Update work registry

Output:
  removed: [{worktree paths}]
  skipped: [{paths with changes}]

📚 Resource: [worktree-guide.md](resources/worktree-guide.md)
   Load when: Cleaning up worktrees or consolidating memory before removal
```

### 🔧 sync-remote

```text
When: Pushing/pulling changes
Enforcement: ENF-L1 Sentinel
Prerequisite: cf-security-management:sandbox-check (for network)

Procedure:
  1. Verify sandbox-check sentinel
  2. Determine operation (push/pull)
  3. For push:
     - Check for local commits
     - Execute: git push -u origin {branch}
  4. For pull:
     - Check for local changes
     - Execute: git pull --rebase
  5. Record sync in memory

Output:
  operation: push | pull
  branch: {branch name}
  commits: {count synced}
```

### 🔧 merge-branch

```text
When: Merging feature branch to target
Enforcement: ENF-L1 Sentinel (via bash-sentinel pattern: ^git merge)
Prerequisite: cf-security-management:sandbox-check

Procedure:
  1. Verify sandbox-check sentinel
  2. Validate target branch (not protected without approval)
  3. Check for merge conflicts:
     git merge --no-commit --no-ff {source}
  4. If conflicts:
     - List conflicting files
     - Abort merge
     - BLOCK with conflict resolution guidance
  5. If clean:
     - Complete merge: git merge {source} --no-ff
  6. Record merge in memory

Output:
  merged: true | false
  source: {source branch}
  target: {target branch}
  conflicts: [{conflicting files if any}]
  commits_merged: {count}

On Failure:
  - If protected branch: BLOCK with "protected branch, use PR"
  - If conflicts: BLOCK with conflict file list
```

### 🔧 review-changes

```text
When: Reviewing uncommitted or branch changes
Enforcement: None (read-only operation)

Procedure:
  1. Determine review scope:
     - Uncommitted: git diff + git diff --staged
     - Branch: git diff {base}...{head}
  2. Generate summary:
     - Files changed count
     - Insertions/deletions
     - Changed file list with status
  3. Format as readable summary

Output:
  scope: uncommitted | branch
  files_changed: {count}
  insertions: {count}
  deletions: {count}
  files: [{path, status, changes}]
  summary: {human-readable summary}

Note: This is an informational operation, no sentinel required.
```

### 🔧 check-branch-status

```text
When: Checking current branch state
Enforcement: None (read-only operation)

Procedure:
  1. Get current branch: git branch --show-current
  2. Check tracking: git status -sb
  3. Count ahead/behind commits
  4. Check for uncommitted changes
  5. Check for stashed changes

Output:
  branch: {current branch}
  tracking: {remote/branch or null}
  ahead: {commits ahead of remote}
  behind: {commits behind remote}
  uncommitted: {true/false}
  staged: {file count}
  unstaged: {file count}
  stashed: {stash count}
  clean: {true if nothing to commit}

Note: This is an informational operation, no sentinel required.
```

### 🔧 rebase-interactive

```text
When: Cleaning up commit history before PR
Enforcement: ENF-L1 Sentinel (via bash-sentinel pattern: ^git rebase)
Prerequisite: cf-memory-management:search-related-work (conflict awareness)

IMPORTANT: Interactive rebase is NOT supported in automated contexts.
Use non-interactive rebase or squash merge instead.

Procedure:
  1. Check for interactive context availability
  2. If automated/non-interactive:
     - Use git rebase --autosquash for fixup commits
     - Or recommend squash merge at PR time
  3. If interactive available:
     - Count commits to rebase
     - Execute: git rebase -i {base}
  4. Handle conflicts if any
  5. Record rebase in memory

Output:
  rebased: true | false
  base: {base commit/branch}
  commits_affected: {count}
  conflicts: [{if any}]

On Failure:
  - If interactive not available: Suggest alternatives
  - If conflicts: BLOCK with resolution guidance
  - If force-push needed: WARN about history rewrite

Cross-skill: cf-memory-management:search-related-work
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [branch-naming-guide.md](resources/branch-naming-guide.md) | Branch naming conventions | When creating branches |
| [commit-format.md](resources/commit-format.md) | Commit message format | When creating commits |
| [commit-examples.md](resources/commit-examples.md) | Commit message examples | When unsure of format |
| [pr-format-guide.md](resources/pr-format-guide.md) | PR format requirements | When creating PRs |
| [worktree-guide.md](resources/worktree-guide.md) | Worktree management | When using parallel isolation |
