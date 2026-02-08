# PathFlow Revised Design

> First-principles revision of PathFlow based on M1 feedback. Simplifies architecture, corrects conflations, rethinks teammate model, and grounds every decision.
> Date: 2026-02-06

---

## Table of Contents

| # | Document | Description |
|---|----------|-------------|
| 01 | [Feedback Analysis](01-feedback-analysis.md) | First-principles reasoning through all M1 feedback (14 topics) |
| 02 | [Revised Architecture](02-revised-architecture.md) | 7-phase PathFlow, work stages, task graph model, sentinel model |
| 03 | [Teammate Model](03-teammate-model.md) | Function vs role teammates, roster, agent definition format, skills coexistence |
| 04 | [WorkGraph Schema](04-workgraph-schema.md) | Schema additions for multi-stage tracking (stage, stage_status, stage_history) |
| 05 | [Open Questions & Next Steps](05-open-questions-next-steps.md) | 6 open questions and what's needed before M2 |
| 06 | [Changes from M1](06-changes-from-m1.md) | Side-by-side comparison of M1 vs revised design |

---

## Executive Summary

PathFlow is a **logical progression framework** that guides Claude Code sessions through a series of phases. It uses Claude's Task system as its implementation mechanism, with phase markers and actual tasks coexisting in the same task list.

### Core Architecture (Revised)

```
PF-1: Session Start
  |
PF-2: Context Awareness
  |
PF-3: Work Classification
  |
PF-4: Work Execution  <-- contains work stages (WS-DEV, WS-REV, WS-QA, WS-DEPLOY)
  |
PF-5: Work Verification
  |
PF-6: Work Completion
  |
PF-7: Session End
```

### Teammate Model (Revised)

| Category | Persistence | Examples | Purpose |
|----------|:-----------:|---------|---------|
| **Function** | PERSISTENT | cf-gitops, cf-knowledge | Follow SOPs for a specific operational domain |
| **Role** | ON-DEMAND | cf-developer, cf-planner, cf-reviewer, cf-qa, cf-documenter, cf-ops | Variable-context work on specific tasks |
| **Sub-agents** | EPHEMERAL | Explore type | Quick read-only lookups |

### Key Changes from M1

- 7 phases (not 9 nodes), single unified flow (not outer/inner)
- Function teammates (persistent SOPs) + role teammates (on-demand)
- Direct peer communication (not hub-and-spoke)
- Claude Tasks != CodeFlow WorkGraph (critical distinction preserved)
- Multi-stage work execution: DEV -> REV -> QA with conditional routing
- Radically simplified configurability (tracked vs untracked mode only)
- V3 modification (additive), not V4 replacement

### Key Distinction: Claude Tasks vs CodeFlow WorkGraph

| Aspect | Claude Task System | CodeFlow WorkGraph |
|--------|-------------------|-------------------|
| **What** | TaskCreate, TaskList, TaskUpdate | epics, tasks in SQLite/JSONL/Markdown |
| **Scope** | Single session, ephemeral | Cross-session, permanent |
| **Purpose** | Orchestrate PathFlow phases and teammate assignments | Track project work items, status, planning |
| **Analogy** | Sprint standup board (discarded after) | Jira/Linear project board (persists forever) |

---

## Document Lineage

This revised design supersedes:
- `M1-design-checkpoint.md` — Original M1 synthesis (still valid as historical reference)
- `04-outer-shell-pathflow.md` — Replaced by simplified 7-phase architecture
- `06-configurability-framework.md` — Replaced by simplified tracked/untracked model

These M1 research documents remain valid references:
- `01-v3-session-lifecycle-analysis.md` — V3 lifecycle mapping (still accurate)
- `02-agent-skill-teammate-mapping.md` — Skill/agent analysis (still useful)
- `03-feasibility-findings.md` — Platform constraints (still accurate)
- `05-skill-to-teammate-model.md` — Blueprint format (partially superseded by 03-teammate-model.md)
- `07-use-case-analysis.md` — Use cases (need revision to match new phases)

---

## Status

**Milestone 1: COMPLETE** — Awaiting user review of revised design before proceeding to M2 (V3 impact analysis and implementation planning).
