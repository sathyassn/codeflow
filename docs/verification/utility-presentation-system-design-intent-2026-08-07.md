# Utility presentation system — DESIGN_INTENT

**Date:** 2026-08-07  
**Status:** accepted for implementation (operator-settled hybrid)  
**Applies to:** `cf-present`, `cf-docs-portal`, CodeFlow/agent-os dogfood, consuming projects via CodeFlow  
**Does not apply to:** consuming products’ own product UI or brand

Exploration references (not normative sources of code):

- Grok passes 4–6 under `codeflow-epc-005-design-exploration/docs/verification/grok-utility-system-*`
- Claude Design project *Cf-present design concepts* — `CFU Compare Lane.dc.html`
- Rejected: prose-in-pretty-boxes; permanent `+` on every element; Mermaid/ASCII as primary page form

Stack decision: [ADR-0053](../decisions/ADR-0053-utility-presentation-stack-and-design-system.md).

---

## DESIGN_INTENT

```text
DESIGN_INTENT:
  APPLICABILITY: direction pass — CodeFlow utility presentation system
    (present + portal profiles); implementation on EPC-005 integration

  CREATOR_INTENT:
    Give agents and humans a shared way to present complex work:
    ephemeral session review (cf-present) and durable repository guide
    (cf-docs-portal). One craft language; two jobs. Consumers install
    utilities, not a forced product brand.

  AUDIENCE_JOB_CONTEXT:
    Solo operators and multi-model agents reviewing runs, plans, evidence,
    and navigating repository truth. Evidence: product.md users; SPC-004;
    SPC-005; design exploration 2026-08.

  EXPERIENCE_TARGET:
    Exact, calm, subject-led, full-width with breathing margins.
    Structure carries meaning before prose. Progressive altitude:
    Concept → Architecture → Technical — each level complete, not a dump.
    Quiet chrome; feedback on demand.

  LANGUAGE_AND_VOICE:
    Plain, direct, source-grounded; cf-editorial-review authority for
    substantial prose. No decorative emoji; no promotional personality.

  APPEARANCE_MODES:
    Light and dark; system preference with override where the surface
    supports it. Semantic type scale: Compact / Default / Large with
    legibility floors. Reduced-motion alternatives for any motion that
    encodes meaning.

  VIEWPORTS_AND_PLATFORMS:
    Desktop primary; intermediate and narrow must keep the governing idea.
    Web-only utilities; macOS / Linux / Windows / WSL as present host OSes.

  SYSTEMS_AND_CONSTRAINTS:
    Utility tokens and grammar only — not the consuming product's design
    system. Present: Rust owns document DOM; Preact owns chrome only
    (ADR-0049, ADR-0053). Portal: Astro Starlight + Pagefind generator
    with shared visual system overlaid (ADR-0048, ADR-0053). Offline,
    no remote assets in present renderer. shadcn/Base UI are pattern
    references, not required runtime dependencies.

  OPERATOR_DIRECTION:
    Hybrid craft: Claude Compare Lane for visual/narrative north star
    (system = two profiles + shared base; present = live run figure with
    lineage/confidence; portal = docs shell + layered page). Grok
    exploration for Comment interaction (icon mode, multi-target aim,
    edit notes, send all), settings (skin/scale), and distribution story.
    Stack: Preact + Starlight retained; no deferred “big migration.”

  RESEARCH_OR_REFERENCES:
    Design exploration passes; Claude CFU Compare Lane; team-preview only
    as craft calibration (not product content). Prior TSK-006/008
    settlements remain historical; lived demos failed subject-led bar
    and are superseded on craft by this intent.

  COMPOSITION:
    SHARED:
      - semantic tokens (ink, surface, line, focus, pass/warn/fail/open)
      - type roles (display, prose, label, mono-evidence, instrument)
      - altitude grammar (concept / architecture / technical)
      - stage diagram grammar (labeled nodes, named edges, full-width stage)
      - provenance / freshness chips
      - code/diff surface conventions
      - anti-patterns: prose-in-boxes as “visual”; Mermaid/ASCII as page;
        permanent + affordances; inventing a second visual language
    PRESENT PROFILE:
      - ephemeral session shell (meta, Comment, export, close)
      - concept: visual-first run thesis (dual lineage → spine)
      - architecture: engineer-legible pipeline; Codex off changes structure
        and confidence visibly
      - technical: paneled evidence (diff, gates, revision)
      - Comment: one mode; text select / element pin / region; edit on
        revisit; Send all notes to agent session; Esc backs out
    PORTAL PROFILE:
      - docs product chrome: left nav, crumbs, search, source pins
      - page content uses same altitude grammar under Orient / System /
        Records (or project IA)
      - architecture stages and records grids, full-width with margins
      - no session Comment lifecycle required; feedback may return to chat

  DIRECTIONS_CONSIDERED:
    - Ledger / Atlas / Workbench encodings (pass 1) — useful as L2 techniques
    - Altitude craft + full pages (passes 2–6)
    - Claude C1–C3 editorial pass — rejected (structural sameness)
    - Claude D1–D4 matrix — optional later exploration; not required to ship
    - Claude CFU Compare Lane — adopted as visual north star for three surfaces
    - Single React+shadcn monorepo for present+portal — rejected for fit
      (ADR-0053)

  SETTLED_DIRECTION:
    Shared utility presentation system with two profiles; altitude-first
    authoring; stage-led architecture; quiet Comment on present; Starlight
    portal with shared craft; Preact chrome; no product-brand pack.

  SYSTEM_SCOPE:
    Earned now: tokens, modes, type scale, stage grammar, present chrome +
    Comment, portal shell theming, skill doctrine.
    Not earned: full app component marketplace; consumer product themes;
    shared React SPA for portal.

  ACCESSIBILITY_TARGET:
    WCAG 2.2 AA for web utilities; keyboard Comment paths; focus visible.

  REVIEW_RUBRIC:
    - 5s: governing idea without reading a wall of cards
    - 20s: engineer can explain architecture figure
    - Comment: no permanent +; edit + send-all work
    - Portal still reads as docs, not a vision site
    - Subject-led: structure survives sentence removal

  FIDELITY_EVIDENCE_PLAN:
    Browser quals for present; portal build + visual checks; skill/eval
    canaries for subject-led and anti-box rules; dual-model review of
    exact PR delta.
```

---

## Skill doctrine (implementation target)

Canonical skill resources (authoring authority when skills are installed):

- `cf-present/resources/utility-presentation-system.md`
- `cf-docs-portal/resources/utility-presentation-system.md`

When models use `cf-present` or `cf-docs-portal` (or agent-os equivalents):

1. **Load the skill resource first** — then profile `visual-craft` — then author.
2. Prefer the **utility presentation system** over free-form HTML.
3. Choose **altitude** and **form** from the subject (relationship → stage type).
4. Generate **content** into system forms; do not invent a competing visual language.
5. Fail closed on anti-patterns above (including text-only walls as “present”).
6. Product UI for the consuming app remains out of scope of these skills.

---

## Implementation mapping

| Workstream | Primary artifacts |
|------------|-------------------|
| Present craft + Comment | `crates/codeflow-present/web`, ADR-0049 chrome boundary |
| Portal visual system | `docs-portal` styles/layouts, shared token source |
| Skills | `.agents/skills/cf-present`, `cf-docs-portal`, `cf-design` (+ agent-os) |
| CLI / scaffold | theme/token wiring, portal starter assets as needed |
| PR | codeflow #433, agent-os #43 |
