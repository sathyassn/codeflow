# Markdownlint Rules Quick Reference

**Purpose:** Detailed lint rule explanations with before/after examples.

**Config:** `.markdownlint.json`

## Active Rules

### MD003: Heading style (atx)

Use `#` headings, not underline-style.

```markdown
Wrong:
Heading
=======

Correct:
# Heading
```

### MD007: List indentation (2 spaces)

Nested lists use 2-space indentation.

```markdown
Wrong (4 spaces):
- Item
    - Nested

Correct (2 spaces):
- Item
  - Nested
```

### MD009: Trailing spaces

No trailing whitespace at end of lines.

```markdown
Wrong: Line with trailing spaces
Correct: Line with no trailing spaces
```

**Auto-fixable:** Yes -- remove trailing spaces.

### MD010: Hard tabs

No hard tab characters. Use spaces.

```markdown
Wrong: [TAB]indented content
Correct:    indented content (spaces)
```

**Auto-fixable:** Yes -- convert tabs to spaces.

### MD012: Multiple consecutive blank lines

Only one blank line between sections.

```markdown
Wrong:
Section 1


Section 2

Correct:
Section 1

Section 2
```

**Auto-fixable:** Yes -- collapse to single blank line.

### MD022: Blank line before heading

Headings require a blank line above them.

```markdown
Wrong:
Some content
## Heading

Correct:
Some content

## Heading
```

### MD023: Heading start at beginning of line

Headings must not be indented.

```markdown
Wrong:
  ## Indented Heading

Correct:
## Heading
```

### MD032: Blank line around lists

Lists require blank lines before and after.

```markdown
Wrong:
Some content
- Item 1
- Item 2
More content

Correct:
Some content

- Item 1
- Item 2

More content
```

### MD034: Bare URLs

Wrap URLs in link syntax or angle brackets.

```markdown
Wrong:
Visit https://example.com for details.

Correct:
Visit [Example](https://example.com) for details.
Visit <https://example.com> for details.
```

**Auto-fixable:** Yes -- wrap in angle brackets.

**Note:** MD034 is disabled in the current `.markdownlint.json` config. This rule is enforced at the style-guide level, not the linter level.

### MD036: Emphasis as heading

Standalone bold text that acts as a section title should use a proper heading.

```markdown
Wrong:
**Step 1: Configure Settings**

Do the configuration...

Correct:
#### Step 1: Configure Settings

Do the configuration...
```

**Common patterns requiring conversion:**

| Pattern | Convert To |
|---------|-----------|
| `**Part N: Title**` | `### Part N: Title` |
| `**Step N: Action**` | `#### Step N: Action` |
| `**Option A: Name**` | `#### Option A: Name` |
| `**Phase N: Description**` | `#### Phase N: Description` |

### MD040: Code fence language

All code fences must specify a language identifier.

````markdown
Wrong:
```
directory/
├── file.txt
└── other.txt
```

Correct:
```text
directory/
├── file.txt
└── other.txt
```
````

**Common language identifiers:**

| Content Type | Language |
|--------------|----------|
| Directory structures | `text` |
| Shell commands | `bash` |
| Markdown examples | `markdown` |
| JSON config | `json` |
| YAML config | `yaml` |
| Generic output | `text` |
| Python code | `python` |
| SQL queries | `sql` |

**Auto-fixable:** Yes -- defaults to `text` if unspecified.

### MD047: File ends with newline

Every file must end with exactly one newline character.

**Auto-fixable:** Yes -- append newline if missing.

### MD051: Link fragment references

Table-of-contents links must match actual heading anchors.

```markdown
Flagged (often false positive):
- [Section Name](#section-name)

Fix for false positives:
<!-- markdownlint-disable-next-line MD051 -->
- [Section Name](#section-name)
```

**Note:** Verify the heading exists before adding disable comments. MD051 often triggers false positives for valid TOC links.

## Disabled Rules

| Rule | Setting | Reason |
|------|---------|--------|
| MD013 | disabled | Line length not enforced |
| MD033 | disabled | Inline HTML allowed for formatting |
| MD041 | disabled | First line h1 not required (YAML frontmatter comes first) |

## Additional Active Rules (Configuration Defaults)

| Rule | Setting | Description |
|------|---------|-------------|
| MD024 | siblings_only | Duplicate headings OK if not at the same level under same parent |
| MD046 | fenced | Code blocks must use triple backticks (not indentation) |
| MD049 | asterisk | Emphasis uses `*`, not `_` |
| MD050 | asterisk | Strong uses `**`, not `__` |

## Auto-fixable vs Manual Summary

**Auto-fixable** (safe to apply automatically):

- MD009 -- trailing spaces
- MD010 -- hard tabs
- MD012 -- multiple blank lines
- MD034 -- bare URLs (wrap in angle brackets)
- MD040 -- code fence language (default to `text`)
- MD047 -- final newline

**Manual fix required:**

- MD003 -- heading style (restructure headings)
- MD007 -- list indentation (restructure nesting)
- MD022/MD032 -- blank lines around headings/lists (add blank lines)
- MD023 -- heading indentation (remove leading spaces)
- MD024 -- duplicate headings (rename them)
- MD036 -- emphasis as heading (convert `**Bold**` to `### Heading`)
- MD051 -- link fragments (fix anchor or add disable comment)

## Nested Code Fences

When documenting code fence syntax, use 4 backticks for the outer fence:

`````markdown
Wrong (3 backticks both -- conflicts):
```markdown
## Example
```
content
```
```

Correct (4 backticks outer):
````markdown
## Example
```text
content
```
````
`````
