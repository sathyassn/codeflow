---
id: "epic-01kk4brnw2t3am8i5zo1ut9g7f"
format_id: "INF-EPC-027"
title: "CodeFlow App (Epic E)"
summary: "Tauri v2 + SvelteKit desktop application — streaming terminal, project management views, config viewer, semantic search, analytics dashboard, playground mode"
status: planning
area_type: "INF"
work_type: "FEAT"
domain: "GENL"
is_ongoing: false
file_scope: []
priority: high
pr_number: null
external_id: null
external_url: null
created_at: "2026-03-07T11:00:00Z"
updated_at: "2026-03-07T11:00:00Z"
---

# INF-EPC-027: CodeFlow App (Epic E)

> **MANDATORY VALIDATION:** Files created from this template MUST be validated before committing:
> `codeflow validate epic <file-path>`
> Fix all errors (exit code 1) before requesting a commit. Warnings are advisory.

## Summary

This epic implements the CodeFlow App (Epic E) — a native desktop application built with Tauri v2 (Rust backend) and SvelteKit (TypeScript/Svelte frontend). The app provides the primary graphical interface for the CodeFlow AI-native development framework, featuring real-time streaming terminal views of model CLI sessions, project and session management, WorkGraph visualization via PathFlow phase views, configuration management, semantic search across project memory, an analytics dashboard, and a playground mode for experimentation.

The architecture uses Tauri v2 IPC for communication between the Rust backend and SvelteKit frontend. The backend connects to the Global Intelligence Layer daemon (Epic C) via Unix domain socket IPC for data access, and uses Epic D's PTY capture infrastructure for real-time model CLI output streaming. The frontend uses Svelte 5 runes for reactive state management, xterm.js with WebGL addon for terminal rendering, and a CSS variable-based design system.

Epic E is organized into 6 phases with 24 tasks:

- **Phase E1 (App Foundation):** Project scaffold, core types/IPC, daemon connection, app shell/navigation/theme, sidebar/session navigator (Tasks 001-005)
- **Phase E2 (Streaming Terminal):** PTY bridge backend, xterm.js widget, multi-tab streams, sub-stream embedding, stream controls (Tasks 006-010)
- **Phase E3 (Project Management Views):** Kanban board, epic timeline/roadmap, task detail view with criteria matrix, task dependency graph visualization (Tasks 011-014)
- **Phase E4 (Configuration and Management):** Config backend and file watcher, structured config viewer, change request integration, file/diff viewer (Tasks 015-018)
- **Phase E5 (Search, Analytics, and Intelligence):** Semantic search view, cross-project task aggregation, knowledge graph explorer, team analytics dashboard (Tasks 019-022)
- **Phase E6 (Playground and Testing):** Playground mode scratch sessions, end-to-end integration test suite (Tasks 023-024)

## Scope

### In Scope

- Tauri v2 + SvelteKit project scaffold with workspace integration (Phase E1)
- Core Rust/TypeScript types, IPC protocol definitions, AppEvent enum (Phase E1)
- Daemon IPC client reusing Epic C's IpcClient with connection management (Phase E1)
- App shell layout: sidebar (240px), tab bar, content area, status bar (22px) (Phase E1)
- Multi-project sidebar with session navigator, expandable accordions, status badges (Phase E1)
- PTY bridge backend reusing Epic D's PtyCapture for real-time model CLI streaming (Phase E2)
- xterm.js terminal widget with WebGL addon, Fit addon, 10,000-line scrollback (Phase E2)
- Multi-tab stream management with display-swap tab switching (Phase E2)
- Sub-stream inline embedding for delegated model output (Phase E2)
- Stream controls: pause/resume, search (Search addon), copy, clear, stop, restart (Phase E2)
- Kanban board with task cards, filter bar, CSS Grid layout, daemon SurrealDB data (Phase E3)
- Epic timeline/roadmap view with horizontal bars, phase segments, dependency arcs (Phase E3)
- Task detail view with acceptance criteria stage evaluation matrix (Phase E3)
- Task dependency graph visualization with D3.js (Phase E3)
- Config backend with file watcher for JSON/TOML config files (Phase E4)
- Structured config viewer components with read-only mode (Phase E4)
- Change request integration with modal dialog and clipboard support (Phase E4)
- File viewer with syntax highlighting and diff viewer (Phase E4)
- Semantic search view with ONNX embedding and HNSW vector search (Phase E5)
- Cross-project task aggregation on Kanban board (Phase E5)
- Knowledge graph explorer with force-directed graph rendering (Phase E5)
- Team analytics dashboard: session frequency, stage pass rates, model usage, cost tracking (Phase E5)
- Playground mode: standalone model CLI sessions without PathFlow context (Phase E6)
- End-to-end integration test suite covering Rust backend, SvelteKit frontend, and E2E tests (Phase E6)

### Out of Scope

- Packaging and distribution (Phase E6 — future, not yet scoped)
- Mobile application or web-hosted version
- Epic A: CRDT Coordination (INF-EPC-023 — parallel, independent)
- Epic B: Schema Standardization (INF-EPC-024 — parallel, independent)
- Modifying Epic C daemon or Epic D orchestration internals (consumed, not modified)
- Cloud sync, user authentication, or multi-user collaboration (future epic -- not in current V4 roadmap)
- Plugin/extension system for third-party integrations
- Auto-update mechanism (future enhancement)

## Acceptance Criteria

- [ ] Phase E1: Tauri v2 + SvelteKit app scaffolded, builds to bundled binary <= 60MB, connects to daemon, renders app shell with 6-tab navigation and project sidebar
- [ ] Phase E2: Streaming terminal renders real-time model CLI output via PTY bridge, supports multi-tab streams, sub-stream embedding, and stream controls (pause/resume/search/copy)
- [ ] Phase E3: Kanban board with four columns and filtered task cards, epic timeline/roadmap with dependency arcs, task detail view with criteria matrix, and task dependency graph visualization
- [ ] Phase E4: Config backend watches files, config viewer renders structured views, change request integration works, file/diff viewer with syntax highlighting
- [ ] Phase E5: Semantic search with vector search, cross-project task aggregation, knowledge graph explorer, team analytics dashboard
- [ ] Phase E6: Playground mode provides standalone model CLI sessions, end-to-end integration test suite covers all features with quality gates
- [ ] All existing tests pass; new tests added for Rust backend (cargo test) and frontend (vitest, Playwright)

### PII Handling Review

- [x] Does this epic involve code that handles PII? (N — desktop app displays project data and model output; no user authentication or personal data storage)

## Tasks

| ID | Phase | Title | Work Type | Estimate | Dependencies |
|----|-------|-------|-----------|----------|--------------|
| INF-TSK-027-001 | E1 | Tauri v2 + SvelteKit Project Scaffold | FEAT | L | Epic 0 complete |
| INF-TSK-027-002 | E1 | Core Types, IPC Protocol, and Event Definitions | FEAT | M | 001 |
| INF-TSK-027-003 | E1 | Daemon IPC Client and Connection Manager | FEAT | M | 002, Epic C Phase C1 |
| INF-TSK-027-004 | E1 | App Shell, Navigation, and Theme System | FEAT | L | 002, 003 |
| INF-TSK-027-005 | E1 | Multi-Project Sidebar and Session Navigator | FEAT | M | 003, 004 |
| INF-TSK-027-006 | E2 | PTY Bridge Backend (Tauri Rust) | FEAT | L | 002, Epic D Task 006 |
| INF-TSK-027-007 | E2 | xterm.js Terminal Widget | FEAT | L | 004, 006 |
| INF-TSK-027-008 | E2 | Multi-Tab Stream Management | FEAT | M | 006, 007 |
| INF-TSK-027-009 | E2 | Sub-Stream Inline Embedding | FEAT | L | 007, 008 |
| INF-TSK-027-010 | E2 | Stream Controls (Pause/Resume, Search, Copy) | FEAT | M | 007, 008 |
| INF-TSK-027-011 | E3 | Kanban Board | FEAT | L | 003, 004, 005 |
| INF-TSK-027-012 | E3 | Epic Timeline and Roadmap | FEAT | M | 011 |
| INF-TSK-027-013 | E3 | Task Detail View with Criteria Matrix | FEAT | M | 011 |
| INF-TSK-027-014 | E3 | Task Dependency Graph Visualization | FEAT | M | 011, 012 |
| INF-TSK-027-015 | E4 | Config Backend and File Watcher | FEAT | M | 002, 003 |
| INF-TSK-027-016 | E4 | Structured Config Viewer Components | FEAT | M | 004, 015 |
| INF-TSK-027-017 | E4 | Change Request Integration | FEAT | S | 016 |
| INF-TSK-027-018 | E4 | File Viewer and Diff Viewer | FEAT | M | 004 |
| INF-TSK-027-019 | E5 | Semantic Search View | FEAT | L | 003, 004, INF-TSK-025-015, INF-TSK-025-019 |
| INF-TSK-027-020 | E5 | Cross-Project Task Aggregation | FEAT | M | 011, 019 |
| INF-TSK-027-021 | E5 | Knowledge Graph Explorer | FEAT | L | 014, INF-TSK-025-013, INF-TSK-025-019 |
| INF-TSK-027-022 | E5 | Team Analytics Dashboard | FEAT | M | 003, 004, INF-TSK-025-008, INF-TSK-025-010, INF-TSK-025-011 |
| INF-TSK-027-023 | E6 | Playground Mode | FEAT | M | 006, 007, 008 |
| INF-TSK-027-024 | E6 | End-to-End Integration Test Suite | TEST | XL | all E1-E5 tasks |

## Dependencies

### Blocked By

- INF-EPC-022 (Epic 0: Rust CLI Idiomatic Redesign) — provides Rust crate structure (`codeflow-core`, `codeflow-cli`), Cargo workspace configuration, core traits and types
- INF-EPC-025 (Epic C: Global Intelligence Layer) — provides SurrealDB daemon, Unix domain socket IPC, project registry, global schema, IpcClient
- INF-EPC-026 (Epic D: Model Orchestration Layer) — provides PTY capture infrastructure (`portable-pty`, `vte`), PtyCapture/PtyHandle types, process management

### Blocks

- None (terminal epic in the current roadmap)

## Technical Notes

**Architecture overview:**

The CodeFlow App uses a two-process architecture:

1. **Rust backend** (Tauri v2): Manages daemon connections, PTY sessions, file system access, and IPC command handling. Connects to Epic C's daemon via Unix domain socket for data queries and to Epic D's PTY infrastructure for model CLI streaming.

2. **SvelteKit frontend** (TypeScript/Svelte 5): Renders the UI, manages reactive state via Svelte 5 runes, and communicates with the Rust backend via Tauri IPC (`invoke()` for commands, `listen()` for events).

**Crate structure:**

```text
codeflow-app/
  src-tauri/           # Rust backend (Tauri v2)
    src/
      main.rs          # Tauri app setup, managed state, command registration
      commands/        # #[tauri::command] handlers
      ipc/             # Daemon connection (reuses codeflow-core IpcClient)
      pty/             # PTY bridge (reuses codeflow-core PtyCapture)
      state/           # Tauri managed state structs
    Cargo.toml         # Depends on codeflow-core
  src/                 # SvelteKit frontend
    routes/            # SvelteKit pages
    lib/
      components/      # Svelte 5 components (layout, terminal, views)
      stores/          # Svelte stores (projects, streams, theme, workgraph)
      types/           # TypeScript type definitions mirroring Rust types
      api/             # Tauri invoke() wrappers
    app.css            # Design system CSS variables
  package.json
  svelte.config.js
  vite.config.ts
  tauri.conf.json
```

**Design system:**

CSS variables define the visual theme, with dark mode as default:

- Background: `--bg-darkest: #1e1e2e` (alias: `--bg-primary`), `--bg-dark: #16213e` (alias: `--bg-secondary`), `--bg-panel: #0f3460` (alias: `--bg-tertiary`), `--bg-card` for card/component backgrounds
- Text: `--text-primary: #e0e0ee`, `--text-secondary: #a1a1aa`
- Accent: `--accent-blue: #5b8af5` (alias: `--accent-primary`), `--accent-green: #3ddc84` (alias: `--accent-success`)
- Status dots: green (active), yellow (busy), gray (inactive), red (error)
- Work type badge colors: FEAT=blue, FIX=red, RFCT=purple, DOCS=teal, TEST=amber, PLAN=indigo

**Constraints:**

- Bundled binary size <= 60MB
- SvelteKit static adapter (no SSR, `prerender = false`, `ssr = false`)
- TypeScript strict mode, no `any` types
- Svelte 5 runes for all reactive state (no legacy `$:` syntax)
- Component-scoped CSS (no global styles except app.css variables)
- 60fps minimum for tab transitions and terminal rendering
- xterm.js scrollback: 10,000 lines
- Maximum 10 concurrent stream tabs, 5 concurrent PTY sessions

**Dependency graph:**

```text
Phase E1: Foundation
  001 (Scaffold)
   |
   +---> 002 (Types/IPC) --+---> 003 (Daemon Client)
                            |         |
                            |    +----+----+
                            |    |         |
                            +---> 004 (Shell) ---> 005 (Sidebar)
                                      |
Phase E2: Streaming Terminal          |
  006 (PTY Bridge) <--- Epic D       |
   |                                  |
   +---> 007 (xterm.js) <--- 004 ----+
   |         |
   +---> 008 (Multi-Tab)
   |         |
   +---------+---> 009 (Sub-Streams)
             +---> 010 (Controls)

Phase E3: Project Management
  011 (Kanban Board) <--- 003, 004, 005
   |
   +---> 012 (Epic Timeline) <--- 011
   |
   +---> 013 (Task Detail) <--- 011
   +---> 014 (Dependency Graph) <--- 011, 012

Phase E4: Configuration
  015 (Config Backend) <--- 002, 003
   |
   +---> 016 (Config Viewer) <--- 004
             |
             +---> 017 (Change Request)
  018 (File/Diff Viewer) <--- 004

Phase E5: Search & Analytics
  019 (Search) <--- 003, 004, Epic C
   |
   +---> 020 (Cross-Project) <--- 011
  021 (Knowledge Graph) <--- 014, Epic C
  022 (Analytics) <--- 003, 004, Epic C

Phase E6: Playground & Testing
  023 (Playground) <--- 006, 007, 008
  024 (E2E Tests) <--- all E1-E5 tasks
```

## Related

- INF-EPC-022: Rust CLI Idiomatic Redesign (Epic 0 — prerequisite)
- INF-EPC-025: Global Intelligence Layer (Epic C — prerequisite for daemon IPC)
- INF-EPC-026: Model Orchestration Layer (Epic D — prerequisite for PTY capture)
- INF-EPC-023: Parallel Execution Core (Epic A — parallel, independent)
- INF-EPC-024: Data Layer Standardization (Epic B — parallel, independent)
- `.codeflow/docs/analysis/parallel-work/product-strategy.md` Section 11
- `.codeflow/docs/analysis/parallel-work/decisions.md` -- D22, D24
- `.codeflow/docs/analysis/parallel-work/mockups/codeflow-app-mockup.html` -- Visual reference for Phases E1-E2 (Streams, Tasks, Config, Search tabs)
