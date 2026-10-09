### Fixed

<!-- codeflow:release-impact patch -->
- **A double-quoted backslash stays in the guards' reading.** Inside double
  quotes Bash removes a backslash only before `$`, a backquote, `"` or
  another backslash. The git guard removed it everywhere, so
  `git -C "C:\Users\a\repo" commit` was judged against a path that does not
  exist and refused. It now reads the path the shell passes.
