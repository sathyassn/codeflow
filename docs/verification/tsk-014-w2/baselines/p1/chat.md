# P1 ordinary-chat baseline

The floor this case must beat: what a competent assistant types into the
terminal when asked about the EPC-005 plan graph. Authored before any candidate.

---

EPC-005 currently has ten tasks. TSK-005, TSK-006 and TSK-008 are complete, and
TSK-012 was cancelled. That leaves six unfinished: TSK-007, TSK-009, TSK-010,
TSK-011, TSK-013 and TSK-014.

TSK-014 depends on TSK-005, TSK-006 and TSK-008, all of which are complete, so
it can be started. TSK-013 has no dependencies and is already in progress, so it
can also be worked. Everything else is waiting: TSK-011 needs TSK-014, TSK-009
needs TSK-008 (done) and TSK-014, TSK-007 needs TSK-011, and TSK-010 needs
TSK-007, TSK-009, TSK-013 and TSK-014.

That means TSK-014 sits in front of most remaining work — TSK-011 and TSK-009
both wait on it directly, TSK-007 waits on it through TSK-011, and TSK-010 waits
on it both directly and through the others. TSK-013 is the exception: it reaches
TSK-010 on its own, without going through TSK-014 at all.

The longest remaining chain is TSK-014 → TSK-011 → TSK-007 → TSK-010, four tasks
across four waves, so that chain is what sets the release date. Of the two tasks
you could start today, TSK-014 is on that chain and has no room, while TSK-013
could start as late as wave 3 without moving the finish — two waves of room.
TSK-009 has one wave of room but is not startable yet.

All of these except TSK-005 land on `integration/EPC-005-presentation-system`;
TSK-005 landed on `main`. TSK-012 is cancelled and should not be counted as
outstanding work.
