# present — bounded review sessions

<!-- HOW layer. Graduated from docs/architecture.md per its own rule: the
     area outgrew the single file; a one-line pointer remains behind.
     Sources: ADR-0049, ADR-0050, ADR-0052. -->

## Concept

A present session is one bounded, owner-private review: an agent authors the
catalog document, Rust owns every durable byte, and the browser is a
disposable isolated viewer — never an authority.

```cf-stage
agent catalog | validated document JSON @accent
->
rust service | validation · revisions · feedback · retention · export
->
isolated browser | one session · one profile · one single-use bootstrap
->
feedback envelope | append-only, consumed by any harness @positive
caption: the browser is a viewer — durable authority never leaves rust
```

The runtime owns chrome and Comment; agents author only this session's
subject. Present is not a resident service, a product UI framework, or a
documentation portal.

## Architecture

Durable authority and derived runtime are separate owner-private roots under
one canonical lock order — project mutation → session → runtime control
(ADR-0052) — with closed contracts typed twice: matching Rust types and
managed JSON Schemas under `.codeflow/schemas/present/`.

```cf-stage
durable authority | versioned document · immutable revisions · feedback @accent
->
derived runtime root | bootstrap · ready · launch-recovery controls
->
browser profile | cache only — never durable authority
caption: revisions and feedback are the only quota-governed history
```

Immutable revisions and feedback are the only quota-governed history;
CodeFlow-owned controls have a separate exact budget, and browser growth is
mitigated without being misrepresented as a hard CodeFlow quota.

## Technical

### Engine surface and contracts

Interactive presentation is a separate bounded engine surface
(`codeflow-present`, ADR-0049, ADR-0050, and ADR-0052). Its closed versioned document,
primitive-token, and public-history contracts are represented by matching Rust
types and managed JSON Schemas installed under `.codeflow/schemas/present/`.
Rust owns validation, immutable revisions, append-only feedback, retention,
export, and the per-session loopback service. The service embeds one
content-addressed Preact/Shiki distribution built reproducibly from its
exact lockfile, SBOM, license inventory, integrity manifest, audit, and size
budgets; consumer builds and runtime use require no Node toolchain.

### State, quotas, and bounded growth

Repository state keys hash the canonical path's native OS representation, not
a lossy display string. One project mutation lease serializes every durable
growth path before the per-session lock. Creation, immutable revisions,
feedback transitions, and runtime identity publication reserve exact bounded
disk headroom before publication; they never commit over quota and then invoke
retention. A control reserve and separate non-growth path keep close, runtime
identity release, and clear available for recovery even when legacy active
state is already over its configured bound. Block, per-collection,
and whole-document collection cardinalities bound renderer amplification in
addition to encoded byte limits.

### Authority and runtime separation

Each active review has one project-keyed owner-private durable authority and a
separate derived owner-private runtime root (ADR-0052). Immutable revisions and
feedback are the only quota-governed history. Browser-owned profile/cache data
never becomes durable authority; CodeFlow-owned bootstrap, ready, and launch-
recovery controls have a separate exact budget and the canonical lock order is
project mutation → session → runtime control. Conservative cache flags, bounded
idle lifetime, and identity-scoped cleanup mitigate browser growth without
misrepresenting it as a hard CodeFlow quota. Each session has one loopback
service, one single-use file bootstrap, and one isolated browser profile.
Host/Origin/cookie/CSP checks protect the review chrome;
untrusted static HTML is served from a revision-qualified sandbox without
scripts, same-origin, forms, navigation, or network. Full-fidelity export is a
self-contained read-only HTML artifact with no credentials, review controls,
profile paths, feedback history, or service state. Platform adapters fail
closed rather than falling back to the operator's browser. The `present` CLI
adapter exposes open/update/list/show/history/feedback/resolve/export/close/
clear but
does not become a resident service, product UI framework, or documentation
portal.

### Feedback ledger and renderer bounds

Every state read and recovery path is self-bounded. A single feedback ledger
replay rejects duplicate receipts/deliveries, delivery before receipt,
resolution before delivery, and every post-terminal transition; exact receipt,
delivery, and identical terminal retries append nothing, while conflicts remain
loud. Event tails are read from the same opened
handle used for size and repair decisions; aggregate history,
records, revisions, media, and state entries have explicit limits. The
`diagram` block was removed with Mermaid (ADR-0049, update of 2026-09-28): new
`open` and `update` input with one is refused with its conversion named, and a
revision stored with one loads read only, as the `retired` history kind, with a
notice and its escaped source, and is never rewritten.

### Platform boundaries

Platform boundaries are native and fail closed: Windows discovers trusted
system and known-folder paths without `PATH` lookup, rejects reparse traversal,
parses process identity with Windows command-line rules, emits UTF-8 from
Windows PowerShell, passes a protected owner-only descriptor at creation for
every private file including append/lease files, and verifies owner, protected
DACL, trustees, and inheritance whenever existing state is opened. Every browser or auxiliary system-tool child starts from one
allowlist-only environment, so provider-secret environment variables are not
inherited.
Linux/WSL2 reads bounded, no-follow `/proc` identity and terminates only the
proven process group; macOS uses delimiter-aware identity and the same ownership
rule, and Unix state-root inputs must be absolute. A session lease serializes
each browser launch from exact per-attempt recovery publication through durable
registration; close, show, and later launch consume interrupted evidence. If a
recorded PID disappears or is reused, the native adapter searches for the exact
instance/profile marker. Windows corroborates absence with bounded exclusive
handle checks over the real profile tree rather than a POSIX-style lock-file
assumption; Unix rechecks exact candidates and the process group. Failed enumeration, unreadable identity, a remaining
candidate, or an inconclusive resource probe retains recovery evidence rather
than signalling or deleting. Durable clear/retention reaps derived runtime only
after the same absence boundary; independently corrupt bulk items are retained
and reported without blocking an explicitly selected safe item. A selected
session that is active or still owns a proven runtime is reported as retained,
never represented by an empty successful clear.
Cross-target compilation checks adapter shape only. Native runtime, Unicode
path, ACL, process-tree, browser, and cleanup evidence remains a release gate.

### Review-surface anchoring and resolve

The current review surface loads a bounded recent feedback snapshot. Same-
revision selectors retain their exact offsets; older selectors re-anchor only
when exact quote plus prefix/suffix context has one match. Missing or ambiguous
matches remain visibly orphaned. Agent-side `resolve` transitions require the
event's current delivered version and append addressed/dismissed state; stale
or cross-session updates fail closed. Full append-only history remains available
explicitly without being injected into unrelated work.
