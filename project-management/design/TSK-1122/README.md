# The status dashboard prototype for the operator's walk-through

`prototype.html` is one self-contained page: the CodeFlow 4.0.0 status
report in the design the operator accepted on 2026-11-02 (Graphite, deep
green accent, two-level left menu, shared top bar with one display gear),
on real facts read once from the repository, GitHub and the Coordinator
trackers. It exists so the operator can click through the design before
the dashboard shell (TSK-1120), the topic reuse (TSK-1121) and the status
document (TSK-1098) build it. It is not a report to act on and nothing in
it ships.

Open it from disk in any browser; it needs no server. The hash names a
view (`prototype.html#blockers`) and the page routes on it when it loads.

## The rules this build follows

Rebuilt fresh on 2026-11-02 (17:40 to 20:03 EST) after the operator
rejected the previous build for walls of text. The old build and
`report.json` were data sources only; no layout was kept.

- No text block over two lines at desktop width outside a closed
  disclosure (`_src/textlines.cjs` measures every rendered block at 1440
  and 1280 px; its count must be 0).
- One point per bullet; a figure is the main view of every flow,
  comparison and status; every number carries its unit; names come
  before identifiers; every time is wall clock with a zone.
- An epic is one line, a strip of three short phrases (delivers, for
  whom, why now), a figure for progress, and sources in a collapsed
  disclosure. A task is one line, a stage figure, a strip of eight facts,
  and the whole Outcome in a disclosure.

## What to click, in order

1. Overview: the percent with its change in points, Figure 1 (where the
   246 tasks are), the three outcome cards, Figure 2 (the four streams
   at their step), four tiles, and what moved since the previous report.
2. An outcome card opens its epic view: one line, the strip, the
   capability chips, then Progress (the partition and the theme bars,
   each bar opens its theme), Order (ready, then waiting with the
   predecessor each waits on) and Issues (blocked tasks, open pull
   requests). Arrow keys move between tabs.
3. A theme bar opens the theme view; a task row opens the task view: one
   line, the stage path (todo, pull request, on the line, on main), the
   eight facts, criteria, dependencies, evidence. The breadcrumb names
   each level in words.
4. The rail's second icon: Epics (one card per epic), Lanes (a board by
   Coordinator and state), Evidence (the coverage matrix). The third:
   Your decisions and Script. The utility icon: the ten report kinds
   (nine open as navigable specimens with their own tabs), the docs
   portal layers, one docs topic whose three tabs each open with a
   figure, and the portal's "what changed" page.
5. Your decisions: four questions, each with its options, the
   recommended one marked, context behind a disclosure, and a box for
   changes you name. Every field has a Save button: the first save asks
   for the repository folder once, then writes
   `coordination/coordinators/visual-docs/published/status-prototype/answers.json`
   and confirms with the path and the wall-clock time. A draft of your
   typing stays in this browser across a reload. An answer also appears
   in the Script view's second part.
6. The gear: Mode, Themes, Accent and Text size. Comment toggles the
   comments rail; on desktop the page keeps a lane at its right edge
   for the fixed button, under 768 px the button sits at the end of the
   page, so it covers no content at any width.
7. The menu toggle collapses the panel to the rail. Hovering a rail
   icon then opens its panel after one 200 ms delay with no tooltip (a
   passing mouse opens nothing); keyboard focus opens it at once. A
   tooltip appears only on the pinned menu, where no panel opens.

## How it was built

Everything under `_src/` is the build; nothing in it runs in the product.

| Step | Command (from `_src/`) | What it does |
|---|---|---|
| gather | `node gather.cjs` | Reads the task and epic records on `origin/integration/EPC-022-the-3-2-0-milestone-fixes` and on `origin/main`, each record's last change on the line, `gh pr list` and `gh api compare` (read only) and `codeflow work next`; writes `facts.json` with the read's start and end time. `--refresh-records` re-reads only the record texts at the pinned tips. |
| report facts | `report.json` (by hand) | The deliverable line, names before identifiers, each epic's short phrases (`epicShort`: one line, delivers, for whom, why now, menu label, with its source), streams, lanes, blockers with their short do, why and else (`blockerShort`), decisions, owed actions, terms, the four questions and the ten report kinds, each with its source and read time. |
| build | `node build.cjs` | Writes `../prototype.html` (kit CSS and JS inlined, data embedded, every view rendered: 329 views, 13 of them feature views) and `kit/index.html` (the same page linking the kit files, for the workspace navigation test). `figures.cjs` holds the docs drawings. |
| text lines | `node textlines.cjs` | Opens the page from `file://` at 1440 and 1280 px, activates every view with every tab panel shown, and measures the rendered line count of every visible text block; fails on any block over two lines outside a closed disclosure. |
| render | `node render.cjs` | Opens the page from `file://`; captures 22 views at 1440, 1280, 1200 and 390 px in light and dark to `../captures/` from the top of the page (390 px also full page for the overview, an epic and a task); fails on horizontal scroll, a page error, a wrapped chip, clipped text, or the Comment button over content; checks Graphite, Slate and Sage with the default and large text size at 1200 and 390; checks every one of the 316 views at 1440, 1280, 1200 and 390 px at three scroll positions for the Comment button covering content; drives 49 journeys, among them the collapsed-rail hover (no tooltip, one delay, a passing mouse opens nothing, focus opens at once), Save through a mocked directory handle (file content and confirmation), the download fallback (file content and confirmation) and the draft surviving a reload. |
| axe | `node axe.cjs` | WCAG 2.x A and AA rules over 17 views at 1440, 1280 and 390 px in both modes, plus Slate and Sage and the large size at 1200; fails on any violation of any impact. |
| floors | `node floors.cjs` | Chromium resolves every HTML and SVG stage fill, including `.st-line-f`, and its matching main fill across every theme, mode and accent. Requires 3:1 against the surface and OKLab distance ×100 >= 15 for line/main pairs; prints effective colours and the worst separation. Other token contrast pairs remain source-derived. Run `build.cjs` first. |

The facts in the page were read from 2026-11-02 13:40 to 13:41 EST (line
tip `aa84d8544`, `main` `b2bb61af3`); the record texts were re-read at
the same tips at 16:25 EST and no count changed. The tracker facts in
`report.json` were read at 12:26 EST; the blocker evidence is from 12:33
EST and the Blockers view says so. The change marks compare this read
with the 2026-11-01 14:31 EST report (`_src/previous-2026-11-01-1431.json`
and `_src/previous-live-counts-2026-11-01-1431.json`).

The kit (`_src/kit/cf.css`, `cf.js`, `cf-icons.js`) is the round 2 kit:
the workspace kit of `coordination/coordinators/visual-docs/design/after/`
plus deep green as every theme's default accent, a `--stage-todo` token,
secondary text for table headers and tags, focus management for views,
the gear and the drawer. The page adds its own styles for the figures
(partition, bars, step dots, path, board, matrix), the fact strips and
the phone layout; TSK-1162 and TSK-1120 decide what the shipped kit
carries.

## How the Director reads the answers

Save writes `coordination/coordinators/visual-docs/published/status-prototype/answers.json`
in the agentic-systems workspace: `{ page, factsAsOf, savedAt, answers: [{ id,
label, text, value, page, time }] }`, one entry per answered field (`tree`,
`tabs`, `strip`, `marks`, `changes`), times as `YYYY-MM-DD HH:MM EST`. The
Director reads that file; each save replaces it with the current answers.
In Chromium browsers the page writes it through the File System Access
API (the folder handle is kept in IndexedDB for later saves, with a
permission prompt when the browser asks again). In Safari and Firefox,
Save downloads `answers.json` and the confirmation says so and names the
path to move it to. The Script view's second part remains the shell
route that records the same answers on `integration/workspace`.

## What is real and what is a specimen

Real: every count, percent, location, theme, last-changed time, ready and
waiting reason, pull request number and state, blocker, lane and decision,
each with its source in the Evidence view or the view's own disclosure.
The themes group an epic's tasks by the record section that introduced
them (TSK-1095 AC-15). Each epic's short phrases are written from its
record at the lines the disclosure names; the coordination ledger's "for
whom" comes from its Outcome because its record has no "It serves"
sentence, and the disclosure says so.

Specimens, and said so on each page: the nine report kinds other than the
status report open as views with one tab per view kind and layout in
place of data (figure families drawn with labels, lists, strips, a form,
a command block). The report and portal samples task (TSK-1127) builds the
real ones. The docs topic's three figures are drawn by hand, not by the
shared figure grammar.

Mocked or absent, and said so on the page:

- The runtime chrome (top bar, menu, gear, Comment) is the kit's
  prototype code, not `codeflow present`. Comment markers live only in
  the page.
- Lane and window per task read `not recorded`; TSK-1095's fields do not
  exist on the line yet.
- Epic verification reads "not recorded on the line".
- GitHub had not computed mergeability for most open pull requests at
  the read; the Issues tab shows what it returned.

## Checks, last run on the committed page

| Check | Result |
|---|---|
| `textlines.cjs` | 329 views at 1440 and 1280 px, 27 378 text blocks measured, 0 over two lines outside a closed disclosure |
| `render.cjs` | 248 renders at 1440, 1280, 1200 and 390 px in both modes plus Graphite, Slate and Sage at the default and large size: 0 horizontal scroll, 0 wrapped chips, 0 clipped text, 0 page errors; Comment button over content in 0 of 329 views at each of 1440, 1280, 1200 and 390 px; 55 of 55 journeys, rerun on their own after one script fix, the page unchanged |
| `axe.cjs` | 118 view renders: 0 critical, 0 serious, 0 moderate, 0 minor |
| `floors.cjs` | 168 token pairs and 192 rendered stage pairs over 3 themes, 2 modes and 2 accents: 0 under the floor. Line against main is 15.54 at worst (floor 15). The 3:1 check reads the 1 px outline for line marks, 5.14:1 at worst; the fill alone is 2.71:1 at worst and does not meet 3:1 |
| `audit-decisions.cjs` | 27 of 27: the operator's decisions AC-5 names, driven in Chromium |
| scan of the page text | Earlier result, not rerun: 19 788 text nodes, 0 "..." in content, 0 identifiers as the only label of a link. The zone journey now skips Context, which is the record's own words |
| workspace `textwall-check.cjs` | Earlier result, not rerun: 0 paragraphs over 3 lines |
| workspace `nav.test.cjs` on `_src/kit` | 1 of 1 pages pass every check |
| workspace `link-check.cjs` | Earlier result, not rerun: 4 references; the one "missing" is the string `${E(h)}` inside the inlined `cf.js` source, a JavaScript template, not a link |

All runs open the page from `file://`; none starts a server. Rows without
"Earlier result" ran on 2026-11-03 between 20:59 and 21:35 EST on the page
with SHA-256 prefix `174f2adf712bfb25`.

Captures to look at first: `captures/overview-1440-light.png`,
`overview-1440-dark.png`, `overview-390-light.png`, `overview-390-dark.png`,
`o-EPC-022-1440-light.png`, `o-EPC-022-1440-dark.png`,
`o-EPC-022-390-light.png`, `o-EPC-022-390-dark.png`, and the task view
`k-TSK-1122-*`; the `-390-*-full.png` copies show the whole phone page;
`answers-saved-1440-light.png` shows a Save confirmation,
`answers-fallback-1440-light.png` the download fallback and
`rail-hover-peek-1440-light.png` the panel open on hover without a tooltip.

## Larger than this round

- The nine specimen kinds carry layout, not data; TSK-1127 builds them on
  real records.
- The docs topic figures are hand-drawn SVG; the shared figure grammar
  should draw them in the product.
- The kit and page styles diverge from the vendored workspace kit;
  TSK-1162 and TSK-1120 decide what ships.
- Stage fills are measured on the rendered page; the other token pairs
  are computed from tokens. The stage bars and the dots for the stages a
  task has passed share a 65% line fill with a 1 px accent outline; the
  outline carries the 3:1 boundary (WCAG 1.4.11), and the fill alone is
  2.71:1 at worst. On the approved page those dots were full accent. SVG
  `.st-line` marks stay full accent, because the kit rule
  `:is(.fig,.marks) .st-line` outranks the page's rule; they are the
  "lines added" bars on Docs: what changed and one specimen mark.
- `.part i` has a 2 px minimum width. At that width the outline fills the
  whole segment, so it would read as "on main". The narrowest line segment
  on this page is 14.4 px at 390 px wide, so this does not occur here.
- Task Context is the record's Description with fenced figures left out,
  because no task record has a section named Context.
- The nine specimen kinds use generic content; representative content is
  TSK-1127's.
- The figure check inspects the numbered figures present, not an
  inventory of required figures; the size matrix is default and large
  only; TSK-1127 owns both.
- Tracker facts (lanes, blockers, decisions) were not re-read after
  12:33 EST.
- The guidance rules this prototype follows (names before identifiers,
  context never cut, times with a zone, two-line blocks) are not in the
  shipped guidance; they move to a skills task in their own PR.
