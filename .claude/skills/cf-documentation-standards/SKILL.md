---
name: cf-documentation-standards
description: Markdown documentation standards: templates and validation.
context: fork
agent: cf-general-purpose
---

# Documentation Standards Skill

## Type

**Procedural** - Step-by-step procedures for documentation standards.

## Purpose

**Ensure all markdown documentation follows project standards through template selection and validation.**

## Responsibilities

- Guide template selection for new documents
- Apply correct document structure by type
- Validate markdown files against project rules

## Decision Tree

```text
START: What are you doing with a .md file?
    │
    ├─ Creating NEW .md file?
    │   └─ 🔧 apply-standard
    │       ├─ Determine document type
    │       ├─ Apply appropriate template
    │       └─ Verify with 🔧 lint-file
    │
    └─ Editing EXISTING .md file?
        └─ 🔧 lint-file (after edit)
```

## Operations

### 🔧 apply-standard

**When:** Creating any new .md file

**Purpose:** Apply correct template based on document type

**Procedure:**

1. Determine document type by location:
   - `.claude/skills/*/SKILL.md` → Skill template
   - `.claude/commands/*.md` → Command template
   - `.claude/agents/*.md` → Agent template
   - `project/**/*.md` → Project documentation
   - Other → General markdown

2. Apply template structure

3. Verify with 🔧 lint-file

### 🔧 lint-file

**When:** After creating or editing .md files

**Purpose:** Validate markdown formatting

**Procedure:**

```bash
# If markdownlint available
npx markdownlint --config .markdownlint.json <file-path>

# Or manual check:
# - ATX headers (# style)
# - Blank lines around blocks
# - Fenced code blocks have language
# - No trailing whitespace
```

## Document Templates

### Skill Document

```markdown
---
name: skill-name
description: Brief description.
context: inline|fork
---

# Skill Name

## Type
## Purpose
## Responsibilities
## Decision Tree
## Operations
## Resources
```

### Project Documentation

```markdown
# Title

## Overview
## Details
## Examples
## Related
```

## Key Conventions

- Use ATX-style headers (`#`, `##`)
- Include language identifier on code fences
- One blank line before/after headings
- One blank line before/after code blocks
- No trailing whitespace
- End file with single newline

## Resources

For document types: `resources/document-type-decision-tree.md`
For lint rules: `resources/lint-rules-quick-ref.md`
