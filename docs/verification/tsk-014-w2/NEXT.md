# Exact continuation boundary

The board is complete: five baselines, twelve candidates, seventy renders, all
deterministic checks green. Nothing below is authored work waiting to be done.
What remains is an observation this session structurally cannot perform.

## The boundary

**G1–G8 have not been run, and cannot be run from here.** Every gate in
`rubric.md` needs an observer who authored none of the candidates and has not
read `answer-key.md`. The author fails the first condition; the pass-1 integrity
reviewer failed the second. See `observer-state.md`.

Nothing in this directory records an author observation, an operator selection,
or a production-readiness judgement. The deterministic checks say the study is
internally consistent and accessible. They say nothing about whether any
composition communicates.

## What the next session does

1. Open `board.html` in a fresh session with an observer who authored none of
   the candidates and has not read `answer-key.md`. That session receives the
   board and the five task questions only.
2. Run G1–G8 as written in `rubric.md`, per case, recording each result and
   each *not run* as *not run*. The rubric is byte-pinned by `rubric.lock.json`
   and must not be edited: if a gate genuinely needs to change, that is a new
   dated rubric and a new lock, never an edit in place.
3. Report the observed results. **The operator selects the direction**, from
   the observed results plus the neutral implementation-feasibility record in
   `board.html`. No session before that point may narrow the set.

## Rerunning the study

```sh
node docs/verification/tsk-014-w2/tools/render.mjs
node docs/verification/tsk-014-w2/tools/selftest.mjs            # --only=C,F,G,H,I,J without a browser
node docs/verification/tsk-014-w2/tools/verify.mjs --update
node docs/verification/tsk-014-w2/tools/verify.mjs
```

`render.mjs` needs host state the repository sandbox denies — process
inventory (`ps`), signal delivery, and a profile socket directory. Under the
sandbox it fails closed at launch, retains its marked root and deletes nothing,
which is the designed behaviour, not a defect. Run it in a session where those
are available. Never substitute a manual `rm -rf`, a broad process match, or
any deletion outside the harness: the ownership contract in `render.mjs` is the
only sanctioned deletion path, and a retained root is evidence, not permission.

## Constraints that carry forward

- If a candidate is added or changed, it needs an entry in `CANDIDATES`
  (`tools/render.mjs`), in the per-case lists (`tools/verify.mjs`), in `cases`
  and in the implementation-feasibility table (`board.html`). All of those
  iterate lists; no other structural change is needed.
- A new fact belongs in its case's shared subject source with a matching rule in
  `tools/verify.mjs`, never inline in a candidate. A candidate may not carry a
  claim the sources cannot bind.
- A shared source is data. It is evaluated in a bounded context with no Node
  globals, and every path, revision and pathspec it declares goes through
  `tools/source-authority.mjs`. A source that needs to quote one of the study's
  own records adds it to `STUDY_OWNED_SOURCES` in `tools/verify.mjs`
  deliberately — there is no fallback that would find it by accident, and adding
  `answer-key.md` there would let a candidate quote the answers.
- D1 and D2 must not contradict the TSK-008 `system-atlas` boundary/legend/
  evidence-chain idea; it is a conformance input, not a reopened question.
- Siblings must stay structurally distinct. P3-a locates change in source and P3-b
  puts state transitions on a round axis; D1-a orders concepts by what cannot be
  read before what, D1-b is one path with refusal branches and D1-c is rule ×
  plane ownership with a boundary drawn through it; D2-a shows how far a decision
  reaches and D2-b pairs each claim with its evidence at eye level. Converging any
  two of them into one structure destroys what G6 exists to test.
