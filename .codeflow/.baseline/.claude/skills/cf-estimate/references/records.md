# Adoption, snapshots and the read-only checker

Read when creating durable records or using the checker. Existing project work
records stay authoritative. Calculation snapshots reference them; they never
copy task status, acceptance or package dependency lists into another tracker.
An estimate answer need not create any files.

## Adopt only after confirmation

Before adoption, keep previews in the response or task-owned temporary scratch.
Project-owned profiles, forecasts and adoption records require confirmation even
when untracked or labelled draft/proposed. A checker needing pinned bytes does
not grant permission to create an estimate home: use existing project sources
with a temporary forecast, or disclose what cannot yet be checked. A material
re-offer event permits a new offer, not persistent records or automatic adoption.

The project-owned `.codeflow/estimate.json` records the decision:

```json
{
  "schema_version": 1,
  "status": "adopted",
  "rationale": "Use explicit agent-delivery forecasts for capacity decisions.",
  "method_version": "codeflow-agentic-1",
  "root": "project-management/estimates",
  "re_offer_when": "The delivery boundary or existing planning authority changes."
}
```

For `declined`, preserve the reason and a material event that would warrant a
new offer; `root` may be null. No elapsed-time nag. This file is absent from the
scaffold manifest; neither init/update nor the checker writes it. A skill writes
it only with adoption/decline confirmation. Existing compatible adoption is
reused, not repeatedly re-authorized. Malformed/unknown adoption metadata is a
question to resolve safely, not license to assume adoption.

Full-tier default home is `project-management/estimates/`. Standard/external
methods keep their existing planning home. Minimal does not receive the method
or a new workgraph; an explicit explanatory request does not force a tier upgrade.
Project profiles, forecasts and outcomes remain project-owned, including during
updates. Naming below is illustrative, not another global record allocator:

```text
<chosen estimate home>/
  profile-v1.md          execution boundary and evidence comparability
  rubric-v1.md           pinned local rubric, with adaptation provenance
  forecasts/<name>/v1.json
  forecasts/<name>/v2.json
  outcomes/<name>.md     every started outcome, linked to frozen predictions
```

A profile states what constitutes full delivery, actual resources/availability,
qualified binding references, evidence comparability, any justified local grade
ranges, and prospective evaluation rules. Do not seed invented calibration data.
An outcome record names the forecast and package, observed scope/status and stage
timings, waits, failures/reopens, sources and missing observations. Its timings
may come from `codeflow estimate outcomes`, which derives them from git and
writes no record. Revisions link
their predecessor and reason in the project's accompanying record; the checker
JSON is closed and does not acquire arbitrary history fields.

## Checker contract

`codeflow estimate check <forecast-path> [--json]` reads explicit inputs and
returns exit 0 for no findings or exit 1 for findings/unreadable input. It creates
no adoption/work records, repository directories or user registry entries. It
executes no project code, subprocesses or models, and uses no network. It does
not infer durations, grade, optimize, schedule, grant permission or change status.

JSON objects are closed at every level: unknown/duplicate fields, unsupported
versions, invalid enums and duplicate IDs fail. All fields below are required
unless explicitly nullable; arrays may be empty only where the contract permits.
Integers are unsigned; duration/capacity/demand units are positive.

| Object | Fields |
|---|---|
| Forecast | `schema_version: 1`, `id`, `anchor_epoch_seconds`, `horizon_seconds`, `profile`, `rubric`, `packages`, `boundaries`, `resources`, `scenarios` |
| Pin | `path`, `sha256` (64 lowercase hex; full file bytes) |
| Package | `id`, `source`, `context_pins`, `grade`, `grade_evidence`, `duration_basis`, `stage_exclusions` |
| Canonical source | `kind: "codeflow_task"`, `task_id`, `sha256` |
| External/declared source | `kind: "external"` or `"declared"`, `pin`, `reference` (opaque text, never fetched) |
| Exclusion | `stage`, `reason` |
| Boundary | `id`, `source`, `evidence`, `availability` |
| Availability row | `scenario`, `available_at_seconds`; exactly one row per scenario |
| Resource | `id`, `capacity`, `windows` |
| Window | `start_seconds`, `end_seconds` |
| Scenario | `name`, `assumptions`, `activities`, `milestones` |
| Activity | `id`, `package_id` (required, may be null), `stage`, `start_seconds`, `duration_seconds`, `demands`, `after`, `after_boundaries`, `basis` |
| Demand | `resource_id`, `units` |
| Milestone | `id`, `after` (nonempty activity IDs) |

Profile/rubric use Pins. Source pins include complete Markdown acceptance, not
just frontmatter. Canonical packages pin directly linked and epic-inherited
specs in `context_pins`. Other material context belongs there as well; the checker
cannot know every relevant document. Historical pins stay frozen when sources
change; a current comparison may then correctly fail rather than rewrite history.

Grades: `easy|medium|hard|very_hard|unsized`; duration basis:
`judgment|analogue|probe`. Grade evidence, boundary evidence, scenario assumptions
and activity basis must explain the actual anchors/assumptions. Scenario names
are exactly `favorable`, `planning`, `adverse`, once each. Stage names:
`discovery|implement|review|verify|rework|integrate|human|release`.

Demands are nonempty. Activity predecessor arrays may be empty. Every reference
resolves uniquely. A package covers implement/review/verify/rework in every
scenario or supplies a surfaced stage exclusion. The checker exposes exclusions;
it never approves their adequacy. UNSIZED may allocate bounded discovery with
explicit exclusions, but cannot pass an unconditional implementation allocation.
Integration, human decisions and release are reviewed at whole-forecast level,
not automatically demanded once per package.

## Source and allocation semantics

Canonical dependencies are read from pinned task records, including supported
legacy layouts. Each direct predecessor must be another scheduled package or a
pinned boundary outside scope. Do not chase indefinitely beyond a boundary; a
boundary depending directly on scheduled scope is contradictory. Availability
requires declared evidence, even when a task is marked complete.

Activity edges describe within-package sequencing or whole-forecast coordination.
An edge between distinct non-null packages is invalid if either is canonical;
use canonical package dependencies. Two external/declared packages may use such
edges, with limited assurance about their external authority. Links involving a
null-package activity are valid whole-forecast coordination, not a copied workgraph.

Each activity graph is acyclic and its dependencies finish before its start.
A canonical predecessor package's latest finish, or its boundary availability,
must precede the dependent package's earliest start. Missing allocation fails.
An activity fits in one contiguous finite window of every resource it demands.
Windows are sorted, non-overlapping and half-open; finish events free capacity
before starts at the same instant. Over-capacity overlaps or crossing a gap fail.
A milestone is the latest finish of its named activities, not a release permit.

Epoch/offset/duration and resource-product/sum arithmetic are checked for overflow.
Resource-seconds mean units multiplied by working seconds; elapsed seconds are
not their sum when work overlaps. `elapsed_seconds` runs from the forecast anchor
to the latest activity finish, including initial delay and intervening waiting
represented by offsets/boundaries; it is not a sum of active work durations.
No optimizer, recurring-calendar assumption or percentile arithmetic is hidden
inside a green check.

## Bounds and safe input

Limits are parser/work bounds, not project sizing advice: 1 MiB forecast,
1 MiB per source, 16 MiB total source bytes including identity discovery; 256 packages and
boundaries combined, 64 resources, 256 windows/resource, 1024 activities/scenario,
4096 activity and boundary edges/scenario, 256 total context pins; horizon at
most 366 days. IDs are bounded ASCII identifiers; explanations and reference
strings are bounded nonempty text. Do not embed source contents or credentials
in diagnostic fields to work around file restrictions.
Canonical work-record discovery is also bounded to 16,384 visited directory
entries; exceeding the bound reports a finding rather than silently truncating
the identity inventory or weakening source assurance.
Exceeding a source-byte or inventory bound leaves the forecast unvalidated; it
does not establish that delivery is infeasible.

Pinned sources must be project-relative safe regular files. The explicitly
selected forecast file may have an absolute path; its reads are bounded and
secret-file paths are rejected. For pins, traversal, linked/reparse
escapes, credential locations, secret file patterns, unsafe paths and oversized
content fail before unbounded work. Reports redact source contents and unsafe
parser payloads. Keep provider secrets out of profiles, snapshots and outcomes.
Pin traversal rejects symlink/reparse redirection, including ancestor replacement;
it is not an OS sandbox or proof that file bytes are unaliased by hard links or
mount arrangements. Use an appropriately isolated filesystem for untrusted work.
On Windows, directory handles temporarily prevent ancestor rename/deletion for
the duration of the bounded check; that transient sharing constraint is not a
persistent repository write.

## Interpret the report honestly

The independently versioned JSON report identifies input/source digests, checked
package count, source assurance, exclusions, scenario validity and computed
resource/elapsed/milestone results, plus deterministic findings with locations.
An invalid scenario has no ordinary valid total. External or declared sources
have limited assurance even if their local pins match; a green result never
claims the remote tracker or underlying assertions were verified.

Passing checks proves supplied data and allocation consistency. It does not
prove realistic durations, comprehensive acceptance, code quality, authorization,
actual resource availability, predictive usefulness or readiness to ship. Keep
the existing independent review and verification gates. Preserve every failed
trial and explain any fix; changing the inputs until arithmetic passes is not
evidence that the resulting forecast is realistic.
