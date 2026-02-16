# Document Type Decision Tree

> Companion resource for the **classify-document** operation in the cf-markdown-standards skill.

**Purpose:** Determine the correct template and standard for new `.md` files.

**Rule:** Use this decision tree BEFORE creating any new `.md` file. Do not guess the document type.

## Quick Decision Flowchart

```text
START: Creating new .md file
       |
       v
+-------------------------------------+
|     WHERE is the file located?      |
+-------------------------------------+
       |
       +--- .claude/skills/{name}/SKILL.md
       |    --> TYPE: Skill (use skill frontmatter)
       |
       +--- .claude/agents/cf-{name}.md
       |    --> TYPE: Agent definition (5-section format)
       |
       +--- project-management/epics/**/*-epic.md
       |    --> TYPE: Epic (use epic-template.md)
       |
       +--- project-management/epics/**/tasks/*.md
       |    --> TYPE: Task (use task-template.md)
       |
       +--- *-adr.md
       |    --> TYPE: ADR (use adr-template.md)
       |
       +--- *-brief.md
       |    --> TYPE: Brief (use brief-template.md)
       |
       +--- *-runbook.md
       |    --> TYPE: Runbook (use runbook-template.md)
       |
       +--- *-guide.md
       |    --> TYPE: Guide
       |
       +--- Other location
            |
            v
      +-------------------------------------+
      |  WHAT is the document's purpose?    |
      |  (Use decision questions below)     |
      +-------------------------------------+
```

## Decision Questions (When Location Is Ambiguous)

Answer these questions in order. Stop at the first "YES."

### Q1: Does it record an architecture decision with alternatives considered?

- **YES** --> **ADR** -- Use `adr-template.md`
- **NO** --> Continue to Q2

### Q2: Does it analyze a problem or present findings/proposals?

- **YES** --> **Brief** -- Use `brief-template.md`
- **NO** --> Continue to Q3

### Q3: Does it define a large work item with sub-tasks?

- **YES** --> **Epic** -- Use `epic-template.md`
- **NO** --> Continue to Q4

### Q4: Does it define a single actionable work item?

- **YES** --> **Task** -- Use `task-template.md`
- **NO** --> Continue to Q5

### Q5: Does it describe an operational procedure with rollback steps?

- **YES** --> **Runbook** -- Use `runbook-template.md`
- **NO** --> Continue to Q6

### Q6: Does it teach how to do something step-by-step?

- **YES** --> **Guide** -- Structure: Overview, Prerequisites, Sections, Troubleshooting
- **NO** --> **General document** -- Apply standard markdown conventions

## Template/Standard Quick Reference

| Type | File Pattern | Template | Required Sections |
|------|-------------|----------|-------------------|
| ADR | `*-adr.md` | `templates/adr-template.md` | Status, Context, Decision, Consequences |
| Brief | `*-brief.md` | `templates/brief-template.md` | Summary, Scope, Requirements, Timeline |
| Epic | `*-epic.md` | `templates/epic-template.md` | Summary, Tasks, Acceptance Criteria |
| Task | `tasks/*.md` | `templates/task-template.md` | Description, Status, Blockers |
| Runbook | `*-runbook.md` | `templates/runbook-template.md` | Prerequisites, Steps, Rollback |
| Guide | `*-guide.md` | (standard conventions) | Overview, Sections, References |

## Framework vs Guide: Critical Distinction

This is the most common misclassification.

| Question | Framework | Guide |
|----------|-----------|-------|
| Does it explain WHY something is designed this way? | Yes | No |
| Does it teach step-by-step HOW to do something? | No | Yes |
| Is it meant to be read once for understanding? | Yes | No |
| Is it meant to be referenced repeatedly for steps? | No | Yes |
| Does it define patterns others should follow? | Yes | No |

## After Type Selection

1. Load the identified template
2. Note all required sections
3. Create the document following the template structure
4. Add YAML frontmatter with required metadata fields
5. Validate structure and lint the result
