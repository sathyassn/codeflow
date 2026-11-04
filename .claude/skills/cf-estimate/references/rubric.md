# Evidence-anchored delivery grades

Optional. Read when anchored grades would help the estimate; scenarios from
the operating model stand without them.

Method/rubric version: `codeflow-agentic-1`. This is a provisional classification
instrument, not a validated speed model. It adapts the supplied Agentic
Development Model at commit `87852d69170fad6b15bf2463841c74ef80ceda58` with the
repairs below. Preserve source provenance when a project versions its adaptation.
Weights and cuts are candidate defaults; local predictive value remains unproven.

## Define the package before scoring

A package is an accepted outcome owned and independently verifiable at the
project's existing work boundary, not a file count or a prompt. Link its existing
task/spec and pin complete source bytes. Do not split work to reduce a score.
Record anchors (path and useful line span or equivalent immutable source) for
each driver, including reasoned zeros, and distinguish required from conditional
scope. Driver disagreement is resolved against those anchors, not averaged away.

The available brief/spec must answer: identity and scope; ownership of touched
surfaces; behavior/states/refusals; operations/events; persistent shapes and
migrations or reasoned none; state-change/irreversibility; own versus consumed
security duties; falsifiable acceptance; and material open questions. Existing
project artifacts may answer these together; do not demand another template.

Missing material scope, ownership, existence or core acceptance yields **UNSIZED**
with a named gap and bounded discovery action. Never silently assume ownership
or score missing knowledge as zero. A question changing only internal placement
need not block sizing. Known conditional scope may have separately anchored
base/alternative grades, but cannot become an unconditional commitment. A named
start-gating decision is recorded as blocking; unrelated uncertainty is not.
If acceptance is defined but the technique must be found experimentally, size
the experimental obligation through T rather than inventing an acceptance result.

## Drivers

Count a surface only when the package owns the changed behavior/proof. Consuming
another component's capability does not own that component. Its integration,
conformance testing and operational cost still belong in the duration estimate.

| Driver | Anchored question | Points and boundary |
|---|---|---|
| S1: persisted shape | Owns a new/changed entity, table, index, migration or durable projection? | 3; ordinary audit/telemetry alone does not qualify |
| S2: contract | Owns at least one operation, event or contract member? | 1, plus 1 if at least three operations, any mutation, or an emitted lifecycle event; consumed contracts do not count |
| S3: interface | Owns a screen, panel or multi-state human workflow? | 3; one control on a surface owned elsewhere or ordinary developer documentation does not qualify |
| S4: computation | Owns a computation such as geometry, graph, ranking or generated output with golden/tolerance/determinism proof? | 3; tests alone, or lookup/ordering/clamping recorded state, do not create owned computation |
| S5: AI behavior | Owns model behavior accepted through grounding, refusal or evaluation evidence? | 3; agent access to an ordinary API alone does not qualify |
| S6: external conformance | Owns an external format, protocol or integration with conformance/loss proof? | 1; merely exposing its native shape is insufficient |
| S7: operability | Owns replay, reconciliation, drift detection, restore or environment evidence? | 1; ordinary telemetry emission is insufficient |
| C: consequential act | Performs audience publication/activation, destructive erasure, custody/authority change or stored-data migration? | 2; consumed/read-only behavior or non-production restoration alone does not qualify |
| X: specific security mechanic | Implements a named approval, revocation, disclosure or erasure mechanism on its own path? | 1; boilerplate security applicability is not a mechanic |
| T: experimental technique | Defined acceptance requires establishing technique/model/tolerance/threshold by experiment during delivery? | 2; uncertainty about an unapproved budget or an evidence disclaimer alone is insufficient |
| R: distinct refusal/safeguard obligations | How many semantically distinct, explicit acceptance obligations govern refusal or safeguarding? | 0 for at most 3; 1 for 4 to 7; 2 for 8 or more |

For S2, a verb, flag, policy or configuration key, record field or report
field that a reader depends on is a contract member; an operation that writes
a record or state is a mutation; a rename is neither. A fix that changes an
owned operation's behaviour scores S2, because its owner proves the
operation again; a fix inside a test, a fixture or wording does not.

For R, enumerate each obligation and its anchor. Count semantics, not headings:
the same required behavior counts whether phrased as an error, safeguard, edge
case or invariant. Merge synonyms and repeated tests of the same obligation;
do not count every test assertion or vague quality checklist item. Renaming a
heading must not change the grade. Distinct independently fail-able obligations
may count separately even if one exception type reports them all.

Sum S1–S7, C, X, T and R (maximum 23). Candidate cuts:

| Points | Grade |
|---|---|
| 0–1 | Easy |
| 2–4 | Medium |
| 5–8 | Hard |
| 9–23 | Very Hard |

Always explain multiplicity, data/traffic scale, algorithmic burden, coupling,
integration and proof cost. Boolean surface points can hide large differences.
If the driver set cannot describe a material domain, mark it UNSIZED pending a
versioned adaptation, not arbitrary bonus points. When the score is sound but a
local time band is a poor fit, retain the score and justify an analogue or direct
duration estimate; do not distort the grade to obtain the desired date.

Guards, hooks and policy: each distinct refusal a guard owes counts under R.
X is only a named approval, revocation, disclosure or erasure mechanism on
the task's own path. A recorded approval that the path enforces counts, such
as a pinned table of reviewed digests or a required operator confirmation;
a refusal rule, a choice of policy authority or an exemption class does
not, and a drift check is S7. A guard change with none of these may grade
low by points; its security review set is a reason to move the band, never
to add points.

Boundaries the reliability check settled:

- S4: counting or exact lookup over recorded text is not S4; a matcher that
  normalizes, or a graph walk, proved by a golden case table, is S4.
- S5: counts for the package that owns the model behaviour or the graded
  runs that accept it; a package that builds evaluation tooling whose live
  runs belong to another task scores the tooling (a case kind under S2,
  environment evidence under S7), not S5.
- R: a regression check that existing behaviour stays unchanged is not a
  refusal or safeguard obligation.
- C and X: a sanctioned edit that git history keeps is not destructive
  erasure; a redaction route is X. When a package performs a custody change
  and builds the approval that grants it, C and X both apply.
- H: a required recorded review, waiver or operator confirmation is a named
  human approval path.

Never counts, in one list: tests alone; consumed APIs, platforms and
dependencies; ordinary documentation, help wording and telemetry;
boilerplate security applicability; one control on a surface owned
elsewhere; lookup, ordering or clamping of recorded state; and duration
uncertainty.

A disputed point at a cut (1 or 2, 4 or 5, 8 or 9) is settled against the
pinned anchors below. Where the anchors do not decide because the rubric
text is ambiguous, record the ambiguity and amend the rubric by a versioned
adaptation. Never move a band to settle a grade dispute; missing scope is
UNSIZED, not a tie.

Under an adopted time-box profile (operating-model.md), the planner may move
a task's band one step from its grade, never two, for a reason the drivers
miss: proof volume, record volume or a key review set. The reason is stated
apart from the score and recorded in the task's Estimate block; the points
stay as scored.

## Consequence and use

Flag human/authority consequence (H) when the package has a named human approval
path, C applies, or it introduces an authority/role/grant kind. Flag a named
start-gating decision (B) separately. Neither is a numeric productivity discount
or permission grant. An Easy security-sensitive package may still need deep
analysis, independent review and operator approval under existing doctrine.

Grades are initial local reference-class grouping keys and scope/proof summaries.
They are not universal hours, worker effort settings, legacy XS/S/M/L/XL values,
or acceptance gates. A project may retain its existing method; if grades do not
improve decisions over a simpler analogue/area/work-type grouping, stop using
them as a duration predictor rather than defending added complexity.
Under an adopted time-box profile, a grade maps to the profile's band: a
budget with a stop rule, not a prediction of the task's duration.

Note, 2026-11-04 (ADR-0096): the sentence above, the S2 definitions, the
guard anchors, the boundaries the reliability check settled, the
never-counts list, the dispute rule, the band move rule
and the anchored examples were added. Points and cuts are unchanged.

Before trusting a changed rubric, independently grade representative pinned
packages across the relevant drivers and ambiguous boundaries. Preserve both
initial judgments, disagreements and reconciled reasons. Choose meaningful
cases and acceptance rules before grading; neither a magic sample size nor
agreement on contrived fixtures establishes field accuracy.

## Anchored examples

From reliability check 1 (`project-management/estimates/rubric-check-1/` in
CodeFlow's repository, records pinned at `47403d356`): 22 landed CodeFlow
tasks graded independently by two Claude graders, 18 agreeing on the grade,
every difference reconciled; one model family, so reduced assurance. Each
line is the reconciled grade, which both graders gave, pinned at
`<task id>@47403d356`.

| Grade | Task and outcome | Drivers |
|---|---|---|
| Easy | TSK-1018, test fixtures resolve their root on macOS | none, 0: tests alone |
| Easy | TSK-228, `codeflow ci` warns on a long PR body | S2 1 (one report member); counting is not S4; R 0 (three obligations) |
| Medium | TSK-212, local `ci` matches the hosted verdict (a fix) | S2 2 (three owned operations change), R 2 (nine refusals): 4 |
| Medium | TSK-1149, a task branch pushes its own line sync | S2 1, R 1 (six refusals); X 0, a guard relaxation is R: 2 |
| Hard | TSK-239, outcomes derived from git | S2 1, S4 3 (timings, ratios, medians), R 1: 5 |
| Hard | TSK-322, blocked names refused at any depth | S2 2 (three policy keys), S4 3 (normalizing matcher, remerge walk), R 2: 7 |
| Very Hard | TSK-119, form answers stored against revisions | S1 3, S2 2, S3 3, R 2: 10 |
| Very Hard | TSK-019, portal ownership with schema migration | S1 3, S2 2, S7 1, C 2, X 1, R 2: 11; C scores the custody change and X its confirmation, as the C and X boundary above decides |
