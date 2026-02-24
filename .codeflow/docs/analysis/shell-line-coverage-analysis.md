# Shell Line Coverage Analysis

**Date:** 2026-02-23
**Status:** Investigation Complete — Pending Implementation
**Related Epic:** INF-EPC-014 (Stale Session PID Detection)
**Branch:** fix/stale-session-pid-detection

---

## 1. Problem Statement

CodeFlow has **78 shell scripts** (hooks, libraries, CLI) with **zero line coverage measurement**. Python scripts (18 files) have 97.33% line coverage via pytest-cov, but the shell layer — which makes up the majority of the codebase — has no equivalent.

The existing `test-coverage.sh` enforces:

- **Direction 1:** Every source script has a corresponding test file (structural)
- **Direction 2:** Every test file maps to a valid source script (reverse mapping)
- **Direction 3:** Changed source scripts have changed test files (git-aware, added in PR #65)

None of these measure **actual line execution coverage** — they only verify test files exist and are maintained.

## 2. Current State

### Python Coverage (Well-Measured)

| Metric | Value |
|--------|-------|
| Line coverage | 97.33% |
| Branch coverage | 96.91% |
| Tool | pytest-cov |
| CI enforced | Yes (`fail_under` in test-config.json) |

### Shell Coverage (Not Measured)

| Metric | Value |
|--------|-------|
| Line coverage | **Unknown** (no tooling) |
| Branch coverage | **Unknown** |
| Tool | None integrated |
| CI enforced | No (`fail_under: 0` in test-config.json) |

### Sample Measurements (kcov, 3 files)

| Script | Lines Covered | Total Lines | Coverage |
|--------|-------------|-------------|----------|
| `cf-session-end-cleanup.sh` | 63 | 80 | **78.75%** |
| `cf-session-start-init.sh` | 139 | 195 | **71.28%** |
| `cf-post-tool-use-pathflow-sentinel.sh` | 48 | 80 | **60.00%** |

These samples suggest shell scripts have 60-80% line coverage — decent but not enforced.

## 3. Tools Evaluated

### 3.1 kcov

| Attribute | Details |
|-----------|---------|
| Mechanism | `ptrace` (process tracing) |
| Speed | ~40 seconds per test file |
| Accuracy | High — tracks actual execution at process level |
| Output | Cobertura XML, HTML, JSON |
| Dependencies | C++ build (available via brew/apt) |
| CI support | GitHub Actions compatible |
| Limitation | Slow for large suites. Sourced file tracking is partial. |

**Estimated full suite time:** ~18 minutes for all 78 scripts (sequential).

**Result:** Produces accurate coverage. Used for the sample measurements above.

### 3.2 bashcov

| Attribute | Details |
|-----------|---------|
| Mechanism | `BASH_XTRACEFD` (bash xtrace) |
| Speed | ~1-2 seconds per test file |
| Accuracy | High for direct execution, broken for temp-dir isolation |
| Output | SimpleCov (JSON/HTML), Cobertura via plugin |
| Dependencies | Ruby 3.2+, simplecov gem |
| CI support | Yes — `ruby/setup-ruby` action + `gem install bashcov` |
| Limitation | **Cannot track files copied to temp directories** |

**Estimated full suite time:** 30-50 seconds with parallelism.

**Result:** Tests pass (80/80) but reports **0% coverage**. Root cause below.

### 3.3 bash -x + Custom Parser

| Attribute | Details |
|-----------|---------|
| Mechanism | Parse `set -x` trace output |
| Speed | Fast (bash-native) |
| Accuracy | Basic — no branch coverage, manual implementation |
| Output | Custom (must build parser) |
| Dependencies | None |
| CI support | Manual integration |
| Limitation | Maintenance burden, no ecosystem |

**Result:** Not evaluated. Reserved as fallback option.

## 4. bashcov 0% Coverage — Root Cause Analysis

### The Problem

bashcov was installed (Ruby 4.0.1, bashcov 3.3.0) and executed successfully:

```bash
bashcov --bash-path /opt/homebrew/bin/bash -- \
  .codeflow/testing/claude-hooks/session-end/test-cf-session-end-cleanup.sh
```

All 80 tests passed, but coverage reported **0/133 lines (0.0%)**.

### Root Cause: Test Isolation Pattern

CodeFlow's test framework creates **isolated environments** for each test:

```text
1. mkdir /tmp/cf-test-isolated-XXXXX/
2. cp -r source scripts → temp directory
3. Execute tests against the COPIES in temp dir
4. rm -rf temp directory on cleanup
```

bashcov tracks coverage by **source file path**. When the test executes a copy at `/tmp/cf-test-isolated-abc/.codeflow/scripts/...`, bashcov records coverage for that temp path. But:

- The temp file is **deleted** before bashcov writes the report
- bashcov warns: `"was executed but has been deleted since then - it won't be reported"`
- The only file bashcov DID track was the `codeflow` CLI entry point (not under test)

### Evidence

From `.resultset.json`:

- Only tracked file: `/Volumes/.../codeflow` (the CLI, 0/133 lines)
- No hook scripts tracked at all

bashcov warnings:

```text
bashcov: warning: /private/var/folders/.../cf-test-isolated-ngigr5/
  .codeflow/scripts/state/cf-work-state.sh was executed but has been
  deleted since then - it won't be reported in coverage.
```

## 5. Possible Solutions

### Option A: Use kcov (Works Today)

**Approach:** Integrate kcov into the test runner and CI workflow.

| Pro | Con |
|-----|-----|
| Already produces accurate results | Slow (~40s per test file) |
| Cobertura output for CI integration | ~18 min full suite (sequential) |
| No test framework changes needed | Needs kcov installed in CI |

**Optimization:** Run only on changed files (git-aware), parallelize with `xargs -P`.

### Option B: bashcov + Path Remapping

**Approach:** Configure SimpleCov to remap temp directory paths back to source paths.

| Pro | Con |
|-----|-----|
| Fast (~1-2s per test) | Requires `.simplecov` config with path translation |
| Rich ecosystem (SimpleCov) | Tests must NOT delete temp dirs during coverage runs |
| Native CI integration | May need test framework modifications |

**Implementation:**

1. Create `.simplecov` with `SimpleCov.root` and path filters
2. Add `--no-cleanup` mode to test framework (skip `rm -rf` on coverage runs)
3. Map `/tmp/cf-test-isolated-*/` back to repo root paths

### Option C: Modify Test Isolation Strategy

**Approach:** Instead of copying files, use PATH/environment isolation.

| Pro | Con |
|-----|-----|
| Scripts stay in place — all tools work | Large refactor of test framework |
| Cleanest long-term solution | Risk of test contamination |
| Both kcov and bashcov work natively | Needs careful env sandboxing |

**Implementation:**

1. Replace file copying with PATH manipulation
2. Use env vars to redirect state/config directories
3. Source scripts from original locations

### Option D: kcov for CI + bashcov for Local Development

**Approach:** Hybrid — use kcov in CI (accuracy matters, time less critical) and bashcov locally (speed matters).

| Pro | Con |
|-----|-----|
| Best of both tools | Two tool configurations to maintain |
| Fast local feedback loop | Must ensure results are comparable |
| Accurate CI enforcement | More complexity |

## 6. Recommendation

**Short-term (next sprint):** Option A — Integrate kcov into CI with git-aware filtering (only measure coverage for changed scripts). This works today with zero test framework changes.

**Medium-term:** Option B — Add bashcov support with path remapping for fast local development coverage. Requires adding a `--no-cleanup` mode to the test framework.

**Long-term:** Option C — Refactor test isolation to use environment-based sandboxing instead of file copying. This unblocks all coverage tools natively.

## 7. Proposed Coverage Thresholds

Based on the sample measurements (60-79% range):

| Phase | Threshold | Timeline |
|-------|-----------|----------|
| Initial | 50% (warning only) | Immediate |
| Baseline | 60% (hard fail) | After measuring all scripts |
| Target | 75% (hard fail) | After gap-filling tests |
| Stretch | 85% (hard fail) | Long-term goal |

## 8. Files Referenced

| File | Purpose |
|------|---------|
| `.codeflow/testing/lib/test-coverage.sh` | Structural coverage enforcement (3 directions) |
| `.codeflow/testing/lib/test-config.json` | Coverage thresholds (`fail_under: 0` for shell) |
| `.codeflow/testing/lib/test-isolation.sh` | Test isolation framework (creates temp dirs) |
| `coverage/.resultset.json` | bashcov output (gitignored) |
| `/tmp/claude/codeflow/kcov-single/` | kcov output from sample runs |
