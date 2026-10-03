# Release integration after landing

Read after an epic-line landing, only when the project configures a
release branch matching its release pattern, R-120, and a workflow
integrating into it; `cf-ship` step 9 points here.

Read the configured integration result after the landing; a task pull request
never waits for release integration. Find the run for that landing with
`gh run list --workflow <configured-workflow>`, then use
`gh run view <run-id> --exit-status` and `gh run view <run-id> --log-failed`
on failure. A pending run is missing evidence.

Route a failure to the open task with `role: release-integration`; if none
carries the role, report "no release-integration task to own it". Follow the
result's local reproduction commands. The CodeFlow-only integration workflow
and runner are not installed for adopters. For that workflow, reproduce the
current result without pushing with
`cargo run -p codeflow-cli --example release_integration -- --release <branch>`.
Every surviving run catches up all verified lines; if a pending run was replaced,
inspect the later run's result for the landed tip. Add `--line <line>` only to
narrow a local investigation, not to reproduce the full workflow batch.
Without a configured release argument and
workflow, the runner reports no configured integration and does nothing.
