# The status dashboard prototype for the operator's walk-through

`prototype.html` is one self-contained page: the CodeFlow 4.0.0 status
report in the design the operator accepted on 2026-11-02 (Graphite, deep
green accent, two-level left menu, shared top bar with one display gear),
on real facts read once from the repository, GitHub and the Coordinator
trackers. It exists so the operator can click through the design before
TSK-1120, TSK-1121 and TSK-1098 build it. It is not a report to act on and
nothing in it ships.

Open it from disk in any browser; it needs no server. The hash names a
view (`prototype.html#blockers`) and the page routes on it when it loads.

## What to click, in order

1. Overview: the hero percent with its delta, the three outcome cards,
   Figure 1 (where the 246 tasks are), the four streams, the pull requests.
2. An outcome card opens its topic view: context strip, then the tabs
   Progress (what it is, partition, themes), Order (ready, then waiting
   with the predecessor each waits on) and Issues (blocked tasks and open
   pull requests).
3. A theme row opens the theme view; a task row opens the task view with
   the eight-fact strip, criteria, dependencies and evidence. The
   breadcrumb goes back one level at a time.
4. The rail's second icon shows Epics, Lanes and Evidence; the third shows
   Your decisions and Script; the two utility icons show the ten report
   kinds and the docs portal layers.
5. Your decisions: four form controls, each with context, options, the
   recommended answer and why. An answer appears in the Script view's
   second part; the script writes only the answers file.
6. The gear: Mode, Themes, Accent and Text size. The menu toggle collapses
   the panel to the rail on desktop and opens a drawer under 768 px.

## How it was built

Everything under `_src/` is the build; nothing in it runs in the product.

| Step | Command (from `_src/`) | What it does |
|---|---|---|
| gather | `node gather.cjs` | Reads the task and epic records on `origin/integration/EPC-022-the-3-2-0-milestone-fixes` and on `origin/main`, each record's last change on the line, `gh pr list` and `gh api compare` (read only) and `codeflow work next` from this worktree; writes `facts.json` with the read's start and end time. Run `git fetch origin` first. |
| report facts | `report.json` (by hand) | The deliverable line, streams, lanes, blockers, decisions, owed actions, the four walk-through questions and the ten report kinds, each with its source file and line and the time it was read from the trackers. |
| build | `node build.cjs` | Writes `../prototype.html` (the kit CSS and JS inlined, the data embedded) and `kit/index.html` (the same page linking the kit files, for the workspace navigation test). |
| render | `node render.cjs` | Captures 15 views at 1440 and 390 px in light and dark to `../captures/`, fails on horizontal scroll, a page error or a paragraph over three lines in a card, and drives the drill-down, tabs, rail, answers and drawer journeys. |
| axe | `node axe.cjs` | WCAG 2.x A and AA rules over 12 views at both widths and modes; fails on a serious or critical violation. |

The facts in the committed page were read from 2026-11-02 12:59 to 13:00
EST (line tip `dd1913165`, `main` `b2bb61af3`); the tracker facts in
`report.json` are from 12:26 EST and carry their own times. The change
marks compare today's read with the 2026-11-01 14:31 EST report
(`_src/previous-2026-11-01-1431.json` and
`_src/previous-live-counts-2026-11-01-1431.json`, copies of the workspace
files that report was built from).

The kit (`_src/kit/cf.css`, `cf.js`, `cf-icons.js`) is vendored unchanged
from `coordination/coordinators/visual-docs/design/after/` as it stood on
2026-11-02 (workspace commit `d758a9e` plus that day's uncommitted edits to
the display gear). It uses the kit's token names (`--accent`, `--ok`,
`--paper`); TSK-1162 reconciles them with the shipped `--cf-*` names.

## What is real and what is mocked

Real: every count, percent, location, theme, last-moved time, ready and
waiting reason, pull request number and state, blocker, lane and decision,
each with its source in the Evidence view. The themes group an epic's
tasks by the record section that introduced them (TSK-1095 AC-15); a task
no amendment introduced reads "Original scope".

Mocked or absent, and said so on the page:

- The runtime chrome: the top bar, menu, gear and Comment button are the
  kit's prototype code, not `codeflow present`. The Comment button opens
  nothing.
- Lane and window per task read `not recorded` on every task view;
  TSK-1095's fields do not exist on the line yet.
- Epic verification reads "not recorded on the line" because no epic
  record on the line carries a verification result.
- The installed `codeflow` is 3.1.0, whose `status` prints no progress
  facts, so counts come from the records, read the way the workspace's
  `live-count.cjs` reads them.
- GitHub had not computed mergeability for most open pull requests at the
  read; the Issues tab shows what it returned.

## Checks, last run on the committed page

| Check | Result |
|---|---|
| `render.cjs` | 60 captures, 0 horizontal scroll, 0 walls, 0 page errors; 11 of 11 journey checks pass |
| `axe.cjs` | 0 serious or critical violations over 48 view renders |
| workspace `textwall-check.cjs` | 0 paragraphs over 3 lines at 1440 and 390, on `prototype.html` and `_src/kit/index.html` |
| workspace `nav.test.cjs` on `_src/kit` | 31 of 31 navigation checks pass |
| workspace `link-check.cjs` | 4 references; the one "missing" is the string `${E(h)}` inside the inlined `cf.js` source, a JavaScript template, not a link |

Captures are at 1440 and 390 px (the brief named 1200 or 1440) in both
modes, one per view, plus `journey-script-1440-light.png` (an answer in the
script) and `drawer-390-light.png` (the phone drawer open). The 390 px
captures are full page.
