# Part 5: Open Questions and Next Steps

> Decision points that need resolution before proceeding to M2 implementation planning.

---

## Table of Contents

- [Q1: V3 Modification vs V4?](#q1-v3-modification-vs-v4)
- [Q2: Stage Configuration](#q2-stage-configuration)
- [Q3: Who Determines When to Spawn a Team?](#q3-who-determines-when-to-spawn-a-team)
- [Q4: Autorun Integration](#q4-autorun-integration)
- [Q5: Peer Communication Reliability](#q5-peer-communication-reliability)
- [Q6: Teammate Routing Logic](#q6-teammate-routing-logic)
- [What I Need From You](#what-i-need-from-you)

---

## Q1: V3 Modification vs V4?

Given the revised design, the changes to V3 are:
- Add `stage`, `stage_status`, `stage_history` to tasks table
- Add `current_stage`, `team_name` to active_work table
- Create agent definitions in `.claude/agents/`
- Add PathFlow-aware hooks
- Modify commands to support team routing

These are ADDITIONS to V3, not replacements. The existing V3 architecture (skills, hooks, sub-agents, WorkGraph, three-tier data) remains intact and operational.

**Revised recommendation**: MODIFY V3, don't create V4. The changes are additive, not breaking. Agent Teams mode is a layer ON TOP of V3, not a replacement.

---

## Q2: Stage Configuration

Where should the stage configuration (which work types get which stages) live?

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A | In the WorkGraph task metadata (per-task) | Maximum flexibility | Requires per-task configuration |
| B | In a PathFlow config file (global defaults by work type) | Single source of truth | Less flexible per-task |
| C | Determined dynamically by the lead based on task complexity | No config maintenance | Inconsistent across sessions |

**Leaning toward**: Option B with Option A overrides. A config file maps work types to default stages, individual tasks can override via metadata.

---

## Q3: Who Determines When to Spawn a Team?

Not every session needs a team. When the user says "what's the status of my tasks?" — no team needed. When the user says "implement this feature" — team needed.

**Options**:
- Auto-detect based on whether work involves file modifications
- User signals explicitly (e.g., a command or flag)
- Based on work classification results (PF-3 determines team need)
- Always spawn cf-knowledge at minimum; full team only when warranted

**The pragmatic answer**: The lead (which IS the session) always exists. cf-knowledge is spawned early (PF-1/PF-2) for most sessions since context awareness needs it. cf-gitops and role teammates are only spawned when actual development work begins (PF-3/PF-4). For simple Q&A sessions, no teammates are spawned at all.

---

## Q4: Autorun Integration

In autorun mode, the batch config specifies the WorkGraph task ID. PathFlow runs automatically. But how does the batch config specify which stages to run?

**Options**:
- Batch config includes `stages: [dev, review, qa, deploy]`
- Batch config says `full_pipeline: true` (all stages)
- Stages determined from the task's work_type (automatic)

**Leaning toward**: Automatic from work_type, with batch config override option. A FEAT task gets the full pipeline by default; the batch config can trim stages if needed.

---

## Q5: Peer Communication Reliability

Our testing showed that teammates CAN message each other directly. But messages are fire-and-forget (no delivery guarantee to shut-down teammates).

For the direct communication model to work reliably:
- **Overloaded teammates**: What if cf-gitops gets multiple commit requests simultaneously? Each message is queued and processed in order by Agent Teams — this should be fine since teammates process one message at a time.
- **Shut-down recipients**: If a message is sent to a teammate that has shut down, it is silently lost. The sender should get a timeout + escalation pattern: if no response within N turns, escalate to lead.
- **Message ordering**: Messages are delivered in order per sender. No cross-sender ordering guarantee. This is acceptable for our use case.

**Mitigation strategy**: Teammates should always confirm receipt ("Committed as abc123", "WorkGraph updated"). If no confirmation within a reasonable time, the sender escalates to the lead.

---

## Q6: Teammate Routing Logic

In team mode, when the user says something, the lead needs to know which teammate to spawn. This routing logic needs to be documented.

**The natural answer**: Stage-driven routing.
- DEV stage -> spawn cf-developer
- REV stage -> spawn cf-reviewer
- QA stage -> spawn cf-qa
- DEPLOY stage -> cf-gitops (already persistent)

For non-stage work:
- Planning request -> spawn cf-planner
- Documentation request -> spawn cf-documenter
- Deployment/CI request -> spawn cf-ops

This mapping lives in the lead's instructions (CLAUDE.md or the PathFlow config).

---

## What I Need From You

Before proceeding to M2 (V3 impact analysis and implementation planning):

1. **Teammate roster**: Does the cf-gitops + cf-knowledge (persistent function) + role-based on-demand model make sense? Any roles missing or unnecessary?

2. **Work stages**: Is DEV -> REV -> QA -> DEPLOY the right default progression? Should some stages be mandatory vs optional per work type?

3. **Multi-pathway routing**: When review finds issues, the revised design routes back to DEV. Is this the right pattern? Are there other routing conditions to handle?

4. **WorkGraph schema changes**: The proposed `stage`, `stage_status`, `stage_history` additions — any concerns about the schema design?

5. **V3 modification**: Agreed on modifying V3 rather than creating V4?

6. **Team spawning trigger**: What should trigger team creation vs solo work?

7. **Anything else that needs rethinking?**
