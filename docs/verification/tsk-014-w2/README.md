# TSK-014 W2 — rendered composition study

Disposable design evidence for the TSK-014 recovery. It settles five composition
questions with rendered candidates instead of prose, and is **retained until the
operator records a decision**.

Nothing here is production authority. No file in this directory is referenced by
the renderer, the schema, the documentation portal, the scaffold, or any managed
artifact.

> **G1–G8 are not run.** Deterministic checks pass; that means the study is
> internally consistent and accessible, not that any composition reads. The
> author's self-critique is evidence, not a verdict. See `observer-state.md`.

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

Authored and verified: the study frame, the pre-registered rubric and its lock,
the real-content inventory and answer key, the P1 and P2 baselines, and five
candidates — `p1/{a-wave-lanes,b-blocking-matrix,c-critical-ribbon}` and
`p2/{a-evidence-grid,b-provenance-rail}` — with renders and checks.

Not authored: P3 ×2, D1 ×3, D2 ×2. Exact boundary in `NEXT.md`. No placeholder
stands in for an unauthored candidate.

## Layout

```
README.md              this index
rubric.md              pre-registered gates (byte-pinned; never edited in place)
rubric.lock.json       the rubric digest plus an honest record of how ordering was observed
observer-state.md      why G1-G8 are not run and when they can be
content-inventory.md   every fact used, with its repository source path
answer-key.md          the exact task question per case and its verifiable answer
NEXT.md                the precise continuation boundary
baselines/<case>/      chat.md, markdown.md, baseline.html — authored before candidates
cases/<case>/<candidate>/index.html   one self-contained page per candidate
shared/plan-model.js   the P1 subject derivation (waves, chain, room) — no styling
shared/tsk007-facts.js the P2 fact registry, every claim carrying a verbatim quotation
board.html             neutral comparison surface + implementation-feasibility record
tools/render.mjs       exact-owned sequential headless render + axe + network + lifecycle
tools/verify.mjs       integrity, content binding, checksums (`--update` to refresh)
tools/selftest.mjs     proves the survivor, file-containment and update-guard paths
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

**Subject derivation is shared, on purpose.** `shared/plan-model.js` computes
waves, the longest chain and room; `shared/tsk007-facts.js` holds the P2 facts.
Sharing these guarantees that candidates differ only in encoding, never in
content, and it lets `tools/verify.mjs` prove it: the P1 dataset is byte-identical
across all three candidates and matches `project-management/tasks/*.md`
frontmatter; every P1 candidate publishes its derived answer at runtime and all
three must agree with an answer recomputed independently from the repository;
every material P2 claim carries a quotation checked verbatim against
`docs/verification/tsk-007-presentation/README.md`.

## Running the study

```sh
node docs/verification/tsk-014-w2/tools/render.mjs
node docs/verification/tsk-014-w2/tools/selftest.mjs           # harness contracts
node docs/verification/tsk-014-w2/tools/verify.mjs --update    # refresh SHA256SUMS
node docs/verification/tsk-014-w2/tools/verify.mjs             # compare, fail on drift
```

`render.mjs` uses the repository's already-locked `playwright-core` and
`axe-core`. It **resolves the browser and those modules before touching any
owned output**, so a missing browser cannot destroy the previous evidence set.
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
