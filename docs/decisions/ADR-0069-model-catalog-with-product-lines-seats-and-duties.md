---
id: ADR-0069
uid: 89d5fdf0-3b29-43a3-b7b5-658e876b31db
title: "Model catalog with product lines, seats and duties"
status: accepted
date: 2026-09-23
supersedes: []
superseded_by: []
architecture_impact: "docs/architecture.md: catalog data, eligibility and pure duty resolution"
---

# Model catalog with product lines, seats and duties

## Context

The current ensemble binds each family to one concrete model and describes
fallbacks in prose. It cannot compute the latest eligible version or return
both participants owed by independent planning. EPC-018 Plan v3.4 replaces
that representation with a catalog while preserving qualification boundaries,
author-relative review and ADR-0056 effort floors.

The operator designated the intended roster on 2026-09-23. Designation is an
explicit assignment of a seat, not evidence that a native qualification suite
ran. TSK-079 built the engine and TSK-080 the command and its approved-plan
lookup. TSK-085 accepted this decision when it switched the managed catalog
to schema 5 with the roster below, accepted 2026-09-25.

## Decision

Keep the managed file name `current-ensemble.json`. Schema 5 describes
families, ordered product versions, seats and duties. The engine returns data
and gaps; it never launches a model, probes availability or reads a clock.

```text
family -> product line -> versions, oldest to newest
       -> primary seat -> eligible lines, preferred first

duty -> required participants + triggered participants
     -> each participant's ordered alternatives
     -> eligible pinned identity, or a named open participant
```

Figure: version order applies inside a line; alternative order applies
between routes. Every owed participant is resolved independently.

A family identifies its provider, lineage, supported harnesses, trusted probe
ids and usage bucket. Harness and probe ids must already be supported by
`harnesses.json` and the code allowlist; catalog data cannot supply commands.
There is one primary seat per family. Two standing seats must have distinct
lineages, and the design owner is the standing Claude seat.

A version records an alias for discovery, a pinned launch id, selectors per
harness, supported efforts and lifecycle: `active`, `fallback-only` or
`retired`. Designations contain the seat, date and instruction record.
Scoped evidence separately names its exact harness, selector, effort and
duty, with its record and evidence paths. Qualification starts empty for
the designated roster; a follow-up adds actual evidence without relabeling
the designation. An approved full-suite binding remains the ADR-0039
record, including native observations and human approval.

The validator rejects unknown fields and duplicate JSON keys, unsupported
harnesses or probes, collapsed standing lineages, a non-Claude design owner,
multiple primary seats in a family, and a listed seat line without any
potentially eligible version. A retired version cannot carry a designation;
a designation must name a seat that lists its line. Missing designation or
evidence records and defaults below duty floors are errors.

### Eligibility and selection

A seat needs its designation or an approved full-suite binding for the exact
role, harness, selector and effort. Scoped worker evidence never satisfies a
seat. Designated output is labeled "designated, full suite not run" unless
the exact full qualification exists.

Candidate workers may perform the bounded, light, evidence, advisory
reasoning, consultation and non-design execution work allowed by ADR-0060.
Design implementation requires scoped evidence for the exact tuple. A worker
never gains direction or fidelity approval.

The participant's effort must be supported; unsupported effort excludes that
version instead of clamping it. Retired versions are always ineligible.
A fallback-only version serves only on a listed seat fallback. A fresh native
exclusion removes the affected version or harness; exhaustion of a usage
bucket removes every line using it. Unknown usage excludes nothing. A stale
Codex plugin login is a native exclusion; the qualified Herdr interactive
Codex seat remains the fallback transport allowed by ADR-0059.

Resolve alternatives in their declared order, then versions newest first.
Each line has an adoption policy and an `adopted_version` reference. Manual
worker selection uses that reference; `workers` adoption admits newer
eligible candidate versions automatically. Seat selection always needs
designation or exact full qualification, regardless of worker adoption.
An overlay addition stays inert on a manual line because the overlay cannot
change its `adopted_version`. A line with `workers` adoption may select that
addition for a candidate-eligible worker duty. Q1 remains open with the
operator; the seed keeps manual adoption.

### Duties and effort

A duty carries required participants and triggered participants. Each
participant has ordered seat or line alternatives, an author-lineage relation
and entry effort. Trigger ids come from the routing policy or an explicitly
named policy's list of facts. Policy is data, not an expression language.

Independent planning and body review owe both standing seats. Unit review
requires the opposite lineage: the Codex seat reviews Claude authorship; the
Claude seat reviews Codex authorship; Grok authorship uses Codex, then Claude.
The Grok participant is added on its review trigger when Grok is not the
author. A second opinion remains labeled as such and cannot close an
independent-review gap. An unfilled optional second opinion is retained as
a labeled diagnostic but does not make `is_open()` true; an unfilled required
participant still leaves the duty open.

Reasoning support excludes the version currently serving the owning seat.
Computer-use QA selects the opposite lineage's capable harness: Codex on
`codex-app`, or Claude on `claude-code`. Grok-authored UI uses those
alternatives in that order. Test authoring belongs to the unit's executing
participant and is not resolved separately.

ADR-0056 is unchanged. Seats enter at high; bounded workers may enter at
medium. A high trigger raises a worker's entry effort from medium to high,
subject to the version supporting high; it does not raise the seat's entry
effort. An xhigh trigger adds an `xhigh-reasoning` obligation at xhigh in
the seat's family, potentially using the same version in a separate worker
session. The seat stays at high and retains approval. A seat lacking xhigh
remains eligible. No eligible same-family worker leaves the obligation open
and the trigger explicitly unmet.

### Launch identity and design authority

Every launch uses the resolved version's pinned id. Aliases are used only
by the identity canary to discover newer versions. An observed mismatch
inherits neither designation nor scoped or full-suite evidence. Resolution
tries the next eligible alternative; only a candidate-eligible worker duty
may then use the observed model as a candidate, with drift recorded.
Otherwise the participant remains open. An unobservable native identity is
an explicit limitation.

The Claude design owner's first line owns direction, design production and
fidelity approval. With that line ineligible, design stays open. Fable may
hold the seat as a recorded fallback for orchestration, planning and review
with reduced assurance; without a matching operator override it never
authors design or gives design or fidelity approval.

The only exception is an `OPERATOR_OVERRIDE` in the approved Plan vN task
record. It names the exact task, design duty, route including harness,
effort, plan version and location of the operator instruction. TSK-080 reads
that record as committed on the task's integration target using the planning
anchor rule; arbitrary caller strings and working-tree records confer no
authority. The engine takes `requested_override` as a separate input containing
the invocation route and effort, in addition to the typed `operator_override`
record. TSK-080 must supply both and match the record against the requested
task, duty, route and effort. A missing invocation route or a mismatched
record leaves design open even when the ordinary first line is eligible.
A record naming a non-design duty is rejected: design resolution returns
an open gap for the duty mismatch, and an override supplied to a non-design
resolution returns an explicit error instead of being ignored. Approval of
the plan supplies authority; this does not independently authenticate the
operator.

A matching override replaces only the ordinary first-line restriction for
that task. It may name any line listed by a catalog seat, including the
Claude seat's second line or another family's seat line. Seat eligibility,
supported effort at or above high, exclusions and pinned identity still
apply. The override effort is checked explicitly against the design duty's
floor before ordinary seat eligibility is checked. Missing or mismatched
fields, retirement, unsupported effort, an
exclusion or identity drift leave design open. The record grants nothing
for another task or a non-design duty. Workers never receive direction or
fidelity approval through this exception.

### Local and project selection

The optional personal file `~/.codeflow/model-catalog.local.json` may add
candidate versions to existing lines or exclude versions. It cannot add a
family, harness or probe, change seat or duty policy, designate a version,
or claim evidence. Any invalid entry rejects the whole overlay.

The project file `.codeflow/model-selection.json` stays at schema 1 under
ADR-0041. It references approved binding ids for exact roles and harnesses;
a reference cannot transfer qualification to another effort or harness.

Project schema 2 is deferred until a consuming project needs it. It may
eventually narrow eligible choices by reordering approved alternatives,
excluding a version, pinning an eligible version or choosing a supported
effort at or above its floor. It must never introduce raw selectors or
commands, add a family, harness or probe, confer designation or evidence,
collapse independent lineages, lower effort floors, transfer tuple evidence,
override native exclusions or drift, or bypass task-specific design
authority.

The model `grok-4.7-build-fast` is known and not routed.

### Roster

The managed catalog carries this roster. The operator designated it for its
seats on 2026-09-23, in the EPC-018 Q2 answer; each designation record
cites that answer. Every version's qualification field is empty; a
follow-up that TSK-078 files adds evidence per seat version as its native
suite passes.

| Family | Line | Version and pinned id | Lifecycle | Seat use and designation |
|---|---|---|---|---|
| Claude | `opus` | Opus 5.5, `claude-opus-5-5` | active | `claude-primary` first line; designated 2026-09-23 |
| Claude | `fable` | Fable 5.1, `claude-fable-5-1` | active | `claude-primary` second line, a fallback for orchestration, planning and review; design only by OPERATOR_OVERRIDE; designated 2026-09-23 |
| Codex | `astra` | GPT-6 Astra, `gpt-6-astra` | active | `codex-primary` first line; designated 2026-09-23 |
| Codex | `sol` | GPT-6 Sol, `gpt-6-sol` | active | `codex-primary` second line; designated 2026-09-23 |
| Codex | `sol` | GPT-5.6 Sol, `gpt-5.6-sol` | fallback-only | `codex-primary` last fallback; designated 2026-09-23 |
| Codex | `luna` | GPT-6 Luna, `gpt-6-luna` | active | light execution only, medium effort; none |
| Codex | `terra` | GPT-5.6 Terra, `gpt-5.6-terra` | retired | none |
| Grok | `grok` | Grok 4.7, `grok-4.7` | active | `grok-primary`; designated 2026-09-23 |
| Grok | `grok` | Grok 4.6, `grok-4.6` | retired | none |

Adoption is `manual` on every line (Q1 open). The Grok trigger policy is the
named `extra-family-review` policy, equal to today's `routing-policy.json`
triggers (Q4 open). A later roster change is a catalog data edit that this
table does not track.

### Identity canary record

`~/.codeflow/model-canary.json` records identity canary observations on one
machine. Schema 1 is strict JSON with exactly two fields: `schema_version`
(1) and `observed_ids`, a map from a pinned id to the observed id, both
nonempty. It is diagnostic only: `codeflow doctor --check model-bindings`
warns about a recorded mismatch, and resolution never reads it, because a
recorded observation may be stale; launchers pass fresh `--observed` facts.
The identity canary that TSK-078 runs produces it, and so do later canaries.

### Derived scans and diagnostics

Two scans read their tokens from the catalog: every version's alias, pinned
id and selectors. No such token may appear in an operative instruction
surface (skills and their resources, the contract and Claude templates,
reviewer agents and managed workflow examples), and no retired version's
token may appear outside history (`docs/decisions/`, `docs/verification/`,
`project-management/`, `docs/plan/`, `CHANGELOG.md`). Excluded exactly: the
catalog and its four mirrors, and the evaluation kit fixture files, which
use fictional names. The `Claude Fable 5` attribution trailers in the
commit-policy fixtures (`standards.rs`, `git_hook.rs`, `ci.rs`) and
`"model": "opus"` in the settings tests preserve historical behaviour, are
not routing, and sit outside both scans.

Doctor takes the user's CodeFlow home and the binding-record directory as
two explicit options; neither is derived from the other. It keeps the
harness-version and declared-settings drift checks for binding records, and
a drifted record that the project selection uses fails.

## Consequences and verification

A roster change becomes a catalog edit validated against the same eligibility
rules. Model names stay in catalog data and historical records; rules and
duty definitions refer to families, seats and lines. Configuration can
represent a gap explicitly instead of silently substituting a weaker
participant.

TSK-079 uses fictional catalogs to test every validator boundary, participant
set, effort obligation, drift case, override match and overlay restriction.
It also tests schema 1 binding references against schema 5. TSK-085 removed
the transitional schema 4 reader when it switched the managed catalog; a
schema 4 file is now rejected. TSK-085's tests derive every structural
check from the managed catalog and name no model. Native identity canaries
and final qualification are later tasks in EPC-018; these tests do not claim
their evidence.

The general-review fictional fixture intentionally lists only the Grok and
Claude seats as independent alternatives. For Claude-authored work, excluding
Grok therefore leaves that participant open while the same-lineage second
opinion remains advisory. The managed catalog applies the epic's broader
rule: its independent alternatives are the Grok, Codex and Claude seats in
that order, filtered by opposite lineage, so for Claude-authored work with
Grok excluded the Codex seat fills the gap.

## Note (2026-10-02)

The operator set the roster for the 3.0.x line on 2026-10-02. Opus 5.5
stays the preferred Claude primary. Sonnet 5.5 is added for execution and
routine work where Opus is not needed. Fable 5.1 is for design,
architecture, technical planning and consultation. GPT-6 Astra stays,
GPT-6.1 Sol is the Sol version to use, GPT-6 Luna serves low-level tasks,
and Grok 4.7 is unchanged. TSK-204 applies it to the managed catalog as a
data change:

| Family | Line | Version and pinned id | Lifecycle | Seat use and designation |
|---|---|---|---|---|
| Claude | `sonnet` | Sonnet 5.5, `claude-sonnet-5-5` | active | worker line, medium and high; first Claude alternative for `bounded-execution`, `evidence-collection` and the Claude fallback of `engineering-implementation`, ahead of Opus; none |
| Codex | `sol` | GPT-6.1 Sol, `gpt-6.1-sol` | active | adopted; `codex-primary` second line; designated 2026-10-02 |
| Codex | `sol` | GPT-6 Sol, `gpt-6-sol` | fallback-only | `codex-primary` fallback; designated 2026-09-23 |

The other rows of the roster table stand. Sonnet holds no seat, design,
review or orchestration duty. Every new version's qualification field is
empty.

The Fable part of the direction (design, architecture and technical
planning) is not applied. `design`, `independent-plan` design-primary and
`body-review` design-primary resolve to the `claude-primary` seat, Opus
first and Fable second, as before. Only `consultation` and
`reasoning-support` route to Fable first, as they already did.

No catalog-only route exists. In `crates/codeflow-core/src/model_catalog`:

- Validation accepts only the design owner seat as a `design` target
  (`validate.rs:333-335`), and resolution fills `design` only from that
  seat's first line (`resolve.rs:228-232`); a line with no seat never holds
  direction or fidelity approval (`resolve.rs:233-238`).
- A line target on `independent-plan` or `body-review` is a worker, and
  outside the candidate duties (`resolve.rs:168-181`) a worker needs scoped
  qualification evidence for the exact tuple (`resolve.rs:291-295`), so
  Fable there stays open.
- The seat order cannot change instead: there are exactly two standing
  seats (`validate.rs:154`), the design owner must be the standing Claude
  seat (`validate.rs:171-176`), and the host seat is the first seat of the
  host family (`resolve.rs:395-401`). Putting Fable first in
  `claude-primary` would make Fable orchestrate too.

Moving design, architecture and technical planning to Fable while Opus
keeps orchestration is an engine change and a change to "Launch identity
and design authority" above. It was offered to the operator on 2026-10-02
as their decision. Until then the Fable row above stands.

## Note (2026-10-04)

The operator decided issue 43 on 2026-10-04 and TSK-236 applies it. This
narrows the project schema 2 "never" list in "Local and project
selection" and answers the 2026-10-02 offer above for adopting projects.

- D1, yes, same family only: a project may name another line of the design
  owner seat (for example Fable after Opus) as a standing design co-owner
  in `.codeflow/model-selection.json` schema 2 `design_authority`. Another
  family still needs the task-specific `OPERATOR_OVERRIDE`, which is
  unchanged.
- D2, yes: a separate opt-in `design-approval` duty, held by the co-owner
  (consultants advisory), recorded apart from the cross-family review and
  never counted as the independent review.
- D3: standing extra-family reviews live in schema 2 `standing_reviews` and
  add the catalog's own extra-family participant to `unit-review` or
  `body-review` resolved with `--area`; `routing-policy.json` is unchanged.
- D4: the "fallback, reduced assurance" label is dropped only for a line
  the project designated, and only on `design` and `design-approval`;
  elsewhere the later line stays a fallback.
- D5: applying this to CodeFlow's own `model-selection.json` is a later
  separate task.

The protections hold. The owner must be the seat's first line, so
authority never follows list order silently, and every fallback stays
labelled. The block confers authority only as committed at the merge-base
of `HEAD` and the task's integration target, read the same way as the
override; a working-tree copy, a task-branch commit and a caller string
confer none. The block names only lines the catalog's design owner seat
already lists, in that seat's family, each with an active version
designated or fully qualified for the seat, so repository content still
cannot add a selector, command, family, harness or designation. A
same-family approval never fills the independent review, and a standing
review adds a named, labelled participant, so an extra family never votes
silently. With no file or a schema 1 file every duty resolves exactly as
before.
