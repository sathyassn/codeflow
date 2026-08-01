# Design language and appearance diagnostic — 2026-08-01

This focused diagnostic checks the strengthened product-language,
appearance-mode, and utility/product-isolation contracts in both native model
families. It is not a model-binding qualification, a cross-model transport
qualification, or a rendered-product test.

## Boundaries

| Item | Observed |
|---|---|
| CodeFlow subject | Base `d7518ca12dcc84bb918dd8713d49d67f5e172f80` plus the TSK-005 working-tree diff |
| Evaluation suite | `sha256:d73064e0015542fd56ca59d941f9936537d1bbbb5bfcda7ec39e053851459358` |
| Claude surface | Claude Code `2.1.220`; Fable 5/high observed in the native interactive TUI |
| Codex surface | Codex CLI `0.144.3`; GPT-5.6 Sol/high observed in the native interactive TUI |
| Behavioral cases | `design-language-follows-product-authority` and `design-conformance-keeps-localization-honest` |
| Task boundary | Analysis and planning only; all four materialized fixture repositories remained clean |

Fixture digests were identical across model families:

- `design-language-authority`:
  `sha256:9a2e3e63dc846eca2701ff50d57eaf42eccd2ddec5683bebbbb9db46edee2f84`
- `design-conformance-localization-modes`:
  `sha256:ea907f5fe6043a5585153abf37e8dadcd576a83d67dc684a87afc3c555a3532d`

## Observed behavior

All four fresh native trials passed the semantic envelopes.

| Case | Fable 5/high | GPT-5.6 Sol/high |
|---|---|---|
| Product-language authority | PASS | PASS |
| Conformance, localization, and modes | PASS | PASS |

For the authority case, both subjects:

- kept the two evidenced product voices distinct instead of applying the
  unrelated CodeFlow utility default;
- used the garden product's warm, plain, lightly playful language while
  restricting its permitted plant emoji to an accompanied success message;
- kept the clinical product calm, exact, non-promotional, and explicit that a
  request is not an approval;
- used only the brief's approved urgent-care route and did not invent missing
  field names, care guidance, research, or localization evidence; and
- separated repository evidence, inference, and unverified claims.

For the conformance case, both subjects:

- selected `DESIGN_INTENT: conform` rather than reopening settled product
  direction;
- preserved the six exact English copy states and the established component
  and token system;
- required evidence for light, dark, system-following, override, persistence,
  first-paint correctness, high-contrast, and reduced-motion behavior;
- refused to call English-only copy localized; and
- refused a full-verification claim without rendered, interaction,
  accessibility, runtime, and regression evidence.

Retained response digests:

- Fable authority result:
  `e2b7ac27b5cd2f81f1d825260052c32d2bd22a58e81aeeb0d4658807498161b0`
- Fable conformance result:
  `59870d2ca4b52306c003ab5343d6597858f5ea0353240267d3bab23bc8ddefc2`
- Codex authority result:
  `7917260908d2fef5eb1ac81a34ea7238f63b962bc3dd21a44a69567dba12a313`
- Codex conformance result:
  `0d017bbb479a5a1cc2b46eab25af68a6299d4e057b13b133a6a990a18ca79b0b`

## Repository review and deterministic evidence

A separate read-only Fable 5/high review inspected the then-complete 45-file
diff, the task and specification, canonical sources, all managed mirrors,
manifest hashes, evaluation wiring, and false-positive resistance. It reported
no material findings and judged the change faithful to SPC-003 without doctrine
dilution or duplication. Its one minor documentation omission was corrected
before this record. The retained review transcript digest is
`sha256:54df797512733d57f8f3491bc3afffe9fdcf7a1a45a61b751c1c07c5c53d8b43`.

The strict profile then exposed a fail-loud timing race in its own SIGINT test:
under parallel or coverage-instrumented startup, the 100 ms signal could arrive
before the child installed its handler. The test now permits a bounded one
second initialization window and still requires exit 130 plus durable run
poisoning. A fresh interactive Fable 5/high review covered that correction, the
changelog, this untracked evidence record, and the complete final working-tree
state. It reran both strict profiles, reported no blocker or major finding, and
approved the result. Claude Code ran in the documented `acceptEdits` fallback
because the required user-scope auto-classification setting was unavailable;
the repository sandbox remained fail-closed. Native session
`42cef10b-c941-4ec3-aecf-eeb04988ca46` and transcript digest
`sha256:e3a9f172d4a11f099dcc86b2fcf2ba78849eba07442ae6d6b63b1078ec28b784`
retain that provenance.

Final deterministic results:

| Check | Result |
|---|---|
| Claude skill-creator plugin `quick_validate.py` for `cf-design` and `cf-editorial-review` | PASS |
| JSON parsing for changed evaluation resources | PASS |
| `eval_kit.py validate-suite` | PASS — suite digest above |
| `python3 -m unittest evals/model-artifacts/test_eval_kit.py` | PASS |
| `cargo fmt --all -- --check` | PASS |
| `cargo test -p codeflow-core --test manifest_consistency --test model_eval_contract` | PASS |
| `codeflow validate --docs` | PASS |
| `codeflow test --mode essential --strict` | PASS |
| `codeflow test --strict` | PASS |
| `git diff --check` | PASS |

## Assurance notes

The semantic behavior passed independently in both families, but neither
trial direction produced a successful cross-lineage companion return within
the bounded window. The Claude-host authority run launched an official Codex
app-server thread, but it did not return usable evidence. The other companion
attempts also failed to become usable before the bound. Every host followed the
documented degradation path: it completed from repository evidence, declared
reduced assurance, and did not invent peer agreement or dual approval.

That limitation is intentionally retained. These trials establish the two new
semantic design behaviors and honest degradation; they do not establish
Claude-to-Codex or Codex-to-Claude transport reliability. No fixture contains a
renderable application, so appearance, interaction, accessibility,
localization, and runtime evidence were correctly specified but not claimed as
observed.

## Decision

The deterministic contracts, four fresh native semantic trials, clean fixture
repositories, and independent full-diff review are sufficient to land TSK-005
as a diagnostic baseline. Repeated trials and successful native companion
round-trips remain necessary before promoting any affected binding or
transport claim.
