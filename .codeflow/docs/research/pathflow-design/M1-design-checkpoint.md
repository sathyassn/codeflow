# PathFlow Design: Milestone 1 Check-In

> Core design complete. Ready for review before proceeding to V3 impact analysis and implementation planning.
> Date: 2026-02-06

---

## Executive Summary

PathFlow is CodeFlow's session lifecycle engine that transforms every Claude Code session into an orchestrated Agent Team with enforced workflow paths. The Milestone 1 design phase is complete, producing 7 detailed design documents covering the full system architecture.

**The core idea works.** Agent Teams provides the graph structure (task dependencies, team coordination, message passing); CodeFlow's sentinel system provides the enforcement layer (hard gates via PreToolUse hooks); JSON pathway templates provide the configuration. Together they create a dynamic, configurable workflow engine comparable to LangGraph but built on Claude Code's native infrastructure.

---

## What Was Designed

### 7 Design Documents

| # | Document | Lines | Content |
|---|----------|:-----:|---------|
| 01 | V3 Session Lifecycle Analysis | 637 | Complete V3 lifecycle mapping: 78 skill operations, 26 hooks, cross-skill dependency chains, sentinel prerequisite chains |
| 02 | Agent/Skill to Teammate Mapping | 575 | All 8 agents and 11 skills analyzed with persistence recommendations, blueprint format, communication model shift |
| 03 | Feasibility Findings | 318 | Delegate mode (interactive-only), read delegation (still needed), work graph (lazy registration), context monitoring (hook-based), settings gaps |
| 04 | Outer Shell PathFlow | 1,263 | 9-node generic session lifecycle: OS-1 through OS-9 with sentinel chain, session profiles, inner pathway plug-in, dynamic modification rules |
| 05 | Skill-to-Teammate Model | 932 | Blueprint specification, persistence model (4 types), spawn prompt builder, complete skill dissolution map, team composition templates, context management |
| 06 | Configurability Framework | ~800 | 4-layer configuration hierarchy, master config schema, enforcement policies (strict/standard/permissive), session profiles, runtime overrides |
| 07 | Use Case Analysis | ~750 | 4 full walkthroughs: structured development, ad-hoc fix, autorun batch, exploratory research — node-by-node traversal with sentinel timelines |

All documents are at: `.codeflow/docs/research/pathflow-design/`

---

## Core Architecture

### The Two-Layer Model

```
OUTER SHELL (session lifecycle — same for all sessions)
┌─────────────────────────────────────────────────────────────┐
│ OS-1    OS-2      OS-3     OS-4        OS-5      OS-6-9    │
│ Boot → Context → Team → Work Reg → [INNER] → Finalize    │
│ (sys)  (lead)   (lead)  (ckpt)     (team)   → Verify     │
│                                               → Close      │
│                                               → End        │
└─────────────────────────────────────────────────────────────┘

INNER PATHWAY (work-specific — plugged in at OS-5)
┌─────────────────────────────────────────────────────────────┐
│ development.json | bugfix.json | planning.json | review.json│
│ research.json    | hotfix.json | testing.json  | docs.json  │
│                                                             │
│ Each contains: nodes → dependencies → sentinel requirements │
│ Selected based on: command, branch pattern, work type       │
└─────────────────────────────────────────────────────────────┘
```

### How Enforcement Works

```
Tool Invocation (Edit, Write, Bash, etc.)
    │
    ├── Layer 0: settings.json deny list (Claude Code native)
    │
    ├── Layer 1a: Skill sentinels (existing CodeFlow enforcement)
    │   e.g., complete-work sentinel required before git commit
    │
    ├── Layer 1b: Pathway sentinels (NEW — PathFlow enforcement)
    │   e.g., pathflow:shell:work-reg required before any Edit/Write
    │   e.g., pathflow:development:plan required before implementation
    │
    ├── Layer 2: Stop hook (PCV verification — existing)
    │
    └── Layer 3: Advisory hooks (logging, guidance — existing)

Both Layer 1a AND Layer 1b must pass. They are complementary.
```

### Team Model

| Role | Persistence | When Spawned | Purpose |
|------|:-----------:|:-------------|---------|
| Team Lead | PERSISTENT | Always (is the session) | Orchestrates outer shell, assigns tasks, manages checkpoints |
| cf-developer | PERSISTENT | OS-3 (for dev/fix sessions) | Core implementation work |
| cf-planner | ON-DEMAND | When planning needed | Epic/task breakdown |
| cf-reviewer | ON-DEMAND | When review needed | Code review |
| cf-qa | ON-DEMAND | After implementation | Test writing/execution |
| cf-ops | ON-DEMAND | For shipping | PR creation, deployment |
| cf-documenter | ON-DEMAND | When docs needed | Documentation |
| cf-support | SUB-AGENT | On help request | Read-only assistance |

**Configurable**: Persistence is per-session overridable (e.g., promote cf-planner to persistent for intensive planning sessions).

### Configurability

4-layer configuration hierarchy:
```
Layer 0: Hardcoded defaults (PathFlow engine)
    ↓
Layer 1: Pathway template defaults (outer-shell.json, development.json, etc.)
    ↓
Layer 2: Session profile (interactive, autorun, structured, unstructured)
    ↓
Layer 3: Runtime overrides (team lead modifies during session)
```

3 enforcement policies: `strict` (all gates enforced, no skipping) → `standard` (configurable relaxation) → `permissive` (advisory only, maximum flexibility).

---

## Key Design Decisions Made

| # | Decision | Choice | Why |
|---|----------|--------|-----|
| 1 | Outer shell topology | Linear 9-node chain (no branching) | Session types differ in behavior, not structure. Branching adds complexity without value. |
| 2 | Team init before work registration | OS-3 before OS-4 | Team needs to be ready before work starts. Strategy can be refined after classification. |
| 3 | Inner pathway plug-in | Clean boundary at OS-4→OS-5 | Separation of lifecycle (outer) from workflow (inner). Any inner pathway can run in any session. |
| 4 | Blueprint loading | Instruct-to-read (not inline) | Saves spawn prompt tokens (~400 vs ~2,700). One Read call is negligible overhead. |
| 5 | Skill dissolution | Embed in blueprints, not centralize | Skills are behavioral instructions, not services. Embedding avoids indirection. |
| 6 | Delegate mode enforcement | Hook-based (not built-in) | Built-in delegate mode is interactive-only. Hooks provide programmatic enforcement. |
| 7 | Context monitoring | Hook-based self-reporting | No built-in API. PostToolUse hooks on teammates report context usage at thresholds. |
| 8 | Communication model | Hub-and-spoke via team lead | Matches Agent Teams design. Lead mediates all coordination. |
| 9 | Work tracking | Lazy registration with ongoing epics | Follows V3 pattern. No hard gate rejecting untracked work. |
| 10 | Network operations | cf-ops exclusive | Centralizes risky operations. Easier to audit and control. |

---

## Open Questions for Discussion

### Q1: V3 Modification vs V4 New Spec?

The PathFlow design represents a fundamental architectural shift:
- V3: Hub-and-spoke sub-agents, skill-based operations, Knowledge Layer (DB) communication
- PathFlow: Agent Teams, teammate blueprints, message-based communication, sentinel + task graph enforcement

**Options**:
- **Modify V3**: Add Agent Teams as an optional mode. V3 skills remain for non-team sessions, PathFlow activates when teams are enabled. Backward-compatible but adds complexity.
- **Create V4**: Clean break. PathFlow is the primary architecture. V3 skill system deprecated. Simpler design but migration path needed.
- **Hybrid**: V3 spec untouched. PathFlow is an "overlay" spec that transforms V3 primitives when Agent Teams is enabled.

**Recommendation**: Create V4. The architecture shift is too fundamental for a V3 modification. V3 remains the reference for non-team sessions. V4 assumes Agent Teams as the default.

### Q2: Multi-Pathway Sessions

Can a single session traverse the outer shell multiple times? (e.g., user does a fix, then a review, then documentation)

**Options**:
- **Single traversal**: Each session = one outer shell pass. New work = new session.
- **Multi-traversal**: OS-7 can loop back to OS-4 for a new inner pathway.
- **Nested sessions**: Virtual sub-sessions within one Claude Code session.

**Current design**: Single traversal. Multi-traversal is a natural extension (OS-7 → OS-4 loop).

### Q3: Cross-Session Teammate Persistence

Agent Teams teammates don't persist across sessions. But V3's Knowledge Layer (JSONL + SQLite) provides cross-session memory. How should PathFlow handle cross-session context?

**Options**:
- Continue using JSONL/SQLite for cross-session memory (separate from task system)
- Use MEMORY.md auto-memory for lightweight cross-session context
- Use task metadata for structured cross-session state
- Combination of all three

### Q4: Maximum Concurrent Teammates

No documented hard limit. Practical limit is API rate limits and total context budget.

**Recommendation**: Cap at 3 concurrent active teammates (lead + 2) for initial implementation. The lead typically only needs 1-2 active at any time anyway (pipeline model, not parallel model).

### Q5: Session-Length PathFlow Sentinel TTL

Session-lifetime sentinels need a TTL. Sessions range from 5 minutes to 2+ hours.

**Current design**: 14400s (4 hours) with cleanup on next session start.
**Alternative**: No-expiry sentinels with explicit cleanup.

### Q6: Conditional Branching in Pathways

Current inner pathways are static dependency graphs. Should they support conditional edges? (e.g., "if review finds security issues, auto-insert security-audit node")

**Current design**: No conditional edges. Dynamic node insertion by the team lead handles this manually.
**Future consideration**: Condition-based auto-insertion rules in pathway templates.

---

## What Comes Next: Milestone 2

After check-in approval, the next phase is **V3 Impact Analysis**:

1. **V3 vs V4 Decision** — Based on discussion of Q1 above
2. **Spec Gap Analysis** — What V3 spec sections need modification or replacement
3. **Migration Path** — How to transition from current V3 implementation to PathFlow
4. **Implementation Phases** — Ordered implementation plan with dependencies
5. **Test Strategy** — How to validate PathFlow works end-to-end
6. **Settings/Hook Changes** — Concrete changes to `.claude/settings.json` and hooks

---

## Milestone 2+ Roadmap

| Milestone | Focus | Deliverable |
|-----------|-------|-------------|
| M1 (complete) | Core design | 7 design documents |
| M2 | V3 impact analysis | Spec gap analysis, V3/V4 decision, migration path |
| M3 | Implementation plan | Phased implementation with dependencies, test strategy |
| M4 | Blueprint creation | Actual `.claude/agents/cf-*.md` files |
| M5 | Pathway templates | Actual `.codeflow/config/pathways/*.json` files |
| M6 | Hook implementation | PathFlow gate hook, updated existing hooks |
| M7 | Integration testing | End-to-end validation with real sessions |

---

## Document Index

```
.codeflow/docs/research/pathflow-design/
├── M1-design-checkpoint.md          ← YOU ARE HERE
├── 01-v3-session-lifecycle-analysis.md
├── 02-agent-skill-teammate-mapping.md
├── 03-feasibility-findings.md
├── 04-outer-shell-pathflow.md
├── 05-skill-to-teammate-model.md
├── 06-configurability-framework.md
└── 07-use-case-analysis.md
```

Earlier research:
```
.codeflow/docs/research/
├── agent-teams-integration-proposal.md
├── agent-teams-test-findings.md
└── agent-teams-workflow-graph-sentinel-integration.md
```
