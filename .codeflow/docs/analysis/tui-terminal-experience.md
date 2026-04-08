# TUI Terminal Experience: Analysis and Design

## Overview

This document covers the design and architecture for CodeFlow's terminal user interface (TUI) layer, built on ratatui + crossterm. The TUI provides three primary use cases: autorun monitoring, interactive session monitoring, and an onboarding wizard. It also establishes the shared `tui/` module architecture that future features (including the graphical App layer from INF-EPC-027) will build upon.

## 1. Technology: ratatui + crossterm

### Capabilities

**ratatui** (v0.29) is a Rust library for building terminal user interfaces. It uses an immediate-mode rendering model where the application describes the entire UI state each frame, and ratatui computes the minimal diff to update the terminal. crossterm is bundled as `ratatui::crossterm` and provides the backend for terminal manipulation.

**Core widget library:**

| Widget | Purpose | CodeFlow Use |
|--------|---------|-------------|
| Block | Bordered container with title | Section panels (header, table, detail, keys) |
| Table | Scrollable rows with column headers | Task/session lists |
| Paragraph | Styled text block | Detail pane, status messages |
| Gauge | Progress bar | Batch progress indicator |
| List | Selectable item list | Wizard step list |
| Tabs | Tab-based navigation | Future: multi-view switching |
| Sparkline | Inline mini-chart | Future: duration trend |

**Rendering capabilities:**

- 256-color and true-color support (via crossterm ColorType detection)
- Bold, italic, underline, dim, reversed text styles
- Unicode rendering (box-drawing characters, symbols, CJK)
- Responsive layout via `Layout::horizontal()` / `Layout::vertical()` with Constraint-based sizing (percentage, min, max, ratio)
- Buffer-level diffing for flicker-free updates

**Input handling:**

- Keyboard events: key press, key release, modifiers (Ctrl, Alt, Shift)
- Mouse events: click, scroll, drag (crossterm MouseEventKind)
- Async event stream via `crossterm::event::EventStream` (tokio-compatible)
- Non-blocking poll with configurable timeout

**Platform support:**

- Linux, macOS, Windows (via crossterm abstraction)
- Handles terminal resize events (crossterm::event::Event::Resize)
- Raw mode for full terminal control, alternate screen buffer to preserve shell history
- Graceful degradation: detects terminal capabilities, falls back for limited terminals

### Design Decisions

**Why ratatui over alternatives:**

| Option | Pros | Cons | Decision |
|--------|------|------|----------|
| ratatui + crossterm | Mature ecosystem, excellent Rust integration, immediate-mode simplicity, bundled crossterm | Learning curve for layout system | **Selected** -- best Rust TUI library, active community, used by gitui and bottom |
| tui-rs (predecessor) | Similar API | Unmaintained since 2022, ratatui is the maintained fork | Rejected -- ratatui supersedes it |
| cursive | Higher-level widget abstraction | Heavier dependency, callback-based model less idiomatic | Rejected -- over-abstraction for our needs |
| Raw crossterm | Minimal dependency | Must build all widgets from scratch | Rejected -- too much boilerplate |
| Custom ANSI escape codes | Zero dependency | Fragile, no abstraction, no diffing | Rejected -- not maintainable |

**Dependency placement:**

- `ratatui = "0.29"` in `codeflow-cli/cli/Cargo.toml` (CLI-layer only, where commands live)
- `ratatui = { version = "0.29", optional = true }` in `codeflow-cli/core/Cargo.toml` behind a `tui` feature flag (shared widgets and data layer)
- crossterm is bundled with ratatui -- no separate dependency needed

## 2. Use Cases

### 2.1 Autorun Monitoring (`codeflow autorun status --watch`)

**Current state:** The `run_status_watch()` function (autorun.rs:1364) uses a polling loop that clears the screen and reprints plain text every 2 seconds. No interactivity, no selection, no detail view. Status information is limited to task_id, status, and exit_code.

**Proposed TUI:**

```
+---- Autorun: batch-024 ---- Status: * Running ---- Workers: 2/3 ---- Elapsed: 1:23 ----+
| Target: autorun/int-batch-024 -> main                                                    |
+---- Task Table (scrollable, selectable) -------------------------------------------------+
| TASK              STATUS    PHASE   BRANCH              PR   DUR                         |
|>INF-TSK-923-002   * run     PF4     fix/worker-fixes    --   23m                         |
| INF-TSK-923-003   v done    PF7     fix/queue-mgmt      #42  52m                         |
| INF-TSK-923-004   x fail    PF4     feat/tui-dash       --   18m                         |
| INF-TSK-923-005   o pend    --      --                  --   --                          |
+---- Detail Pane (shows selected task) ---------------------------------------------------+
| Branch: fix/worker-fixes   PR: --   Exit: --                                             |
| Stages: WS-DEV *  ->  WS-SEC o  ->  WS-REV o  ->  WS-QA o                              |
| Worktree: /Volumes/.../worktree-ses-01knn5v2...                                          |
+---- Keybindings ------------+
| [Enter] Attach  [a] Abort  [r] Retry  [Up/Down] Navigate  [q] Quit                     |
+------------------------------------------------------------------------------------------+
```

Unicode symbols: v (checkmark U+2713), x (cross U+2717), * (bullet U+25CF), o (circle U+25CB), -> (arrow U+2192), > (triangle U+25B8)

**Data sources:**

- SurrealDB: `autorun_session`, `autorun_worker`, `autorun_task_run` tables
- Filesystem: `{worktree}/.state/sentinels/pathflow/{session_id}/` for phase/stage detection
- Filesystem: `.state/worktrees/worktrees.yaml` for worktree paths
- Process: PID liveness via `kill(pid, 0)` for worker health

**Key interactions:**

| Key | Action | Implementation |
|-----|--------|---------------|
| Up/Down | Navigate task table | Table state selection index |
| Enter | Attach to worker tmux session | `tmux attach-session -t {tmux_name}` |
| `a` | Abort selected task | Calls `run_cancel()` (both Running and Pending) |
| `r` | Retry failed/timed-out task | Re-dispatches task (new worker) |
| `q` | Quit TUI | Exit alternate screen, restore terminal |
| Mouse click | Select row | Map click coordinates to table row |
| Mouse scroll | Scroll table | Adjust viewport offset |

### 2.2 Interactive Session Monitoring (`codeflow interactive status --watch`)

**Current state:** The `run_status()` function (interactive.rs:194) outputs a plain text table of sessions with session_id, status, pid, and created_at. No live updates, no detail view, no cleanup actions.

**Proposed TUI:**

```
+---- CodeFlow Interactive Sessions ---- Active: 2 ---- Stale: 0 ---- Complete: 5 --------+
+---- Session Table -----------------------------------------------------------------------+
| SESSION ID          STATUS  PHASE   BRANCH           TYPE   DUR                          |
|>ses-01knn5v2...     * active PF4    feat/validation  FEAT   45m                          |
| ses-01kmf000...     v done   PF7    docs/readme      DOCS   22m                          |
+---- Detail Pane -------------------------------------------------------------------------+
| Session: ses-01knn5v2gr22eskdzxhtzw6sbp                                                  |
| Worktree: /Volumes/.../worktree-ses-01knn5v2...                                          |
| Team: codeflow-autorun-reliability   PID: 12345 (alive)                                  |
+---- Keybindings -------------------------------------------------------------------------+
| [Enter] Attach  [c] Cleanup  [Up/Down] Navigate  [q] Quit                               |
+------------------------------------------------------------------------------------------+
```

**Data sources:**

- SurrealDB: `interactive_session` table (currently missing branch, work_type, team_name -- Task 2 populates these)
- Filesystem: `pathflow-session-status.json` for phase progress
- Filesystem: `worktrees.yaml` for worktree paths and branch names
- Process: PID liveness for session health

### 2.3 Onboarding Wizard (`codeflow init`)

**Current state:** The `init.rs` command is a stub. The original Go implementation (wizard.go, 557 lines) had a 7-step wizard with interactive prompts. The spec is preserved in `project-management/epics/INF/INF-EPC-015/tasks/INF-TSK-015-009.md`.

**Proposed TUI wizard:**

```
+---- CodeFlow Setup ---- Step 2 of 7 ----------------------------------------------------+
|                                                                                          |
| Steps:            | Prerequisites Check                                                  |
|  v 1. Location    |                                                                      |
|  @ 2. Prereqs     | Checking system requirements...                                      |
|  o 3. Auth        |                                                                      |
|  o 4. Git         |   v  git 2.45.0                                                      |
|  o 5. Config      |   v  Claude Code 1.0.29                                              |
|  o 6. Setup       |   x  gh (not installed)                                               |
|  o 7. Verify      |                                                                      |
|                   | Install gh: brew install gh                                           |
|                   |                                                                      |
+---- [Enter] Continue  [Esc] Back  [q] Quit ----------------------------------------------+
```

Unicode: v (U+2713 completed), @ (U+25C9 current), o (U+25CB pending), x (U+2717 failed)

**7 steps:**

1. **Project location** -- detect new/existing/join project
2. **Prerequisites check** -- verify git, Claude Code installed (checklist display)
3. **Claude Code authentication** -- subscription vs API key selection, run `claude login`
4. **Git provider setup** -- GitHub/GitLab/Bitbucket selection, install gh, run `gh auth login`
5. **Project configuration** -- text input for name, description
6. **CodeFlow setup** -- create directories, init DB, optional PathFlow config
7. **Verification** -- run health checks, display results as pass/fail checklist

**Non-TUI fallback:** When stdout is not a TTY (piped or redirected), fall back to sequential text prompts. Detected via `std::io::stdout().is_terminal()`.

**CLI flags:** `--existing`, `--join`, `--skip-auth`, `--yes` (non-interactive, accept defaults)

## 3. Future Concept: Embedded Terminal via PTY + vte

### Architecture

A future capability (Epic D in the roadmap, not part of this epic) would embed a terminal emulator within the TUI or App. This enables watching Claude Code work in real-time without detaching to tmux.

**Key components:**

| Component | Crate | Purpose |
|-----------|-------|---------|
| PTY allocation | `portable-pty` or `nix::pty` | Create pseudo-terminal for subprocess |
| VT parsing | `vte` crate | Parse ANSI escape sequences from PTY output |
| Screen buffer | `alacritty_terminal` or custom | Maintain virtual terminal state (cursor, scrollback) |
| Rendering | ratatui Paragraph or custom widget | Render screen buffer into a ratatui widget |

**PtyHandle concept:**

```rust
pub struct PtyHandle {
    child: Box<dyn portable_pty::Child>,
    reader: Box<dyn std::io::Read + Send>,
    writer: Box<dyn std::io::Write + Send>,
    parser: vte::Parser,
    screen: ScreenBuffer,
}

impl PtyHandle {
    /// Spawn a subprocess in a PTY.
    pub fn spawn(cmd: &str, args: &[&str], size: (u16, u16)) -> Result<Self>;

    /// Read available output, parse VT sequences, update screen buffer.
    pub fn poll(&mut self) -> Result<bool>;

    /// Send input to the subprocess.
    pub fn write(&mut self, data: &[u8]) -> Result<()>;

    /// Render current screen state as a ratatui widget.
    pub fn render(&self) -> impl ratatui::widgets::Widget;
}
```

**Serving both App and TUI:** The `PtyHandle` abstraction is backend-agnostic. For the TUI, `render()` produces a ratatui widget. For the graphical App (INF-EPC-027), the same `PtyHandle` feeds a GPU-rendered terminal surface. The screen buffer is the shared interface -- both consumers read from it, using different renderers.

## 4. Relationship to INF-EPC-027 (App)

| Aspect | TUI (this epic) | App (INF-EPC-027) |
|--------|-----------------|-------------------|
| Runtime | Terminal (ratatui + crossterm) | Native window (tauri or similar) |
| Target | Developers who live in the terminal | Broader audience, visual preference |
| Permanence | Permanent -- terminal is always available | Additive layer on top of TUI data |
| Shared code | `tui/data.rs` (data fetching), `tui/theme.rs` (color constants) | Consumes same data layer, may adapt theme |
| PTY embed | Future: ratatui widget via PtyHandle | Future: GPU-rendered terminal surface via PtyHandle |

The TUI is not a stopgap for the App -- it is the permanent terminal-native experience. The App adds a graphical layer for users who prefer it. Both share the data fetching layer (`tui/data.rs`) and can share theme constants.

## 5. Shared tui/ Module Architecture

### Module Layout

```
codeflow-cli/core/src/tui/
  mod.rs          -- Public API exports, feature-gated behind [features] tui
  data.rs         -- Unified data fetching (DB + filesystem + PID liveness)
  theme.rs        -- Color palette, border style, text style presets, Unicode symbols
  widgets/
    mod.rs        -- Widget module exports
    status_badge.rs   -- Status indicator with color (done=green, fail=red, run=yellow, pend=dim)
    phase_badge.rs    -- PF phase as colored label
    duration_cell.rs  -- Duration formatting with color thresholds
    detail_pane.rs    -- Selected item detail panel
```

### data.rs -- Unified Data Fetching

```rust
/// View model for a single autorun task in the TUI.
pub struct TaskView {
    pub task_id: String,
    pub status: TaskStatus,        // Pending, Running, Completed, Failed, Timeout, Skipped
    pub phase: Option<String>,     // "PF1".."PF7" from sentinels
    pub branch: Option<String>,
    pub pr_number: Option<u64>,
    pub duration_secs: Option<i64>,
    pub worktree_path: Option<String>,
    pub tmux_session: Option<String>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub stages: Vec<StageInfo>,    // Derived from sentinel files
}

/// View model for an interactive session.
pub struct SessionView {
    pub session_id: String,
    pub status: SessionStatus,     // Active, Complete, Stale
    pub phase: Option<String>,
    pub branch: Option<String>,
    pub work_type: Option<String>,
    pub team_name: Option<String>,
    pub duration_secs: Option<i64>,
    pub worktree_path: Option<String>,
    pub pid: Option<u32>,
    pub pid_alive: bool,
}

/// Batch-level summary.
pub struct BatchView {
    pub name: String,
    pub status: BatchStatus,
    pub total_tasks: usize,
    pub completed: usize,
    pub failed: usize,
    pub running: usize,
    pub pending: usize,
    pub elapsed_secs: i64,
    pub integration_branch: String,
    pub final_pr_target: String,
    pub tasks: Vec<TaskView>,
}
```

**Data sources per field:**

| Field | Source | Fallback |
|-------|--------|----------|
| task status | SurrealDB `autorun_task_run.status` | Filesystem worker log |
| phase | Sentinel directory scan: `{worktree}/.state/sentinels/pathflow/{sid}/` | None (show "--") |
| branch | SurrealDB `autorun_worker.branch_name` | worktrees.yaml cross-reference |
| pr_number | SurrealDB `autorun_task_run.pr_number` | None |
| duration | SurrealDB timestamps (created_at, completed_at) | Worker log timestamps |
| stages | Sentinel files: pathflow-ws-dev, pathflow-ws-rev, etc. | None |
| pid_alive | `kill(pid, 0)` via nix crate | Assume dead if check fails |

### theme.rs -- Design System

**Color palette:**

| Constant | Color | Hex | Usage |
|----------|-------|-----|-------|
| GREEN_SUCCESS | Green | #00FF00 | Completed, passed, checkmark |
| RED_FAILURE | Red | #FF0000 | Failed, error, cross |
| YELLOW_RUNNING | Yellow | #FFFF00 | In progress, active |
| DIM_PENDING | DarkGray | #808080 | Pending, not started |
| BLUE_ACCENT | Blue | #0080FF | Phase labels PF5-PF6, headers |
| WHITE_TEXT | White | #FFFFFF | Primary text |

**Unicode symbol constants:**

| Constant | Symbol | Codepoint | Usage |
|----------|--------|-----------|-------|
| CHECKMARK | v | U+2713 | Success, completed |
| CROSS | x | U+2717 | Failure, error |
| BULLET | * | U+25CF | Running, active |
| CIRCLE | o | U+25CB | Pending, not started |
| ARROW | -> | U+2192 | Flow direction, stage transitions |
| TRIANGLE | > | U+25B8 | Selected row indicator |
| CAUTION | ! | U+26A0 | Warning (only where semantically correct) |

**Border style:** `Rounded` (ratatui BorderType::Rounded) -- consistent across all panels.

**Text style presets:**

| Preset | Style | Usage |
|--------|-------|-------|
| `header()` | Bold, White | Panel titles |
| `selected()` | Bold, reversed | Selected table row |
| `dim()` | DarkGray | Secondary text, pending items |
| `error()` | Bold, Red | Error messages |
| `success()` | Green | Success messages |

### widgets/ -- Reusable Components

**status_badge.rs:**
Renders a task/session status as a colored symbol + text:
- `completed` -> green "v done"
- `failed` -> red "x fail"
- `running` -> yellow "* run"
- `pending` -> dim "o pend"
- `timeout` -> red "x time"
- `skipped` -> dim "- skip"

**phase_badge.rs:**
Renders a PF phase with semantic coloring:
- PF1-PF3: dim (setup phases)
- PF4: yellow (active work)
- PF5-PF6: blue (verification/completion)
- PF7: green (done)
- None: dim "--"

**duration_cell.rs:**
Formats duration with color thresholds:
- < 30 minutes: default color, "MM:SS" format
- 30-60 minutes: yellow, "MM:SS" format
- > 60 minutes: red, "HH:MM:SS" format

**detail_pane.rs:**
Renders a bordered panel showing details of the selected item:
- Branch, PR number, exit code
- Stage pipeline with status indicators (e.g., "WS-DEV * -> WS-REV o -> WS-QA o")
- Worktree path, error message if any

## 6. TUI Design Guidelines

### Principles

1. **Elegant and minimal** -- no visual clutter. Every element serves a purpose.
2. **Unicode symbols, never emoji** -- use v x * o -> > for status. Exception: caution ! (U+26A0) where semantically required.
3. **Progressive disclosure** -- summary view shows essential info. Detail pane appears on selection.
4. **Color is semantic, not decorative** -- green=success, red=failure, yellow=running, dim=pending. No gratuitous color.
5. **Responsive layout** -- adapt to terminal width using Constraint-based sizing. Truncate long paths, abbreviate session IDs.

### Quality Benchmarks

The design quality should match these ratatui ecosystem projects:

- **gitui** -- clean git TUI with table navigation, detail panels, keyboard-driven workflow
- **bottom** -- system monitor with responsive layout, sparklines, tabbed views

Both demonstrate: Rounded borders, restrained color palette, keyboard-first interaction with optional mouse support, and efficient use of terminal space.

### Accessibility

- All information conveyed by color is also conveyed by symbol (v/x/*/o)
- Keyboard navigation is complete -- mouse is optional enhancement
- Detail pane provides full text for truncated table cells
- Non-TTY fallback for piped/redirected output

## 7. Assumptions

| # | Assumption | Verified? | Evidence |
|---|-----------|-----------|----------|
| 1 | `codeflow-cli/core/src/tui/` does not exist yet | YES | Glob returned no files |
| 2 | ratatui is not currently a dependency | YES | Grep for "ratatui" in Cargo.toml returned no matches |
| 3 | `run_status_watch()` exists at autorun.rs:1364 | YES | Grep confirmed function at that line |
| 4 | `run_status()` exists at interactive.rs:194 | YES | Grep confirmed function at that line |
| 5 | `TaskSpec` struct at batch.rs:47 has no `timeout_secs` field | YES | Read confirmed fields: id, depends_on, file_scope, scope_policy |
| 6 | `AutorunConfig` default `worker_timeout_secs` is 3600 | YES | Read config.rs:65 confirmed default |
| 7 | `spawn_worker` at orchestrator.rs:667 does not handle panics | YES | Read confirmed: tokio::spawn with no catch_unwind |
| 8 | `serialized_merge` at worker.rs:909 gates on `exit_code == 0` | YES | Read confirmed: `invoke_result.exit_code == 0 && invoke_result.pr_number > 0` |
| 9 | Worktree cleanup at worker.rs:1009 has single attempt | YES | Read confirmed: single `self.worktree.cleanup()` call |
| 10 | `init.rs` exists as a stub | YES | Glob confirmed file exists |
| 11 | `welcome.rs` exists as a stub | YES | Glob confirmed file exists |
| 12 | `onboarding-flows.md` spec file does not exist | YES | Glob returned no files for that path |
| 13 | `INF-TSK-015-009.md` spec exists | YES | Glob confirmed file exists |

## 8. Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| ratatui version compatibility | Low | Pin to 0.29, crossterm bundled |
| Terminal rendering differences across OS | Low | crossterm abstracts platform differences; test on macOS (primary) |
| DB query latency causing UI jank | Medium | Async data fetch on background task, UI renders from cached state |
| Large batch (50+ tasks) performance | Low | ratatui uses buffer diffing; only visible rows are rendered |
| Non-TTY environments (CI, pipes) | Medium | Detect `is_terminal()`, fall back to plain text |
| Feature flag complexity | Low | Single `tui` feature in core; CLI always enables it |
