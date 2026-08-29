# Utility presentation — implementation plan

**Date:** 2026-08-07  
**Branch:** `integration/EPC-005-presentation-system` (PR #433)  
**Depends on:** DESIGN_INTENT 2026-08-07, ADR-0053

## Task set (ordered)

### T1 — Present craft recovery
- [ ] Token + type scale + light/dark alignment to DESIGN_INTENT (Instrument default)
- [ ] Full-width document layout with breathing margins
- [ ] Concept stage: visual dual-lineage thesis (Codex off affects structure/confidence)
- [ ] Architecture stage: large labeled pipeline (not micro caption boxes)
- [ ] Technical panes: keep exact evidence, composed not dumped
- [ ] Remove / demote Mermaid-as-default-page patterns where they violate intent

### T2 — Comment mode (present)
- [x] Single Comment icon mode (not permanent +)
- [x] Aim: text select · element pin (content under document root) · region
- [x] Edit existing notes; Send all notes (session feedback path)
- [x] Esc: capture → Comment mode; C toggles present-only
- [x] Browser qualification coverage for Comment

### T3 — Portal visual system
- [ ] Shared tokens overlaid on Starlight shell
- [ ] Layered page grammar (concept / architecture / technical) in dogfood content
- [ ] Full-width main with margins; engineer-legible architecture stages
- [ ] Preserve source-link, Pagefind, adapter contracts

### T4 — Skills and quals
- [x] `cf-present`, `cf-docs-portal` compose into utility system; `cf-design` stays product-generic
- [x] Doctrine: CLI/skills reuse the design system for **new** subject matter; exploration board is reference only (both profiles)
- [ ] Eval/canary hooks for subject-led / no prose-in-boxes where applicable
- [ ] agent-os portable doctrine alignment (PR #43)

### T5 — CLI / scaffold
- [ ] Expose theme/token assets consumers need without manual file copy
- [ ] Portal starter reflects visual system; present assets content-hashed as today

### T6 — Close PR
- [ ] Local gates + dual-model review of exact delta
- [ ] Undraft #433 / #43 when experience matches intent

## Non-goals this plan
- React + shadcn migration
- Leaving Starlight
- Claude D1–D4 full territory builds (optional later)
- Consumer product design systems
