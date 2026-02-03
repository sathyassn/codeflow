# Commit Examples

**Purpose:** Concrete examples of good and bad commits with explanations. Reference when uncertain about proper formatting or when validating commit message quality.

**Git hooks (`commit-msg`, `pre-commit`) will block the bad examples shown below.**

## Good Commits

### Minimal (subject only)

```text
feat: add interactive rebase guide to git docs
```

**Why good:**

- Clear type (`feat`)
- Under 50 chars (45)
- Imperative mood ("add")
- Descriptive but concise

### With Body (3 bullets)

```text
docs: reorganize agent framework into 14 sections

- Split protocol into focused documents
- Add decision-making tiers with examples
- Document memory organization patterns
```

**Why good:**

- Subject under 50 chars (48)
- Body has 3 bullets (at limit)
- Each bullet under 72 chars
- Explains what and why

### Refactoring

```text
refactor: consolidate permission presets

- Move templates to .claude/settings-templates/
- Update configure script to use new location
- Preserve backward compatibility
```

**Why good:**

- Correct type for code restructuring
- Clear impact statements
- Mentions backward compatibility

## Bad Commits

### Subject Too Long

```text
docs: complete template restructure planning and protocol standardization work
```

**Why bad:** 78 characters (exceeds 50 char limit)

**Fix:**

```text
docs: restructure template and protocol
```

### AI Attribution

```text
feat: add worktree support

Generated with Claude Code
Co-Authored-By: Claude <noreply@anthropic.com>
```

**Why bad:** Contains AI attribution

**Fix:**

```text
feat: add worktree support

- Enable parallel work isolation
- Add worktree creation helpers
- Document worktree workflow
```

### Too Many Bullets

```text
feat: add agent framework

- Add 14-section structure standard
- Document memory organization
- Add decision-making tiers
- Add work agreements
- Update all agents to new format
- Create framework documentation
```

**Why bad:** 6 bullets (exceeds 3 bullet limit)

**Fix:**

```text
feat: add agent framework with 14-section standard

- Implement structure with memory and decision tiers
- Update all agents to follow new format
- Create comprehensive framework documentation
```

### Wrong Format

```text
Added new feature for agents
```

**Why bad:** Past tense, missing type prefix

**Fix:**

```text
feat: add agent support for new workflow
```

### Capitalized Type

```text
Feat: Add agent support
```

**Why bad:** Capitalized type and description

**Fix:**

```text
feat: add agent support
```

### Period at End

```text
feat: add agent support.
```

**Why bad:** Period at end of subject line

**Fix:**

```text
feat: add agent support
```
