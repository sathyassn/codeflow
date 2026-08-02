# cf-present exact-review correction evidence — 2026-08-01

**Task:** TSK-011

**Decision envelope:** accepted ADR-0052 and SPC-004; no outcome or graph change

**Correction base:** `e11c9de8674328cfeb3fc65889657b870672c53f`

## Claim-matched cases

| Boundary | Deterministic evidence | Result |
|---|---|---|
| Launch crash and retry | Per-attempt record, launch lease, unregistered and committed-record recovery tests | Pass |
| PID absence/reuse | Exact marker inventory; reused current PID is not signalled; Windows candidate chasing uses a bounded duplicate-aware worklist | Pass on macOS; Windows adapter cross-compiles, native qualification remains TSK-007 |
| Profile/resource proof | Unix exact candidate/process-group checks; Windows bounded exclusive-handle tree walk | Pass on macOS; native Windows pending TSK-007 |
| Crash cleanup | Exact UUID/nonce directory plus matching create/delete transaction marker | Pass; markerless exact names remain untouched and fail closed |
| Selected clear | Unrelated corrupt session and malformed staging state are not loaded or mutated | Pass; retained selected state is an explicit failure |
| Service crash | Live CLI service is killed, then close and selected clear remove stale identity and runtime | Pass |
| Private files | Windows every create-new append/lease path uses the protected `CreateFileW` descriptor | Cross-target compile and static path review pass; native ACL qualification remains TSK-007 |
| State-root confinement | Relative XDG state is ignored and relative HOME is rejected | Pass |

## Local verification

- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `cargo test -p codeflow-present --all-targets`: 90 passed in five
  consecutive final runs. Full mode runs workspace and coverage test processes
  concurrently, so the three macOS process-group fixtures share a test-only
  cross-process lease; the previously intermittent full-mode case passed after
  this correction. Production ownership checks remain fail closed.
- `cargo test -p codeflow-cli --test present_cli`: 2 passed, including service
  crash → close → selected clear.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo cross-check-windows`: passed without warnings.
- `cargo cross-check-linux`: passed against the pinned GNU 2.17 zigbuild
  target.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`: passed.
- `codeflow validate --docs`: 29 records clean; documentation graph clean.
- Presentation web `check`, browser, and supply-chain scripts: passed;
  reproducible tree
  `d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900`,
  lazy/interactive/no-script/selection/diagram/axe/320 px cases passed, and
  154 packages reported zero vulnerabilities.
- Release-built `codeflow test --mode full`: all seven configured targets
  passed, including the 90% line-coverage floor, gate parity, and model-eval
  kit.
- `cargo test --workspace --all-targets`: passed, including 1,458 core tests,
  90 presentation tests, and all CLI/integration contract suites.

- `gitleaks protect --staged --redact`: approximately 53 KB exact staged range;
  no leaks found.
- Final bounded-worklist and test-integrity delta: approximately 6.7 KB staged;
  no leaks found.

Cross-compilation is not represented as native Windows runtime evidence;
native Windows qualification remains TSK-007.
