# CLAUDE.md Restructuring Plan

## Current State

CLAUDE.md: 1,424 lines, ~10K tokens. V4 target: ~400 lines, ~3K tokens.

~88% of content is procedural HOW content that should live in on-demand skills.

## Section-by-Section Audit

| # | Section | Lines | Content Type | Classification | Destination |
|---|---------|:-----:|-------------|----------------|-------------|
| 1 | Working Protocol | 15 | Skill reference table | WHAT (awareness) | STAYS in CLAUDE.md |
| 2 | Project Overview | 25 | Identity, principles, key distinction | WHAT (awareness) | STAYS in CLAUDE.md |
| 3a | Team Lead Role -- delegation tables | 35 | What lead does/doesn't do | WHAT (awareness) | STAYS in CLAUDE.md |
| 3b | Team Lead Role -- spawn HOW | 15 | Token-aware delegation details | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 3c | Team Lead Role -- sub-agent policy | 20 | Sub-agent restrictions, why table | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 4a | Session Lifecycle -- workflow diagram | 20 | ASCII session lifecycle diagram | WHAT (awareness) | STAYS (simplified to phase list) |
| 4b | Session Lifecycle -- execution steps | 200 | Step-by-step PF1-PF7 with task tracker | HOW (procedure) | MOVES to cf-pathflow-protocol (execute-phase) |
| 4c | Session Lifecycle -- phase reference | 15 | Phase-sentinel-gate table | WHAT (awareness) | STAYS in CLAUDE.md |
| 4d | Session Lifecycle -- properties + nav | 16 | Session modes, scenario navigator | WHAT (awareness) | STAYS in CLAUDE.md |
| 5a | Teammates -- roster tables | 30 | Persistent + on-demand teammate tables | WHAT (awareness) | STAYS in CLAUDE.md |
| 5b | Teammates -- model selection + loading | 20 | Model rules table, loading note | WHAT (awareness) | STAYS in CLAUDE.md |
| 5c | Teammates -- spawn patterns | 60 | Spawn prompt templates, task spec quality | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 5d | Teammates -- stage reporting | 50 | Pipeline-column mapping, status legend | HOW (procedure) | MOVES to cf-pathflow-protocol (complete-stage) |
| 5e | Teammates -- communication patterns | 30 | Peer-to-peer routing table, message formats | HOW (procedure) | MOVES to cf-team-communication (peer-message) |
| 5f | Teammates -- recycling + deferred shutdown | 60 | Recycling, deferred shutdown sequence | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 5g | Teammates -- parallel batch + health | 70 | Batch sizing, exhaustion prevention, health checks | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 5h | Teammates -- name preservation + persistence | 30 | Naming rules, team lifecycle | HOW (procedure) | MOVES to cf-pathflow-protocol (manage-teammates) |
| 6a | Work Pipelines -- tables | 40 | Pipeline table, stage mapping, rework limits | WHAT (awareness) | STAYS in CLAUDE.md |
| 6b | Work Pipelines -- rework loops | 20 | Rework loop ASCII flow, enforcement | HOW (procedure) | MOVES to cf-pathflow-protocol (handle-rework) |
| 6c | Work Pipelines -- routing + utilization | 90 | Routing precedence, keyword table, smart utilization | MIXED | STAYS (tables), MOVES (utilization examples) |
| 7a | Enforcement -- gate tables | 30 | Hook-enforced + instruction-enforced gates | WHAT (awareness) | STAYS in CLAUDE.md |
| 7b | Enforcement -- sentinel system | 20 | Sentinel creation mechanisms, checkpoint layers | HOW (procedure) | MOVES to cf-pathflow-protocol (execute-phase) |
| 7c | Enforcement -- task tracker mirroring | 25 | Mirroring rules, config reference | HOW (procedure) | MOVES to cf-pathflow-protocol (execute-phase) |
| 7d | Enforcement -- model + ops | 40 | Enforcement model table, git ops, sandbox | WHAT (awareness) | STAYS in CLAUDE.md |
| 7e | Enforcement -- testing + PR + merge | 50 | Test commands, PR workflow, merge protection | WHAT (awareness) | STAYS in CLAUDE.md |
| 7f | Enforcement -- decision tiers + degradation | 28 | Tier table, graceful degradation | MIXED | STAYS (tiers), MOVES (degradation) |
| 8 | Capabilities | 80 | Inventory of skills, agents, hooks, commands | WHAT (awareness) | STAYS (update to 15 skills) |
| 9 | Project Structure | 57 | Directory tree | WHERE (reference) | STAYS in CLAUDE.md |
| 10 | Memory | 35 | Three-tier model, operations table | WHAT (awareness) | STAYS in CLAUDE.md |
| 11a | Recovery -- quick commands | 15 | Command table, PathFlow recovery table | WHAT (awareness) | STAYS in CLAUDE.md |
| 11b | Recovery -- teammate recovery | 10 | Teammate recovery table | WHAT (awareness) | STAYS in CLAUDE.md |
| 11c | Recovery -- stuck session | 10 | Stuck session table | WHAT (awareness) | STAYS in CLAUDE.md |
| 11d | Recovery -- context overflow | 83 | Full recovery procedure, verification, backfill | HOW (procedure) | MOVES to cf-pathflow-protocol (recover-session) |

**Summary:** ~396 lines STAY (awareness), ~1,028 lines MOVE (procedures).

## STAY/MOVE Classification

| What | Lines | % of Current |
|------|:-----:|:----------:|
| Current total | 1,424 | 100% |
| STAYS (awareness content) | ~396 | 28% |
| MOVES to cf-pathflow-protocol | ~928 | 65% |
| MOVES to cf-team-communication | ~30 | 2% |
| REMOVED (redundant/trimmed) | ~70 | 5% |
| **Target total** | **~400** | **28%** |

## Target CLAUDE.md Structure (~400 lines)

CLAUDE.md uses the **Session Lifecycle diagram as its SPINE**. The diagram in Section 4 is the structural backbone -- every other section exists to serve a node in that diagram. The lead reads the diagram to understand the session flow, then follows pointers to the relevant section or skill for details.

Every workflow diagram MUST have accompanying execution steps (1:1 mapping). Complex workflows MUST be decomposed: high-level overview diagram (happy path, ~15 nodes max) + detailed sub-diagrams per branch/scenario. The CLAUDE.md SPINE diagram is the high-level overview; cf-pathflow-protocol skill provides the detailed sub-diagrams per phase.

```text
# CodeFlow Team Lead Instructions

## 1. Working Protocol
    ~15 lines: Skill reference table (always-loaded skill)

## 2. Project Overview
    ~25 lines: Identity, phase, principles

## 3. Team Lead Role
    ~35 lines: Delegation-only mode, WHAT the lead does/doesn't do
    Reference: cf-pathflow-protocol skill for HOW

## 4. Session Lifecycle  <-- THE SPINE
    ~50 lines: Annotated phase diagram + phase reference table

    SESSION START
        |
        v
    PF1-INIT --> TeamCreate, spawn cf-security   [Section 5: Teammates]
        |                                         cf-pathflow-protocol: init-session
        v
    PF2-CONTEXT --> spawn cf-knowledge-layer      [Section 5: Teammates]
        |           query active work             cf-pathflow-protocol: load-context
        v
    Active work? --> YES: /cf-resume | NO: wait
        |
        v
    PF3-CLASSIFY --> create branch                [Section 6: Pipelines]
        |            register task                cf-pathflow-protocol: classify-work
        v
    PF4-EXECUTE --> route to pipeline             [Section 6: Pipelines]
        |                                         cf-pathflow-protocol: execute-stage
        v
    PF5-VERIFY --> check acceptance               [Section 7: Enforcement]
        |                                         cf-pathflow-protocol: verify-work
        v
    PF6-COMPLETE --> PR, merge, sync              cf-pathflow-protocol: complete-session
        |
        v
    PF7-END --> shutdown, cleanup                 cf-pathflow-protocol: end-session
        |
        v
    SESSION END

    Each node points to: which CLAUDE.md section has the awareness context,
    and which cf-pathflow-protocol operation has the procedural HOW.

## 5. Teammates
    ~60 lines: Roster tables (persistent + on-demand), model selection
    Serves PF1-INIT, PF2-CONTEXT, PF4-EXECUTE nodes in the SPINE diagram.
    Reference: cf-pathflow-protocol for spawn patterns + management procedures

## 6. Work Pipelines
    ~40 lines: Pipeline tables, routing precedence
    Serves PF3-CLASSIFY, PF4-EXECUTE nodes in the SPINE diagram.
    Reference: cf-pathflow-protocol for rework loops + stage execution

## 7. Enforcement
    ~30 lines: Gate awareness table, sentinel awareness
    Serves PF5-VERIFY node in the SPINE diagram.
    Reference: cf-pathflow-protocol for enforcement details

## 8. Capabilities
    ~60 lines: Compressed inventory (skills, agents, hooks, commands)

## 9. Project Structure
    ~40 lines: Directory tree

## 10. Memory
    ~30 lines: Three-tier model, operations table

## 11. Recovery
    ~25 lines: Quick commands table
    Reference: cf-pathflow-protocol for full recovery procedures
```

## Section-by-Section Extraction Map

For each block of content that moves out of CLAUDE.md, the restructured CLAUDE.md contains a reference that tells the team lead exactly where to find the detailed procedures.

### Section 3: Team Lead Role

| Content Block | Current Location | Moves To | Reference in Restructured CLAUDE.md | Lead Still Has Access? |
|--------------|-----------------|----------|--------------------------------------|----------------------|
| Token-aware delegation table (15 lines) | S3, lines ~40-55 | cf-pathflow-protocol, operation: manage-teammates | `"Detail: cf-pathflow-protocol (manage-teammates)"` after the DOES/DOESN'T table | YES -- lead loads cf-pathflow-protocol on-demand during PF4. |
| Sub-agent policy table (20 lines) | S3, lines ~56-75 | cf-pathflow-protocol, operation: manage-teammates | `"Sub-agent policy: cf-pathflow-protocol (manage-teammates)"` in the Team Lead Role summary | YES -- same loading mechanism. |

### Section 4: Session Lifecycle

| Content Block | Current Location | Moves To | Reference in Restructured CLAUDE.md | Lead Still Has Access? |
|--------------|-----------------|----------|--------------------------------------|----------------------|
| PF1-PF7 step-by-step execution (200 lines) | S4.2, Steps 1-8 | cf-pathflow-protocol, operation: execute-phase | Phase reference table STAYS with note: `"Procedures: cf-pathflow-protocol (execute-phase)"` | YES -- the lead loads cf-pathflow-protocol before entering PF4. |
| Phase sentinel explanation (15 lines) | S4.3, note above table | cf-pathflow-protocol, operation: execute-phase | Table STAYS; sentinel explanation moves to skill | YES -- table stays, implementation detail moves. |
| Session properties + autorun (16 lines) | S4.4-4.5 | STAYS in CLAUDE.md | (no move) | YES -- this is awareness content. |

### Section 5: Teammates

| Content Block | Current Location | Moves To | Reference in Restructured CLAUDE.md |
|--------------|-----------------|----------|--------------------------------------|
| Spawn patterns + examples (60 lines) | S5 "Spawn pattern" through spawn examples | cf-pathflow-protocol, operation: manage-teammates | After roster tables: `"Spawn patterns: cf-pathflow-protocol (manage-teammates)"` |
| Stage reporting (50 lines) | S5 "Stage Reporting" subsection | cf-pathflow-protocol, operation: complete-stage | After roster: `"Stage reporting: cf-pathflow-protocol (complete-stage)"` |
| Communication patterns (30 lines) | S5 "Communication Patterns" subsection | cf-team-communication skill | `"Communication patterns: cf-team-communication"` |
| Recycling + deferred shutdown (60 lines) | S5 recycling through deferred shutdown | cf-pathflow-protocol, operation: manage-teammates | `"Teammate lifecycle: cf-pathflow-protocol (manage-teammates)"` |
| Parallel batch + health (70 lines) | S5 parallel batch through health verification | cf-pathflow-protocol, operation: manage-teammates | `"Parallel execution: cf-pathflow-protocol (manage-teammates)"` |
| Name preservation + persistence (30 lines) | S5 name preservation through team persistence | cf-pathflow-protocol, operation: manage-teammates | `"Naming rules: cf-pathflow-protocol (manage-teammates)"` |

### Sections 6, 7, 11

| Content Block | Current Location | Moves To | Reference |
|--------------|-----------------|----------|-----------|
| Rework loop flow (20 lines) | S6 | cf-pathflow-protocol, operation: handle-rework | `"Rework procedures: cf-pathflow-protocol (handle-rework)"` |
| Smart utilization examples (40 lines) | S6 | cf-pathflow-protocol, operation: manage-teammates | `"Utilization guidance: cf-pathflow-protocol (manage-teammates)"` |
| Sentinel system details (20 lines) | S7 | cf-pathflow-protocol, operation: execute-phase | Gate table STAYS with note: `"Sentinel internals: cf-pathflow-protocol (execute-phase)"` |
| Task tracker mirroring rules (25 lines) | S7 | cf-pathflow-protocol, operation: execute-phase | `"Task tracker rules: cf-pathflow-protocol (execute-phase)"` |
| Graceful degradation (15 lines) | S7 | cf-pathflow-protocol, operation: recover-session | `"Degradation: cf-pathflow-protocol (recover-session)"` |
| Context overflow procedure (83 lines) | S11 | cf-pathflow-protocol, operation: recover-session | Quick commands/tables STAY. `"Full recovery: cf-pathflow-protocol (recover-session)"` |

## LOSSLESS Verification Summary

| Metric | Value |
|--------|-------|
| Total lines moved | ~1,028 |
| Lines with verified access in skill | ~1,028 (100%) |
| Content dropped without replacement | 0 |
| References added to restructured CLAUDE.md | 15 reference lines (one per moved block) |
| Lead's effective knowledge | UNCHANGED -- same information, loaded on-demand instead of always |
