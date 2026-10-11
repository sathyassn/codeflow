# Model and harness upgrades

<!-- Split out of docs/adoption.md: the maintenance path for a new model,
     harness release, permission profile, or material instruction revision.
     Carries the CAP-013 evaluation-kit detail graduated back from
     docs/capabilities/ under TSK-052. -->

## Concept

**A model change is qualified, never assumed.**

CodeFlow separates stable method from changing bindings (architecture decision
record ADR-0039): a change moves from qualification evidence to the managed
ensemble and a project's choice, and the durable doctrine is not rewritten. Do
not promote a new production model, harness release, permission profile, or
material instruction rewrite from a single successful task.

## Architecture

The maintenance boundary is how far a change is allowed to reach. Each row
names what to update, what evidence to regenerate, and what the change does not
license. A model-family upgrade changes bindings, never the doctrine. A new
harness first has to earn the guarantees CodeFlow depends on, before it reaches
a binding.

A binding reaches a project in this order:

```text
 full native run, graded against the pinned baseline
        |  explicit human approval
        v
 promoted local binding record   (one model, effort, harness, settings)
        |
        v
 current-ensemble.json           (managed primaries, efforts, workers)
        |
        v
 .codeflow/model-selection.json  (a project's choice, by binding ID)
```

`codeflow doctor --check model-bindings` checks the chain and fails closed on
any broken link.

| Change | Update | Re-prove | Do not add |
|---|---|---|---|
| New managed model/version or effort policy on a supported harness | Promoted binding evidence, then `current-ensemble.json` | Controlled full native evaluation with requested/observed identity | Doctrine rewrites or automatic routing |
| Consuming-project model choice | `.codeflow/model-selection.json` references an approved local binding ID for an exact stable role | `codeflow doctor --check model-bindings` plus normal duo canary | Raw selectors, partial fallback, or same-lineage pseudo-duos |
| Material harness release/configuration change | Capability evidence only if the contract changed; refresh the concrete binding | Native capability canary plus full binding qualification where behavior or settings changed | An inferred pass from `--version` alone |
| Genuinely new harness/provider | One catalog entry and the smallest reviewed transport/probe seam actually required | Every universal capability, then each production binding | Generic plugin machinery, arbitrary catalog commands, or speculative providers |
| Durable orchestration duty change | Doctrine/quality/routing contract and linked requirement/cases | Regression and over-trigger cases plus both primary judgments | Binding facts duplicated through many skills |
| Harness/model retirement | Remove its ensemble use and catalog support when no retained binding needs it | Graceful-degradation and remaining-ensemble canaries | Harness-internal worker tracking |
| Diagnostic case grouping | `packs.json` only | Pack resolution and underlying unchanged cases | A promotion shortcut |

Each part owns one thing:

- The orchestrator and quality and routing resources own duties that should
  survive model releases.
- `harnesses.json` owns the minimum guarantees and evidence for each
  capability-supported native harness. That catalog status does not qualify a
  model.
- A promoted local record binds one actual model, effort, harness, settings
  digest, and approved full result.
- `current-ensemble.json` selects the managed primaries, effort defaults,
  worker classes, and escalation triggers.
- Standard and full projects also own `.codeflow/model-selection.json`. Leave
  it absent or empty to use those defaults. An override maps only a stable role
  to a promoted local binding ID.
- Schema 2 of that file may also name a later line of the design owner seat
  as a design co-owner or consultant, and standing extra-family reviewers by
  area. Neither adds a model, selector or designation; the cf-model-orchestrator
  model-overrides reference describes both.
- Run `codeflow doctor --check model-bindings` before using an override. The
  entire selection fails closed rather than partly applying when a record is
  missing, ineligible, unsupported, drifted, or would collapse the two primary
  lineages.

The evaluator and human approval promote evidence. Neither the catalog, doctor
nor the current ensemble promotes anything on its own.

## Technical

Below are the qualification run, the kit's contracts, and the files and commands
each step touches. Run `/cf-evaluate-model` from the orchestrated maintenance
flow (ADR-0027); each step produces the evidence the next one depends on.

### The qualification run

| # | Step | Evidence it produces | Owner |
|---|---|---|---|
| 1 | Validate requirement and case traceability | Stable hard requirement IDs resolved against canonical source markers | The skill's standard-library tool |
| 2 | Materialize fresh disposable fixtures | A fresh one-commit repository per trial, grader material removed, evaluator state outside the subject tree, an opaque neutral subject path | The materializer |
| 3 | Exercise the candidate natively | A supervised native interactive Codex App/CLI or Claude Code session with the actual tools and Model Context Protocol (MCP) servers being qualified | The maintainer running the session |
| 4 | Run the full suite | Every case runs three times; canary mode runs each selected regression once and never qualifies | `/cf-evaluate-model` |
| 5 | Grade the retained evidence independently | Status recomputed from expected versus observed signals, with model, effort, harness, settings, permissions, tools, and budgets retained | The grader |
| 6 | Compare with the pinned baseline | No hard semantic regression; token and latency deltas are diagnostics that never compensate for lost behavior | The evaluator |
| 7 | Promote the binding | Explicit human approval, then a compact non-secret record under `~/.codeflow/qualified-bindings/` and an updated `current-ensemble.json` | Human approval |
| 8 | Re-check drift | `codeflow doctor --check model-bindings` reports requested-versus-observed contradictions and observable harness/settings drift, without inferring live model state | `codeflow doctor` |

- Canary mode maintains the corpus. Only a full run qualifies.
- A full run rebuilds fixture history after the materializer strips the skill
  and the expected answers. It retains network and resource budgets alongside
  the rest of the run conditions.
- Promotion additionally requires resolved validity and grader findings and
  complete full-suite evidence.
- Use the skill's marker- and run-ID-gated cleanup for fixtures. Never use it
  against the consuming project itself.
- The feature adds no CLI subcommand, model runtime, headless peer execution,
  CI model call, or generic cleanup surface (ADR-0027).

### Catalog, ensemble, and scoped routes

A source-controlled harness catalog marks a harness `capability-supported` only
after evidence of:

- native-interactive execution
- runtime provenance
- configured tools
- scoped work
- bounded failure
- recheckable results
- an effective permission boundary
- the git backstop

Catalog status does not qualify a concrete model binding.

One current ensemble record owns concrete primary selectors, effort policy,
typed internal routes, route status, and escalation triggers.

- A new harness or model name is not usable as a standing primary merely
  because it parses. The harness needs catalog evidence, and a concrete primary
  binding needs approved native full qualification.
- Standard and full projects may then reference an approved binding ID for an
  exact stable role in `.codeflow/model-selection.json`.
- The file is reference-only. Doctor resolves it atomically and fails closed on
  malformed, ineligible, unsupported, drifted, or lineage-collapsing overrides.
- An absent or empty file keeps the managed catalog.

Diagnostic packs and binding records:

- Composable diagnostic packs select existing cases without changing graders or
  promotion. They help isolate failures but never qualify a binding.
- Approved full results can emit non-secret local binding records.
- Doctor detects record contradictions and observable harness or settings drift
  without launching, inferring, promoting, or routing a model (ADR-0039).

Three statuses sit below full primary-binding promotion, and none substitutes
for it (ADR-0060). The evidence path belongs to the catalog's source
repository.

| Status | What it permits | Evidence it needs |
|---|---|---|
| Candidate | Bounded non-design execution under owning-primary inspection; configured is distinct from natively available and applied | Current routing evidence; it does not establish scoped quality |
| Scoped-qualified | Claims only for the exact evidenced harness, selector, effort and workload tuples | Three fresh accepted trials per pre-registered case and arm, complete applied provenance, primary integration, cross-family review and no unresolved validity threat |
| Economical default | A recommendation to use the tuple by default | Measured all-attempt benefit, including coordination and rework |

### What the evaluation skill carries

`/cf-evaluate-model` qualifies a new model or version, native harness release,
permission profile, or material CodeFlow instruction change as the complete
system users will run. The standard and full managed skill carries:

- stable hard requirement IDs
- source-marker traceability
- balanced regression and capability cases
- exact fixture overlays
- a native-interactive run protocol
- a standard-library tool for deterministic validation, materialization,
  scoring, baseline comparison, and fail-closed cleanup

| Focused pack | What it tests |
|---|---|
| `release-policy` | Compatibility judgment, misleading commit labels, compatible and no-release counterexamples, independent version domains, project-owned tool and adoption choices, and stale or conflicting publication evidence |
| `responsible-autonomy` | Ten standard-tier synthetic cases for privacy and delegation, outbound authority and retry, pressured incident and security work, and identity and fair decisions |
| `responsible-autonomy` | Effectful cases use a finite loopback simulator whose journal stays outside the subject tree, and setup records both materialized and configured tree digests |
| `guidance-retention` | Whether guidance survives a long session: the scripted multi-turn case kind, where the fixture supplies warm-up turns, the case prompt is the probe, and only the probe turn is graded |
| `guidance-retention` | Three hard probes: a landing time asked for in a plan, a status report, and a multi-part explanation |
| `guidance-retention` | Each probe and its paired negative runs in a fresh arm and an after-compaction arm on the Claude host |

Deterministic suite and grader checks are distinct from retained native trials.
A focused pack does not qualify a model binding, prove universal detection, or
stand in for a full promotion or a real-service authorization test.

For `guidance-retention`:

- The materializer sets the auto-compaction window in the disposable fixture's
  local settings.
- It pins the compaction percentage there at 100, so the scaffold's default of
  50 does not move the threshold.
- It refuses a scaffold without the rule map and re-injection hooks.
- `check-session` checks one session, the scripted turns and the compaction
  before extracting the probe turn.
- `retention-report` applies the bar.
- Offline checks prove the kit, not live behaviour.

A case may also carry `expected.files` and `expected.effects`, graded by
`eval_kit.py grade` on the work a session left:

- file state
- coherent reviews in the reviewer's verdict format
- branches and their tracking
- records consistent with the id registry
- changed paths
- judgements bound to the exact text from a judge whose calibration meets every
  labelled control (otherwise the assertion is ungraded and the trial never
  scores as a pass)
- confined product checks with expected output
- the shipped `codeflow validate --docs` and `codeflow ci`, which must finish
  their checks

This grading does not prove CLI use, readiness checks or review before
completion. Those need the harness's own record of the session (TSK-116), and a
command's process record is reported as supporting evidence only.

Graded cases live in a graded suite outside the shipped kit and binary:

- a public development suite in `evals/grader-dev/`
- the live delivery holdout of SPC-013 R-105 on the private archive's
  `test/live-delivery-holdout` ref, which is never merged

The graded suite works as follows:

- `evals/holdout.json` records the holdout's paths, digests and text
  fingerprints, and `holdout-check` keeps it out of the tracked tree.
- Subjects work in a separate subjects root under a fixture boundary that
  covers both roots.
- A trial's status is recomputed from a grade bound to the current case,
  fixture and grader.
- A timed-out or errored session is kept and graded as a failure, and a pack
  result must keep every trial (TSK-111).

### Hard contracts the suite evaluates

| Contract | What its cases require |
|---|---|
| `CF-OUT-002` | Contextual editorial quality without surface-cue policing |
| `CF-OUT-002` | Cases cover technical semantic preservation, operator uncertainty, consuming-project voice, sycophancy/inflation/formatting, medium-appropriate emoji, and false positives for legitimate punctuation, terms, and lists |
| `CF-DES-005` | Language/voice and appearance modes stay contextual and collapsible |
| `CF-DES-005` | Distinct project-evidenced voices, legible titles/actions/states, honest localization claims, applicable system/user mode and persistence evidence, and isolation between CodeFlow utility defaults and consuming-product design authority |
| `CF-DES-006` and `CF-DES-007` | Later exploration and asset use stay bounded and reviewable |
| `CF-DES-006` and `CF-DES-007` | A named unresolved choice before in-direction variants, an explicit stop condition, and exact reviewed-version provenance for material Plan vN+1 feedback |
| `CF-DES-006` and `CF-DES-007` | Separation of inspiration from user evidence, rights/consent and transformation evidence, refusal of unauthorized private-data uploads, and verification in the actual product context |
| `CF-DES-006` and `CF-DES-007` | Provider catalogs and parallel design databases remain outside the portable doctrine (ADR-0051) |
| `CF-QA-002` | Browser headlessness stays separate from interactive peer-model transport, with claim-matched behavior, visual, runtime, trace, and accessibility evidence |
| `CF-QA-002` | Its regression canary rejects screenshot-only verdicts, indiscriminate tracing, Computer Use as the default web driver, and helper-model ownership of the Claude primary's design judgment |
| `CF-QA-005` | Materiality-led review, execution focus, and proactive routing (see the cases below) |
| `CF-QA-007` and `CF-QA-008` | Complementary deterministic and contextual verification: applicable syntax/style, software composition analysis (SCA), source/data-flow, taint, secret, and architecture checks stay distinct from independent intent and semantic review |
| `CF-QA-007` and `CF-QA-008` | A deterministic red result cannot be waived by model consensus, and missing relevant static application security testing (SAST) or taint evidence remains visible residual risk |
| `CF-QA-007` and `CF-QA-008` | `CF-QA-008` adds a bounded-history craftsmanship case so repeated dependency and duplication erosion cannot hide behind a passing point diff |

`CF-QA-005` cases require the following:

- Consequential findings lead cosmetic nits.
- The review approves when only non-blocking preferences remain.
- Repeated symptoms are recognized as a possible systemic cause.
- Remediation effort stays out of severity.
- Common Vulnerability Scoring System (CVSS) aligned security severity and
  separate confidence are preserved before the result maps to the general gate.

A paired execution fixture tests two distinctions:

- An actionable material blocker versus cosmetic bait.
- A completed critical path versus a clear, safe, in-scope improvement. Models
  must protect the required gates without reflexively deferring bounded work.

Other `CF-QA-005` rules:

- Only genuinely uncertain observations are consolidated for one natural duo
  checkpoint. When retained, they are tracked once with evidence and a
  deterministic revisit event.
- Other cases escalate or track evidenced out-of-scope risk without silently
  expanding scope or generating one issue per nit.

The paired governance cases separate persistence from assumption:

- A discoverable tool failure must move through a new evidenced hypothesis or
  an accepted-outcome-preserving reversible strategy. It must not repeat, and
  it must not ask the operator to choose a tactic.
- Missing product intent or a public contract still blocks for a well-framed
  operator decision.
- Retry counts alone prove neither case.
