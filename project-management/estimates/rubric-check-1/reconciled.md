# Rubric reliability check 1: reconciliation

Both graders agreed on the grade of **18 of 22 cases (82 percent)**. They
differed on four: TSK-189, TSK-241, TSK-130 and TSK-015. Inside agreeing
grades, points differed on six more cases, and the H flag differed on
four. Every difference is reconciled below under the predeclared rule
(`cases.md`, "The acceptance rule"). The rate is reported, not gated.

- Grader A: `grades-a.md`, committed at `7343daa61` before grader B's file
  was read.
- Grader B: `grades-b.md`, a fresh Claude Opus 5.5 subagent at medium that
  saw only `cases.md`, committed unedited at `bd85402fa`.
- Reconciler: grader A, reading both files against the rubric and the
  records at `47403d356`. A reconciler who was also a grader is a limit: a
  third reader did not check these reasons. Both grades came from one model
  family, so this check has reduced assurance (`cases.md` rule 6).

## Grades side by side

| # | Task | A | B | Reconciled | Grade agreement |
|---|---|---|---|---|---|
| 1 | TSK-1183 | 0 Easy | 0 Easy | 0 Easy | yes |
| 2 | TSK-1018 | 0 Easy | 0 Easy | 0 Easy | yes |
| 3 | TSK-219 | 0 Easy | 0 Easy | 0 Easy | yes |
| 4 | TSK-228 | 1 Easy | 1 Easy | 1 Easy | yes |
| 5 | TSK-212 | 4 Medium | 4 Medium | 4 Medium | yes |
| 6 | TSK-1301 | 3 Medium | 3 Medium | 3 Medium | yes |
| 7 | TSK-275 | 2 Medium | 2 Medium | 2 Medium | yes |
| 8 | TSK-1149 | 2 Medium | 2 Medium | 2 Medium | yes |
| 9 | TSK-293 | 3 Medium | 2 Medium | 2 Medium | yes |
| 10 | TSK-277 | 2 Medium | 2 Medium | 2 Medium | yes |
| 11 | TSK-1145 | 4 Medium | 3 Medium | 3 Medium | yes |
| 12 | TSK-225 | 9 Very Hard | 10 Very Hard | 10 Very Hard | yes |
| 13 | TSK-239 | 5 Hard | 5 Hard | 5 Hard | yes |
| 14 | TSK-105 | 8 Hard | 5 Hard | 5 Hard | yes |
| 15 | TSK-278 | 7 Hard | 6 Hard | 7 Hard | yes |
| 16 | TSK-119 | 10 Very Hard | 10 Very Hard | 10 Very Hard | yes |
| 17 | TSK-189 | 4 Medium | 5 Hard | 5 Hard | no |
| 18 | TSK-322 | 7 Hard | 7 Hard | 7 Hard | yes |
| 19 | TSK-241 | 7 Hard | 9 Very Hard | 9 Very Hard | no |
| 20 | TSK-130 | 2 Medium | 6 Hard | 3 Medium | no |
| 21 | TSK-015 | 9 Very Hard | 6 Hard | 6 Hard | no |
| 22 | TSK-019 | 10 Very Hard | 11 Very Hard | 11 Very Hard | yes |

Reconciled distribution: Easy 4, Medium 8, Hard 6, Very Hard 4. The cut
cases: 1 against 2 (TSK-228 at 1, TSK-275 and TSK-1149 at 2), 4 against 5
(TSK-212 and TSK-1145 near 4, TSK-239, TSK-105 and TSK-189 at 5), 8 against
9 (TSK-241 at 9; no case reconciled at 8).

## Each difference and its reason

Kinds, from rule 3: **record** (a grader misread the record), **rubric** (a
grader misread the rubric) and **ambiguity** (the rubric text does not
decide).

| # | Task | Driver | A | B | Reconciled | Kind | Reason |
|---|---|---|---|---|---|---|---|
| 9 | TSK-293 | R | 1 | 0 | 0 | ambiguity | Second pass: AC-3 (rebase as at v3.1.0, existing tests unchanged) is a regression check, not a refusal obligation; that leaves three (the head not rewritten, a red gate and a conflict leave the line unchanged), so R 0. Applied the same way to TSK-1149 AC-4 and TSK-239 AC-7, neither of which changes a band. Decided by the new R line in the rubric. |
| 11 | TSK-1145 | S2 | 2 | 1 | 1 | rubric | Full, essential and quick are modes of one operation, `codeflow test`; with `reserve` that is two operations, no mutation and no lifecycle event, so no second point. |
| 12 | TSK-225 | X | 0 | 1 | 1 | ambiguity | A pinned table of reviewed archive digests, refusing any other bytes, records an approval of specific artifacts on the installer's own path. Resolved: a recorded approval that a path enforces is an approval mechanism. A digest check against a source the requester controls would not be. |
| 12 | TSK-225 | H | no | yes | yes | record | The operator applies the live workflow patches (AC-10, AC-14), a named human path. |
| 14 | TSK-105 | S1 | 3 | 0 | 0 | record | The Description says "The parser and the transition rules are TSK-102's"; the block shape is consumed. AC-8's fields are required by skill text and a fixture. |
| 15 | TSK-278 | S2 | 2 | 1 | 2 | record | The Deliverables put the redaction route in `status_verb.rs` as well as `acceptance.rs`, so the package owns a record-writing operation, a mutation. |
| 15 | TSK-278 | H | no | yes | yes | record | AC-1 requires a recorded review of the change, a named human approval path. |
| 17 | TSK-189 | S7 | 0 | 1 | 1 | record | AC-4: doctor and orient report "a local policy that differs from the landed one", drift detection. A did not read AC-4. Grade moves from Medium to Hard. |
| 17 | TSK-189 | H | no | yes | yes | record | The operator lands enforcement changes and runs `git remote set-head` (Description, AC-1). |
| 19 | TSK-241 | S7 | 0 | 1 | 1 | record | AC-6: a written index that no longer matches the items warns, drift detection of a projection. |
| 19 | TSK-241 | X | 0 | 1 | 1 | record | AC-3 and AC-4: a decline needs `--confirmed-by operator`, and validation fails a declined item without it, a named approval on its own path. Grade moves from Hard to Very Hard. |
| 19 | TSK-241 | H | no | yes | yes | record | Same anchor as X. |
| 20 | TSK-130 | S5 | 0 | 3 | 0 | ambiguity | S5 asks whether the package "owns model behavior accepted through ... evaluation evidence". The behaviour graded (rules kept across compaction) belongs to the guidance tasks, and the live runs are TSK-116's (AC-5). This package owns the measurement tooling, which scores as tooling. Resolved by a rubric sentence (second pass, grader B's wording): S5 counts for the package that owns the model behaviour or the graded runs that accept it, not for one that builds evaluation tooling whose live runs belong to another task. |
| 20 | TSK-130 | S7 | 0 | 1 | 1 | record | AC-4 and AC-5: the transcript check marks a trial without compaction `broken_fixture`, owned environment evidence. Reconciled sum 3, Medium. |
| 21 | TSK-015 | S1 | 3 | 0 | 0 | record | The record names no persisted shape; A took the forecast schema from SPC-007 and `records.md`, which `cases.md` puts out of scope. Grade moves from Very Hard to Hard. |
| 22 | TSK-019 | X | 0 | 1 | 1 | ambiguity | A confirmed whole-runtime transfer is both a custody change (C) and the approval that grants it (X). Resolved: C scores the act, X the named approval mechanism; both apply when the package builds both. |

R enumerations differed inside one R band on TSK-1301, TSK-275, TSK-1149,
TSK-277, TSK-278 and TSK-239 (for example A counted six obligations on
TSK-275 and B four, both R 1). No point changed, so no reason is owed
under rule 2; the counts are in the two grade files.

## Ambiguities both graders met

Each is decided by a sentence now in `rubric.md`, as rule 3 allows: an
anchor or a clarification, never a point or a cut.

| Ambiguity | Seen by | Decided by |
|---|---|---|
| Policy and config keys: S1 or S2 | both (A's note; B ambiguity 4) | keys are S2 contract members (the S2 definition) |
| A fix to an owned operation | both (TSK-212 at S2 2) | a fix that changes an owned operation's behaviour scores S2 |
| Whether X's list is closed | both (A's note; B ambiguity 1) | refusal rules, policy authority and exemption classes are R; a recorded approval that a path enforces is X (the guard anchors) |
| Parse-and-count against match-and-walk for S4 | both (TSK-228 S4 0, TSK-322 S4 3) | counting or exact lookup over recorded text is not S4; a matcher that normalizes, or a graph walk, proved by a golden case table, is S4 |
| Redaction as C | both (TSK-278, C 0, X 1) | a sanctioned edit that git history keeps is not destructive erasure; the route is X |
| S5 for an evaluation kit | both (A reasoned zero; B ambiguity 3) | S5 counts for the package that owns the model behaviour or the graded runs that accept it; evaluation tooling whose live runs belong to another task scores as tooling |
| A regression check under R | both, in the second pass (TSK-293) | a regression check that existing behaviour stays unchanged is not a refusal obligation |
| A recorded review or waiver as H | B (ambiguity 7), and A on TSK-105 | a required recorded review or operator confirmation is a named human approval path |

B's ambiguity 6 (scope statements and thresholds under R) and 8 (records
that defer to a spec) were met by B alone and changed no reconciled grade;
they are recorded here, not added to the rubric.

## Anchored examples chosen

Rule 5: up to two cases per grade whose reconciled grade both graders
accept. All are pinned at `47403d356`.

| Grade | Case | Reconciled drivers | Why it anchors |
|---|---|---|---|
| Easy | TSK-1018 | none, 0 | tests alone |
| Easy | TSK-228 | S2 1 | one report member; counting is not S4 |
| Medium | TSK-212 | S2 2, R 2, 4 | a fix scoring S2 at the 4 to 5 cut |
| Medium | TSK-1149 | S2 1, R 1 (six refusals), 2 | a guard change graded through R, not X |
| Hard | TSK-239 | S2 1, S4 3, R 1, 5 | owned computation at the 4 to 5 cut |
| Hard | TSK-322 | S2 2, S4 3, R 2, 7 | a guard with an owned matcher |
| Very Hard | TSK-119 | S1 3, S2 2, S3 3, R 2, 10 | three weighted drivers |
| Very Hard | TSK-019 | S1 3, S2 2, S7 1, C 2, X 1, R 2, 11 | migration and custody change; X 1 rests on the reconciled C and X ambiguity |

## Second pass, after grader B's confirmation

Grader B checked this reconciliation (`grades-b-confirm.md`, committed
unedited). It agreed with the TSK-278 and TSK-130 outcomes and disputed
TSK-293. This pass, in one commit:

- **TSK-293:** settled for B. A regression check is not a refusal
  obligation, so R 0 and the sum 2; still Medium. The rule is applied the
  same way to TSK-1149 AC-4 (seven obligations become six, still R 1) and
  TSK-239 AC-7 (not counted by either grader). The kind is ambiguity, not a
  misreading by B.
- **S5 sentence:** replaced with B's wording, which clarifies "owns the
  changed behavior/proof" without requiring both. TSK-130 stays S5 0 under
  either reading, because AC-5 gives the live runs to TSK-116.
- **S4 sentence:** replaced with B's wording, so matching that normalizes is
  S4 and exact lookup is not.
- **TSK-019 anchor:** its driver line now names the C and X decision.
- **The rubric:** the first pass said the sentences were "now in
  `rubric.md`" when they existed only in this file. This commit puts them,
  the R line and the anchored examples in `rubric.md` in all five homes
  (`assets/base/`, `.claude/skills`, `.agents/skills` and both baselines),
  byte-identical, with the example forecast's rubric pin updated.
- **B's own counts:** B's grades were Easy 4, Medium 7, Hard 7, Very Hard 4,
  as its table shows; an earlier report of 3 Easy and 8 Medium was wrong.

Agreement is unchanged: 18 of 22 grades.
