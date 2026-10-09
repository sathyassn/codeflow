### Fixed

<!-- codeflow:release-impact patch -->
- **A branch claimed on a reviewed predecessor head passes its own push
  check.** `codeflow work claim <id> --on <pred>@<sha>` accepted a reviewed
  pin, but the push it then made ran `codeflow ci` over the predecessor's
  commits as if the successor had made them and refused on the
  predecessor's own criteria change and on its status at the target
  ([#69](https://github.com/sathyassn/codeflow/issues/69)). A push of a
  task branch now honours each predecessor head it contains that a review
  row names, through the same lookup `work claim` uses: the commits up to
  it are judged as the predecessor's reviewed pull request, so its criteria
  change is printed, its completion binds at the pin, its status is read
  there, and the journey rule holds the successor only to the paths its
  own commits change and each merge changes against the automatic remerge
  of its parents, so a deletion of the predecessor's file counts even
  inside a merge, and an octopus merge refuses. A head
  no review names is not honoured, a successor that changes, reverts or
  removes the predecessor's record as the pin has it is refused, and the
  pull request into the target still waits for the predecessor to land.
  A pin may now also be the predecessor's tip when the review names the
  commit before it and the tip adds only the predecessor's status and
  Closeout, as cf-ship records the completion after review. An acceptance
  block may write `follow_ups: "none: <reason>"`, quoted, so the block
  loads as YAML; the plain form stays valid.
