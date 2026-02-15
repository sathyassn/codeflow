# Document Type Decision Tree

**Purpose:** Determine the correct template/standard for new .md files

**MANDATORY:** Use this decision tree BEFORE creating any new .md file

Do NOT guess the document type. Do NOT skip this step.

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
       |    --> TYPE: Skill
       |        --> docs/templates/skill-template.md
       |
       +--- .claude/commands/{name}.md
       |    --> TYPE: Command
       |        --> docs/templates/command-template.md
       |
       +--- .claude/agents/{name}.md (sub-agent)
       |    --> TYPE: Sub-agent
       |        --> docs/templates/sub-agent-template.md
       |
       +--- .claude/hooks/README.md or hook documentation
       |    --> TYPE: Hook
       |        --> docs/templates/hook-template.md
       |
       +--- .claude/CLAUDE.md
       |    --> TYPE: Main-agent
       |        --> docs/templates/main-agent-template.md
       |
       +--- docs/frameworks/*.md
       |    --> TYPE: Framework
       |        --> docs/standards/framework-documentation-standard.md
       |
       +--- docs/guides/*.md
       |    --> TYPE: Guide
       |        --> docs/standards/documentation-standards.md (Part 5)
       |
       +--- docs/standards/*.md
       |    --> TYPE: Standard
       |        --> docs/standards/documentation-standards.md (Part 5)
       |
       +--- docs/planning/*.md
       |    --> TYPE: Planning
       |        --> docs/standards/documentation-standards.md (Part 5)
       |
       +--- .claude/memory/**/*.md
       |    --> TYPE: Memory/Work artifact
       |        --> docs/standards/documentation-standards.md (minimal)
       |
       +--- Other location
            |
            v
      +-------------------------------------+
      |  WHAT is the document's purpose?    |
      |  (Use questions below)              |
      +-------------------------------------+
```

## Decision Questions (When Location Unclear)

If the file location doesn't immediately indicate type, answer these questions IN ORDER:

### Q1: Is this a Claude Code operational document?

- **YES** - Check if it's a skill, command, agent, or hook (see table below)
- **NO** - Continue to Q2

### Q2: Does it teach architecture, patterns, or "why" something works?

- **YES** - **TYPE: Framework**
  - MUST use `docs/standards/framework-documentation-standard.md`
  - MUST have "Part 1: Why This Architecture" section
- **NO** - Continue to Q3

### Q3: Does it teach "how to do" something step-by-step?

- **YES** - **TYPE: Guide**
  - Use `docs/standards/documentation-standards.md`
  - Structure: Overview - Prerequisites - Sections - Troubleshooting
- **NO** - Continue to Q4

### Q4: Does it define rules, conventions, or specifications?

- **YES** - **TYPE: Standard**
  - Use `docs/standards/documentation-standards.md`
  - Structure: Part 0 Quick Reference - Numbered Parts
- **NO** - Continue to Q5

### Q5: Is it analysis, proposal, or planning work?

- **YES** - **TYPE: Planning**
  - Use `docs/standards/documentation-standards.md`
  - Include: Status, Date, Summary, Analysis/Proposal sections
- **NO** - **TYPE: General**
  - Use `docs/standards/documentation-standards.md`

## Template/Standard Quick Reference

| Type | Location Pattern | Template/Standard | Key Required Sections |
|------|------------------|-------------------|----------------------|
| Skill | `.claude/skills/*/SKILL.md` | `docs/templates/skill-template.md` | YAML frontmatter, Type, Purpose, Operations, Resources |
| Command | `.claude/commands/*.md` | `docs/templates/command-template.md` | Purpose & Usage, Arguments & Flags, Workflow |
| Sub-agent | `.claude/agents/*.md` | `docs/templates/sub-agent-template.md` | Responsibilities, Decision Autonomy, Boundaries |
| Hook | `.claude/hooks/README.md` | `docs/templates/hook-template.md` | Event Type, Matcher, Implementation |
| Main-agent | `.claude/CLAUDE.md` | `docs/templates/main-agent-template.md` | Project Identity, Commands, Skills, Guidelines |
| Framework | `docs/frameworks/*.md` | `docs/standards/framework-documentation-standard.md` | **Part 1: Why (MANDATORY)**, TOC, References |
| Guide | `docs/guides/*.md` | `docs/standards/documentation-standards.md` | Overview, Prerequisites, Sections, Troubleshooting |
| Standard | `docs/standards/*.md` | `docs/standards/documentation-standards.md` | Part 0 Quick Ref, Numbered Parts |
| Planning | `docs/planning/*.md` | `docs/standards/documentation-standards.md` | Status, Date, Summary, Analysis |

## Framework vs Guide: Critical Distinction

**This is the most common misclassification. Read carefully.**

### Framework Documents

**Purpose:** Teach the "why" - principles, patterns, architecture

**Required:** `docs/standards/framework-documentation-standard.md`

**MANDATORY structure:**

- Part 1: Why This Architecture (explains reasoning)
- Narrative teaching style
- Enables understanding, not just doing

**Examples:**

- `docs/frameworks/skills-framework.md`
- `docs/frameworks/hooks-framework.md`
- `docs/frameworks/working-protocol.md`

### Guide Documents

**Purpose:** Teach the "how" - step-by-step instructions

**Required:** `docs/standards/documentation-standards.md`

**Structure:**

- Overview (what this covers)
- Prerequisites (what you need)
- Sections (step-by-step)
- Troubleshooting
- References

**Examples:**

- `docs/guides/ai-workflow-guide.md`
- `docs/guides/claude-code-setup-guide.md`

### How to Tell the Difference

| Ask yourself... | Framework | Guide |
|-----------------|-----------|-------|
| Does it explain WHY something is designed this way? | Yes | No |
| Does it teach step-by-step HOW to do something? | No | Yes |
| Is it meant to be read once for understanding? | Yes | No |
| Is it meant to be referenced repeatedly for steps? | No | Yes |
| Does it define patterns others should follow? | Yes | No |

## After Type Selection

Once you've determined the document type:

1. **Read the identified template/standard file completely**
2. **Note all required sections**
3. **Create document following the template structure**
4. **Invoke lint-file operation to verify**
