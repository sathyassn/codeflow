# Model and harness upgrades

<!-- Split out of docs/adoption.md: the maintenance path for a new model,
     harness release, permission profile, or material instruction revision.
     Carries the CAP-013 evaluation-kit detail graduated back from
     docs/capabilities/ under TSK-052. -->

## Concept

**A model change is qualified, never assumed.**

```cf-stage
durable doctrine | orchestrator and quality duties @accent
->
harness capability contract | universal guarantees per harness
->
approved binding evidence | one model · effort · harness · settings
->
managed current ensemble | primaries · effort policy · triggers
->
project selection | a stable role mapped to a binding ID @positive
caption: a change moves rightward from the doctrine; the doctrine is not rewritten
```

CodeFlow separates stable method from changing bindings (ADR-0039), and the
figure is that separation read left to right. Do not promote a new production
model, harness release, permission profile, or material instruction rewrite
from a single successful task. Architecture says which stage each kind of
change touches; Technical is the run itself.

## Architecture

The maintenance boundary is the point in the figure a change is allowed to
reach. Each row names what to update, what evidence to regenerate, and what the
change does not license. A model-family upgrade stays right of the doctrine; a genuinely new
harness has to earn the guarantees CodeFlow depends on before it reaches a
binding at all.

| Change | Update | Re-prove | Do not add |
|---|---|---|---|
| New managed model/version or effort policy on a supported harness | Promoted binding evidence, then `current-ensemble.json` | Controlled full native evaluation with requested/observed identity | Doctrine rewrites or automatic routing |
| Consuming-project model choice | `.codeflow/model-selection.json` references an approved local binding ID for an exact stable role | `codeflow doctor --check model-bindings` plus normal duo canary | Raw selectors, partial fallback, or same-lineage pseudo-duos |
| Material harness release/configuration change | Capability evidence only if the contract changed; refresh the concrete binding | Native capability canary plus full binding qualification where behavior or settings changed | An inferred pass from `--version` alone |
| Genuinely new harness/provider | One catalog entry and the smallest reviewed transport/probe seam actually required | Every universal capability, then each production binding | Generic plugin machinery, arbitrary catalog commands, or speculative providers |
| Durable orchestration duty change | Doctrine/quality/routing contract and linked requirement/cases | Regression and over-trigger cases plus both primary judgments | Binding facts duplicated through many skills |
| Harness/model retirement | Remove its ensemble use and catalog support when no retained binding needs it | Graceful-degradation and remaining-ensemble canaries | Harness-internal worker tracking |
| Diagnostic case grouping | `packs.json` only | Pack resolution and underlying unchanged cases | A promotion shortcut |

The orchestrator and quality/routing resources own duties that should survive
model releases. `harnesses.json` owns the minimum guarantees and evidence for
each capability-supported native harness; that catalog status does not qualify
a model. A promoted local record binds one actual model, effort, harness,
settings digest, and approved full result.
`current-ensemble.json` selects the managed primaries, effort defaults, worker
classes, and escalation triggers. Standard/full projects also own
`.codeflow/model-selection.json`. Leave it absent or empty to use those
defaults; an override maps only a stable role to a promoted local binding ID.
Run `codeflow doctor --check model-bindings` before using an override. The
entire selection fails closed rather than partly applying when a record is
missing, ineligible, unsupported, drifted, or would collapse the two primary
lineages.

For a model upgrade on a capability-supported harness, evaluate the new
concrete binding and update the ensemble record; do not rewrite the doctrine. A
new harness additionally needs evidence for every universal capability and
only the transport-specific code or instructions its observed behavior
requires. It becomes eligible for a concrete binding only after the full
native evaluation and approval. Removing a harness retires its
catalog/ensemble entry while keeping graceful degradation. None of these paths
adds automatic discovery, promotion, routing, or vendor-internal worker
tracking. This division keeps model-family upgrades localized while making a
new harness earn the guarantees CodeFlow depends on. The evaluator and human
approval promote evidence; neither the catalog, doctor, nor current ensemble
promotes anything automatically.

## Technical

Below are the qualification run, the kit's contracts, and the files and commands
each step touches. Run `/cf-evaluate-model` from the orchestrated maintenance
flow (ADR-0027); each step produces the evidence the next one depends on.

### The qualification run

| # | Step | Evidence it produces | Owner |
|---|---|---|---|
| 1 | Validate requirement and case traceability | Stable hard requirement IDs resolved against canonical source markers | The skill's standard-library tool |
| 2 | Materialize fresh disposable fixtures | A fresh one-commit repository per trial, grader material removed, evaluator state outside the subject tree, an opaque neutral subject path | The materializer |
| 3 | Exercise the candidate natively | A supervised native interactive Codex App/CLI or Claude Code session with the actual tools and MCPs being qualified | The maintainer running the session |
| 4 | Run the full suite | Every case run three times; canary mode runs selected regressions only and never qualifies | `/cf-evaluate-model` |
| 5 | Grade the retained evidence independently | Status recomputed from expected versus observed signals, with model, effort, harness, settings, permissions, tools, and budgets retained | The grader |
| 6 | Compare with the pinned baseline | No hard semantic regression; token and latency deltas are diagnostics that never compensate for lost behavior | The evaluator |
| 7 | Promote the binding | Explicit human approval, then a compact non-secret record under `~/.codeflow/qualified-bindings/` and an updated `current-ensemble.json` | Human approval |
| 8 | Re-check drift | `codeflow doctor --check model-bindings` reports requested-versus-observed contradictions and observable harness/settings drift, without inferring live model state | `codeflow doctor` |

Canary mode maintains the corpus; only a full run qualifies. A full run
rebuilds fixture history after the materializer strips the skill and the
expected answers, and retains network and resource budgets alongside the rest of
the run conditions. Promotion additionally requires resolved validity and grader
findings and complete full-suite evidence. Use the skill's marker- and
run-ID-gated cleanup for fixtures; never use it against the consuming project
itself. The feature adds no CLI subcommand, model runtime, headless peer
execution, CI model call, or generic cleanup surface (ADR-0027).

### Catalog, ensemble, and scoped routes

A source-controlled harness catalog marks a harness `capability-supported` only
after evidence of native-interactive execution, runtime provenance, configured
tools, scoped work, bounded failure, recheckable results, an effective
permission boundary, and the git backstop. Catalog status does not qualify a
concrete model binding. One current ensemble record owns concrete primary
selectors, effort policy, typed internal routes, route status, and escalation
triggers. A new harness or model name is not usable as a standing primary merely
because it parses; the harness needs catalog evidence and a concrete primary
binding needs approved native full qualification.
Standard/full projects may then reference an approved binding ID for an exact
stable role in `.codeflow/model-selection.json`. The file is reference-only;
doctor resolves it atomically and fails closed on malformed, ineligible,
unsupported, drifted, or lineage-collapsing overrides. An absent or empty file
keeps the managed ensemble.
Composable diagnostic packs select existing cases without changing graders or
promotion; they help isolate failures but never qualify a binding. Approved full results can emit non-secret local binding records;
doctor detects record contradictions and observable harness/settings drift
without launching, inferring, promoting, or routing a model (ADR-0039).

A configured candidate is distinct from native availability and applied
selection: current routing evidence may permit bounded non-design execution
under owning-primary inspection without establishing scoped quality. A
scoped-qualified claim covers only exact evidenced harness/selector/effort/
workload tuples and requires three fresh accepted trials per pre-registered
case and arm, complete applied provenance, primary integration, cross-family
review, and no unresolved validity threat. The evidence path belongs to the
catalog's source repository. This focused status never substitutes for full
primary-binding promotion; an economical-default recommendation separately
requires measured all-attempt benefit including coordination and rework
(ADR-0060).

### Hard contracts the suite evaluates

| Contract | What its cases require |
|---|---|
| `CF-OUT-002` | Contextual editorial quality without surface-cue policing: technical semantic preservation, operator uncertainty, consuming-project voice, sycophancy/inflation/formatting, medium-appropriate emoji, and false positives for legitimate punctuation, terms, and lists |
| `CF-DES-005` | Language/voice and appearance modes stay contextual and collapsible: distinct project-evidenced voices, legible titles/actions/states, honest localization claims, applicable system/user mode and persistence evidence, and isolation between CodeFlow utility defaults and consuming-product design authority |
| `CF-DES-006` and `CF-DES-007` | Later exploration and asset use stay bounded and reviewable: a named unresolved choice before in-direction variants, an explicit stop condition, exact reviewed-version provenance for material Plan vN+1 feedback, separation of inspiration from user evidence, rights/consent and transformation evidence, refusal of unauthorized private-data uploads, and verification in the actual product context. Provider catalogs and parallel design databases remain outside the portable doctrine (ADR-0051) |
| `CF-QA-002` | Browser headlessness stays separate from interactive peer-model transport, with claim-matched behavior, visual, runtime, trace, and accessibility evidence. Its regression canary rejects screenshot-only verdicts, indiscriminate tracing, Computer Use as the default web driver, and helper-model ownership of the Claude primary's design judgment |
| `CF-QA-005` | Materiality-led review, execution focus, and proactive routing (see the cases below) |
| `CF-QA-007` and `CF-QA-008` | Complementary deterministic and contextual verification: applicable syntax/style, SCA, source/data-flow, taint, secret, and architecture checks stay distinct from independent intent and semantic review; a deterministic red result cannot be waived by model consensus, and missing relevant SAST/taint evidence remains visible residual risk. `CF-QA-008` adds a bounded-history craftsmanship case so repeated dependency and duplication erosion cannot hide behind a passing point diff |

`CF-QA-005` cases require consequential findings to lead cosmetic nits, approve
when only non-blocking preferences remain, recognize repeated symptoms as a
possible systemic cause, keep remediation effort out of severity, and preserve
CVSS-aligned security severity and separate confidence before mapping the
result to the general gate. A paired execution fixture distinguishes an
actionable material blocker from cosmetic bait, then a completed critical path
from a clear, safe, in-scope improvement: models must protect the required
gates without reflexively deferring bounded work. Only genuinely uncertain
observations are consolidated for one natural duo checkpoint and, when
retained, tracked once with evidence and a deterministic revisit event. Other
cases escalate or track evidenced out-of-scope risk without silently expanding
scope or generating one issue per nit.

The paired governance cases separate persistence from assumption. A
discoverable tool failure must move through a new evidenced hypothesis or an
accepted-outcome-preserving reversible strategy rather than repeat or ask the
operator to choose a tactic. Missing product intent or a public contract still
blocks for a well-framed operator decision. Retry counts alone prove neither
case.
