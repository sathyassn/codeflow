# Utility presentation system

<!-- HOW layer. Graduated area page for the utility presentation system.
     Sources: ADR-0048, ADR-0049, ADR-0053, ADR-0063, ADR-0068. The normative
     authoring doctrine is the shared skill resource and the figure grammar
     beside it; this page records the system's structure and contracts. -->

## Concept

**One design system carries every utility surface CodeFlow renders.** Agents
author the subject and the runtime owns the chrome, so the same tokens,
altitude and figure grammar apply to a bounded review session and to a durable
repository guide.

It is not the design system of a product that consumes CodeFlow.

The two surfaces differ only in job. The portal is a guide to the project as
it stands: a composed visual presentation of the sources with supporting text,
never the Markdown re-rendered, and it points to decisions and work records as
folders. A present session is a composed review document, never a chat answer
restyled.

## Architecture

The doctrine is two files, copied into both skills by parity rather than by
reference, and the token values are one contract held equal across both
stylesheets. Two tests keep the copies honest.

The surface-specific contracts live in each skill's own reference, so a change
to one surface's gate never edits the shared file. Graphite, Slate and Sage
are the kit skins; Archivo, Inter and IBM Plex Sans are independent font
choices. An unset font is Inter on both surfaces.

## Technical

The runtime controls first, then the contracts and what verifies each one.

| Display control | Values | How it applies |
|---|---|---|
| Font | Archivo, Inter, IBM Plex Sans; the Palette row below selects the skin separately | root data attributes read before first paint, on both surfaces |
| Size | Compact 0.94, Default 1.00, Large 1.12 | the type floors below are never crossed |
| Palette | Graphite, Slate, Sage | each pill carries a canvas swatch and an accent swatch read from the live custom properties of the skin it selects, so the control shows the three families rather than naming them |
| Appearance | Light, Dark, System | set through the same root data attributes on both surfaces |

`codeflow present export --theme` accepts Graphite, Slate and Sage as lowercase values.

Older theme names still resolve: `instrument` and `technical` to Graphite,
`editorial` to Slate and `ink` to Sage, and in the portal `signal` to Graphite
and `folio` to Sage. Saved skin choices use the same mapping, and saved font
choices `instrument` and `editorial` become Archivo and Inter. Explicit font
choices are kept. Export keeps its `editorial` default, which resolves to
Slate.

| Contract | Where | Verified by |
|---|---|---|
| Shared doctrine text | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/utility-presentation-system.md` | scaffold parity tests; `codeflow doctor --check managed-drift` |
| Figure grammar | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/figure-grammar.md`: nine families, twelve rules, altitude contract, mark vocabulary, declaration field reference; `figure-grammar-specimens.md` beside it: one specimen per family, loaded when authoring a figure (decision record ADR-0068) | scaffold parity tests; the families table and the evidence-board pointer pinned in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Figure declaration | the schema in ADR-0068 as updated on 2026-09-24; the field reference is `figure-grammar.md` section 6 | the grammar module `docs-portal/scripts/figure-grammar.mjs` and its figure gate (`npm run browser:verify`, present `npm run check:figures`); `codeflow validate --portal` re-derives each fact and rebuilds each companion |
| Figure thresholds | text floor 12.5 px (token sheet); mark floor 9 px on the inner mark, narrow break 646 px, elongation ceiling 1.5, clearance 8 px and collision depth 1 px (grammar module); per-figure elongation ceiling with reason (portal configuration) | declared once by the project; the figure gate reads them |
| Portal carriers | `docs-portal/scripts/lib.mjs` (`cf-stage` parser, altitude trio tablist, full-width `text` fences, tables, media) | adapter tests; `npm run browser:verify` |
| Present blocks | `crates/codeflow-present` document schema under `.codeflow/schemas/present/` | present contract tests |
| Type floors | micro 12.5 · caption 13 · ui 13.5 · body 15 px; measure 68ch; scales 0.94 / 1.00 / 1.12 | `utility-tokens.css` and `styles.css` values; token contract test |
| Token equality | 27 kit roles × Graphite/Slate/Sage × light/dark, plus three typeface stacks and the mono stack | `portal_utility_tokens_match_present_skins` in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Page classes | explanatory: the altitude trio with a family figure in every panel and in every how-to section, chosen by the altitude contract; illustrated; pass-through with a named reason; derived lookup, generated from a closed set of derivations; records are folders, not portal pages (ADR-0064) | `codeflow validate --portal`; portal composition gate |
| Precedence | shared resource is normative; ADR-0053's design-intent note is historical evidence | ADR-0063 |

Unsupported carriers (Mermaid and any other diagram syntax on both surfaces:
the portal renders a Mermaid fence as plain code and present refuses a
`diagram` block; a hierarchy tree in the portal, where a structure figure
carries it) are not promised anywhere; adding one requires an adapter or
runtime change and an ADR. The family figure is the default form (ADR-0068)
and a rendered carrier on both surfaces. A portal page binds a declaration in `portal.config.json` and the
adapter draws it as a companion beside the unchanged source; raw HTML in a
source stays escaped, so a figure enters a page only through its declaration.
A present document carries the declaration in a `figure` block, which the
client draws with the same grammar module.
