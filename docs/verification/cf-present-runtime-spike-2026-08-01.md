# `cf-present` bounded runtime spike

**Task:** TSK-011  
**Date:** 2026-08-01  
**Status:** decision evidence; not production verification

## Purpose and boundary

This spike resolves the implementation choices that SPC-004 deliberately left
to TSK-011. It tests renderer weight, reproducible packaging, credential
bootstrap, untrusted HTML containment, authenticated events, DOM/selection
stability, and exact annotation re-anchoring before dependencies or public
schemas become production commitments.

The scratch programs lived outside the repository and are disposable. Exact
observations, versions, hashes, expected invariants, and limitations are kept
here so ADR-0049 and ADR-0050 do not turn a successful prototype into a broader
claim. Production tests must independently reproduce every adopted invariant.

## S1 — renderer composition and package weight

### Exact toolchain and alternatives

The comparison ran on macOS 26.5.2 arm64 with Node 26.4.0, npm 11.17.0,
Rust/Cargo 1.94.0, and Chrome 150. It used an exact lockfile and these exact
packages:

| Package | Version | Purpose |
|---|---:|---|
| Preact | 10.29.8 | stateful review chrome candidate |
| Lit | 3.3.3 | light-DOM component candidate |
| esbuild | 0.25.9 | deterministic asset build |
| Shiki core/engine/langs/themes | 4.4.1 | curated syntax highlighting |
| Mermaid | 11.16.0 | earned diagram block |
| TypeScript | 5.9.2 | build-time type checking |
| Playwright core | 1.55.0 | isolated browser spike only |

The corrected Preact measurement used automatic JSX with Preact as the JSX
source. The first provisional result that left a React reference in the bundle
is excluded.

| Minimal chrome candidate | Raw | gzip | What the figure proves |
|---|---:|---:|---|
| Preact | 11,044 B | 4,650 B | representative component/state floor |
| Lit, light DOM | 15,314 B | 5,872 B | no encapsulation benefit at greater weight |
| vanilla JavaScript | 252 B | 182 B | syntax floor only; not a full controller |

The vanilla result does **not** include equivalent focus, state, lifecycle,
cleanup, annotation, or accessibility behavior and therefore is not a valid
product-cost comparison. Preact is the smallest coherent component boundary
measured for the stateful chrome. Rust owns the stable semantic document tree;
Preact mounts beside it and does not rerender it.

The production-like split build included Bash, diff, JavaScript, JSON, Python,
Rust, and TypeScript grammars, GitHub light/dark themes, and lazy Mermaid. An
initial raw-embed pass established the approximate 4.1 MiB problem; the exact
content-hashed compressed comparison below supersedes those preliminary bytes
and earns the accepted budgets.

### Compressed packaging, requests, and lazy loading

A fresh content-hashed ESM split build targeted Chrome 120, Firefox 121, and
Safari 17. The final nine-grammar set produced 122 payload files measuring
4,129,129 B raw, 1,091,823 B under deterministic gzip-9, and 934,719 B under
Brotli q11. The largest file measured
668,390 B raw, 146,876 B gzip, and 120,083 B Brotli.

The raw-versus-compressed matrix used the seven-grammar candidate and actual
stripped/LTO Rust release binaries with rust-embed 8.12.0:

| Embedded corpus | Binary | Delta from 333,632 B baseline |
|---|---:|---:|
| raw only | 4,495,136 B | +4,161,504 B |
| gzip only | 1,440,416 B | +1,106,784 B |
| Brotli only | 1,291,808 B | +958,176 B |
| raw + Brotli | 5,453,344 B | +5,119,712 B |
| gzip + Brotli | 2,398,624 B | +2,064,992 B |

The final nine-grammar Brotli-only binary measured 1,291,824 B, a 958,192 B
delta from the same baseline. YAML and TOML added only 3,492 B to the full
Brotli corpus; the raw/duplicate variants were not retained or remeasured.

Brotli-only is the coherent service package. The qualified modern browser must
advertise `br`; otherwise the service returns `406 Not Acceptable`. It does not
keep a raw fallback or silently transfer the wrong representation. Hashed
assets carry correct MIME, `Content-Encoding: br`, `Vary: Accept-Encoding`, a
SHA-256 ETag, `Cache-Control: public,max-age=31536000,immutable`, and
`X-Content-Type-Options: nosniff`; document HTML is `no-store`. Unknown and
unmanifested paths return 404.

Thirty fresh-process/warm-filesystem runs measured a 4.090 ms baseline median
and 4.116 ms Brotli median, with median peak RSS 1,556,480 B and 1,638,400 B
respectively. Differences below about 1 ms are not decision-grade, and this is
not a true cold-filesystem-cache or real-service measurement. Production
service startup, request RSS, and browser heap remain explicit tests.

Each browser case used a fresh isolated Chrome context and all response bodies
were Brotli:

| Document | Encoded response bodies | Feature chunks observed |
|---|---:|---|
| prose only | 5,031 B | none from Shiki or Mermaid |
| Rust code | 58,559 B | syntax core + Rust grammar only |
| YAML code | 58,143 B | syntax core + YAML grammar only |
| TOML code | 57,293 B | syntax core + TOML grammar only |
| Mermaid flowchart | 193,348 B | Mermaid core/shared + flowchart only; no Shiki |

The rendered outputs carried their expected Shiki and Mermaid completion
markers. The server exposed only paths from the generated integrity manifest.

The initial nine grammars are representative of CodeFlow's shipped Rust
implementation, shell and diff workflows, JSON transport/configuration,
YAML-frontmatter and TOML project records, and the JavaScript/TypeScript and
Python stacks already recognized by the product. The list is manifest-owned,
not frozen as a universal language claim. Any other language degrades to a
labelled, safely escaped plain-code block with no fetch or dynamic install.

### Reproducibility and supply chain

Repeated clean builds from the same exact lockfile were byte-identical. The
final nine-grammar packaging manifest SHA-256 was
`676f74b8d66c3db712399e4f428c9016ce3a38449d12ec5d1328098546614363`.
`npm sbom --omit=dev --sbom-format=cyclonedx` produced CycloneDX 1.5 with 160
components and 161 dependency nodes. `npm audit --omit=dev` reported zero known
production vulnerabilities at every severity. The production license set was
111 MIT, 34 ISC, 11 BSD-3-Clause, one Apache-2.0, one Unlicense, and one
MPL-2.0-or-Apache-2.0 package. `khroma@2.1.0` omitted package metadata but
shipped an MIT license file with SHA-256
`66b333b0f66759a0b710459e03f7029abe17f4358114a128d2c972e642961b49`;
the production assertion must pin that exact package/version/hash and reject a
new missing or disallowed license rather than generalize the exception.

These point-in-time results are not standing safety claims. The release gate
regenerates the audit, Cargo and npm SBOMs, license inventory, asset manifest,
and reproducibility comparison from committed locks.

### Offline export

Full standalone fidelity is required by SPC-004, so export uses a deliberately
separate compressed artifact rather than a hidden raw copy. A no-splitting
all-feature export module measured 4,145,304 B raw, 1,058,196 B deterministic
gzip, and 798,341 B Brotli. Its gzip SHA-256 was
`e3114cac63296d91ade1459e858dc6a5dad858ba3c0226b025cfb0242e798b10`.

The proof generated one 1,412,263 B self-contained HTML file containing a
semantic static fallback, base64 gzip module, and a tiny SHA-256-CSP-authorized
bootstrap that uses the browser-native `DecompressionStream('gzip')`. Opened
directly from `file:`, it rendered Mermaid and requested only that file and a
local `blob:null/...` module: no network or loopback service. The export CSP
denied connect, font, object, frame, base, and form sources; permitted data
images and bounded inline style; and admitted only the hashed bootstrap plus
its local blob module. Document, selected utility theme, and safe rendered
output were present; session capability, cookie, profile path, review chrome,
event log, feedback/history, and other private state were absent by
construction. Static semantic content remains readable if JavaScript or gzip
decompression is unavailable.

This adds one explicit compressed universal export payload, not a second raw
asset corpus, and no Rust decoder. Browser heap during full Mermaid export was
not measured and remains a production/release test.

### Earned initial fitness budgets

The first hard budgets include 20–31% bounded headroom over the best coherent
measured packaging. The combined-binary budget uses the measured gzip+Brotli
binary as a conservative proxy until the exact production service-plus-export
artifact exists; the production gate measures the real binary before release.
Raising a budget requires new measured ADR evidence:

- raw build corpus: 5,000,000 B; largest raw chunk: 850,000 B;
- embedded service Brotli corpus: 1,150,000 B; largest Brotli chunk: 150,000 B;
- service-corpus Rust binary delta: 1,250,000 B;
- dedicated gzip export payload: 1,300,000 B;
- generated empty-document export shell: 1,750,000 B;
- combined service/export embedded binary delta: 2,600,000 B;
- prose page: 7,500 B; one supported-code page: 75,000 B; and basic flowchart
  page: 250,000 B, all measured as encoded response bodies.

## S2 — static untrusted HTML containment

The adversarial block was served as a separate sandbox document, not `srcdoc`.
The parent used an iframe `sandbox` with neither `allow-scripts` nor
`allow-same-origin`; the sandbox response used its own CSP, and the application
cookie was scoped to `/app`.

Observed in Chrome 150 on macOS:

- the parent sentinel remained `parent-safe`;
- static content rendered and bounded inline CSS computed as `rgb(1, 2, 3)`;
- the sandbox request carried no application cookie and used the exact Host;
- a hostile image request was blocked by CSP; and
- the trap server received zero requests.

Production must additionally test script, form, navigation, download, popup,
top-frame, oversized content, cross-session ID, and CSP regression cases. The
result supports static HTML only; it supplies no evidence for `allow-scripts`.

## S3 — authenticated event channel

A bounded POST long-poll representing the final `/app/api/events/poll` route
required:

- exact `Host: 127.0.0.1:<port>`;
- exact `Origin: http://127.0.0.1:<port>`;
- `X-CF-Present: 1`;
- the HttpOnly session cookie;
- exact JSON content type; and
- a bounded request body.

The representative response succeeded with 62 bytes. Closing the session woke
the pending channel and later polls returned `410 Gone`. This validates the
transport shape and avoids EventSource credentials in a URL. Production still
needs concurrency, timeout, slow-client, replay, stale-cursor, size, shutdown,
and malformed-request tests.

## S4 — DOM stability and exact re-anchoring

In the split-root browser spike, feedback rerender and theme change preserved
the same text-node object and live selected text. An explicit document revision
updated that same node and invalidated the old browser selection, which makes
capture-before-revision a required transition rather than an incidental detail.

The Rust selector spike used UTF-16 code-unit offsets plus exact quote, prefix,
and suffix context. Five tests covered:

1. exact same-revision anchoring;
2. unique cross-revision recovery, visibly flagged as re-anchored;
3. duplicate quote disambiguated by exact context;
4. ambiguous, missing, or deleted content becoming visibly orphaned; and
5. emoji offsets matching browser Range units.

No fuzzy match or first-match fallback is permitted. Production must also prove
the browser-to-Rust canonical-text mapping, cross-block rejection, nested
Markdown semantics, normalized line endings, bidi text, combining characters,
and concurrent revision behavior.

## S5 — one-time credential bootstrap

Chrome 150 on macOS opened an owner-readable local bootstrap file and submitted
a form capability to an exact loopback endpoint. The server observed an exact
Host, `Origin: null`, no Referrer, no cookie, and form content type. The
transition set an HttpOnly SameSite=Strict cookie, and the next `/app` request
carried it. The capability appeared in no URL, referrer, or process argument.

This is one native Chrome/macOS canary, not cross-platform launch qualification.
Firefox discovery through the available Playwright system route did not produce
a qualified launch and is not claimed. Production removes the bootstrap on
success, expiry, failure, and stale recovery; unsupported native routes fail
closed or use explicit `--no-launch` until TSK-007 records their own evidence.

## Model checkpoint and decision gate

Claude Fable 5 at xhigh effort independently reviewed the initial runtime
proposal in a native interactive session. It requested changes before any
dependency or schema lock. A same-session reconciliation then approved the
hybrid renderer, thin CLI / dedicated crate, one-time bootstrap, authenticated
long-poll, static sandbox boundary, platform-state authority, and exact selector
approach for ADR drafting, subject to S1–S5 evidence being embedded.

After the compressed-packaging and grammar follow-up, Claude re-reviewed the
exact corrected ADRs in the same native session and returned an unqualified
`approved` verdict. Its three findings—media authority, the atomic typed review
envelope, and manifest-owned YAML/TOML coverage—are resolved in the accepted
texts and remeasured evidence. Codex independently re-derived the security,
size, export, schema, state, and product-boundary decisions and also approves
ADR-0049 and ADR-0050. Production implementation may begin under their recorded
gates. TSK-007 owns native runtime qualification beyond the explicitly scoped
canaries above.
