---
id: analysis-artifact-token-efficiency
title: "Artifact Token Efficiency Analysis and Restructuring Plan"
status: proposed
author: cf-planning
created: "2026-02-26"
task_ref: PLN-TSK-001-004
---

# Artifact Token Efficiency Analysis Package

## Problem Statement

CodeFlow's Claude artifacts -- CLAUDE.md, 8 agent definitions, 14 commands, and 6 skills -- total 13,782 lines (~96K tokens). The V4 specification targets ~2,650 tokens for CLAUDE.md alone; the current implementation is 1,424 lines (~10K tokens), roughly 4x over budget.

The core issue: CLAUDE.md mixes **awareness content** (WHAT teammates exist, WHEN to spawn them, WHERE things live) with **procedural content** (HOW to execute PathFlow phases step-by-step, HOW teammates coordinate, HOW enforcement works in detail). This violates the V4 design principle stated in `codeflow-specification-v4/05-claude-components/main-agent/01-claude-md-spec.md:68`:

> **Key Principle:** CLAUDE.md provides AWARENESS (WHAT/WHEN/WHERE), not PROCEDURES (HOW).

### How We Got Here: The V4 Phase 4 Flattening

Before V4 Phase 4, CodeFlow already had the correct skill-based architecture: skills defined functional domains with declaratively named operations, and agent definitions referenced those skills. During V4 Phase 4 (PathFlow Integration), all procedural content from skills was embedded directly into agent definitions. The original skills were archived to `.codeflow/docs/archived/skills/`.

This embedding "flattened" the skill layer -- it worked for migration purposes but created the duplication and token overhead documented here. **This restructuring restores the existing skill pattern, not invents a new one.**

### Root Causes

1. **CLAUDE.md contains procedural HOW content** -- ~88% HOW, target ~28% (awareness only)
2. **Agent definition duplication** -- Working Protocol, Communication patterns, stage completion protocol repeated across 8 agents (~680 lines duplicated)
3. **Command file bloat** -- Working Protocol copies, Three-Tier tables, general hooks tables repeated across 14 commands (~700 lines duplicated)
4. **Skill layer flattening** -- 10 archived skills embedded into agent defs during V4 Phase 4

### Size Inventory

| Artifact Category | Files | Lines | Est. Tokens | V4 Target Tokens | Over Budget |
|-------------------|-------|-------|-------------|-----------------|-------------|
| CLAUDE.md | 1 | 1,424 | ~10K | ~2,650 | ~3.8x |
| Agent definitions | 8 | 3,933 | ~27K | ~16K (est.) | ~1.7x |
| Commands | 14 | 6,439 | ~45K | ~30K (est.) | ~1.5x |
| Skills (active) | 6 | 1,986 | ~14K | ~14K | On target |
| **Total** | **29** | **13,782** | **~96K** | **~63K** | **~1.5x** |

## Token Savings Summary

| Metric | Current | Target | Savings |
|--------|---------|--------|---------|
| Always-loaded (every session) | ~13K tokens | ~6K tokens | ~7K (54%) |
| Function agent spawn (avg) | ~3.2K tokens | ~1.2K tokens | ~2K (63%) |
| Role agent spawn (avg) | ~3.6K tokens | ~3.4K tokens | ~0.2K (6%) |
| Command invocation (avg) | ~3.2K tokens | ~2.1K tokens | ~1.1K (34%) |

The highest-impact improvement is always-loaded content (54% reduction), affecting every session. Function agent spawn costs drop dramatically (63%) because Execution Steps move to on-demand skills.

## How to Use This Package

This package contains 7 analysis files plus this README. Each file is self-contained for its topic.

**MANDATORY first read:** `01-design-principles.md` -- every executor must read this before any other file. It defines the 6 design principles and SPINE mandate that govern ALL restructuring work.

### Package Navigation

| File | What It Is | When to Use | What to Do |
|------|-----------|-------------|------------|
| [01-design-principles.md](01-design-principles.md) | 6 design principles + SPINE mandate + anti-patterns + artifact interaction flow | **Before starting ANY task** in this epic | Read in full; apply all principles to every artifact you touch |
| [02-skill-taxonomy.md](02-skill-taxonomy.md) | 15-skill tree with Used-by scope, skill-to-agent access matrix, effective knowledge per agent | When you need to understand which skills exist, who uses them, and how agents access content | Check the access matrix to verify your changes preserve agent knowledge |
| [03-claude-md.md](03-claude-md.md) | Section-by-section CLAUDE.md audit, STAY/MOVE classification, target structure (~400 lines) | When working on INF-TSK-019-008 (CLAUDE.md restructure) or INF-TSK-019-013 (capabilities) | Follow the extraction map: each row tells you what stays, what moves, and where |
| [04-agent-definitions.md](04-agent-definitions.md) | All 8 agent audits, function vs role strategy, per-agent slimming tables, target template | When working on INF-TSK-019-009 (agents batch 1) or INF-TSK-019-010 (agents batch 2) | Use per-agent breakdown tables to identify what to remove, what to keep, and what to reference |
| [05-commands.md](05-commands.md) | 14-command audit, common duplicated patterns, per-command targets, SPINE guidance | When working on INF-TSK-019-011 (commands batch 1) or INF-TSK-019-012 (commands batch 2) | Follow the target structure template; remove duplicated sections per the common patterns table |
| [06-skills-detail.md](06-skills-detail.md) | Per-skill specs with source extraction tables, target line counts, LOSSLESS verification | When working on any skill task (INF-TSK-019-001 through 007) | Read the extraction map for YOUR skill to find exact source content and target operations |
| [07-implementation-order.md](07-implementation-order.md) | 5-phase bottom-up plan (A-E), dependency graph, task-to-phase mapping, timing, open questions, assumptions | When planning execution order or checking dependencies between tasks | Follow the phase ordering; verify assumptions before starting work |

### V4 Specification Vision

According to `codeflow-specification-v4/05-claude-components/main-agent/01-claude-md-spec.md`:

1. **CLAUDE.md = Awareness only.** The team lead needs WHAT/WHEN/WHERE, not HOW.
2. **Skills = Procedures.** Detailed step-by-step instructions live in skills loaded on-demand.
3. **Agent defs = WHAT/WHEN + skill references for HOW.** Agents read def + skills at spawn.
4. **Restoration, not invention.** Archived skills already follow the correct structure.

### Related Resources

- Epic: `project-management/epics/INF/INF-EPC-019/INF-EPC-019.md`
- Planning task: `project-management/epics/PLN/PLN-EPC-001/tasks/PLN-TSK-001-004.md`
- V4 Spec (main agent): `codeflow-specification-v4/05-claude-components/main-agent/01-claude-md-spec.md`
- V4 Spec (skills framework): `codeflow-specification-v4/11-reference/frameworks/cf-skills-framework.md`
- Archived skills: `.codeflow/docs/archived/skills/`
