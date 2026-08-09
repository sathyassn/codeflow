# Pass 10 → product reference contract

**Date:** 2026-08-08  
**Status:** normative craft/interaction lift for EPC-005 presentation recovery  
**Source:** design-exploration pass10 interactive board (not shippable HTML)  
**Architecture:** ADR-0049, ADR-0053, DESIGN_INTENT 2026-08-07

## Separation of planes

| Plane | Owns | Does not own |
|-------|------|--------------|
| Utility presentation design system | Tokens, altitude, stage, Comment SM, themes for present + portal | Product brand, consumer UI kits |
| cf-present | Session review document + Comment lifecycle + feedback envelopes | Durable docs, portal search |
| cf-docs-portal | Source-linked durable guide, layers, twins, evidence | Session Comment, verdicts |
| cf-design | Generic product/UX design craft for **any** surface | CodeFlow utility tokens, present/portal chrome specifics |

`cf-design` must stay product-generic. Utility authoring doctrine lives in
`cf-present` and `cf-docs-portal` only.

## Shared craft (both profiles)

- Semantic tokens: canvas/surface/ink/line, accent, proof/positive, warn, danger, lineage hues when needed
- Appearance: light / dark / system
- Type roles: display, prose, label, mono-evidence
- Altitude: concept → architecture → technical (each level complete)
- Stage grammar: labeled nodes, named edges, full-width stage with margins
- Diff / code conventions from utility tokens
- Anti-patterns: prose-in-pretty-boxes as “visual”; Mermaid/ASCII as primary page; permanent `+` on every block; free-form agent HTML

## cf-present Comment contract

1. **Single Comment mode** — one control arms annotation; not permanent per-block `+`.
2. **Gesture priority:** text selection → region drag → element click.
3. **Flow:** gesture → float (optional) → composer / note body → notes rail → **Submit review**.
4. **Annotatable surface:** Rust-owned `#cf-present-document` only. Chrome, settings, float, markers, rail are non-annotatable.
5. **Markers:** numbered speech-style marks while notes pending; re-measure anchors on resize.
6. **Esc ladder:** composer → float → exit Comment mode. Notes may remain queued (count badge), but the **notes rail only appears while Comment mode is on**.
7. **Keyboard:** `C` toggles Comment **only inside an open present session window**. Never jumps from portal/other apps. Ignore when focus is editable. `⌘/Ctrl+Enter` saves note body when editing. Existing review keymap (`j`/`k`/`r`/`e`) remains for the notes rail when open.
8. **Submit** posts the existing `/app/api/reviews` payload (verdict + notes). Harness-agnostic: any client that runs `codeflow present feedback` receives the envelope (Claude Code, Codex, Grok CLI, etc.).
9. **Shortcut scope:** present session only — demo “jump to present” is not product behavior.

## Feedback → harness (any harness)

```text
browser Submit review
  → POST /app/api/reviews (session-authenticated)
  → session store append FeedbackEnvelope
  → codeflow present feedback <session-id> [--follow]
  → stdout envelope (event_id, notes, verdict)
  → harness includes envelope in active turn
  → codeflow present resolve … addressed|dismissed
```

Delivery is at-least-once by `event_id`. No harness-specific transport.

## cf-docs-portal

- Same craft tokens overlaid on Starlight; no Comment lifecycle required.
- Layers: concept / architecture / technical from source-in-place.
- Agents maintain repository sources; portal is derived (adapter + evidence).

## Implementation mapping

| Pass10 element | Product owner |
|----------------|---------------|
| Document column | Rust `#cf-present-document` |
| Comment chrome / rail / markers | Preact `#cf-present-chrome` |
| Display settings | Chrome config + appearance |
| Submit review | Existing review API + CLI feedback |
| Demo Codex toggle | Content only (status block), not shell chrome |

## Verification bar

- Present web `browser-check` (selection, element, region, limits, axe, reflow)
- Present Rust service/session tests
- Portal `browser:verify` / validate when portal craft changes
- Dual-lineage review of skills and integrated delta
- Operator dogfood: open present → comment → `present feedback` reaches harness
