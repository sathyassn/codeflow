### Fixed

<!-- codeflow:release-impact patch -->
- **A reviewed task can take its moved target without a new review.** The
  release-impact check needs a pull request to contain the current target,
  so after every merge to `main` a reviewed task merges `main` in. In a
  clone whose local `main` lags `origin/main`, such as a root checkout that
  is never pulled, `codeflow ci` and the pre-push hook then refused the
  completion with `work.acceptance_binding`, because they read the task's
  target from the stale local branch, took the merge for foreign work and
  asked for a new review. The binding now checks the merge against the
  target tip the run is judged against: the base `codeflow ci` is given,
  which hosted CI sets to the pull request's base, and in the pre-push hook
  also the destination default branch's advertised tip. A local branch, its
  upstream configuration or a remote-tracking ref never decides it. A merge
  whose second parent is on that tip's first-parent line, and whose
  recorded result equals the conflict-free automatic merge of its parents,
  keeps the binding. Any other merge, a merge of more than two parents, a
  later commit beyond the record's status and Closeout, a graft or replace
  ref, or a shallow cut on the walked chain refuses, even when the change
  cancels out, and the refusal names that commit. For the reopen rule in
  the entry on new tasks above, a landed record named `TSK-NNN.MD` counts
  as on the target, since the record reader takes the `.md` extension in
  any case, and only the target the run is judged against supplies the
  criteria a reopened task keeps: a local branch or remote-tracking ref,
  such as an `origin/main` or another remote's upstream pointed at the
  task's own branch, can make the check stricter but never supplies
  criteria.
