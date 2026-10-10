### Added

<!-- codeflow:release-impact minor -->
- **Task records name their deliverables and where they go.** The task
  template has a `## Deliverables` section after Description: each output
  (files or a folder, a decision record, a research note, evidence, a
  record update, a human board) and its home as a path in the project's
  structure, or a provisional home with what decides it. The epic
  template's "Affected surfaces and interfaces" asks for the homes the
  epic's tasks write, or a pointer to the project's structure authority,
  and the `cf-method` clarity checklist that `cf-plan` applies checks every
  task's deliverables and homes against that authority before records are
  materialized. `codeflow validate --docs` warns about an open task with
  no filled section and no path in its Description; the warning never
  blocks, has no policy key, and never reads a complete or cancelled
  record. It errs toward silence: anything that plausibly names a path,
  Windows paths and `README` included, satisfies it. After `codeflow update`, an adopter's
  `project-management/templates/` carries the new section.
