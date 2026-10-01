## Blocker navigation and gate redness

Do not confuse missing evidence with missing operator intent. The
classification of an impediment, the bounded probe, the one confirmation of a
prior failure and the escalation form are stated once in the workflow
discipline rules, "Navigate blockers". What this section adds:

- for a defect that resists a first glance, that probe is one command
  already run that fails on the **exact reported symptom** before
  hypothesising;
- an external dependency or enforced gate is recorded with the exact evidence
  or input that clears it;
- a choice that `cf-method/references/autonomy.md` reserves to the operator
  goes to them as one question with a recommendation; check its "What belongs
  to the operator" list at this point, not from memory;
- the departure form, for a change that newly departs from the approved
  contract, scope, authority or risk boundary (a public contract break, a
  moved security boundary, scope growth, an irreversible action): situation
  with evidence, the boundary crossed, options with cost and reversibility,
  and one recommendation. An already approved departure is reused and not
  asked again. Compatibility is judged by the git rules' breaking-change rule
  and the cf-ship release-policy reference (affected consumers, migration or
  deprecation, mixed-version operation, recovery).

Gate failure is information to fix or honor, not automatic evidence that the
operator must decide.

A **gate** is the verification check (`codeflow test` target, coverage floor,
OSV/security scan, `validate --docs`, …), not the CI job that happens to run
it. Classify redness before acting:

1. **Assertion-red.** The check ran to completion and failed its contract.
   Honor it: fix, or do not ship. Model consensus and a green sibling job for a
   *different* check cannot override it.
2. **Infra-incomplete.** The job never finished (hosted runner lost
   communication, SIGTERM/shutdown, OOM, billing or minutes cutoff, timeout
   with no test result). That is missing *job* evidence, not a failed check.
   Do not treat the job name as a failed test. If the same check already
   completed green in a sibling CI job or a local `codeflow test` /
   `codeflow validate` run of that target, the gate is evidenced; record the
   infra death as `track once` (runner capacity or job shape), not a product
   defect. Retrying the same unfinished umbrella job without a new hypothesis
   is orbiting.
3. **Never ran.** The owed check has no completed result anywhere and no
   local equivalent. That is a missing gate: name it as the blocker, never a
   pass. The pull request stays draft where required evidence is missing, and
   other authorized work continues.

A red job that only restacks already-green checks is (2), not (1). Asking the
operator to pick an implementation tactic because a job name is red is the
failure ADR-0038 forbids. Asking them to wait, rerun, or override a host
required-status that is infra-incomplete *is* operator-owned: it is merge
authorization on that host, not a failed test. "Ready for your merge on local
evidence" needs a completed green result of every owed required check at the
pull request head; `cf-ship`'s `references/pr-evidence.md` owns that report.
Merges follow `cf-ship` step 8: the primary merges only into an `integration/`
branch no protected-branch policy covers, and a human merges every protected
target.

A failing or missing gate cannot be overridden by model consensus.
"Failing" means an assertion-red completed check. "Missing" means the owed
check never completed anywhere. An infra-incomplete duplicate job does not
make a completed same-check missing.
