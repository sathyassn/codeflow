# Utility presentation system

<!-- HOW layer. Graduated area page for the utility presentation system.
     Sources: ADR-0048, ADR-0049, ADR-0053, ADR-0063. The normative authoring
     doctrine is the shared skill resource; this page records the system's
     structure and contracts. -->

## Concept

**One design system carries every utility surface CodeFlow renders.** Agents
author the subject and the runtime owns the chrome, so the same tokens,
altitude and stage grammar apply to a bounded review session and to a durable
repository guide.

```cf-stage
subject sources | catalog blocks · repository Markdown @accent
->
shared doctrine | composition rule · page classes · supported carriers
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

The doctrine is one file, duplicated by parity rather than by reference, and
the token values are one contract enforced across both stylesheets. Read the
figure top to bottom as the chain that keeps the copies honest.

```cf-stage
utility-presentation-system.md | shared resource, byte-identical in cf-present and cf-docs-portal @accent
->
cf-present visual-craft | composition gate · Comment surface · feedback pipeline
cf-docs-portal visual-craft | page composition gate · layers · verification matrix
->
utility-tokens.css (portal) | 16 roles × 6 skin/mode pairs
styles.css (present) | the same values
->
manifest_consistency test | fails the build on any role drift @positive
caption: skill parity tests keep the two copies identical; the token test keeps the two sheets identical
```

The surface-specific contracts live in each skill's own reference, so a change
to one surface's gate never edits the shared file. Skins and faces are
instrument (Archivo), editorial (Inter) and ink (IBM Plex Sans); the portal's
bundled theme names map `signal` → instrument and `folio` → ink at build time.
The exact controls and the enforcement points are in Technical.

## Technical

The runtime controls first, then the contract table that authenticates them.

| Display control | Values | How it applies |
|---|---|---|
| Font | Archivo (instrument), Inter (editorial), IBM Plex Sans (ink) | root data attributes read before first paint, on both surfaces |
| Size | Compact 0.94, Default 1.00, Large 1.12 | the type floors below are never crossed |
| Palette | Neutral, Cool, Warm | each pill carries a canvas swatch and an accent swatch read from the live custom properties of the skin it selects, so the control shows the three families rather than naming them |
| Appearance | Light, Dark, System | set through the same root data attributes on both surfaces |

`codeflow present export --theme` accepts `editorial`, `instrument` and
`technical`. `technical` is the documented alias of `instrument`: both resolve
to the instrument skin and produce the same artifact.

| Contract | Where | Verified by |
|---|---|---|
| Shared doctrine text | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/utility-presentation-system.md` | scaffold parity tests; `codeflow doctor --check managed-drift` |
| Portal carriers | `docs-portal/scripts/lib.mjs` (`cf-stage` parser, altitude trio tablist, full-width `text` fences, tables, media) | adapter tests; `npm run browser:verify` |
| Present blocks | `crates/codeflow-present` document schema under `.codeflow/schemas/present/` | present contract tests |
| Type floors | micro 12.5 · caption 13 · ui 13.5 · body 15 px; measure 68ch; scales 0.94 / 1.00 / 1.12 | `utility-tokens.css` and `styles.css` values; token contract test |
| Token equality | 16 semantic roles × instrument/editorial/ink × light/dark, plus three typeface stacks and the mono stack | `portal_utility_tokens_match_present_skins` in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Page classes | orient, architecture and reference: the altitude trio with a figure in Concept, a stage on architecture pages; record pointer: one table of folders; records are not portal pages (ADR-0064) | rendered review per class; portal composition gate |
| Precedence | shared resource is normative; ADR-0053's design-intent note is historical evidence | ADR-0063 |

Unsupported carriers (tree and arbitrary diagram syntaxes on both surfaces;
Mermaid in the portal, where a fence renders as plain code) are not promised
anywhere; adding one requires an adapter or runtime change and an ADR.
