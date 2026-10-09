### Fixed

<!-- codeflow:release-impact patch -->
- **The exec-guard refusal names the file route for text that mentions a
  peer.** On a line exec-guard cannot fully parse, such as one with a
  variable as the program or a here-string, it judges the raw text, so a
  heredoc or inline string that names a peer with a headless flag is
  refused, and a review brief or commit message written that way was
  refused with no way forward. That matching is unchanged and is flagged
  by design, refused at the default block level: a reader that tried to
  leave such text out kept
  missing shell forms that still run a peer. The refusal now says so and
  names the route that works: write the text to a file with the editor
  tool and pass it by path, as `git commit -F <file>`,
  `gh pr create --body-file <file>` or `gh api ... -F body=@<file>`.
  Verdicts are unchanged: 1,618 headless run forms and 12 text shapes
  compared with 3.0.0 get the same verdict.
