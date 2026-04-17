### 1. Overall Test Pass Status

| Target | Mode | Passed | Failed | Skipped | Duration |
|--------|------|--------|--------|---------|----------|
| rust-core | full | 2400 | 0 | 0 | <DURATION>s |
| shell-scripts | full | 240 | 0 | 0 | <DURATION>s |
| **Total** | | **2640** | **0** | **0** | **<DURATION>s** |

Runs: 1 consecutive clean.

### 2. Overall Coverage

| Target | Workspace-equivalent | Per-rule summary |
|--------|--------------------:|------------------|
| rust-core | 91.0% | 2640 files evaluated, 2631 pass, 0 fail |
| shell-scripts | N/A | no coverage configured |

#### Exempted Files (per-target)

| Target | File | Coverage | Configured Threshold | Reason |
|--------|------|---------:|---------------------:|--------|
| rust-core | core/src/autorun/worker.rs | 79% | 79% | Autorun worker contains process spawning (tmux sessions, Claude Code invocation), async timeout handling, and worktree lifecycle management that require real process infrastructure to exercise. 20 tests cover all testable paths via injectable trait mocks (TmuxProvider, ClaudeProvider, WorktreeProvider). The uncovered lines are in run_worker() async execution, process output parsing, cleanup-on-crash paths, and the timeout salvage push (branches 1152-1192 in run()) which requires a real git worktree with unpushed commits to exercise — the worktree path includes a dynamically-generated session ID making pre-test setup impossible without refactoring. These paths cannot be unit-tested without spawning real tmux+Claude processes or a real git infrastructure. |
| rust-core | cli/src/cmd/interactive.rs | 66% | 65% | 68+ tests cover all extractable pure functions and inner implementations. Remaining uncovered lines are structurally untestable: run_launch() calls exec(2) which replaces the process (cannot return to test harness); run_status/run_list/run_cleanup call detect_project_root() which requires real CWD with .git directory. Outer wrapper functions (sweep_stale_session_dirs, clean_session_worktree_map, mark_stale_worktree_entries) delegate to tested inner functions but are themselves uncovered because they construct paths from project_dir. Additionally, the --watch TUI dashboard added in INF-TSK-044-005 (run_status_tui event loop, render_session_header, render_session_table, render_session_detail at lines 757-1250) requires raw terminal mode and cannot be unit-tested, further reducing line coverage. |
| rust-core | cli/src/cmd/init.rs | 73% | 70% | init.rs TUI wizard contains ratatui event loop (run_tui_wizard_with_runner) and 8 step rendering functions (render_step_*) that require a live terminal in raw mode -- cannot be unit-tested without spawning a real terminal. Injectable CommandRunner trait ensures business logic (handle_*_input, check_prerequisites, run_setup, run_verification_checks, run_non_interactive) is fully tested. 54 unit tests cover all extractable pure functions. |
| rust-core | cli/src/cmd/autorun.rs | 68% | 67% | Large CLI command file (7500+ lines) with tmux process management, batch orchestration, and interactive terminal output. Three TUI event loops (run_status_watch, run_batches_tui, run_batch_list/detail navigation) are structurally untestable. All extractable pure functions covered by 23 unit tests. |
| rust-core | core/src/tui/mod.rs | N/A | 0% | Pure module declaration file (pub mod data/theme/widgets) with no executable code. llvm-cov reports 0% but there is nothing to test. |
| rust-core | core/src/tui/widgets/mod.rs | N/A | 0% | Pure module re-export file (pub mod + pub use) with no executable code. llvm-cov reports 0% but there is nothing to test. |
| rust-core | core/src/pathflow/mod.rs | N/A | 0% | Pure module declaration file (pub mod checkpoint/gates/sentinel/transitions and a file_lock re-export) with no executable code. llvm-cov reports N/A but there is nothing to test. |
| rust-core | core/src/testing/mod.rs | N/A | 0% | Pure module declaration file (pub mod config/coverage/doctor/error/pr_body/report/runner/setup/threshold) with no executable code. llvm-cov reports 0% but there is nothing to test. |
| rust-core | core/tests/integration_testing_setup.rs | N/A | 0% | Integration test file — compiled as a separate test binary, not instrumented by llvm-cov unit coverage. llvm-cov reports N/A because integration tests run in their own binary outside the library instrumentation scope. |
| rust-core | core/tests/integration_self_host_parity.rs | N/A | 0% | Integration test file — compiled as a separate test binary, not instrumented by llvm-cov unit coverage. llvm-cov reports N/A because integration tests run in their own binary outside the library instrumentation scope. |

### 3. Modified File Coverage

| Target | File | Coverage | Threshold | Status |
|--------|------|---------:|----------:|--------|
