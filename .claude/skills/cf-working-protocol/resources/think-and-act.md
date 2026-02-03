# think-and-act - Extended Guide

This operation combines systematic thinking with deliberate action validation.

## PART 1: THINK SYSTEMATICALLY

### Application Contexts

**Problem analysis:** Apply all 3 dimensions (scenarios, users, holistic)
**Solution design:** Focus on scenarios + holistic (users implicit)
**Code review:** Focus on users (who uses this?) + holistic (system-wide impact)
**Refactoring:** All 3 dimensions (how used? by whom? what breaks?)

### Scenario Analysis - Execution Patterns

**Sequential:** One actor, step-by-step (most common workflows)
**Parallel:** Multiple actors simultaneously (CI/CD, multi-agent, concurrent users)
**Mixed:** Some parallel, some sequential (realistic production environments)
**Evolution:** How patterns change over time (scaling, growth)

**Questions to ask:**

- Does this work if one actor? Multiple actors?
- What if steps happen out of order?
- What if some fail while others succeed?

### User/Audience Analysis - Who's Affected

**Human users:**

- Developers (primary)
- End users (indirect)
- Maintainers (future)
- Different skill levels (junior vs senior)

**AI agents:**

- Other agents consuming outputs
- Future iterations of same agent
- Different agent types (planner vs developer)

**Systems:**

- Automated processes (CI/CD)
- Integration points (APIs, webhooks)
- Monitoring/logging systems

**Critical question:** Who benefits? Who's affected by this decision?

### Holistic Analysis - System View

**Before diving into details:**

- See whole system first
- Understand context and constraints
- Identify all moving parts

**While working:**

- Consider second-order effects
- What else changes if I change this?
- What am I missing?

**Before finalizing:**

- Review: Does this fit the larger system?
- Check: Any unintended consequences?
- Ask: What dependencies did I overlook?

## PART 2: ACT DELIBERATELY (PAC-5)

### Pre-Action Checkpoint - The 5 Questions

Before EVERY tool call, ask yourself:

| # | Check | Full Question | If Answer is NO |
|---|-------|---------------|-----------------|
| Goal | Does this action advance my current goal? | STOP - reassess your approach |
| Tool | Is this the RIGHT tool for this specific task? | Consider better alternatives |
| Skill | Does a SKILL exist that covers this operation? | Invoke skill FIRST |
| Logic | Does this action make LOGICAL sense right now? | Think harder before proceeding |
| Cost | Is this action WORTH the token/time/API cost? | Find a more efficient path |

### Tool-Specific Validations

| Tool Type | Required PAC-5 Check |
|-----------|---------------------|
| `Bash(git *)` | Did I invoke `Skill('cf-git-workflow')` first? |
| `Grep` on `.py` files | Would LSP be more accurate? |
| `Edit`/`Write` on `.md` | Will I invoke `Skill('cf-documentation-standards')` after? |
| `Edit`/`Write` on `.sh`/`.py` | Will I invoke `Skill('cf-script-standards')` after? |
| `Task` (sub-agent spawn) | Did I check if this can be parallelized with other work? |
| Any repeated operation | Should I parallelize instead of doing sequentially? |

### Red Flags - Signs of Mindless Execution

If you notice any of these patterns, STOP and reassess:

1. **Repetitive tool calls:** Calling the same tool type 3+ times without stepping back
2. **Wrong tool for language:** Using Grep for Python when LSP is available
3. **Missing skill invocation:** Git operations without git-workflow skill
4. **Creation over editing:** Creating new files when you should edit existing ones
5. **Sequential tunnel vision:** Processing items one-by-one when they're independent
6. **Retry without change:** Repeating a failed approach without changing strategy
7. **Memory over skill:** Working from memory when a validated skill procedure exists

## Anti-Patterns

### PART 1: Thinking Anti-Patterns

**Narrow scenario thinking:**

User: "Add caching to the API"
Agent: "I'll add Redis caching to the endpoint"

Problem: Only considered sequential single-user scenario, didn't think about parallel access (cache invalidation, race conditions)

**Forgetting AI agent users:**

User: "Design the output format for this tool"
Agent: "I'll make it human-readable with nice formatting"

Problem: Forgot that other AI agents might consume this output (need structured data, not just pretty text)

**Ignoring second-order effects:**

User: "Move this function to a utility file"
Agent: "Moved function to utils/helpers.ts"

Problem: Didn't check what else imports this function, broke 5 other files

### PART 2: Acting Anti-Patterns

**Mindless sequential tool calls:**

Agent calls Grep 5 times in a row, each time adjusting the pattern slightly

Problem: Should have thought about the pattern thoroughly BEFORE the first call

**Ignoring skills:**

Agent runs `git commit` without invoking git-workflow skill

Problem: Hooks fail, format is wrong, needs retry - wasted tokens

## Correct Patterns

**Systematic thinking:**

"This caching works for single user, but with parallel requests we need cache locks or TTL strategy"

**User-aware design:**

"Human devs want readable logs, but CI/CD pipeline needs structured JSON - I'll support both formats"

**Holistic analysis:**

"Moving this function affects 3 files. Let me check dependencies first, then update all references"

**PAC-5 compliance:**

"Before this Grep call: Goal? Tool? Skill? Logic? Cost? - Proceed"
