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

```cf-stage
subject sources | catalog blocks · repository Markdown @accent
->
shared doctrine | composition rule · page classes · supported carriers
figure grammar | nine families · twelve rules · altitude contract
->
present runtime | Rust document · Preact chrome · Comment
portal runtime | Starlight shell · adapter · Pagefind
->
one token contract | three skins × light/dark · type floors · Display panel @positive
caption: authors compose the subject; the runtimes render it under one craft
```

The two runtimes diverge only in job. The portal is a guide to the project as
it stands, a composed visual presentation of the sources with supporting text,
never the Markdown re-rendered, and it points to decisions and work records as
folders; present is a composed review document, never a chat answer restyled.

## Architecture

The doctrine is two files, duplicated by parity rather than by reference, and
the token values are one contract enforced across both stylesheets. Read the
figure top to bottom as the chain that keeps the copies honest.

```cf-stage
utility-presentation-system.md | shared resource, byte-identical in cf-present and cf-docs-portal @accent
figure-grammar.md | families, rules, altitude contract, declaration schema; specimens beside it @accent
->
cf-present visual-craft | composition gate · Comment surface · feedback pipeline
cf-docs-portal visual-craft | page composition gate · layers · verification matrix
->
utility-tokens.css (portal) | 27 kit roles × 6 skin/mode pairs
styles.css (present) | the same values
->
manifest_consistency test | fails the build on any role drift @positive
caption: skill parity tests keep the two copies identical; the token test keeps the two sheets identical
```

The surface-specific contracts live in each skill's own reference, so a change
to one surface's gate never edits the shared file. Graphite, Slate and Sage
are the kit skins; Archivo, Inter and IBM Plex Sans are independent font
choices. An unset font defaults to Inter on both surfaces.
The exact controls and the enforcement points are in Technical.

## Technical

The runtime controls first, then the contract table that authenticates them.

| Display control | Values | How it applies |
|---|---|---|
| Font | Archivo, Inter, IBM Plex Sans; the Palette row below selects the skin separately | root data attributes read before first paint, on both surfaces |
| Size | Compact 0.94, Default 1.00, Large 1.12 | the type floors below are never crossed |
| Palette | Graphite, Slate, Sage | each pill carries a canvas swatch and an accent swatch read from the live custom properties of the skin it selects, so the control shows the three families rather than naming them |
| Appearance | Light, Dark, System | set through the same root data attributes on both surfaces |

`codeflow present export --theme` accepts Graphite, Slate and Sage as lowercase values.

Compatibility: `instrument` and `technical` resolve to Graphite, `editorial` to Slate and `ink` to Sage; portal `signal` resolves to Graphite and `folio` to Sage. Saved skin choices use the same mapping; saved font choices `instrument` and `editorial` become Archivo and Inter independently. Unset fonts now use Inter (previously the portal chose Archivo or Plex from its skin, present chose Archivo, and export used system fonts first); explicit font choices survive. Export retains its `editorial` default, resolving to Slate, pending the separate default decision.

| Contract | Where | Verified by |
|---|---|---|
| Shared doctrine text | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/utility-presentation-system.md` | scaffold parity tests; `codeflow doctor --check managed-drift` |
| Figure grammar | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/figure-grammar.md`: nine families, twelve rules, altitude contract, mark vocabulary, declaration field reference; `figure-grammar-specimens.md` beside it: one specimen per family, loaded when authoring a figure (ADR-0068) | scaffold parity tests; the families table and the evidence-board pointer pinned in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Figure declaration | the schema in ADR-0068 as updated on 2026-09-24; the field reference is `figure-grammar.md` section 6 | the grammar module `docs-portal/scripts/figure-grammar.mjs` and its figure gate (`npm run browser:verify`, present `npm run check:figures`); `codeflow validate --portal` re-derives each fact and rebuilds each companion |
| Figure thresholds | text floor 12.5 px (token sheet); mark floor 9 px on the inner mark, narrow break 646 px, elongation ceiling 1.5, clearance 8 px and collision depth 1 px (grammar module); per-figure elongation ceiling with reason (portal configuration) | declared once by the project; the figure gate reads them |
| Portal carriers | `docs-portal/scripts/lib.mjs` (`cf-stage` parser, altitude trio tablist, full-width `text` fences, tables, media) | adapter tests; `npm run browser:verify` |
| Present blocks | `crates/codeflow-present` document schema under `.codeflow/schemas/present/` | present contract tests |
| Type floors | micro 12.5 · caption 13 · ui 13.5 · body 15 px; measure 68ch; scales 0.94 / 1.00 / 1.12 | `utility-tokens.css` and `styles.css` values; token contract test |
| Token equality | 27 kit roles × Graphite/Slate/Sage × light/dark, plus three typeface stacks and the mono stack | `portal_utility_tokens_match_present_skins` in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Page classes | orient, architecture and reference: the altitude trio with a family figure in every panel and in every how-to section, chosen by the altitude contract; record pointer: one table of folders; records are not portal pages (ADR-0064) | rendered review per class; portal composition gate |
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
