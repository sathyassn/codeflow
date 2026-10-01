# Waiting on and classifying PR checks

Read from `pr-evidence.md`, "After opening", when a required check is red or
stuck, or the adopted policy requires hosted checks green before landing.

## The bounded wait

Only where the adopted policy requires hosted checks green before landing,
run `timeout 30m gh pr checks <url> --required --watch --interval 60`.
Use the harness command timeout of 1800 seconds if `timeout` is unavailable.
This is the only polling command: do not add a preliminary poll, a parallel
poll or a background loop. Wait for this command to finish before reporting
readiness; timeout means missing evidence, not a failed product check.
Never run `--watch` without the ceiling.

If remote protection marks no check required, `--required` fails with
`no required checks reported`. An empty list is not readiness. In that case
only, replace the watch with `timeout 30m gh pr checks <url> --watch
--interval 60`, within the original thirty-minute budget. Match the returned
runs to the project gates. A gate with no readable matching run is missing
evidence.

## Redness classes

Classify each red or stuck check with the quality contract's redness classes:

- **Assertion-red, caused by this change.** Return it to its owner (skill
  step 1), fix it, run the check locally where you can, and push. Do not
  wait for the operator to name the job. A fix that changes the accepted
  contract goes back to `cf-plan`.
- **Red, and you cannot run it locally.** Read its log with
  `gh run view <run-id> --log-failed`. Fix it if the cause is in the change;
  otherwise report it as red with the failing line. It still blocks the PR.
- **Infrastructure-incomplete.** The job was queued past the budget,
  cancelled, lost its runner, or never started, for example because GitHub
  Actions was refused for billing or a spending limit. Report it as missing
  evidence with the reason the tool gave, not as a product defect. A
  completed green run of the same check still counts, as the quality contract
  says. Do not rerun the same job without a new reason.

Classify CI redness with the quality contract: assertion-red blocks;
an infra-killed job that only restacks already-green checks does not. If
the host merge UI still requires that unfinished job by name, the human
waits, reruns, or overrides. That is merge authorization, not a failed test.
