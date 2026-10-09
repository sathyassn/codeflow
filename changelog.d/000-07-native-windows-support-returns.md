### Added

<!-- codeflow:release-impact patch -->
- **Native Windows support returns.** The release publishes an x86-64
  Windows archive with `codeflow.exe` and a PowerShell installer again,
  beside the macOS and Linux builds; 3.0.0 published neither. The Windows
  jobs gate every pull request to `main` and every publication again,
  behind one `windows` check.
  The defects it found are fixed: the guards, `codeflow doctor` and the
  Codex and Grok trust checks now treat the short (`RUNNER~1`), long and
  `\\?\` spellings of one Windows path as the same file; the full-gate lock
  can be read by a second gate on Windows; a directory in the way of a
  ledger file is named as a directory there; a shipped spec checked out
  with CRLF line endings no longer counts as edited; and the model
  evaluation kit keeps its signing key owner-only through the key's and
  its folder's access lists, since Windows has no POSIX mode bits: it
  writes the key only once both lists are proven private and refuses
  either list that lets in another account.
  On Windows the guards refuse a recursive delete below any `/`-rooted
  path, such as `rm -r /tmp/scratch`: that path names no fixed place
  there, and a junction can send the delete anywhere. A relative path is
  judged as before.
