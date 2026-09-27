# Live delivery evaluation holdout route

This records where the six live delivery cases of SPC-013 R-105 live and why,
for TSK-111. They are the qualification holdout of the TSK-116 sessions: the
cases, fixtures, expected values and the scripted solutions that prove the
grader. None of it lives in this repository. The generic grader, its public
development suite and the checks that keep the holdout out stay here.

The public repository is a filtered copy of the private archive's `main`, so
anything merged into a CodeFlow line would be published, and a published case
counts as exposed for good: a model can meet it in training before any
session starts, which no runtime confinement undoes. A filter at every sync
would be one manual step between the holdout and publication, so the holdout
never enters a line at all.

```text
 private archive (sathyassn/codeflow-archive)
 +---------------------------------------+     +----------------------------------+
 | main, integration/*, task/*           |     | test/live-delivery-holdout       |
 |   eval_kit.py (the generic grader)    |     |   orphan history, never merged   |
 |   evals/grader-dev/  public dev suite |     |   cases.json fixtures.json       |
 |   evals/holdout.json paths, digests   |     |   packs.json                     |
 |   holdout-check in test_eval_kit.py   |     |   tests/test_live_pack.py        |
 +------------------+--------------------+     |   tools/gen_live.py              |
                    | filtered sync of main    +----------------+-----------------+
                    v                                           | checkout beside
 public repository: no holdout path, file,                      | a CodeFlow tree
 entry or string (holdout-check in CI)          --graded-suite <checkout>
```

## Route

- The holdout ref is `test/live-delivery-holdout` on the private archive. Its
  history is an orphan with no commit shared with the CodeFlow lines, so a
  merge into a line is refused as unrelated history unless forced. Its
  `README.md` says it is never merged.
- An evaluator checks it out beside a CodeFlow tree and passes it as
  `--graded-suite <checkout>` to the kit. The run root records the suite and
  its digest.
- `CODEFLOW_EVAL_HOLDOUT=<checkout> cargo test -p codeflow-cli --test
  live_eval_pack` runs its dry grading against this tree's kit and the built
  binary. Without the variable the test says it was skipped and checks
  nothing.
- `evals/holdout.json` records the holdout's forbidden paths
  (`evals/live-delivery/` and `evals/model-artifacts/test_live_pack.py`, where
  it first lived on the task branch) and the digests of its files and suite
  entries, never their content. `test_eval_kit.py` runs
  `holdout_leaks` over the tracked tree in every `codeflow test`, so CI fails
  on a holdout path, a copied holdout file or a copied case, fixture or pack.
  With the checkout at hand it also fails on any string only the holdout
  holds (case, fixture and long assertion ids, rubric openings) and on a
  manifest that no longer matches the holdout.
- After changing the holdout, run `eval_kit.py holdout-check --manifest
  evals/holdout.json --holdout <checkout> --update` and commit the manifest.

## History

The holdout was only ever on the unprotected task branch
`task/TSK-111-live-eval-grader` of the private archive (commits 825ab5046,
3dc9bd76a and f6eaa48a2). The branch was rewritten so no commit on it
carries the holdout, with a local backup ref taken first. It was never on
`main` or an integration line, so nothing was published and the cases need
no rotation.

## Limits

- The check covers the tracked tree of the checkout it runs in, not other
  refs or history; the rewrite covers the task branch.
- It catches exact copies of holdout files and suite entries without the
  holdout, and strings only with the holdout checked out. A paraphrase of a
  case is not caught mechanically.
- Whether the sync from the archive publishes only `main` is the operator's
  arrangement; it was not verified here.
