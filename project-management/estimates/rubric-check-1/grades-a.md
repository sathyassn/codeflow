# Rubric reliability check 1: grader A

Grader A: Claude Opus 5.5, the TSK-1013 builder session, graded before
reading any other grade. Inputs: `cases.md` at `5ad10cb0f`, and the rubric
and the 22 records at `47403d356`, read from git objects. Records were read
up to their Closeout; the Closeout, acceptance blocks and review history
were not used. Only nonzero drivers are listed per case; every other driver
is 0, with the argued zeros given under "Zeros".

Anchors name the record section or criterion. R lists the obligations
counted.

| # | Task | Drivers | Sum | Grade | H, B |
|---|---|---|---|---|---|
| 1 | TSK-1183 | none | 0 | Easy | none |
| 2 | TSK-1018 | none | 0 | Easy | none |
| 3 | TSK-219 | none | 0 | Easy | none |
| 4 | TSK-228 | S2 1 | 1 | Easy | none |
| 5 | TSK-212 | S2 2, R 2 | 4 | Medium | none |
| 6 | TSK-1301 | S2 2, R 1 | 3 | Medium | none |
| 7 | TSK-275 | S2 1, R 1 | 2 | Medium | none |
| 8 | TSK-1149 | S2 1, R 1 | 2 | Medium | none |
| 9 | TSK-293 | S2 2, R 1 | 3 | Medium | none |
| 10 | TSK-277 | S2 1, R 1 | 2 | Medium | none |
| 11 | TSK-1145 | S2 2, R 2 | 4 | Medium | none |
| 12 | TSK-225 | S1 3, S2 2, S6 1, S7 1, R 2 | 9 | Very Hard | none |
| 13 | TSK-239 | S2 1, S4 3, R 1 | 5 | Hard | none |
| 14 | TSK-105 | S1 3, S2 2, X 1, R 2 | 8 | Hard | H |
| 15 | TSK-278 | S1 3, S2 2, X 1, R 1 | 7 | Hard | none |
| 16 | TSK-119 | S1 3, S2 2, S3 3, R 2 | 10 | Very Hard | none |
| 17 | TSK-189 | S2 2, R 2 | 4 | Medium | none |
| 18 | TSK-322 | S2 2, S4 3, R 2 | 7 | Hard | none |
| 19 | TSK-241 | S1 3, S2 2, R 2 | 7 | Hard | none |
| 20 | TSK-130 | S2 1, R 1 | 2 | Medium | none |
| 21 | TSK-015 | S1 3, S2 1, S4 3, R 2 | 9 | Very Hard | none |
| 22 | TSK-019 | S1 3, S2 2, S7 1, C 2, R 2 | 10 | Very Hard | H |

## Per case

1. **TSK-1183.** A consumed dependency's pin moves; no owned surface. R:
   no other package changes; markdown tests pass; the advisory is gone.
   Three, so 0. Zeros: X 0, the advisory fix is not an approval,
   revocation, disclosure or erasure mechanism.
2. **TSK-1018.** Tests alone (Description: the fixture root resolves).
   Zeros: S2 0, no product behaviour changes ("Non-goal: any change to the
   guard").
3. **TSK-219.** Ordinary documentation and help wording; the behaviour is
   unchanged (AC-1, `light_paths_journey` passes). Zeros: S2 0, help text
   that changes no contract is documentation.
4. **TSK-228.** S2 1: one new `codeflow ci` finding with a remedy (AC-2), a
   contract member a reader depends on; no mutation, one operation. R:
   never blocks at default or `block` (AC-3); silent at `off` and `allow`
   (AC-3); no policy key added (AC-3). Three, so 0. Zeros: S4 0, word
   counting is parsing and counting recorded text, not owned computation
   with golden or tolerance proof.
5. **TSK-212.** A fix that changes owned operations. S2 2: the policy
   authority of `codeflow ci`, the pre-push judgment and `work start`'s
   note (AC-1 to AC-3), at least three operations. R 2: a branch that
   loosens its own policy is judged by the base (AC-1); the only candidate
   authority is the default branch tip (AC-2); commit checks cover all the
   head adds (AC-2); an unrelated history is not diffed (AC-2); shallow or
   overlaid history keeps the tip (AC-2); an unreadable or invalid policy
   refuses (AC-2); no authority still checks at block (AC-2); no step the
   guard refuses is printed (AC-3); git's refusal is named (AC-4). Nine.
6. **TSK-1301.** S2 2: three policy keys (Description, AC-2), contract
   members, read by several operations (pre-push, `ci`, `task status`, id
   sync, AC-6). R 1: timeout still refuses (AC-1); out-of-bound values
   refuse naming the key (AC-3); nothing wraps or panics (AC-3); keys read
   only from the default branch (AC-7); the guard refuses after its wait
   naming the fetch (AC-8); only a plain fetch gets the allowance (AC-8);
   no configured wait reaches the guard (Description). Seven. Zeros: S1 0,
   policy keys are contract members, not a persisted entity.
7. **TSK-275.** A fix. S2 1: `codeflow ci` range and context resolution
   and the help semantics (AC-1 to AC-4, AC-6); one operation, no mutation.
   R 1: same verdict for both spellings (AC-1); never a silent frozen
   fallback (AC-2); context and source named (AC-3); `--branch` advice
   before a reopen (AC-3); no reopen of tasks reviewed on main (AC-5);
   warning when the declared target does not resolve (AC-7). Six.
8. **TSK-1149.** S2 1: the pre-push acceptance judgment for one branch
   kind (Description). R 1: ready still refuses (AC-2); a merge bringing
   anything beyond the tip refuses (AC-3); a non-task branch refuses (AC-3);
   a protected branch refuses (AC-3); the line itself refuses (AC-3); the
   TSK-1066 judgments are unchanged (AC-4); facts never a pusher-chosen
   name (Description). Seven. Zeros: X 0, a guard relaxation is not an
   approval or revocation mechanism; its refusal obligations are R.
9. **TSK-293.** S2 2: a new `--merge` flag on an operation that mutates
   the line (AC-1). R 1: a red gate leaves the line unchanged (AC-2); a
   conflict leaves it unchanged (AC-2); no push (AC-1); default rebase
   unchanged (AC-3). Four. Zeros: C 0, landing on an integration line is
   not publication, erasure, custody change or data migration.
10. **TSK-277.** S2 1: one new `codeflow ci` class (AC-1). R 1: any other
    path loses the class (AC-2); only the stable-anchor check is skipped
    (AC-3); the secret scan and security review still run (AC-3); existing
    review and approval requirements apply (AC-3); no new code-owner
    enforcement (AC-3). Five. Zeros: X 0, the class narrows a check; it is
    not an approval mechanism.
11. **TSK-1145.** S2 2: the config keys `execution.host_jobs` and
    `host_exclusive` and the admission behaviour of `test` and `reserve`
    (Deliverables, AC-3 to AC-5). R 2: never queues (AC-1, AC-2); a
    host-exclusive target runs alone in both directions (AC-4); its wait is
    bounded (AC-4); the TSK-1128 hand-over checks hold (AC-5); an
    unreadable slot record refuses (AC-6); an unopenable folder refuses
    (AC-6); a planted link or swapped file refuses (AC-6); a live target
    group counts as busy (AC-6); the old setting and tickets are gone
    (AC-8). Nine. Zeros: S4 0, the budget is a division clamped at one,
    which the rubric excludes ("lookup/ordering/clamping recorded state");
    S1 0, slot records are runtime lock state, not a durable entity.
12. **TSK-225.** S1 3: the `[scaffold_sha256]` table in the project state
    file (AC-3, AC-5). S2 2: `codeflow update --pin` writes state (AC-5),
    a mutation; doctor's report members (AC-6). S6 1: the GitHub, GitLab,
    Bitbucket and generic CI formats, proved by tests that execute the
    shipped scripts (AC-1). S7 1: the doctor perimeter check detects drift
    of the pin and hook (AC-6). R 2: well over eight (AC-3, AC-4, AC-5,
    AC-7, AC-12 to AC-14). Zeros: X 0, digest verification is integrity
    checking, not one of the named mechanisms.
13. **TSK-239.** S2 1: a new read-only verb and the status line (AC-1,
    AC-6); no mutation. S4 3: derived timings, ratios, medians and the
    contradiction verdict with fixture proof (AC-1 to AC-3). R 1: writes
    nothing (AC-5); unknown start never filled (AC-8); below minimum gives
    no verdict (AC-3); a missing home exits 1 (AC-4); no adoption still
    derives and exits 0 (AC-4). Five.
14. **TSK-105.** S1 3: the acceptance block gains the reviewed binding, the
    waiver form and the post-release criterion fields (AC-1, AC-8). S2 2:
    `task status complete` (a mutation) and `ci` (AC-1). X 1: a waiver is
    accepted only through an amendment commit on the target, a named
    approval mechanism on the task's own path (Description, AC-1). R 2:
    every criterion once; mandatory verified or waived; a waiver names its
    amendment; the reviewed commit is the head or an allowed ancestor
    (AC-1); a criteria change blocks (AC-2); a journey criterion is
    required (AC-3); a copied older block is refused (AC-5); a post-release
    criterion is never verified at build time (AC-8). Eight. H: the waiver
    is a named human approval path.
15. **TSK-278.** S1 3: the record gains the digest of replaced text
    (Description, AC-1). S2 2: the `ci` route and the `task status` write
    (Deliverables). X 1: a redaction route is a disclosure and erasure
    mechanism on its own path. R 1: a changed result refuses; a changed
    reviewed commit refuses; changed provenance refuses; a file outside the
    record refuses (AC-2); the waiver is documented and named in the
    message (AC-4). Five. Zeros: C 0, git history keeps the original text,
    so the route is not destructive erasure.
16. **TSK-119.** S1 3: the `form` block schema and append-only answer
    records (AC-1, AC-6). S2 2: the submit endpoint, receipts and
    `present update --expected-revision` (AC-2, AC-8), with mutation. S3 3:
    the form's multi-state workflow in the page (AC-4, AC-7). R 2:
    idempotent retry; a different payload refused; a correction is an
    amendment (AC-3); malformed refused; oversized refused; a stale
    revision keeps the draft (AC-4); a crash leaves all or nothing (AC-5);
    nothing in browser storage (AC-7); annotation never changes values
    (AC-9); v1 unchanged (AC-10); a recommended option never preselected
    (AC-1). Eleven. Zeros: S7 0, crash atomicity is a store property, not a
    restore or replay path.
17. **TSK-189.** S2 2: the guards' policy source, the hook shims' exit
    contract and doctor's report (AC-1 to AC-3), several operations. R 2:
    dozens of refusal obligations (AC-1, AC-2: each faked-ref route, the
    missing tracking ref, the missing binary). Zeros: X 0, binding policy
    authority is integrity, not approval, revocation, disclosure or
    erasure; S1 0, no new persisted shape.
18. **TSK-322.** S2 2: three policy keys read by pre-commit, `ci` and
    doctor (Description "Keys", "Where"). S4 3: a path matcher with NFC
    normalization, a fold and remerge-diff walking, proved by a case table
    (AC-1). R 2: refused component forms, link and gitlink rules, octopus
    and unreadable merges, add-then-delete, unreadable policy, exceptions
    only from the target (Description). More than eight. Zeros: X 0, a path
    refusal is a safeguard counted in R.
19. **TSK-241.** S1 3: the FB record kind and the `[feedback]` section
    (Description, Deliverables). S2 2: `feedback new`, `status` and `list`
    with mutation (AC-1 to AC-5). R 2: unlisted topic refused (AC-2); each
    illegal transition refused (AC-3); the validation failures (AC-4);
    stale index only a warning (AC-6); refusal at other tiers (AC-8); an
    older binary warns (Description); renumbering keeps Verbatim and avoids
    links (Deliverables); other added records still refused (AC-10). Eight
    or more.
20. **TSK-130.** S2 1: a declared case kind in the pack schema (AC-1). R 1:
    no probe names the rule (AC-2); compaction never in the operator's
    session (AC-3); a trial without compaction is `broken_fixture` (AC-5);
    no over-triggering on negatives (AC-5). Four. Zeros: S5 0, the task
    builds the measurement; the live runs and the behaviour they grade are
    TSK-116's (AC-5); T 0, the compaction technique is named in the record
    (the fixture setting), not established by experiment.
21. **TSK-015.** S1 3: the strict typed forecast schema (SPC-007). S2 1:
    one new verb with a JSON report. S4 3: precedence, calendars, shared
    resources and overflow, with golden tests (AC-2, AC-3). R 2: malformed,
    oversized, duplicate key, stale source, secret path, no writes, no
    leaked content, cycles, overflow (AC-1, AC-2). Nine or more.
22. **TSK-019.** S1 3: the state and identity schema (Affected surfaces).
    S2 2: the portal CLI transfer, a mutation (AC-3). S7 1: drift and
    collision detection and repair of managed files (AC-1, AC-2). C 2: a
    fail-closed stored-data schema migration and a whole-runtime custody
    transfer (Description, AC-3, AC-6). R 2: more than eight (AC-1 to
    AC-7). H: C applies.

## Ambiguities met while grading

- Policy and config keys: I scored them as S2 contract members, never S1.
  The rubric names neither.
- A fix that changes an owned operation's behaviour: I scored S2, as the
  rubric's "owns the changed behavior" reads.
- Guards and refusal paths: I scored their obligations under R and gave X
  0 where no approval, revocation, disclosure or erasure mechanism exists.
- Integrity checks (digests, policy authority): X 0; the rubric's list does
  not name integrity.
