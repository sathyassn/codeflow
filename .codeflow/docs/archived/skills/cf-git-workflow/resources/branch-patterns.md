# Branch Patterns

**Purpose:** Quick reference for validating branch names against taskflow conventions. Use when verifying current branch or checking if branch name follows correct pattern.

## Valid Patterns

### Common Patterns (Quick Reference)

| Pattern | Work Type | Example |
|---------|-----------|---------|
| `plan/*` | Planning | `plan/agent-decision-autonomy` |
| `feat/*` | Feature development | `feat/progressive-disclosure` |
| `fix/*` | Bug fixes | `fix/memory-leak-session-logs` |
| `docs/*` | Documentation | `docs/decision-framework` |
| `chore/*` | Maintenance, dependencies | `chore/update-markdownlint` |
| `experiment/*` | Exploratory/POC | `experiment/websocket-integration` |

### All Valid Prefixes (20 total)

`feature`, `feat`, `fix`, `bugfix`, `hotfix`, `release`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`, `build`, `ci`, `revert`, `plan`, `merge`, `experiment`, `wip`, `refine`

**Notes:**

- `feature` is an alias for `feat` (git-flow compatibility)
- `release`, `experiment`, and `wip` are branch-only (not valid commit types)
- All others are valid for both branches and commits

**See:** `.codeflow/config/enforcement/enforcement-policy.json` for authoritative list (git_format.branch_types)

## Protected Patterns

**CRITICAL:** Never work directly on these branches:

- `main` - Primary protected branch
- `master` - Legacy protected branch

## Validation Quick Check

```bash
# Valid examples
plan/oauth-integration
feat/skills-system-foundation
fix/context-overflow
docs/update-workflow-guide
experiment/new-approach

# Invalid examples
main (protected branch)
master (protected branch)
add-feature (missing type prefix)
Feature/AddThing (capitalized)
fix_bug (underscores instead of hyphens)
```
