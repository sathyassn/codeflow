---
name: cf-markdown-standards
description: Markdown documentation standards, templates, lint rules, and validation reference. Loaded on-demand by cf-documentation when working with .md files.
context: on-demand
---

# Markdown Standards Reference

## Type

**Procedural** -- On-demand reference for markdown documentation quality.

## Purpose

Quick reference for document types, frontmatter requirements, lint rules, style conventions, and templates. Loaded by cf-documentation during documentation work.

## Document Types

| Type | File Pattern | Required Sections | Frontmatter |
|------|-------------|-------------------|-------------|
| ADR | `*-adr.md` | Status, Context, Decision, Consequences | id, title, status, date, deciders |
| Brief | `*-brief.md` | Summary, Scope, Requirements, Timeline | id, title, status, author, created |
| Epic | `project-management/epics/**/*-epic.md` | Summary, Tasks, Acceptance Criteria | id, title, status, area_type, work_type |
| Task | `project-management/epics/**/tasks/*.md` | Description, Status, Blockers | id, epic_id, title, status, work_type |
| Runbook | `*-runbook.md` | Prerequisites, Steps, Rollback | id, title, status, author, last_tested |
| Guide | `*-guide.md` | Overview, Sections, References | title, status |
| Skill | `.claude/skills/*/SKILL.md` | Type, Purpose, Operations or Sections | name, description, context |
| Agent | `.claude/agents/cf-*.md` | Identity, Constraints, SOPs, Communication | name, description |

## Frontmatter Requirements

All typed documents require YAML frontmatter between `---` delimiters at the top of the file.

**Universal fields:** `title`, `status`

**Type-specific fields:**

| Field | ADR | Brief | Epic | Task | Runbook |
|-------|-----|-------|------|------|---------|
| id | required | required | required | required | required |
| date/created | date | created, updated | created_at | created_at | created, updated |
| author | - | required | - | - | required |
| status values | proposed/accepted/deprecated/superseded | draft/review/approved | draft/planning/in_progress/complete | todo/blocked/in_progress/complete | active/retired |
| area_type | - | - | required | required | - |
| work_type | - | - | required | required | - |
| epic_id | - | optional | - | required | - |

## Markdown Lint Rules

### Active Rules

| Rule | Check | Severity | Auto-fixable |
|------|-------|----------|-------------|
| MD003 | Heading style: use `#` (atx), not underlines | Error | No |
| MD007 | List indentation: 2 spaces | Error | No |
| MD009 | No trailing spaces | Warning | Yes |
| MD010 | No hard tabs | Warning | Yes |
| MD012 | No multiple consecutive blank lines | Warning | Yes |
| MD022 | Blank line before heading | Error | No |
| MD023 | Heading start at beginning of line | Error | No |
| MD032 | Blank line around lists | Error | No |
| MD034 | No bare URLs (wrap in angle brackets or link syntax) | Warning | Yes |
| MD036 | No emphasis as heading (use `###` not `**bold**`) | Error | No |
| MD040 | Code fences must specify language | Error | Yes |
| MD047 | File ends with single newline | Warning | Yes |
| MD051 | Link fragment references must match headings | Error | No |

### Disabled Rules

| Rule | Reason |
|------|--------|
| MD013 | Line length not enforced |
| MD033 | Inline HTML allowed |
| MD041 | First line h1 not required (frontmatter) |

**Resources:** [lint-rules-quick-ref.md](resources/lint-rules-quick-ref.md) for before/after examples.

## Style Guide

| Guideline | Rule |
|-----------|------|
| Voice | Active voice ("The hook blocks..." not "The operation is blocked by...") |
| Tense | Present tense ("Creates a branch" not "Will create a branch") |
| Sentences | Concise, one idea per sentence |
| Headings | Title case for H1, sentence case for H2+ |
| Heading hierarchy | Never skip levels (H1 then H3 without H2) |
| Emphasis | Use `*italic*` and `**bold**`, never `_underscores_` |
| Code fences | Always specify language (`bash`, `json`, `text`, etc.) |
| Lists | 2-space indentation for nesting |
| Links | Relative paths for internal links, descriptive text (not "click here") |
| Tables | Use for structured comparisons; align columns with pipes |
| TOC | Required for documents with more than 3 sections |
| Final newline | Every file ends with exactly one newline |

## Anti-Patterns

| Anti-Pattern | Correct Approach |
|-------------|-----------------|
| `**Bold text as heading**` on its own line | Use proper heading: `### Heading text` |
| Bare URLs: `https://example.com` | Wrap: `[Example](https://example.com)` or `<https://example.com>` |
| Skipped heading levels (H1 to H3) | Use sequential levels: H1, H2, H3 |
| Hard tabs for indentation | Use spaces (2 for lists, 4 for code) |
| Multiple consecutive blank lines | Single blank line between sections |
| Code fence without language | Always specify: ` ```bash `, ` ```json `, ` ```text ` |
| Placeholder text left in final doc | Replace all `{placeholder}` tokens with real content |
| Trailing whitespace | Remove trailing spaces from all lines |
| Inconsistent list markers | Use `-` for unordered lists throughout |

## Quick Reference: Document Type Selection

```text
New .md file needed?
  |
  +-- Has alternatives analysis + decision? --> ADR
  +-- Analyzes a problem or presents findings? --> Brief
  +-- Defines a large work item with sub-tasks? --> Epic
  +-- Defines a single work item? --> Task
  +-- Describes an operational procedure with rollback? --> Runbook
  +-- Teaches how to do something step-by-step? --> Guide
  +-- None of the above? --> Check path pattern in Document Types table
```

**Resources:** [document-type-decision-tree.md](resources/document-type-decision-tree.md) for the full decision flowchart.

## Templates

| Template | Path |
|----------|------|
| ADR | [resources/templates/adr-template.md](resources/templates/adr-template.md) |
| Brief | [resources/templates/brief-template.md](resources/templates/brief-template.md) |
| Epic | [resources/templates/epic-template.md](resources/templates/epic-template.md) |
| Task | [resources/templates/task-template.md](resources/templates/task-template.md) |
| Runbook | [resources/templates/runbook-template.md](resources/templates/runbook-template.md) |
