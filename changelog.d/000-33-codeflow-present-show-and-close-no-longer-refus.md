### Fixed

<!-- codeflow:release-impact patch -->
- **`codeflow present show` and `close` no longer refuse a browser that has
  just exited.** When the browser's main process was gone but another
  process of its group had not finished exiting, or had exited and was not
  yet reaped by its parent, both commands failed with "browser process group
  exists without its verifiable leader" (seen on Linux in hosted CI). Right
  after a browser closes, its helper processes exit a moment later; in a
  container whose first process never reaps, they stay as zombies for good.
  The check now waits up to 2 seconds for such a group to finish exiting,
  and on Linux it counts a group whose every member has exited, by its
  `/proc` state, as gone. A `/proc` read that finds the process already
  reaped counts as gone too, in this check and in the browser scan. A group
  that still holds a running member, or one it cannot read, is refused as
  before, and no process it has not proven is signalled.
