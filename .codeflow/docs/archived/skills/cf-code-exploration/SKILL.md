---
name: cf-code-exploration
description: Provides code navigation using LSP-first approach with Tree-sitter and grep fallbacks. Enables efficient symbol lookup, dependency analysis, and codebase understanding. Use when exploring unfamiliar code or finding implementations.
context: fork
agent: cf-general-purpose
---

# Code Exploration Skill

## Type

**Procedural** - Provides step-by-step procedures for code navigation and search.

## Purpose

**Enables efficient code navigation using semantic tools (LSP, Tree-sitter) with intelligent fallback to pattern matching.**

## Responsibilities

- Select optimal search strategy for query type
- Load compressed code context via Tree-sitter
- Search symbols using FTS index
- Analyze dependencies via symbol graph
- Navigate to precise definitions (LSP)
- Find all references across codebase
- NOT: Code modification (that's developer agents)
- NOT: Memory persistence (that's cf-memory-management)

## Decision Tree

```text
Need architecture overview?
└── 🔧 load-compressed-context (Tree-sitter code map)

Finding symbol by name?
└── 🔧 search-symbol (Tree-sitter FTS)

Impact analysis needed?
└── 🔧 analyze-dependencies (symbol graph)

Need precise definition location?
└── 🔧 navigate-to-definition (LSP → Tree-sitter → grep)

Finding all usages?
└── 🔧 find-all-references (LSP → Tree-sitter → grep)

Unsure which tool?
└── 🔧 select-search-strategy (routes to optimal)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | select-search-strategy | ENF-L3 Advisory | Route to optimal tool (Tree-sitter/LSP/Vector/Grep) |
| 2 | load-compressed-context | ENF-L3 Advisory | Load Tree-sitter code map for architecture |
| 3 | search-symbol | ENF-L3 Advisory | Fast symbol lookup via Tree-sitter FTS |
| 4 | analyze-dependencies | ENF-L3 Advisory | Build dependency graph via Tree-sitter |
| 5 | navigate-to-definition | ENF-L1 Sentinel | LSP or Read file at line |
| 6 | find-all-references | ENF-L1 Sentinel | LSP find_references or Grep |

## Operation Details

### 🔧 select-search-strategy

```text
When: Before any code exploration
Purpose: Route to optimal tool based on query type
Enforcement: ENF-L3 Advisory

Decision Matrix:
  | Query Pattern | Primary Tool | Script/Tool | Index |
  |---------------|--------------|-------------|-------|
  | Architecture overview | Tree-sitter | get_code_map() | Tree-sitter |
  | Find symbol by name | Tree-sitter | search_symbols() | Tree-sitter FTS |
  | Dependency analysis | Tree-sitter | get_symbol_graph() | Tree-sitter graph |
  | Precise navigation | LSP | go_to_definition() | Real-time |
  | All usages | LSP/Grep | find_references() | Real-time |
  | Semantic code search | Vector | Go CLI (planned) | Vector embeddings |
  | Exact string match | Grep/FTS | Go CLI (planned) | FTS5 |

Output:
  strategy: tree-sitter | lsp | vector | grep
  tool: {specific tool to use}
  rationale: {why this strategy}

📚 Resources:
   [lsp-configuration.md](resources/lsp-configuration.md) - Load when: LSP is the selected strategy
   [grep-patterns.md](resources/grep-patterns.md) - Load when: grep is the selected strategy
```

### 🔧 load-compressed-context

```text
When: Starting work on new area of codebase
Purpose: Load Tree-sitter code map for architecture overview
Enforcement: ENF-L3 Advisory

Token Budget Strategy:
  Total Context: 100%
  ├── Compressed map (Tree-sitter): 20-30%
  ├── Full files (active work): 40-50%
  ├── Conversation: 20-30%
  └── Response room: 10%

Procedure:
  1. Identify target directory/module
  2. Call Tree-sitter tool: get_code_map(path)
  3. Load compressed symbol index
  4. Identify key entry points

Output:
  code_map: {compressed architecture view}
  entry_points: [{symbol, file, purpose}...]
  token_usage: {estimated tokens}

📚 Resource: [symbol-index-schema.md](resources/symbol-index-schema.md)
   Load when: Understanding code map structure or debugging index issues
```

### 🔧 search-symbol

```text
When: Finding symbol by name
Purpose: Fast symbol lookup via Tree-sitter FTS
Enforcement: ENF-L3 Advisory

Procedure:
  1. Query Tree-sitter symbol index
  2. Use FTS for fuzzy matching
  3. Return ranked results

Output:
  matches: [{name, kind, file, line, score}...]
  exact_match: {if found}

📚 Resource: [symbol-index-schema.md](resources/symbol-index-schema.md)
   Load when: Understanding symbol FTS index or search ranking
```

### 🔧 analyze-dependencies

```text
When: Impact analysis before changes
Purpose: Build dependency graph via Tree-sitter
Enforcement: ENF-L3 Advisory

Procedure:
  1. Call Tree-sitter tool: get_symbol_graph(symbol)
  2. Build upstream/downstream graph
  3. Identify impact radius

Output:
  upstream: [{symbols that this depends on}...]
  downstream: [{symbols that depend on this}...]
  impact_radius: high | medium | low

📚 Resource: [symbol-index-schema.md](resources/symbol-index-schema.md)
   Load when: Understanding dependency graph structure
```

### 🔧 navigate-to-definition

```text
When: Need precise location of symbol definition
Enforcement: ENF-L1 Sentinel (via enforcement-policy.json grep-sentinel)

Procedure:
  1. Detect file language
  2. Check LSP availability:
     | Language | LSP Server |
     |----------|------------|
     | Python | pylsp / pyright |
     | TypeScript | tsserver |
     | Go | gopls |
     | Rust | rust-analyzer |

  3. If LSP available:
     - Send textDocument/definition request
     - Return file:line:column

  4. If LSP unavailable:
     - Use Tree-sitter symbol index
     - Fallback to Grep patterns

Output:
  location: {file}:{line}:{column}
  context: {surrounding code snippet}
  method: lsp | tree-sitter | grep

📚 Resources:
   [lsp-configuration.md](resources/lsp-configuration.md) - Load when: LSP not working for language
   [grep-patterns.md](resources/grep-patterns.md) - Load when: Using grep fallback patterns
```

### 🔧 find-all-references

```text
When: Need to find all usages of a symbol
Enforcement: ENF-L1 Sentinel (via enforcement-policy.json grep-sentinel)

Procedure:
  1. Detect file language
  2. If LSP available:
     - Send textDocument/references request
     - Return all locations

  3. If LSP unavailable:
     - Use Tree-sitter symbol graph
     - Fallback to Grep for symbol name

Output:
  references: [
    {file: path, line: number, context: snippet}...
  ]
  count: {total references}
  method: lsp | tree-sitter | grep

📚 Resources:
   [lsp-configuration.md](resources/lsp-configuration.md) - Load when: LSP not finding all references
   [grep-patterns.md](resources/grep-patterns.md) - Load when: Using grep fallback
```

## Tool Selection Matrix

| Query Type | Python | TypeScript | Shell | Other |
|------------|--------|------------|-------|-------|
| Where is X defined? | LSP | LSP | Grep | Grep |
| Where is X used? | LSP | LSP | Grep | Grep |
| What symbols exist? | LSP | LSP | Grep | Grep |
| Find text pattern | Grep | Grep | Grep | Grep |
| Find files by name | Glob | Glob | Glob | Glob |

## Grep Fallback Patterns

### Python

```text
Definition: (def|class|async def)\s+{symbol}\s*[\(:]
Import: (from\s+\S+\s+)?import\s+.*{symbol}
Reference: {symbol}\s*[\(.]
```

### TypeScript

```text
Definition: (function|class|const|let|var|interface|type|enum)\s+{symbol}
Import: import\s+.*{symbol}
Reference: {symbol}\s*[\(.<]
```

### Go

```text
Definition: (func|type|var|const)\s+{symbol}
Reference: {symbol}\s*[\(.]
```

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [lsp-configuration.md](resources/lsp-configuration.md) | LSP server setup | When LSP not working |
| [grep-patterns.md](resources/grep-patterns.md) | Language-specific patterns | When using grep fallback |
| [symbol-index-schema.md](resources/symbol-index-schema.md) | Index structure | When debugging index |
