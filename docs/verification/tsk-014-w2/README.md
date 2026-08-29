# TSK-014 W2 — rendered composition study

Disposable design evidence for the TSK-014 recovery. It settles five composition
questions with rendered candidates instead of prose, and is **retained until the
operator records a decision**.

Nothing here is production authority. No file in this directory is referenced by
the renderer, the schema, the documentation portal, the scaffold, or any managed
artifact.

> **The first blind observation is preserved but does not qualify the board.**
> Claude's independent design review found that visible prose states material
> parts of the expected answers, confounding the comprehension comparison. The
> board must be corrected and re-observed before selection. See
> `observer-state.md` and `observations/`.

## What is being decided

| Case | Subject | Decision asked of the operator |
|---|---|---|
| P1 | EPC-005 plan graph | Which encoding should `cf-present` use for a dependency/ownership graph? |
| P2 | TSK-007 qualification evidence | Which encoding should `cf-present` use for sparse claim/evidence coverage? |
| P3 | Review convergence | Which encoding for findings across review iterations? |
| D1 | Portal Orient | How should a reader be brought into this system? |
| D2 | Portal Record | How should a record expose its authority and evidence? |

The TSK-008 `system-atlas` boundary/legend/evidence-chain idea is a
**conformance input**, not a reopened question.

## Status

**The board is complete.** All five baselines and all twelve candidates are
authored, rendered and verified: `p1/{a-wave-lanes,b-blocking-matrix,c-critical-ribbon}`,
`p2/{a-evidence-grid,b-provenance-rail}`, `p3/{a-finding-anchored-delta,b-convergence-ledger}`,
`d1/{a-concept-dependency-path,b-task-first-entry,c-contract-map}` and
`d2/{a-chain-in-place,b-evidence-adjacent-margin}`. Each case renders from one
shared subject source, and each publishes a derived answer that `tools/verify.mjs`
recomputes independently from the repository.

The first blind G1–G8 observation and the independent Claude design judgment are
recorded under `observations/`; see `observer-state.md` for their exact limits.
The observation exposed useful candidate failures, but its comprehension gates
are confounded by visible answer prose. Correction, a fresh blind run, Claude
re-review and operator selection remain open.

## Layout

```
README.md              this index
rubric.md              pre-registered gates (byte-pinned; never edited in place)
rubric.lock.json       the rubric digest plus an honest record of how ordering was observed
observer-state.md      observation qualification, invalidation and next gate
observations/          qualified blind rendered-observation records
content-inventory.md   every fact used, with its repository source path
answer-key.md          the exact task question per case and its verifiable answer
NEXT.md                the precise continuation boundary
baselines/<case>/      chat.md, markdown.md, baseline.html — authored before candidates
cases/<case>/<candidate>/index.html   one self-contained page per candidate
shared/plan-model.js   the P1 subject derivation (waves, chain, room) — no styling
shared/tsk007-facts.js the P2 fact registry, every claim carrying a verbatim quotation
shared/p3-convergence.js  the P3 rounds/findings ledger, bound to real commits and diff lines
shared/repo-entry.js   the D1 concepts, preconditions, planes and mutation gates
shared/record-authority.js the D2 record, decision links and evidence states
board.html             neutral comparison surface + implementation-feasibility record
tools/render.mjs       exact-owned sequential headless render + axe + network + lifecycle
tools/verify.mjs       integrity, content binding, checksums (`--update` to refresh)
tools/source-authority.mjs  the bounded evaluation of a shared source and the one
                       contract every path, revision and pathspec it declares must pass
tools/path-containment.mjs  pure platform-correct containment, shared by both
tools/selftest.mjs     proves the survivor, containment, update-guard, sandbox,
                       source-authority and verifier-wiring paths (`--only=` to select)
tools/fixtures/        self-test inputs only — never candidates, never rendered by a normal run
renders/               <case>-<candidate>-<mode>-<viewport>.png
checks/                axe, network, answers and lifecycle records
SHA256SUMS             digests for every authored source and every render
```

## What is shared, and what deliberately is not

**Presentation is never shared.** Each candidate carries its own inline CSS and
its own markup. A shared stylesheet would become a premature system and would
make candidates converge on one house look — the exact failure this study exists
to avoid.

**Subject derivation is shared, on purpose.** Each case has exactly one subject
source, and every sibling loads it. Sharing these guarantees that candidates
differ only in encoding, never in content, and it lets `tools/verify.mjs` prove
it — for every case, not just P1:

| Case | Source | What is bound to the repository |
|---|---|---|
| P1 | `shared/plan-model.js` | the dataset is byte-identical across all three candidates and matches `project-management/tasks/*.md` frontmatter |
| P2 | `shared/tsk007-facts.js` | every material claim carries a quotation checked verbatim against `docs/verification/tsk-007-presentation/README.md` |
| P3 | `shared/p3-convergence.js` | every round matches `git log`; every quoted diff line matches `git show` character for character; a re-opening that came from off the round axis must name its origin, and the origin's identifying tokens must appear in its own verbatim quotation |
| D1 | `shared/repo-entry.js` | every rule id is emitted by a named enforcement source; every quotation is verbatim; a plane's narrow-viewport label may only use words from its full name |
| D2 | `shared/record-authority.js` | every frontmatter value matches the real record file and every quotation is verbatim |

On top of that, **every candidate publishes its derived answer at runtime**, and
`verify.mjs` recomputes that answer from the repository rather than reading it
off the page — so a candidate cannot display an answer the sources do not
support, and two siblings cannot quietly disagree. Every recorded render is
compared, not one per page.

**A shared source is data, not trusted code.** It is repository-controlled, and
everything it declares reaches an interpreter, the filesystem or `git`, so
`tools/source-authority.mjs` qualifies all three before they do:

- it is evaluated in a fresh `node:vm` context with no Node globals and code
  generation disabled, bounded by a wall clock, and only a JSON snapshot taken
  inside that same context crosses back — so nothing it defines ever runs in the
  verifier, and every check recomputes from its raw fact arrays rather than
  calling anything it exports;
- a declared source path is read from exactly one root — the repository, or the
  study for its own records, named explicitly and with no fallback between them
  — and only when it is a canonical regular file inside that root. Traversal, an
  absolute path, a symlink leading out, a directory, a missing file and an empty
  file are each refused by name, so a repository claim can never be quietly
  satisfied by a file this study wrote;
- a revision reaches `git` only as a hex object id and a diff path only as a
  `:(literal,top)` pathspec, so neither can become an option (`--output=<file>`
  alone makes `git log` write a file) and neither can widen a diff past the one
  path the page names.

`tools/selftest.mjs` sections G, H, I and J prove each of those refusals,
including that `verify.mjs` really applies them.

## Running the study

```sh
node docs/verification/tsk-014-w2/tools/render.mjs
node docs/verification/tsk-014-w2/tools/selftest.mjs           # harness contracts
node docs/verification/tsk-014-w2/tools/verify.mjs --update    # refresh SHA256SUMS
node docs/verification/tsk-014-w2/tools/verify.mjs             # compare, fail on drift
```

Sections A, B, B2, D, E and E4 of `selftest.mjs` drive `render.mjs` and need the
same host state it does. On a host that denies it, run the rest with
`--only=C,F,G,H,I,J`; a selected run names the sections it did not run and
claims nothing for them.

`render.mjs` uses the repository's already-locked `playwright-core` and
`axe-core`. It **resolves the browser and those modules before touching any
owned output**, so a missing browser cannot destroy the previous evidence set.

It renders **reproducibly**: every page is settled for fonts and two committed
frames before the shot, and Chromium is launched with its tiled-raster and
threaded-compositing paths disabled. Without that, a run re-rasterises a few
dozen anti-aliased edge and glyph pixels differently each time — no layout or
content change, but enough to move a digest and make two consecutive runs
incomparable. With it, two consecutive normal runs produce all 70 renders
byte-identical, which is what makes `SHA256SUMS` a drift check rather than a
timestamp.

`render.mjs` needs host state a restrictive sandbox denies: process inventory
(`ps`), signal delivery, and a profile socket directory. Denied any of them it
fails closed at launch, retains its marked root and deletes nothing. That is the
ownership contract working, not a defect — never substitute a manual `rm -rf` or
a broad process match for it.
It then creates one marked task root holding the profile and every redirected
environment root — `HOME`, `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`, `TEMP` and
an owner-only `XDG_RUNTIME_DIR`, so the operator's runtime directory and sockets
are never exposed. It renders sequentially, captures console and page errors,
and allows `file:` reads **only inside the study root**, with the decoded path
canonicalised so traversal, percent-encoded traversal and sibling-prefix paths
are all rejected and recorded separately from remote requests.

Cleanup does not trust a successful close: the owned identity set is refreshed
again afterwards, any still-live exact identity triggers the same
fingerprint-rechecked TERM/KILL fallback, and the marked root is removed **only
on positive proof** — an inventory that completed and showed zero live exact
identities. Any inventory failure is uncertainty: the root is retained, the run
fails, and nothing is deleted. Only `ENOENT` counts as proof of removal; any
other `stat` error is recorded as uncertainty. A failure to write the lifecycle
record fails the run rather than being swallowed. A normal run fails if the
fallback was needed at all, and cleanup failures are preserved alongside any
original failure. It never launches headed and never touches the operator's
browser.

Deletion authority is a **positive ownership contract**, not a directory
comparison. `--output-root` is accepted only when it canonically resolves inside
the host temporary directory, is an owner-only directory owned by this user, and
carries a `.cf-w2-selftest-root` marker whose token matches `--output-token` —
checked before any `mkdir` or `rm`. In the study and in a valid self-test root
alike, only a closed set of known output names is ever removed; anything else is
reported and left alone.

`verify.mjs --update` runs every other check first and **refuses to refresh
`SHA256SUMS` while any check has failed**, leaving the recorded inventory
byte-identical. `selftest.mjs` proves the paths a healthy run never reaches,
each in its own owner-only temporary root.

The self-tests cover: a task-owned survivor outliving the close (A); traversal,
percent-encoded traversal, absolute and sibling-prefix file reads (B); a symlink
inside the study pointing outside it (B2, POSIX hosts only); a refused inventory
refresh (C); an injected inventory failure proving the root is retained and no
deletion attempted (D); unmarked, wrong-token and marked-but-non-temp output
roots refused with foreign canaries byte-identical (E1–E3); an unexpected file
in one output directory blocking any deletion in the other, proving the reset is
inventory-then-clear rather than clear-while-scanning (E4); and a pure
containment table including Windows drive and UNC semantics (F). **Native Windows runtime
behaviour is not exercised anywhere in this study** — F evaluates path logic
only, and no Windows claim is made from it.
