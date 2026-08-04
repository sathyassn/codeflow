# Ordinary chat reply — P1

What an agent would type into the terminal today, with no rendered surface at all.
Retained so the plain-baseline differential in `rubric.v2.md` has something real to
compare against; it is not a design.

---

EPC-005 has six unfinished tasks. Here they are with their still-open dependencies
and their wave positions:

| Task | Status | Open dependencies | Earliest wave | Latest wave |
|---|---|---|---|---|
| TSK-013 | in_progress | none | 1 | 3 |
| TSK-014 | in_progress | none | 1 | 1 |
| TSK-009 | blocked | TSK-014 | 2 | 3 |
| TSK-011 | blocked | TSK-014 | 2 | 2 |
| TSK-007 | blocked | TSK-011 | 3 | 3 |
| TSK-010 | todo | TSK-007, TSK-009, TSK-013, TSK-014 | 4 | 4 |

Earliest wave is one more than the latest wave of any dependency that is still open.
Latest wave is the epic's last wave minus the longest run that still has to follow
the task. Where the two are equal there is nothing to give.
