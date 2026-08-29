# `cf-present` prototype study — Codex independent lane

**Task:** TSK-006

**Date:** 2026-08-01

**Scope:** research and disposable prototypes only; no production renderer or
scaffold code

**Decision status:** independent Codex evidence retained; the exact dual-model
direction is settled in
[`cf-present-design-settlement-2026-08-01.md`](cf-present-design-settlement-2026-08-01.md)

## Outcome

Three materially different directions were built against the same representative
CodeFlow decision content, rendered in light and dark at desktop and phone
widths, and exercised as interactive review surfaces. This independent lane
recommended **Review workbench** as the default `cf-present` experience. The
later cross-seat reconciliation retained that document-first shell and added
the manuscript lane's reading discipline as **the reviewer's document**.

The recommendation is not a generic dashboard. It is a document-first review
surface:

```text
section route | explanation / evidence / exact changes | feedback queue
              |                                        |
              +----- one attributable review state ----+
```

The central artifact remains readable without the side regions. Navigation and
feedback support the document; equal-weight cards do not replace its hierarchy.
At compact widths, navigation becomes a horizontally scrollable section strip
and the review queue follows the artifact or opens as an explicit sheet.

## Research synthesis

The evidence supports six decisions.

1. **Match the block to the information.** Narrative, comparison, code, flow,
   state, and visual evidence need different semantics. A visual earns its place
   by carrying relationship, sequence, scale, state, or appearance—not by
   placing prose in a decorated container.
2. **Keep primary meaning visible.** Progressive disclosure is useful for
   secondary detail, but important content should not be hidden by default.
   The document needs a readable default path before tabs or disclosures are
   introduced.
3. **Anchor feedback and also batch it.** GitHub's review flow demonstrates the
   value of comments attached to exact changes, a pending review, explicit
   submit, and visible unresolved/resolved state. For `cf-present`, the durable
   target should be document revision + stable block ID, with optional text
   quote/position selectors; a CSS path alone is too brittle.
4. **Reflow the document; contain truly two-dimensional material.** Narrative
   must fit a 320 CSS-pixel viewport without page-level horizontal scrolling.
   Tables, diffs, and diagrams may keep a two-dimensional representation in a
   labelled, keyboard-focusable local scroller.
5. **A closed document schema is the safety and reuse boundary.** JSON Schema
   should declare its dialect and version. The normal path renders typed blocks;
   it does not execute agent-authored HTML. Unsupported newer documents degrade
   to a complete read-only/raw view instead of silently dropping blocks.
6. **Use the platform before adding a component suite.** Native headings,
   landmarks, tables, lists, `details`, form controls, colour preference, and
   reduced-motion behavior cover much of the surface. A rendering library may
   organize state and reusable blocks, but must not import a house style or a
   consuming product's component system.

### Relevant current implementations

- `lavish-axi` at commit `7c64184adce8b2b18c1cb072779305303b8079d9`
  demonstrates local review, element/text annotations, queued feedback,
  presence, end attribution, export, layout diagnostics, and a durable prompt
  inbox. CodeFlow should learn from those interaction ideas, not copy its
  arbitrary-HTML/file-path identity or Tailwind/DaisyUI/React/Excalidraw
  dependency shape. SPC-004 deliberately uses a validated document, UUID
  sessions, private project-keyed state, and one tightly sandboxed escape block.
- GitHub review demonstrates intent context, line-specific comments, pending
  batches, approve/request-changes verdicts, re-review, and explicit resolution.
  `cf-present` needs similarly legible states without pretending every
  presentation is a pull request.
- W3C Web Annotation provides useful selector concepts. V1 should borrow only
  the minimum stable target tuple, not adopt the full JSON-LD model.

## Candidate directions

All three candidates contain an outcome, contextual explanation, comparison,
task plan/flow, code diff, diagram, evidence envelope, review state, and
attributable feedback entry. The content is intentionally identical enough to
expose layout strengths rather than reward a candidate for better prose.

### A. Decision briefing

**Concept.** A calm editorial reading path with a large outcome, sticky outline,
numbered sections, and margin-style annotations.

**Strongest fit.** Explanation, proposal, postmortem, or decision narrative in
which comprehension precedes critique.

**Evidence from the render.** The opening outcome and long-form hierarchy are
the clearest of the three. Whitespace and rules convey pacing without card
grids. Light and dark remain coherent; the phone layout preserves the reading
order and confines the outline, table, flow, and diff scrollers.

**Why it is not the default.** The feedback queue is not persistently visible
without weakening the reading measure. It makes review feel secondary, and a
large title spends too much initial viewport on dense evidence work. Retain the
idea as an optional narrative arrangement, not the universal shell.

Renders:
[light desktop](evidence/tsk-006/renders/briefing-light-desktop.webp),
[dark desktop](evidence/tsk-006/renders/briefing-dark-desktop.webp),
[light mobile](evidence/tsk-006/renders/briefing-light-mobile.webp),
[dark mobile](evidence/tsk-006/renders/briefing-dark-mobile.webp).

### B. Review workbench — Codex recommendation

**Concept.** A central document with a low-noise section route and a persistent
feedback queue. The content owns most of the width; side regions stay narrow and
visually quiet.

**Strongest fit.** Plans, implementation reviews, comparisons, evidence-backed
recommendations, and complex answers that expect one or more feedback rounds.

**Evidence from the render.** It gives the fastest path between rationale,
exact change, evidence, and unresolved feedback. The selected option is shown
inside the comparison rather than repeated as a decorative status card. The
three-column desktop layout collapses without losing document order; local
two-dimensional blocks scroll without widening the page.

**Material risk.** Persistent rails can become dashboard chrome or squeeze the
artifact. The production contract should cap their width, make both subordinate
to the document, and collapse them at a content-driven breakpoint. A simple
reply must never invoke this shell.

Renders:
[light desktop](evidence/tsk-006/renders/workbench-light-desktop.webp),
[dark desktop](evidence/tsk-006/renders/workbench-dark-desktop.webp),
[light mobile](evidence/tsk-006/renders/workbench-light-mobile.webp),
[dark mobile](evidence/tsk-006/renders/workbench-dark-mobile.webp).

### C. Decision map

**Concept.** A graph-led path in which consequence and dependency nodes are the
primary navigation; a contextual inspector carries details and feedback.

**Strongest fit.** Architecture, dependency, incident, rollout, and decision
work whose meaning is genuinely topological.

**Evidence from the render.** It makes the partition and dependency decisions
more legible than either document-shaped candidate. On a phone, the two-sided
path becomes one vertical spine without page overflow.

**Why it is not the default.** Many explanations are not graphs. Imposing a
spatial metaphor would fabricate relationships, increase authoring decisions,
and make comparison/code content harder to scan. Preserve it as a flow/graph
block or an earned document template.

Renders:
[light desktop](evidence/tsk-006/renders/map-light-desktop.webp),
[dark desktop](evidence/tsk-006/renders/map-dark-desktop.webp),
[light mobile](evidence/tsk-006/renders/map-light-mobile.webp),
[dark mobile](evidence/tsk-006/renders/map-dark-mobile.webp).

## Independent technical analysis for TSK-011

This lane examined implementation options, but TSK-006 does not authorize
production code or select a renderer. The settled design requires a
selection/annotation-compatible inspectable document boundary; TSK-011 owns a
bounded spike and Tier-3 ADR across the qualified options.

| Option | Strength | Material concern | Codex position |
|---|---|---|---|
| Rust-rendered HTML + bespoke controller | Small runtime and direct security ownership | Review, revision, annotation, and block state can become an untyped DOM state machine | Keep as the fallback baseline; do not assume it stays smaller |
| Lit + TypeScript, prebuilt and embedded | Small standards-based component layer; declarative reusable blocks; no Node runtime | DOM composition must prove selection, annotation, theming, and test boundaries | Qualified spike candidate; not selected here |
| Preact + TypeScript, prebuilt and embedded | Familiar state/test ecosystem and inspectable DOM composition | Easier to grow a general app/framework dependency surface than this closed utility needs | Qualified spike candidate; not selected here |
| Agent-authored HTML plus injected editor SDK | Maximum visual freedom | Rebuilds every response, expands trust surface, and bypasses the typed catalog | Reject for the normal path; SPC-004 already confines one sandbox escape block |

Proposed split of responsibility:

```text
Rust
  schema validation · service/auth · revisions · feedback/history · retention
       |
       v
embedded, pinned browser bundle (no Node at runtime)
  semantic block rendering · modes · interaction · local annotation capture
       |
       v
strict optional renderers
  safe Markdown · fine-grained code highlighting · strict/sandboxed diagrams
```

- The browser bundle must be reproducible and version-pinned. It must make no
  remote request; fonts, themes, grammars, and renderer assets are embedded.
- Normal Markdown disables raw HTML or sanitizes it through one audited path.
  Diagram rendering keeps Mermaid at `strict` or a sandboxed boundary and
  rejects content-level attempts to weaken the renderer policy.
- Code highlighting should use only the languages/themes the document requests
  from a fine-grained bundle; shipping every grammar is unnecessary.
- Annotation identity is `{session UUID, document revision, block ID}` plus an
  optional text quote/position selector. Revisions can mark a target outdated;
  absence or a CSS-path mismatch never silently resolves feedback.
- Native semantic components come first. A third-party design-system package is
  not warranted for v1 and would blur CodeFlow utility appearance with product
  design authority.

## Representative document cases for the block catalog

1. Short answer that stays in chat and does not invoke `cf-present`.
2. Concept or recommendation with a clear outcome and optional deeper evidence.
3. Multi-step plan with dependencies, decisions, and unresolved questions.
4. Option comparison with an explicit selected/rejected rationale.
5. Code review with file/diff evidence and line- or block-anchored feedback.
6. Verification report that distinguishes observed, inferred, passed, failed,
   and not-run claims.
7. Product/UX review with real screenshots or media and region annotations.
8. Graph-shaped architecture, incident, or rollout explanation.
9. Mixed document containing narrative, table, code, flow, evidence, and
   feedback without converting every section into a card.
10. Unsupported schema version that remains completely inspectable in a safe
    read-only/raw representation.

## Settled utility `DESIGN_INTENT`

The Codex candidate above was superseded through independent Claude research,
Claude-led rendered critique, Codex feasibility counter-review, and exact
cross-seat reconciliation. The durable verbatim contract and approvals are in
[`cf-present-design-settlement-2026-08-01.md`](cf-present-design-settlement-2026-08-01.md).

## Prototype evidence

The disposable source is
[`prototype.html`](evidence/tsk-006/prototype.html),
[`prototype.css`](evidence/tsk-006/prototype.css), and
[`prototype.js`](evidence/tsk-006/prototype.js). It is evidence, not a production
starter. The 12 optimized WebP renders total 488 KiB.

An isolated Playwright 1.62.1 + Chrome run used one fresh browser context per
direction/mode/viewport. Axe-core 4.10.3 checked WCAG 2 A/AA, 2.1 AA, and 2.2 AA
rules. The final 13-case result (12 render cases plus the preference/persistence
case) reported:

| Claim | Observed result |
|---|---|
| Light/dark × 1440×1000 and 390×844 | 12/12 rendered |
| Whole-page horizontal overflow | 0/12; page and body width equalled viewport |
| Automated WCAG-tagged violations | 0/12 after fixing contrast and focusable local scrollers |
| Console/page errors | 0/12 |
| Non-loopback requests | 0/12 |
| Annotation control moves focus to feedback | 12/12 |
| Feedback send updates live status | 12/12 |
| Manual mode toggle and exposed active/next state | 12/12 |
| Evidence marker matches explicit pass/fail/pending/not-run state | every retained evidence item |
| Diff element/sign/boundary semantics | every retained diff line |
| Workbench route changes from its initial section after scrolling | 4/4 workbench cases |
| Map inspector updates from a focused semantic control | 4/4 map cases |
| Reduced-motion computed scroll behavior | `auto`, 12/12 |
| Dark system preference + persisted manual override | dark initially; light after toggle and reload |

The initial Codex run found mobile page overflow from grid min-content,
unfocusable local diff/table/flow scrollers, and light-mode warning contrast.
Cross-seat reconciliation later identified decorative blanket evidence checks,
pointer-only map inspection, simulated section-route state, hidden mode state,
colour-led diff markup, and a host-font dependency. All were corrected and the
exact final surface was rerun. Visual inspection of all 12 final renders checked
hierarchy, mode coherence, information-bearing visuals, code/diagram
legibility, and mobile collapse.

Not verified in this disposable lane: production CSP/auth/sandbox/history,
screen-reader behavior, Windows high-contrast/forced-colours, RTL/localized
copy, print/export fidelity, or real text-selection rebasing across revisions.
TSK-011/TSK-007 own those claims.

## Task-level settlement

Claude and Codex approved the same exact `DESIGN_INTENT`; no operator decision
remains. Production renderer selection, real Mermaid integration, and the full
fidelity matrix remain explicitly owned by TSK-011 and TSK-007 rather than this
disposable research lane.

## Sources

- [W3C WCAG 2.2: Reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html)
- [W3C WCAG 2.2: Target Size (Minimum)](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html)
- [W3C APG: Disclosure](https://www.w3.org/WAI/ARIA/apg/patterns/disclosure/)
- [W3C APG: Tabs](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/)
- [W3C Web Annotation Data Model](https://www.w3.org/TR/annotation-model/)
- [GitHub: Reviewing proposed changes](https://docs.github.com/en/pull-requests/how-tos/review-pull-requests/reviewing-proposed-changes-in-a-pull-request)
- [GitHub: About pull request reviews](https://docs.github.com/en/pull-requests/collaborating-with-pull-requests/reviewing-changes-in-pull-requests/about-pull-request-reviews)
- [GOV.UK Design System: Details](https://design-system.service.gov.uk/components/details/)
- [JSON Schema: dialect and vocabulary declaration](https://json-schema.org/understanding-json-schema/reference/schema)
- [Lit 3 overview](https://lit.dev/docs/v3/)
- [Preact 10 guide](https://preactjs.com/guide/v10/getting-started/)
- [Mermaid security levels](https://mermaid.js.org/config/schema-docs/config-properties-securitylevel.html)
- [Shiki fine-grained bundles](https://shiki.style/guide/bundles)
- [`lavish-axi` repository](https://github.com/kunchenguid/lavish-axi)
