### Fixed

<!-- codeflow:release-impact patch -->
- **A secret scan finding on one branch no longer fails every pull
  request.** The managed secret scan read the whole history of HEAD, so a
  finding already in the base, or on any branch merged into it, failed the
  scan of every pull request until each target's `.gitleaksignore` carried
  it (sathyassn/codeflow#48). A pull request now scans only the commits it
  brings, and a push only the pushed range. The workflow also runs weekly
  and on manual dispatch, and those runs, a push that creates the branch
  and a push whose previous tip is gone scan the full history, so nothing on
  the default branch goes unscanned; the gates job stays off the schedule.
  Each run prints which history it read, and exemptions are still read only
  from the trusted commit.
  This changes what a pull request's scan means: a finding already on
  another branch no longer fails it, and only a full-history scan (weekly,
  manual or fallback) reports it.
  The weekly schedule runs only once the workflow is on the default branch. To
  adopt, run `codeflow update`, review the workflow it merges with your
  edits, land it on the default branch, and check that the scheduled run
  appears there.
