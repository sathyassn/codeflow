# Waiting on and classifying PR checks

Read from `pr-evidence.md`, "After opening", when a required check is red or
stuck, or the adopted policy requires hosted checks green before landing.

## The bounded wait

Only where the adopted policy requires hosted checks green before landing,
wait for them with a bounded poll: `gh pr checks <url> --required` at most
once a minute, for at most thirty minutes (exit code 8 means still
pending), or `timeout 30m gh pr checks <url> --watch --interval 60`. Do not
use `--watch` without a ceiling, and do not poll without an end: `--watch`
refreshes every ten seconds and has no timeout of its own. Remote protection
may mark no check required: then `--required` fails with `no required checks
reported`, and an empty list is not readiness. Read all check runs with
`gh pr checks <url>` on the same cadence and match each project gate to the
run that carries it. A gate with no matching run,
or whose run you cannot read, is missing evidence.

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
