### Fixed

<!-- codeflow:release-impact patch -->
- **A pull request can no longer lower its own security review.** The
  managed `security review` job read `git.security_review` and
  `git.dep_audit` from the pull request's own checkout and fell back to
  `warn`, so a pull request could set both to `off`, or delete them, and
  skip the dependency audit that judged it (sathyassn/codeflow#81). The job
  now reads both keys, and the `osv-scanner.toml` suppressions, from the
  trusted commit: the pull request's base, or the pushed commit on a push.
  A missing policy file, a missing or unreadable key, or a value other than
  `block`, `warn` or `off` fails the job instead of warning. `codeflow ci`
  names a change that lowers or removes either key. osv-scanner now runs
  with `--no-ignore`, so a `.gitignore` the change edits cannot hide a
  lockfile, and its verdict comes from its exit status, so a file path in
  its output cannot turn an advisory into a pass and a lockfile it cannot
  read is a scan error. The scanner is downloaded outside the checkout, so
  a link the change commits at its download path cannot redirect the
  write. The project setup hook cannot skip the test gate by accident: a
  `codeflow` function, a `PATH` entry, `set +e` or an `exit` in it leaves
  the gate running or fails the run. The hook still runs with the gate's
  authority, like the CI file a change can edit, so `codeflow ci` names a
  change that adds, edits or removes it for its reviewer. To adopt: if your
  policy lacks either key, land the keys first under your current workflow
  (`codeflow update` adds them), then the 3.1.0 workflow, since the job
  reads the keys from the base. A first CodeFlow adoption has no policy on
  its base, so its security review fails until the policy lands. In
  `block` mode, a new suppression takes effect once it lands, so an
  advisory that blocks every pull request is cleared by fixing the
  dependency, or by an administrator merging the suppression over the
  failing check.
