### Fixed

<!-- codeflow:release-impact patch -->
- **The scaffold passes its own secret scan.** In every repository
  scaffolded by 3.0.0, the CI secret scan failed from the first commit:
  gitleaks' `generic-api-key` rule took the words "vulnerable/malicious" in
  the pipeline workflow's security stage (line 209 of
  `.claude/workflows/pipeline.workflow.js` and its baseline copy) for a key.
  New scaffolds word it differently. For repositories that already hold the
  3.0.0 line, the CI workflow `codeflow update` installs runs gitleaks with
  your configuration as before, then drops only `generic-api-key` findings
  whose value and matched text are exactly that prose in those two paths, so
  the scan passes without editing history. Anything else, on the same line
  included, still fails the job, and so does a scan that logs an error,
  reads no commit, or whose git run fails part way. A wrapper that
  runs gitleaks itself can add the entry the CI README shows.
