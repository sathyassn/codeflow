# Command File Slimming Plan

## Current State

14 commands totaling 6,439 lines (~45K tokens). All follow the same 10-section format with significant cross-command duplication.

## Common 10-Section Structure (Every Command)

| # | Section | Typical Lines | Content Type | Restructuring Action |
|---|---------|:------------:|-------------|---------------------|
| WP | Working Protocol | ~17 | Duplicated boilerplate | Replace with single-line reference |
| 1 | Purpose & Usage | ~25 | Unique per command | KEEP |
| 2 | Arguments & Flags | ~40 | Unique per command | KEEP |
| 3 | Prerequisites | ~30 | Unique per command | KEEP |
| 4 | Workflow Definition | ~70 | Mixed: diagram + steps | KEEP diagram (as SPINE), slim steps to skill refs |
| 5 | Skills Integration | ~10 | Duplicated boilerplate | Replace with skill references in workflow |
| 6 | Hooks Integration | ~35 | Mostly duplicated | Keep only command-specific hooks |
| 7 | Memory Integration | ~30 | Partially duplicated | Keep unique; remove Three-Tier table |
| 8 | Error Handling | ~40 | Unique per command | KEEP |
| 9 | Examples | ~90 | Unique per command | KEEP |
| 10 | References | ~10 | Duplicated boilerplate | Slim to skill refs |

## Duplicated Content Across 14 Commands

| Pattern | Per Command | Total (x14) | Restructuring |
|---------|:----------:|:----------:|---------------|
| Working Protocol preamble | ~17 | ~238 | Single line: "Apply cf-working-protocol skill" |
| Three-Tier Data Model table | ~8 | ~112 | Remove (lives in cf-knowledge-layer agent def) |
| Skills Integration boilerplate | ~10 | ~140 | Merge into workflow diagram skill annotations |
| General hooks tables (non-command-specific) | ~15 | ~210 | Remove (lives in CLAUDE.md S8 Capabilities) |
| **Total duplicated** | **~50** | **~700** | **Saved: ~700 lines across 14 commands** |

## Per-Command Detail

| Command | Lines | Removable | Target | Primary Skill References |
|---------|:-----:|:---------:|:------:|--------------------------|
| cf-develop.md | 509 | ~160 | ~350 | cf-pathflow-protocol (execute-stage, classify-work), cf-team-communication |
| cf-plan.md | 398 | ~100 | ~300 | cf-pathflow-protocol (execute-stage), cf-team-communication |
| cf-ship.md | 485 | ~140 | ~350 | cf-pathflow-protocol (complete-session, verify-work), cf-git-workflow (create-pr) |
| cf-resume.md | 400 | ~100 | ~300 | cf-pathflow-protocol (recover-session, load-context) |
| cf-autorun.md | 656 | ~50 | ~600 | cf-pathflow-protocol (execute-stage), unique autorun logic |
| cf-document.md | 546 | ~160 | ~390 | cf-pathflow-protocol (execute-stage), cf-team-communication |
| cf-cleanup.md | 525 | ~50 | ~475 | cf-pathflow-protocol (end-session), cf-git-workflow |
| cf-deploy.md | 499 | ~160 | ~340 | cf-pathflow-protocol (execute-stage), cf-team-communication |
| cf-test.md | 454 | ~140 | ~320 | cf-pathflow-protocol (execute-stage), cf-team-communication |
| cf-doctor.md | 437 | ~50 | ~390 | Unique diagnostic logic (minimal skill refs) |
| cf-review.md | 423 | ~140 | ~285 | cf-pathflow-protocol (execute-stage), cf-team-communication |
| cf-help.md | 404 | ~50 | ~355 | Unique help content (minimal skill refs) |
| cf-stack.md | 355 | ~50 | ~305 | Unique state display logic (minimal skill refs) |
| cf-approval-mode.md | 348 | ~50 | ~300 | Unique mode switching logic (minimal skill refs) |
| **Totals** | **6,439** | **~1,400** | **~5,060** | |

## Command Grouping by Restructuring Impact

| Group | Commands | Removable/Cmd | Notes |
|-------|----------|:------------:|-------|
| **Pipeline commands** (spawn teammates, run stages) | cf-develop, cf-plan, cf-document, cf-deploy, cf-test, cf-review | ~150 | Highest savings: Working Protocol + Skills Integration + Hooks + Three-Tier all duplicated. Workflow diagram becomes SPINE with cf-pathflow-protocol annotations. |
| **Lifecycle commands** (PR, resume, cleanup) | cf-ship, cf-resume, cf-cleanup | ~100 | Medium savings: Working Protocol + Three-Tier duplicated. Unique lifecycle logic stays. Note: cf-ship is grouped here by conceptual category (lifecycle), but is assigned to INF-TSK-019-011 batch 1 (pipeline commands) operationally because its restructuring effort aligns with that batch. Task files are authoritative for implementation batching. |
| **Utility commands** (status, help, diagnostics) | cf-autorun, cf-doctor, cf-help, cf-stack, cf-approval-mode | ~50 | Lowest savings: Mostly unique content. Only Working Protocol preamble and general hooks removable. |

## Target Command Structure (SPINE Pattern)

Every restructured command follows the Workflow Diagram as SPINE pattern (Principle 6). Every workflow diagram MUST have accompanying execution steps (1:1 mapping). Complex workflows MUST be decomposed: high-level overview diagram + detailed sub-diagrams per branch/scenario.

```text
/cf-{command}:

    ## Workflow Overview  <-- SPINE diagram with skill annotations
    ## Prerequisites      <-- serves CHECK node
    ## Steps              <-- one subsection per workflow node
    ## Error Handling     <-- serves error branches
    ## Examples           <-- KEPT in full (unique, high value)
    ## Working Protocol   <-- single line reference

Removed sections:
    - Skills Integration (merged into workflow annotations)
    - Hooks Integration (generic hooks removed; command-specific stay in Steps)
    - Memory Integration / Three-Tier table (removed; in agent def)
    - References (merged into workflow skill annotations)
```

Restructured command template (~300 lines):

```text
---
name: cf-{command}
description: "{purpose}"
---

# /cf-{command}

## Workflow Overview  <-- THE SPINE
    ASCII art diagram with annotations:

    USER invokes /cf-{command}
        |
        v
    CHECK prerequisites                        --> Prerequisites section below
        |
        v
    STEP 1: {action}                            --> Step 1 below
        |                                           cf-{skill}: {operation}
        v
    STEP 2: {action}                            --> Step 2 below
        |                                           cf-{skill}: {operation}
        v
    ...
        |
        v
    COMPLETE                                    --> output summary to user

    Every node points down to a step section and across to a skill operation.

## Prerequisites
    Table of requirements (serves the CHECK node in the SPINE)

## Steps
    One subsection per workflow node (matching diagram 1:1):

    ### Step 1: {action}
        WHAT to do and WHEN.
        For HOW: load cf-{skill} skill, {operation} operation.

    ### Step 2: {action}
        ...

## Error Handling
    Table of errors and resolutions (serves any error branch in the SPINE)

## Working Protocol
    "Apply cf-working-protocol skill throughout execution."
    (single line, not full table)
```

## Command Content Movement Verification

Commands are loaded on-demand when the user invokes them. Common sections duplicated across all 14 commands are replaced with compact references.

| Duplicated Content | Per Command | Total (x14) | Replacement | Agent Access Preserved? |
|-------------------|:----------:|:----------:|-------------|----------------------|
| Working Protocol preamble | ~17 lines | ~238 | Replace with 2-line reference: `"Working Protocol: cf-working-protocol (loaded at session start, applies to all execution)."` | YES -- cf-working-protocol is already loaded before any command runs. The preamble was a redundant reminder. |
| Three-Tier Data Model table | ~8 lines | ~112 | Replace with 1-line reference: `"Data model: Three-tier (JSONL -> SQLite -> Markdown). See CLAUDE.md Section 10."` | YES -- CLAUDE.md Section 10 stays always-loaded. Commands can reference it by section number. |
| Skills Integration boilerplate | ~10 lines | ~140 | Replace with 2-line reference: `"Skills: Load on-demand per standards files. See CLAUDE.md Section 8."` | YES -- skill loading is a session-level concern, not a per-command concern. |
| General hooks tables | ~15 lines | ~210 | Replace with 1-line reference: `"Hooks: See CLAUDE.md Section 8 (Hooks) for complete hook inventory."` | YES -- CLAUDE.md Section 8 stays always-loaded with the hook inventory. |

**What commands KEEP (unchanged):** Purpose & Usage, Arguments & Flags, Prerequisites, Workflow Diagram (ASCII), Execution Steps (command-specific), Error Handling (command-specific), Examples. These are the unique value of each command.

## Aggregate Impact

| Category | Current Avg | Target Avg | Reduction |
|----------|:----------:|:----------:|:---------:|
| Per command | ~460 lines | ~310 lines | ~33% |
| **Total (14 files)** | **~6,439** | **~4,340** | **~33%** |

| Component | Current Avg Tokens | Target Avg Tokens | Savings |
|-----------|:-----------------:|:----------------:|:-------:|
| Command file | ~3.2K | ~2.1K | ~1.1K (34%) |
| **All 14 commands** | **~45K** | **~29K** | **~16K (34%)** |
