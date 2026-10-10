### Fixed

<!-- codeflow:release-impact patch -->
- **`codeflow present show` and `close` no longer fail on Linux because of an
  unrelated process.** The scan for the presentation browser read the command
  line of every process of the same user and refused the whole scan with
  "browser identity is not UTF-8" when one was not UTF-8, or with an
  "exceeded its bound" error when one was over 64 KiB, so a single such
  process anywhere on the machine broke `present show`, `present close` and
  the recovery of an interrupted launch (issue 60, seen in hosted CI). The
  scan now reads each command line once, in fixed memory, and skips a process
  that lacks an argument equal to the profile argument or one equal to the
  instance argument of the browser it is looking for, whatever its other
  bytes are. A process that has both is checked as strictly as before, and a
  command line over 8 MiB, which the kernel does not allow a new process,
  still fails the scan. macOS lists processes with `ps` and already
  tolerated such lines.
