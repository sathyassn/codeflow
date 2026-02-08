# 05: Communication Model

> Direct peer messaging, communication patterns, routing instructions, the lead's role, and multi-pathway reliability.

---

## Table of Contents

- [5.1 Direct Peer Communication](#51-direct-peer-communication)
- [5.2 Communication Patterns Table](#52-communication-patterns-table)
- [5.3 Communication Flow Diagram](#53-communication-flow-diagram)
- [5.4 Routing Instructions in Agent Definitions](#54-routing-instructions-in-agent-definitions)
- [5.5 The Lead's Role](#55-the-leads-role)
- [5.6 Multi-Pathway Reliability](#56-multi-pathway-reliability)
- [5.7 Message Delivery Semantics](#57-message-delivery-semantics)
- [5.8 Anti-Patterns](#58-anti-patterns)

---

## 5.1 Direct Peer Communication

The fundamental communication principle in PathFlow: **teammates message each other directly**. The team lead orchestrates the overall flow but does not mediate every interaction.

This is the key improvement Agent Teams enables over the V3 sub-agent model. In V3, the main agent mediated ALL communication (hub-and-spoke). In PathFlow, teammates communicate peer-to-peer for operational work, with the lead handling orchestration and escalation.

```
V3 Sub-Agent Model              PathFlow Communication Model
(Hub-and-Spoke)                 (Direct Peer + Lead Orchestration)
====================            ==================================

      +------+                        +------+
      | Lead |                        | Lead |
      +--+---+                        +--+---+
        /|\                              |
       / | \                     orchestrates + escalations
      /  |  \                            |
  +--+ +--+ +--+               +--+   +--+   +--+
  |D | |R | |Q |               |D +---+R +---+Q |
  +--+ +--+ +--+               +--+   +--+   +--+
                                  \     |     /
  All messages go                  +----+----+
  through the lead.                |  gitops |
  Lead context fills up.           |  knowl. |
                                   +---------+
                                Direct operational
                                messages between peers.
                                Lead stays lightweight.
```

---

## 5.2 Communication Patterns Table

| Communication Type | Direction | Mechanism | Example |
|-------------------|-----------|-----------|---------|
| **Task assignment** | Lead -> Teammate | TaskUpdate (set owner) + SendMessage | Lead assigns "implement login" to cf-developer |
| **Work handoff** | Teammate -> Teammate | SendMessage (direct) | cf-developer -> cf-gitops: "Please commit with message X" |
| **Stage completion** | Teammate -> cf-knowledge-layer | SendMessage (direct) | cf-developer -> cf-knowledge-layer: "Dev stage complete for FRT-TSK-042" |
| **Status report** | Teammate -> Lead | SendMessage (direct) | cf-qa -> Lead: "All tests pass, 94% coverage" |
| **Escalation** | Teammate -> Lead | SendMessage (direct) | cf-developer -> Lead: "Blocked -- need clarification on requirements" |
| **Review feedback** | cf-reviewer -> cf-developer | SendMessage (direct) | cf-reviewer -> cf-developer: "Missing error handling for timeout case" |
| **Progress recording** | Any teammate -> cf-knowledge-layer | SendMessage (direct) | cf-reviewer -> cf-knowledge-layer: "Record decision: using strategy pattern" |
| **Phase transition** | Lead -> relevant teammates | TaskUpdate + SendMessage | Lead marks PF-4 complete, notifies team |
| **Shutdown** | Lead -> Teammate | SendMessage (shutdown_request) | Lead -> cf-developer: shutdown request after WS-DEV |

---

## 5.3 Communication Flow Diagram

### Full Feature Development Session

```
PHASE    LEAD              cf-knowledge-layer    cf-gitops    cf-developer    cf-reviewer    cf-qa
=====    ====              ==================    =========    ============    ===========    =====

PF-1     [start]
           |
PF-2     spawn ----------> [alive]
           |
         assign task -----> detect work
                            load context
                     <----- "context loaded"
           |
PF-3     classify work
         spawn --------------------------------> [alive]
         assign task -----> register work
         assign task -----------------------------> create branch
                     <----- "registered"
                     <------------------------------- "branch ready"
           |
PF-4     spawn --------------------------------------------------> [alive]
WS-DEV   assign task -------------------------------------------> implement
                                                     |
                                                     +--------> "commit: feat(auth)..."
                                                     |              |
                                                     |         <--- "committed abc123"
                                                     |
                                              <----- "dev stage complete"
                     <------------------------------------------------ "task done"
                                                                   |
         shutdown ------------------------------------------------> [dead]
           |
WS-REV   spawn ------------------------------------------------------------------> [alive]
         assign task ------------------------------------------------------------> review
                                                                        |
                                                                 <----- "review complete, approved"
                     <-------------------------------------------------------------- "task done"
         shutdown ----------------------------------------------------------------> [dead]
           |
WS-QA    spawn ----------------------------------------------------------------------------> [alive]
         assign task --------------------------------------------------------------------> write tests
                                                                                 |
                                                                                 +------> "commit tests"
                                                                                 |           |
                                                                                 |      <--- "committed"
                                                                                 |
                                                                          <----- "QA complete"
                     <-------------------------------------------------------------------------- "task done"
         shutdown --------------------------------------------------------------------------> [dead]
           |
PF-5     verify work
           |
PF-6     assign task -----> mark complete
         assign task -----------------------------> create PR
                     <----- "WorkGraph updated"
                     <------------------------------- "PR created"
           |
PF-7     shutdown --------> [dead]
         shutdown -----------------------------> [dead]
         cleanup
```

### Rework Loop (Review Requests Changes)

```
WS-REV   cf-reviewer reviews code
          |
          cf-reviewer -> cf-developer: "Missing error handling for timeout"
          cf-reviewer -> cf-knowledge-layer: "Review verdict: changes_requested"
          cf-reviewer -> Lead: "Changes requested, see findings"
          |
          Lead creates new WS-DEV tasks
          Lead assigns rework to cf-developer (still alive)
          |
WS-DEV   cf-developer addresses feedback
(rework)  |
          cf-developer -> cf-gitops: "Commit: fix(auth): add timeout handling"
          cf-developer -> cf-knowledge-layer: "Rework dev stage complete"
          cf-developer -> Lead: "Rework done"
          |
WS-REV   Lead assigns re-review to cf-reviewer (still alive)
(re-rev)  |
          cf-reviewer re-reviews
          cf-reviewer -> Lead: "Approved"
```

In rework loops, both cf-developer and cf-reviewer are kept alive to preserve their context across iterations. This avoids the cost of re-reading the codebase and re-establishing understanding.

---

## 5.4 Routing Instructions in Agent Definitions

For direct peer communication to work, BOTH the sender and receiver must know how to communicate. Routing instructions are included in BOTH agent definitions:

### Sender-Side Instructions (in cf-developer agent def)

```
## Communication

When you need git operations:
  -> Message cf-gitops directly.
  -> Format: "Please commit: {type}({scope}): {description}"
  -> Wait for confirmation before proceeding.

When a work stage completes:
  -> Message cf-knowledge-layer: "{stage} stage complete for {task-id}"

When your task is done:
  -> Message the Lead: "Task #{n} complete. {summary}"

When you are blocked:
  -> Message the Lead: "Blocked on task #{n}: {reason}"

When responding to review feedback:
  -> Message cf-reviewer directly with your response.
```

### Receiver-Side Instructions (in cf-gitops agent def)

```
## Communication

You receive commit requests from:
  cf-developer, cf-qa, cf-documenter

Expected format:
  "Please commit: {type}({scope}): {description}"

Your response:
  "Committed as {hash}" or "Commit failed: {reason}"

You receive PR requests from:
  Lead, cf-ops

Your response:
  "PR #{number} created against {base}" or "PR creation failed: {reason}"
```

### Why Both Sides Need Instructions

If only the sender knows to message cf-gitops, but cf-gitops does not know to expect messages from cf-developer, the communication may break down. Both sides need:

1. **Who** to expect messages from / send messages to
2. **What** format to use
3. **When** to send (triggers/conditions)
4. **What** response to expect / provide

---

## 5.5 The Lead's Role

The team lead has a specific, bounded role in the communication model. It orchestrates but does not mediate.

### What the Lead Does

| Responsibility | How |
|---------------|-----|
| Create the PathFlow task graph | TaskCreate with phase markers and work tasks |
| Spawn teammates at the right time | Progressive spawning based on phase progression |
| Assign tasks to teammates | TaskUpdate (set owner) + SendMessage (provide context) |
| Manage phase transitions | Mark phase markers as completed when appropriate |
| Handle escalations | Receive and resolve blocked/ambiguous situations from teammates |
| Monitor overall progress | Periodically check TaskList for status |
| Make routing decisions | Determine which teammate handles what |
| Manage team lifecycle | Shutdown, recycling, scaling up/down |

### What the Lead Does NOT Do

| Anti-Pattern | Why It's Wrong |
|-------------|----------------|
| Relay messages between teammates | Defeats the purpose of direct communication; fills lead's context |
| Execute work (edit files, run tests) | Lead should orchestrate, not implement |
| Micro-manage teammate operations | Teammates have SOPs; trust them to follow procedures |
| Broadcast every status update | Use targeted messages; broadcasts are expensive |

### Lead as Safety Net

The lead monitors overall progress as a safety net. If a teammate stops responding, if a work stage takes unexpectedly long, or if the task graph gets stuck, the lead intervenes. But in the normal flow, teammates self-coordinate.

```
Normal Flow:
  cf-developer -> cf-gitops: "commit please"
  cf-gitops -> cf-developer: "committed"
  (Lead is not involved)

Escalation Flow:
  cf-developer -> cf-gitops: "commit please"
  (no response for extended time)
  cf-developer -> Lead: "cf-gitops not responding to commit request"
  Lead investigates and resolves
```

---

## 5.6 Multi-Pathway Reliability

When work stages loop (review requests changes, routing back to DEV), communication reliability becomes critical. PathFlow addresses this through three strategies:

### Strategy 1: Keep Teammates Alive During Rework Loops

During a rework loop (WS-REV -> WS-DEV -> WS-REV), the lead keeps both cf-developer and cf-reviewer alive rather than shutting them down and respawning. This ensures:

- cf-developer retains context about what it implemented and why
- cf-reviewer retains context about what it found and what it expects
- Direct communication between them works because both instances are the same ones that were involved in the original work

### Strategy 2: Lead Monitors as Safety Net

The lead watches for signs that the rework loop is stuck:

- If cf-developer reports "rework done" but cf-reviewer does not respond to the re-review assignment, the lead nudges
- If the loop iterates more than a configurable number of times, the lead escalates to the user
- If a teammate degrades (garbled output, missed instructions), the lead recycles it

### Strategy 3: Confirmation-Based Communication

Every operational message expects a confirmation response:

| Request | Expected Confirmation |
|---------|----------------------|
| "Please commit: ..." | "Committed as {hash}" |
| "Dev stage complete for ..." | (cf-knowledge-layer updates WorkGraph) |
| "Review complete. Verdict: ..." | (Lead processes verdict) |
| "Task #{n} complete." | (Lead marks task completed) |

If no confirmation arrives within a reasonable time, the sender escalates to the lead. This handles the case where a message is sent to a teammate that has unexpectedly shut down (messages to shut-down teammates are silently lost -- a known platform behavior).

---

## 5.7 Message Delivery Semantics

Understanding the platform's message delivery model is critical for designing reliable communication:

| Property | Behavior |
|----------|----------|
| **Delivery guarantee** | Fire-and-forget. No delivery confirmation from the platform. |
| **Ordering** | Messages from a single sender are delivered in order. No cross-sender ordering guarantee. |
| **Recipient validation** | The platform does NOT validate that the recipient is alive. Messages to shut-down teammates are silently accepted and never delivered. |
| **Queuing** | Messages to idle teammates are queued and delivered when the teammate's next turn begins. |
| **Broadcast** | Sends a separate copy to each teammate. Expensive (N messages for N teammates). |
| **Latency** | Messages to idle teammates wake them up. There is a processing delay as the teammate starts its turn. |

**Implications for PathFlow**:

1. Always use confirmation-based communication for critical operations
2. Do not assume delivery to teammates that may have been shut down
3. The lead should verify teammate liveness before assigning critical tasks
4. Use broadcast sparingly -- only for critical team-wide announcements

---

## 5.8 Anti-Patterns

### Anti-Pattern 1: Hub-and-Spoke Through the Lead

```
BAD:  cf-developer -> Lead -> cf-gitops -> Lead -> cf-developer
GOOD: cf-developer -> cf-gitops -> cf-developer
```

Funneling all communication through the lead recreates the V3 sub-agent bottleneck. The lead's context fills up with relay messages, and every operation becomes sequential.

### Anti-Pattern 2: Broadcasting for Targeted Messages

```
BAD:  Lead broadcasts "cf-developer, your task is ready"
GOOD: Lead messages cf-developer: "Your task is ready"
```

Broadcast sends to ALL teammates. If only one teammate needs the information, use a direct message. Broadcasting is N times more expensive than a direct message.

### Anti-Pattern 3: Ignoring Confirmations

```
BAD:  cf-developer -> cf-gitops: "commit" (then immediately continues)
GOOD: cf-developer -> cf-gitops: "commit" (waits for "committed as abc123")
```

Without waiting for confirmation, cf-developer might proceed assuming the commit succeeded when it did not. Confirmation-based communication prevents cascading failures.

### Anti-Pattern 4: Duplicate Status Reporting

```
BAD:  cf-developer -> cf-knowledge-layer: "done"
      cf-developer -> Lead: "done"
      cf-developer -> cf-gitops: "I'm done"
      cf-developer -> cf-reviewer: "I'm done"

GOOD: cf-developer -> cf-knowledge-layer: "Dev stage complete for FRT-TSK-042"
      cf-developer -> Lead: "Task #12 complete. Implementation done."
```

Each teammate should communicate only with the teammates that NEED the information. cf-gitops and cf-reviewer do not need unsolicited status updates from cf-developer -- they will be assigned their own tasks by the lead when the time comes.
