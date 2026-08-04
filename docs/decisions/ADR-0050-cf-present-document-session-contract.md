---
id: ADR-0050
title: versioned cf-present document and session contract
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: add versioned presentation documents, immutable revisions, anchor selectors, and feedback delivery semantics
---

# ADR-0050 — versioned cf-present document and session contract

## Context

Presentation content crosses an agent-to-renderer trust boundary and feedback
must survive process interruption without becoming a second task or decision
authority. A silent partial render, guessed annotation anchor, duplicate model
delivery, or unbounded input would make the review record misleading. The
schema and state contracts therefore need to be public, closed, versioned, and
testable independently of the browser implementation.

## Decision

### Public schemas

CodeFlow ships canonical JSON Schemas and matching Rust types for three closed
contracts, each with a required integer `schema_version` starting at `1`:

- a presentation document envelope with title, optional language and durable
  provenance IDs, and an ordered block tree;
- optional project utility tokens limited to declared colour, typography,
  spacing, radius, and approved identity-asset values; and
- exported session history containing document revisions, feedback events, and
  delivery state, but never cookies, capabilities, local profile paths, service
  ports, or private review chrome.

Unknown fields, malformed discriminators, invalid IDs, duplicate block IDs,
invalid nesting, path-like identity assets, and values outside named limits are
rejected before a session starts. Limits are centralized constants and mirrored
in the schemas and tests; they cover encoded document bytes, block count,
nesting, prose/code/diagram/HTML lengths, table dimensions, media data, feedback
body, event response, and poll duration. The first concrete values are settled
from bounded implementation and adversarial fixtures rather than copied among
handlers.

An older renderer receiving a higher document version displays the complete
raw envelope as escaped, read-only text with an explicit unsupported-version
warning. It performs no partial typed rendering and exposes no mutation
controls. Unknown persisted state versions fail closed and remain untouched;
supported migrations are explicit, one-way, fixture-tested transformations.

### Block and revision model

The closed v1 block catalog covers narrative Markdown, bullets, callouts,
comparisons, decisions/questions, tables, status/evidence, code/diffs,
trees, diagrams/timelines/flows, embedded media, disclosures/tabs, structured
feedback, and one static sandboxed-HTML block. Every block has a unique stable
ID. Renderers may degrade an unsupported code language to escaped plain code,
but may not reinterpret an unknown block kind.

V1 media blocks carry only schema-allowed MIME and size-bounded bytes embedded
in the document envelope and, when exported, the standalone artifact.
Filesystem paths, `file:` references, remote URLs, and other external media
references are rejected by schema validation. Any future path-referencing media
requires a new confinement and symlink decision rather than reader tolerance.

A session has a UUID and a monotonically increasing immutable document
revision. Opening creates revision 1; a material document update appends the
next revision and publishes an event only after durable storage succeeds.
Feedback is always attributed to session UUID, revision, block ID, type, body,
and lifecycle state. Accepted durable outcomes are promoted separately to the
task, spec, or ADR; the presentation history is not a replacement authority.

### Selection and re-anchoring

Browser positions use UTF-16 code-unit offsets, matching DOM Range semantics.
A text annotation stores the selected exact quote plus bounded prefix and
suffix context against the block's canonical review text. On the same immutable
revision, an in-range exact match is anchored. Across revisions, an exact quote
and context may produce one uniquely re-anchored result, which is visibly
flagged. Zero or multiple valid matches, a deleted block, or a mismatch yields
an orphaned-but-visible annotation. CodeFlow never uses fuzzy similarity or a
first-match guess.

Rust defines canonical review text for every annotatable block, and the browser
uses the same emitted text mapping when capturing a selection. A selection that
crosses block boundaries or cannot be mapped uniquely is retained as an
unpositioned quote for user correction; it is not assigned a fabricated range.
Document-chrome updates occur in a separate DOM root so feedback and theme
changes preserve live document node identity and selection. A revision update
captures or cancels the pending selection before changing document content.

The same review envelope may instead target a semantic element or a visual
region. Element targets contain only the runtime's closed structural-path
grammar plus the exact rendered-block digest; arbitrary CSS selectors are
rejected. Regions use integer normalized coordinates in an exact block or the
source document, with capture dimensions retained as evidence. A block-scoped
element or region may re-anchor only while that block digest is unchanged. A
changed block or document-wide region in a later revision becomes visibly
orphaned. Whole-block and whole-document notes use these same boundaries.
While a review remains pending, the browser keeps numbered marks over the
selected targets so the note list and document stay mutually legible.

### Feedback delivery

Feedback lifecycle states are `received`, `delivered`, `addressed`, and
`dismissed`. Individual notes remain browser-local pending state until one
review submission atomically creates one durable review envelope with a stable
event ID, actor/session/revision attribution, its typed anchored notes, and
`verdict` exactly `approve`, `approve_with_notes`, or `request_changes`.
`request_changes` requires a nonempty instruction; the other verdicts may carry
one. History export contains the whole envelope. Creation appends `received`;
the CLI may claim a bounded envelope for delivery, but records `delivered` only
after the complete envelope has been written and flushed to its stdout
protocol. Interruption before that acknowledgement causes at-least-once
redelivery of the same stable event ID, not loss. Consumers deduplicate by event
ID. `delivered` therefore proves receipt by the invoking command consumer, not
that a later model has understood or acted on the review. The active harness
must include the envelope in its current model turn before resolving it.
Address/dismiss transitions require the current event version and reject
stale or cross-session updates.

One replayed ledger is the feedback-lifecycle authority. It requires exactly
one `received` record before `delivered`, permits at most one `delivered` and
one terminal record, and rejects duplicate, missing, or post-terminal
transitions as corruption. Exact receipt, delivery, and resolution retries are
no-ops, including when an older project has already exceeded its current
storage budget; a retry cannot manufacture another durable transition.

The append-only log is written under a per-session lock, synchronizes accepted
records, and treats only an interrupted final record as recoverable; corruption
before the tail is a loud state error. Session summaries use write-to-new-file,
sync, and atomic replace. Revisions are immutable files. History reads operate
from a consistent locked snapshot and never inject themselves into later model
context unless the user or active workflow explicitly resumes them.

### Configuration and exports

`.codeflow/present/config.toml` and an optional primitive-token file are
project-owned and created only by explicit customization. They do not contain
credentials or arbitrary executable paths and are never overwritten by
`codeflow update`. Standalone export is one deterministic self-contained HTML
file: it includes the chosen immutable revision, selected utility theme,
semantic static fallback, and the separately budgeted compressed renderer
needed for offline fidelity. It excludes feedback controls and all
authentication, profile, service, event-log, and session-private state by
construction; it opens without a server or network and writes only to the
explicit confined output path. A separate explicit history export uses the
public session schema and is never implicitly included in the HTML.
Every export is created as a new owner-private file or refused. On Windows the
restricted DACL is applied and verified through the already-open file handle,
so path replacement cannot create a hardening gap.

## Consequences

- Documents, tokens, and exports remain inspectable and interoperable without
  making internal session files a public API.
- Strict closed schemas and centralized limits reject some superficially valid
  inputs; this is preferable to browser-dependent interpretation or unbounded
  resource use.
- Exact selectors do not recover every annotation after substantial edits, but
  they make every recovery explainable and never hide lost context.
- At-least-once delivery can repeat an event after a crash; stable IDs and an
  explicit acknowledgement boundary make repetition safer than silent loss.
- Schema evolution requires fixtures, migration evidence, and a new decision
  for incompatible semantics rather than ad hoc reader tolerance.

## Architecture impact

`docs/architecture.md` gains the closed public schemas, canonical block-review
text, immutable revision and append-only event authorities, exact re-anchoring
states, and at-least-once feedback-delivery boundary.
