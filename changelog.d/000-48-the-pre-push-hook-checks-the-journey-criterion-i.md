### Fixed

<!-- codeflow:release-impact patch -->
- **The pre-push hook checks the journey criterion its pull request
  will.** `codeflow ci` classified a range only when a pull request body
  was given, so the pre-push run never reached `work.journey_criterion`,
  and a task branch that changes an adopter-facing path without a journey
  criterion passed the push and was blocked by hosted CI once its pull
  request opened. A run without a body now holds a branch that carries its
  task (`task/TSK-NNN-...`) to the journey rule over its range, which needs
  only the task record and the paths the range changes, so the push is
  refused with the finding the pull request check gives. The refusal also
  names a criterion that carries `(journey)` inside its text and says the
  tag counts only where it opens or closes the criterion.
