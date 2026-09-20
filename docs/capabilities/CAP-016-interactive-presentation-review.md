# CAP-016: interactive-presentation-review

<!-- WHAT layer, graduated from docs/capabilities.md. The registry keeps the
     machine-read yaml block, a summary, and a link here; this file holds the
     full prose. Update both in the PR that ships the work. -->

`codeflow present` turns a closed versioned JSON+Markdown document into one
bounded local review surface. The standard/full scaffold supplies the
cross-harness `cf-present` authoring skill, canonical document/token/history
schemas, representative assets, and proportional routing: simple answers stay
in chat; a complex explanation, comparison, plan, decision, evidence set, diff,
or review uses the utility only when coherent visual inspection or anchored
feedback materially helps. Agents author **this session's** subject into the
catalog; the runtime owns chrome, themes, and the Comment system. The
design-exploration board is craft reference, not a document to clone. Durable
docs belong to `cf-docs-portal`.

Text feedback retains the selected occurrence and revalidates live ranges before
pinning. Toolbar capture survives focus changes; leaving Comment releases its
capture state. Iframe figures use native hit-testing while commenting and regain
their prior pointer interaction afterward, without overlays on neighboring text.

The runtime validates the declarative block tree, embeds its deterministic
renderer, stores immutable revisions and append-only feedback in owner-private
project-keyed durable state, keeps browser-owned profile/cache and bounded
runtime controls in a separate derived root, and exposes open/update/list/show/history/feedback/resolve/
export/close/clear through the CLI. A one-time tokenless file bootstrap opens a
CodeFlow-owned isolated browser profile against an authenticated loopback-only
service. Host/Origin/CSP/path/body limits, inert revision-qualified HTML
sandboxing, strict primitive-token import, crash recovery, bounded retention,
and identity-scoped cleanup are code boundaries. Event parsing and partial-tail
repair are self-bounded and operate through one opened handle. Mermaid input is
capped per diagram and per document; browser enhancement is serialized, yields
between diagrams, and fails remaining items to escaped source when the eager
fallback exhausts its cumulative budget. Browser and auxiliary system-tool
children share one allowlist-only environment. Windows ACL mutation is confined
to creation for every private file, including append and lease files; existing
state uses native read-only owner/protected-DACL/trustee/inheritance
verification. Unix ignores a relative XDG state override and rejects a relative
home rather than placing state in the worktree. Static export remains self-contained and excludes
review/authentication/runtime state.

Native-path repository identity, per-block and whole-document collection
cardinality, a project→session→runtime-control lock order, pre-publication
capacity admission for every durable growth route, and an over-quota-safe
control path keep quota boundaries deterministic under concurrent creation,
revision, feedback, and runtime registration. The browser loads a bounded recent
feedback snapshot, uniquely re-anchors exact selectors across revisions, leaves
missing/ambiguous selectors visibly orphaned, and exposes the current lifecycle
version. Resolve accepts only a current delivered event and appends
addressed/dismissed state. Exact accepted receipt and identical terminal retries
are zero-growth operations; conflicting reuse and unrelated stale versions fail
closed. Retention recomputes bounded durable size after every eviction, selected
cleanup loads only its named session, and bulk cleanup reports isolated partial
failures without deleting ambiguous state. A selected session that cannot be
removed reports its exact retained outcome instead of succeeding silently.
Verified Unix process groups receive bounded graceful shutdown and then an
identity recheck before forced termination. Unverifiable orphan process
groups/trees retain recovery state rather than killing an unproven process or
deleting its profile. Launches are serialized under a session lease and publish
one exact record per attempt; close, show, and retry consume it. Reused PIDs are
never signalled: bounded exact-marker discovery and native profile-resource
proof either complete cleanup or retain an actionable error. Windows checks
exclusive handles across the actual profile tree rather than assuming a
POSIX-style lock file. After a retained group/tree exits or an operator verifies
and terminates it, retrying close completes cleanup. Review input controls expose
the Rust-owned note, text, selection, and payload bounds before submission.

The capability remains `building` until TSK-007 records the full native
macOS/Linux/WSL2/Windows and qualified-browser matrix, adversarial service and
state evidence, design/accessibility/responsive comparisons, deterministic
asset/release checks, and fresh native interactive model trials. That matrix
includes Windows Unicode known-folder/profile paths, creation-time ACL
hardening plus read-only weakened-ACL rejection, trusted system tools, exact
process-tree identity and file URLs; Linux/WSL2
bounded `/proc` identity and group signaling; and a dense multi-diagram browser
corpus with long-task evidence. Cross-builds alone do not claim native runtime
support.
