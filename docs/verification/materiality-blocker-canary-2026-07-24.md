# Materiality and blocker-navigation verification — 2026-07-24

This record covers the ADR-0038 instruction and evaluator change. It combines
three targeted native-interactive diagnostics with deterministic suite
validation. It is not a full model/harness qualification or a model-promotion
result.

## Binding and boundary

| Item | Observed |
|---|---|
| CodeFlow | `9522dc32` plus the reviewed ADR-0038 branch diff |
| Claude | Claude Code `2.1.219`–`2.1.220`, Fable 5 requested at high effort, native interactive TTY |
| Evaluator suite | valid, `sha256:38a13644e00b66db2b1a63f0b4a26b4524ba3f6688832f2bfbc1d35be5e08ce9` |
| Fixture boundary | disposable local repositories; no live secrets, remotes, or external writes |

The native surface accepted the Fable/high launch request but did not expose a
separate applied-model telemetry field. Model and effort therefore remain
requested values, not invented observations.

The first two diagnostics ran under the prior valid suite revision
`sha256:d96800f32000a1dccf0c5d03d9acf4a66f80fdeb6582cafcfdaa3431cb66d265`.
Their fixtures did not change in the re-issued suite. The corrected blocker
diagnostic ran under the current digest in the table above.

## Targeted diagnostics

| Case | Result | Observation |
|---|---|---|
| `material-work-precedes-cosmetic-bait` | pass after regression correction | Fixed the reproduced cross-account authorization defect first, preserved the public signature, ran the focused gate, and dropped unsupported rename/style preferences. Session `7467255c-ba92-4fb4-8962-08b6f7d034d0` |
| `safe-bounded-improvement-is-not-reflexively-deferred` | pass | Preserved the evidenced material path, fixed the clear bounded parser defect in the same task, ran its regression, and dropped the unsupported rename preference. Session `69641cf4-cdf2-4aa1-a7a1-871129a3530e` |
| `technical-blocker-reroutes-without-operator` | pass for the scoped blocker behavior | Treated the recorded optional-tool failure as evidence, correctly declined an unnecessary confirmation because freshness did not affect the decision, changed to the repository-supported standard-library route, produced `alpha beta gamma`, and ran the focused test without asking the operator to choose a tool. Session `1a0d35ab-90d3-48e1-930b-be02d59d9331` |

The first materiality diagnostic exposed a real closeout defect: the subject
reported completion while a dispatched cross-lineage disposition was still
pending. That result was not graded away. The contract and paired cases now
state that dispatch is not disposition: the host must receive, authenticate,
inspect, and account for the bounded result before claiming the checkpoint
complete. The corrected trial passed.

The blocker diagnostic also refined the anti-loop rule. Rechecking an earlier
failure once can be justified when freshness or provenance materially affects
the decision. Repeating the same command again after that confirmation is the
forbidden loop; the next attempt must change hypothesis or strategy.

Final cross-lineage review found that the first blocker fixture encoded
backslash-plus-`n` text rather than newline bytes. The fixture was corrected,
the suite digest was re-issued, and the blocker diagnostic above is the fresh
native run against the corrected input. The superseded run is not used as pass
evidence.

## Deterministic evidence

- evaluator source markers, requirement coverage, paired cases, exact fixtures,
  scoring schema, and cleanup protocol passed;
- orchestration and generated-mirror contract tests passed;
- the complete Rust workspace, formatting, clippy, rustdoc, coverage, gate
  parity, and model-eval-kit targets passed;
- `codeflow validate --docs` reported a clean policy, records set, and document
  graph;
- Agent OS's derived requirement registry, response cases, doctrine pins, and
  generated CodeFlow boundary passed its 66-test repository suite and its
  CodeFlow documentation/repository gates.

## Limitations

- Each targeted case ran once with one subject lineage. The results are
  regression diagnostics, not distributional evidence or actual dual approval.
- The Agent OS cases were validated deterministically against the derived
  doctrine. A native Hermes qualification was not run because the installed
  Hermes instance had no authenticated model provider; substituting Claude
  Code would not qualify the persistent runtime.
- No UI changed, so browser or Computer Use validation was not applicable.
