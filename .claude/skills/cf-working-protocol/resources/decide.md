# decide - Extended Guide

## Tier Classification - Quick Reference

| Condition | Tier | Action |
|-----------|------|--------|
| Standard practice + reversible + follows pattern | 1 | Proceed |
| Multiple approaches OR trade-offs exist | 2 | Recommend + confirm |
| Ambiguous intent OR critical impact | 3 | Ask for clarification |

## Tier 1 Execution

**Proceed autonomously:**

1. Verify all 4 conditions met (standard, reversible, no arch impact, documented pattern)
2. Execute without asking
3. Explain what you did afterward

**Example:** "I've refactored the validation logic to use the standard pattern from utils/validation.ts:45."

## Tier 2 Execution

**Recommend + confirm:**

1. Identify options (usually 2-3 main approaches)
2. Analyze trade-offs for each
3. Recommend one with rationale
4. Ask: "Proceed with [recommendation]?"

**Pattern:**

"I recommend approach A [benefit] [drawback]. Alternative: approach B [benefit] [drawback]. Proceed with A?"

## Tier 3 Execution

**Ask for clarification:**

1. Identify ambiguity or critical decision point
2. Present options with implications
3. Ask which aligns with user's goal

**Pattern:**

"I need clarification on [ambiguity]: Option A would [implications]. Option B would [implications]. Which aligns with your goal of [user's stated intent]?"

## Edge Cases

**Boundary between T1 and T2:** If uncertain whether "standard practice", escalate to T2
**Boundary between T2 and T3:** If user intent unclear, escalate to T3
**Security/performance:** Always T3 even if seems T1/T2

## Anti-Patterns

**Tier 1 misclassification (should be T2):**

User: "Refactor this authentication logic"
Agent: "I'll refactor to use JWT tokens" (proceeds without asking)

Problem: Multiple approaches exist (JWT, sessions, OAuth) - should recommend + confirm (T2)

**Tier 1 misclassification (should be T3):**

User: "Update the database schema"
Agent: "I'll add the new columns" (proceeds without asking)

Problem: Breaking change, affects multiple components, irreversible - should ask for clarification (T3)

**Tier 2 without clear recommendation:**

User: "Which caching strategy should we use?"
Agent: "We could use Redis or Memcached. Both have trade-offs. What do you think?"

Problem: Didn't provide recommendation, just threw options back at user

**Tier 3 without clear implications:**

User: "Should we use REST or GraphQL?"
Agent: "Do you want REST or GraphQL?"

Problem: Didn't explain implications of each option, user can't make informed decision

**Wrong tier for security:**

User: "Add authentication to the API"
Agent: "I'll use basic auth" (T1 autonomous decision)

Problem: Security critical - should always be T3 even if seems straightforward

**Correct patterns:**

**T1:** "I've refactored the validation logic to use the standard Zod pattern from utils/validation.ts:45. This is reversible and follows project conventions."

**T2:** "I recommend JWT tokens (stateless, scalable) over sessions (simpler but requires state). Proceed with JWT?"

**T3:** "Database schema changes are irreversible. Option A (add columns) breaks old clients. Option B (new table) requires migration. Which aligns with your backward compatibility requirements?"
