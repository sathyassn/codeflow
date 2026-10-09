### Fixed

<!-- codeflow:release-impact patch -->
- **`codeflow present show` says when the browser is already open.** On a
  session whose browser is still running, `show` exited 4 with
  "presentation browser launch is not qualified". It now says the session's
  browser is already open and tells you to switch to its window, or quit
  that browser and run `show` again; `show --no-launch` prints the
  session's address. The exit code is still 4.
