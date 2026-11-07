# Rubric reliability check 1: cases and acceptance rule

This is the predeclared reliability check that TSK-1013 AC-15 asks for
(`rubric.md`, "Before trusting a changed rubric"). The cases and the
acceptance rule below were written and committed before any grade was
given. Two graders grade every case independently. Their initial grades,
the disagreements and the reconciled reasons are kept beside this file, and
the reconciled cases supply the anchored examples in `rubric.md`.

## What is graded

- **The rubric:** `.agents/skills/cf-estimate/references/rubric.md` as it
  stands at commit `47403d3561c91183c381701997a01a013734dce1`, the EPC-022
  line tip when TSK-1013 was claimed. Read it with
  `git show 47403d356:.agents/skills/cf-estimate/references/rubric.md`.
  Graders use that text, not any later edit made from this check.
- **Each case:** one landed CodeFlow task record, read at the same commit:
  `git show 47403d356:project-management/tasks/<id>.md`. Grade the task as
  it was planned: its Description, Outcome, Deliverables and Acceptance
  Criteria. Do not use the Closeout, the acceptance block, review history,
  diff size or elapsed time. The rubric grades scope and proof, not the
  effort the work took.
- **Per case, record:** each driver's points (S1 to S7, C, X, T, R) with an
  anchor (the record line or criterion that earns it) and a reason for each
  zero that could be argued; for R, the enumerated obligations; the sum; the
  grade; the H and B flags; and UNSIZED with the gap when the record cannot
  be graded.

## The cases

The set covers every driver and the cuts between 1 and 2, 4 and 5, and 8
and 9 points. It includes fixes, tests, docs, guards and security work,
which the rubric audit found are often under-read. The expected region is
the chooser's guess, written only to show coverage. It is not a grade, and
the graders ignore it.

| # | Task | Title | Why it is in the set | Expected region |
|---|---|---|---|---|
| 1 | TSK-1183 | Bump pulldown-cmark to 0.13.6 for RUSTSEC-2026-0433 | dependency change, security advisory | Easy, low |
| 2 | TSK-1018 | Worktree include tests resolve the fixture root on macOS | tests alone | Easy, low |
| 3 | TSK-219 | say plainly that a standalone task never uses a plan branch | ordinary docs | Easy, low |
| 4 | TSK-228 | codeflow ci warns when a PR body is too long | one report field, warnings | 1 to 2 cut |
| 5 | TSK-212 | local ci matches the hosted verdict | a fix that changes an owned operation | 1 to 2 cut |
| 6 | TSK-1301 | The pre-push destination probe's timeout and attempt count come from policy | policy keys as contract members | 1 to 2 cut |
| 7 | TSK-275 | codeflow ci gives one verdict for a range however it is spelled | a fix with several refusal obligations | Medium |
| 8 | TSK-1149 | A task branch can push its own line sync before review | a guard change, R or X | 4 to 5 cut |
| 9 | TSK-293 | codeflow integrate can land a reviewed head with a merge commit | a mutation, possible C | 4 to 5 cut |
| 10 | TSK-277 | A secret-scan exemption into the base can be classified | security, X or R | 4 to 5 cut |
| 11 | TSK-1145 | Heavy test runs share the host by capacity instead of one lock | possible T, computation | 4 to 5 cut |
| 12 | TSK-225 | Managed CI runs a project setup step, checks a pinned release digest and scans only a change's own commits | external format (S6), CI | Medium |
| 13 | TSK-239 | Estimates learn from outcomes derived from git | computation (S4), report contract | Hard |
| 14 | TSK-105 | acceptance bound to the reviewed commit | persisted shape (S1), mutation | Hard |
| 15 | TSK-278 | A closed record can be redacted without losing its results | erasure (C), disclosure (X) | 8 to 9 cut |
| 16 | TSK-119 | present answers stored against revisions | persisted shape and interface (S1, S3) | 8 to 9 cut |
| 17 | TSK-189 | guards judge by the landed policy and never go missing | guard, many refusal obligations | Hard |
| 18 | TSK-322 | Blocked names and protected prefixes are refused at any depth | guard, R count | Hard |
| 19 | TSK-241 | Operator feedback has one tracker with ids the CLI issues | S1, S2 and records | 8 to 9 cut |
| 20 | TSK-130 | guidance retention evals across compaction | AI behavior (S5), experiment (T) | Hard |
| 21 | TSK-015 | check explicit forecast allocations safely | computation, many obligations | Very Hard |
| 22 | TSK-019 | replace portal baseline reconciliation with explicit ownership | reconciliation (S7), migration (C) | 8 to 9 cut |

## The acceptance rule

Written before grading, and not changed after it.

1. **Agreement** for a case means both graders give the same grade: Easy,
   Medium, Hard, Very Hard or UNSIZED. Points may differ inside one grade.
2. **Every difference gets a reconciled reason:** each case where the grades
   differ, and each driver where the points differ even inside one grade.
   It is settled against the rubric text and the record's anchors, never by
   averaging or splitting the difference.
3. **A reconciled reason is one of three kinds:** a grader misread the
   record (name the line); a grader misread the rubric (name the rubric
   line); or the rubric text does not decide the case. The third is recorded
   as an ambiguity, with the rubric sentence that would decide it. Such a
   sentence enters the rubric only as an anchor or a clarification, never by
   changing a point value or a cut.
4. **The result is reported, not gated.** The agreement rate (cases agreeing
   out of 22) and the driver-level disagreements are stated as found. No
   rate passes or fails the rubric. A low rate is evidence for more anchors
   or a simpler grouping (`rubric.md`, "Consequence and use").
5. **Anchors.** After reconciliation, up to two cases per grade whose
   reconciled grade both graders accept become the rubric's anchored
   examples, each pinned as `<task id>@47403d356` with its driver line.
6. **Assurance.** The second grade comes from a fresh Claude Opus 5.5 grader
   at medium effort that sees only this file. No Codex seat was available
   for this check, so it is recorded as reduced assurance (one model family
   graded twice) under the operator's standing rule for an unavailable seat.
