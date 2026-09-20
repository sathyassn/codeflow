# utility presentation: one craft, two surfaces

<!-- HOW layer. Graduated area page for the utility presentation system.
     Sources: ADR-0048, ADR-0049, ADR-0053, ADR-0063. The normative authoring
     doctrine is the shared skill resource; this page records the system's
     structure and contracts. -->

## Concept

One design system carries every utility surface CodeFlow renders. Agents
author the subject and the runtime owns the chrome. The same tokens, altitude
and stage grammar apply to a bounded review session and to a durable
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

The portal is a composed visual presentation of the sources with supporting
text, never the Markdown re-rendered; present is a composed review document,
never a chat answer restyled.

## Architecture

The doctrine is one shared file, byte-identical in both skills, with the
surface-specific contracts in each skill's own reference; the token values are
one contract enforced across the portal token sheet and the present stylesheet
by a Rust test.

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

Skins and faces: instrument (Archivo), editorial (Inter), ink (IBM Plex Sans);
the portal's bundled theme names map `signal` → instrument and `folio` → ink
at build time. The Display panel on both surfaces sets Font, Size (Compact
0.94 / Default / Large 1.12), Palette (Neutral / Cool / Warm) and Appearance
(Light / Dark / System) through root data attributes read before first paint.
Each Palette pill carries a canvas swatch and an accent swatch read from the
live custom properties of the skin it selects, so the control shows the three
families rather than naming them.

`codeflow present export --theme` accepts `editorial`, `instrument` and
`technical`. `technical` is the documented alias of `instrument`: both resolve
to the instrument skin and produce the same artifact.

## Technical

| Contract | Where | Verified by |
|---|---|---|
| Shared doctrine text | `assets/base/agents/skills/{cf-present,cf-docs-portal}/resources/utility-presentation-system.md` | scaffold parity tests; `codeflow doctor --check managed-drift` |
| Portal carriers | `docs-portal/scripts/lib.mjs` (`cf-stage` parser, altitude trio tablist, full-width `text` fences, tables, media) | adapter tests; `npm run browser:verify` |
| Present blocks | `crates/codeflow-present` document schema under `.codeflow/schemas/present/` | present contract tests |
| Type floors | micro 12.5 · caption 13 · ui 13.5 · body 15 px; measure 68ch; scales 0.94 / 1.00 / 1.12 | `utility-tokens.css` and `styles.css` values; token contract test |
| Token equality | 16 semantic roles × instrument/editorial/ink × light/dark, plus three typeface stacks and the mono stack | `portal_utility_tokens_match_present_skins` in `crates/codeflow-core/tests/manifest_consistency.rs` |
| Page classes | orient: claim + carrier; architecture: trio + stage; technical/records: tables and evidence; accepted ADRs exempt | rendered review per class; portal composition gate |
| Precedence | shared resource is normative; ADR-0053's design-intent note is historical evidence | ADR-0063 |

Unsupported carriers (tree and arbitrary diagram syntaxes on both surfaces;
Mermaid in the portal, where a fence renders as plain code) are not promised
anywhere; adding one requires an adapter or runtime change and an ADR.
