# Scope Pattern Reference

## Pattern Syntax

File scope uses glob patterns stored as JSON arrays.

### Basic Patterns

| Pattern | Description | Example Matches |
|---------|-------------|-----------------|
| `*` | Any single path segment | `src/*` matches `src/file.ts` |
| `**` | Any number of path segments | `src/**` matches `src/a/b/c.ts` |
| `?` | Any single character | `file?.ts` matches `file1.ts` |
| `[abc]` | Character class | `[st]rc` matches `src` or `trc` |
| `{a,b}` | Alternation | `*.{ts,js}` matches `.ts` or `.js` |

### Common Scope Patterns

```json
// Single directory, all files
["src/auth/*"]

// Directory tree, specific extension
["src/auth/**/*.ts"]

// Multiple directories
["src/auth/**/*", "src/utils/**/*"]

// Include tests with source
["src/feature/**/*.ts", "tests/feature/**/*.test.ts"]

// Exclude patterns (prefix with !)
["src/**/*.ts", "!src/generated/**"]
```

## Scope Policy Modes

| Mode | Description | Set By |
|------|-------------|--------|
| soft | Scope can be expanded by developer | Default |
| hard | Scope cannot be expanded | cf-planner |
| permissive | No scope enforcement | cf-planner (spikes) |

## Setting Scope

**Do not set scope directly.** Scope is established through:

### At Task Creation

```text
cf-task-management:create-task
→ Sets initial file_scope from task definition
→ Sets scope_policy
```

### At Work Start

```text
cf-memory-management:begin-work
→ Copies task scope to active_work
→ Validates against scope_policy
```

### Expanding Scope (soft policy only)

```text
cf-task-management:update-task
→ Validates policy allows expansion
→ Records scope change event
→ Updates both task and active_work
```

## Scope Validation

When an edit is attempted, the system validates:

1. File path matches at least one include pattern
2. File path doesn't match any exclude patterns
3. Policy allows the operation

Validation is handled by `.codeflow/scripts/security/validate-scope.sh`.

## Conflict Detection

Before starting work, check for scope conflicts:

```text
cf-memory-management:search-related-work
→ Finds active work with overlapping scope
→ Returns conflict warnings
```

## Best Practices

1. **Be specific** - Use narrow patterns for focused work
2. **Include tests** - Add test patterns alongside source
3. **Avoid wildcards at root** - `**/*` is too broad
4. **Use exclusions sparingly** - Prefer positive patterns
5. **Check conflicts first** - Run search-related-work before begin-work
