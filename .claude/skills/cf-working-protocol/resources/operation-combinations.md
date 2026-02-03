# Operation Combinations - Extended Guide

## Common Patterns

**Planning work:**

1. think-and-act (analyze holistically)
2. decide (choose approach tier)
3. respond-organized (structured proposal)

**Implementation work:**

1. meta-awareness (confirmation)
2. decide (tier-classify implementation choices)
3. verify-work (checklist before done)
4. respond-organized (appropriate-length summary, file:line references)

**Research tasks:**

1. research-quality (find and cite sources)
2. think-and-act (consider users/scenarios)
3. respond-organized (concise synthesis)

**Code review:**

1. think-and-act (who's affected? system-wide impact?)
2. verify-work (thoroughness of review)
3. respond-organized (measured feedback with file:line references, not exhaustive)

## Sequential Application

Most operations apply **throughout** the workflow, not just once:

- meta-awareness: Once at start
- think-and-act: Every analysis/decision point
- decide: Every decision
- respond-organized: Every response (includes file:line references)
- research-quality: When citing information
- verify-work: EVERY response (before response ends)

## Anti-Patterns

**Skipping think-and-act in implementation:**

User: "Add logging to the authentication function"
Agent: "I'll add console.log statements" (proceeds immediately)

Problem: Didn't think systemically (who uses logs? CI/CD needs structured format, devs need readable format)

**Forgetting verify-work before claiming done:**

User: "Implement user registration"
Agent: "I've implemented user registration with email validation and password hashing."

Problem: Claimed complete without running verify-work checklist (forgot password strength requirements, email confirmation, error handling)

**Missing research-quality for technical claims:**

User: "Use the Claude Code built-in caching feature"
Agent: "I'll enable the built-in caching in settings.json"

Problem: Made technical claim without research (Claude Code may not have built-in caching - hallucination)

**Inconsistent meta-awareness:**

Agent response 1: "Starting implementation..."
Agent response 2: "I've completed step 1" (inconsistent)
Agent response 3: "Done!"

Problem: Inconsistent application of meta-awareness operation

**Correct patterns:**

**Implementation with all operations:**

User: "Add logging to authentication"
Agent: "I'll add structured logging (JSON for CI/CD, readable for devs) to auth.ts:45. This is T1 (standard practice, reversible). [Implements] [Runs verify-work checklist] Done - verified all requirements met, no regressions."

**Research task with proper operations:**

User: "How does the memory cleanup work?"
Agent: "According to agent-framework.md:234-267 (internal research), memory cleanup has 7-day grace period after work completion. This is project-specific, no external validation needed."

**Code review with systematic thinking:**

User: "Review this PR"
Agent: "Reviewing changes (think-and-act: affects auth, impacts all users + CI/CD). Issues found at auth.ts:45 (security concern), session.ts:12 (edge case unhandled). Verified review thoroughness."
