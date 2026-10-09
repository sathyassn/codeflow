### Added

<!-- codeflow:release-impact minor -->
- **Reported defects are fixed by cause and class, with a written critical
  path.** A new `cf-method` reference, `issue-handling.md`, takes a reported
  defect from intake to closure: reproduce it and judge its severity, name
  the cause and the defect class, search the tree for every site of the
  class, group issues that share a cause into one unit with a design first
  for guards, parsers, policy, acceptance rules, hooks and CI, fix the whole
  class with a durable check where one can be written, and close the issue
  with the sites fixed and deferred and the release. A review round that
  finds a new instance of the same class stops the rounds and sends the
  unit back to design. A critical defect (live in a release or blocking
  current work, and blocking adopters, weakening a security boundary,
  losing data or hanging a gate) gets interim guidance the same day, a
  prioritized fix when that guidance does not clear the block, a recorded
  release decision and a notice to affected adopters; moving another
  epic's planned work for it is the operator's call. The lifecycle reference's repair bullet points a
  reported defect at it, and `cf-reviewer` checks that the fix covers the
  class and that the sweep is recorded. It adds no check, pull request,
  approval or review round. Standard and full tiers receive it with
  `codeflow update`; nothing else needs to change. For CodeFlow itself,
  `docs/releasing.md` "Critical issues" states the routes and why releases
  stay on `main` with no maintenance branch, and the bug report template
  asks for a severity.
