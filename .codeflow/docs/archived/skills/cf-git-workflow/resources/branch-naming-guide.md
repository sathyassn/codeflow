# Branch Naming Guide

**Purpose:** Guidelines for creating properly formatted branch names with correct type prefix and slug formatting. Reference when creating new branches.

**These naming rules are enforced by git hooks** (`pre-commit`). Invalid branch names will block commits.

## Branch Types

| Type | Purpose | Example |
|------|---------|---------|
| `feature/` | New features or capabilities | `feature/add-worktree-isolation` |
| `feat/` | New features (short form) | `feat/progressive-disclosure` |
| `fix/` | Bug fixes during development | `fix/validation-edge-case` |
| `bugfix/` | Bug fixes (alternative) | `bugfix/resolve-hook-error` |
| `hotfix/` | Urgent production fixes | `hotfix/security-patch-cve-2024` |
| `release/` | Release preparation | `release/v1.2.0` |
| `docs/` | Documentation updates | `docs/update-workflow-guide` |
| `refactor/` | Code restructuring | `refactor/simplify-hooks` |
| `test/` | Test additions or updates | `test/add-commit-validation` |
| `chore/` | Maintenance, dependencies | `chore/update-markdownlint` |
| `perf/` | Performance improvements | `perf/optimize-git-checks` |
| `style/` | Formatting and code style | `style/fix-markdown-lint` |
| `build/` | Build system changes | `build/update-npm-scripts` |
| `ci/` | CI/CD pipeline changes | `ci/add-github-actions` |
| `revert/` | Revert previous changes | `revert/bad-commit-abc123` |
| `plan/` | Planning and design | `plan/agent-decision-autonomy` |
| `merge/` | Conflict resolution | `merge/integrate-feature` |
| `experiment/` | Experimental or POC work | `experiment/new-workflow` |

**Total:** 18 branch types available

## Format Structure

```text
type/description-with-hyphens
```

## Naming Rules

**REQUIRED:**

- Use lowercase with hyphens (kebab-case)
- Keep names concise but descriptive (3-5 words max)
- Use `/` separator between type and description
- Use `-` separator between words in description

## Examples

**Good branch names:**

```text
feat/add-memory-persistence
fix/commit-msg-validation
docs/add-branch-naming-guide
plan/oauth-integration
experiment/new-workflow-approach
```

**Bad branch names:**

```text
add-feature
   Missing type prefix

Feature/AddNewThing
   Capitalized, camelCase

fix_bug_in_validation
   Underscores instead of hyphens

feat/this-is-a-very-long-descriptive-name-that-goes-on-forever
   Too long (keep to 3-5 words)
```

## Creating Good Slugs

**Process:**

1. Identify work type - determines prefix (plan/, feat/, fix/, docs/, experiment/)
2. Describe work in 3-5 words - "add memory persistence"
3. Convert to kebab-case - "add-memory-persistence"
4. Combine - "feat/add-memory-persistence"

**Tips:**

- Be specific but concise
- Focus on WHAT not HOW
- Use verbs for actions (add, fix, update, remove)
- Avoid generic terms (updates, changes, work)
