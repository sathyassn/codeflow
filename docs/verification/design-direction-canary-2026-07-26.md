# Design-direction diagnostic — 2026-07-26

This targeted diagnostic checks the new proportionate design-intent contract,
its managed mirrors, and its behavior in both native interactive host
directions. It is not a full model-binding qualification or a rendered-product
test.

## Boundaries

| Item | Observed |
|---|---|
| CodeFlow subject | `2.1.0`, base `34efbff64b6780ccaf5562a8aa5de0a34ce18530` plus the `feat/design-direction` working-tree diff |
| Claude surface | Claude Code `2.1.220`; Fable 5 and high effort observed in the native TUI |
| Codex surface | Codex CLI `0.144.3`; the operator-side TUI showed GPT-5.6 Sol/high, while the retained Codex-host result did not expose runtime-applied model or effort |
| Behavioral cases | `design-intent-proportionality` and `evidence-grounded-design-choice`, fresh materialized trials |
| Task boundary | Planning only; the fixture repositories remained clean |

## Deterministic repository evidence

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | PASS |
| `cargo test --workspace` | PASS |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo test -p codeflow-core --test manifest_consistency` | PASS — 9 tests |
| `cargo test -p codeflow-core --test model_eval_contract` | PASS — 21 tests |
| `cargo test -p codeflow-core --test orchestration_contract` | PASS — 11 tests |
| `codeflow validate --docs` | PASS |
| `codeflow test --mode quick` | PASS — 6 targets |
| `eval_kit.py validate-suite` | PASS — `sha256:2ddb9414ea65c6df34e7939c3d313a7dfeafc10916d14d678c11dc69d6a232a1` |
| `quick_validate.py assets/base/agents/skills/cf-design` | PASS |
| `git diff --check` | PASS |

The complete uncommitted diff also received a read-only Fable 5/high review.
Its final verdict was **approve** with no blocker or major finding. The review
checked every changed file for proportionality, existing-doctrine dilution,
model-name coupling, duplicate authority, fabrication controls,
accessibility/fidelity semantics, eval validity, and mirror drift. Its two
actionable minor findings were addressed before this record: alternate
accessibility targets now require a recorded surface-specific rationale, and
the solo develop flow now routes applicable work through `cf-design`.

## Native results

| Host direction | Result | Cross-lineage evidence |
|---|---|---|
| Claude host to Codex | PASS | Independent discovery, one `changes_requested` review, a revised Plan v2, and Codex approval through the official plugin. Native Codex thread `019f9fbe-711e-7fc3-bc7a-8c98ab33cb11`; turns `019f9fbe-79c6-7b23-ae84-baf2eaed99ad`, `019f9fc7-6026-78b2-bb91-ffd60cc3f10e`, and `019f9fcc-0d53-74c0-92c5-2a066802e9fd` |
| Codex host to Claude | PASS with reduced assurance | Candidate `codeflow delegate` lifecycle recorded native Claude session `1be1887c-4193-4db5-9bc7-fc44f6cdb8c9`, prompt acceptance, terminal returns, independent discovery, and exact Plan v1 approval |

Both directions independently produced the intended proportional collapse:

- A, an exact two-pixel correction with unchanged accepted design, became
  `DESIGN_INTENT: N/A`.
- B, a bounded warning inside an established billing system, became
  `DESIGN_INTENT: conform`.
- C, a new materially open setup surface, compared three structurally distinct
  directions and settled one coherent direction before implementation.

Both plans grounded intent in repository evidence, labeled inference, refused
fabricated users/research/metrics, separated operator-owned decisions from
reversible implementation choices, named WCAG 2.2 AA and rendered fidelity
evidence, selected verification strength proportionately, and stopped without
editing the fixture.

The revised counterfactual case was then observed in a fresh native Fable
5/high session and a fresh native GPT-5.6 Sol/high session. Both subjects:

- retained the archive's warm, editorial, generously spaced register because
  the brief, primary material, curator direction, and finding aids supported
  it;
- rejected transplanting the same register into a time-critical incident
  console whose accepted compact hierarchy, status roles, and keyboard scanning
  contradicted it;
- rejected the unrelated player metaphor and fabricated `99.99%` claim; and
- treated contextual evidence—not familiarity, novelty, or a style
  blacklist—as the deciding principle.

Retained response digests:

- Claude-host result:
  `b7f1675f7809f54d0dee7d9a31090c25f29c630c9e16ff083314f42615eb4a00`
- Codex-host result:
  `70a52c42058f84782432a843ea9cc34f1a77d49cd1e9a662e309848457414b7a`
- Fable counterfactual transcript:
  `0e17ba6c744993212cf3c3004469f9db0bb19cd4f3fc4ffcf70f490e00945d84`
- Codex counterfactual transcript:
  `b004fa2d388a5e5debb49ab9beb5bad3a5d55a1410fd2d854f5fac1cd56faf8c`

## Assurance notes

The Claude-host run used the official Codex plugin and retained native thread
and turn provenance. The plugin transport did not echo applied model and effort,
so its `gpt-5.6-sol@high` selector remains requested rather than independently
observed.

The corrected Codex-host run used the candidate CodeFlow binary on `PATH`;
`codeflow doctor --check model-bindings` passed outside the sandbox and the
candidate `delegate` lifecycle completed. The selected user-scope Claude
`autoMode.classifyAllShell` boundary did not verify, so the host correctly used
the documented sandboxed `acceptEdits` fallback and labeled the result reduced
assurance. The first canary prompt was delivered manually after the host sent it
before the interactive TUI was ready; subsequent discovery and approval turns
completed through the correlated lifecycle without intervention. This run
therefore demonstrates the design behavior and the lifecycle's evidence
contract, but is not claimed as an unattended transport qualification.

An earlier Codex-host calibration accidentally resolved the globally installed
CodeFlow binary rather than the candidate. It produced the right design ladder
but degraded because that binary lacked the current model-binding check and
delegate lifecycle. It is retained as diagnostic calibration and is not counted
as the candidate result.

The counterfactual trials resolved the candidate binary, but both subjects
initially attempted the repository's non-trivial duo workflow while the other
subject was doing the same. This created a circular bounded wait. The operator
ended each wait through the documented missing-seat degradation path. Each
subject then returned its independently formed verdict and explicitly labeled
the reduced assurance. These trials count as observed design-choice behavior,
not as successful cross-model transport evidence. Two earlier counterfactual
calibrations that resolved the globally installed binary are retained outside
the repository and excluded entirely.

The other new behavioral cases were forward-checked analytically against their
fixtures and contract:

- explicit operator direction,
- design-fidelity review, and
- accessibility-target selection.

Those checks passed, but they are not reported as observed model behavior. The
rendered comparison case was protocol-validated only: the fixture contains no
application to build or render, so no screenshot, browser journey, or
accessibility conformance result is claimed.

## Decision

The repository gates, full-diff review, and two native host-direction trials are
sufficient to land the design-direction contract as a diagnostic baseline. They
do not promote a binding, prove repeated reliability, or replace rendered
verification when a consuming project contains an implemented surface.
