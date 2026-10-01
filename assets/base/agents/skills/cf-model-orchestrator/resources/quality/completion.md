## Completion: the acceptance block and PR body

A task is complete when its acceptance block, the task PR's last commit, and
its PR body show:

- one approval of the plan's shape, with any recorded settled dissent on
  named reversible items, and for a mode without implementation the
  final research, analysis, plan or review artifact in its place;
- every acceptance criterion not marked deferred is evidenced, and the result
  the task exists to produce is reached: criteria that pass while that result
  is missed are a finding that returns to `cf-plan`, not a pass;
- required deterministic gates are green, with redness classified as in
  [blocker navigation and gate redness](blockers-and-gates.md);
- the PR's required evidence exists and is cited; nothing polls by default,
  and only where the adopted policy requires hosted checks green before
  landing are they awaited with the bounded wait `cf-ship` step 7 names, never
  an open-ended poll. The operator received the readiness report with the PR URL the tool
  printed; no agent merged a protected branch;
- coverage meets the applicable floor;
- UI/design evidence is present or explicitly N/A;
- every unit has approved cross-lineage review and the selected Claude judgment
  primary has approved the integrated design/code judgment;
- design and implementation proportionality are approved;
- every finding from every review, material and minor, is recorded in the PR
  body as finding, severity, disposition and evidence, with a disposition that
  [materiality and prioritization](materiality.md) defines: fixed, with the
  commit; tracked once, with its home and event trigger; or dropped, with the
  reason. A minor finding never blocks, and it is never left unrecorded;
- changed prose that meets the `cf-editorial-review` trigger has its
  editorial approval;
- no unresolved critical/high security issue or material assumption remains;
- every catastrophic action, if any, has the human authorization and recovery
  evidence required in
  [irreversible actions](irreversible.md); without it, the action was not executed.

Apply only the gates relevant to the mode. A batch lands only on a green full
gate of its exact candidate, not on a collection of green task branches.
