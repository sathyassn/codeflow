---
name: cf-documentation-standards
description: Provides document quality enforcement including structure validation, frontmatter, and linting. Ensures consistent documentation across ADRs, briefs, epics, and tasks. Use when creating or editing documentation files.
context: fork
agent: cf-general-purpose
---

# Documentation Standards Skill

## Type

**Procedural** - Provides step-by-step documentation validation and templating.

## Purpose

**Ensures consistent documentation quality across all file types through template application and validation.**

## Responsibilities

- Apply document standards based on type
- Lint markdown files for quality issues
- Validate document structure against requirements
- Check for broken links
- Generate tables of contents
- NOT: Content creation (that's agents)
- NOT: Code documentation (that's inline comments)

## Decision Tree

```text
Creating new documentation?
└── 🔧 apply-standard (detect type, apply template)

Editing existing documentation?
└── 🔧 lint-file (after editing)

Before committing docs?
├── 🔧 validate-structure (check required sections)
└── 🔧 check-links (find broken links)

Long document needs navigation?
└── 🔧 generate-toc

Batch documentation check?
└── 🔧 lint-all (lint multiple files)

Found lint errors?
└── 🔧 fix-file (auto-fix where possible)
```

## Operations

| # | Operation | Enforcement | Purpose |
|---|-----------|-------------|---------|
| 1 | apply-standard | ENF-L1 Sentinel | Apply doc standards for file type |
| 2 | lint-file | ENF-L1 Sentinel | Check documentation quality |
| 3 | lint-all | ENF-L3 Advisory | Batch lint multiple files |
| 4 | fix-file | ENF-L1 Sentinel | Auto-fix lint errors |
| 5 | validate-structure | ENF-L3 Advisory | Verify structure compliance |
| 6 | check-links | None | Find broken links |
| 7 | generate-toc | None | Generate table of contents |

## Operation Details

### 🔧 apply-standard

```text
When: Creating new documentation file
Enforcement: ENF-L1 Sentinel

Procedure:
  1. Detect document type from path/name
  2. Load template from resources/
  3. Apply required sections
  4. Add frontmatter with metadata
  5. Validate structure

Templates:
  | Type | Template File |
  |------|---------------|
  | ADR | resources/templates/adr-template.md |
  | Brief | resources/templates/brief-template.md |
  | Runbook | resources/templates/runbook-template.md |
  | Epic | resources/templates/epic-template.md |
  | Task | resources/templates/task-template.md |

Output:
  file_path: {created file}
  type: {document type}
  sections: [{required sections added}]

📚 Resources:
   [document-type-decision-tree.md](resources/document-type-decision-tree.md) - Load when: Unsure which document type to use
   Templates in resources/templates/ - Load when: Creating specific document types
```

### 🔧 lint-file

```text
When: After editing documentation files
Enforcement: ENF-L1 Sentinel

Procedure:
  1. Run markdownlint on file
  2. Parse errors/warnings
  3. Categorize issues:
     - Structure (headings, lists)
     - Style (line length, trailing spaces)
     - Links (broken, relative)
  4. Report issues with line numbers

Output:
  errors: [{rule, line, message}...]
  warnings: [{rule, line, message}...]
  passed: true | false

📚 Resource: [lint-rules-quick-ref.md](resources/lint-rules-quick-ref.md)
   Load when: Understanding lint errors or deciding which rules to fix
```

### 🔧 lint-all

```text
When: Batch documentation check (pre-commit, CI)
Enforcement: ENF-L3 Advisory (read-only batch check; individual file edits gated by lint-file sentinel)

Procedure:
  1. Identify all markdown files in scope:
     - project/*.md
     - project-management/epics/**/*.md
     - .claude/skills/**/*.md
  2. Run markdownlint on each
  3. Aggregate results
  4. Report summary

Output:
  files_checked: {count}
  files_passed: {count}
  files_failed: {count}
  errors_by_file: {map of file -> errors}

📚 Resource: [lint-rules-quick-ref.md](resources/lint-rules-quick-ref.md)
   Load when: Interpreting batch lint results or prioritizing fixes
```

### 🔧 fix-file

```text
When: After lint-file finds fixable errors
Enforcement: ENF-L1 Sentinel (edits gated by lint-file sentinel in enforcement-policy.json)

Auto-fixable Issues:
  | Rule | Fix |
  |------|-----|
  | MD009 | Remove trailing spaces |
  | MD010 | Convert tabs to spaces |
  | MD012 | Remove multiple blank lines |
  | MD023 | Fix heading punctuation |
  | MD034 | Wrap bare URLs |
  | MD047 | Add final newline |

Procedure:
  1. Identify fixable issues from lint-file output
  2. Apply fixes in order (line-safe)
  3. Re-lint to verify fixes
  4. Report remaining issues

Output:
  fixed: [{rule, line, description}...]
  remaining: [{rule, line, message}...]
  passed: true | false

📚 Resource: [lint-rules-quick-ref.md](resources/lint-rules-quick-ref.md)
   Load when: Understanding which rules are auto-fixable
```

### 🔧 validate-structure

```text
When: Before commit
Enforcement: ENF-L3 Advisory (read-only validation; no sentinel hook)

Procedure:
  1. Parse document
  2. Extract existing sections
  3. Compare against required sections for type
  4. Report missing/extra sections

Output:
  valid: true | false
  missing: [{section names}]
  warnings: [{non-critical issues}]

📚 Resources:
   Templates in resources/templates/ - Load when: Need required sections for specific document type
   [document-type-decision-tree.md](resources/document-type-decision-tree.md) - Load when: Need document type rules
```

### 🔧 check-links

```text
When: Before commit
Enforcement: None

Procedure:
  1. Extract all links from document
  2. For each link:
     - Check if internal file exists
     - Validate anchor references
     - Check external URL accessibility (optional)
  3. Report broken links

Output:
  broken_links: [{link, reason}...]
  valid: true | false
```

### 🔧 generate-toc

```text
When: Long documents need table of contents
Enforcement: None

Procedure:
  1. Parse document headings
  2. Build hierarchical TOC structure
  3. Generate markdown TOC
  4. Insert at designated location

Output:
  toc: {generated table of contents}
  heading_count: {number of headings}
```

## Document Types

| Type | Pattern | Required Sections |
|------|---------|-------------------|
| ADR | `*-adr.md` | Status, Context, Decision, Consequences |
| Brief | `*-brief.md` | Summary, Scope, Requirements, Timeline |
| Runbook | `*-runbook.md` | Prerequisites, Steps, Rollback |
| Incident | `*-incident.md` | Summary, Timeline, Impact, Resolution |
| Epic | `project-management/epics/**/*-epic.md` | Summary, Tasks, Acceptance Criteria |
| Task | `project-management/epics/**/tasks/*.md` | Description, Status, Blockers |

## Operation Composition

Common operation sequences:

| Workflow | Operations |
|----------|------------|
| New doc | apply-standard → lint-file |
| Edit doc | (edit) → lint-file → fix-file (if needed) |
| Pre-commit | lint-all → validate-structure → check-links |
| Large doc | generate-toc → validate-structure |

## Resources

| Resource | Purpose | When to Load |
|----------|---------|--------------|
| [document-type-decision-tree.md](resources/document-type-decision-tree.md) | Document type selection guide | When creating new documentation |
| [lint-rules-quick-ref.md](resources/lint-rules-quick-ref.md) | Markdownlint rule explanations | When fixing lint errors |
| [templates/adr-template.md](resources/templates/adr-template.md) | ADR template with frontmatter | When creating ADRs |
| [templates/brief-template.md](resources/templates/brief-template.md) | Brief template with frontmatter | When creating briefs |
| [templates/epic-template.md](resources/templates/epic-template.md) | Epic template with DB fields | When creating epics |
| [templates/task-template.md](resources/templates/task-template.md) | Task template with DB fields | When creating tasks |
| [templates/runbook-template.md](resources/templates/runbook-template.md) | Runbook template | When creating runbooks |
