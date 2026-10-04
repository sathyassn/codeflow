# Present review sessions

<!-- HOW layer. Graduated from docs/architecture.md per its own rule: the
     area outgrew the single file; a one-line pointer remains behind.
     Sources: ADR-0049, ADR-0050, ADR-0052. -->

## Concept

**A present session is one bounded, owner-private review:** an agent authors
the catalog document, Rust owns every durable byte, and the browser is a
disposable isolated viewer, never an authority.

The runtime owns the review chrome and the Comment tool; the agent authors
only the subject of this one session. Your notes travel back through the
service, which the agent reads when it is ready.

## Architecture

Each active review keeps its state in three places, so growth is governed
where it happens.

- One project-keyed, owner-private durable authority holds the record.
- A separate derived runtime root holds only what the browser needs to start
  and recover.
- The browser profile holds nothing that counts as record.

Growth is bounded as follows.

- Immutable revisions and feedback are the only quota-governed history.
- The browser profile and cache never become durable authority.
- The CodeFlow-owned bootstrap, ready, and launch-recovery controls have a
  separate exact budget.
- Conservative cache flags, bounded idle lifetime, and identity-scoped cleanup
  limit browser growth without presenting it as a hard CodeFlow quota.
- One lock order covers both roots: project mutation, then session, then
  runtime control (architecture decision record ADR-0052).

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
| Recovery reserve | a control reserve and a separate non-growth path keep close, runtime identity release, and clear available for recovery even when active state is already over its configured bound |
| Cardinalities | block, figure, per-collection, and whole-document collection cardinalities bound renderer amplification in addition to encoded byte limits |

### Session surface and export

| Surface | Contract |
|---|---|
| Per session | one loopback service, one single-use file bootstrap, and one isolated browser profile |
| Review chrome | Host, Origin, cookie and Content Security Policy (CSP) checks protect it |
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
| Retired diagram block | new `open` and `update` input with a `diagram` block is refused with its conversion named; a revision stored with one loads read only, as the `retired` history kind, with a notice: every other block renders as before, each diagram shows its escaped source in its place, and the revision is never rewritten |

### Platform boundaries

Platform boundaries are native and fail closed.

| Platform | Boundary |
|---|---|
| Windows | discovers trusted system and known-folder paths without `PATH` lookup, rejects reparse traversal, parses process identity with Windows command-line rules, emits UTF-8 from Windows PowerShell, passes a protected owner-only descriptor at creation for every private file including append and lease files, and verifies owner, protected discretionary access control list (DACL), trustees, and inheritance whenever existing state is opened |
| Linux and WSL2 | reads bounded, no-follow `/proc` identity and terminates only the proven process group |
| macOS | uses delimiter-aware identity and the same ownership rule; a listed process whose command line is not UTF-8, such as one caught mid-start, neither fails the inventory nor matches an owned identity |
| Unix generally | state-root inputs must be absolute; a relative XDG state override is ignored and a relative home is rejected rather than placing state in the worktree |

Every browser or auxiliary system-tool child starts from one allowlist-only
environment, so provider-secret environment variables are not inherited. Launch
and cleanup follow these rules.

| Lifecycle rule | Behavior |
|---|---|
| Launch lease | a session lease serializes each browser launch from exact per-attempt recovery publication through durable registration; close, show, and later launch consume interrupted evidence |
| Lost PID | if a recorded PID disappears or is reused, the native adapter searches for the exact instance and profile marker |
| Absence proof | Windows corroborates absence with bounded exclusive handle checks over the real profile tree rather than a POSIX-style lock-file assumption; Unix rechecks exact candidates and the process group |
| Inconclusive result | failed enumeration, unreadable identity, a remaining candidate, or an inconclusive resource probe retains recovery evidence rather than signalling or deleting |
| Clear and retention | durable clear and retention reap derived runtime only after the same absence boundary; independently corrupt bulk items are retained and reported without blocking an explicitly selected safe item |
| Retained session | a selected session that is active or still owns a proven runtime is reported as retained, never represented by an empty successful clear |
| Shutdown | verified Unix process groups receive a bounded graceful shutdown, then an identity recheck before forced termination; after a retained group exits or an operator verifies and ends it, retrying close completes cleanup |

Cross-target compilation checks adapter shape only. Native runtime, Unicode
path, access control list (ACL), process-tree, browser, and cleanup evidence
remains a release gate. The capability stays `building` until the full native
macOS, Linux, WSL2 and Windows matrix with a qualified browser is recorded.

### Review-surface anchoring and resolve

| Rule | Behavior |
|---|---|
| Snapshot | the current review surface loads a bounded recent feedback snapshot |
| Same-revision selectors | retain their exact offsets |
| Older selectors | re-anchor only when the exact quote plus prefix and suffix context has one match |
| Missing or ambiguous match | remains visibly orphaned |
| Agent-side `resolve` | requires the event's current delivered version and appends addressed or dismissed state; stale or cross-session updates fail closed |
| Text selection | retains the selected occurrence and revalidates live ranges before pinning |
| Comment capture | toolbar capture survives focus changes, and leaving Comment releases it; iframe figures use native hit-testing while commenting and regain their pointer interaction afterwards |
| Input bounds | review controls show the Rust-owned note, text, selection and payload bounds before submission |
| Full history | remains available explicitly, without being injected into unrelated work |

### Conversation and revision projections

- Replies, reopens and tombstones extend `responses.jsonl` additively and
  record the revision at each transition.
- Replies are agent output. Only reviewer reopens and tombstones join the v2
  delivery stream.
- Thread routes use the same host, origin, cookie and request-header checks as
  answers.
- Deletion redacts public projections, including note replies, while the
  private ledger remains append-only until clear. Previously delivered copies
  cannot be recalled.
- New revisions optionally record repository commit, dirty state and code/diff
  source paths. Existing records omit those fields and still load.
- Captured paths are metadata only. The service has no repository-file route,
  and the path/commit pair does not certify snippet equality.
- `present diff` compares stored block bodies and reanchors carried notes
  against its target revision.
- `present check` reuses document validation without launching a browser.
- Default exports omit the conversation. `--with-notes` adds a read-only
  appendix.
