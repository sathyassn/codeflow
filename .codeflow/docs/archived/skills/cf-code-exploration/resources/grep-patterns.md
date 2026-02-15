# Grep Patterns Reference

## Overview

When LSP and Tree-sitter are unavailable, grep patterns provide text-based code search. These patterns are language-specific to improve accuracy.

## Using Grep Fallback

Grep is used automatically by `cf-code-exploration:navigate-to-definition` and `cf-code-exploration:find-all-references` when semantic tools unavailable.

**Do not construct grep patterns manually.** The operations use pre-built patterns from `.codeflow/scripts/code/grep-patterns/`.

## Pattern Files by Language

| Language | Pattern File |
|----------|--------------|
| Python | `.codeflow/scripts/code/grep-patterns/python.sh` |
| TypeScript | `.codeflow/scripts/code/grep-patterns/typescript.sh` |
| JavaScript | `.codeflow/scripts/code/grep-patterns/javascript.sh` |
| Go | `.codeflow/scripts/code/grep-patterns/go.sh` |
| Rust | `.codeflow/scripts/code/grep-patterns/rust.sh` |
| Shell | `.codeflow/scripts/code/grep-patterns/shell.sh` |

## Pattern Categories

### Definition Patterns

Find where a symbol is defined:

| Language | Matches |
|----------|---------|
| Python | `def {symbol}`, `class {symbol}`, `async def {symbol}` |
| TypeScript | `function {symbol}`, `class {symbol}`, `const {symbol}`, `interface {symbol}` |
| Go | `func {symbol}`, `type {symbol}`, `var {symbol}` |
| Rust | `fn {symbol}`, `struct {symbol}`, `enum {symbol}`, `trait {symbol}` |

### Reference Patterns

Find where a symbol is used:

| Language | Matches |
|----------|---------|
| Python | `{symbol}(`, `{symbol}.`, `import {symbol}` |
| TypeScript | `{symbol}(`, `{symbol}<`, `{symbol}.`, `new {symbol}` |
| Go | `{symbol}(`, `{symbol}.`, `{symbol}{` |
| Rust | `{symbol}(`, `{symbol}::`, `{symbol}.` |

## Limitations

Grep patterns have limitations compared to semantic tools:

| Issue | Example |
|-------|---------|
| False positives | Matches in comments/strings |
| Missed renames | Aliased imports not found |
| No type info | Can't distinguish overloads |
| Cross-file | Requires searching all files |

## Improving Accuracy

The grep scripts include filters to reduce false positives:

- Exclude comment lines
- Exclude string contents (best effort)
- Exclude test files (optional)
- Exclude generated files

## Custom Patterns

To add patterns for additional languages:

1. Create pattern file: `.codeflow/scripts/code/grep-patterns/{language}.sh`
2. Define `definition_pattern()` function
3. Define `reference_pattern()` function
4. Register in `.codeflow/config/languages.json`
