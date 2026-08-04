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

## P3, D1, D2

Not authored in this pass. Their inventories are part of the continuation
recorded in `NEXT.md` and must be completed before any candidate for those cases
is written.
