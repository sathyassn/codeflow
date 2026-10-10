### Added

<!-- codeflow:release-impact minor -->
- **Pull request Summaries open with a prose lead, then bullets.**
  `codeflow ci` now checks the shape of a pull request body's Summary under
  a new policy key, `git.pr_summary`, which blocks by default: one prose
  paragraph that anchors the reader, then the details as a list or a table,
  then at most one closing paragraph. Only visible blocks count, so text in
  an HTML comment never supplies the lead or the list, and a heading, code
  block, quote or HTML block in the Summary fails. It judges shape, never a
  word or sentence count (ADR-0071, note of 2026-10-03). It runs at warn
  while a kept PR template is diagnosed, a trusted automation profile skips
  it, and a project lowers it by setting `git.pr_summary` to `warn` or
  `off`. The PR template, `writing.md` "Summaries" and cf-ship's PR
  evidence reference teach the shape. The shipped policy file does not list
  the key, so neither `init` nor `update` writes it and an older binary
  never meets it; a project that sets it runs 3.1.0 or later locally and in
  CI.
  cf-ship also says that a pull request already reported ready goes back
  to draft before any further change to its branch, and its release
  integration steps move to their own reference, read only after an
  epic-line landing with a configured release branch.
