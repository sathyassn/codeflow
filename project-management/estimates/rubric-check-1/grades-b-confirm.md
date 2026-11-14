Grader B confirmation of the reconciliation: Claude Opus 5.5, medium, the same subagent that wrote grades-b.md; reduced assurance, Codex unavailable

# Grader B's check of the rubric check reconciliation

Inputs, read from git: `reconciled.md` at `8c3511fc2`, `grades-a.md` at `7343daa61`, and a diff of `47403d356..8c3511fc2`. I did not write in the repository.

## Result

- **Cases where the reconciliation differs from my grade, points or H: 3.** I agree with 2 (TSK-278, TSK-130) and dispute 1 (TSK-293). The disputed point does not change the case's grade.
- **No rubric change exists at `8c3511fc2`.** `reconciled.md` says each ambiguity "is decided by a sentence now in `rubric.md`". But `git diff --name-only 47403d356 8c3511fc2` lists only the four files under `project-management/estimates/rubric-check-1/`. None of the five copies of `cf-estimate/references/rubric.md` changed, and the branch tip is `8c3511fc2`. The new sentences, including the S5 sentence behind TSK-130, and the anchored examples exist only in `reconciled.md`. Either a later commit must add them to the rubric, or that claim should be removed.
- `grades-b.md` at `bd85402fa` is byte-identical to my scratchpad file (checked with `cmp`), as the reconciliation says.
- The factual claims check out: 18 of 22 grades agree (disagreements on TSK-189, TSK-241, TSK-130 and TSK-015), points differ on six more cases, and the reconciled distribution is Easy 4, Medium 8, Hard 6, Very Hard 4.
- A correction to my own earlier report: my grades were Easy 4, Medium 7, Hard 7, Very Hard 4, not "3 Easy, 8 Medium". The table in `grades-b.md` was already right.

## Cases where the reconciliation differs from mine

| # | Task | Mine | Reconciled | Verdict | Reason |
|---|---|---|---|---|---|
| 9 | TSK-293 | R 0, sum 2, Medium | R 1, sum 3, Medium | dispute (grade unchanged) | AC-3 ("Without `--merge`, `integrate` shall rebase and fast-forward as at v3.1.0; evidence: the existing integrate tests unchanged") is a regression check. It governs no refusal or safeguard. The R definition asks for obligations that "govern refusal or safeguarding" and drops "vague quality checklist" items. The "invariant" clause is about phrasing, so it does not turn a regression check into a safeguard. The reason type "rubric" (B misread the rubric) does not fit: this is at most an ambiguity. If it stands, it has to apply to every case. A did not count "estimate check behaves as before" (TSK-239 AC-7) or the "TSK-1066 judgments unchanged" (TSK-1149 AC-4) the same way. Neither would change a band. |
| 15 | TSK-278 | S2 1, sum 6, Hard | S2 2, sum 7, Hard | agree | The Deliverables put "the redaction route" in `status_verb.rs` as well as `acceptance.rs`, so the package owns a record-writing operation, which is a mutation. I under-read the Deliverables. No criterion names the verb, so this is an inference from the Deliverables line. |
| 20 | TSK-130 | S5 3, S7 1, sum 6, Hard | S5 0, S7 1, sum 3, Medium | agree with the grade; the sentence needs rewording (see below) | AC-5 says "The live sessions and their grades are TSK-116's evidence". The guidance behaviour being graded comes from TSK-127 and TSK-128 (AC-4). So TSK-130 owns neither the model behaviour nor its acceptance evidence. It owns the kit, which its own tests prove over synthetic transcripts. My S5 3 rested on "the package owns the proof", and the record gives the proof to TSK-116. That reading was wrong. |

The other cases match my grades, points and H. The remaining reconciliation rows (TSK-1145, TSK-225, TSK-105, TSK-189, TSK-241, TSK-015, TSK-019) settled on my values. I accept them.

## TSK-130 and the S5 sentence

- **The original text.** "Count a surface only when the package owns the changed behavior/proof." S5 asks: "Owns model behavior accepted through grounding, refusal or evaluation evidence?" The slash in "behavior/proof" leaves open whether owning the proof alone is enough.
- **The new sentence**, in `reconciled.md` only: "S5 counts for the package that owns the model behaviour and its acceptance evidence, not for one that builds the evaluation kit."
- **Clarification or change.** The sentence changes no point value or cut, and it agrees with S5's own question ("owns model behavior") and with "consuming another component's capability does not own that component". So it is a clarification in kind. But "behaviour and its acceptance evidence" requires both, where the original "behavior/proof" reads as either. That narrows the original text a little.
- **The grade does not depend on that narrowing.** TSK-130 owns neither the behaviour nor the evidence (AC-5), so it scores S5 0 under either reading.
- **Suggested wording**, which clarifies without narrowing: "S5 counts for the package that owns the model behaviour or the graded runs that accept it; a package that builds evaluation tooling whose live runs belong to another task scores the tooling, not S5." It decides TSK-130 the same way.
- **Whether it was written to fit the result.** The reconciler was grader A, and the sentence lands on A's S5 0. That is the risk the reconciliation itself names. The record supports the result independently (AC-5's amendment), so I find it justified on the evidence, not fitted.

## The anchored examples

Every anchored example matches how its case was reconciled, and in each case both graders reached the same grade, as rule 5 requires.

| Grade | Case | Drivers in reconciled.md | Matches the reconciled table | Both graders gave this grade |
|---|---|---|---|---|
| Easy | TSK-1018 | none, 0 | yes | yes |
| Easy | TSK-228 | S2 1 | yes | yes |
| Medium | TSK-212 | S2 2, R 2, 4 | yes | yes |
| Medium | TSK-1149 | S2 1, R 1, 2 | yes | yes |
| Hard | TSK-239 | S2 1, S4 3, R 1, 5 | yes | yes |
| Hard | TSK-322 | S2 2, S4 3, R 2, 7 | yes | yes |
| Very Hard | TSK-119 | S1 3, S2 2, S3 3, R 2, 10 | yes | yes, with the same drivers |
| Very Hard | TSK-019 | S1 3, S2 2, S7 1, C 2, X 1, R 2, 11 | yes | yes; A had X 0, and X 1 is the reconciled point |

Three problems with the examples:

1. **They are not in `rubric.md`.** Rule 5 says the anchors become the rubric's examples, each pinned as `<task id>@47403d356` with its driver line. None of that is in the rubric at `8c3511fc2`.
2. **The S4 sentence contradicts itself.** It reads: "counting or matching recorded text is lookup; a graph walk or normalizing matcher proved by a golden case table is S4". TSK-322's matcher also matches recorded text, so the two halves overlap. The TSK-228 and TSK-322 anchors depend on this line. Suggested wording: "counting or exact lookup over recorded text is not S4; a matcher that normalizes, or a graph walk, proved by a golden case table, is S4."
3. **The TSK-019 anchor depends on a reconciled ambiguity** (C and X both scored for one confirmed transfer). The anchor's driver line should cite that decision so a later reader sees why X is 1.
