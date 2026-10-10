### Added

<!-- codeflow:release-impact minor -->
- **One planning amendment can span several epics.** A planning pull
  request names every epic it changes on its one `Task:` line, such as
  `Task: EPC-002, EPC-003`, so one reviewed plan change lands as one pull
  request instead of one per epic (ADR-0078). It may also carry files under
  `docs/` outside the adopter-facing paths and the project section of
  `AGENTS.md`, as long as the managed block stays byte-identical to the
  target's; a byte inside that block is refused by name, and product code,
  `CLAUDE.md`, harness settings, skills, policy, hooks and CI still keep a
  range out of the planning class, as do a hidden path, an instruction
  file, a symbolic link or a submodule entry in any folder, judged
  against the target's policy; docs and `AGENTS.md` ride only in a
  repository with no symbolic link or submodule. A release brings such an amendment's
  criteria change as a planning landing. `codeflow ci` prints the class as a
  planning-only amendment of the named epics and adds a
  `work.planning_amendment` note per change, grouped by epic: each task's
  criteria delta, records added or removed, status changes, the doc and
  instruction files touched, and a task record that changed on its
  integration line since the line last merged the target. A change to a
  record of an epic the line does not name is refused, a criteria change
  to a complete task is flagged, and a standalone task or a spec is
  listed. A task pull request still changes only its own
  criteria, and the frozen message now names the planning amendment as the
  route for another task's criteria. There is no policy key.
  Migration: CodeFlow 3.0.0 reads `Task: EPC-001, EPC-002` as a malformed
  `Task:` line and refuses the pull request, so run 3.1.0 locally and in
  the CI that judges a multi-epic amendment; a single `Task: EPC-NNN` works
  on both. A planning pull request that names one epic but changes another
  epic's records, such as a breakdown that creates two epics, now fails
  until its `Task:` line names both. The "PR bodies" paragraph of the
  managed `git-rules.md` names this form, so agents that follow the rule
  use it.
