# Model-role and layered-verification diagnostic — 2026-07-25

This is a focused native-interactive diagnostic, not a binding qualification or
promotion result. It compares Claude Fable 5 high and Claude Opus 5 high on
three judgment-heavy cases from the revised suite while holding the Claude Code
version (`2.1.220`), permission mode (`auto`), prompt, fixture digest, and
CodeFlow build constant. Each subject ran in a fresh trusted disposable
repository with lifecycle-correlated session and terminal evidence.

## Cases and observed outcome

| Case | Fable 5 high | Opus 5 high |
|---|---|---|
| `reject-overengineered-correct-change` | Correctly blocked speculative abstraction, dead configuration, and unverifiable tests. It skipped the peer lane as disproportionate. | Reached the same substantive verdict with precise public-API and evidence findings. It also skipped the peer lane. |
| `longitudinal-craftsmanship-erosion` | Correctly identified the four-point boundary/duplication trend, attempted the Codex peer lane, recorded the missing plugin companion, and degraded legibly. | Produced a strong bounded-history analysis, but skipped the available peer lane without first proving it unavailable. |
| `layered-verification-red-gate` | Correctly separated deterministic and contextual evidence and blocked on the unresolved healthy CodeQL finding. It did not modify the fixture. | Reached the same security verdict, but skipped the peer lane and created an extra verdict file during review-only work. |

Both models found the material defects. Neither candidate produced a clean
formal result for all three cases because the expected orchestration/cross-lineage
evidence was incomplete. The longitudinal Fable run additionally exposed an
environment validity issue: the official Codex plugin companion was unavailable
inside the disposable subject environment. That is retained as a capability
gap, not graded away.

## Evidence index

| Subject | Native session | Terminal record SHA-256 |
|---|---|---|
| Fable / over-engineered | `8f49f37b-f647-4f61-b9cc-a715225cb0d3` | `199253b6b2799265b92f3d4a95c17b2851006bfbf31915273e9254dc3b38eb06` |
| Opus / over-engineered | `4212be57-cbac-4aee-808d-f25fa738744e` | `0745d6dadd42815bf4419756320cdf25efc4286b5ccbc20d9965030c4cfc4e93` |
| Fable / craftsmanship | `c0830fff-34a3-4de8-bdd6-25cfef0c9e03` | `052361fcdab77da4615074c6c72929ab525970cbfe1ff56d5cd90c209ff5071c` |
| Opus / craftsmanship | `1043782e-f6db-4488-8d15-334fce06ed2a` | `e7e7e35b4ce6f696f6996d9b0e00acce6771eded421557b2a5c54d2a9450f81b` |
| Fable / layered verification | `077e8c2c-a51a-40db-b523-61ca2d094ccf` | `62b87957400567f67c93402c023372dd7b0df1a6f899b566f890277f5bd29dcb` |
| Opus / layered verification | `eb7712a3-9396-4dad-9855-87db66871e74` | `88b2baed02b1471482ff85a627034a9ca7e013ccb587aefd7bb53ca82e666cca` |

The terminal records remain in the marked temporary evaluation run root until
the pull request is approved. Their hashes permit later correlation without
putting transcript bodies or evaluator state in the repository.

## Decision

Keep Fable 5 high as the managed `claude-judgment-primary`. This sample is too
small for promotion and does not show an Opus advantage in durable contract
adherence. Opus remains a qualified internal worker route under the current
ensemble. A future primary change requires the complete full suite, three trials
per case, requested/observed binding evidence, independent grading, no hard
regression, and human approval. The project-selection mechanism introduced by
ADR-0041 can then adopt the approved binding without rewriting durable doctrine.
