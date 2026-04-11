# Part 6: Changes from M1 Design

> Side-by-side comparison showing what changed from the original M1 design and why.

---

## Summary Table

| Aspect | M1 Design | Revised Design | Why |
|--------|-----------|---------------|-----|
| PathFlow structure | 9-node outer shell + pluggable inner pathways | 7-phase single flow with work stages inside PF-4 | Simpler, one unified flow |
| Naming | OS-1 through OS-9, inner pathway templates | PF-1 through PF-7, WS-DEV/REV/QA | Clearer, hierarchical |
| Teammate model | Role-based only | Function (persistent SOPs) + Role (on-demand variable work) | Functions like git/memory benefit from persistence |
| cf-gitops | Didn't exist (git was everyone's responsibility) | Dedicated persistent function teammate for all git ops | Consistent git procedures, loaded once |
| cf-knowledge | Didn't exist (cf-memory-management dissolved) | Dedicated persistent function teammate for Knowledge Layer | Single interface to DB/JSONL/WorkGraph |
| cf-task-management | "Dissolved into native TaskCreate" | PRESERVED as cf-knowledge's SOP for WorkGraph CRUD | WorkGraph != Claude Tasks. WorkGraph is permanent. |
| Communication | Hub-and-spoke through lead | Direct peer communication + lead orchestration | Leverages Agent Teams' key advantage |
| Delegate mode | Hook-based enforcement | Instruction-based guidance | Simpler, sufficient |
| Context monitoring | Custom hook-based self-reporting | Dropped | Not reliable without native support |
| Configurability | 4 layers, 3 enforcement policies, 4 session types | Tracked vs Untracked mode, interactive vs autorun property | Radically simplified |
| Sentinels | Session-scoped TTL-free pathway sentinels | Same, but fewer needed due to PathFlow phase coverage | Simplification |
| Multi-pathway | Deferred to future | Core feature: work stages (DEV -> REV -> QA) with conditional routing | Essential for real development workflows |
| WorkGraph task status | No changes | Add stage, stage_status, stage_history columns | Enables multi-stage tracking |
| V3 impact | V4 recommended | V3 modification (additive) | Changes are additions, not replacements |

---

## Key Corrections

### 1. Claude Tasks != WorkGraph

The M1 design conflated Claude's ephemeral Task system (TaskCreate/TaskList) with CodeFlow's persistent WorkGraph (epics/tasks in SurrealDB/JSONL). The revised design clearly separates these:

- **Claude Tasks**: Session orchestration (PathFlow phases + actual work items). Ephemeral.
- **CodeFlow WorkGraph**: Project management (epics, tasks, acceptance criteria). Persistent.

cf-task-management operations (classify-work, ensure-work-registered, create-epic, etc.) are preserved and embedded in cf-knowledge's SOP.

### 2. Function vs Role Teammates

The M1 design treated all teammates as role-based (on-demand). The revised design recognizes that some operations (git, knowledge layer) are:
- Procedural (follow SOPs exactly)
- Repeated (many times per session)
- Cumulative (benefit from session context)

These are better served by persistent function teammates that load SOPs once and handle all operations of their type.

### 3. Direct Peer Communication

The M1 design funneled all communication through the team lead (hub-and-spoke), which recreates the sub-agent model with extra overhead. The revised design embraces Agent Teams' direct messaging: cf-developer messages cf-gitops directly for commits, cf-knowledge directly for WorkGraph updates.

### 4. Simplified Configurability

The M1 design had 4 configuration layers, 3 enforcement policies, 4 session types, solo mode, and session profiles. The revised design reduces this to:
- **Two modes**: Tracked (enforcement on) vs Untracked (no enforcement)
- **Two session properties**: Interactive vs Autorun, has_existing_task (boolean)
- **Team spawning**: Pragmatic (spawn when beneficial), not modal

---

## What Remains Unchanged from M1

These M1 design elements are still valid and carried forward:

- PathFlow as a logical progression framework (refined, not replaced)
- Task graph using Claude's Task system with dependency ordering
- Phase markers coexisting with actual tasks in the same list
- Sentinel-based defense-in-depth enforcement
- Skills preserved alongside agent definitions
- Agent definition format loaded at spawn time
- Session-scoped PathFlow sentinels (no TTL)

---

## Document Lineage

| M1 Document | Status | Revised Counterpart |
|------------|--------|-------------------|
| 01-v3-session-lifecycle-analysis.md | Valid reference | (No change needed) |
| 02-agent-skill-teammate-mapping.md | Valid reference | (Informed 03-teammate-model.md) |
| 03-feasibility-findings.md | Valid reference | (Constraints still apply) |
| 04-outer-shell-pathflow.md | Superseded | 02-revised-architecture.md |
| 05-skill-to-teammate-model.md | Partially superseded | 03-teammate-model.md |
| 06-configurability-framework.md | Superseded | Simplified to tracked/untracked in 01-feedback-analysis.md section 1.10 |
| 07-use-case-analysis.md | Needs revision | Use cases need updating to match new 7-phase model |
| M1-design-checkpoint.md | Historical | This folder (pathflow-revised/) is the current design |
