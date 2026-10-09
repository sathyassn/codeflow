### Added

<!-- codeflow:release-impact minor -->
- **`codeflow estimate outcomes` derives completed tasks' timings from git
  and compares them with the frozen forecasts.** For each complete task
  record it reports when the task was planned (its record reached the
  target), started (the first task-branch commit after the target, a lower
  bound), blocked and cleared, completed (the commit that wrote its
  acceptance block) and landed (the first target commit holding the
  reviewed commit), from commit author times; a squash, rebase or
  fast-forward landing gives `started: unknown` with the reason. It joins
  each task by id to the planning scenario of the adopted home's frozen
  forecasts, or of one `--forecast`, and prints each task's ratio and, once
  a work type has `--minimum` ratios (default 3), the median and range with
  the line "outcomes contradict the forecast" when the median is below
  `--low` (0.5) or above `--high` (2). The thresholds are printed defaults,
  not policy keys and not a gate. An adopted `.codeflow/estimate.json` whose
  home does not exist is reported and the command exits 1. It writes
  nothing, and `--json` emits a versioned report. `codeflow status` prints
  one estimates line, only where `.codeflow/estimate.json` exists.
  cf-estimate's operating reference says outcome timings come from this
  report and adds a cold-start trigger: the first three completed outcomes,
  or one probe, prompt a recorded recalibration (ADR-0079, GitHub issue 77).
  Standard and full tiers receive the method text with `codeflow update`; no
  record, key or required file changes.
