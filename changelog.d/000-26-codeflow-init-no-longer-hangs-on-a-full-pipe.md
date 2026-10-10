### Fixed

<!-- codeflow:release-impact patch -->
- **`codeflow init` no longer hangs on a full pipe.** In a repository with
  enough folders, `codeflow init` could block forever: it wrote all of the
  folder names to `git check-ignore -v -n --stdin -z` before reading any
  answer, while git wrote an answer per name, so once the output pipe
  filled each side waited for the other. The input is now written from
  its own thread while the output is read, through one module that is the
  only place codeflow pipes a child's stdin: the nested-repository scan,
  the `codeflow doctor` probes that pass input, the `gh` calls that pass a
  request body, the ID registry, the pre-push and CI git reads, the
  conflict-marker attribute check and the portal's git reads. Output of
  any size completes, and a child that stops reading early is judged by
  its exit status. A test that parses the Rust sources fails when new code names a child's
  stdin outside that module; code inside macro invocations and raw file
  descriptors are not covered.
