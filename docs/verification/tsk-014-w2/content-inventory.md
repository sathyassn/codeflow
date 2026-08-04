# Real-content inventory

Every fact used by a baseline or candidate, with the repository path it comes
from. Nothing here is invented, rounded, or illustrative. If a fact is not in
this table, it must not appear in a candidate.

## P1 — EPC-005 plan graph

Source: frontmatter of `project-management/tasks/TSK-005.md`, `TSK-006.md`,
`TSK-007.md`, `TSK-008.md`, `TSK-009.md`, `TSK-010.md`, `TSK-011.md`,
`TSK-012.md`, `TSK-013.md`, `TSK-014.md` (fields `title`, `status`,
`depends_on`, `specs`, `integration_target`).

| Task | Title | Status | depends_on | specs | integration_target |
|---|---|---|---|---|---|
| TSK-005 | strengthen editorial and product design contracts | complete | — | SPC-003 | main |
| TSK-006 | research and prototype the presentation experience | complete | TSK-005 | SPC-004 | integration/EPC-005-presentation-system |
| TSK-007 | verify and qualify the presentation utility | blocked | TSK-011 | SPC-004 | integration/EPC-005-presentation-system |
| TSK-008 | research and prototype documentation portals | complete | TSK-005 | SPC-005 | integration/EPC-005-presentation-system |
| TSK-009 | build and qualify the documentation portal utility | blocked | TSK-008, TSK-014 | SPC-005 | integration/EPC-005-presentation-system |
| TSK-010 | integrate and release the CodeFlow system | todo | TSK-007, TSK-009, TSK-013, TSK-014 | — | integration/EPC-005-presentation-system |
| TSK-011 | build the interactive presentation utility | blocked | TSK-006, TSK-014 | SPC-004 | integration/EPC-005-presentation-system |
| TSK-012 | refine design exploration, sourcing, and revision contracts | cancelled | TSK-005 | SPC-006 | integration/EPC-005-presentation-system |
| TSK-013 | prepare the v3 release candidate | in_progress | — | — | integration/EPC-005-presentation-system |
| TSK-014 | recover the design method and utility directions | in_progress | TSK-005, TSK-006, TSK-008 | SPC-004, SPC-005, SPC-006 | integration/EPC-005-presentation-system |

**Derived in-page, never hard-coded.** Each P1 candidate computes the following
from the table above at load time, so no candidate can assert a number the data
does not support:

- *ready* — every dependency is `complete` (a `cancelled` dependency does not
  block; there are none in this set).
- *wave* — `1 + max(wave of incomplete dependencies)`, with complete and
  cancelled tasks placed outside the waves as landed history.
- *slack* — `latest wave − earliest wave`, where latest wave is derived from the
  longest remaining path to the sink task TSK-010.
- *stream* — from `specs`: SPC-004 → presentation, SPC-005 → portal, SPC-003 or
  SPC-006 → method, both SPC-004 and SPC-005 → spans both utilities, none →
  release.

## P2 — TSK-007 qualification evidence

Source: `docs/verification/tsk-007-presentation/README.md` (lines 1–100) and the
TSK-006 closeout in `project-management/tasks/TSK-006.md` (lines 101–114).

| Fact | Value | Source |
|---|---|---|
| Exact implementation candidate | `826715510440df53fc999e68d77429d7b49a6d55` | tsk-007 README:3 |
| macOS arm64 result | `codeflow test --mode full --strict` passed all nine targets | README:11–25 |
| Presentation target duration | 158.336 s | README:22 |
| Renderer digest | `d9d37a03b53f0006b79fbd77fcf53b9fec1fc0df5a3aef17e5a4d86032e0a900` | README:33 |
| Release binary | 16,518,448 bytes, SHA-256 `6cc9d167…ea795e` | README:35–37 |
| Linux evidence revision | `74feaf04ca0eea0062a4554200e7a7e774fc5b6f` — **older than the exact candidate** | README:54–55 |
| Linux source archive digest | `33fcfd57a55a709deddddbcf66ae33bb129084932dd1d4501d8313e3745665b4` | README:56–57 |
| Linux desktop Chromium | exited `SIGTRAP` under the VZ guest; **not claimed as passed** | README:62–63 |
| Linux promotion state | retained as bounded diagnostic; **not promoted to exact-candidate approval** | README:68–70 |
| Windows guard | proves start-time confinement only, not account/profile/VM teardown | README:81 |
| Windows/WSL2 run | none available — installed Parallels licence had expired | README:83–84 |
| Independent review | two GPT-5.6 Sol/high reviews approved candidate `82671551`, no material findings | README:89–90 |
| Claude verdict | blocked — accepted turn ended with `oauth_org_not_allowed` | README:96–100 |
| Native launcher | `--no-launch` with a directly owned Playwright browser; **not** native product-launcher evidence | README:48–50 |

## P3 — review convergence

### Correction to the inventory framing recorded in `NEXT.md`

`NEXT.md` asked for "the six review findings and their resolution across rounds"
plus "the exact iteration in which each finding was raised, fixed, and
re-verified". Those two requirements cannot both be met from the same source.

The "six" are the W1 **design-method** review findings, recorded in
`project-management/tasks/TSK-014.md:119–121` as one batch: *"Claude's exact
compacted-diff review requested six material corrections. After the
localization/mode and visual/verbal-coherence regressions, reference routing,
gate/lens boundary, quality-contract binding, and managed wiring were corrected,
its final W1 verdict was `APPROVED`."* That record carries **no per-iteration
timing** — the six were raised in one review and corrected before one verdict.
Building a findings × iterations ledger over them would require inventing the
iteration each was raised and re-verified in.

The lineage that *does* carry real per-round evidence is the `cf-present`
browser-qualification harness: eight commits on this branch, each a round, with
the raise/fix/re-verify story recorded in the TSK-014 closeout paragraphs and in
the diffs themselves. **That is the P3 subject.** Seven findings, not six. The
registered question was corrected to match — see `answer-key.md`.

### Rounds

Source: `git log` on `task/TSK-014-design-recovery`, restricted to commits
touching `crates/codeflow-present/web/scripts/real-browser-check.mjs` or
`real-browser-cleanup-check.mjs`.

| Round | Commit | Subject | Date |
|---|---|---|---|
| R1 | `86582dc0` | test(present): qualify owned runtime boundaries | 2026-08-02 |
| R2 | `74feaf04` | test(present): harden qualification cleanup | 2026-08-02 |
| R3 | `82671551` | test(present): close qualification review gaps | 2026-08-02 |
| R4 | `14107e48` | test(present): tolerate cold browser startup | 2026-08-03 |
| R5 | `c5ede869` | test(present): follow bootstrap readiness semantics | 2026-08-03 |
| R6 | `30133fd7` | test(present): bound cold renderer startup | 2026-08-03 |
| R7 | `e194b1fd` | fix(present): harden browser close recovery | 2026-08-03 |
| R8 | `a47b62c3` | fix(present): align browser qualification driver | 2026-08-03 |

`R3` is the exact implementation candidate the P2 case is about.

### Findings and their per-round state

| Finding | Raised | Addressed | Re-opened | Closed | Cause of re-opening |
|---|---|---|---|---|---|
| F1 child environment inherited unrelated host values | R2 | R2 | R3 | R3 | the previous fix — the allow-list omitted Windows' real `SystemRoot` casing |
| F2 cleanup swallowed the primary error and its own failures | R2 | R2 | — | R2 | — |
| F3 no deterministic proof that an injected failure still cleans up | R2 | R2 | — | R2 (extended R7, R8) | — |
| F4 Windows profile confinement unscoped | R3 | R3 | — | R3, with an open residual | — |
| F5 cold-runner startup exceeded the qualification's bounded waits | R4 | R4 | R5, again R6 | R6 | the previous fix — twice |
| F6 browser-context close timed out with an earlier task-owned Chrome present | R7 | R7 | R8 | R8, with an open residual | outside the harness — Chrome 150 driven by Playwright 1.55 |
| F7 POSIX process inventory failed closed on a transient `ps` overrun | R8 | R8 | — | R8 | — |

An **extension** (F3 at R7 and R8) is deliberately not a re-opening: the later
round widened what the check proves without the earlier fix having failed. The
ledger candidate encodes that distinction; the answer depends on it.

### Verbatim sources for each finding

Every finding's claim is quoted from `project-management/tasks/TSK-014.md` and/or
carries the exact diff lines from its round. `tools/verify.mjs` checks each
quotation against the task record and each diff line against `git show <sha>`.

| Fact | Source |
|---|---|
| the gate refused the first merge | TSK-014.md:132 — "The first task-to-integration gate then refused the merge." |
| close timeout with an earlier task-owned Chrome present | TSK-014.md:135–137 |
| the zero-fallback gate and its injected counterpart | TSK-014.md:141–142 |
| the retry rejected an intermittent normal close fallback | TSK-014.md:145–146 |
| Chrome 150 driven by Playwright 1.55, outside its tested window | TSK-014.md:146–148 |
| `playwright-core` aligned to 1.62.1 | TSK-014.md:149–150 |
| intermittent, reduced not eliminated | TSK-014.md:150–153 |
| the POSIX inventory timeout and the five-second `ps` overrun | TSK-014.md:155–158 |
| `NAVIGATION_TIMEOUT_MS`, `openAuthenticatedPresentation`, `timeoutWithinQualification`, `browserCloseFallbacks`, `PROCESS_INVENTORY_TIMEOUT_MS`, the allow-list casing, the `playwright-core` bump | the diffs of R2–R8, quoted line for line |

**Derived in-page, never hard-coded.** Both P3 candidates compute from the table
above: which findings needed more than one round (any finding with a `reopened`
state), and for each, whether the cause was the previous fix or something outside
the harness.

## D1 — portal Orient

Source: `AGENTS.md` (six-layer table, "Planning and tracking", "Git rules",
"Worktree doctrine"), `docs/product.md`, `docs/capabilities.md`, and the three
enforcement planes in code:

- `crates/codeflow-core/src/workgraph/work_start.rs` — the `WorkStartError`
  variants *are* the precondition list; each variant's `#[error(...)]` string is
  quoted verbatim.
- `crates/codeflow-cli/src/cmd/git_hook.rs:129–171` — `durable_work_preflight`,
  the `pre-commit` plane.
- `crates/codeflow-cli/src/cmd/ci.rs:177–182, 252–297` — `evaluate_work_start`,
  the authoritative plane, emitting `work.valid_graph`,
  `work.stable_planning_anchor`, `work.task_record`.
- `crates/codeflow-core/src/hooks/git_hook.rs` — the git rule ids by plane:
  `git.commit_to_protected` and `git.secret_scan` (pre-commit, lines 58, 100–146),
  `git.commit_format`/`git.commit_body`/`git.ai_attribution`/`git.commit_emoji`
  (commit-msg, lines 174–307), `git.merge_to_protected` (pre-merge-commit, 411),
  `git.local_ref_protection`/`git.delete_protected` (reference-transaction, 491,
  515), `git.push_to_protected`/`git.force_push_protected`/`git.branch_naming`/
  `git.test_gate_on_push` (pre-push, 646–762).

| Fact | Value | Source |
|---|---|---|
| shipped / building capabilities | 13 shipped, 3 building (CAP-014, CAP-015, CAP-016) | `docs/capabilities.md` frontmatter blocks |
| six layers | product · this file + skills · capabilities · architecture + ADRs · project-management · ledger | `AGENTS.md`, "Project organization" |
| target must resolve | "The target must resolve to a real local or remote-tracking branch, never `HEAD`, a tag, an object ID, or another revision expression." | `AGENTS.md`, "Planning and tracking" |
| the anchor is the merge-base | "Git history is the planning seal." | `work_start.rs:3` |
| startable statuses | `todo` or `in_progress` | `work_start.rs:56–57` |
| spec statuses accepted | `approved` or `implemented` | `work_start.rs:50–55` |
| this repo's real perimeter | remote branch protection is unavailable and not pursued; CI + local hooks + human-merged PRs | `AGENTS.md`, "Project-specific instructions" |

**Derived in-page:** which single rule covers the most preconditions (the answer's
gate), and the plane set that reports it.

## D2 — portal Record

Source: the frontmatter and body of `project-management/specs/SPC-004.md`,
`docs/decisions/ADR-0049…ADR-0052`, `project-management/tasks/TSK-011.md`,
`TSK-014.md`, and `project-management/epics/EPC-005.md`.

| Record | Real frontmatter | Source |
|---|---|---|
| SPC-004 | `status: approved`, `created: 2026-07-31` | SPC-004.md:1–6 |
| ADR-0049 | bounded cf-present runtime and renderer boundary, `2026-08-01`, `accepted`, `superseded_by: null` | ADR-0049:1–8 |
| ADR-0050 | versioned cf-present document and session contract, `2026-08-01`, `accepted`, `superseded_by: null` | ADR-0050:1–8 |
| ADR-0052 | separate cf-present ephemeral runtime from durable state, `2026-08-01`, `accepted`, `superseded_by: null`, carries `architecture_impact` | ADR-0052:1–8 |
| TSK-011 | `status: blocked`, `specs: [SPC-004]`, `depends_on: [TSK-006, TSK-014]` | TSK-011.md frontmatter |
| TSK-014 | `status: in_progress`, `specs: [SPC-004, SPC-005, SPC-006]` | TSK-014.md frontmatter |
| EPC-005 | `adrs: [ADR-0047 … ADR-0052]` | EPC-005.md frontmatter |

The load-bearing relationship, quoted verbatim from ADR-0052:

> ADR-0049 and ADR-0050 remain historical decisions; this ADR supersedes only
> their same-tree runtime assumption and post-create Windows DACL mechanism where
> those details conflict.

That is a **partial** supersession, and `superseded_by` on ADR-0049 and ADR-0050
is still `null` — the frontmatter alone cannot express it.

SPC-004's central design claim, quoted verbatim (§ Behavior 3):

> The selected default must be modern, elegant, aesthetically coherent, pleasing,
> functional, responsive, and suited to focused explanation/review rather than
> resembling a generic model artifact or dashboard template.

The evidence state behind that claim, from TSK-014.md:99–104 and
`docs/verification/tsk-014-w2/observer-state.md`: the operator's real trial
invalidated the prior design acceptance, and the replacement rendered board's
blind gates are **not run**. The evidence that does exist for the surrounding
runtime claims is the TSK-007 qualification record — macOS arm64 only at the
exact candidate, with the Claude judgment verdict blocked on
`oauth_org_not_allowed`.

**Derived in-page:** which decision governs each SPC-004 claim, whether that
authority is whole or partial, and which claims have no backing evidence at all.

## Conformance input — TSK-008 `system-atlas`

`docs/verification/tsk-008-portal-prototypes/system-atlas/index.html` is a
conformance input for D1 and D2, not a reopened question. Three ideas carry
forward and neither D1 nor D2 contradicts them:

- **boundary** — CodeFlow-owned surfaces are visually distinct from an external
  boundary (`index.html:50–54`);
- **legend** — every non-obvious distinction is named, including what the drawing
  does *not* mean: "Lines show runtime relationships, not data volume."
  (`index.html:98`);
- **evidence chain** — an ordered, source-linked chain with the honest note
  "Generated explanations must reconcile against these sources; missing rationale
  remains visible." (`index.html:118–121`), and the traceability spine
  capability → decision → implementation → verification (`index.html:127`).
