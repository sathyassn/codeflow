---
id: ADR-0049
title: bounded cf-present runtime and renderer boundary
date: 2026-08-01
status: accepted
superseded_by: null
architecture_impact: add the session-scoped presentation renderer, service, browser-isolation, and build boundaries
---

# ADR-0049 — bounded cf-present runtime and renderer boundary

## Context

SPC-004 requires a reusable local review document while CodeFlow remains a CLI
discipline layer, not a GUI product, browser-profile manager, or persistent
service. The renderer must support accessible Markdown, code, diagrams, typed
feedback, and a sandboxed HTML escape without giving untrusted content access
to credentials, review controls, remote resources, or the operator's browser
state. The implementation must also remain one cross-platform Rust binary whose
normal build and use do not require a JavaScript toolchain.

The bounded S1–S5 spike in
[`cf-present-runtime-spike-2026-08-01.md`](../verification/cf-present-runtime-spike-2026-08-01.md)
measured renderer alternatives, credential bootstrap, untrusted-content
containment, event transport, anchor stability, and deterministic packaging.
Its figures and limitations are part of this decision rather than performance
claims inferred from package names.

## Decision

### Process and ownership

`crates/codeflow-present` owns document rendering, session state, the local
service, browser launch, export, and cleanup. `crates/codeflow-cli` remains a
thin command adapter. Each open session starts one loopback-only service that
owns that session, uses an operating-system-assigned port, and exits on explicit
close, idle timeout, or unrecoverable state error. The launcher waits for a
bounded ready handshake; loss of the launcher before readiness cancels startup,
while a ready service is intentionally independent of the short-lived CLI
parent until its session termination. CodeFlow does not install an autorun
service or persistent daemon.

The browser launcher is a qualified platform adapter, not a call to the default
URL opener. It creates a new app window with a unique CodeFlow-owned profile
directory and never attaches to an existing profile or active view. A platform
route that has not passed its native canary fails closed and offers an explicit
`--no-launch` path; it does not fall back to the operator's default browser.
The qualification ledger names browser Brotli advertisement and, on native
Windows, owner-private state/profile ACL verification as separate canaries.

### Renderer composition

Rust renders the stable, semantic review-document subtree. Preact 10.29.8 owns
only the separately mounted review chrome and never rerenders the document
root. This preserves DOM identity, selection, native semantics, and readable
no-script output while keeping stateful controls componentized. Lit was larger
in the representative light-DOM spike without providing encapsulation; a
vanilla controller would retain the same syntax/diagram toolchain while making
the state, accessibility, and cleanup burden application code.

Shiki 4.4.1 supplies fine-grained dual-theme highlighting from a curated,
exactly pinned grammar set owned by the committed asset manifest and constrained
by the measured chunk/corpus budgets. The initial set is Bash, diff, JavaScript,
JSON, Python, Rust, TOML, TypeScript, and YAML. An unbundled language renders as
safely escaped plain code with its language label; it never triggers a network
fetch or an unpinned dynamic import. Production syntax palettes derive from the
utility semantic-token contract and pass per-token contrast checks; the spike's
GitHub themes are measurement inputs, not the shipped design direction.
Mermaid 11.16.0 is lazy-loaded only for diagram blocks and receives a closed
theme bridge plus required accessible title and description. No renderer asset
may fetch a remote script, style, font, image, grammar, or diagram resource.

Maintainers build those assets from an exact lockfile with Node 26.4.0, npm
11.17.0, and esbuild 0.25.9. The repository commits the content-hashed
distribution, license inventory, CycloneDX SBOM, and integrity manifest. Rust
builds, cross-compiles, installs, and runtime sessions consume only that
committed distribution and do not require Node.
The exact renderer `package-lock.json` stays in the repository path scanned by
the ordinary SCA perimeter; the release-specific audit supplements rather than
replaces that check.

The service embeds Brotli-only assets; the qualified modern browser must
advertise `br`, otherwise the service returns `406 Not Acceptable` with a
plain-text explanation naming the qualification requirement and `--no-launch`
recovery path. It never silently keeps or serves a raw fallback. Hashed assets have exact MIME,
`Content-Encoding: br`, `Vary: Accept-Encoding`, SHA-256 ETags, one-year
immutable caching, and `nosniff`; document responses are `no-store`. The build
gate enforces the spike record's raw-corpus, chunk, embedded-binary,
encoded-request, reproducibility, license, SBOM, and audit budgets.

Full-fidelity offline export is one self-contained HTML file with a semantic
static fallback and one separately budgeted gzip-compressed universal renderer
module. A hash-authorized bootstrap uses the browser's native
`DecompressionStream` and a local blob module, so export adds no Rust decoder
and embeds no second raw corpus. Export CSP denies network and active embedding;
the artifact contains the chosen document/theme but no capability, cookie,
review controls, profile path, feedback/event history, or service state.

### Service and trust boundary

The service uses axum 0.8.9 with only the HTTP/1, Tokio, JSON, and form features
needed by the routes. It accepts only the exact authority
`127.0.0.1:<assigned-port>`; no wildcard host, hostname alias, LAN bind, or
remote mode exists in v1. Static assets use content hashes and a restrictive
CSP. Application cookies are per-session names, `HttpOnly`, `SameSite=Strict`,
and scoped to `/app` because browser cookies are not port-scoped.

The session capability is 256 random bits and crosses the process boundary only
in an owner-readable one-time bootstrap HTML file. The isolated browser opens
that `file:` URL and submits one bounded form body to the exact
`http://127.0.0.1:<port>/bootstrap` endpoint. That endpoint alone accepts an
absent or `null` Origin, requires the exact Host and form content type, has a
short TTL, consumes the capability once, sets the application cookie, and
redirects by same-origin replacement. The capability is never placed in a URL,
fragment, process argument, log, referrer, export, durable session record, or
application DOM. The file is removed after success, expiry, failure, and stale
session recovery.

Every application page and API route lives below `/app`. Every state mutation
and event poll then requires the exact HTTP Origin, exact Host, session cookie,
a static non-secret request header, accepted content type, and bounded body.
Events use bounded authenticated POST long-poll with a cursor; v1 does not use
EventSource or a query-string credential. Closing a session causes pending
polls to finish and later polls to return a terminal status.

The sandboxed HTML block is served from a separate `/sandbox/<opaque-id>`
document in an iframe with `sandbox` and without `allow-scripts` or
`allow-same-origin`. It gets a sandbox-specific CSP that permits bounded inline
style but denies network, navigation, forms, embedding, and all script. The
application cookie's `/app` path excludes it. Content is size-bounded, real
paths are never accepted, and the opaque ID grants access only to the immutable
block in its owning live session. Scripted HTML is outside v1 and requires a
new specification and threat-model decision.

### State and cleanup

Session state lives below the platform-native, owner-private application-state
directory, under a SHA-256 project key derived from the canonical Git common
directory so worktrees share state. Moving or recloning a repository creates a
new identity by design; old state remains independently eligible for expiry.
The state authority is versioned files with an explicit per-session lock,
atomic session snapshots and immutable revisions, plus an append-only feedback
event log that tolerates and reports an interrupted final record. SQLite is not
part of the authority or v1 index.

Cleanup uses the SPC-004 retention order, never deletes active or locked
sessions, confines every removal beneath the verified state root, and reports
partial failure. Process/profile/bootstrap cleanup is idempotent and scoped by
the recorded session identity; broad process-name matching and recursive
deletion of an unverified path are forbidden.

A qualified process group receives a bounded graceful stop before cleanup
re-proves its exact recorded identity and may escalate to a forced stop. If the
recorded leader disappears before group or tree identity can be proved, the
runtime retains the recovery state and never signals the unproven processes.
The operator inspects or terminates that identity with native OS tools and then
retries `codeflow present close`; CodeFlow does not turn ambiguity into a broad
kill.

## Consequences

- Normal use gains one coherent, inspectable review document without turning
  CodeFlow into a frontend framework or requiring Node on consumer machines.
- The renderer has a deliberately narrow JavaScript island and a larger
  committed asset supply chain; exact pins, SBOM, license review, audit, hash,
  lazy-load, and size checks become release obligations.
- Full-fidelity export costs a separately measured compressed all-feature
  payload. Its static semantic fallback remains readable without script, while
  diagram/highlight enhancement requires a qualified browser with native gzip
  decompression; other native routes stay unclaimed until TSK-007 qualifies
  them.
- The bootstrap is less convenient than a bearer query parameter and native
  launch coverage must be earned per platform, but credentials stay out of the
  usual URL, history, referrer, and process-list leak paths.
- Untrusted HTML is useful for static bespoke layouts but cannot execute code.
  Requests for interactive arbitrary HTML are explicit future scope, not a
  hidden weakening of this boundary.
- `cf-present` is the sole bounded browser exception to the product's no-GUI /
  no-daemon non-goal; a dashboard, remote viewer, resident service, or reusable
  product UI runtime remains out of scope.

## Architecture impact

`docs/architecture.md` gains the `cf-present` crate, immutable renderer-asset
pipeline, per-session loopback service, tokenless browser bootstrap, isolated
profile, sandbox document, platform-state authority, and cleanup boundaries.
