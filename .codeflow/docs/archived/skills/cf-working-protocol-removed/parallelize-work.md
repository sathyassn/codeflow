# Parallelize Work

**Purpose:** Detailed patterns for decomposing tasks, spawning ephemeral sub-agents in parallel, and synthesizing results coherently.

**Related operation:** `parallelize-work` in SKILL.md

## Quick Reference

| Step | Action |
|------|--------|
| 1. Decompose | Split task into 2+ independent sub-tasks |
| 2. Verify | Confirm no sequential dependencies |
| 3. Spawn | Multiple Task calls in single message |
| 4. Synthesize | Merge results, flag conflicts, note gaps |

**Model selection:**

| Task Type | subagent_type | Model |
|-----------|---------------|-------|
| Code search/exploration | `Explore` | haiku |
| Complex reasoning | `general-purpose` | sonnet |
| Architecture decisions | `Plan` | sonnet |

**Limits:** Max 10 concurrent agents (additional queue automatically)

## Details

### When to Parallelize

**Parallelize when ALL conditions are true:**

| Condition | Check |
|-----------|-------|
| Multiple sub-tasks | Can task be split into 2+ distinct parts? |
| Independence | Do sub-tasks have NO sequential dependencies? |
| Substantial scope | Is each sub-task worth spawning an agent? |

**Do NOT parallelize when:**

- Sub-task B depends on sub-task A's output
- Task is trivially small
- Iterative refinement needed across steps

### Task Decomposition Patterns

#### Pattern A: Multi-Domain Search

```text
"Find all error handling code"
 - Find try/catch blocks
 - Find error handler functions
 - Find error logging calls
```

#### Pattern B: Multi-Criteria Analysis

```text
"Review this code for issues"
 - Security analysis
 - Performance analysis
 - Code quality analysis
```

#### Pattern C: Multi-Source Research

```text
"Best practices for OAuth 2.0"
 - Search codebase for existing auth patterns
 - WebSearch for external standards
 - Read internal docs for requirements
```

### Result Synthesis

**Process:**

1. Collect all agent results
2. Merge findings (union approach)
3. De-duplicate overlapping results
4. Identify conflicts - flag for user
5. Identify gaps - note what wasn't found
6. Present unified response

**Output template:**

```markdown
## [Task Summary]

### Findings
[Merged, de-duplicated results]

### Conflicts (if any)
[Contradictory findings]

### Gaps
[What wasn't found]

### Recommendations
[Synthesized conclusions]
```

### Error Handling

| Scenario | Response |
|----------|----------|
| Agent times out | Retry once with simpler scope |
| Agent returns error | Retry once, then note failure |
| Agent returns empty | Verify scope, note as gap |
| Retry fails | Continue with successful agents, document gap |

## Advanced Patterns

### Wave-Based Parallelization

When initial parallel exploration reveals MORE parallelization opportunities, use waves:

**Pattern:**

```text
Wave 1: Initial parallel exploration
    |
Synthesis: Merge results, identify new independent targets
    |
Wave 2: Spawn agents for newly discovered targets
    |
Final Synthesis: Comprehensive merged output
```

**When to use:**

- Initial exploration reveals multiple independent domains
- Breadth-first discovery needed before depth analysis
- Complex systems with layered architecture

**Key principle:** Main agent orchestrates all waves; sub-agents do focused work.

### Sub-Agent Parallelization Behavior

Sub-agents CANNOT spawn agents but SHOULD recommend parallelization when discovered.

**When a sub-agent identifies parallelizable work:**

1. **Complete YOUR assigned scope first** (don't stop early)
2. **Include recommendation in return message:**

```text
PARALLELIZATION OPPORTUNITY DETECTED:
This investigation revealed N independent targets for deeper analysis:
- Target A: [description and why it's independent]
- Target B: [description and why it's independent]
- Target C: [description and why it's independent]

Recommend main agent spawn parallel agents for comprehensive coverage.
```

1. **Main agent reads recommendation** and decides whether to execute parallel wave

**What sub-agents should NOT do:**

- Attempt to spawn agents (will fail - no Task tool access)
- Stop early to report opportunity (complete your scope first)
- Assume main agent will act on recommendation (it's advisory)

## Anti-Patterns

| Anti-Pattern | Problem | Solution |
|--------------|---------|----------|
| Parallelizing dependent tasks | Results incomplete/wrong | Check independence first |
| Spawning for trivial tasks | Overhead exceeds benefit | Use sequential for simple ops |
| Ignoring conflicts | Contradictory output | Flag conflicts explicitly |
| Not documenting gaps | Incomplete picture | Always note what wasn't found |
| Truncating agent work | Incomplete analysis | Apply thoroughness directive |

## Limitations

| Constraint | Description |
|------------|-------------|
| Main agent only | Sub-agents cannot spawn other sub-agents |
| Isolated context | Agents can't see each other during execution |
| No inter-agent messaging | All coordination through main agent |
| Synchronous synthesis | Must wait for all agents to complete |
| 10 agent limit | Additional tasks queue |
