# Symbol Index Schema Reference

## Overview

Tree-sitter builds a symbol index for fast lookups without requiring a language server. The index is stored in `.state/symbol-index/`.

## Index Structure

```text
.state/symbol-index/
├── index.db           # SQLite database
├── files.json         # Indexed file list
└── last-update        # Timestamp
```

## Index Tables

### symbols

| Field | Type | Description |
|-------|------|-------------|
| id | INTEGER | Primary key |
| name | TEXT | Symbol name |
| kind | TEXT | function, class, method, variable, etc. |
| file | TEXT | File path |
| line | INTEGER | Line number |
| column | INTEGER | Column number |
| parent_id | INTEGER | Parent symbol (for methods) |
| signature | TEXT | Function signature (if applicable) |

### symbol_fts

Full-text search virtual table for fuzzy symbol lookup.

| Field | Type | Description |
|-------|------|-------------|
| name | TEXT | Symbol name (searchable) |
| file | TEXT | File path (searchable) |

## Accessing the Index

**Never query the index directly.** Use cf-code-exploration operations:

| Need | Operation |
|------|-----------|
| Find by name | `cf-code-exploration:search-symbol` |
| Get dependencies | `cf-code-exploration:analyze-dependencies` |
| Architecture view | `cf-code-exploration:load-compressed-context` |

## Index Maintenance

### Building Index

Index is built automatically when needed. To force rebuild:

```bash
.codeflow/scripts/code/rebuild-index.sh
```

### Incremental Updates

On file change, only affected symbols are updated:

```bash
.codeflow/scripts/code/update-index.sh "{file}"
```

### Index Invalidation

Index is invalidated when:

- File modified after last index update
- Project structure changes (new directories)
- Manual rebuild requested

## Symbol Kinds

| Kind | Languages | Example |
|------|-----------|---------|
| function | All | `def foo()`, `function bar()` |
| class | Python, TS, JS | `class MyClass` |
| method | All | `def method(self)` |
| variable | All | `const x`, `var y` |
| interface | TypeScript | `interface IFoo` |
| type | TypeScript, Go | `type Foo`, `type MyType` |
| struct | Go, Rust | `type Foo struct` |
| enum | Rust, TypeScript | `enum Color` |
| trait | Rust | `trait MyTrait` |

## Code Maps

`cf-code-exploration:load-compressed-context` generates a compressed code map:

```text
src/
├── auth/
│   ├── login.ts
│   │   ├── function authenticate(user, pass)
│   │   ├── function validateToken(token)
│   │   └── class AuthService
│   └── logout.ts
│       └── function logout(session)
```

This provides architecture overview with minimal token usage.

## Troubleshooting

### Index Out of Date

Run: `.codeflow/scripts/code/rebuild-index.sh`

### Missing Symbols

1. Check file is in indexed paths
2. Verify file type is supported
3. Check for syntax errors in file

### Slow Searches

1. Ensure FTS index is built
2. Consider narrowing search scope
3. Check index size (may need pruning)
