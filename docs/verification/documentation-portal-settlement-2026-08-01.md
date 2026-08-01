# Documentation portal direction settlement — 2026-08-01

This record closes the design and technical-direction questions researched by
TSK-008. It preserves why the selected direction won, what was rejected, and
which claims remain for TSK-009 to prove. SPC-005 is the normative contract.

## Assurance and inputs

Claude and Codex researched independently before seeing the other lane's
conclusion. Codex retained three runnable, rendered directions and a framework
feasibility study in
[`documentation-portal-research-2026-08-01.md`](documentation-portal-research-2026-08-01.md).
The Claude judgment primary inspected the exact Codex branch and led two
reconciliation passes. Their retained transcript digests are:

- independent Claude lane:
  `sha256:d0d9122ba2f844eb53e331bb703aa3c8d9d3dd88a0cfbd3f61849246e1817f5c`;
- first exact-branch reconciliation:
  `sha256:9a54f8b12be53fdf03580a6022b2811d6aca0221de50938eebe830bd962c8cef`;
- final settlement reconciliation:
  `sha256:9f961b039935b92d6d7367bd5e2c0ac16f3f5c50e68956f7fbe8d074ca664e54`.

The lanes converged on the core direction independently. Codex accepted the
final expanded contract. The Claude judgment primary accepted the same
contract and resolved the remaining toolchain, theme, version, agent-output,
and snippet-integrity details. No operator choice remains: EPC-005 Plan v2 was
approved by the operator and both primary seats on 2026-08-01, including this
portal utility and the downstream adoption sequence.

## Settled direction

The portal is a layered field guide:

```text
Orient
  guided purpose and task journey
        |
        v
System
  architecture islands only where a relationship merits exploration
        |
        v
Records
  evidence-adjacent decisions, work, verification, and source detail
```

- **Orient** uses an editorial register, numbered reader path, persistent
  source context, and task escape routes. It is the default entry.
- **System** uses a guide register. C4-flavored diagrams are bounded islands,
  not a universal page chassis; relationships, titles, descriptions, and
  notation must remain explicit.
- **Records** uses a reference register. Generated entity pages expose ADR
  status and amendments, work and verification relationships, and derived
  backlinks beside their evidence.

The experience is calm, evidence-forward, and quietly confident: editorial at
orientation, denser through system material, and exact in records. It does not
look like a generic documentation template, dashboard, or grid of prose cards.
Language stays plain, precise, neutral, topic-led, and source-grounded.

## Settled technical and evidence contract

- Astro Starlight plus Pagefind is the exact-pinned baseline, with a committed
  lockfile and a locked-branch install/build/audit check. The Node toolchain is
  opt-in and isolated; non-adopters receive no Node dependency. The source
  adapter preserves the possibility of a future generator without shipping a
  second implementation now.
- Authoritative sources are read in place. The disposable build model carries
  source path, source hash, built-from commit, and release/repository version
  when defined. V1 uses snapshot-at-commit versioning, not copied historical
  documentation trees.
- Strict CodeFlow IDs are auto-linked and reverse relationships are derived.
  Stale content remains visibly stale and leaves the search index. Broken IDs
  or links, duplicate routes, orphan pages, and missing provenance fail the
  build.
- Every code excerpt or source claim is extracted from a pinned range or
  generated metadata. A snippet manifest records its path, range, and content
  hash; drift fails the build. Plausible manually authored code attributed to
  a real source path is prohibited.
- V1 includes `llms.txt` and per-page Markdown twins generated from and checked
  against the same source graph as HTML.
- The utility provides exactly two portal-owned themes through one token
  contract. Each supports coherent light/dark modes; one is the default, the
  initial mode follows the system before paint, and an explicit override
  persists. Portal tokens and runtime components do not flow into cf-present
  or a consuming product.
- WCAG 2.2 AA is the implementation target, including focus-not-obscured and
  minimum-target-size requirements. Search must use an audited accessible
  pattern; diagrams and code themes must work in both themes and modes.

The product non-goal remains intact. The portal is a generated static
documentation artifact, not an operated CodeFlow GUI, TUI, runtime harness, or
product-design system.

## Corrective findings from reconciliation

The first exact-branch review found that attractive prototype evidence was not
enough: two technical fixtures attributed invented content to real paths. The
corrected prototypes now use:

| Finding | Corrected evidence |
|---|---|
| Invented `RunPhase` and `DelegateRun` under `delegate.rs` | Verbatim `WaitUntil` and `WaitResult` excerpt from `crates/codeflow-core/src/delegate.rs:24` |
| Invented `launch.json` and `terminal.json` state files | Real `settings.json`, `ready.json`, and per-turn `request.json`, `accepted.json`, and `result.json` layout |
| Capability count omitted the building capability | `13 shipped capabilities · 1 building` |
| Context rail named an unrelated repository commit | Stable prototype base `e96db64bc0de66c9cb7b76fcb8eab163963c0647` |

The shared fixtures also apply theme selection before stylesheet paint, treat
the theme switch as a single action rather than two competing state signals,
and expose search as a labeled search form with a live results region. These
fixture corrections do not substitute for TSK-009's full accessibility and
production qualification.

### Corrected-branch evidence

- `verify.mjs` compared the displayed code byte-for-byte with
  `delegate.rs:24-42`, confirmed every displayed delegate state filename exists
  in the implementation, derived the 13-shipped/one-building count from the
  capability registry, rejected the stale/fabricated tokens, checked pre-paint
  script ordering and search semantics, and resolved every repository source
  path: PASS, prototype digest
  `sha256:44a542234ad4c257a2200763821f7f1dc07327bcd3241ac6b68ae36b7d5fcaeb`.
- Playwright 1.62.1 with isolated headless Chrome 151 exercised search and live
  result status, theme action and reload persistence, mobile navigation state,
  every Atlas level, and the delegate inspector without a console warning or
  page error: PASS.
- Fourteen corrected desktop/mobile light/dark captures were regenerated. A
  direct Guided Path implementation capture and direct Atlas code capture show
  the two corrected findings; `rendered/SHA256SUMS` identifies the full set.
- `html-validate` passed all prototype documents. Media-type checks confirmed
  that every `.jpg` is a JPEG at its recorded desktop or mobile viewport.

## Rejected directions and alternatives

- A full System Atlas as the primary chassis: costly, architecture-first, and
  weak for new or sparse repositories.
- Evidence Notebook as the primary chassis: strong editorial rhythm but a
  long-form archive risk and weaker random access.
- VitePress in its evaluated state: unresolved supported-chain audit findings
  and a transition between stable and v2 alpha.
- Docusaurus: unnecessary version-copying and weight without a demonstrated
  concurrent-version requirement.
- Fumadocs and Nextra: additional application/theme coupling without a current
  benefit that justifies it.
- MkDocs Material, Zensical, mdBook, Zola, or hosted-only documentation
  products: lifecycle, maturity, source-format, component, local/private, or
  isolation constraints conflict with this contract.
- MDX or Markdoc syntax inside authoritative source files, Diátaxis categories
  forced into top-level navigation, decorative text-card grids, force-directed
  graphs, timestamp-only staleness, branch-name source links, and copied
  historical documentation trees.

## Claims deferred to TSK-009

TSK-008 selects and evidences the direction; it does not prove the production
portal. TSK-009 must still demonstrate the source adapter, snippet-drift and
staleness failures, ID and inverse-link generation, page provenance, agent
outputs, clean scaffold init/update behavior, byte limits, toolchain isolation,
locked dependency audit and upgrade rehearsal, incremental update behavior,
empty/monorepo/tiny-repository cases, and CodeFlow dogfood output.

It must also provide rendered fidelity across mobile, tablet, and desktop in
both themes and both modes; cross-browser behavior; no-flash mode persistence;
axe, Lighthouse, keyboard, screen-reader, target-size, focus-obscuration, and
measured-contrast evidence. Prototype HTML validation and screenshots are
bounded inputs, not those production claims.
