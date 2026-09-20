# Model and harness upgrades

<!-- Split out of docs/adoption.md: the maintenance path for a new model,
     harness release, permission profile, or material instruction revision. -->

**A model change is qualified, never assumed.**

Do not promote a new production model, harness release, permission profile, or
material instruction rewrite from a single successful task. Run
`/cf-evaluate-model` from the orchestrated maintenance flow (ADR-0027); each
step below produces the evidence the next one depends on.

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

Diagnostic packs help isolate failures but never qualify a binding. Use the
skill's marker- and run-ID-gated cleanup for fixtures; never use it against the
consuming project itself.

## Why the boundary sits here

CodeFlow separates stable method from changing bindings (ADR-0039):

```text
durable doctrine
  -> universal harness capability contract
    -> approved concrete binding evidence
      -> managed current ensemble
        -> project selection by qualified binding ID
```

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
tracking.

## What each kind of change touches

| Change | Update | Re-prove | Do not add |
|---|---|---|---|
| New managed model/version or effort policy on a supported harness | Promoted binding evidence, then `current-ensemble.json` | Controlled full native evaluation with requested/observed identity | Doctrine rewrites or automatic routing |
| Consuming-project model choice | `.codeflow/model-selection.json` references an approved local binding ID for an exact stable role | `codeflow doctor --check model-bindings` plus normal duo canary | Raw selectors, partial fallback, or same-lineage pseudo-duos |
| Material harness release/configuration change | Capability evidence only if the contract changed; refresh the concrete binding | Native capability canary plus full binding qualification where behavior or settings changed | An inferred pass from `--version` alone |
| Genuinely new harness/provider | One catalog entry and the smallest reviewed transport/probe seam actually required | Every universal capability, then each production binding | Generic plugin machinery, arbitrary catalog commands, or speculative providers |
| Durable orchestration duty change | Doctrine/quality/routing contract and linked requirement/cases | Regression and over-trigger cases plus both primary judgments | Binding facts duplicated through many skills |
| Harness/model retirement | Remove its ensemble use and catalog support when no retained binding needs it | Graceful-degradation and remaining-ensemble canaries | Harness-internal worker tracking |
| Diagnostic case grouping | `packs.json` only | Pack resolution and underlying unchanged cases | A promotion shortcut |

This division keeps model-family upgrades localized while making a new harness
earn the guarantees CodeFlow depends on. The evaluator and human approval
promote evidence; neither the catalog, doctor, nor current ensemble promotes
anything automatically.
