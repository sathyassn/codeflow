# Strategic Review: V4 Parallel Work Epics (March 2026)

## Purpose

This document records strategic observations from the cross-epic holistic review of INF-EPC-022 through INF-EPC-027 (Epics 0, A, B, C, D, E). These observations are recorded for future planning reference. No scope changes are applied to any of the 6 epics or 132 tasks based on these findings. All epics proceed as planned.

## Scope Assessment

The V4 implementation comprises 132 tasks across 6 epics:

| Epic | ID | Tasks | Estimate Range | Phase Count |
|------|----|-------|---------------|-------------|
| Epic 0: Rust CLI Idiomatic Redesign | INF-EPC-022 | 27 | S-XL | 7 phases |
| Epic A: CRDT Coordination | INF-EPC-023 | 61 | XS-XL | ongoing |
| Epic B: Schema Standardization | INF-EPC-024 | 12 | S-L | 3 phases |
| Epic C: Global Intelligence Layer | INF-EPC-025 | 26 | S-XL | 5 phases |
| Epic D: Model Orchestration Layer | INF-EPC-026 | 12 | M-XL | 3 phases |
| Epic E: CodeFlow App | INF-EPC-027 | 24 | S-XL | 6 phases |

**Observation:** This is an ambitious scope. The critical path runs through Epic 0 (foundation) to Epic C (daemon infrastructure) to Epic E (app), with Epics A, B, and D as parallel streams. The longest sequential chain is approximately 80+ tasks across Epics 0, C, and E.

## Differentiation Assessment

**Epic C (Global Intelligence Layer) is the primary technical differentiator.** Its knowledge graph with ONNX-based local embeddings, entity extraction via COGNIFY pipeline, and hybrid search (GRAPH_COMPLETION) represent capabilities not commonly found in development framework tools. Epic C's daemon architecture (SurrealDB + Unix domain socket IPC) is also the foundational infrastructure consumed by both Epic D (model orchestration) and Epic E (desktop app).

**Epic E (CodeFlow App) is the primary user-facing differentiator.** It transforms the CLI-only workflow into a visual experience with streaming terminals, Kanban boards, dependency graphs, and analytics dashboards. However, Epic E's differentiation depends heavily on Epic C -- without the daemon's semantic search, knowledge graph, and analytics data, the app would be a well-designed shell without unique data capabilities.

## Go-to-Market Gap

**Observation:** The current V4 roadmap focuses entirely on technical implementation. There is no epic covering:

- **Distribution:** Packaging, auto-update, installation experience
- **Onboarding:** First-run experience, project setup wizard, documentation for new users
- **Pricing/licensing:** Open source vs commercial model decisions
- **Developer marketing:** Website, examples, comparison with alternatives

These concerns are deliberately out of scope for V4 implementation, which focuses on building the core product. They represent natural follow-on work after the technical foundation is complete.

## Dependency Risk Analysis

| Risk | Impact | Mitigation |
|------|--------|------------|
| Epic 0 delays block all downstream epics | High -- C, D, E all depend on Epic 0's crate structure | Epic 0 has the fewest unknowns (well-understood Rust patterns); prioritize completion |
| Epic C daemon complexity | Medium -- 26 tasks spanning ONNX, SurrealDB, CRDT, LLM extraction | Phase-gated delivery (C1-C5); Epic E can use mock daemon for early development |
| Epic A size (61 tasks) | Low -- Epic A is parallel/independent, does not block other epics | Can deprioritize or phase without affecting critical path |
| Cross-epic integration points | Medium -- 15+ cross-epic task dependencies identified | Dependencies now specified at task-ID level (not generic "Phase X"), enabling precise scheduling |

## Future Considerations

A "Developer Experience & Distribution" epic may be valuable after V4 implementation to address go-to-market concerns. This would cover:

- Tauri app packaging (DMG for macOS, AppImage/deb for Linux)
- Auto-update mechanism (Tauri's built-in updater)
- First-run project setup wizard
- Documentation site with tutorials and API reference
- CLI installation via Homebrew/cargo

This epic is not planned or scoped -- it is noted here as a strategic observation for future roadmap planning.

## Status

These observations are recorded for future planning reference. No scope changes are applied to Epics 0-E based on these findings. All 132 tasks and 6 epics proceed as planned.

## Related

- `.codeflow/docs/analysis/parallel-work/product-strategy.md` -- V4 product strategy and vision
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- Architecture decision log
- `project-management/epics/INF/` -- All 6 V4 epic definitions
