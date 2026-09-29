## Completion gate

Completion requires:

- both seats approved the final plan version and task breakdown, or it is
  approved with recorded settled dissent on named reversible items, or the
  standing seats approved it after a recorded seat loss;
- every acceptance criterion is evidenced, and the result the task exists to
  produce is reached: criteria that pass while that result is missed are a
  finding that returns to `cf-plan`, not a pass;
- required deterministic gates are green, with redness classified as in
  [blocker navigation and gate redness](blockers-and-gates.md);
- when a PR was opened, its required checks were followed within the ship
  poll budget, and the operator received the readiness report with the PR
  URL the tool printed; no agent merged it into a protected target;
- coverage meets the applicable floor;
- UI/design evidence is present or explicitly N/A;
- every unit has approved cross-lineage review and the selected Claude judgment
  primary has approved the integrated design/code judgment;
- design and implementation proportionality are approved;
- every finding from every review, material and minor, is recorded in the task
  closeout or the PR body as finding, severity, disposition and evidence, with
  a disposition that [materiality and prioritization](materiality.md) defines:
  fixed, with the commit; tracked once, with its home and event trigger; or
  dropped, with the reason. A minor finding never blocks, and it is never left
  unrecorded;
- substantial changed prose has its contextual editorial approval;
- no unresolved critical/high security issue or material assumption remains.
- every catastrophic action, if any, has the human authorization and recovery
  evidence required in
  [irreversible actions](irreversible.md); without it, the action was not executed.

For a mode without implementation, read “task breakdown” as the final research,
analysis, plan, or review artifact and apply only the relevant gates above. For
parallel implementation, completion additionally requires a green integrated
worktree and review of the combined diff, not a collection of green task
branches.
