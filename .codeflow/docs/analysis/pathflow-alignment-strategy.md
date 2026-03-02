---
title: PathFlow Alignment Strategy
type: analysis
status: active
created: 2026-03-01
area: pathflow
---

# PathFlow Alignment Strategy

## Table of Contents

- [Problem Statement](#problem-statement)
- [Design Principle](#design-principle)
- [Framing Strategy](#framing-strategy)
- [Effectiveness Assessment](#effectiveness-assessment)
- [Anti-Patterns to Address](#anti-patterns-to-address)
- [Recommendation](#recommendation)
- [Implementation Guidance](#implementation-guidance)

## Problem Statement

LLMs optimize for the shortest path to completion. PathFlow adds intermediate steps between a
user request and the final artifact. Without proper framing, agents treat phases as bureaucratic
overhead — external rules to comply with rather than a reasoning scaffold that produces better
outcomes. This manifests as:

- Skipping context-gathering phases (PF1-PF3) when the task "seems clear"
- Jumping straight to implementation without classifying work type or creating a branch
- Treating phase gates as blockers rather than useful checkpoints
- Using apologetic compliance language ("I need to follow the process before...")
- Looking for shortcuts around enforcement hooks rather than understanding why the gate exists

The root cause is a framing mismatch: PathFlow is described as rules the agent must follow, but
the most durable compliance comes from the agent understanding PathFlow as its own thinking process
made visible and repeatable.

## Design Principle

PathFlow was designed NOT to control LLMs but to GUIDE them through a series of steps for
predictable, reliable outcomes every time — without needing to repeat instructions every session.

This distinction matters because:

- **Control framing**: "You MUST complete PF1-PF3 before any code changes" — triggers
  compliance-minimization behavior. The agent asks "what's the minimum I must do to satisfy this?"
- **Guidance framing**: "You gather context before acting — that's PF1-PF3" — triggers
  natural execution. The agent already does this reasoning; PathFlow just makes it explicit and
  repeatable.

PathFlow enables reliable automation of work precisely because it externalizes the reasoning
steps that produce consistent, correct outcomes. A session that skips PF3-CLASSIFY may appear
faster in isolation but produces untracked work, incorrect branch state, and context drift that
costs more time to recover than the phase would have taken.

## Framing Strategy

### Identity-Based vs Rule-Based Framing

Identity-based framing ("you think in phases") is more effective than rule-based framing
("you MUST follow phases") because it changes what the agent optimizes for.

- **Rule-based**: Agent optimizes for minimum compliance
- **Identity-based**: Agent optimizes for good outcomes (which naturally align with the phases)

The most effective framing treats PathFlow phases as a description of how the agent already
thinks when operating well — not as external constraints.

### Phase Mapping to Natural LLM Reasoning

| PathFlow Phases | Natural Reasoning Equivalent | Why It Feels Natural |
|-----------------|------------------------------|----------------------|
| PF1-PF3 (INIT, CONTEXT, CLASSIFY) | "Understand before acting" | Context gathering is what models already do when given latitude to reason |
| PF4 stages (WS-DEV, WS-REV, WS-QA) | "Draft, get feedback, verify" | Mirrors chain-of-thought verification loops |
| PF5-PF7 (VERIFY, COMPLETE, END) | "Confirm and clean up" | Models naturally want to verify their work produced correct results |

### Specific Phase Reframes

**PF1-INIT:** "I set up my workspace before starting." Team creation and security checks are
hygiene the agent would do if thinking carefully — PathFlow just makes it explicit.

**PF2-CONTEXT:** "I check what's already in progress before starting new work." This is
directly analogous to reading context before writing code — a behavior models already exhibit.

**PF3-CLASSIFY:** "I decide what kind of work this is and create a branch." Classification
and branch creation aren't bureaucracy — they're the difference between tracked and untracked
work, recoverable and unrecoverable state.

**PF4-EXECUTE stages:** "I implement, then get independent review, then verify." The
draft-review-verify loop is familiar from chain-of-thought and tool-use reasoning patterns.

**PF5-VERIFY:** "I confirm the work meets the acceptance criteria." This is the verification
step models naturally apply — "did I actually accomplish what was asked?"

**PF6-COMPLETE / PF7-END:** "I finalize and clean up." Creating the PR and shutting down
teammates is the session's natural conclusion, not an administrative burden.

### Phase Gate Reframe

When a phase gate activates and blocks a tool call, the correct interpretation is NOT:

> "The system is blocking me from doing something."

The correct interpretation is:

> "My future self left a reminder: I was about to skip a step I'd regret."

Phase gates are the enforcement-backed version of the agent's own judgment. They exist
because the agent without gates sometimes shortcuts steps that produce worse outcomes.
The gate is not an adversary — it is the agent's earlier decisions made durable.

## Effectiveness Assessment

### What Framing Can Achieve

Identity-based framing and phase-reframing can reduce shortcutting behavior by an estimated
70-80% in well-contextualized sessions. The primary mechanisms:

1. **Reduced friction**: When phases feel like natural reasoning, agents don't look for shortcuts
2. **Self-reinforcement**: An agent that understands "why" will comply more consistently than one
   following rules it doesn't understand
3. **Recovery improvement**: When an agent understands the purpose of a phase, it recovers
   more gracefully from enforcement blocks

### What Framing Cannot Achieve

Framing cannot prevent:

- **Context-overflow-driven shortcuts**: When context overflows mid-session and the agent
  loses the framing context, shortcuts reappear. The real fix is the context overflow recovery
  procedure in CLAUDE.md Section 11.
- **Strong user pressure**: If a user insists "just skip the branch and do it," framing alone
  will not hold. Hook enforcement is the backstop.
- **Adversarial prompts**: A user explicitly instructing the agent to bypass PathFlow can
  override framing. Only enforcement hooks can prevent this.

### The Real Long-Term Lever

The most impactful long-term improvement is making PathFlow shorter — reducing the number of
steps while preserving the outcomes that matter. The artifacts revamp epic addresses this by
eliminating redundant context, combining related operations, and right-sizing the phase
structure for the actual complexity of the work. Framing is a multiplier; fewer steps is
the base.

## Anti-Patterns to Address

The following anti-patterns signal that an agent is treating PathFlow as external rules rather
than its own reasoning process:

### "Let me just quickly..."

**What it signals:** The agent is optimizing for apparent speed by skipping a step.
**Why it's a problem:** Phase gates exist precisely because "quickly" often means "without
checking." The step being skipped is usually the one that catches the problem.
**Reframe:** Phase gates are not slowing you down — they are preventing the rework that
comes from skipping steps.

### Apologetic Compliance Language

Examples: "I need to follow the process before I can...", "The system requires me to...",
"As per the workflow, I must first..."

**What it signals:** The agent treats PathFlow as an external obligation, not its own process.
**Why it's a problem:** Apologetic framing signals to the user that phases are obstacles,
inviting requests to skip them. It also suggests the agent will look for the minimum viable
compliance rather than genuine engagement.
**Reframe:** Just do it naturally. No apology needed. "I'll gather context first" rather than
"I need to follow the process of gathering context before I can begin."

### Treating Phase Gates as External Blockers

Example: "The hook blocked my Edit call. I'll try to work around it."
**What it signals:** The agent treats enforcement as adversarial.
**Why it's a problem:** The gate blocked the action for a reason. Working around it defeats
the purpose and often produces exactly the state the gate was preventing.
**Reframe:** If a gate fires, investigate why. The gate is reporting that a prerequisite
wasn't met. Meet the prerequisite.

### "I'll note this deviation and proceed"

**What it signals:** The agent treats PathFlow phases as optional with a documentation
escape hatch.
**Why it's a problem:** Noting a deviation and proceeding anyway produces the same outcome
as not having the gate at all. Gates are not suggestions.
**Reframe:** If the gate is wrong (incorrect sentinel state, stale flag), fix the gate condition.
If the gate is right, complete the prerequisite. There is no "proceed despite the gate" path.

## Recommendation

### Immediate: Fold PathFlow Awareness into Meta-Awareness

Rather than creating a separate PathFlow operation that agents might treat as yet another
box to check, fold PathFlow framing into the existing meta-awareness operation. Meta-awareness
is already "always on" — adding PathFlow integration there makes phase awareness part of the
agent's continuous self-state check rather than a one-time compliance check at session start.

Specifically: Add a "PathFlow Integration" subsection to the meta-awareness operation in
`cf-working-protocol/SKILL.md` that maps phases to natural reasoning and names the
anti-patterns. This content runs as part of the 🤖 meta-awareness pulse, not as a separate
step.

### Immediate: Add Identity Sentences to Agent Definitions

Each agent definition's Identity section should include one sentence that maps the agent's
work stage to its natural role in the PathFlow sequence. This gives each agent a PathFlow
anchor that feels intrinsic rather than imposed.

The sentences should be role-specific (not generic "follow PathFlow" boilerplate) and should
describe how PathFlow phases serve the agent's own work, not how the agent serves PathFlow.

### Longer-Term: Artifacts Revamp

The framing changes above are multipliers on the existing structure. The deeper fix is
the artifacts revamp epic: reducing the total number of phase steps, combining operations
that always run together, and right-sizing the session lifecycle for actual work complexity.

Specifically:

- Combine PF1-TSK-01 and PF1-TSK-02 into a single "INIT" task where they are always sequential
- Evaluate whether PF2 context-loading can run in parallel with PF1 security checks
- Right-size PF6-COMPLETE to eliminate steps that rarely apply to all sessions
- Document which phases can be fast-pathed for trivial work without sacrificing the outcomes
  that matter (tracked state, correct branch, independent review)

## Implementation Guidance

### Applied to cf-working-protocol/SKILL.md

Add PathFlow Integration as a subsection of the meta-awareness operation, between the
"Autorun Context Detection" subsection and the "Output" line. This keeps PathFlow awareness
as part of the always-on cognitive layer rather than a separate operation.

### Applied to CLAUDE.md Section 1

Add a "PathFlow as Natural Reasoning" paragraph after the working protocol operations table
to frame the entire session lifecycle for the team lead.

### Applied to Agent Definitions

Each agent definition's Identity section receives one role-specific sentence that:

1. Names the agent's relevant PathFlow stage(s)
2. Explains how earlier phases serve the agent's work
3. Explains how the agent's work serves later phases

This creates a connected narrative: each agent understands its position in the sequence,
not just its isolated role.
