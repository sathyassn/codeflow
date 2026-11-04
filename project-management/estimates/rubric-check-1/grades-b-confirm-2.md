Grader B second confirmation: Claude Opus 5.5, medium, the same subagent as grades-b.md; reduced assurance, Codex unavailable

# Grader B's check of the second pass

I checked commit `8f89e411a` (the branch `task/TSK-1013-the-estimation-method-is-in-the-shipped-referenc`) against `fae18a186`, reading from git only. I did not write in the repository.

## Result

- **All six of my points are fixed.**
- **The commit introduces four new problems.** Two stale pins will fail checks. Two new rules point at things that do not exist. One of the new rules also reaches past what the reliability check allowed. Details are in the second table.

## My six points

| # | Point | Fixed | Evidence |
|---|---|---|---|
| 1 | `rubric.md` carries the boundaries, the new R line and the 8 anchored examples in all five homes | yes | All five homes give the same SHA-1, `9de910f08f7ae3f2f66e7a0b382824a58cd61a39`: `.agents/skills`, `.claude/skills`, both `.codeflow/.baseline` copies and `assets/base/agents/skills`. The S4, S5, R, C and X, and H boundaries are under "Boundaries the reliability check settled". The R line reads "a regression check that existing behaviour stays unchanged is not a refusal or safeguard obligation". "## Anchored examples" has 8 rows. The example forecast's pin, `91e4ada0…`, matches the new file's SHA-256 in all five homes. |
| 2 | TSK-293 is R 0, with the same rule applied to TSK-1149 AC-4 and TSK-239 AC-7 | yes | `reconciled.md` row 9 now reads R 0, sum 2, kind "ambiguity". The second-pass note applies the rule to TSK-1149 AC-4 (seven to six, still R 1) and to TSK-239 AC-7 (counted by neither grader). Neither changes a band. |
| 3 | The S5 wording | yes | The rubric reads "counts for the package that owns the model behaviour or the graded runs that accept it; a package that builds evaluation tooling whose live runs belong to another task scores the tooling ... not S5". It says "or", so it no longer requires both. |
| 4 | The S4 wording | yes | The rubric reads "counting or exact lookup over recorded text is not S4; a matcher that normalizes, or a graph walk, proved by a golden case table, is S4". The contradiction is gone. |
| 5 | The TSK-019 anchor note | yes | The rubric row says "C scores the custody change and X its confirmation, as the C and X boundary above decides". `reconciled.md` adds "X 1 rests on the reconciled C and X ambiguity". |
| 6 | My corrected counts | yes | The second-pass note records Easy 4, Medium 7, Hard 7, Very Hard 4, and that my earlier report was wrong. |

## New problems in this commit

| Severity | Finding | Evidence | Fix |
|---|---|---|---|
| Material | The managed-file manifest still pins the old rubric hash, so `manifest_consistency` should fail. I did not run the check. | `git show 8f89e411a:.codeflow/manifest.json` lines 281 to 285 give `.agents/skills/cf-estimate/references/rubric.md` sha256 `4d0ab400…`. The file is now `91e4ada0…`. The `.claude` entry near line 1127 has the same source. | Update the manifest hashes, and check whether the baseline record needs regenerating, through the usual update path. Then run `manifest_consistency`. |
| Material | cf-evaluate-model's embedded forecast fixtures still pin the old rubric hash. | `.agents/skills/cf-evaluate-model/resources/fixtures.json` lines 297 and 310 (and their mirrors) embed `"rubric": {"sha256": "4d0ab400…"}`. Only `examples/forecast.json` was updated. Unless a fixture is meant to carry a stale rubric pin, the estimate checker will now also report that the rubric changed, which the case does not intend. The pack's eval results could shift. | Update the embedded pins, or state why each fixture keeps the old one. |
| Material | The new band-move rule and the guard paragraph's "security review set is a reason to move the band" are outside the reliability check's scope. They also point at things that are not in the tree. | Rule 3 of `cases.md` lets a sentence from this check enter the rubric "only as an anchor or a clarification". The band-move rule does not come from any reconciled case. It cites "an adopted time-box profile (operating-model.md)", but `assets/base/agents/skills/cf-estimate/references/operating-model.md` at `8f89e411a` has no time-box profile (grep finds "time-box" only in `rubric.md`). The note cites ADR-0096, but no ADR-0096 file exists at `8f89e411a`; the string appears only in the rubric copies. The task's other units may be meant to add both. | Land the band-move and time-box text with the change that adds the profile and ADR-0096, or add those in this unit. Keep the check's boundaries separate from it. If this rule changes how grades are used, it needs the operator's decision recorded, not this reconciliation. |
| Minor | The rubric changed bytes but kept its version label. | Line 6 still reads `codeflow-agentic-1`. The new dispute rule says to amend the rubric "by a versioned adaptation", and forecasts pin the rubric by hash. | Either bump the version, or state in the ADR-0096 note that anchors and clarifications keep the version. |
| Nit | Two anchor rows state one grader's obligation count as if it were settled. | TSK-1149 "six refusals" is A's count after removing AC-4; I counted five. TSK-212 "nine refusals" is A's count; I counted ten. The R points agree, and rule 2 required no reconciliation of counts. | Drop the counts or write "at least" (for example "R 2 (eight or more refusals)"). Disposition: fix now, it is one line each. |

## What I did not verify

- I did not run `manifest_consistency`, `artifact_budget_contract`, the reading-budget doctor check or the cf-evaluate-model pack. The rubric grew to 11,285 bytes, and its effect on the reading budget is unchecked.
- I did not read the branch's other commits for a planned time-box profile or ADR-0096.
