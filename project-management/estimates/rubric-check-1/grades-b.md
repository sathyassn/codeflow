Grader B: Claude Opus 5.5, medium, a fresh subagent independent of grader A; reduced assurance, Codex unavailable

# Rubric reliability check 1: grader B's grades

- Standard: `.agents/skills/cf-estimate/references/rubric.md` at `47403d356`.
- Cases: `project-management/estimates/rubric-check-1/cases.md` at `5ad10cb0f`.
- Source per case: `project-management/tasks/<id>.md` at `47403d356`, sections Description, Outcome, Deliverables and Acceptance Criteria. Where a record has "Affected surfaces and interfaces" in place of Deliverables (TSK-105, TSK-119, TSK-189, TSK-130, TSK-015, TSK-019), I read it as the deliverables list. I did not read Closeout, acceptance blocks, the Execution contract, "Known limits from review" or "Review round" sections.
- Drivers not listed for a case scored 0. A zero is explained where it could be argued.
- H: human approval path, C, or a new authority, role or grant kind. B: a named start-gating decision.
- Grade cuts: 0-1 Easy, 2-4 Medium, 5-8 Hard, 9-23 Very Hard.

## Summary

| # | Task | Sum | Grade | H | B |
|---|---|---|---|---|---|
| 1 | TSK-1183 | 0 | Easy | no | no |
| 2 | TSK-1018 | 0 | Easy | no | no |
| 3 | TSK-219 | 0 | Easy | no | no |
| 4 | TSK-228 | 1 | Easy | no | no |
| 5 | TSK-212 | 4 | Medium | no | no |
| 6 | TSK-1301 | 3 | Medium | no | no |
| 7 | TSK-275 | 2 | Medium | no | no |
| 8 | TSK-1149 | 2 | Medium | no | no |
| 9 | TSK-293 | 2 | Medium | no | no |
| 10 | TSK-277 | 2 | Medium | no | no |
| 11 | TSK-1145 | 3 | Medium | no | no |
| 12 | TSK-225 | 10 | Very Hard | yes | no |
| 13 | TSK-239 | 5 | Hard | no | no |
| 14 | TSK-105 | 5 | Hard | yes | no |
| 15 | TSK-278 | 6 | Hard | yes | no |
| 16 | TSK-119 | 10 | Very Hard | no | no |
| 17 | TSK-189 | 5 | Hard | yes | no |
| 18 | TSK-322 | 7 | Hard | no | no |
| 19 | TSK-241 | 9 | Very Hard | yes | no |
| 20 | TSK-130 | 6 | Hard | no | no |
| 21 | TSK-015 | 6 | Hard | no | no |
| 22 | TSK-019 | 11 | Very Hard | yes | no |

No case was graded UNSIZED.

## How I read the X list

I read X's "named approval, revocation, disclosure or erasure mechanism" as a closed list. A refusal rule, a choice of policy authority, or a guard relaxation is not X; those count under R. X was given only where the package builds one of the four named kinds: a recorded reviewed approval (TSK-225 digest pin, TSK-105 review binding and waiver, TSK-241 operator confirmation, TSK-019 confirmed transfer) or redaction as erasure (TSK-278).

## Cases

### 1. TSK-1183: Bump pulldown-cmark to 0.13.6 (sum 0, Easy)

- S2 0: no operation or contract member is owned; only a dependency pin moves (Description, "changes nothing else").
- S6 0: it consumes the parser crate and conforms to nothing new.
- X 0: patching a dependency for an advisory is security applicability, not one of the named mechanisms.
- R 0. Obligations: (1) no other package version changes (AC-1); (2) the scanner reports no RUSTSEC-2026-0433 (AC-3). Two, at most 3.
- H no, B no.

### 2. TSK-1018: Worktree include tests resolve the fixture root on macOS (sum 0, Easy)

- All drivers 0. The change is a test fixture only (Deliverables), and the Non-goal says the guard is unchanged.
- S4 0: "tests alone ... do not create owned computation".
- R 0: no refusal obligations. AC-1 is a pass criterion.
- H no, B no.

### 3. TSK-219: say plainly that a standalone task never uses a plan branch (sum 0, Easy)

- S3 0: ordinary developer documentation and help text, which the S3 boundary excludes.
- S2 0: help wording changes and behaviour does not (Non-goal: "any change of behaviour").
- R 0: AC-1 checks wording and an unchanged journey; no refusal obligation.
- H no, B no.

### 4. TSK-228: codeflow ci warns when a PR body is too long (sum 1, Easy)

- S2 1: owns one new contract member of `codeflow ci`, the warning-level finding with its remedy row (AC-2, Deliverables). There is no mutation, no lifecycle event and only one operation.
- S4 0 (arguable): the word count follows stated rendering rules (AC-1), but counting text is closer to lookup than to the rubric's examples (geometry, graph, ranking, generated output). There is no golden or tolerance proof beyond unit tests ("tests alone").
- R 0. Obligations: (1) never blocks under the default policy or `block` (AC-3); (2) no finding when `off` or `allow` (AC-3); (3) no policy key added (AC-3). Exactly 1,000 words reporting nothing (AC-2) is a threshold boundary, not a safeguard. Three, at most 3.
- H no: the operator's approval on 2026-10-03 is provenance, not an approval path the package builds. B no.

### 5. TSK-212: local ci matches the hosted verdict (sum 4, Medium)

- S2 2: owns changed behaviour of at least three operations, `codeflow ci` (AC-1, AC-4), the pre-push hook judgement (AC-2) and `work start` (AC-3), giving 1 + 1.
- C 0 (arguable): the package changes which commit's policy judges a range, but it performs no custody or authority change on data or audience.
- X 0: binding the verdict to the base or destination policy is a policy-authority rule, not an approval, revocation, disclosure or erasure mechanism.
- S7 0: no replay, reconciliation or drift report is owned.
- R 2. Obligations:
  1. A branch that loosens its own policy is judged by the base's (AC-1).
  2. The only candidate authority is the destination default tip, never another branch (AC-2).
  3. An integration line's inherited commits are judged (AC-2).
  4. An unrelated history is never diffed against the tip (AC-2).
  5. A shallow, graft, replace-ref or unreadable history keeps the tip (AC-2).
  6. An unreadable or invalid default policy refuses the push with or without a range (AC-2).
  7. With no candidate authority the hook still checks at block level (AC-2).
  8. A failed default-tip fetch is tried once per push (AC-2).
  9. No printed step is one git-guard refuses (AC-3).
  10. A git base refusal is named instead of the fetch advice (AC-4).
  That is 8 or more.
- H no, B no.

### 6. TSK-1301: probe timeout and attempts from policy (sum 3, Medium)

- S2 2: three new policy keys as contract members, read by at least three operations: pre-push, `codeflow ci`, `task status`, the id sync and the guard refusal (AC-2, AC-6, AC-8).
- S1 0 (arguable): the keys are configuration in the policy file, not a stored entity, table or durable projection.
- X 0: the plain-fetch allowance (AC-8) is a refusal exception, not one of the four named mechanisms.
- R 1. Obligations:
  1. A destination silent past the wait still refuses with `ci.destination_unavailable` (AC-1).
  2. Out-of-bounds values refuse at load with no wrap or panic; zero, negative, fractional, overflow and non-number are one obligation (AC-3).
  3. Keys are read from the default branch, never the working copy or the pushed head (AC-7).
  4. With no default branch, shipped defaults apply, never `HEAD` (AC-7, `a_policy_with_no_default_branch_never_sets_the_waits`).
  5. The guard refuses after its 10 s wait, before the 20 s deadline (AC-8).
  6. Only a plain fetch gets the recovery allowance (AC-8).
  Six, in the 4 to 7 band.
- H no, B no.

### 7. TSK-275: codeflow ci gives one verdict for a range however it is spelled (sum 2, Medium)

- S2 1: owns changed output members of one operation, `codeflow ci`: the context source, the range rule line and the fallback warning (AC-3, AC-6, AC-7). There is no mutation and fewer than three operations.
- R 1. Obligations:
  1. The verdict is the same whether the base is a ref or a commit (AC-1); AC-5's "no reopen of main-reviewed tasks" is merged here.
  2. There is never a silent fallback to frozen mode: the command reaches the named-base verdict or refuses naming `--into` or `--branch` (AC-2).
  3. Before recommending a reopen, the command tells the caller to pass `--branch` (AC-3).
  4. A protected fallback with an unresolved declared target warns before the findings (AC-7).
  Four. AC-6 is positive behaviour.
- H no, B no.

### 8. TSK-1149: A task branch can push its own line sync before review (sum 2, Medium)

- S2 1: owns changed behaviour of one operation, the pre-push hand-off of `codeflow ci` (`receiving_merges`).
- X 0 (arguable): this relaxes when an existing review binding applies, using unforgeable facts. It does not implement an approval mechanism.
- R 1. Obligations:
  1. The pull request judgment still refuses without a review (AC-2).
  2. A merge bringing in anything the tip lacks keeps the refusal (AC-3).
  3. A branch carrying no task keeps the refusal (AC-3).
  4. A protected branch keeps the refusal (AC-3).
  5. A push of the line itself keeps the refusal (AC-3).
  AC-4 merges with items 1 and 5. Five, each independently fail-able.
- H no, B no.

### 9. TSK-293: codeflow integrate can land a reviewed head with a merge commit (sum 2, Medium)

- S2 2: a new mode of `integrate`, and it is a mutation that advances the line with a merge commit (AC-1), giving 1 + 1.
- C 0 (arguable): the merge does not push (AC-1), so there is no audience publication. Advancing a local line is not custody change.
- R 0. Obligations:
  1. The head is not rewritten (AC-1).
  2. A failed gate leaves the line unchanged (AC-2).
  3. A conflict leaves the line unchanged (AC-2).
  "It does not push" is a scope statement with no named evidence. AC-3 is a regression check. Three, at most 3.
- H no, B no.

### 10. TSK-277: A secret-scan exemption into the base can be classified (sum 2, Medium)

- S2 1: owns one new classification class of `codeflow ci` (AC-1).
- X 0 (arguable): the class skips one check. It builds no approval and leaves the existing review and approval requirements in place (AC-3).
- R 1. Obligations:
  1. Any other changed path denies the class; the per-neighbour fixtures are one obligation (AC-2).
  2. Only the stable-anchor check is skipped (AC-3).
  3. The secret scan and security review still run (AC-3).
  4. Existing review, approval and code-owner requirements still apply, and protection settings are not altered (AC-3).
  Four.
- H no: the approval path is consumed and unchanged. B no.

### 11. TSK-1145: Heavy test runs share the host by capacity (sum 3, Medium)

- S2 1: owns config contract members (`execution.host_jobs`, `host_exclusive`, removal of `heavy_wait_seconds`) and the admission behaviour of `codeflow test` and `codeflow reserve`. These are two operations, with no mutation or lifecycle event.
- S4 0 (arguable): the job budget is a division of host capacity clamped at one (AC-3). That is clamping, which the S4 boundary excludes.
- S1 0: slot lock files are runtime state, not a durable entity.
- S7 0: reading cores and memory is admission input, not environment evidence.
- R 2. Obligations:
  1. A host-exclusive target starts only when no heavy run holds the host (AC-4).
  2. No heavy run starts while an exclusive target runs (AC-4).
  3. A waiting exclusive target is bounded by the wait, then reported not run (AC-4).
  4. A forged hand-over value is refused (AC-5).
  5. A hand-over from another binary is refused (AC-5).
  6. A hand-over from a surviving target is refused (AC-5).
  7. An unreadable slot record refuses (AC-6).
  8. An unopenable lock folder refuses (AC-6).
  9. A planted link or swapped file refuses (AC-6).
  10. A free slot naming a live target group counts as busy (AC-6).
  That is 8 or more.
- H no, B no.

### 12. TSK-225: Managed CI setup step, pinned release digest, own-commit scan (sum 10, Very Hard)

- S1 3: a new persisted `[scaffold_sha256]` table in the committed project state file, read by the installers and written by `update --pin` (AC-3, AC-5).
- S2 2: owns several operations, including the CI jobs, `update --pin` (a mutation that writes state), `doctor --check ci-perimeter` and the `codeflow ci` notes (AC-5, AC-6, AC-14).
- S6 1: owns shipped templates for GitHub, GitLab, Bitbucket and generic CI, proven by tests that execute the shipped scripts (AC-1), plus a state-file form the shell installers must parse (AC-4, AC-5).
- S7 1: doctor detects drift between the checkout's pin table and the version or triple, with a remedy (AC-6).
- X 1: the pinned reviewed archive digest is an approval mechanism on the installer's own path: a stale version, missing triple or different digest fails closed (AC-3).
- C 0 (arguable): `update --pin` sets a trust root, but it is a local file write reviewed through a pull request, not a performed custody change or publication.
- R 2. Obligations, among many:
  1. Each pin mismatch kind fails closed and installs nothing (AC-3).
  2. A forged table form refuses (AC-4).
  3. Each `--pin` refusal: malformed, lowered, missing listing, archive mismatch, refused state (AC-5).
  4. The scan scope keeps a base finding from failing the pull request and catches added leaks (AC-7).
  5. Exemptions come only from the trusted commit (AC-7).
  6. Enforcement levels come from the trusted commit and fail closed (AC-12).
  7. A file path in scanner output cannot turn an advisory into a pass (AC-12).
  8. `exit 0` in the hook fails the run (AC-13).
  9. A lowered security key is named (AC-14).
  That is 8 or more.
- H yes: the operator applies the live workflow patches (AC-10, AC-14), a named human path. B no.

### 13. TSK-239: Estimates learn from outcomes derived from git (sum 5, Hard)

- S2 1: one new read-only operation, `estimate outcomes`, plus a status line member. There is no mutation (AC-5).
- S4 3: derives timings by walking git history (planned, started, blocked spans, completed, landed) and computes ratios, medians and ranges with a minimum. The proof is determinism over built fixtures with exact expected values (AC-1 to AC-3, AC-8).
- S1 0: the Non-goal says no new record key or acceptance-block key.
- S7 0 (arguable): reporting a missing estimate home is an existence check, not drift detection.
- R 1. Obligations:
  1. A missing adopted home exits 1 (AC-4).
  2. The command writes nothing to the tree or index (AC-5).
  3. It records nothing in the user registry (AC-5).
  4. Below the minimum, no verdict is printed (AC-3).
  5. A squash or fast-forward landing gives `started` as unknown with no number (AC-8).
  Five.
- H no, B no.

### 14. TSK-105: acceptance bound to the reviewed commit (sum 5, Hard)

- S2 2: owns behaviour of `codeflow ci` and `task status complete`, a mutation of task state, plus `git.work_records` levels (AC-1, AC-6).
- X 1: binds acceptance to the reviewed commit and admits a waiver only through a planning amendment commit. That is an approval mechanism on its own path (AC-1).
- S1 0 (arguable): the Description says "the parser and the transition rules are TSK-102's", so the block shape is consumed. AC-8's owner, window and follow-up fields are required by skill text and fixture, not a new stored entity.
- S5 0: the cf-reviewer behaviour is accepted by skill text and manifest checks, not by grounding, refusal or evaluation evidence (AC-5).
- R 2. Obligations:
  1. Every criterion appears once (AC-1).
  2. Mandatory criteria are verified or waived (AC-1).
  3. A waiver names an amendment commit for that record and criterion (AC-1).
  4. The reviewed commit is the head or an ancestor with only record changes after it (AC-1).
  5. A criteria change on a task branch blocks (AC-2).
  6. Adopter-facing ranges require a journey criterion (AC-3).
  7. A leaf serving the epic journey links evidence or states its narrower path (AC-3).
  8. A copied older block is refused (AC-5).
  9. A post-release criterion is never accepted as verified at build time (AC-8).
  The AC-4 output disclaimer is excluded as an evidence disclaimer. That is 8 or more.
- H yes: introduces the waiver as a grant kind bound to an amendment, and binds completion to a review. B no.

### 15. TSK-278: A closed record can be redacted without losing its results (sum 6, Hard)

- S1 3: a completed record gains a recorded digest of the replaced text and the review of the change. That is a changed persisted record shape (AC-1, Description).
- S2 1: owns the `codeflow ci` acceptance of the redaction route and the documented waiver field (AC-1, AC-4). `codeflow ci` judges the edit; the package owns no mutation operation.
- X 1: implements redaction, an erasure mechanism, on its own path.
- C 0 (arguable): the package sanctions a record-only edit that the user commits. Git history keeps the original, so the package performs no destructive erasure.
- R 1. Obligations:
  1. Refuse a changed result (AC-2).
  2. Refuse a changed reviewed commit (AC-2).
  3. Refuse changed review provenance (AC-2).
  4. Refuse a file outside the record (AC-2).
  Four, each independently fail-able with its own fixture. AC-1's "results unchanged" is the same obligation.
- H yes: the redaction requires a recorded review of the change (AC-1). B no: the provisional ADR choice is internal placement.

### 16. TSK-119: present answers stored against revisions (sum 10, Very Hard)

- S1 3: append-only answer records, receipts and amendments in the session store (Description, AC-2, AC-6).
- S2 2: a submit endpoint (mutation), `present update --expected-revision`, and the v2 `form` block contract (AC-1, AC-2, AC-8).
- S3 3: owns page form rendering, drafts, validation, stale confirmation and the submit states up to "stored" (Affected surfaces, AC-4, AC-7).
- S7 0 (arguable): crash atomicity and reopen (AC-5) is a storage safeguard, counted under R, not a restore feature.
- C 0: answers go to a private local store, with no publication or migration. AC-10 keeps v1 unchanged.
- R 2. Obligations:
  1. The server validates against the shown revision's form digest (AC-2).
  2. A same-id, same-payload retry appends nothing (AC-3).
  3. A same-id, different-payload retry is refused (AC-3).
  4. A new id is an amendment, never a retry (AC-3).
  5. A malformed submission is refused, leaving the store unchanged (AC-4).
  6. An oversized submission is refused (AC-4).
  7. A stale revision keeps the draft and asks to confirm (AC-4).
  8. A crash during append leaves the whole answer or none (AC-5).
  9. Nothing goes to browser storage (AC-7).
  10. Annotation never changes a value or submits (AC-9).
  11. A recommendation is never preselected (AC-1).
  12. A stale expected revision fails (AC-8).
  That is 8 or more.
- H no (arguable): the package stores decision answers, but what an answer authorizes belongs to TSK-120 and later work. B no.

### 17. TSK-189: guards judge by the landed policy and never go missing (sum 5, Hard)

- S2 2: owns behaviour of several operations: the guards, the hook shims with `--contract 3`, `doctor`, orient, `work claim` and `work start` (AC-1 to AC-4, AC-6).
- S7 1: doctor and orient report a local policy that differs from the landed one (AC-4), which is drift detection.
- S1 0: the Reason lines say "without a new state store" and "without a separate receipt store".
- X 0 (arguable): the landed-policy authority and the tracking-ref protections are refusal rules, not one of the four named mechanisms.
- C 0 (arguable): the package changes where the guards read authority from, but performs no custody or authority change on data.
- R 2. The refusal forms in AC-1 and AC-2 alone exceed 8:
  1. A weaker local policy changes nothing (AC-1).
  2. A dangling remote HEAD refuses (AC-1).
  3. Missing candidates refuse (AC-1).
  4. `update-ref` and `symbolic-ref` on `refs/remotes` refuse (AC-2).
  5. A foreign-destination fetch refuses (AC-2).
  6. A push to a local path refuses (AC-2).
  7. Remote add, set-url, set-head, rename and remove refuse (AC-2).
  8. Remote branch deletion refuses (AC-2).
  9. Config writes to the listed keys refuse (AC-2).
  10. An `insteadOf` URL mismatch refuses (AC-2).
  11. Shell writes to the integrity list refuse (AC-2).
  12. A missing tracking ref refuses (AC-2).
  13. A missing or older binary fails the shims and harness hooks (AC-3).
- H yes: authority to change enforcement is the operator's reviewed landing, and the operator's `set-head` recovery is named (Description, AC-1). B no.

### 18. TSK-322: Blocked names and protected prefixes refused at any depth (sum 7, Hard)

- S2 2: three policy keys, pre-commit, `ci`, doctor and a typed composition API (Description, Deliverables).
- S4 3 (arguable): owns a matcher with NFC normalization, an ASCII fold, two pattern kinds and anchored prefixes. It also owns a commit-graph walk using remerge diffs. Both are proven by a case table and fixtures (AC-1, AC-3). I read this as a graph and matching computation with golden-table proof, not a lookup of recorded state.
- S7 0: doctor naming sources, conflicts and waivers is a configuration report, not drift detection.
- X 0 (arguable): `allow` entries with reasons are policy exceptions, not an approval mechanism.
- R 2. Obligations, among many:
  1. Each refused component form refuses (AC-1).
  2. Symlinks and gitlinks are hits and are not followed (AC-2).
  3. Octopus merges refuse (AC-3).
  4. Unreadable merges refuse (AC-3).
  5. Add-then-delete refuses (AC-3).
  6. Each change kind refuses (AC-4).
  7. An unreadable landed policy refuses (AC-5).
  8. A branch-only exception discharges nothing (AC-5).
  9. Built-in exemptions hold in any spelling (AC-6).
  10. A branch removing a target entry changes nothing (AC-9).
  That is 8 or more.
- H no, B no: the TSK-400 routing is conditional scope with no gating decision.

### 19. TSK-241: Operator feedback has one tracker with CLI-issued ids (sum 9, Very Hard)

- S1 3: a new FB record kind and file, a new registry id kind (`ids/FB/NNN.toml`) and a `[feedback]` section in the state file (Description, AC-1, AC-2).
- S2 2: owns `feedback new`, `status` and `list`, three or more operations including mutations, plus status, recall and `validate` members.
- S7 1: the written index is reported stale when it differs from what the items render now (AC-6, Description), which is drift detection.
- X 1: declining needs `--confirmed-by operator`, and validation fails a declined item without it. That is a named approval on its own path (AC-3, AC-4).
- S3 0 (arguable): the lifecycle is driven through CLI verbs and record files, with no screen or panel. The human supplies words but does not operate a multi-state interface.
- R 2. Obligations:
  1. An unlisted topic is refused (AC-2).
  2. An illegal transition is refused and nothing is written (AC-3).
  3. to 10. Eight distinct `validate` failures (AC-4).
  11. Lower tiers refuse (AC-8).
  12. Other work records are still refused (AC-10).
  That is 8 or more.
- H yes: an operator confirmation path for declining. B no: the operator decisions are already made.

### 20. TSK-130: guidance retention evals across compaction (sum 6, Hard)

- S2 1: a new case kind in the pack schema contract (AC-1).
- S5 3 (arguable): owns the evaluation proof of model guidance retention: the probes, paired negatives, deterministic grading and the report bar (AC-2, AC-5). The live runs are TSK-116's. I counted S5 because the rubric counts a surface when the package owns "the changed behavior/proof", and this package owns the proof.
- S7 1 (arguable): environment evidence. The transcript check marks a trial `broken_fixture` when no automatic compaction happened, and the fixture recipe and run log prove the environment (AC-4, AC-5).
- T 0: the bar and the technique are fixed in the criteria. Live grading belongs to TSK-116, so nothing has to be established by experiment here.
- R 1. Obligations:
  1. No probe names the rule (AC-2).
  2. The paired negatives must not over-trigger (AC-2, AC-5).
  3. Compaction never happens in the operator's session (AC-3).
  4. A trial without compaction is marked `broken_fixture` (AC-5).
  5. Every hard failure is listed for human review (AC-5).
  Five.
- H no: human review of failures is not an approval path. B no.

### 21. TSK-015: check explicit forecast allocations safely (sum 6, Hard)

- S2 1: one read-only CLI operation, with no mutation (Description).
- S4 3: precedence, canonical boundary closure, cycles, calendars, shared resources and overflow, with golden JSON tests (AC 2 and 3).
- X 0 (arguable): "without leaking document content" is a non-disclosure safeguard, counted under R, not a disclosure mechanism.
- R 2. Obligations:
  1. Malformed input fails (AC 1).
  2. Oversized input fails (AC 1).
  3. Duplicate keys fail (AC 1).
  4. A stale or missing source fails (AC 1).
  5. Secret and path inputs fail (AC 1).
  6. Nothing is written (AC 1).
  7. No content leaks (AC 1).
  8. Cycles are handled (AC 2).
  9. Overflow is handled (AC 2).
  10. Equal endpoints are handled (AC 2).
  That is 8 or more.
- The record defers detail to SPC-007, which I did not read. Scope and core acceptance are present in the record, so it is graded, not UNSIZED.
- H no, B no.

### 22. TSK-019: replace portal baseline reconciliation with explicit ownership (sum 11, Very Hard)

- S1 3: state and identity schema with a fail-closed schema migration (Description, Affected surfaces).
- S2 2: portal CLI operations that write the portal and persist transfer, which are mutations (AC 3).
- S7 1: owns reconciliation, drift detection and legacy journal recovery (AC 1, 6).
- C 2: performs a stored-data migration (bounded authenticated baseline migration, AC 6) and a custody change (explicit whole-runtime transfer, AC 3).
- X 1: an explicit confirmed transfer is an approval mechanism, and authenticated removals are reported (AC 3, 4).
- R 2. Obligations:
  1. Drift stops before any write (AC 1).
  2. Collisions stop before any write (AC 1).
  3. Edited retirement stops before any write (AC 1).
  4. Project-owned missing files stay absent (AC 2).
  5. Configuration seeds only on fresh adoption (AC 2).
  6. Tampered blobs are untouched (AC 4).
  7. No managed pin or evidence check is weakened (AC 5).
  8. Adversarial paths, content and races preserve data (AC 6).
  9. Transaction fault injection preserves data (AC 6).
  10. Lockfiles are never line-merged (AC 7).
  That is 8 or more.
- H yes: C applies. B no.

## Ambiguities and how I resolved them

1. **Whether X's list is closed** (TSK-212, TSK-1149, TSK-277, TSK-189, TSK-322, TSK-1301). I read "a named approval, revocation, disclosure or erasure mechanism" as a closed list, so a refusal rule, a choice of policy authority or a guard relaxation is not X. A sentence that would decide it: "Policy-authority selection, refusal rules and exemption classes are R obligations, not X, unless they record an approval, revocation, disclosure or erasure."
2. **S4 for matching and counting rules.** TSK-228's word count got S4 0 and TSK-322's normalized matcher and remerge walk got S4 3. The deciding line was "lookup/ordering/clamping recorded state" versus a graph walk and normalization proven by a golden case table. If S4 were 0, TSK-322 would be 4 (Medium). If S4 were 3, TSK-228 would be 4 (Medium). Deciding sentence needed: whether parse-and-count or pattern-match rules with case tables count as owned computation.
3. **S5 when the package owns the evaluation but not the live run** (TSK-130). I gave 3 because the package owns the proof. With 0, the sum is 3 (Medium).
4. **S1 for config keys and record fields.** Policy keys got S1 0 (TSK-1301, TSK-322, TSK-1145). New tables or fields in committed state or record files that a component reads back got S1 3 (TSK-225, TSK-278, TSK-241). TSK-105 got 0 because the record names the parser as TSK-102's. Without S1, TSK-225 is 7 (Hard) and TSK-278 is 3 (Medium).
5. **C for redaction** (TSK-278). A sanctioned record edit with history retained was judged not to be destructive erasure. It took X 1, not C 2. With C, the sum is 8, still Hard.
6. **R for scope statements and thresholds.** "It does not push" (TSK-293) and the exactly-1,000-words boundary (TSK-228) were not counted, because neither is an independently evidenced safeguard. Counting "does not push" would make TSK-293's R 1 and its sum 3, still Medium.
7. **H for decision forms and review requirements.** TSK-119 stores decision answers but defines no authority, so H is no. TSK-278 and TSK-105 require a recorded review or amendment, so H is yes. The rubric does not say whether a recorded review or waiver counts as a "named human approval path".
8. **Records that defer to a spec** (TSK-015, TSK-019). They were graded from their terse criteria as instructed, without reading SPC-007 or SPC-008. Scope and acceptance were present, so neither was UNSIZED.
