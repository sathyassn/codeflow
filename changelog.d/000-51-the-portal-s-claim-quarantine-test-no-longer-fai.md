### Fixed

<!-- codeflow:release-impact patch -->
- **The portal's claim quarantine test no longer fails at random.** The
  docs-portal test "unverified retired claims are quarantined without
  regaining authority" replaced the retired workflow claim by deleting it
  and writing a new file. The portal identifies a claim by device and inode
  number, so on a filesystem that hands a freed inode straight back (ext4,
  tmpfs) the replacement could be taken for the original, the lease was
  released, and the test saw no error code. It failed once in hosted CI and
  passed on rerun. The test, in the shipped starter and in this repository's
  own copy, now builds the replacement while the original still exists, so
  the two inodes differ on a filesystem that numbers coexisting files
  uniquely, and asserts that. A portal adopted earlier gets the fix when
  `codeflow portal setup` reconciles the starter, with an updated binary
  and managed files you have not edited; a portal whose ownership was
  transferred, or whose managed test file was modified, is not updated.
  The portal's runtime and its claim identity check are unchanged.
