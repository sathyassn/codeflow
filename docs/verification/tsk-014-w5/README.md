# W5 — the successor board

W5 answers the findings an independent answer-blind observer returned against
**W4**. It does not rewrite W4. W4, W3 and W2 stay byte-identical under their own
locks, and `tools/verify.mjs` checks that on every run.

Read `NEXT.md` first if you are continuing the work, and `observer-state.md` if
you are deciding what this board's evidence is worth. **Do not open
`answer-key.md` if you are here to observe.**

## The shape of it

```
rubric.v5.md        the gates, pinned            registry.json     the declarations, pinned
  + .lock.json                                     + .lock.json
amendments.json     every divergence from the pinned files, with its reason
answer-key.md       the derived answers — closed to an observer until after answering
cases/              p1, p2, p3          three candidates, one per registered question
portal/             orient-model, record-trust, area-drilldown
directions/         a-primitives, b-scene, c-recipes
rejected/           d-authored-svg, e-raster, p3b-source-region-history — records, not candidates
baselines/          p1 p2 p3 d1 d2 d3   chat, Markdown and plain HTML for every scorable surface
renders/            90 PNGs — 15 artefacts x 3 widths x 2 modes
checks/             what the machine decided, per render
tools/              render.mjs, verify.mjs, pin.mjs, build-raster.mjs
```

## W4 → W5, one disposition per finding

Outcomes are **pending** everywhere. This session authored the board and may not
record an outcome for its own work; the repairs below are what was *built*, and
whether they worked is G-gate territory. `registry.json` carries the same table
in machine-readable form with `target_disposition` / `outcome` on every entry.

| Finding | Artefact | Built |
|---|---|---|
| F01, F10, E1 | portal/orient-model | The spine gets its own axis in its declared order, five distinct nodes, connectors crossing back to the layer each belongs to. All seven authority kinds drawn and named, banded by three. **A18 now re-derives both from the source and compares.** |
| F02, D17 | cases/p2 | Five lane states → four, each pair separated on at least two of three channels (interior, edge, overlay), none hue. The merge is *not* the pre-registered one — see A-01. |
| F03, D01, D02 | cases/p2, directions/b-scene | Lane identity survives narrow: initials strip, full names in the description, lane order stated once at the head of the band. |
| F04, D03, D12 | portal/record-trust | Reach rotates rather than vanishing: at narrow each decision is a measured horizontal extent over a shared all-claims scale. Decision heads no longer rotated — stacked, with leaders to their columns. |
| F05, D04 | portal/orient-model | At narrow the spine **leads** and the layers attach to it; the two layers the contract does not place on the spine are drawn as exactly that. |
| F06, D07 | cases/p3b | **Withdrawn**, not repaired. See A-03. |
| F07, D05 | portal/area-drilldown | Narrow stops being a matrix — each area names its own contracts. "No owner" gets a broken-bracket mark of its own at both bands, plus an owner row under the grid. |
| F08, D10, D11 | cases/p3a | The re-open ring means re-opened and nothing else. Cause is carried by geometry and terminal — dipped arc + arrowhead against straight entry + open square. Residual marks keyed. |
| F09 | cases/p2 | The stop carries the record's own reason, not only the cause class; and a per-lane foot answers the registered question directly, which W4 drew nowhere. |
| F11, D09 | directions/d-authored-svg | **Rejected.** Retained as a record with its W4 renders and no repair. |
| F12, D08 | directions/e-raster | **Rejected**, per the operator's direction. Kept as the zero-cost floor any direction must beat. |
| F13 | directions/c-recipes | The second binding varied — the five directions now sit at the stage this board's own evidence puts them (reached 2–4, distinct causes) instead of five identical rails. **Partial**: see below. |
| F14 | cases/p1 | Dependency arcs: one reduction instead of three stacked (1.9px, full opacity, own token). |
| D06 | cases/p2 | The claim label gets its own wrapped column. |
| D13 | portal/orient-model | Cadence and split-authority labels move to a fixed reserved shoulder, never hung off a bar whose length is data. |
| D14, D15 | cases/p3a | Titles wrap instead of truncating; the exterior column is drawn **once** for the band, not seven times for one datum. |
| D16 | directions/b-scene | The dependency edge was a 1px `2 3` dash in the softest line token — invisible in dark at partial coverage. New mid-contrast token, 1.75px, `4 3`. |
| F15 | board-wide | Rendering coverage closed (15 × 6, no gap). **Observation coverage is not**, and A16 says so explicitly. |

## What the gates found that no observer had to

The collision gate (`amendments.json` A-05) is new, because **every blocking Part
D defect was an overprint that passed every W4 gate** — nothing overflowed, two
things were simply drawn in the same place. On its first clean run it caught two
real overprints in this author's own repairs (13px deep between lane-foot columns
at the intermediate band, 2px between a header and the first claim block), then
four more introduced by later edits. All were fixed before any observer saw them.

Two of its own measurements had to be corrected first, and both corrections are
worth more than the gate:

- an axis-aligned test reported **174** hits, most of them rotated lane headers
  whose bounding boxes overlap while their ink never touches. True oriented-box
  intersection cut it to 48 real ones. A false-positive field hides real defects.
- verdicts taken on overlap **area** flag two stacked lines kissing by a fifth of
  a pixel across 200px, and miss a short label printed squarely on a glyph strip.
  Depth is the honest measure.

Likewise A15 first reported 730 marks under the floor because it measured
`min(width, height)` — which is 0 for every horizontal rule on the board. A line's
information-bearing dimension is its length. After the ruler was fixed, one real
failure remained (b-scene's 5.7px rail nodes), and it was fixed in the drawing.

### The gate that was not there at all

Independent verification then found that **A15's other half had never been
implemented**. The rubric requires `checks/channels.json` to record each declared
channel as present in the rendered markup; no such file existed, and the verifier
decided the question by reading `registry.channels` and asserting that the strings
`channel_a` and `channel_b` were non-empty. A declaration checking itself cannot
fail. A15 passed on nothing.

It is now decided from tokens measured off the render — paint kind, dash ratio,
shape vocabulary, overlay geometry — and it requires presence, discrimination,
**every pair of states separated by at least two channels**, and hue-independence
proved by the tokens being identical in light and dark. What it found, once it
could see:

- **portal/orient-model**: `yours` and `conditional` were identical on every
  non-hue channel. That pair was carried by **colour alone** — in the figure that
  also carried the factual spine error, and against a registry that declared two
  non-hue channels for it.
- **cases/p3a**: the re-open links carried no `data-state` at all, so the
  distinction was unaddressable by any probe. Grouping them exposed a second
  defect: the wide and intermediate bands had never received the terminal repair,
  so both states drew the same arrowhead and were separated by a dash and a hue.
- **portal/area-drilldown**: `consumes` vs `migrating` rested on the dash alone,
  and `consumes` vs `unowned` on the form alone.
- **cases/p2**: the overlay strike sat outside the element naming its state — the
  third channel was on screen and could not be pointed at.

All four are repaired in the drawing. The gate was then proved able to fail: with
`conditional` reverted to what W5 had shipped, A15 fails in all six contexts with
`"yours" vs "conditional" separated by nothing`. Details in `amendments.json`
A-07, A-08 and A-09.

### And the gate that was there but not bound

Independent review then found **two further fail-open defects in that repair**:

- the rendered-state selection ended in `|| true`, and compared against a field
  the registry does not have. Evidence was matched by state **name** alone and
  never confined to its distinction. Masked only because each affected page draws
  one material distinction with non-colliding state names — the guarantee the
  gate reported was false, and any future second distinction or shared state name
  would have cross-contaminated it silently;
- the gate proved "at least two of interior, edge, geometry, overlay differ",
  which is not what the rubric asks. It asks for the **declared** channels. A
  figure could have lost both of them and still passed on an unregistered
  difference in form or size.

Both are closed. Evidence is now selected by an explicit binding — artefact to
page and rendered distinction — in `amendments.json` A-10, and by nothing else.
Every registered `channel_a`/`channel_b` is tied to one named measured channel
and to an **exact per-state token map**, with the registry prose quoted verbatim
and compared character for character. Where the built encoding diverges, the
named amendment must carry a matching `authorises` entry: an amendment id alone
authorises nothing. Exhaustiveness runs both ways — an unbound rendered
distinction, an orphan binding, a state vocabulary that does not match exactly,
or a dropped registered channel all fail closed.

**This repair changed no drawing.** The 90-render corpus digest is byte-identical
to the previous verified corpus (`c1f21339f031b2d6`); the only markup change was
naming the two marks in `cases/p3a` that play the roles the registry's channels
describe, so that "an arrowhead against an open square" is tested against the
terminal rather than against a lumped overlay token.

Five negative controls, each mutating one thing and then reverted:

| Control | Result |
|---|---|
| hue collapse — `conditional` reverted to its shipped encoding | FAIL: `"yours" vs "conditional" separated by nothing` |
| wrong distinction mapping — p2 bound to another page's distinction | FAIL: unbound rendered distinction, orphan binding, empty state set |
| wrong channel mapping — orient `channel_a` measured as `geometry` | FAIL: no matching `authorises` entry, and every token mismatched |
| a registered channel dropped from the binding | FAIL: `registered channel_b is bound to no measured channel` |
| unauthorised divergence — `authorises` entry removed, amendment kept | FAIL: proving an amendment id alone does not authorise |
| binding misquotes the pinned registry | FAIL: `the binding misquotes the pinned registry` |

Recorded in `amendments.json` A-10 (the bindings) and A-11 (the defects).

## What is not done

Stated here rather than left to be discovered:

- **F13 is partial.** It asked for three things: variance in the reuse binding, a
  third recipe W4 never demonstrated, and a real extension exercise costing out a
  form the library lacks. Only the variance is built. The claim "reuse is proved"
  should be read as "the second binding now varies" and nothing more — the
  extension cost is still asserted in prose rather than demonstrated, which is
  precisely what F13 objected to about W4.
- **A19 is weak and stays weak.** `amendments.json` A-04 measures the gap: a ratio
  compares a composition with itself, so the worst offender passed at 1.87×.
  Writing a stronger gate after seeing which artefact it would have failed is
  post-hoc gate-writing; it is left for rubric v6.
- **Three portal families remain specified and not built** — `capability-reach`,
  `code-drilldown`, `search-find` — and are declared `scorable: false` with that
  reason, not quietly dropped.
- **No outcome is recorded for any repair.** All 40 registered dispositions carry
  `outcome: pending`, and A21 fails the run if any of them claims otherwise.

## Reproducing it

```
node tools/render.mjs     # 90 renders + every per-render check
node tools/verify.mjs     # section A, and only section A
```

Both were run twice, end to end, with byte-identical results — same render
corpus digest, same `checks/channels.json`, same verifier output: 90 renders, 0
axe violations, 0 frame failures, 0 remote requests, 0 console errors, 0
collisions; 35 gates pass, 0 fail, 1 not run. Only `checks/lifecycle.json`
differs between runs, because it records the per-run temporary profile path. The one *not run* is W3's `amendments.lock.json`,
which carries no single pinned file with a digest — reported as unverifiable
rather than as a pass, which is the distinction that section exists for.

`render.mjs` needs host state a restrictive sandbox denies; under denial it fails
at launch and writes nothing.
