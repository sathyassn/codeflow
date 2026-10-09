### Fixed

<!-- codeflow:release-impact patch -->
- **A pull request can no longer exempt its own leak from the secret
  scan.** The CI template's gitleaks step read `.gitleaksignore`,
  `.gitleaks.toml`, a `.gitleaks.json` beside it and inline
  `gitleaks:allow` comments from the pull request's checkout, so the change
  that added a secret could add its exemption too and pass. The step now
  reads every exemption from the trusted commit: the pull request's base,
  or the pushed commit on a push. A new exemption takes effect once its own
  pull request merges, and an inline `gitleaks:allow` comment counts only
  on a commit the trusted commit already holds. A file your configuration
  extends by `[extend] path`, and a `GITLEAKS_CONFIG` file in the
  repository, are read from the trusted commit as well. The step fails
  with a message naming the fix when the trusted commit is not in the
  checkout, when it does not hold an extended file, or when an extended
  file is named by an absolute path into the checkout. gitleaks is now
  downloaded and unpacked under the runner's temp directory, so a file or
  link a pull request commits at that name is not written through. Nothing
  from the checkout runs or steers the scan: its Python helpers run
  isolated (Python 3.11 or later), git reads `.gitattributes` from the
  trusted commit (git 2.41 or later), and no step before the scan runs
  code from the checkout. If you add a step to the secret-scan job, add it
  after the scan. **If you customised the workflow and the secret-scan job
  already runs a step of yours before the gitleaks step, move that step
  after the scan or into another job when you update:** the 3-way merge
  keeps it, and `codeflow update` now warns about it on every run until it
  moves. If you renamed the scan step, update cannot check the order and
  says on every run that the job's step order needs your review. gitleaks
  now reads the whole history of HEAD, the base's
  included (on a pull request, the pull request merged into its base; on
  a push, the pushed commit), instead of every fetched branch and tag, so
  an unrelated branch can no longer fail a pull request's scan. This
  narrows coverage on purpose: a branch with no pull request, or a tag, is
  not scanned by this workflow unless its commits become reachable from a
  scanned HEAD, so a repository-wide audit needs a scan of its own. The
  refusals below check only the commits a pull request brings; on a push
  that range is empty, since the pushed commit is the trusted commit, so a
  push scan reads its history without them. gitleaks also reads what a merge
  itself adds, files whose type changes and files git judges binary, which
  gitleaks' default history scan leaves out, so a secret added in a merge
  resolution, in a file that replaces a link or after a NUL byte is
  reported, under the file's own path. In the commits a pull request
  brings, git cannot show what an octopus merge adds, so the step refuses
  one; merge the branches one at a time. It also refuses a path with a
  backslash, a double quote or a control character that one of those
  commits changes, or that either side of a merge among them changes,
  since gitleaks cannot read such names reliably. Each refusal names the
  commit and the refs that hold it; rewrite the pull request's commits, or
  rebase onto the base when the change is on the base's side, since a
  later rename leaves the name in the earlier commit. Names with spaces or
  non-ASCII letters pass. The scan pins git's patch format, so git
  configuration on the runner, such as `diff.noprefix`, cannot move a
  finding to another path.
