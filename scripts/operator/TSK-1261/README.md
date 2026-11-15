# Changed-line coverage workflow patch

The patch adds changed-line reporting where coverage already runs. It also
updates the coverage job templates in `scripts/gate-parity.py`, so a missing
binding, base input or report step fails parity. It leaves the aggregate
coverage command, workflow event placement and shipped scaffold unchanged.

The builder commits only this patch package. The operator applies its two
changes after the task's independent security review. The patch is pinned to
the workflow and parity bytes at the task's base, `87874f43fb14d4e9c189d3bc8b00d340fe4d03e6`.
Overlapping work must reconcile the patch before applying it.

From the task worktree, these checks establish that the inputs match HEAD
and the patch applies without changing them:

```sh
git diff --exit-code HEAD -- .github/workflows/codeflow-ci.yml scripts/gate-parity.py
python3 -B scripts/operator/TSK-1261/apply.py --check
```

`--check` checks the patch digest and both preimage digests, applies exact
hunks to temporary copies, verifies the resulting digests and prints them.
It needs only Python. It reads no git configuration or remote and opens no
terminal. A mismatched preimage fails without changing either source file.

The builder's `scripts/test_changed_coverage_patch.py` runs the real parity
checker against patched copies and explicitly named support files in a
temporary directory. It also checks that removing required inputs fails.

After review, the operator runs `python3 -B scripts/operator/TSK-1261/apply.py
--apply`, reviews both resulting changes, commits them and obtains hosted
evidence through coord/ci's landing candidate. The builder never runs
`--apply`. The script refuses that mode inside a Claude agent session.

Expected output digests are recorded in `patch-manifest.json`. This package
contains no baseline or managed-manifest change because no shipped copy is
changed. The workflow checkouts still test the proposed merge revision, and
the report binds to that same `github.sha`.
