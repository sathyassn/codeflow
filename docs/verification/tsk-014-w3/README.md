# TSK-014 W3 — rendered composition study, version 2 of the board

Disposable design evidence for the TSK-014 recovery, **retained until the operator
records a decision**. It replaces nothing: `../tsk-014-w2/` stays exactly as it was,
including its failed board, its pinned rubric and both observation records. Those are
append-only evidence about what went wrong, and this directory exists because of them.

Nothing here is production authority. No file in this directory is referenced by the
renderer, the schema, the documentation portal, the scaffold, or any managed artifact.

> **No composition has been shown to communicate, and the board is not fully
> admissible.** The observation gates have not been run — they need someone who authored
> nothing here and has read no answer. An independent Claude design-primary audit then found
> that A1–A6 could pass while ten renders clipped their own content, that the registry's
> account of its own amendments was false, and that four further bands lose their governing
> idea in ways no machine check sees. `rubric.v3.md` governs this board and adds **A7** and
> **A8**; fourteen bands are registered invalidated and cannot be observed. See
> `observer-state.md`.

## Why there is a W3 at all

W2 produced a complete board, a clean deterministic pass, and an unusable comprehension
result. An independent Claude design review found the reason: the candidates printed
material parts of the expected answer in visible prose, so the blind observer could read
rather than decode. Four further defects came with it — responsive checks that preserved
labels while losing the spatial idea, no intermediate width at all, a D1 question that did
not match the surface's job, and carrier feasibility assessed after the expensive work.

Every one of those is now a machine-checked precondition rather than a review finding.
`rubric.v2.md` explains each change against the v1 gate it replaces.

## What is being decided

| Case | Surface | The question the candidates must settle |
|---|---|---|
| P1 | `cf-present` | If exactly one unfinished EPC-005 task slipped by one wave, which tasks would move the finish and which would not? |
| P2 | `cf-present` | Which platforms carry executed evidence for the exact candidate, and for each platform that does not, what kind of thing stopped it? |
| P3 | `cf-present` | Which findings took more than one round to close, and which of those was re-opened by something that did not come from the review itself? |
| D1 | documentation portal | Where does this repository keep each kind of knowledge, and how is each kind allowed to change? |
| D2 | documentation portal | Which decision governs this record now, how much of it does that decision actually reach, and which of its claims has nothing behind it? |

`registry.json` is the authority for all of it: per case the surface's real job, the
question, whether the question is compound and how that is handled; per candidate the
primary unit, primary axis, encoded relationship, the composition declared for each of
three width bands, the non-colour channel behind every state, and what would carry it in
the product's real contract at what loss.

It was **intended** to be written before any candidate existed, and the study originally
claimed that flatly. That claim is withdrawn as unsupported. Nothing on disk witnesses it —
the file carries no lock and its inode postdates most of the candidate pages — and for
`a-critical-path-and-room` it is disproved: both of that candidate's composition
declarations were rewritten, in one edit, after its renders were inspected. Read
`registry.pre_registration_evidence` and `registry.amendments` before weighing A2, A3 or A4.

**D1 was re-registered from scratch.** W2 asked the portal's Orient surface an exact
lookup question, which a numbered list wins by construction. That comparison was decided
by the question's shape before anything was drawn, so the question was replaced and three
materially different candidates were built against the new one.

**D2 is a decomposition, not a competition.** Its job is genuinely two-part — how far the
governing decision reaches, and what stands behind each claim — and the two parts want
opposite geometry. Forcing one winner across both would select a compromise nobody chose,
so both candidates are registered, each naming the part it optimises and what carries the
other. The settlement may take one, the other, or the composition of both.

## Status

Eleven candidates and five plain baselines are authored, rendered and verified:

```
p1/{a-critical-path-and-room, b-paths-to-the-sink}
p2/{a-stopped-provenance, b-absence-by-cause}
p3/{a-convergence-with-source-depth, b-source-region-history}
d1/{a-layers-by-change-rate, b-the-spine, c-write-authority-map}
d2/{a-decision-reach, b-evidence-margin}
```

Each case renders from one shared subject source, and each candidate publishes its derived
answer into a **non-visible** channel that `tools/verify.mjs` recomputes from the
repository. No candidate states that answer where a reader can see it, and the verifier
proves it from the text actually captured off every render.

## Layout

```
README.md              this index
registry.json          the registration, its amendments, and what its priority is worth
rubric.v2.md           versioned gates (digest-pinned; the W2 rubric is untouched)
rubric.v2.lock.json    the v2 digest plus an honest record of how ordering was observed
rubric.v3.md           the governing rubric: v2 plus A7 frame containment and A8
                       registration honesty, written after the board was rendered
rubric.v3.lock.json    the v3 digest, and an explicit statement that v3 pre-registers
                       nothing
amendments.lock.json   the amendments an outside seat established from evidence, pinned
                       by digest over their verbatim prior wording
observer-state.md      what has and has not been established, and by whom
answer-key.md          the expected answer per case — never shown to an observer
content-inventory.md   every fact used, with its repository source
hypotheses.md          cross-candidate convergence, recorded as hypothesis only
NEXT.md                the precise continuation boundary
board.html             the neutral operator-facing index
baselines/<case>/      chat.md, markdown.md, baseline.html — the plain differential
cases/<case>/<candidate>/index.html   one self-contained page per candidate
shared/plan-model.js       P1 — waves, routes, room
shared/tsk007-facts.js     P2 — the fact registry, causes and stops
shared/p3-convergence.js   P3 — rounds and findings, bound to real commits
shared/repo-layers.js      D1 — the six layers, their homes and write authority
shared/record-authority.js D2 — the record, its sections, decisions and evidence
shared/registry.js         generated wrapper so board.html can read registry.json
tools/render.mjs       exact-owned sequential render + axe + network + text + motion
tools/verify.mjs       integrity and admissibility (`--update` to refresh SHA256SUMS)
tools/source-authority.mjs  the bounded evaluation of a shared source and the contract
                       every path, revision and pathspec it declares must pass
tools/path-containment.mjs  pure platform-correct containment, shared by both
tools/selftest.mjs     proves the refusal paths a healthy run never reaches
tools/fixtures/        self-test inputs only — never candidates, never rendered normally
renders/               <case>-<candidate>-<mode>-<viewport>.png
checks/                axe, network, answers, text, motion, frame and lifecycle records
SHA256SUMS             digests for every authored source and every render
```

## Three widths, and why the middle one is not a convention

Every candidate declares the two widths at which **its own** composition changes, and an
explicit composition for each of the three resulting bands. `tools/verify.mjs` requires
the tablet render (900 px) to fall strictly between those two declared breakpoints and
requires the page to really declare a media query at each, so the intermediate frame always
shows a composition neither of the other two frames shows. An intermediate render that
looked like the desktop one would test nothing, and W2 skipped it entirely.

The narrow compositions are declared alternates, not compressions. P1-A rotates its wave
axis to vertical rather than becoming full-width boxes with a wave number printed on them;
P2-A rotates its rails and keeps the untravelled remainder drawn; P3-A gives up the page's
one shared round axis so each finding keeps its own; D2-B turns its evidence margin into a
reserved band that keeps its full height when empty.

## Motion

No candidate encodes motion, and none of these subjects has a relationship that movement
would carry. That is registered, and then **proven** rather than asserted: every render is
probed under both `prefers-reduced-motion` settings for a running animation, a declared
animation name, or a non-zero transition duration, and `checks/motion.json` records the
result. W2 shot a reduced-motion still per candidate and every one was byte-identical to
its normal still — evidence about the screenshot, not about motion. No such file is
produced here.

The one carrier that could host the `cf-present` compositions forbids the CSS that motion
and its reduction would both need, which is recorded in `registry.json` as part of the
carrier finding rather than discovered later.

## What is shared, and what deliberately is not

**Presentation is never shared between candidates.** Each carries its own inline CSS and
its own markup. A shared stylesheet would become a premature system and would make the
candidates converge on one house look — the exact failure this study exists to avoid. The
five *baselines* do share one plain stylesheet, on purpose: the baseline is not a design
and must not become one.

**Subject derivation is shared.** Each case has exactly one subject source and every
sibling loads it, so candidates differ only in encoding, and `tools/verify.mjs` proves it:

| Case | Source | What is bound to the repository |
|---|---|---|
| P1 | `shared/plan-model.js` | the dataset is byte-identical across all three P1 pages and matches `project-management/tasks/*.md` frontmatter including titles; every short label is a word-subset of its task's real title |
| P2 | `shared/tsk007-facts.js` | every material claim carries a quotation checked verbatim against the TSK-007 record; every absence is classified exactly once against a closed cause set, and every cause carries the record's own sentence |
| P3 | `shared/p3-convergence.js` | every round matches `git log`; every quoted diff line matches `git show` character for character; a re-opening from off the round axis must name its origin and the origin's tokens must appear in its own quotation |
| D1 | `shared/repo-layers.js` | the six-layer table is re-parsed out of `AGENTS.md` and compared cell by cell; every declared home is checked to exist in this checkout, or to deliberately not exist; every authority rule is quoted verbatim |
| D2 | `shared/record-authority.js` | every frontmatter value matches the real record file; the record's own headings and its eleven numbered behaviours are re-parsed out of SPC-004; every quotation is verbatim |

**A shared source is data, not trusted code.** It is repository-controlled and everything
it declares reaches an interpreter, the filesystem or `git`, so
`tools/source-authority.mjs` qualifies all three first: evaluation in a fresh `node:vm`
context with no Node globals and code generation disabled, bounded by a wall clock, with
only a JSON snapshot crossing back; a declared path admitted only as a canonical regular
file inside one explicitly named root with no fallback between the repository and the
study; a revision reaching `git` only as a hex object id and a diff path only as a
`:(literal,top)` pathspec. `tools/selftest.mjs` sections G, H, I and J prove each refusal,
including that `verify.mjs` really applies them.

## Carrier, before the expensive work

Every candidate names what would carry it in the product's real technical contract, and
that claim is bound to the source that imposes it — quoted verbatim, and checked. The
short version, which the operator should read before selecting anything:

- **`cf-present`.** The typed block catalogue is closed. Of it, only `diagram` (Mermaid,
  seven kinds) and `html` can carry drawn geometry, and `diagram` cannot place a mark at a
  computed position, size a bar to a quantity, reserve empty space or hatch a region. The
  `html` escape block can — it renders inside a sandboxed iframe and inline SVG survives —
  but its inline CSS may not contain `@`, so a composition placed there has **no media
  query**: no responsive recomposition, no `prefers-color-scheme`, no
  `prefers-reduced-motion`. Script is refused twice over, so the block cannot size its own
  frame, and the review stylesheet constrains only its inline size. Five of the six
  `cf-present` candidates are therefore `needs-new-block`: selecting one commits to a new
  typed block whose renderer owns mode and responsive behaviour. That is real, plannable
  work, and it is stated here rather than discovered after a winner is chosen.
- **The documentation portal.** Raw HTML written in a repository Markdown source is
  escaped, and no MDX integration is installed, so a portal composition can only ever be
  emitted by the generator — but there the constraint ends: the portal's composition lives
  in a real stylesheet, so media queries, colour-scheme handling and reduced motion are all
  available. Four of the five portal candidates are `needs-source-model` for a different
  reason: the drawing is ordinary generator work, and what is missing is a *fact* the
  source graph does not carry (a layer's change cadence and write authority; a
  supersession's scope against the claims it covers; a claim-level evidence link).
  `d1/b-the-spine` is the exception at `needs-generator-work`: the spine is already
  declared in the operating contract and the relationships it draws are ones the portal
  already derives.

## Running the study

```sh
node docs/verification/tsk-014-w3/tools/render.mjs
node docs/verification/tsk-014-w3/tools/selftest.mjs           # harness contracts
node docs/verification/tsk-014-w3/tools/verify.mjs --update    # refresh SHA256SUMS
node docs/verification/tsk-014-w3/tools/verify.mjs             # compare, fail on drift
```

Sections A, B, B2, D, E and E4 of `selftest.mjs` drive `render.mjs` and need the same host
state it does. On a host that denies it, run the rest with `--only=C,F,G,H,I,J`; a selected
run names the sections it did not run and claims nothing for them.

`render.mjs` uses the repository's already-locked `playwright-core` and `axe-core`, and
**resolves the browser and those modules before touching any owned output**, so a missing
browser cannot destroy the previous evidence set. It renders reproducibly: every page is
settled for fonts and two committed frames, and Chromium is launched with its tiled-raster
and threaded-compositing paths disabled, so two consecutive normal runs produce every
render byte-identical and `SHA256SUMS` is a drift check rather than a timestamp.

It needs host state a restrictive sandbox denies — process inventory (`ps`), signal
delivery, and a profile socket directory. Denied any of them it fails closed at launch,
retains its marked root and deletes nothing. That is the ownership contract working, not a
defect; never substitute a manual `rm -rf` or a broad process match for it.

Cleanup does not trust a successful close: the owned identity set is refreshed again
afterwards, any still-live exact identity triggers a fingerprint-rechecked TERM/KILL
fallback, and the marked root is removed **only on positive proof** — an inventory that
completed and showed zero live exact identities. Any inventory failure is uncertainty: the
root is retained, the run fails, and nothing is deleted. Only `ENOENT` counts as proof of
removal. A normal run fails if the fallback was needed at all. It never launches headed and
never touches the operator's browser.

Deletion authority is a **positive ownership contract**, not a directory comparison.
`--output-root` is accepted only when it canonically resolves inside the host temporary
directory, is an owner-only directory owned by this user, and carries a
`.cf-w3-selftest-root` marker whose token matches `--output-token` — checked before any
`mkdir` or `rm`. Only a closed set of known output names is ever removed; anything else is
reported and left alone.

`verify.mjs --update` runs every other check first and **refuses to refresh `SHA256SUMS`
while any check has failed**, leaving the recorded inventory byte-identical.

## Limitations

- **No composition has been observed.** G1–G8 are not run. Nothing here is a selection or
  a production-readiness judgement.
- **Native Windows runtime behaviour is exercised nowhere.** The Windows containment cases
  in `selftest.mjs` are pure path logic and no runtime claim is made from them.
- **A1 checks the text a render actually shows**, captured off the render rather than read
  out of the page source. It cannot judge a paraphrase it was not registered to catch. It
  narrows the leak surface; it does not eliminate the author's responsibility for the
  distinction the rubric draws. A page edited without re-rendering is caught by the
  checksum, not by A1.
- **A3 proves a declared alternate composition exists at the intermediate width; it cannot
  prove that composition is materially different.** The gate checks that the tablet frame
  falls between each candidate's own declared breakpoints and that both media queries are
  real. How far the intermediate composition actually departs from the wide one varies by
  candidate — P1-A splits into two registers, P2-A shortens its lane heads, P3-B moves its
  rail from a margin to a strip — and whether each departure is a decision or a
  compression is **G8's** judgement, not the machine's. The author's own view, recorded
  because it is the author's and therefore not a gate: the P1, P2-A and P3-B intermediate
  frames earn their place; P2-B, D1-A and D2-B change least between wide and intermediate
  and an observer may reasonably grade them as compressions.
- **Two composition declarations were rewritten after their renders were inspected, and the
  claim that only marker tokens changed was false.** `registry.json`'s `amendments` array
  records every known post-registration change with the prior wording verbatim:
  `a-critical-path-and-room`'s `intermediate_composition` and `narrow_composition`, both
  changed in a single edit; the marker-token correction; and two divergences where the
  drawing and the declaration disagree and neither was moved
  (`c-write-authority-map`'s straddle, `a-stopped-provenance`'s unkeyed mark). The prior
  wordings were recovered from the authoring session's transcript by the host — an earlier
  version of this correction called them unrecoverable, which was itself false — and the two
  established ones are pinned by digest in `amendments.lock.json`, so `A8` fails if either is
  dropped or quietly reworded. The narrow-band rewrite matters twice over: the shared gutter
  it was rewritten to describe is the direct cause of that band's recorded failure, so the
  rewrite removed the one declaration that could have caught it.
- **Only the rubric's priority can be shown from the files.** `registry.json` carries no
  lock, and its inode was created after most candidate pages had been written, so nothing on
  disk witnesses that the registry preceded the candidates — and for the two fields above it
  is disproved. `registry.pre_registration_evidence` states this, `rubric.v3.md` explains
  what it costs A2, A3 and A4, and `A8` in `tools/verify.mjs` makes the account mandatory
  rather than optional.
- **Ten of the eighty-one renders clip their own content or draw past their own frame**, and
  every gate in `rubric.v2.md` passed anyway. `A7` now measures it per render
  (`checks/frame.json`). Fourteen bands in total are registered in
  `registry.invalidated_bands` — the ten containment failures plus four composition failures
  the machine cannot see. Each entry carries **two** statements: the render is inadmissible
  as comparison evidence, *and* the demonstrated failure that made it so, with its kind,
  severity and failing axes. Six are disqualifying, six material, two minor. **Inadmissible
  is not neutral**: no band here is inadmissible for a procedural reason. What no entry
  claims is a rubric gate result, because the audit read the answer key — but a measured clip
  and two marks in one position do not need a blind observer to be true.
- **The accessible equivalent of each figure is its `title`/`desc` plus the disclosure
  table beneath it**, which carries the same facts. That is honest for a disposable
  comprehension study; a production surface owes more, and the automated axe pass is
  partial evidence, never proof of conformance.
- **The bounded evaluation context** is a boundary against an accidental or careless
  source, not a claim of isolation against a determined V8 escape, and it bounds
  synchronous execution rather than memory.
