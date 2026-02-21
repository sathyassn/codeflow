---
name: cf-markdown-standards
description: Markdown documentation standards, templates, lint rules, and validation reference. Loaded on-demand by cf-documentation when working with .md files.
---

# Markdown Standards Skill

## Type

**Procedural** -- On-demand reference for markdown documentation quality.

## Purpose

**Provides document type classification, template selection, structural validation, and lint rules for all markdown files in the project.**

## Responsibilities

- Define document type classification and template requirements
- Specify frontmatter fields per document type
- Enforce markdown lint rules and style conventions
- Provide anti-pattern identification and correction guidance
- NOT: Document creation (cf-documentation SOPs)
- NOT: Lint tooling execution (agent runs markdownlint directly)
- NOT: Git operations on documentation (cf-git-operations)

## Decision Tree

```text
Working with .md file:
├── New document? → 🔧 classify-document → 🔧 apply-template
├── Editing existing? → 🔧 validate-structure
└── Before commit? → 🔧 validate-structure → 🔧 fix-violations (if needed)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | classify-document | ENF-L3 Advisory | Determine document type from path and purpose |
| 2 | apply-template | ENF-L3 Advisory | Apply correct frontmatter and structure for type |
| 3 | validate-structure | ENF-L3 Advisory | Check lint rules and style conventions |
| 4 | fix-violations | ENF-L3 Advisory | Correct anti-patterns with proper approaches |

## Operation Details

### 🔧 classify-document

```text
When: Creating a new .md file or determining the type of an existing document
Purpose: Identify the correct document type to apply the right template and frontmatter
Enforcement: ENF-L3 Advisory

Document Types:

  | Type | File Pattern | Required Sections | Frontmatter |
  |------|-------------|-------------------|-------------|
  | ADR | *-adr.md | Status, Context, Decision, Consequences | id, title, status, date, deciders |
  | Brief | *-brief.md | Summary, Scope, Requirements, Timeline | id, title, status, author, created |
  | Epic | project-management/epics/**/*-epic.md | Summary, Tasks, Acceptance Criteria | id, title, status, area_type, work_type |
  | Task | project-management/epics/**/tasks/*.md | Description, Status, Blockers | id, epic_id, title, status, work_type |
  | Runbook | *-runbook.md | Prerequisites, Steps, Rollback | id, title, status, author, last_tested |
  | Guide | *-guide.md | Overview, Sections, References | title, status |
  | Skill | .claude/skills/*/SKILL.md | Type, Purpose, Operations or Sections | name, description |
  | Agent | .claude/agents/cf-*.md | Identity, Constraints, SOPs, Communication | name, description |

Quick Reference Decision Tree:

  New .md file needed?
    +-- Has alternatives analysis + decision? --> ADR
    +-- Analyzes a problem or presents findings? --> Brief
    +-- Defines a large work item with sub-tasks? --> Epic
    +-- Defines a single work item? --> Task
    +-- Describes an operational procedure with rollback? --> Runbook
    +-- Teaches how to do something step-by-step? --> Guide
    +-- None of the above? --> Check path pattern in Document Types table

Procedure:
  1. Check the file path against the File Pattern column
  2. If path matches a pattern, that is the document type
  3. If no path match, use the Quick Reference Decision Tree above
  4. If still ambiguous, default to Guide type

Output: Document type classification (ADR, Brief, Epic, Task, Runbook, Guide, Skill, Agent)

For the full decision flowchart with edge cases, load: resources/document-type-decision-tree.md
```

### 🔧 apply-template

```text
When: After classifying a new document, or verifying frontmatter on an existing document
Purpose: Apply the correct frontmatter fields and locate the template for the document type
Enforcement: ENF-L3 Advisory

Frontmatter Requirements:

  All typed documents require YAML frontmatter between --- delimiters at the top of the file.

  Universal fields: title, status

  Type-specific fields:

  | Field | ADR | Brief | Epic | Task | Runbook |
  |-------|-----|-------|------|------|---------|
  | id | required | required | required | required | required |
  | date/created | date | created, updated | created_at | created_at | created, updated |
  | author | - | required | - | - | required |
  | status values | proposed/accepted/deprecated/superseded | draft/review/approved | draft/planning/in_progress/complete | todo/blocked/in_progress/complete | active/retired |
  | area_type | - | - | required | required | - |
  | work_type | - | - | required | required | - |
  | epic_id | - | optional | - | required | - |

Templates:

  | Template | Path |
  |----------|------|
  | ADR | resources/templates/adr-template.md |
  | Brief | resources/templates/brief-template.md |
  | Epic | project-management/templates/epic-template.md |
  | Task | project-management/templates/task-template.md |
  | Runbook | resources/templates/runbook-template.md |

Procedure:
  1. Look up the document type (from classify-document) in the Frontmatter Requirements table
  2. Include all required fields in the YAML frontmatter
  3. Read the corresponding template from the Templates table
  4. Follow the template structure for required sections and ordering

Output: Document with correct frontmatter and section structure per type
```

### 🔧 validate-structure

```text
When: After editing a .md file, or before committing markdown changes
Purpose: Check the document against lint rules and style conventions
Enforcement: ENF-L3 Advisory

Active Lint Rules:

  | Rule | Check | Severity | Auto-fixable |
  |------|-------|----------|-------------|
  | MD003 | Heading style: use # (atx), not underlines | Error | No |
  | MD007 | List indentation: 2 spaces | Error | No |
  | MD009 | No trailing spaces | Warning | Yes |
  | MD010 | No hard tabs | Warning | Yes |
  | MD012 | No multiple consecutive blank lines | Warning | Yes |
  | MD022 | Blank line before heading | Error | No |
  | MD023 | Heading start at beginning of line | Error | No |
  | MD032 | Blank line around lists | Error | No |
  | MD034 | No bare URLs (wrap in angle brackets or link syntax) | Warning | Yes |
  | MD036 | No emphasis as heading (use ### not **bold**) | Error | No |
  | MD040 | Code fences must specify language | Error | Yes |
  | MD047 | File ends with single newline | Warning | Yes |
  | MD051 | Link fragment references must match headings | Error | No |

Disabled Rules:

  | Rule | Reason |
  |------|--------|
  | MD013 | Line length not enforced |
  | MD033 | Inline HTML allowed |
  | MD041 | First line h1 not required (frontmatter) |

Style Guide:

  | Guideline | Rule |
  |-----------|------|
  | Voice | Active voice ("The hook blocks..." not "The operation is blocked by...") |
  | Tense | Present tense ("Creates a branch" not "Will create a branch") |
  | Sentences | Concise, one idea per sentence |
  | Headings | Title case for H1, sentence case for H2+ |
  | Heading hierarchy | Never skip levels (H1 then H3 without H2) |
  | Emphasis | Use *italic* and **bold**, never _underscores_ |
  | Code fences | Always specify language (bash, json, text, etc.) |
  | Lists | 2-space indentation for nesting |
  | Links | Relative paths for internal links, descriptive text (not "click here") |
  | Tables | Use for structured comparisons; align columns with pipes |
  | TOC | Required for documents with more than 3 sections |
  | Final newline | Every file ends with exactly one newline |

Procedure:
  1. Run markdownlint against the file (or review manually)
  2. Check the Active Lint Rules table for any violations
  3. Verify Style Guide conventions are followed
  4. If violations found, proceed to fix-violations operation

Output: List of violations found (or clean pass)

For lint rule before/after examples and fix patterns, load: resources/lint-rules-quick-ref.md
```

### 🔧 fix-violations

```text
When: After validate-structure identifies violations or anti-patterns
Purpose: Correct common anti-patterns using the proper approach
Enforcement: ENF-L3 Advisory

Anti-Patterns and Corrections:

  | Anti-Pattern | Correct Approach |
  |-------------|-----------------|
  | **Bold text as heading** on its own line | Use proper heading: ### Heading text |
  | Bare URLs: https://example.com | Wrap: [Example](https://example.com) or <https://example.com> |
  | Skipped heading levels (H1 to H3) | Use sequential levels: H1, H2, H3 |
  | Hard tabs for indentation | Use spaces (2 for lists, 4 for code) |
  | Multiple consecutive blank lines | Single blank line between sections |
  | Code fence without language | Always specify: ```bash, ```json, ```text |
  | Placeholder text left in final doc | Replace all {placeholder} tokens with real content |
  | Trailing whitespace | Remove trailing spaces from all lines |
  | Inconsistent list markers | Use - for unordered lists throughout |

Auto-fixable Rules (can use markdownlint --fix):
  MD009, MD010, MD012, MD034, MD040, MD047

Non-auto-fixable Rules (require manual correction):
  MD003, MD007, MD022, MD023, MD032, MD036, MD051

Procedure:
  1. Review the violations from validate-structure
  2. For auto-fixable rules, run markdownlint --fix
  3. For non-auto-fixable rules, apply corrections from the Anti-Patterns table
  4. Re-run validate-structure to confirm all violations resolved

Output: Clean document with all violations resolved
```

## Resources

Companion resources provide expanded detail beyond the operation summaries above. Load when the inline guidance is insufficient for the task at hand.

| Resource | Companion To | Contains |
|----------|-------------|----------|
| `resources/document-type-decision-tree.md` | classify-document | Full decision flowchart with edge cases |
| `resources/lint-rules-quick-ref.md` | validate-structure, fix-violations | Lint rule before/after examples, fix patterns |
| `resources/templates/` | apply-template | Documentation templates (ADR, brief, runbook) |
| `project-management/templates/` | apply-template | Project management templates (epic, task) |
