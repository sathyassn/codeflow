### Added

<!-- codeflow:release-impact minor -->
- **`codeflow ci` warns when a pull request body is too long.** A body over
  1,000 words, counted as a reader sees it (HTML comments left out, fenced
  blocks and tables counted), draws one warning that names the count, the
  limit and the three largest `##` sections, and asks for the body to be
  rewritten to its final state with records linked instead of copied. It
  joins the existing presentation warnings under `git.pr_sections`: no new
  policy key, advisory at any level, and never a blocking finding. It runs
  wherever `codeflow ci` is given a body: a pull request event in hosted CI,
  `CODEFLOW_PR_BODY`, `--pr-body` or `--pr-body-file`. The pre-push hook
  passes no body, so it runs no body check. cf-ship's PR evidence reference
  and the PR template say so.
