# Present review sessions

<!-- HOW layer. Graduated from docs/architecture.md per its own rule: the
     area outgrew the single file; a one-line pointer remains behind.
     Sources: ADR-0049, ADR-0050, ADR-0052. -->

## Concept

**A present session is one bounded, owner-private review:** an agent authors
the catalog document, Rust owns every durable byte, and the browser is a
disposable isolated viewer, never an authority.

```cf-stage
agent catalog | validated document JSON @accent
->
rust service | validation · revisions · feedback · retention · export
->
isolated browser | one session · one profile · one single-use bootstrap
->
feedback envelope | append-only, consumed by any harness @positive
caption: the browser is a viewer, durable authority never leaves rust
```

Read the figure as the direction of trust: authority flows left to right and
never back. The runtime owns chrome and Comment; agents author only this
session's subject. Present is not a resident service, a product UI framework,
or a documentation portal.

## Architecture

Each active review splits its state in two, so growth is governed where it
happens. One project-keyed owner-private durable authority holds the record; a
separate derived owner-private runtime root holds what the browser needs. One
canonical lock order covers both: project mutation → session → runtime control
(ADR-0052).

```cf-stage
durable authority | versioned document · immutable revisions · feedback @accent
->
derived runtime root | bootstrap · ready · launch-recovery controls
->
browser profile | cache only, never durable authority
caption: revisions and feedback are the only quota-governed history
```

Immutable revisions and feedback are the only quota-governed history.
Browser-owned profile and cache data never becomes durable authority, and the
CodeFlow-owned bootstrap, ready, and launch-recovery controls have a separate
exact budget. Conservative cache flags, bounded idle lifetime, and
identity-scoped cleanup mitigate browser growth without misrepresenting it as a
hard CodeFlow quota.

## Technical

The contracts, the bounds, and the platform evidence that keep the split above
true.

### Engine surface and contracts

| Contract | Detail |
|---|---|
| Engine surface | interactive presentation is a separate bounded engine surface, `codeflow-present` (ADR-0049, ADR-0050, ADR-0052) |
| Schemas | the closed versioned document, primitive-token, and public-history contracts are represented by matching Rust types and managed JSON Schemas installed under `.codeflow/schemas/present/` |
| Rust ownership | validation, immutable revisions, append-only feedback, retention, export, and the per-session loopback service |
| Embedded distribution | one content-addressed Preact/Shiki bundle with the figure grammar, built reproducibly from its exact lockfile, software bill of materials (SBOM), license inventory, integrity manifest, audit, and size budgets; consumer builds and runtime use require no Node toolchain |

### State, quotas, and bounded growth

| Rule | Behavior |
|---|---|
| State keys | repository state keys hash the canonical path's native OS representation, not a lossy display string |
| Serialization | one project mutation lease serializes every durable growth path before the per-session lock |
| Headroom | creation, immutable revisions, feedback transitions, and runtime identity publication reserve exact bounded disk headroom before publication; they never commit over quota and then invoke retention |
| Recovery reserve | a control reserve and a separate non-growth path keep close, runtime identity release, and clear available for recovery even when legacy active state is already over its configured bound |
| Cardinalities | block, figure, per-collection, and whole-document collection cardinalities bound renderer amplification in addition to encoded byte limits |

### Session surface and export

| Surface | Contract |
|---|---|
| Per session | one loopback service, one single-use file bootstrap, and one isolated browser profile |
| Review chrome | Host, Origin, cookie and CSP checks protect it |
| Untrusted static HTML | validated (no scripts, handlers, links, embeds, forms or remote URLs) and inlined into the live page under a per-block scoped host and the application CSP, so notes can target its parts; export wraps it in a sandboxed `iframe` `srcdoc` without scripts or same-origin (ADR-0049 update of 2026-09-26) |
| Export | a self-contained read-only HTML artifact at full fidelity, with no credentials, review controls, profile paths, feedback history, or service state |
| Platform adapters | fail closed rather than falling back to the operator's browser |
| CLI adapter | `present` exposes open, update, list, show, history, feedback, resolve, export, close and clear, and does not become a resident service, product UI framework, or documentation portal |

### Feedback ledger and renderer bounds

Every state read and recovery path is self-bounded.

| Bound | Behavior |
|---|---|
| Ledger replay | a single replay rejects duplicate receipts and deliveries, delivery before receipt, resolution before delivery, and every post-terminal transition |
| Idempotence | exact receipt, delivery, and identical terminal retries append nothing, while conflicts remain loud |
| Event tails | read from the same opened handle used for size and repair decisions |
| Explicit limits | aggregate history, records, revisions, media, and state entries each have one |
| Figure count | at most 24 figure blocks per document, counted inside disclosures and tabs |
| Retired diagram block | new `open` and `update` input with a `diagram` block is refused with its conversion named; a revision stored with one loads read only, as the `retired` history kind, with a notice and its escaped source, and is never rewritten |

### Platform boundaries

Platform boundaries are native and fail closed.

| Platform | Boundary |
|---|---|
| Windows | discovers trusted system and known-folder paths without `PATH` lookup, rejects reparse traversal, parses process identity with Windows command-line rules, emits UTF-8 from Windows PowerShell, passes a protected owner-only descriptor at creation for every private file including append and lease files, and verifies owner, protected discretionary access control list (DACL), trustees, and inheritance whenever existing state is opened |
| Linux and WSL2 | reads bounded, no-follow `/proc` identity and terminates only the proven process group |
| macOS | uses delimiter-aware identity and the same ownership rule |
| Unix generally | state-root inputs must be absolute |

Every browser or auxiliary system-tool child starts from one allowlist-only
environment, so provider-secret environment variables are not inherited.

| Lifecycle rule | Behavior |
|---|---|
| Launch lease | a session lease serializes each browser launch from exact per-attempt recovery publication through durable registration; close, show, and later launch consume interrupted evidence |
| Lost PID | if a recorded PID disappears or is reused, the native adapter searches for the exact instance and profile marker |
| Absence proof | Windows corroborates absence with bounded exclusive handle checks over the real profile tree rather than a POSIX-style lock-file assumption; Unix rechecks exact candidates and the process group |
| Inconclusive result | failed enumeration, unreadable identity, a remaining candidate, or an inconclusive resource probe retains recovery evidence rather than signalling or deleting |
| Clear and retention | durable clear and retention reap derived runtime only after the same absence boundary; independently corrupt bulk items are retained and reported without blocking an explicitly selected safe item |
| Retained session | a selected session that is active or still owns a proven runtime is reported as retained, never represented by an empty successful clear |

Cross-target compilation checks adapter shape only. Native runtime, Unicode
path, ACL, process-tree, browser, and cleanup evidence remains a release gate.

### Review-surface anchoring and resolve

| Rule | Behavior |
|---|---|
| Snapshot | the current review surface loads a bounded recent feedback snapshot |
| Same-revision selectors | retain their exact offsets |
| Older selectors | re-anchor only when the exact quote plus prefix and suffix context has one match |
| Missing or ambiguous match | remains visibly orphaned |
| Agent-side `resolve` | requires the event's current delivered version and appends addressed or dismissed state; stale or cross-session updates fail closed |
| Full history | remains available explicitly, without being injected into unrelated work |
