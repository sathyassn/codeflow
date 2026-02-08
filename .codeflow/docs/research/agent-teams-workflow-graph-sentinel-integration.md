# PathFlow: Dynamic Workflow Path Enforcement Engine

## CodeFlow's PathFlow — Combining Agent Teams Task Dependencies with Sentinel Enforcement

**Date**: 2026-02-06
**Status**: Design Proposal
**Depends On**: agent-teams-integration-proposal.md, agent-teams-test-findings.md

---

## 1. Executive Summary

Agent Teams provides a flexible task dependency graph (advisory, not enforced). CodeFlow provides sentinel-based enforcement (hard blocks via PreToolUse hooks). By combining them, we can build a **dynamic workflow path engine** where:

- The **task graph** defines the workflow structure (nodes and edges)
- **Sentinels** enforce that each node is properly completed before downstream nodes can execute
- **Pathway templates** define pre-built graphs for common work types (planning, development, review)
- The graph is **dynamic** — nodes can be inserted, re-opened, or removed mid-flow
- Enforcement is **config-driven** — pathway definitions live in JSON, not code

This is analogous to **LangGraph's node-based execution** but built on CodeFlow's existing infrastructure.

---

## 2. Architecture: How the Pieces Fit Together

### Current State (Separate Systems)

```
Agent Teams Task System          CodeFlow Sentinel System
┌─────────────────────┐          ┌─────────────────────┐
│ TaskCreate/Update    │          │ Skill invocation     │
│ blockedBy/blocks     │          │ PostToolUse creates   │
│ Advisory only        │          │ sentinel file         │
│ No execution control │          │ PreToolUse validates  │
└─────────────────────┘          │ Hard enforcement      │
                                 └─────────────────────┘
```

### Proposed Integration (Unified Workflow Engine)

```
┌─────────────────────────────────────────────────────────────────┐
│                    WORKFLOW PATH ENGINE                          │
│                                                                 │
│  ┌──────────────┐    ┌───────────────┐    ┌──────────────────┐ │
│  │ Task Graph    │───▶│ Pathway Node  │───▶│ Sentinel Gate    │ │
│  │ (structure)   │    │ (definition)  │    │ (enforcement)    │ │
│  │               │    │               │    │                  │ │
│  │ - Dependencies│    │ - Required    │    │ - Node sentinel  │ │
│  │ - Ordering    │    │   skill ops   │    │   created on     │ │
│  │ - Parallel    │    │ - Acceptance  │    │   node completion│ │
│  │   branches    │    │   criteria    │    │ - Downstream     │ │
│  │ - Checkpoints │    │ - Agent role  │    │   nodes check    │ │
│  │               │    │ - TTL         │    │   predecessor    │ │
│  │               │    │ - Retry       │    │   sentinels      │ │
│  └──────────────┘    └───────────────┘    └──────────────────┘ │
│                                                                 │
│  Config: .codeflow/config/pathways/                             │
│  Runtime: /tmp/claude/managed/sentinels/pathway-*               │
└─────────────────────────────────────────────────────────────────┘
```

### How It Works (End-to-End)

```
1. Session starts → detect work type → load pathway template
2. Pathway template instantiates task graph with sentinel requirements
3. Agent claims task → PreToolUse checks:
   a. Is this task's blockedBy list empty? (graph check)
   b. Do predecessor pathway sentinels exist? (sentinel check)
   c. Both pass → allow work
4. Agent completes task → PostToolUse creates pathway sentinel
5. Downstream tasks check for this sentinel before proceeding
6. Checkpoint nodes can be re-opened (sentinel deleted, dependents re-blocked)
```

---

## 3. Pathway Definition Format

### Template Structure

Each pathway template lives at `.codeflow/config/pathways/{type}.json`:

```json
{
  "pathway_id": "development",
  "description": "Standard development workflow path",
  "version": "1.0.0",
  "applicable_commands": ["/cf-develop"],
  "applicable_branches": ["feat/*", "fix/*", "refactor/*"],

  "nodes": [
    {
      "id": "init",
      "type": "waypoint",
      "label": "Session Initialization",
      "agent_role": "team-lead",
      "required_skills": ["cf-memory-management:load-work-context"],
      "sentinel_name": "pathflow:development:init",
      "ttl": 3600,
      "acceptance": "Work context loaded, branch verified, task identified",
      "blockedBy": []
    },
    {
      "id": "planning",
      "type": "work",
      "label": "Implementation Planning",
      "agent_role": "cf-planner",
      "required_skills": ["cf-task-management:classify-work"],
      "sentinel_name": "pathflow:development:planning",
      "ttl": 1800,
      "acceptance": "Plan documented with acceptance criteria",
      "blockedBy": ["init"]
    },
    {
      "id": "checkpoint-plan-review",
      "type": "checkpoint",
      "label": "Plan Quality Gate",
      "agent_role": "team-lead",
      "required_skills": [],
      "sentinel_name": "pathflow:development:checkpoint-plan",
      "ttl": 600,
      "acceptance": "Plan reviewed and approved by lead",
      "blockedBy": ["planning"],
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "delete_sentinel_and_reblock"
      }
    },
    {
      "id": "implementation",
      "type": "work",
      "label": "Code Implementation",
      "agent_role": "cf-developer",
      "required_skills": ["cf-git-workflow:create-commit"],
      "sentinel_name": "pathflow:development:implementation",
      "ttl": 7200,
      "acceptance": "Code complete, tests written, committed",
      "blockedBy": ["checkpoint-plan-review"]
    },
    {
      "id": "checkpoint-impl-review",
      "type": "checkpoint",
      "label": "Implementation Quality Gate",
      "agent_role": "team-lead",
      "required_skills": [],
      "sentinel_name": "pathflow:development:checkpoint-impl",
      "ttl": 600,
      "acceptance": "Implementation meets acceptance criteria",
      "blockedBy": ["implementation"],
      "checkpoint_config": {
        "reopenable": true,
        "max_reopens": 3,
        "on_reopen": "delete_sentinel_and_reblock"
      }
    },
    {
      "id": "review",
      "type": "work",
      "label": "Code Review",
      "agent_role": "cf-reviewer",
      "required_skills": [],
      "sentinel_name": "pathflow:development:review",
      "ttl": 3600,
      "acceptance": "Review complete, verdict issued",
      "blockedBy": ["checkpoint-impl-review"]
    },
    {
      "id": "testing",
      "type": "work",
      "label": "Testing & Verification",
      "agent_role": "cf-qa",
      "required_skills": [],
      "sentinel_name": "pathflow:development:testing",
      "ttl": 3600,
      "acceptance": "All tests pass, coverage met",
      "blockedBy": ["checkpoint-impl-review"]
    },
    {
      "id": "finalize",
      "type": "waypoint",
      "label": "Session Finalization",
      "agent_role": "team-lead",
      "required_skills": [
        "cf-memory-management:complete-work",
        "cf-git-workflow:create-pull-request"
      ],
      "sentinel_name": "pathflow:development:finalize",
      "ttl": 600,
      "acceptance": "PR created, memory updated, work complete",
      "blockedBy": ["review", "testing"]
    }
  ],

  "dynamic_insertion_rules": {
    "allowed": true,
    "insertion_points": ["after:planning", "after:implementation", "after:review"],
    "max_inserted_nodes": 5,
    "require_lead_approval": true
  }
}
```

### Node Types

| Type | Purpose | Sentinel Behavior | Reopenable |
|------|---------|-------------------|------------|
| **waypoint** | Mandatory start/end gates | Created on completion, long TTL | No |
| **work** | Actual work nodes | Created on completion, work-appropriate TTL | No |
| **checkpoint** | Quality gates / progress reviews | Created on approval, short TTL | Yes (configurable) |
| **dynamic** | Inserted mid-flow | Created on completion, inherits from pathway | No |

### Pre-defined Pathway Templates

```
.codeflow/config/pathways/
  planning.json          # Plan → Review → Finalize
  development.json       # Init → Plan → Implement → Review → Test → Finalize
  review.json            # Init → Analyze → Document → Finalize
  testing.json           # Init → Plan → Write → Run → Report → Finalize
  bugfix.json            # Init → Investigate → Fix → Test → Finalize
  documentation.json     # Init → Plan → Write → Review → Finalize
  refactoring.json       # Init → Analyze → Plan → Implement → Test → Finalize
  hotfix.json            # Init → Fix → Test → Ship (minimal gates)
  research.json          # Init → Search → Analyze → Document → Finalize
```

---

## 4. Sentinel Integration Design

### New Sentinel Type: Pathway Sentinels

Extend the existing sentinel system with a new category:

```json
{
  "sentinel": "pathflow:development:implementation",
  "type": "pathway",
  "skill": null,
  "operation": null,
  "pathway_id": "development",
  "node_id": "implementation",
  "task_id": "T-AUTH-004",
  "team": "cf-E-AUTH-001",
  "tool_pattern": null,
  "created": 1707123456,
  "expires": 1707130656,
  "id": "pathway-dev-impl-abc123",
  "acceptance_met": true,
  "completed_by": "cf-developer"
}
```

**Key differences from skill sentinels:**

| Property | Skill Sentinel | Pathway Sentinel |
|----------|---------------|-----------------|
| Created by | PostToolUse on Skill() call | PathFlow engine on node completion |
| Validated by | PreToolUse hooks | Pathway enforcement hook |
| Pattern matching | Regex on tool input | Node dependency check |
| TTL | 600s default (short) | Work-appropriate (600s to 7200s) |
| Purpose | "Was this skill invoked?" | "Was this workflow step completed?" |
| Deletable | On cleanup/expiry | On checkpoint reopen |

### Enforcement Hook: `cf-pre-tool-use-pathflow-gate.sh`

New PreToolUse hook that integrates with both systems:

```bash
#!/usr/bin/env bash
# cf-pre-tool-use-pathflow-gate.sh
# Enforces workflow path ordering via task graph + sentinel combination

# 1. Check if a pathway is active for this session
# 2. Determine which node the current operation belongs to
# 3. Verify all predecessor pathway sentinels exist
# 4. If missing → BLOCK (exit 2) with guidance on what's needed
# 5. If present → ALLOW (exit 0)

check_pathway_gate() {
    local active_pathway
    active_pathway=$(get_active_pathway)
    [[ -z "$active_pathway" ]] && return 0  # No active pathway, skip

    local current_node
    current_node=$(identify_current_node "$TOOL_NAME" "$TOOL_INPUT" "$active_pathway")
    [[ -z "$current_node" ]] && return 0  # Tool not associated with any node

    local predecessors
    predecessors=$(get_node_predecessors "$active_pathway" "$current_node")

    for pred in $predecessors; do
        local sentinel_name="pathflow:${active_pathway}:${pred}"
        if ! sentinel_exists "$sentinel_name"; then
            # Blocked: predecessor node not completed
            echo "BLOCKED: Workflow path requires completing '${pred}' before '${current_node}'"
            echo "Run the required step or ask team-lead to approve skipping."
            exit 2
        fi
    done

    return 0  # All predecessors satisfied
}
```

### Integration with Existing Sentinel System

The pathway sentinel system **extends**, not replaces, the existing sentinel system:

```
Tool Invocation
    │
    ├── L0: settings.json deny list (unchanged)
    │
    ├── L1a: Existing sentinel hooks (skill enforcement)
    │   ├── cf-pre-tool-use-bash-sentinel.sh
    │   ├── cf-pre-tool-use-file-sentinel.sh
    │   └── cf-pre-tool-use-grep-sentinel.sh
    │
    ├── L1b: NEW pathway gate hook (workflow enforcement)
    │   └── cf-pre-tool-use-pathflow-gate.sh
    │
    ├── L2: Stop hook (PCV enforcement)
    │   └── cf-stop-verify-work.sh
    │
    └── L3: Advisory hooks (guidance)
        └── cf-post-tool-use-*.sh
```

Both L1a (skill sentinels) and L1b (pathway sentinels) must pass for an operation to proceed.

---

## 5. Dynamic Pathway Modification

### Inserting Nodes Mid-Flow

The team lead can insert new nodes at allowed insertion points:

```
Pathway Engine API (via skill operation):

insert_pathway_node(
    pathway_id="development",
    after_node="implementation",
    new_node={
        "id": "security-review",
        "type": "work",
        "label": "Security Audit",
        "agent_role": "cf-reviewer",
        "sentinel_name": "pathflow:development:security-review",
        "ttl": 3600,
        "acceptance": "No security vulnerabilities found"
    }
)

Effect:
  1. Creates new task in Agent Teams task list
  2. Wires dependencies: new node blockedBy "implementation", "checkpoint-impl-review" blockedBy new node
  3. No pathway sentinel created yet (must be earned)
  4. Downstream nodes remain blocked until new node completes
```

### Checkpoint Re-Opening

When a checkpoint fails (quality gate not met):

```
reopen_checkpoint(
    pathway_id="development",
    node_id="checkpoint-impl-review",
    reason="Tests failing, coverage below 80%"
)

Effect:
  1. Task status set back to "pending" (Agent Teams task system)
  2. Pathway sentinel "pathflow:development:checkpoint-impl" DELETED
  3. Downstream nodes (review, testing) become re-blocked
  4. The work node before the checkpoint (implementation) can be reassigned
  5. Reopen count incremented (max_reopens enforced)
  6. Audit log entry created
```

### Skipping Nodes (With Approval)

For hotfix workflows or emergencies:

```
skip_pathway_node(
    pathway_id="development",
    node_id="review",
    reason="Hotfix: production down, post-merge review planned",
    approved_by="team-lead"
)

Effect:
  1. Task marked as "completed" with skip flag
  2. Pathway sentinel created with "skipped: true" metadata
  3. Downstream nodes unblocked
  4. Audit log captures skip with reason and approver
  5. Finalize node's acceptance criteria updated to include "post-merge review required"
```

---

## 6. Session-Level Pathway Activation

### Automatic Pathway Selection

At session start, the pathway is selected based on work type:

```
Session Start
    │
    ├── User runs /cf-develop → development.json pathway loaded
    ├── User runs /cf-plan → planning.json pathway loaded
    ├── User runs /cf-review → review.json pathway loaded
    ├── User runs /cf-test → testing.json pathway loaded
    ├── General work → detect from task metadata → appropriate pathway
    │
    └── Pathway loaded:
        1. Read template from .codeflow/config/pathways/{type}.json
        2. Instantiate task graph (TaskCreate for each node)
        3. Wire dependencies (TaskUpdate with blockedBy)
        4. Create init waypoint sentinel
        5. Store active pathway ID in session state
```

### Manual Pathway Selection

For custom or hybrid workflows:

```
Skill('cf-working-protocol', args='select-pathway type=development')
```

Or via environment variable for Autorun:

```yaml
# batch.yaml
tasks:
  - id: T-001
    pathflow: development
    team_strategy: full
```

### Pathway State Persistence

Active pathway state stored at `/tmp/claude/managed/pathways/active-pathway.json`:

```json
{
  "pathway_id": "development",
  "template": "development.json",
  "instantiated_at": 1707123456,
  "session_id": "abc-123",
  "team_name": "cf-E-AUTH-001",
  "current_nodes": {
    "init": "completed",
    "planning": "completed",
    "checkpoint-plan-review": "completed",
    "implementation": "in_progress",
    "checkpoint-impl-review": "pending",
    "review": "blocked",
    "testing": "blocked",
    "finalize": "blocked"
  },
  "inserted_nodes": [],
  "checkpoint_reopens": {
    "checkpoint-plan-review": 1
  },
  "skipped_nodes": []
}
```

---

## 7. Integration with Agent Teams

### Team Lead as Pathway Manager

The team lead manages the pathway lifecycle:

```
Team Lead Responsibilities:
  1. Select pathway at session/work start
  2. Instantiate task graph from template
  3. Assign tasks to teammates based on node agent_role
  4. Monitor checkpoint gates
  5. Approve/reject checkpoint completions
  6. Insert dynamic nodes when needed
  7. Skip nodes in emergencies (with documentation)
  8. Manage checkpoint re-opens
  9. Verify finalize waypoint at session end
```

### Teammate as Node Executor

Each teammate works on assigned nodes:

```
Teammate Workflow:
  1. Check TaskList for claimable tasks (unblocked, no owner)
  2. Claim task (TaskUpdate with owner)
  3. PreToolUse pathway gate validates predecessor sentinels
  4. Execute work per node acceptance criteria
  5. On completion: pathway engine creates pathway sentinel
  6. Mark task completed (TaskUpdate)
  7. Check TaskList for next available task
```

### Checkpoint Nodes as Plan Approval Gates

Checkpoint nodes naturally map to Agent Teams' plan approval workflow:

```
cf-developer completes implementation
    │
    ▼
PathFlow engine creates sentinel for "implementation" node
    │
    ▼
Team lead reviews at checkpoint-impl-review
    │
    ├── Approved → sentinel created, downstream unblocked
    │
    └── Rejected → checkpoint re-opened, implementation re-assigned
        └── sentinel deleted, review/testing re-blocked
```

---

## 8. Predefined Pathway Examples

### Development Pathway (Full)

```
init ──▶ planning ──▶ [checkpoint-plan] ──▶ implementation ──▶ [checkpoint-impl] ──▶ review ─┐
                                                                                    testing ─┤──▶ finalize
```

8 nodes, 2 checkpoints, 1 parallel branch, enforced by 8 pathway sentinels.

### Bugfix Pathway (Streamlined)

```
init ──▶ investigation ──▶ fix ──▶ test ──▶ finalize
```

5 nodes, 0 checkpoints (speed over ceremony), enforced by 5 pathway sentinels.

### Hotfix Pathway (Minimal)

```
init ──▶ fix ──▶ test ──▶ ship
```

4 nodes, 0 checkpoints, post-merge review flagged. Shortest path.

### Planning Pathway

```
init ──▶ requirements ──▶ breakdown ──▶ [checkpoint-scope] ──▶ document ──▶ finalize
```

6 nodes, 1 checkpoint (scope approval), enforced by 6 pathway sentinels.

### Review Pathway

```
init ──▶ load-context ──▶ analyze ──▶ document-findings ──▶ finalize
```

5 nodes, 0 checkpoints (reviewer is the quality gate), enforced by 5 pathway sentinels.

### Research Pathway

```
init ──▶ search ──▶ [checkpoint-scope] ──▶ deep-analysis ──▶ synthesize ──▶ finalize
```

6 nodes, 1 checkpoint (scope validation), enforced by 6 pathway sentinels.

---

## 9. Implementation Plan

### Phase 1: Foundation (Can Start Now)

1. **Create pathway template directory**: `.codeflow/config/pathways/`
2. **Define JSON schema** for pathway templates
3. **Create 3 initial templates**: development.json, bugfix.json, planning.json
4. **Extend sentinel library** with pathway sentinel functions:
   - `pathway_sentinel_create(pathway_id, node_id, ...)`
   - `pathway_sentinel_validate(pathway_id, node_id)`
   - `pathway_sentinel_delete(pathway_id, node_id)` (for checkpoint re-open)
   - `pathway_sentinel_list(pathway_id)` (for status display)

### Phase 2: Enforcement (After Phase 4 - Agents)

5. **Create enforcement hook**: `cf-pre-tool-use-pathflow-gate.sh`
6. **Create pathway management skill**: `cf-pathway-management` with operations:
   - `select-pathway` — Choose and instantiate pathway
   - `insert-node` — Dynamic node insertion
   - `reopen-checkpoint` — Checkpoint re-opening
   - `skip-node` — Emergency skip with audit
   - `pathway-status` — Current pathway state display
7. **Integrate with Agent Teams**: Pathway instantiation creates Agent Teams tasks

### Phase 3: Integration (Phase 6+)

8. **Auto-detect pathway from command**: `/cf-develop` auto-selects development pathway
9. **Autorun integration**: batch.yaml `pathway` field support
10. **Pathway metrics**: Track completion rates, checkpoint failure rates, common insertion patterns

---

## 10. Comparison: This Approach vs LangGraph

| Aspect | LangGraph | CodeFlow PathFlow |
|--------|-----------|------------------------|
| Graph definition | Python code (StateGraph) | JSON config templates |
| Node execution | Python functions | Agent Teams teammates |
| Edge conditions | Python conditionals | Sentinel existence checks |
| State management | TypedDict / Pydantic | Pathway sentinels + session state |
| Enforcement | Hard (code-level) | Hard (sentinel + hook gates) |
| Dynamic modification | Limited (compile-time) | Full (runtime insertion, re-open, skip) |
| Checkpoints | Built-in persistence | Sentinel-based with re-open |
| Parallel branches | Supported (fan-out/fan-in) | Supported (multi-predecessor blocking) |
| Human-in-the-loop | interrupt_before/after | Checkpoint nodes with lead approval |
| Retry | Built-in retry logic | Checkpoint re-open with max attempts |
| Visualization | LangGraph Studio | Pathway status display (TaskList) |

### What This Approach Adds Over LangGraph

1. **Config-driven, not code-driven**: Pathways are JSON templates, not Python code. Non-developers can modify workflows.
2. **Dynamic runtime modification**: Nodes can be inserted, skipped, or re-opened mid-execution. LangGraph graphs are compiled.
3. **Multi-agent native**: Each node maps to a specialized agent role. LangGraph requires custom agent orchestration.
4. **Audit trail built-in**: Sentinel creation/deletion logged to JSONL. LangGraph requires custom logging.
5. **Layered enforcement**: Pathway sentinels layer on top of existing skill sentinels, creating defense-in-depth.

### What LangGraph Does Better

1. **Conditional edges**: LangGraph can route based on state values (if/else branching). PathFlow engine currently only supports fixed dependencies.
2. **Streaming**: LangGraph streams intermediate results. Agent Teams uses message passing.
3. **Persistence**: LangGraph has built-in checkpointing for crash recovery. PathFlow engine relies on sentinel files (volatile).
4. **Ecosystem**: LangGraph integrates with LangChain, LangSmith. PathFlow engine is CodeFlow-specific.

---

## 11. Open Questions

1. **Conditional branching**: Should pathways support conditional edges? E.g., "if review finds security issues, insert security-audit node automatically." This would require node completion callbacks.

2. **Cross-pathway communication**: If two Autorun workers use different pathways, should their pathway sentinels be visible to each other? Currently pathways are session-scoped.

3. **Pathway versioning**: When a pathway template is updated mid-session, should active instances migrate? Likely no — lock version at instantiation.

4. **Pathway composition**: Should complex workflows be composable from simpler pathways? E.g., "development = planning-pathway + implementation-pathway + review-pathway."

5. **Metrics and optimization**: Track which pathways are most used, where checkpoints fail most, which nodes take longest. Use data to optimize pathway templates over time.
