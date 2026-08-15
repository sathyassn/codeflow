# Utility presentation system — reference contract

**Date:** 2026-08-08  
**Status:** repository verification record (supporting)  
**Product name:** CodeFlow **utility presentation system** (ADR-0053)  
**Normative skill copies (prefer these when authoring):**

- `assets/base/agents/skills/cf-present/resources/utility-presentation-system.md`
- `assets/base/agents/skills/cf-docs-portal/resources/utility-presentation-system.md`

Exploration demos and historical board renders under design-exploration
(historically “pass 10”) are a **design reference** for **both** `cf-present`
and `cf-docs-portal`. They are **not** shippable product HTML, **not** a
document to clone into a present session, and are **not** named in
operator-facing skill doctrine. They informed settlement of this contract only.

The CLI and skills exist so each agent invocation **applies that craft to new
subject matter**: `codeflow present` for this-session explanation/review
(including the runtime-owned Comment system); `codeflow portal` for durable
docs for CodeFlow or any consuming project. Do not treat “show present” as
re-rendering the reference board.

**Architecture:** ADR-0049, ADR-0048, ADR-0053, DESIGN_INTENT 2026-08-07.

## Separation of planes

| Plane | Owns | Does not own |
|-------|------|--------------|
| Utility presentation design system | Tokens, altitude, stage, Comment SM, themes for present + portal | Product brand, consumer UI kits |
| cf-present | Session review document + Comment lifecycle + feedback envelopes | Durable docs, portal search |
| cf-docs-portal | Source-linked durable guide, layers, twins, evidence | Session Comment, verdicts |
| cf-design | Generic product/UX design craft for **any** surface | CodeFlow utility tokens, present/portal chrome specifics |

`cf-design` must stay product-generic. Utility authoring doctrine lives in
`cf-present` and `cf-docs-portal` skill resources only.

## Shared craft (both profiles)

- Semantic tokens: canvas/surface/ink/line, accent, proof/positive, warn, danger
- Appearance: light / dark / system
- Type roles: display, prose, label, mono-evidence
- Altitude: concept → architecture → technical (each level complete)
- Stage grammar: labeled nodes, named edges, full-width stage with margins
- Anti-patterns: prose-in-pretty-boxes as “visual”; Mermaid/ASCII as primary page;
  permanent `+` on every block; free-form agent HTML; text-only walls as “present”

## cf-present Comment contract

1. **Single Comment mode** — one control arms annotation; not permanent per-block `+`.
2. **Gesture priority:** text selection → region drag → element click.
3. **Flow:** gesture → float (optional) → composer / note body → notes rail → **Submit review**.
4. **Annotatable surface:** Rust-owned `#cf-present-document` only.
5. **Markers:** numbered speech-style marks while notes pending; re-measure on resize.
6. **Esc ladder:** composer → float → exit Comment mode. **Notes rail only while Comment mode is on.**
7. **Keyboard:** `C` only inside an open present session window.
8. **Submit** → session store → `codeflow present feedback` (any harness).
9. **Operator handoff:** bootstrap file path / openable link first, then session id.

## cf-docs-portal

- Same craft tokens overlaid on Starlight; no Comment lifecycle.
- Layers: concept / architecture / technical from source-in-place.
- Agents maintain repository sources; portal is derived.

## Implementation mapping

| Concern | Product owner |
|---------|---------------|
| Document column | Rust `#cf-present-document` |
| Comment chrome / rail / markers | Preact `#cf-present-chrome` |
| Display settings | Chrome config + appearance |
| Submit review | Review API + CLI feedback |
| Portal craft overlay | docs-portal styles / themes |

## Verification bar

- Present web `browser-check` (selection, element, region, limits, axe, reflow)
- Present Rust service/session tests
- Portal `browser:verify` / validate when portal craft changes
- Skill source/live/baseline byte identity (`cf-present`, `cf-docs-portal`)
- Dual-lineage review of skills and integrated delta
- Operator dogfood: open present → comment → `present feedback` reaches harness

---

## Supersession

This file supersedes `pass10-reference-contract-2026-08-08.md` naming. Do not
reintroduce exploration codenames in skills or operator handoffs.
