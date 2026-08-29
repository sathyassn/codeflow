# EPC-005 plan graph

Ordinary Markdown treatment of the same content — the second baseline. Authored
before any candidate. `baseline.html` is a faithful hand-authored HTML rendering
of this file (no Markdown library, so the study adds no dependency).

## Tasks

| Task | Status | Depends on | Stream |
|---|---|---|---|
| TSK-005 | complete | — | method |
| TSK-006 | complete | TSK-005 | presentation |
| TSK-007 | blocked | TSK-011 | presentation |
| TSK-008 | complete | TSK-005 | portal |
| TSK-009 | blocked | TSK-008, TSK-014 | portal |
| TSK-010 | todo | TSK-007, TSK-009, TSK-013, TSK-014 | release |
| TSK-011 | blocked | TSK-006, TSK-014 | presentation |
| TSK-012 | cancelled | TSK-005 | method |
| TSK-013 | in_progress | — | release |
| TSK-014 | in_progress | TSK-005, TSK-006, TSK-008 | both utilities |

## Notes

- Startable now: TSK-013, TSK-014.
- Blocked: TSK-007, TSK-009, TSK-010, TSK-011.
- Longest remaining chain: TSK-014 → TSK-011 → TSK-007 → TSK-010.
- Room: TSK-013 has two waves, TSK-009 has one. Everything on the longest chain
  has none.
- TSK-012 is cancelled and is not outstanding work.
- All tasks target `integration/EPC-005-presentation-system` except TSK-005,
  which targeted `main`.
