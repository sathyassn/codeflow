# CodeFlow time-box profile, version 1

Profile version: 1; effective 2026-11-05. Method `codeflow-agentic-2`
(`cf-estimate` rubric), with the time-box profile of
`operating-model.md`, "Time-box profile" (ADR-0096). The bands are issue
334's, chosen by the operator as CodeFlow's starting model on 2026-11-01.

## Confirmation

The operator's own record of these bands is issue 334
(https://github.com/sathyassn/codeflow/issues/334), "Band" section, which
reads:

```text
| Band | Clock hours |
|---|---|
| Easy | 1 to 2 |
| Medium | 2 to 4 |
| Hard | 4 to 6 |
| Very hard | 6 to 8 |
```

with "Very hard is always split before claim". The operator ruled on
2026-11-04, relayed by the Director at 20:5x: "issue 334 is the operator's
own record of the bands, so cite it as the confirmation; nothing goes back
to the operator".

## The clock

- **Started:** the claim commit, or the earlier first task commit for an
  adopted branch.
- **End:** the later of the bound approval and the completion of
  verification; the landing stands in for the end only by ancestry, for a
  merged landing whose completion a later records change wrote, never by
  comparing clocks.
- Recorded blocked spans (reason, owner, revisit) are subtracted. A busy
  reviewer is not a block. Waits are never inferred.

## Bands

| Grade | Points | Band (clock hours) |
|---|---|---|
| Easy | 0 to 1 | 1 to 2 |
| Medium | 2 to 4 | 2 to 4 |
| Hard | 5 to 8 | 4 to 6 |
| Very Hard | 9 or more | 6 to 8; split before claim |

A band may move one step from its grade, never two, for a reason recorded
in the task's Estimate block. A band top is a stop rule for one task, not a
forecast and not a gate.

## Full delivery and comparability

- Full delivery is the task's criteria proven, its independent review
  bound and its verification complete, as the end point above states.
- Builders and reviewers are the seats `.codeflow/model-selection.json`
  and the task's Execution contract name; host capacity is one machine,
  shared by parallel worktrees.
- Only tasks created on or after the effective date carry a band. Older
  tasks read "no band" and are never back-filled. A task keeps the profile
  revision it was frozen with.

## Prospective evaluation

Hits, misses and breaches are read at each epic close from
`codeflow estimate outcomes` and kept in `outcomes/<epic id>.md`. After
the first epic closed under this profile, its outcome record decides
whether the bands are revised in `profile-v2.md`. No calibration data is
seeded here.
