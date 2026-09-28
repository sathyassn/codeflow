# `cf-present` browser assets

This directory is the maintainer-only source and build boundary for the local
`cf-present` review surface. Consumers, normal Rust builds, cross-compilation,
and presentation sessions use the committed files in `../assets/`; they do not
need Node or npm.

## Ownership boundary

- Rust owns `#cf-present-document`, its semantic blocks, canonical review text,
  schema validation, state, authentication, and service policy.
- Preact mounts only in the sibling `#cf-present-chrome`. Chrome updates must
  not replace or rerender the document root.
- Syntax highlighting and figure drawing are direct, bounded DOM enhancement.
  Each loads only when a corresponding block approaches the viewport. The app
  entry's dynamic imports are exactly the syntax, figure and fonts chunks.
- Service requests are same-origin paths below `/app`, carry the static
  `X-CF-Present: 1` header, and rely on the HttpOnly session cookie. No bearer
  value belongs in this bundle or its configuration payload.

The exact HTML attributes and configuration fields are defined in
`src/contracts.ts`, `src/selection.ts`, `src/syntax.ts`, and `src/figure.ts`.
Changing them requires a matching Rust change and contract test.

## Maintainer workflow

Use the official Node 26.4.0 distribution and npm 11.17.0, as pinned in
`package.json`. The build also checks the bundled zlib and Brotli versions:
system-library builds can report the same Node version but emit different
compressed bytes. CI's `actions/setup-node` uses the official distribution.
On a deliberate toolchain upgrade, requalify compression and regenerate all
assets together. The two-build, byte-for-byte check remains the final proof;
version checks alone do not prove reproducibility. Direct Node invocation has
no npm operation to check; npm invocations must carry the pinned npm version.

```text
npm ci --ignore-scripts
npm run build
npm run supply-chain
npm run check
npm run check:browser
npm run check:figures
```

`check:browser` uses a fresh headless browser profile and never attaches to the
operator's active browser. Set `CF_PRESENT_BROWSER` when the qualified browser
is not in one of the explicit platform locations in the script.

The checks that drive the real binary (`check:entities`, `check:figures`,
`check:forms`, `check:matrix` and `check:real-browser`) use
`CF_PRESENT_CODEFLOW` when it is set, else the debug build in
`CARGO_TARGET_DIR`, else `target/debug/codeflow` in the repository.

`check:figures` needs that built binary. It opens a document of
the figure specimens through the real binary, exports it in light and dark, and
reads each drawn figure with the portal's figure probe at 1280 and 390 px. The
rule outcomes must equal the portal's specimen table. `src/figure-grammar.mjs`
and `src/figure.css` are byte copies of the portal's grammar module and the
design kit's figure sheet; edit the originals and copy them here.

Commit the exact lockfile together with all generated changes under
`../assets/`. Never hand-edit generated payloads, the integrity manifest, audit,
SBOM, or license inventory. `npm run check` performs two clean builds and
compares them byte-for-byte with the committed service/export tree.

## Distribution contract

`../assets/manifest.json` is versioned and carries stable logical identifiers:

- `present.app` — the split browser entrypoint;
- `present.style` — the complete utility stylesheet;
- `present.prepaint` — the inline source and CSP hash used before first paint;
- `present.export` — the separate all-feature gzip renderer for standalone
  export.

Service payloads are content-hashed and committed only as Brotli. Their public
request path and private stored path are distinct manifest fields. The export
renderer is one deterministic gzip payload; it is not a service fallback.
Raising a build budget requires new measured ADR evidence.

The browser check covers prose-only lazy loading, code highlighting and figure
drawing, zero CSP violations, zero non-loopback requests, Rust-document node
identity, UTF-16 selection, both themes in light and dark modes, WCAG-tagged
axe checks, and 320 CSS-pixel reflow. The broader platform, assistive-technology, browser-launch, print, and
render matrix remains the task-level native verification boundary.
