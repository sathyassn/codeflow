# Markdownlint Rules Quick Reference

**Purpose:** Essential markdownlint rules configured for this project

**Config:** `.markdownlint.json`

## Active Rules

| Rule | Setting | What it checks |
|------|---------|----------------|
| MD003 | `atx` | Heading style (use `#` not underlines) |
| MD007 | indent: 2 | List indentation (2 spaces) |
| MD013 | disabled | Line length (disabled - no limit enforced) |
| MD024 | siblings_only | Duplicate headings (OK if not siblings) |
| MD046 | `fenced` | Code block style (use triple backticks) |
| MD049 | `asterisk` | Emphasis style (use `*` not `_`) |
| MD050 | `asterisk` | Strong style (use `**` not `__`) |

## Disabled Rules

| Rule | Why disabled |
|------|--------------|
| MD033 | Allow inline HTML (needed for some formatting) |
| MD034 | Allow bare URLs |
| MD041 | Don't require first line to be h1 (YAML frontmatter) |

## Common Errors & Fixes

### MD007: Unordered list indentation

```markdown
Wrong (4 spaces):
- Item
    - Nested

Correct (2 spaces):
- Item
  - Nested
```

### MD009: Trailing spaces

```markdown
Wrong: Line with trailing spaces
Correct: Line with no trailing spaces
```

### MD012: Multiple consecutive blank lines

```markdown
Wrong:
Section 1


Section 2

Correct:
Section 1

Section 2
```

### MD022/MD032: Headings/lists need blank lines

```markdown
Wrong:
## Heading
Content here

Correct:
## Heading

Content here
```

### MD036: Emphasis used instead of heading

Standalone bold text that should be a heading.

```markdown
Wrong:
**Step 1: Configure Settings**

Do the configuration...

Correct:
#### Step 1: Configure Settings

Do the configuration...
```

**Common patterns requiring conversion:**

- `**Part N: Title**` - `### Part N: Title`
- `**Step N: Action**` - `#### Step N: Action`
- `**Option A: Name**` - `#### Option A: Name`
- `**Phase N: Description**` - `#### Phase N: Description`

### MD040: Code fence missing language

All code fences must specify a language identifier.

```markdown
Wrong:
` ` `
directory/
├── file.txt
└── other.txt
` ` `

Correct:
` ` `text
directory/
├── file.txt
└── other.txt
` ` `
```

**Common language identifiers:**

| Content Type | Language |
|--------------|----------|
| Directory structures | `text` |
| Shell commands | `bash` |
| Markdown examples | `markdown` |
| JSON config | `json` |
| YAML config | `yaml` |
| Generic output | `text` |

### MD051: Link fragments (TOC links)

Table of contents links that don't match heading anchors.

```markdown
Flagged (often false positive):
- [Section Name](#section-name)

Fix for false positives:
<!-- markdownlint-disable-next-line MD051 -->
- [Section Name](#section-name)
```

**Note:** MD051 often triggers false positives for valid TOC links. Verify the heading exists before adding disable comments.

### Nested code fences (showing markdown examples)

When documenting code fence syntax, use 4 backticks for the outer fence:

````markdown
Wrong (3 backticks both - conflicts):
` ` `markdown
## Example
` ` `
content
` ` `
` ` `

Correct (4 backticks outer):
` ` ` `markdown
## Example
` ` `text
content
` ` `
` ` ` `
````

## Auto-fixable vs Manual

**Auto-fixable** (pre-commit hook):

- MD009 (trailing spaces)
- MD010 (hard tabs)
- MD012 (multiple blank lines)
- MD040 (code fence language - defaults to `text`)
- MD047 (file should end with newline)

**Manual fix required:**

- MD007 (list indentation structure)
- MD022/MD032 (blank line around headings/lists)
- MD024 (duplicate headings - rename them)
- MD036 (emphasis as heading - convert `**Bold:**` to `### Heading`)
- MD051 (link fragments - fix anchor or add disable comment)
