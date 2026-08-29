# TSK-014 W4 — the contract altitude, and the corrected composition board

Disposable design evidence for the TSK-014 recovery, **retained until the operator records a
decision**. It replaces nothing. `../tsk-014-w2/` and `../tsk-014-w3/` stay exactly as they were,
including their failed boards, their pinned rubrics and every observation record. Those are
append-only evidence about what went wrong, and this directory exists because of them.

Nothing here is production authority. No file in this directory is referenced by the renderer,
the schema, the documentation portal, the scaffold, or any managed artifact. Nothing here edits
`crates/codeflow-present/`, `docs-portal/`, a spec, or an ADR.

> **No composition on this board has been shown to communicate.** The observation gates
> (`rubric.v4.md`, G1–G10) have not been run — they need someone who authored nothing here and
> has read no answer. Every green result is deterministic. Start at `board.html`.

## Why there is a W4

W3 asked *which composition explains this subject best* and answered a different question first.
It qualified eleven candidates against the product's real technical contract and **ten came back
negative**: five `cf-present` candidates `needs-new-block`, four portal candidates
`needs-source-model`, one `needs-generator-work`. Only one could be carried at all, and only with
loss.

That is not a result about compositions. The closed block catalogue cannot express the
relationships this product's own subjects have, and the portal's source graph does not carry the
facts its own pages would need to draw. Choosing a composition winner under that carrier chooses
which drawing to fail to ship.

So W4 has **two decision altitudes** and judges them separately. The contract altitude
(`directions/`) decides first.

## What is here

```
board.html             the operator-facing index — start here
rubric.v4.md           the gates, written before any artefact existed
rubric.v4.lock.json    its digest, pinned in that state
registry.json          every declaration the gates check against
registry.lock.json     its digest, pinned in that state — byte-frozen since
amendments.json        every divergence since, recorded rather than edited away
dispositions.json      one entry per failed W3 band: before, after, what changed
directions/<id>/       five content models, each demonstrated by its own mechanism
portal/<id>/           three page families demonstrated, three specified
cases/<case>/<id>/     the repaired and replaced W3 candidates
baselines/<case>/      the plain controls, repaired to fit a 390px viewport
shared/shell.css       the neutral comparison chrome, byte-identical everywhere
shared/*.js            the subject sources, carried from W3 unchanged
tools/render.mjs       task-owned headless render + every probe
tools/verify.mjs       the gates, run against the recorded evidence
tools/w3-recheck.mjs   re-measures the W3 bands with the corrected probe
tools/pin.mjs          writes the two digest locks
tools/dispositions.mjs builds dispositions.json from the W3 registry verbatim
checks/                frame, axe, network, console, type, keys, mechanism,
                       shell, motion, coverage, lifecycle, w3-recheck
renders/               <artefact>-<mode>-<viewport>.png — 84 of them
SHA256SUMS             digests for every authored source and every render
```

## The five content models

Each renders real CodeFlow content through its own mechanism, at three widths in both modes, and
each ships one subject it genuinely cannot carry. They differ on where **composition authority**
sits — which is what decides the cost of a new subject form.

| Direction | Authority | A new subject form costs |
|---|---|---|
| A · curated semantic primitives | product code | a block type, a schema version, a renderer, tests, a release |
| B · bounded scene grammar | the document | nothing — a new scene. A new *mark kind* is product code |
| C · project-neutral recipe library | a reviewed library | a managed asset review, versioned with the skill |
| D · authored SVG under a service | the authoring agent | nothing to extend, and nothing accumulates |
| E · pre-rendered raster | the agent, out of band | nothing — and nothing is possible |

`A9` is the gate this rests on: every figure must be emitted by the mechanism its direction
claims, from an input that direction's contract would really accept. A drawing of what a content
model *would* produce is a claim about a content model, and that is the failure this whole task
exists to correct.

## What changed from W3's method, and why

| W3 | What went wrong | W4 |
|---|---|---|
| `registry.json` carried no lock, and its inode postdated seven of eleven candidate pages | Two composition declarations were rewritten after their renders were inspected, and the study said only marker tokens had changed | Both `rubric.v4.md` and `registry.json` are pinned by digest in locks written while `cases/`, `directions/`, `portal/`, `baselines/` and `renders/` did not exist |
| Amendments lived inside the registry | A registry that must stay editable cannot be digest-pinned | `registry.json` is byte-frozen; every divergence goes in `amendments.json`, which `A8` requires |
| One candidate drew five cell states, keyed four, and named four in its description; A2, A4 and axe all passed | Nothing compared what was drawn with what was keyed | **A12** compares the drawn set, the declared set, the keyed set and the described set |
| Type size was never measured | An 8.5px label is inside its frame and unreadable | **A13** measures the smallest computed type inside every figure, off the rendered page |
| The containment probe compared `getBBox()` against `viewBox.baseVal` | Two coordinate systems; every translated group reads as an overshoot | **A7** measures client rects on both sides, in CSS pixels. `checks/w3-recheck.json` records what each formulation said |
| Carrier feasibility was a per-candidate note | Ten of eleven came back negative and the board still ran as a composition contest | The contract is its own altitude, decided first, with its own gates (`A9`, `A10`, `G9`, `G10`) |

## The corrected containment probe, and what it cost

Three formulations were written. Only the third is used:

1. `getBBox()` compared directly with `viewBox.baseVal` — the W3 harness. Mixes the node's own
   user space with the root's viewBox, so every translated group reports an overshoot that is not
   on the page.
2. the same box mapped through `getCTM()` — this study's first correction. `getCTM()` maps to the
   nearest *viewport*, not to viewBox units, so it over-reports wherever a viewBox is scaled up
   and under-reports wherever it is scaled down.
3. **client rects on both the node and its SVG root, in CSS pixels** — what a reader actually
   loses. An SVG root clips to its box; a clipped child still reports its full rect.

Formulation 2 briefly suggested that two of W3's fourteen bands were probe artefacts. **That
reading is withdrawn.** Under formulation 3, all ten machine-visible W3 findings are confirmed
and none is an artefact; the remaining four are the `p1-a` and `p2-a` mobile bands, which W3
itself recorded as `machine_visible: false` composition failures, and the probe agrees they are
inside their frames — which is what W3 said. The W3 audit was right.

## The fourteen bands

Ten repaired, four replaced, none withdrawn. `dispositions.json` carries the W3 failure verbatim
beside what changed. The two replacements — `p1-a` and `p2-a` at the narrow band — had their new
composition **declared in `registry.json` before it was drawn**, which is the discipline W3 broke:

- **p1-a.** The W3 narrow band measured every task's later start downward in one shared gutter at
  a single x. Two tasks whose room overlapped merged into one column, three caps landed on one
  wave boundary as a single mark, and a zero-room cap was orphaned below the last row. Everything
  was inside the frame, so no probe saw it. Extent no longer rotates with the axis: each task's
  later start is measured horizontally **inside its own row** against one shared scale drawn once
  at the head. Two tasks cannot occupy one column because no two rows are the same row.
- **p2-a.** The W3 narrow band drew each stop as a rule from the figure's left edge across empty
  width to the rail — a section divider, not a stop on a rail — and left each cause caption
  abutting the *next* claim's title. Each claim is now a bounded lane; the rail runs inside it,
  inset from both edges; the stop is a cross-tick; the cause sits inside the same lane.
- **p2-a** also drew a fifth cell state that appeared in neither its legend nor its accessible
  description. It is keyed in both now.

## Running it

```sh
node docs/verification/tsk-014-w4/tools/render.mjs        # render + probe
node docs/verification/tsk-014-w4/tools/w3-recheck.mjs    # re-measure the W3 bands
node docs/verification/tsk-014-w4/tools/verify.mjs        # the gates
node docs/verification/tsk-014-w4/tools/verify.mjs --update   # refresh SHA256SUMS
```

`verify.mjs --update` runs every other check first and **refuses to refresh `SHA256SUMS` while
any check has failed**, leaving the recorded inventory byte-identical.

`render.mjs` uses the repository's already-locked `playwright-core` and `axe-core`. It launches
**headless only**, on its own loopback port with its own temporary profile, and never touches the
operator's browser or active view. On this host it needs Mach port rendezvous, which a
restrictive sandbox denies; under denial it fails at launch and writes nothing.

## Limitations

- **No composition has been observed.** G1–G10 are not run. Nothing here is a selection or a
  production-readiness judgement, and no direction has been security-reviewed.
- **The lifecycle proof is weaker than W3's.** This harness does not reimplement W3's
  process-inventory ownership contract; it closes through Playwright and proves its own profile
  root absent. A surviving browser process would not be detected. `checks/lifecycle.json` says so.
- **A12 only reaches pages that emit the attributes it reads.** The four candidates carried
  forward from W3 do not use `data-state`/`data-legend-state`, so the gate is silent on them; the
  `p2-a` unkeyed mark was found by reading the page, not by the probe. A gate that cannot see an
  artefact has not passed it.
- **Three portal families are specified and not built** (`capability-reach`, `code-drilldown`,
  `search-find`). They are in `registry.portal_families` with `demonstrated: false` so the set is
  honest rather than trimmed to what was finished.
- **The monorepo family renders a declared fixture**, not a real content graph, because this
  repository is a single project. It says so on the page, in the registry, and in the figure's own
  accessible description.
- **The demonstrations are demonstrations.** Direction A ships three primitives, not a catalogue;
  direction B's grammar has eight mark kinds and no layout operator; direction C's library has
  three recipes. Each is enough to judge the trade and none is enough to ship.
- **One browser build.** Chrome 151 on macOS arm64. No claim is made about Firefox, WebKit,
  Windows or Linux, and the W3 renders were taken under Chrome 150, so a byte comparison across
  the two boards would be meaningless and none is made.
- **The accessible equivalent of each figure is its `title`/`desc` plus the disclosure beneath
  it.** That is honest for a disposable study; a production surface owes more, and the automated
  axe pass is partial evidence, never proof of conformance.
