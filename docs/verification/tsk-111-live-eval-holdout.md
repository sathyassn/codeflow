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
  it first lived on the task branch), the digests of its files and of every
  JSON object in it, and fingerprints of its text, never its content. The
  fingerprinted text is its JSON string values, the whole source and string
  literals of its code, and any other file whole, but not its README, as runs
  of 12 words; text `assets/` and `evals/model-artifacts/` already hold when
  the manifest is written is left out.
- `test_eval_kit.py` runs `holdout_leaks` over the tracked tree in every
  `codeflow test`, so CI fails on a holdout path, a copied holdout file, a
  holdout object nested anywhere in any file that parses as JSON whatever its
  name, and a copied run of 27 or more consecutive words of the fingerprinted
  text, as written or as a JSON string (a lone JSON document, a value inside
  one, or an escaped literal). With the checkout at hand it also fails on an
  id or rubric opening only the holdout holds and on a manifest that no longer
  matches the holdout.
- The holdout also keeps the labelled judge controls (`judge-controls.json`):
  reviews whose free text reverses their verdict in paraphrase, with the
  judgement a qualified judge must record. The grader counts a judgement only
  from a judge whose calibration meets every control; any other judgement
  leaves its assertion ungraded and the trial not measured. Scoring re-reads
  the retained judgements and calibration files and takes each judge from
  the judgements themselves, so a saved pass stops counting once the
  calibration of the judge who wrote it is edited, removed or swapped.
  Every judgement and calibration is signed under the evaluator key when it
  is recorded, so a verdict relabelled as another judge, or signed under
  another key, counts as no judge's. Grading signs each grade with a receipt
  that binds every judgement it read, and scoring counts a saved pass only
  when the receipt verifies and those retained judgements rederive it, so a
  signed judgement of another excerpt, or a signed failure, pointed at by
  an edited grade, is not measured. The evaluator key is the trust
  boundary: its holder is trusted. The holdout's
  scripted judge misses controls by design and tests transport and
  fail-closed binding only; its synthetic oracle meets them and tests the
  binding path. Neither is evidence that meaning was judged.
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
- It promises exact copies only: whole files, JSON objects, and runs of 27 or
  more consecutive words of the fingerprinted text without the holdout; ids
  and rubric openings only with it. Shorter runs are often caught but not
  promised. A paraphrase, a run broken by edits, or an encoding other than
  JSON string escaping is not caught mechanically, and neither is text that
  sat in `assets/` or `evals/model-artifacts/` when the manifest was written
  (`--update` refuses a tree the current manifest finds a leak in).
- Whether the sync from the archive publishes only `main` is the operator's
  arrangement; it was not verified here.
