# Claude Agent Teams - Test Findings

**Date**: 2026-02-05
**Environment**: Claude Code with Opus 4.6, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`
**Mode**: In-process (default)

---

## Test Summary

| # | Test | Result | Details |
|---|------|--------|---------|
| 1 | Sub-agent definitions as teammate blueprints | PARTIAL | `subagent_type` registers in config but does NOT auto-inject agent definition into teammate context |
| 1b | Agent def loading via explicit spawn instruction | PASS | Including "read your agent def file" in spawn prompt works — teammate loads and follows the definition |
| 1c | Agent def loading via post-spawn message | PASS | Sending "read your agent def file" as first message after spawn also works — equivalent outcome |
| 2 | Teammate persistence and task reassignment | PASS | Teammates persist across idle periods, retain full context, accept multiple sequential tasks |
| 3 | On-demand teammate spawning | PASS | Teammates can be added to an existing team at any time after creation |
| 4 | Multiple instances of same agent type | PASS | Multiple teammates with identical `subagent_type` spawn without conflict, communicate peer-to-peer |
| 5 | Context clearing | NOT POSSIBLE | No self-service mechanism for teammates to clear their own context; only auto-compaction at ~95% capacity |
| 6 | Individual shutdown and team survival | PASS | Individual teammates can be shut down; remaining teammates and team lead continue operating normally |

---

## Test 1: Sub-Agent Definitions as Teammate Blueprints

### Setup
- Created `.claude/agents/cf-test-reviewer.md` with specific behavioral instructions (prefix messages with `[cf-test-reviewer]`, identify as code reviewer)
- Spawned teammate with `subagent_type: "cf-test-reviewer"`

### Findings

**What DOES happen:**
- The `subagent_type` value is recorded in `config.json` as the teammate's `agentType` field
- The teammate is spawned successfully

**What does NOT happen:**
- The `.claude/agents/cf-test-reviewer.md` file contents are NOT injected into the teammate's system prompt
- The teammate does not know its own name or agent type from its initial context
- Environment variables `CLAUDE_CODE_AGENT_NAME`, `CLAUDE_CODE_AGENT_TYPE`, `CLAUDE_CODE_TEAM_NAME` are NOT set (despite being documented)

**What IS injected at spawn:**
- Standard Claude Code system prompt
- CLAUDE.md project instructions
- A generic "you are running as an agent in a team" note with SendMessage instructions
- The spawn prompt provided by the team lead
- Git status snapshot

**Workaround:**
The team lead must include relevant agent definition instructions in the spawn `prompt` parameter. Alternatively, the spawn prompt can instruct the teammate to read its agent definition file at `.claude/agents/{subagent_type}.md` on startup.

### Test 1b: Agent Definition Loading via Explicit Spawn Instruction (PASS)

**Approach**: Include in the spawn prompt: "Before doing anything else, read your agent definition file at `.claude/agents/cf-test-reviewer.md` and adopt the role, behavior, and output format defined in it."

**Result**: The teammate successfully:

- Read the `.claude/agents/cf-test-reviewer.md` file
- Adopted the code reviewer role
- Used the `[cf-test-reviewer]` prefix on all subsequent messages
- Produced a structured code review when assigned a task
- Maintained the role identity across multiple interactions

### Test 1c: Agent Definition Loading via Post-Spawn Message (PASS)

**Approach**: Spawn with a generic prompt ("Wait for instructions"), then send the same "read your agent def" instruction as a first message via SendMessage.

**Result**: Equivalent outcome to Test 1b. The teammate:

- Read and internalized the agent definition after receiving the post-spawn message
- Adopted the same role and output format
- Produced an equivalent structured code review
- One caveat: the teammate occasionally went idle without sending a response, requiring a nudge. This is a general teammate behavior issue (not specific to post-spawn loading).

### Comparison: Spawn-Time vs Post-Spawn Loading

| Aspect | Spawn-Time (1b) | Post-Spawn (1c) |
|--------|-----------------|-----------------|
| Agent def loaded? | Yes | Yes |
| Role adopted? | Yes | Yes |
| Output format followed? | Yes | Yes |
| Functional review quality | Equivalent | Equivalent |
| Reliability | Higher — instruction is in initial context | Slightly lower — depends on teammate processing the message |
| Extra round-trip needed? | No | Yes (send message + wait for ack) |
| Latency | Lower | Higher (extra message exchange) |

**Recommended approach**: **Spawn-time loading (1b)** is preferred. It is more reliable (instruction is part of the initial context, not a separate message that might be missed) and avoids the extra round-trip.

### Implication for CodeFlow (Updated)

Agent definitions cannot be passively loaded via `subagent_type` alone, but they CAN be reliably loaded via explicit instruction. The recommended pattern for CodeFlow's team management skill:

1. Team lead reads `.claude/agents/{subagent_type}.md`
2. Incorporates key instructions into the spawn prompt
3. Adds a standard preamble: "Read and adopt your agent definition at `.claude/agents/{type}.md`"
4. This becomes a standard part of the "spawn prompt builder" in the `cf-team-management` skill

---

## Test 2: Teammate Persistence and Task Reassignment

### Setup
- Spawned reviewer-alpha, let it complete Test 1, go idle
- Assigned Task A (read a file and report)
- Let it complete Task A, go idle again
- Assigned Task B (recall earlier findings from memory)

### Findings

**Persistence confirmed:**
- Teammate survived multiple idle → active → idle cycles
- Full conversation context retained across all cycles
- Could recall specific details from earlier turns (env var values) without re-running commands
- No context degradation observed across 3 task assignments

**Task reassignment confirmed:**
- Idle teammates accept new messages instantly and begin working
- No re-initialization needed between tasks
- The teammate maintained awareness of its role and previous work

**Implication for CodeFlow:**
Teammates are suitable for long-running roles (e.g., a reviewer that stays active throughout a development session). The team lead can assign sequential tasks without respawning.

---

## Test 3: On-Demand Teammate Spawning

### Setup
- Created team with only reviewer-alpha initially
- Later spawned worker-one and worker-two after team was already active

### Findings

**On-demand spawning works:**
- worker-one joined the existing team successfully
- worker-two joined immediately after worker-one
- Both appeared in `config.json` with correct metadata
- All teammates (including the pre-existing reviewer-alpha) could see all members in config

**No ordering constraint:**
- Teammates can be spawned at any time during the team's lifetime
- The team lead decides when to spawn based on workload
- New teammates have full access to the shared task list

**Implication for CodeFlow:**
The team lead can start with a minimal team (e.g., just a planner) and scale up as tasks progress (add developer after planning completes, add reviewer after development starts). This enables adaptive team composition.

---

## Test 4: Multiple Instances of Same Agent Type

### Setup
- Spawned worker-one with `subagent_type: "cf-test-worker"`
- Spawned worker-two with `subagent_type: "cf-test-worker"` (same type)

### Findings

**Multi-instance works:**
- Both teammates spawned successfully with the same `agentType`
- Each has a unique `agentId` (worker-one@agent-teams-test vs worker-two@agent-teams-test)
- Both visible in `config.json` simultaneously
- Differentiated by `name` parameter (worker-one vs worker-two)

**Peer-to-peer messaging between same-type instances:**
- worker-two successfully sent a direct message to worker-one
- Messages routed correctly using names (not agent types)

**Implication for CodeFlow:**
Multiple reviewers can review different files in parallel. Multiple developers can work on separate modules. The name parameter is the unique identifier, not the agent type. This enables patterns like:
- `reviewer-auth`, `reviewer-api`, `reviewer-ui` (all `subagent_type: cf-reviewer`)
- `developer-frontend`, `developer-backend` (all `subagent_type: cf-developer`)

---

## Test 5: Context Clearing

### Setup
- Asked worker-one if it could clear its own conversation context
- Checked for /clear, /compact, or context management tools

### Findings

**No self-service context clearing:**
- Teammates have no access to `/clear` or `/compact` commands (those are interactive CLI commands)
- No dedicated context management tool available
- The only context reduction is automatic compaction at ~95% capacity
- Teammates cannot voluntarily reset their conversation state

**Context is fully retained:**
- All previous messages, tool calls, and results remain in context
- Memory recall is accurate across multiple turns

**Workaround for "fresh context":**
The only way to give a teammate a clean context is to:
1. Shut down the teammate
2. Respawn a new teammate with the same name and agent type
3. The new instance starts with a fresh context window

**Implication for CodeFlow:**
For long-running teams, context accumulation is a concern. If a teammate's context fills up, it must be recycled (shutdown + respawn). The team lead should monitor teammate context usage and recycle when approaching limits. The auto-compaction at ~95% provides some relief but may lose important earlier context.

---

## Test 6: Individual Shutdown and Team Survival

### Setup
- Shut down worker-two while reviewer-alpha and worker-one remained active
- Verified team continued operating
- Tested messaging to shut-down teammate
- Shut down remaining teammates one by one
- Verified team lead continued after all teammates gone

### Findings

**Individual shutdown works:**
- worker-two shut down cleanly via `shutdown_request` → `shutdown_response` (approve: true)
- worker-two was removed from `config.json` upon shutdown
- Remaining teammates (reviewer-alpha, worker-one) continued operating normally

**Team survives partial shutdown:**
- Team lead fully operational after worker-two shutdown
- Active teammates could still message each other
- worker-one successfully messaged reviewer-alpha after worker-two's departure

**Messaging to shut-down teammate:**
- SendMessage to worker-two (shut down) returned SUCCESS (no error)
- The message was silently accepted into the inbox queue
- The message was never delivered (no active process to receive it)
- The system does NOT validate recipient liveness before accepting messages

**Team lead operates alone after all teammates gone:**
- After shutting down all teammates, the team lead continues operating normally
- The team still exists (cleanup must be called explicitly)
- The team lead can spawn new teammates after others are gone

**Shutdown is not always instant:**
- One teammate (worker-one) went idle instead of processing the shutdown request on the first attempt
- A follow-up message reminding it to approve the shutdown was needed
- Shutdown requests may be queued behind other processing

**Implication for CodeFlow:**
- Teammates can be scaled down individually as their role is no longer needed
- The team lead has full lifecycle control
- The team is NOT automatically dissolved when the last teammate shuts down — explicit cleanup is required
- When messaging, do not assume delivery to teammates that may have shut down
- Build in shutdown retry logic for teammates that don't respond promptly

---

## Environment Variables Observed

| Variable | Present | Value |
|----------|---------|-------|
| `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS` | Yes | `1` |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` | Yes | `64000` |
| `CLAUDE_CODE_SSE_PORT` | Yes | `24079` |
| `CLAUDE_CODE_ENTRYPOINT` | Yes | `cli` |
| `CLAUDECODE` | Yes | `1` |
| `TMPDIR` | Yes | `/tmp/claude` |
| `CLAUDE_CODE_TMPDIR` | Yes | `/tmp/claude-501` |
| `CLAUDE_CODE_AGENT_NAME` | **No** | - |
| `CLAUDE_CODE_AGENT_TYPE` | **No** | - |
| `CLAUDE_CODE_TEAM_NAME` | **No** | - |
| `CLAUDE_CODE_PLAN_MODE_REQUIRED` | **No** | - |

Note: The documented agent-specific environment variables (`CLAUDE_CODE_AGENT_NAME`, `CLAUDE_CODE_AGENT_TYPE`, `CLAUDE_CODE_TEAM_NAME`, `CLAUDE_CODE_PLAN_MODE_REQUIRED`) were NOT present in the teammate's environment. This may be a bug, a documentation error, or a feature that's not yet implemented in the current version.

---

## Test 7: Task Dependency System as Workflow Engine

### Setup

Created a structured workflow graph with mandatory waypoints:

```
#1 init → #2 planning → #3 checkpoint-1 → #4 implementation → #5 checkpoint-2 → #6 review  ─┐
                                                                                  → #7 testing ─┤→ #8 finalize
```

Then tested enforcement, dynamic insertion, checkpoint re-opening, and task deletion.

### Test A: Initial Graph Visibility

- Only task #1 (SESSION-INIT) was claimable at start
- All downstream tasks correctly showed their blockers
- **Result**: Dependency graph display works correctly

### Test B & C: Dependency Enforcement (Out-of-Order Execution)

- Attempted to set blocked task #4 to `in_progress` while #3 was still pending
- **Result: ALLOWED.** The system did NOT prevent it.
- Attempted to set blocked task #4 to `completed` while still blocked
- **Result: ALLOWED.** No enforcement.
- **Critical finding: Task dependencies are ADVISORY ONLY, not enforced at the tool level.** Agents can freely start and complete blocked tasks.

### Test D: Cascading Unblock Behavior

- Completing #1 unblocked #2; completing #2 unblocked #3
- Dependencies unblock one level at a time as expected
- **Result**: Cascading unblock works correctly

### Test E: Checkpoint Blocking

- After completing #2, task #4 remained blocked by #3 (the checkpoint)
- Completing #3 unblocked #4
- **Result**: Checkpoints correctly gate downstream work (at the display level)

### Test F: Dynamic Task Insertion Mid-Flow

- Team lead created task #10 (SECURITY-REVIEW) and wired it:
  - #10 blockedBy #4
  - #5 blockedBy #4 AND #10
- Worker confirmed:
  - New task appeared in TaskList immediately
  - Dependency wiring was correct
  - #5 showed blocked by both #4 and #10
- **Result: Dynamic mid-flow insertion works.** New tasks can be wired into the existing graph and relationships are immediately reflected.

### Test G: Checkpoint Re-Opening

- Task #3 (completed) was set back to `pending`
- **Result: ALLOWED.** The system permits re-opening completed tasks.
- After re-opening #3, task #4 became **re-blocked by #3**
- **Critical finding: blockedBy is LIVE and REACTIVE.** Re-opening a completed task re-blocks its dependents. This is not a one-way ratchet.
- Status blocking rules:
  - `pending` → BLOCKS dependents
  - `in_progress` → BLOCKS dependents
  - `completed` → UNBLOCKS dependents
  - `deleted` → UNBLOCKS dependents (removes from blockedBy)

### Test H: Task Deletion Mid-Chain

- Deleted task #3 (which blocked #4)
- **Result**: Task #4 became auto-unblocked. Deleted task was cleanly removed from all blockedBy references.
- Dependents do NOT remain blocked by phantom/deleted tasks.

### Summary: Workflow Engine Capabilities

| Capability | Supported? | Enforcement |
|------------|-----------|-------------|
| Dependency graph definition | Yes | Tool-level |
| Cascading unblock on completion | Yes | Automatic |
| Dependency enforcement (prevent out-of-order) | **No** | Advisory only |
| Dynamic task insertion mid-flow | Yes | Immediate |
| Multi-predecessor blocking (#5 blocked by #4 AND #10) | Yes | Correct |
| Checkpoint re-opening (completed → pending) | Yes | Re-blocks dependents |
| Task deletion with auto-unblock | Yes | Clean removal |
| Parallel branches (#6 and #7 both after #5) | Yes | Independent |
| Converging dependencies (#8 blocked by #6 AND #7) | Yes | Both must complete |

### Implication for CodeFlow: Workflow Path Enforcement

The task dependency system provides excellent **graph structure** capabilities (define, modify, re-open, delete, multi-predecessor, parallel branches) but **zero execution enforcement**. This means:

1. **The graph is a map, not a fence.** It tells agents what the intended order is, but doesn't prevent them from ignoring it.

2. **Enforcement must come from the agent layer.** Two approaches:
   - **Instruction-level**: The team lead's spawn prompts must explicitly instruct teammates: "ALWAYS check TaskList before claiming a task. NEVER work on a task that shows blockedBy dependencies."
   - **Hook-level**: A PreToolUse hook could intercept TaskUpdate calls and validate that the target task has no unresolved blockedBy before allowing status changes.

3. **Checkpoint re-opening enables iterative workflows.** A "progress-review" checkpoint can be re-opened if the review fails, forcing the dependent implementation task back into blocked state. This is powerful for quality gates.

4. **LangGraph-style waypoints are achievable** but require a combination of:
   - Task graph structure (provided by Agent Teams)
   - Behavioral enforcement (provided by CodeFlow hooks + agent instructions)
   - The graph provides the structure; CodeFlow provides the enforcement layer on top

---

## Key Takeaways for CodeFlow Integration

1. **Agent definitions require explicit injection**: The spawn prompt must include agent behavioral instructions since `subagent_type` alone does not load the definition file. Build a "spawn prompt builder" that reads `.claude/agents/{type}.md` and incorporates it into the spawn prompt.

2. **Teammates are persistent workers, not ephemeral**: Unlike sub-agents that return results and die, teammates stay alive and accumulate context. Design for long-lived agents with periodic recycling.

3. **On-demand scaling is a first-class capability**: Start small, scale up as needed. The team lead can adaptively compose the team based on emerging work requirements.

4. **Multi-instance enables parallel review/test/development**: Multiple teammates of the same type can work in parallel on different files/modules. Use meaningful names to distinguish them.

5. **No context clearing means recycling is the answer**: For tasks requiring fresh context, shut down and respawn. Consider a "recycle" operation in CodeFlow that handles this automatically.

6. **Team lifecycle is fully in the lead's control**: Individual shutdown, team survival after partial shutdown, and explicit cleanup give the lead fine-grained lifecycle management.

7. **Message delivery is fire-and-forget**: The system accepts messages to any named recipient without validating liveness. Build application-level acknowledgment if delivery confirmation is needed.

8. **Task dependencies are advisory, not enforced**: The graph structure is excellent but execution order is not gated. CodeFlow must add enforcement via hooks and/or agent instructions.

9. **Checkpoint re-opening enables iterative quality gates**: Completed checkpoints can be re-opened to force dependents back into blocked state. Powerful for review/approval workflows.

10. **Dynamic graph modification is fully supported**: Tasks can be inserted, wired, and deleted mid-flow with immediate effect on the dependency graph. Enables adaptive workflow patterns.
