# `cf-present` visual craft (utility presentation system)

Load this when authoring or revising a present document so structure, type,
diagrams, and chrome use the **utility** system—not free-form HTML, not the
consuming product’s brand, and not a parallel design language.

Normative product intent:
[`docs/verification/utility-presentation-system-design-intent-2026-08-07.md`](../../../../docs/verification/utility-presentation-system-design-intent-2026-08-07.md)
and
[`docs/verification/pass10-reference-contract-2026-08-08.md`](../../../../docs/verification/pass10-reference-contract-2026-08-08.md).
Stack: ADR-0049 (Rust document / Preact chrome), ADR-0053 (utility design system).

`cf-design` stays product-generic. This file is **utility-only** doctrine for
present sessions.

## 1. Compose into the system—do not invent a skin

| Do | Do not |
|----|--------|
| Choose blocks that match information shape | Restyle the same chat answer as “cards” |
| One primary visual carrier for the governing idea | Wall of equal-weight panels |
| Use utility themes / light·dark·system | Invent a one-off palette or font stack |
| Encode state with tokens + text | Colour alone as status |
| Prefer standard blocks | `html` as a mini design system |

Agents generate **content into system forms**. They do not ship custom CSS,
remote fonts, or product component kits inside present.

## 2. Altitude (depth grammar)

Structure the document so each altitude is **complete**, not a teaser dump:

1. **Concept** — thesis / outcome / decision. Prefer one full-width stage or
   primary diagram that still makes sense if sentences are removed.
2. **Architecture** — engineer-legible pipeline or structure: labeled nodes,
   named edges, full-width stage with breathing margins.
3. **Technical** — evidence panes: status, tables, code, diff, gates—not
   decorative wrappers around more prose.

Map altitude to block order, not to decorative section chrome. Readers should
get the governing idea in ~5s and an engineer-readable architecture figure in
~20s (DESIGN_INTENT rubric).

## 3. Type and reading roles

The renderer owns typefaces and scale. Author for **roles**, not font names:

| Role | Use for |
|------|---------|
| Display | Title / governing claim (short, literal) |
| Prose | Narrative that needs continuity |
| Label | Kickers, meta, stage labels, chip text |
| Mono / evidence | Code, diffs, IDs, measurements, paths |

- Prefer bullets for enumerable content; prose only when continuity matters.
- Titles stay literal and findable—no cryptic labels or promotional tone.
- Do not embed font files or override type via `html`. Optional project
  primitive tokens (via explicit `cf-customize` / config) may influence family
  names only through the closed import contract—not arbitrary CSS.

## 4. Themes and appearance

Present ships utility themes (e.g. **technical**, **editorial**) each with
**light / dark / system**. 

- Author content that works in both light and dark; do not rely on a single
  mode’s contrast.
- Export and session appearance choose utility theme + mode; never treat that
  choice as product brand authority.
- Semantic meaning uses system roles: surface, text, border, accent, focus,
  positive/pass, warning, danger—not one-off hex in prose.

## 5. UI elements that are already in the system

Use the block catalog as the UI kit:

| Need | Prefer |
|------|--------|
| Emphasis / risk | `callout` (warning / danger / success) |
| Peer options | `comparison` |
| Decision state | `decision` |
| Evidence matrix | `table`, `status` |
| Structure | `tree` |
| Flow / sequence | `diagram` (with `acc_title` + `acc_description`) |
| Exact change | `diff` / `code` |
| Secondary depth | `disclosure` or `tabs` only when progressive depth or true peers |

Chrome (Comment, notes rail, appearance, export/close) is **runtime-owned**. Do
not recreate it inside document blocks.

### Comment surface (interaction, not decoration)

- Single **Comment mode**; no permanent per-block `+` affordances.
- Annotatable: document root only. Chrome is non-annotatable.
- Gestures: text select → region drag → element pin; Esc ladder backs out.
- **Notes rail only while Comment mode is on**; queued notes keep a count badge
  when mode is off.
- Submit posts the session review API; any harness consumes via
  `codeflow present feedback` (Claude Code, Codex, Grok CLI, etc.).

## 6. Motion and animation

Present chrome may use short, restrained transitions (mode arm, rail, focus).
Document content:

- **Do not** depend on animation to convey meaning (no “wait for the animation
  to understand the architecture”).
- Diagrams and stages must read at rest (static geometry, labels, edges).
- Respect reduced motion: never encode unique information only in motion.
- Prefer no decorative motion in blocks; leave motion to qualified chrome.

## 7. Stage and diagram grammar

When the subject is relationship, sequence, or convergence:

- Full-width stage with margins; large labeled nodes; named edges.
- One governing path at rest; secondary crossings only if they teach.
- State changes that matter (e.g. offline lineage, reduced confidence) should
  change structure or labels—not only a colour tint.
- Mermaid/ASCII may support a diagram block; they are not a substitute for a
  subject-led composition when the claim needs a true stage.

## 8. Anti-patterns (fail closed)

- Prose re-housed in pretty boxes presented as “visuals”
- Permanent `+` / annotate affordances on every block
- Free-form HTML that invents a second visual language
- Product design-system components or brand packs inside present
- Cryptic headings, emoji personality, promotional filler
- Colour-only status without text or symbol
- Restyling chat output without a new information structure

## 9. Quick authoring checklist

1. What question does this surface answer better than chat?
2. What is the single primary visual carrier?
3. Concept → architecture → technical order complete?
4. Does the first view survive sentence removal?
5. Blocks only from the catalog; IDs stable across revisions?
6. Accessible labels on diagrams/media; contrast-safe in both modes?
7. Feedback prompt clear; no secrets/paths/credentials in the document?
