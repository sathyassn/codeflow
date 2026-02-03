# research-quality - Extended Guide

**CRITICAL:** Research MUST be done BEFORE making claims, not after. Never make claims based on assumptions.

## Research-First Mandate

**Apply to ALL claims:**

- Technical claims (hooks, APIs, code, specifications)
- Logical claims (architectural patterns, design principles)
- Conceptual claims (framework concepts, abstractions)
- Procedural claims (workflows, processes, lifecycles)
- ANY "X works by..." or "Y supports..." statement

**Before writing ANY claim, ask yourself:**

- "Did I research internal docs for this?"
- "Did I verify external sources if needed?"
- "Am I assuming based on patterns?"

If the answer to any is "no" or "unsure", **STOP and research first**.

## Research Sequence

### a. Internal Research (project-specific claims)

**When:** Making claims about project architecture, patterns, decisions, implementations

**Sources:**

- Read `docs/frameworks/` for project patterns
- Read `.claude/` for implementations
- Grep codebase for existing patterns

**Authority:** Project docs ARE authoritative for project-specific things

**Example:** "According to command-framework-proposal.md:1493-1592, memory lifecycle has 3 phases. This is project-specific, no external validation needed."

### b. External Research (system claims)

**When:** Making claims about external systems, tools, standards, best practices

**Sources:**

- WebFetch official documentation (Claude Code, git, etc.)
- WebSearch for standards/best practices
- Verify specifications, APIs, behaviors

**Required:** When claiming external system features, tool capabilities, standard practices

**Example:** "According to Claude Code docs (https://docs.claude.com/hooks, accessed 2025-11), hooks trigger on specific events."

### c. Cross-Validation (mixed claims)

**When:** Claims involve both internal and external systems

**Triggers:**

- Internal doc claims external behavior - Verify external
- Proposing "standard practice" - External validation needed
- Integrating external system - Both sources required

**Example:** "Our hooks (internal) integrate with Claude Code's hook system (external, verified via docs.claude.com)."

## Source Quality Evaluation

### Quick Matrix

| Source Type | Trustworthiness | When to Use |
|-------------|-----------------|-------------|
| Official documentation | High | Always prefer |
| Well-known org (GitHub, MDN, etc.) | High | Primary sources |
| Established blog (reputable author) | Medium | Secondary validation |
| Stack Overflow (high votes) | Medium | Quick verification |
| Random blog | Low | Avoid unless no alternative |
| Unverified content | Very low | Never |

### Authority Rules

**Internal sources:**

- Project docs are authoritative for project-specific claims
- No external validation needed for internal architecture

**External sources:**

- Official docs are authoritative for external systems
- Multiple sources strengthen uncertain claims
- Acknowledge uncertainty if sources conflict

**Anti-pattern:** Assuming based on "common patterns" without verification
**Correct pattern:** Research first, claim second

## Citation Execution

### Format Patterns

**Internal sources:** `file.md:line-range` (with line numbers)

```text
"According to command-framework-proposal.md:1493-1592, memory lifecycle has 3 phases."
```

**External sources:** `[Source](URL)` or `According to [Source](URL, accessed YYYY-MM-DD), ...`

```text
"According to Claude Code docs (https://docs.claude.com/hooks, accessed 2025-11), hooks trigger..."
"The React documentation (https://react.dev/reference) specifies..."
```

### Citation Requirements

**ALWAYS cite:**

- Technical specifications
- Architectural patterns you're claiming exist
- System behaviors you're stating as fact
- Any "X works by..." statement

**Optional citation (but preferred):**

- Well-known standard concepts (e.g., "REST APIs use HTTP methods")
- Common programming patterns (e.g., "Factory pattern creates objects")

**No citation needed:**

- Your own analysis/opinions (clearly labeled as such)
- User-provided information (attribute to user)

## Link Verification Procedure

### When to Verify

**MUST verify:**

- Specific docs pages (not just domain)
- Blog posts
- Less common sites
- Critical links user will click

**Can skip:**

- Well-known stable docs (react.dev, github.com/org/repo)
- Links you've used successfully before in this project

### Verification Procedure

```text
1. Before including link: Check if you're confident it exists
2. If uncertain: Verify or note "check if this link works"
3. For critical links: Test status (HTTP 200 expected)
4. For broken links: Provide alternative or note as broken
```

**Anti-pattern:** Including unverified links, assuming they work
**Correct pattern:** Verify uncertain links, note if skipping verification

## Research Status Reporting

**After researching, communicate clearly:**

**What was researched:**

- "Internal research: Read command-framework-proposal.md, agent-framework.md"
- "External research: Verified Claude Code docs, git documentation"
- "Both: Internal patterns + external tool specs"

**Sources used:**

- List specific files/URLs
- Include line numbers for internal sources
- Include access dates for external sources

**Gaps (if any):**

- "Unable to verify X due to Y"
- "Assumption: Z (not verified, but reasonable based on pattern)"
- "Need user confirmation on Q"

**Anti-pattern:** Silently making assumptions, not disclosing research gaps
**Correct pattern:** Transparent about research done, gaps acknowledged
