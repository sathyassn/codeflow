### Added

<!-- codeflow:release-impact minor -->
- **`codeflow doctor --check grok` checks that Grok can run the CodeFlow
  guards.** It warns, naming each file, about a CodeFlow hook command
  Grok would skip because it carries a `$`, with the step that fixes it:
  `codeflow update` where update's own steps rewrite the file or write a
  `.new` merge beside it, and a hand edit for any other file, with the
  reason update leaves it: a file update does not manage, such as
  `.claude/settings.local.json`, a file it skips as a symlink or through
  `[scaffold] ignore`, or an edit it keeps because the shipped version
  has not changed. When both kinds of file are stale, it gives both
  steps. When no exec-guard is bound at all, it offers `codeflow update`
  only where update would bind the shipped guard again, and otherwise
  quotes the shipped guard group to add to `.grok/hooks/codeflow.json`.
  It names a `.new` file update left waiting. When the
  shipped exec-guard handler (its command, timeout and environment) is
  bound where Grok's shell tool hits it, matched as Grok matches, doctor
  judges a fixed canary dangerous command in the payload Grok sends with
  the handler `codeflow hook exec-guard --contract 3` runs, in its own
  process under the catastrophic-command floor alone, reading no policy,
  repository, working directory or environment and recording no
  refusal, and warns unless it refuses with exit 2, a reason and Grok's
  deny answer. Doctor executes nothing for the check, so no hook text,
  hook environment, `codeflow` found on PATH or swapped binary can answer
  for it: a customised handler is reported as unverified, and where PATH
  resolves `codeflow` is reported, flagged when it lies inside the
  repository or is not the binary doctor started from. The canary does
  not exercise a shell, the command-line parsing, the `codeflow` on PATH
  or Grok's own hook call; a live session's hook lines prove those.
