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

Round 2 (2026-11-02, 16:10 to 17:05 EST) folded in the operator's
feedback (names in words before every identifier, full context on every
epic card and page, a banner that says what the page is, wall-clock times
with a zone), the accent decision (deep green in every theme) and the
independent Codex read's findings M1 to M8, N1 and N2.

## What to click, in order

1. Overview: the hero opens with what the release delivers and what done
   means, then the percent with its delta, the three outcome cards (each
   with what it delivers, for whom and why now), Figure 1 (where the 246
   tasks are, with its data table), the four streams, the pull requests
   and a Terms list.
2. An outcome card opens its topic view: the "What it is" card first
   (the epic's whole summary, never cut), the context strip, the
   capability chips, then the tabs Progress (partition, themes), Order
   (ready, then waiting with the predecessor each waits on) and Issues
   (blocked tasks and open pull requests). Arrow keys move between tabs.
3. A theme row opens the theme view; a task row opens the task view with
   the whole Outcome first, then the eight-fact strip, criteria,
   dependencies and evidence. The breadcrumb names each level in words.
4. The rail's second icon shows Epics, Lanes and Evidence; the third shows
   Your decisions and Script; the utility icons show the ten report kinds
   (each opens a layout specimen), the docs portal layers, one docs topic
   with its Concept, Architecture and Technical tabs, and the portal's own
   "what changed" page.
5. Your decisions: four form controls, each with context, options, the
   recommended answer and why. An answer appears in the Script view's
   second part; the script writes only the answers file.
6. The gear: Mode, Themes, Accent and Text size. Every theme opens on deep
   green; the second accent is plum (Graphite, Sage) or violet (Slate).
   The menu toggle collapses the panel to the rail on desktop and opens a
   drawer under 768 px. Comment toggles the comments rail and arms the
   page so a click on any block drops a numbered marker.

## How it was built

Everything under `_src/` is the build; nothing in it runs in the product.

| Step | Command (from `_src/`) | What it does |
|---|---|---|
| gather | `node gather.cjs` | Reads the task and epic records on `origin/integration/EPC-022-the-3-2-0-milestone-fixes` and on `origin/main`, each record's last change on the line, `gh pr list` and `gh api compare` (read only) and `codeflow work next` from this worktree; writes `facts.json` with the read's start and end time. Run `git fetch origin` first. `--refresh-records` re-reads only the record texts (whole task outcomes, epic summaries, capability names from `docs/capabilities.md`, the docs diff) at the pinned tips, leaving counts, GitHub and `work next` untouched. |
| report facts | `report.json` (by hand) | The deliverable line, the short names used before each identifier, each epic's for-whom and why-now with its source, streams, lanes, blockers, decisions, owed actions, the glossary, the four walk-through questions and the ten report kinds with their opening figure and reads, each with its source file and line and the time it was read from the trackers. |
| build | `node build.cjs` | Writes `../prototype.html` (the kit CSS and JS inlined, the data embedded) and `kit/index.html` (the same page linking the kit files, for the workspace navigation test). |
| render | `node render.cjs` | Captures 22 views at 1440, 1200 and 390 px in light and dark to `../captures/`, scrolled to the top of the page; fails on horizontal scroll, a page error, a paragraph over three lines in a card, a chip or pill that wraps, or clipped text; checks the Slate and Sage themes and the large text size at 1200; drives 24 journeys (drill-down, tabs and arrow keys, about-first views, focus on the new heading, breadcrumbs in words, the gear's focus in and out, docs tabs that open with a figure, every figure's title, description and data table, the nine specimens, the drawer's inert main, phone table labels). |
| axe | `node axe.cjs` | WCAG 2.x A and AA rules over 16 views at 1440 and 390 px in both modes, plus Slate and Sage and the large size at 1200; counts critical, serious, moderate and minor; fails on serious or critical. |
| floors | `node floors.cjs` | From the kit's tokens, the sRGB contrast of every stage fill, figure mark and text colour against its surface, per theme, mode and accent, against the 3:1 and 4.5:1 floors. Source-derived numbers, not measurements of the rendered page. |

The facts in the committed page were read from 2026-11-02 13:40 to 13:41
EST (line tip `aa84d8544`, `main` `b2bb61af3`); the record texts were
re-read at the same two tips at 16:25 EST and no count changed. The
tracker facts in `report.json` are from 12:26 EST and carry their own
times. The change marks compare this read with the 2026-11-01 14:31 EST
report (`_src/previous-2026-11-01-1431.json` and
`_src/previous-live-counts-2026-11-01-1431.json`, copies of the workspace
files that report was built from).

The kit (`_src/kit/cf.css`, `cf.js`, `cf-icons.js`) started as the
workspace kit in `coordination/coordinators/visual-docs/design/after/`
(workspace commit `d758a9e` plus that day's display gear edits). This
round edited it, and each edit is for the token source task (TSK-1162) or
the shell (TSK-1120) to carry or reject:

- deep green as the default accent of Slate and Sage, violet or plum as
  the second (the operator's decision of 2026-11-02);
- a `--stage-todo` token per theme and mode so the todo fill meets 3:1 on
  every surface;
- secondary text (`--text-2`) for table headers, phone column labels and
  identifier tags, which `--text-3` failed at 4.5:1 on the sunken surface;
- focus: a new view focuses its heading, the gear moves focus into its
  dialog and back, the drawer makes the main column inert and returns
  focus to its toggle;
- phone tables hide the header row and print each cell's label before it.

The prototype also lets menu rows wrap where the kit clips them at one
line, so an epic title is never cut to "..." in the menu. It uses the
kit's token names (`--accent`, `--ok`, `--paper`); TSK-1162 reconciles
them with the shipped `--cf-*` names.

## What is real and what is mocked

Real: every count, percent, location, theme, last-moved time, ready and
waiting reason, pull request number and state, blocker, lane and decision,
each with its source in the Evidence view. The themes group an epic's
tasks by the record section that introduced them (TSK-1095 AC-15); a task
no amendment introduced reads "Original scope". Each epic's what, for whom
and why now is quoted from its record at the lines Evidence names.

Specimens, and said so on each page: the nine report kinds other than the
status report open as layout specimens, with the kind's menu, opening
figure and strip drawn with labels in place of data. The report and portal
samples task (TSK-1127) builds the real ones. The docs topic's three
figures are drawn by hand for this page, not by the shared figure grammar.

Mocked or absent, and said so on the page:

- The runtime chrome: the top bar, menu, gear and Comment button are the
  kit's prototype code, not `codeflow present`. Comment markers live only
  in the page.
- Lane and window per task read `not recorded` on every task view;
  TSK-1095's fields do not exist on the line yet.
- Epic verification reads "not recorded on the line" because no epic
  record on the line carries a verification result.
- The installed `codeflow` is 3.1.0, whose `status` prints no progress
  facts, so counts come from the records, read the way the workspace's
  `live-count.cjs` reads them.
- GitHub had not computed mergeability for most open pull requests at the
  read; the Issues tab shows what it returned.

## Checks, last run on the committed page (2026-11-02, 17:03 to 17:06 EST)

| Check | Result |
|---|---|
| `render.cjs` | 156 renders at 1440, 1200 and 390 in both modes plus the theme and size loop: 0 horizontal scroll, 0 walls, 0 wrapped chips, 0 clipped text, 0 page errors; 24 of 24 journeys pass; 136 captures |
| `axe.cjs` | 80 view renders: 0 critical, 0 serious, 0 moderate, 0 minor |
| `floors.cjs` | 228 pairs over 3 themes, 2 modes and 2 accents: 0 under the floor |
| grep over `prototype.html` | 0 "..." in content; 279 identifier text nodes, every one an `id` tag after a name; 0 identifiers as the only label of a link, button or cell; 0 times without a zone |
| workspace `textwall-check.cjs` | 0 paragraphs over 3 lines at 1440 and 390, on `prototype.html` and `_src/kit/index.html` |
| workspace `nav.test.cjs` on `_src/kit` | 31 of 31 navigation checks pass |
| workspace `link-check.cjs` | 4 references; the one "missing" is the string `${E(h)}` inside the inlined `cf.js` source, a JavaScript template, not a link |

Captures are `captures/<view>-<1440|1200|390>-<light|dark>.png` for 22
views, each from the top of the page (the 390 px ones full page), plus
the Slate and Sage large-size overview, `journey-script-1440-light.png`
(an answer in the script) and `drawer-390-light.png` (the phone drawer
open).

## Larger than this round

- The nine specimen kinds carry labels, not data; TSK-1127 builds them
  on real records.
- The docs topic figures are hand-drawn SVG; the shared figure grammar
  should draw them in the product.
- The kit edits above diverge from the vendored workspace kit; TSK-1162
  and TSK-1120 decide what the shipped kit carries.
- The colour floors are computed from tokens, not measured on the
  rendered page; axe's contrast rule is the rendered check.
- Tracker facts (lanes, blockers, decisions) were not re-read after
  12:26 EST.
