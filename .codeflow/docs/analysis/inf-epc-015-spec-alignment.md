# INF-EPC-015 Spec Alignment Analysis

**Task:** INF-TSK-015-018 -- Go CLI spec alignment and backfill fixes
**Date:** 2026-02-25
**Branch:** fix/cli-spec-alignment
**V4 Spec Source:** `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/08-cli/`
**Epic:** `project-management/epics/INF/INF-EPC-015/INF-EPC-015.md`
**Design Analysis:** `.codeflow/docs/analysis/inf-epc-015-go-cli-design.md`
**PathFlow Config:** `.codeflow/config/pathflow/pathflow-config.json`

---

## A. Epic vs Spec Conflicts (7 Resolved)

These items differ between the V4 specification and the INF-EPC-015 epic. Each has been resolved with a decision and tier classification.

### A1. `autorun run` -> `autorun start`

- **Spec:** `codeflow autorun start batch.yaml` (async design, returns immediately)
- **Epic:** `codeflow autorun run batch.yaml` (synchronous naming)
- **Resolution:** Adopt spec naming `autorun start`. The async design requires `start` semantics -- the orchestrator launches workers and returns. `run` implies blocking execution.
- **Tier:** 1 (naming convention, reversible)

### A2. `autorun list` -> `autorun sessions`

- **Spec:** `codeflow autorun sessions` (noun subcommand pattern)
- **Epic:** `codeflow autorun list` (verb subcommand)
- **Resolution:** Adopt spec naming `autorun sessions`. Consistent with cobra noun-subcommand pattern used elsewhere (`codeflow db`, `codeflow config`).
- **Tier:** 1 (naming convention, reversible)

### A3. Init Wizard: Full 7-Step Flow

- **Spec:** Full 7-step wizard: project location, prerequisites, Claude Code auth, git provider, project config, CodeFlow setup (with PathFlow 6b), verification
- **Epic:** 3 flows (new/existing/join) without explicit step breakdown. Claude Code and git provider listed as optional.
- **Resolution:** Adopt full 7-step wizard from V4 spec. Claude Code and git provider are REQUIRED (not optional) -- the entire CLI wraps Claude Code and needs git for branching.
- **Tier:** 2 (design preference, documented in task)

### A4. Doctor Checks: Keep Epic's 16

- **Spec:** 15 health checks
- **Epic:** 16 health checks (13 V3 + 3 V4)
- **Resolution:** Keep epic's 16 checks. The epic has better granularity, splitting some spec checks into separate concerns. No spec checks are missing from the epic.
- **Tier:** 1 (additive, reversible)

### A5. `--quiet`/`-q` Flag for Welcome Screen

- **Spec:** `--quiet` / `-q` flag skips welcome screen (required for automation/autorun workers)
- **Epic:** Not mentioned
- **Resolution:** Add to welcome screen task (INF-TSK-015-007). Required for autorun workers which should not display interactive output.
- **Tier:** 1 (additive feature flag)

### A6. `--json` Flag for Doctor

- **Spec:** `--json` flag for machine-readable doctor output (required for agent parsing)
- **Epic:** Not mentioned
- **Resolution:** Add to doctor task (INF-TSK-015-010). Required for automated health monitoring and agent-driven diagnostics.
- **Tier:** 1 (additive feature flag)

### A7. `--keep-config` for Uninstall

- **Spec:** `--keep-config` flag preserves `~/.config/codeflow/` during uninstall
- **Epic:** Not mentioned (uninstall is in INF-TSK-015-002)
- **Resolution:** Add to version/uninstall scope. Standard CLI practice -- users should be able to preserve config across reinstalls.
- **Tier:** 1 (additive feature flag)

---

## B. Spec Items With No Epic Coverage (8 Items)

These items exist in the V4 specification but have no corresponding epic task or acceptance criterion. Each is folded into an existing task.

| # | Spec Item | Folded Into | Notes |
|---|-----------|-------------|-------|
| B1 | `codeflow test` delegation to test runner | INF-TSK-015-002 | Simple exec delegation, add as criterion |
| B2 | Argument passthrough to Claude Code | INF-TSK-015-007 | Unknown flags/args passed through to `claude` |
| B3 | `doctor --reset` flag | INF-TSK-015-010 | Confirmation prompt, then reset state |
| B4 | Exit codes 0-5 | INF-TSK-015-002 (backfill via 018) | 0=success, 1=general, 2=config, 3=runtime, 4=external, 5=internal |
| B5 | `update --force` flag | INF-TSK-015-012 | Bypass version compatibility check |
| B6 | PathFlow onboarding Step 6b | INF-TSK-015-009 | Optional PathFlow mode configuration during init |
| B7 | Global `~/.config/codeflow/` config | INF-TSK-015-011 | `--global` flag for config commands |
| B8 | `--quiet` flag on root command | INF-TSK-015-007 | Suppresses welcome screen and non-essential output |

---

## C. Schema/Config Mismatches

### C1. `tasks.stage` CHECK Constraint

- **Current schema:** `CHECK(stage IN ('dev', 'work', 'review', 'qa', 'done'))`
- **Required:** `CHECK(stage IN ('dev', 'plan', 'docs', 'test', 'review', 'qa', 'done'))`
- **Issue:** Missing `plan`, `docs`, `test` stages. Has stale `work` stage.
- **Fix:** Migration 006 adds missing values, removes `work`.
- **Same issue in:** `active_work.current_stage`

### C2. CICD Branch Prefix

- **Schema seed data:** `cicd/` prefix
- **CLAUDE.md Section 6:** `CICD→ci/`
- **Resolution:** Schema is correct. Fix CLAUDE.md to say `CICD→cicd/`.
- **CLAUDE.md Section 7 feature branches list:** `ci/*` should be `cicd/*`.

### C3. SPKE Branch Prefix

- **Schema seed data:** `spike/` prefix
- **CLAUDE.md Section 6:** `SPKE→experiment/`
- **Resolution:** Schema is correct. Fix CLAUDE.md to say `SPKE→spike/`.
- **CLAUDE.md Section 7 feature branches list:** `experiment/*` should be `spike/*`.

### C4. PLAN Commit Type

- **Schema seed data:** `plan` commit type
- **pathflow-config.json:** `plan/` branch prefix
- **Resolution:** Consistent. No change needed.

### C5. Pipeline Stages for HTFX/TEST/CHOR

- **pathflow-config.json:** HTFX, TEST, CHOR all include WS-QA stage
- **Resolution:** pathflow-config.json is the authority for pipeline definitions. Keep as-is.

---

## D. Session ID Format

- **Decision:** Keep `ses-{ulid}` format
- **Rationale:** Matches all table conventions (sessions, autorun_sessions, etc.). ULID provides timestamp-sortable, globally unique identifiers.
- **No change needed.**

---

## E. Critical Gaps in Completed Tasks (Backfill in INF-TSK-015-018)

These items should have been in tasks 001-006 but were missed. They are backfilled as part of INF-TSK-015-018.

| # | Gap | Target | Description |
|---|-----|--------|-------------|
| E1 | Migration 006: stage CHECK constraints | `.codeflow/scripts/db/schema.sql`, `codeflow-cli/internal/db/` | Add `plan`, `docs`, `test` to tasks.stage and active_work.current_stage; remove `work` |
| E2 | Exit codes 0-5 | `codeflow-cli/cmd/exitcodes.go` | Define constants: 0=success, 1=general error, 2=config error, 3=runtime error, 4=external dependency, 5=internal error |
| E3 | User/session model | `codeflow-cli/internal/session/` | Real user from `git config user.email`, Claude UUID in sessions.metadata |
| E4 | `pr_created`/`pr_merged` sync handlers | `codeflow-cli/internal/db/sync.go` | Event handlers for PR lifecycle events |
| E5 | WAL-safe backup | `codeflow-cli/internal/db/` | Use `VACUUM INTO` or WAL checkpoint for safe backup |
| E6 | Embedded migrations | `codeflow-cli/internal/db/` | `//go:embed` for installed binary (no external SQL files) |
| E7 | Bridge test script | `.codeflow/testing/cli/test-go-cli.sh` | Register in test-config.json |
| E8 | Makefile coverage threshold | `codeflow-cli/Makefile` | `test-cover` enforces 85% across all business packages |
| E9 | Version output enrichment | `codeflow-cli/cmd/codeflow/` | `codeflow version` shows CodeFlow + Claude Code version, supports `--json` and `--check` |
| E10 | Uninstall `--keep-config` | `codeflow-cli/cmd/codeflow/` | Preserves `~/.config/codeflow/` by default removes it |
| E11 | Session V4 columns | `codeflow-cli/internal/session/` | Go struct includes pathflow_mode, tracking_level, etc. with UpdateTracking() |
| E12 | Migrate() stub cleanup | `codeflow-cli/internal/db/` | Wire to ApplyMigrations or remove stub |
| E13 | CICD branch prefix in schema seed | `.codeflow/scripts/db/schema.sql` | Verify `cicd/` (already correct, no change needed) |

---

## F. Decisions Confirmed

| # | Decision | Tier | Rationale |
|---|----------|------|-----------|
| F1 | `autorun start` (not `run`) | 1 | Async semantics match the design |
| F2 | `autorun sessions` (not `list`) | 1 | Noun subcommand pattern |
| F3 | Full 7-step init wizard | 2 | Claude Code + git provider are required dependencies |
| F4 | 16 doctor checks (not 15) | 1 | Epic has better granularity |
| F5 | Add `--quiet`/`-q` to welcome screen | 1 | Required for automation |
| F6 | Add `--json` to doctor | 1 | Required for agent parsing |
| F7 | Add `--keep-config` to uninstall | 1 | Standard CLI practice |
| F8 | Keep `ses-{ulid}` session ID format | 1 | Matches all table conventions |
| F9 | `CICD→cicd/` branch prefix (fix CLAUDE.md) | 1 | Schema seed is authority |
| F10 | `SPKE→spike/` branch prefix (fix CLAUDE.md) | 1 | Schema seed is authority |
| F11 | Keep WS-QA for HTFX/TEST/CHOR | 1 | pathflow-config.json is authority |
| F12 | Stage CHECK: add plan/docs/test, remove work | 1 | Align with actual pipeline stages |
| F13 | Real user from git email, Claude UUID in metadata | 2 | Separates human identity from agent identity |

---

## Cross-References

- **V4 Spec:** `/Volumes/DATA/Local/software-workspace/projects/codeflow-specification-v4/08-cli/`
- **Epic:** `project-management/epics/INF/INF-EPC-015/INF-EPC-015.md`
- **Design Analysis:** `.codeflow/docs/analysis/inf-epc-015-go-cli-design.md`
- **PathFlow Config:** `.codeflow/config/pathflow/pathflow-config.json`
- **Schema:** `.codeflow/scripts/db/schema.sql`
- **Task INF-TSK-015-018:** `project-management/epics/INF/INF-EPC-015/tasks/INF-TSK-015-018.md`
