---
id: ADR-002
title: Testing Consolidation — Retire Dual-Config Model and Shell Harness
status: accepted
date: 2026-04-18
deciders: [team-lead, cf-planning, cf-development]
consulted: [cf-quality-assurance, cf-review]
informed: [downstream-adopters]
supersedes: []
related: [ADR-001]
---

# ADR-002: Testing Consolidation — Retire Dual-Config Model and Shell Harness

## Status

Accepted (2026-04-18)

## Context

### Background

ADR-001 (2026-04-11) introduced the generic testing engine with a declarative `test-config.json` schema, a CTRF-shaped canonical model, and structural-check and test-tag capabilities. ADR-001's implementation task (INF-TSK-046-002 through INF-TSK-046-005) shipped the engine alongside the existing shell testing harness, creating a temporary dual-config state:

- **`.codeflow/config/testing/test-config.json`** — the new canonical config, driving `codeflow test --mode full` for Rust and Python targets.
- **`.codeflow/testing/test-config.json`** — the legacy shell-harness config, driving `run-all-tests.sh` and the `validate_structural_integrity` CI step for shell targets.

ADR-001 acknowledged this dual-config state was intentional and temporary. The convergence plan (§23 of the design analysis doc) mapped four phases (0–4) that would progressively retire the shell harness once the capability gaps were closed. The analysis identified four blockers: R4 (structural integrity), R8 (registration invariant), R2 (mode routing at file-set granularity), and R5 (category-based selection). R2 and R5 were assessed as PARTIAL — acceptable gaps given target-level `cwd` and multiple-target strategies. R4 and R8 were assessed as BLOCKERs that had to be resolved before retirement.

### Capability gap closure (INF-TSK-046-008)

INF-TSK-046-008 closed the two BLOCKERs:

**R4 — Structural integrity:** Implemented `codeflow test structural-check` via `codeflow-cli/core/src/testing/structural/mod.rs`. The subcommand enforces a bidirectional source↔test mapping using `structural.source_glob`, `structural.test_glob`, and `structural.pattern_map` fields on each target. Integrated into `codeflow test --mode full` — structural validation runs before a target's test runner, replacing the separate `validate_structural_integrity` CI step.

**R8 — Test registration invariant:** Implemented `test_files[]` array in `TargetConfig` (`codeflow-cli/core/src/testing/config/mod.rs`). Shell test authors enumerate their test files in the canonical config. Tag-based filtering (`--only-tag` / `--skip-tag`) replaces the legacy `priorities.CRITICAL.files[]` model, providing equivalent priority-driven execution through a declarative, per-file mechanism.

With R4 and R8 closed, all exit criteria for Phases 1–4 of the convergence plan were satisfied. Phases 0–4 were executed within INF-TSK-046-008, completing the retirement.

### Files retired in INF-TSK-046-008

| File | Role in shell harness |
|------|-----------------------|
| `.codeflow/testing/run-all-tests.sh` | Primary test runner |
| `.codeflow/testing/lib/test-runner.sh` | Execution orchestration |
| `.codeflow/testing/lib/test-parallel.sh` | Parallel job management |
| `.codeflow/testing/lib/test-reporting.sh` | Output formatting |
| `.codeflow/testing/lib/test-discovery.sh` | Test file discovery |
| `.codeflow/testing/lib/test-isolation.sh` | Environment isolation |
| `.codeflow/testing/lib/test-test-coverage.sh` | Shell coverage logic |
| `.codeflow/testing/lib/test-config.sh` | Config parsing helpers |
| `.codeflow/testing/lib/test-common.sh` | Shared shell utilities |
| `.codeflow/testing/lib/test-helpers.sh` | Assertion helpers |
| `.codeflow/testing/test-config.json` | Legacy config (priority lists) |
| `.codeflow/testing/README.md` | Shell harness docs |

`.codeflow/testing/` itself is retained as the location for shell test body files (`test-*.sh`). Only the harness infrastructure was removed.

## Decision

**Retire the dual-config model.** The legacy `.codeflow/testing/test-config.json` and the `run-all-tests.sh` harness (plus its 10 `lib/*.sh` files and `README.md`) are deleted. The `.codeflow/config/testing/test-config.json` managed by the generic engine is the single authoritative config for all test targets: Rust, Python, shell scripts, and any future stacks. Shell test body files continue to live at `.codeflow/testing/` and are discovered via the `shell-scripts` target's `test_files[]` array.

**`codeflow test --mode full` is the single test execution command** across all pipeline stages (WS-DEV, WS-QA). No CI step invokes `run-all-tests.sh` or `validate_structural_integrity` separately. Structural integrity enforcement runs inside `codeflow test --mode full` via the `structural` block on affected targets.

## Consequences

### Positive

- **One config, one command.** Adopters have a single entry point for test execution and a single file to maintain. The cognitive overhead of knowing "which target is in which config" is eliminated.
- **CI simplification.** The `validate_structural_integrity` CI step is removed. The `.github/workflows/test-suite.yml` run step is a single `codeflow test --mode full` invocation.
- **Structural integrity is now universal.** Any target — not just shell scripts — can declare a `structural` block. This capability was previously available only to shell tests via a shell-specific script.
- **Tag-based priority execution replaces priority blocks.** The old `priorities.CRITICAL.files[]` model required editing a JSON array. The new `test_files[].tags` model is per-file, declarative, and tool-agnostic. Adding a new test file with a `critical` tag automatically includes it in smoke runs without manual list maintenance.
- **ADR-001's stack-agnostic promise is now fulfilled at self-host.** The CodeFlow repo itself demonstrates the generic engine handling multiple targets (Rust, Python, shell) from a single config — the same configuration model a downstream adopter would use.

### Negative / Trade-offs

- **`test_files[]` requires explicit enumeration.** The legacy shell harness auto-discovered `test-*.sh` files by glob. The new model requires each test file to appear in the `test_files[]` array of the canonical config. This is a deliberate trade-off: explicit enumeration enables per-file tags and is protected by the ProtectionGuard (editing the config requires going through the CLI), but it does mean adding a new shell test now requires two steps (create file + update config) instead of one. The structural check's missing-test detection partially mitigates this by catching files that exist but are not registered.
- **`run-all-tests.sh` muscle memory.** Contributors who have internalized `bash .codeflow/testing/run-all-tests.sh --mode full` need to update to `codeflow test --mode full`. The contributor docs and CI references are updated in this same task; the binary is the new command.
- **R2 and R5 remain partial.** Mode routing at file-set granularity (R2) and category-based selection (R5) are not fully covered. These gaps are acceptable at current scale — multiple targets provide sufficient grouping — but may need revisiting as the shell test suite grows.

## Alternatives Considered

### Keep both configs in permanent coexistence

**Rejected.** Permanent coexistence forces every downstream adopter to learn and maintain two separate config formats for the same test surface. It directly contradicts the "one config" promise of ADR-001 and would confuse adopters about which config is authoritative for which behavior. The blocker gaps (R4, R8) were deliberately chosen as the retirement gate — once closed, the justification for coexistence disappears.

### Migrate R4 to a separate CI step only (not integrated into `codeflow test`)

**Rejected.** Keeping structural integrity as a separate CI step (`validate_structural_integrity`) would leave the conceptual split alive even after the shell harness is retired — operators would need to invoke two separate commands to get full coverage. Integrating structural checking into `codeflow test --mode full` provides the cleaner contract: one command runs everything.

### Use glob auto-discovery instead of `test_files[]`

**Partially deferred.** Glob auto-discovery (R8 path (a)) would eliminate the two-step "create file + update config" workflow. The `test_files[]` approach (path (b)) was chosen for INF-TSK-046-008 because it provides the tag capability that is not possible with pure glob discovery. If the two-step friction proves significant in practice, a `codeflow test register` CLI subcommand can be added to automate the config update — reducing the workflow to one command without removing the explicit enumeration model.

## References

- ADR-001: `.codeflow/docs/adr/ADR-001-generic-testing-subsystem.md`
- Design analysis doc (§23, convergence plan): `.codeflow/docs/analysis/generic-testing-subsystem.md`
- INF-EPC-046 epic: `project-management/epics/INF/INF-EPC-046/INF-EPC-046-epic.md`
- INF-TSK-046-008 task: `project-management/epics/INF/INF-EPC-046/tasks/INF-TSK-046-008.md`
- Structural check implementation: `codeflow-cli/core/src/testing/structural/mod.rs`
- Config model extension: `codeflow-cli/core/src/testing/config/mod.rs`
- CLI tag filter wiring: `codeflow-cli/cli/src/cmd/test.rs`
