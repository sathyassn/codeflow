# Documentation Standards - Quick Reference

**CRITICAL:** Apply when creating or editing .md files

**Purpose:** Essential documentation standards for creating compliant markdown files

**For complete standard:** See `docs/standards/documentation-standards.md`

## Visual Markers - Essential Set

**Tier 1: Critical Markers (MUST use)**

- **CRITICAL:** System-breaking if ignored, must follow exactly
- **REQUIRED:** Important for correctness, should not skip
- **CHECKPOINT:** Verification point, stop and verify
- **SAFE TO PROCEED:** Validation passed, green light

**Format standard:**

```markdown
**CRITICAL:** Follow exact procedure - no exceptions
**CHECKPOINT:** Run verification before proceeding
**SAFE TO PROCEED:** All checks passed
```

**Tier 2: Context Markers (use when applicable)**

- **OPERATION:** Skill operation reference
- **OPTIONAL:** Enhancement, nice to have
- **USER INTERACTION:** Requires user input

## Document Structure Essentials

**Heading hierarchy rules:**

- H1 only for document title
- Never skip levels (H2 to H4 is wrong, must go H2 to H3 to H4)
- H2 for major sections
- H3 for subsections
- H4 for granular details
- Avoid H5 except in very complex specifications

**Purpose statements:**

Every major section (H2) needs:

```markdown
## Part N: [Topic Name]

**Purpose:** [1-2 sentences: what this covers]. [1-2 sentences: why it matters].
```

**Table of Contents patterns:**

- Simple docs (<500 lines): Basic list
- Moderate docs (500-1000 lines): Flat list with descriptions
- Comprehensive docs (>1000 lines): Part-grouped structure

## Reference Formats

**File references:**

- `path/to/file.md:123` (always include line number)
- `functionName in path/to/file.ts:456`
- WRONG: `src/utils/validation.ts` (missing line number)

**Cross-document references:**

```markdown
**See:** [Document Name - Section](path#anchor) for [what you'll find]
```

**External links:**

```markdown
[Descriptive Text](https://example.com/docs)
```

**Link verification:** Test links before committing, update or remove broken links (404s)

## Code Blocks

**Always specify language:**

```bash
git status
```

```yaml
key: value
```

```markdown
# Example markdown
```

**For placeholders/templates:**

```text
<required-placeholder>
[optional-placeholder]
{choice1|choice2}
```

## Tables

**Alignment rules:**

- Left-align text columns (names, descriptions)
- Center-align status markers
- Right-align numbers (lines, percentages, dates)

**Example:**

```markdown
| Feature | Status | Lines |
|---------|:------:|------:|
| Auth | Complete | 150 |
| API | In Progress | - |
```

## Length Guidelines

**Target lengths:**

- Purpose statements: 1-2 sentences
- Descriptions: 2-3 sentences
- Section introductions: 2-4 sentences
- Examples: As short as possible while being clear

**Avoid:**

- Fluff and redundancy
- Overly verbose explanations (break into bullets)
- Being too terse (loses context)

## Quick Verification Checklist

Before committing .md files, verify:

- [ ] Visual markers present and correctly formatted
- [ ] Heading hierarchy correct (no skipped levels)
- [ ] File references include line numbers (file.ts:123)
- [ ] Code blocks have language labels
- [ ] Tables have proper alignment
- [ ] Purpose statements present for major sections
- [ ] Links verified (no 404s)
- [ ] Appropriate length (not too verbose, not too terse)
