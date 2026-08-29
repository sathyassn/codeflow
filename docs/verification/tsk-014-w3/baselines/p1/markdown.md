# EPC-005 — unfinished tasks

Ordinary repository Markdown: the form this subject takes today in a task record or
a status note. Retained as the second plain baseline; it is not a design.

## Remaining tasks

- **TSK-013** — prepare the v3 release candidate. `in_progress`, nothing open blocks it.
- **TSK-014** — recover the design method and utility directions. `in_progress`, nothing
  open blocks it.
- **TSK-009** — build and qualify the documentation portal utility. `blocked` on TSK-014.
- **TSK-011** — build the interactive presentation utility. `blocked` on TSK-014.
- **TSK-007** — verify and qualify the presentation utility. `blocked` on TSK-011.
- **TSK-010** — integrate and release the CodeFlow system. `todo`, waiting on TSK-007,
  TSK-009, TSK-013 and TSK-014.

## Wave positions

| Task | Earliest wave | Latest wave |
|---|---|---|
| TSK-013 | 1 | 3 |
| TSK-014 | 1 | 1 |
| TSK-009 | 2 | 3 |
| TSK-011 | 2 | 2 |
| TSK-007 | 3 | 3 |
| TSK-010 | 4 | 4 |

Earliest wave is one more than the latest wave of any still-open dependency. Latest
wave is the epic's last wave minus the longest run that still has to follow the task.
