# TSK-444 option notes

Phase 1 evidence for AC-1: the candidates, how they were rendered, what the
renders showed, and what was not verified. The design intent is in
`DESIGN_INTENT.md`; the review document is `tsk-444-design-direction.json`.

## The options, one line each

| Part | Option | One line | Recommended |
|---|---|---|---|
| Page anatomy | A1 head and headings | title, summary and meta at the top of the document column; a `##` heading's first paragraph is its lead; no schema change | yes |
| Page anatomy | A2 head with anchor lines and section plates | the head adds four anchor lines; a numbered section plate (kicker, title, lead, hairline) needs a section block | |
| Type scale | S1 kit scale | 30 / 20 / 22 / 16.5 / 14 / 15 / 13 px; tokens.css unchanged, present adopts it | |
| Type scale | S2 wider steps | 34 / 19 / 24 / 18 / 15 / 15 / 13 px, about 1.33 per step; tokens.css type values change for both surfaces | yes |
| Figure colour | C1 accent governs | tokens byte-identical; one governing region per figure in accent-soft with an accent edge; the covered cell in accent | |
| Figure colour | C2 accent and two companions | two category inks and tints per skin and mode for peers the subject names; governing region stays accent; dash, width and cap untouched | yes |
| Figure colour | C3 tonal ramp | tokens byte-identical; two color-mix tints of the accent grade the regions below the governing one | |
| Section block | no | headings in narrative blocks, the lead styled by rule | yes |
| Section block | yes | a `section` block with title and lead, listed in the route | |

## How the candidates were rendered

- The four reference figures (structure, layering, sequence, coverage) are
  the portal starter's fixtures, drawn by the grammar module
  (`figure-grammar.mjs` `renderFigure`) and then re-classed per candidate
  (`lab/build.py`). The runtime draws the baseline four as real `figure`
  blocks; the candidates are `html` blocks carrying an `fg-` copy of
  `figure.css` plus `lab/options.css`, because an html block refuses `cf-`
  classes and at-rules.
- C2's hues are derived live from the skin accent in OKLCH (relative colour
  syntax in `options.css`); the values proposed for `tokens.json` are the
  hex table below, computed by `hues.py` with the same rotations
  (Graphite +76 and +129 degrees, Slate to 225 and 295, Sage to 250 and 320)
  and the ink lightness lowered until it clears 4.6 to 1.
- `lab/lab.html` is the same content as a standalone page on the kit
  stylesheets, screenshotted per section in the six pairs
  (`captures/`); the document itself was opened with
  `codeflow present open --no-launch` and captured live in the six pairs
  over CDP (`lab/cdp.mjs`).

## C2 values proposed for tokens.json

Floors: ink 4.5 to 1 on the figure ground (`surface`) and on the canvas;
the figure ink (`fig-line`) 4.5 to 1 on every tint. All 24 values clear
them.

| Skin and mode | Role | Ink | Tint | Ink on ground | Ink on canvas | Figure ink on tint | Ink on its tint |
|---|---|---|---|---|---|---|---|
| graphite-light | fig-cat1 | `#4665a0` | `#e1ebfc` | 5.79 | 5.35 | 10.37 | 4.82 |
| graphite-light | fig-cat2 | `#7c548e` | `#f1e6f6` | 5.97 | 5.53 | 10.31 | 4.95 |
| graphite-dark | fig-cat1 | `#93b7f7` | `#2c3649` | 7.75 | 8.49 | 8.18 | 5.98 |
| graphite-dark | fig-cat2 | `#cfa3e3` | `#3d3043` | 7.49 | 8.20 | 8.32 | 5.88 |
| slate-light | fig-cat1 | `#00709c` | `#d6ebf4` | 5.29 | 4.75 | 8.94 | 4.49 |
| slate-light | fig-cat2 | `#7259b0` | `#e8e4f7` | 5.32 | 4.78 | 8.85 | 4.47 |
| slate-dark | fig-cat1 | `#69c5e7` | `#1c343d` | 8.38 | 9.21 | 8.81 | 6.68 |
| slate-dark | fig-cat2 | `#bbaaf1` | `#312d3f` | 7.91 | 8.69 | 8.98 | 6.42 |
| sage-light | fig-cat1 | `#2e6396` | `#dce9f8` | 6.05 | 5.72 | 8.96 | 5.10 |
| sage-light | fig-cat2 | `#794c83` | `#efe3f2` | 6.37 | 6.02 | 8.90 | 5.34 |
| sage-dark | fig-cat1 | `#88bff8` | `#273647` | 8.26 | 9.01 | 8.66 | 6.37 |
| sage-dark | fig-cat2 | `#d7a5e2` | `#3d2f41` | 7.89 | 8.61 | 8.78 | 6.17 |

## What the renders showed

- Every candidate figure drew in all six pairs; the live page contains no
  "not drawn" or "stopped at" text (probe in `cdp.mjs`).
- Today's inversion reproduces: block titles 18.4 px over a 17.9 px
  subsection; no title or summary on the page.
- C3's first draft put the mid tint darker than the governing region, which
  inverted the ramp; fixed so the owner is strongest (soft, then 55 percent
  and 22 percent of soft over ground).
- Slate light's first companion ink (`#0083a2`) measured 4.21 to 1 on the
  ground; lowering lightness to `#00709c` clears 4.75.
- The first present build renamed `cf-present` inside figure text to
  `fg-present`; fixed by renaming classes only.
- Two-column candidate grids inside the present column shrank figure text
  under the floor; the candidates are stacked full width in the document.
- The export (`codeflow present export`) draws the real figure blocks but
  shows html blocks unstyled in sandboxed frames, so it is not the evidence
  surface for the candidates; one export run under a virtual time budget
  reported "Figure drawing stopped at the local time budget" on the coverage
  figure, which did not recur live.

## Not verified

- Narrow (375 px) renders: the candidates carry only the wide composition.
- The compact and large sizes were reasoned from the floors, not captured.
- The operator's browser: captures are from headless Chromium; the isolated
  present browser was not driven.
- Portal pages: the candidates are present renders on the kit; no portal was
  built.
- The live capture is cut at 16000 px, so the bottom of the page (Part 4 and
  the feedback prompt) is verified from the earlier capture only.
