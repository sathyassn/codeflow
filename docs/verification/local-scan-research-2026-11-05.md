# Local secret scan and dependency audit research

`codeflow scan` runs the hosted secret scan and dependency audit verdicts
locally over `base..head`, and the opt-in `git.scan_on_push` key runs it from
the pre-push hook. This note answers the three research questions TSK-1176
AC-9 names (moved from TSK-430 AC-4) and states the choice each criterion
implements. Checked on 2026-11-05 against gitleaks 8.30.1 and osv-scanner
2.4.0 on macOS arm64.

## Advisory download at pre-push

- **Question.** Can the pre-push run audit dependencies without a network
  call, and should it download anything?
- **Finding.** osv-scanner 2.4.0 reads advisories from the OSV.dev API
  (`api.osv.dev`) during `scan`. An offline mode exists
  (`--offline-vulnerabilities` with a downloaded database), but the database
  is large and the hosted job does not use it, so an offline local verdict
  could differ from the hosted one.
- **Choice.** The local audit queries OSV.dev as the hosted step does, so
  the verdicts match (AC-6). CodeFlow downloads no binaries and no database:
  the operator installs the pinned binaries. `git.scan_on_push` is off by
  default (AC-8), so no push makes a network call unless the project turns
  it on. Any osv-scanner exit code other than 0 (clean), 1 (advisories) or
  128 (no sources) is a scan error, which fails closed at `block` and warns
  at `warn` (AC-7). A run with no network was not tested here.
- **Measured.** On a 10-commit task range with a debug build:
  `codeflow scan --only secrets` 4.5 s, `--only deps` 8.7 s. The hook test
  states a 60 s bound for a task-sized range.

## Binary location per platform

- **Question.** Where does the command find each scanner, and how does it
  know the binary is the pinned one?
- **Finding.** Each release publishes per-platform assets with SHA-256
  sums: gitleaks as archives (the TSK-412 pins in `hooks/gitleaks.rs` hold
  the archive and binary sums), osv-scanner as bare binaries listed in
  `osv-scanner_SHA256SUMS`. The six osv-scanner sums in `hooks/osv.rs`
  (linux, darwin and windows, each amd64 and arm64) were checked by download.
- **Choice.** Both runners search only the absolute entries of `PATH`, in
  order, take the first file named `gitleaks` or `osv-scanner` (`.exe` on
  Windows), and accept it only when its SHA-256 equals the pin for the
  running platform and its `--version` line names the pinned version. A
  platform without a pin cannot verify. A missing or mismatched binary fails
  closed at `block` (AC-7). `codeflow doctor --check scan-tools` reports both
  lookups and pins, and fails when `git.scan_on_push` is on and a scan at
  `block` has no usable binary.

## The shared engine

- **Question.** Does the local command reuse the commit-time scanner or add
  a second one?
- **Finding.** The TSK-412 runner already verifies the pinned gitleaks,
  builds a private scan folder and checks gitleaks' git processes through
  trace2. The hosted job's range rules (every introduced commit, remerge
  diffs, replace refs ignored, octopus merges refused, base-only exemptions)
  are what a local verdict has to match.
- **Choice.** `hooks/gitleaks/range.rs` extends the TSK-412 engine with a
  range scan that applies the hosted job's rules: `GIT_NO_REPLACE_OBJECTS=1`
  and `GIT_ATTR_SOURCE` at the base on every git read, the same `--log-opts`,
  and `.gitleaksignore`, the gitleaks configuration and its extend chain read
  from the base commit (AC-6). The dependency audit is a new runner,
  `hooks/osv.rs`, beside it: it exports the head tree, replaces every
  `osv-scanner.toml` with the base's copy, and runs the base's
  `.codeflow/audit-exceptions.sh` hook when advisories are found, as the
  hosted step does. The pre-push hook calls the same command
  (`cmd/push_set.rs`), so there is one engine for all three surfaces.

## Limits

- A range holding a merge commit is refused on Windows, because the remerge
  header filter is a Unix shell wrapper.
- Parity with the hosted job is shown by tests on fixtures here. A hosted
  run on a draft pull request (AC-6 and AC-10) has not been made.
