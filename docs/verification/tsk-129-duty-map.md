# Reading budget split: duty map

Status: evidence for TSK-129 AC-3. Every duty in the files TSK-129 split has
one home after the split. A duty is kept in place, moved with a pointer from
its old place, or deduplicated: the copy that stays is its one home and the
other place points to it. No duty was deleted.

Paths below are under `assets/base/`. `orchestrator/` means
`agents/skills/cf-model-orchestrator/resources/`, and `delegate/` means
`claude/skills/cf-delegate/`.

## Turn adapter

| Duty | Home after the split | How |
|---|---|---|
| The whole delegated Claude lifecycle adapter | `delegate/resources/claude-turn-completion.md` | kept; its opening now says a Claude host does not load it |
| Load the adapter before a Claude worker launch | `orchestrator/../SKILL.md`, `orchestrator/routing/effort.md`, `delegate/resources/lane-lifecycle.md` | kept, scoped to the Codex host |
| Claude-host worker return (own-launch task notification) | `orchestrator/routing/effort.md` | kept |

## cf-delegate

| Original section | Home after the split | How |
|---|---|---|
| Description, intro, consult, delegate, or neither | `delegate/SKILL.md` | kept |
| Transport: Claude Code to codex, prohibited transports, fallback | `delegate/SKILL.md` | kept |
| Transport: codex to claude host, Herdr and canary detail | `delegate/resources/lane-lifecycle.md`, Preflight | moved; the core names the lane |
| Preflight: shared rules, unavailable seat, never automate auth | `delegate/SKILL.md` | kept |
| Preflight: from Claude Code | `delegate/resources/lane-plugin.md`, Preflight | moved with a pointer |
| Preflight: from codex | `delegate/resources/lane-lifecycle.md`, Preflight | moved with a pointer |
| Lane 1, from Claude Code through the plugin | `delegate/resources/lane-plugin.md` | moved; the core's lane list points to it |
| Lane 2, the durable lifecycle over the interactive claude CLI | `delegate/resources/lane-lifecycle.md` | moved; the core's lane list points to it |
| Evidence contract, five obligations | `orchestrator/routing/evidence.md` | deduplicated; the core points to it |
| Evidence contract, forward lane specifics | `delegate/resources/lane-plugin.md`, Evidence on this lane | moved |
| Evidence contract, reverse lane specifics (provenance record) | `delegate/resources/lane-lifecycle.md`, Evidence on this lane | moved |
| Edit-access doctrine | `delegate/resources/edit-access.md` | moved; the core keeps a summary and pointer read before any write-enabled handoff |
| agy, Guardrails | `delegate/SKILL.md` | kept |

## Quality contract

`orchestrator/quality-contract.md` is now an index. Sections marked every
task are read on every task; the others are read when their trigger fires.

| Original section or paragraph | Home after the split | How |
|---|---|---|
| Portable contract preamble | `orchestrator/quality-contract.md` | kept |
| Versioned plan contract: record, approvals, task graph | `orchestrator/quality/plan.md` (every task) | moved |
| Plan contract: research and planning-only fields | `orchestrator/quality/research-planning.md` (trigger) | moved with a pointer from the plan section |
| Plan contract: `DESIGN_INTENT` rule and design exploration | `orchestrator/quality/ui-design.md` (trigger) | moved; the plan section keeps the N/A, conform and settled record paths and points to it |
| Design and implementation quality | `orchestrator/quality/design-implementation.md` (every task) | moved |
| Primary responsibility and actual execution | `orchestrator/quality/design-implementation.md` keeps the separation and authorship sentence; delegation and direct-execution detail lives in `orchestrator/routing/assignment.md` | deduplicated with a pointer |
| Claude design owner | `orchestrator/routing/design.md` | deduplicated with a pointer |
| Materiality and prioritization, remediation effort, review severity, out-of-scope observations | `orchestrator/quality/materiality.md` (every task) | moved |
| Design findings graded by materiality | `orchestrator/quality/ui-design.md` (trigger) | moved |
| Blocker navigation, gate redness classes | `orchestrator/quality/blockers-and-gates.md` (trigger) | moved; the completion gate points to it |
| A failing or missing gate cannot be overridden | `orchestrator/quality/blockers-and-gates.md` (trigger) | moved with the redness classes it defines |
| Parallel execution contract | `orchestrator/quality/parallel.md` (trigger) | moved |
| Evidence ledger: what to record, model agreement, evidence is not authority | `orchestrator/quality/evidence.md` (every task) | moved |
| Evidence ledger: execution, usage and route qualification evidence | `orchestrator/routing/assignment.md` and `orchestrator/routing/route-status.md` | deduplicated with a pointer |
| Responsible authority and data | `orchestrator/quality/authority.md` (every task) | moved |
| Catastrophic or irreversible action record | `orchestrator/quality/irreversible.md` (trigger) | moved with a pointer from the authority section and the completion gate |
| Required verification, journeys, doubles, layers, longitudinal review, selection | `orchestrator/quality/verification.md` (every task) | moved |
| Performance, scale and concurrency review | `orchestrator/quality/performance.md` (trigger) | moved with a pointer |
| Research and planning run verification | `orchestrator/quality/research-planning.md` (trigger) | moved |
| Record `UI: N/A` when no user-facing surface changed | `orchestrator/quality/verification.md` (every task) | moved so a task without UI reads it |
| Editorial quality and presentation | `orchestrator/quality/editorial.md` (trigger) | moved |
| Coverage | `orchestrator/quality/coverage.md` (every task) | moved |
| UI and design verification | `orchestrator/quality/ui-design.md` (trigger) | moved |
| Independent review: verification order and the judgment primary | `orchestrator/routing/review.md` | deduplicated with a pointer |
| Independent review: reviewer checks, mixed authorship, severity, security | `orchestrator/quality/review.md` (every task) | moved |
| Completion gate | `orchestrator/quality/completion.md` (every task) | moved |

## Capability routing

`orchestrator/capability-routing.md` is now an index in the same form.

| Original section or paragraph | Home after the split | How |
|---|---|---|
| Preamble | `orchestrator/capability-routing.md` | kept |
| Session roles | `orchestrator/routing/roles.md` (every task) | moved |
| Assignment record | `orchestrator/routing/assignment.md` (every task) | moved |
| Route status: readiness of a configured candidate, candidate advice | `orchestrator/routing/assignment.md`, Route readiness (every task) | moved |
| Route status: qualification, promotion and savings claims | `orchestrator/routing/route-status.md` (trigger) | moved with a pointer |
| Claude worker effort preflight and worker return | `orchestrator/routing/effort.md` (every task) | moved |
| Admissible cross-lineage evidence, five obligations | `orchestrator/routing/evidence.md` (every task) | moved; the one home cf-delegate points to |
| Native host routes | `orchestrator/routing/hosts.md` (every task) | moved |
| Claude design authority | `orchestrator/routing/design.md` (trigger) | moved with a pointer from the host routes |
| Review and degradation: verification order, judgment primary, degradation | `orchestrator/routing/review.md` (every task) | moved |
| Review: mixed authorship and discarded attempts | `orchestrator/quality/review.md` | deduplicated with a pointer |
| Review: design role and UI assignment | `orchestrator/routing/design.md` (trigger) | moved |

## Pins and eval markers

- Test pins now read the file that holds each duty. Where a test pins "the
  quality contract" or "capability routing" as a whole, it reads the index
  with every section file. No pin was deleted.
- Four pin texts changed: two routing pins to their dash-free wording, the
  unknown-usage pin to its one home in routing, and the cf-delegate evidence
  pin to the core pointer plus each lane's evidence section.
- In `cf-evaluate-model/resources/requirements.json`, the 40 sources that
  named the quality contract or capability routing (146 markers) now name
  the split file holding each marker. Marker text, statements and levels are
  unchanged. Three sources whose markers now sit in two files (CF-QA-002,
  CF-MM-017, CF-MM-011) became one source per file; each file's markers keep
  their original relative order.
