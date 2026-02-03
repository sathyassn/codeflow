# LSP Configuration Reference

## Overview

LSP (Language Server Protocol) provides semantic code navigation. When available, it offers the most accurate results for definition and reference lookups.

## Supported Language Servers

| Language | Server | Package |
|----------|--------|---------|
| Python | pylsp / pyright | python-lsp-server / pyright |
| TypeScript/JavaScript | tsserver | typescript |
| Go | gopls | gopls |
| Rust | rust-analyzer | rust-analyzer |

## Checking LSP Availability

Use `cf-code-exploration:select-search-strategy` which automatically detects LSP availability.

The operation uses `.codeflow/scripts/code/check-lsp.sh` to:

1. Detect file language
2. Check if language server is installed
3. Verify server is responsive
4. Return availability status

## Fallback Chain

When LSP unavailable, operations fall back automatically:

```text
LSP → Tree-sitter → Grep
```

| Tool | Accuracy | Speed | Availability |
|------|----------|-------|--------------|
| LSP | High (semantic) | Medium | Requires server |
| Tree-sitter | Medium (syntactic) | Fast | Always available |
| Grep | Low (text match) | Fast | Always available |

## Configuring Language Servers

### Python (pylsp)

```bash
pip install python-lsp-server
```

Configuration: `.codeflow/config/pylsp.json`

### TypeScript (tsserver)

```bash
npm install -g typescript
```

Configuration: `tsconfig.json` in project root

### Go (gopls)

```bash
go install golang.org/x/tools/gopls@latest
```

Configuration: `.codeflow/config/gopls.json`

### Rust (rust-analyzer)

Install via rustup or download binary.

Configuration: `.codeflow/config/rust-analyzer.json`

## Troubleshooting

### LSP Not Responding

1. Check server is installed: `.codeflow/scripts/code/check-lsp.sh {language}`
2. Verify server can start: Direct invocation
3. Check project configuration files

### Slow LSP Responses

1. Check project size (large projects may be slow)
2. Consider excluding directories in server config
3. Use Tree-sitter for quick lookups

### Incorrect Results

1. Ensure project is properly configured (tsconfig, pyproject, etc.)
2. Rebuild project indexes
3. Restart language server

## When to Use Each Tool

| Task | Recommended |
|------|-------------|
| Precise definition | LSP (or Tree-sitter fallback) |
| All references | LSP (or Grep fallback) |
| Architecture overview | Tree-sitter code map |
| Quick symbol lookup | Tree-sitter FTS |
| Exact string search | Grep |
