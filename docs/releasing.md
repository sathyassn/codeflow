# Releasing codeflow

How a codeflow release is cut, and how a project that *consumes* codeflow should
think about its own versioning. Decision record: ADR-0012 (supersedes ADR-0010).
Use [the release checklist](release-checklist.md) as the evidence-bearing
approval record for every run of this procedure.

## codeflow's own releases

Version source of truth: `Cargo.toml [workspace.package] version` (all three
crates inherit it via `version.workspace = true`). Releases are conventional-commit
driven and human-gated. Two tools do the work:

- **git-cliff** derives the next SemVer **and** the changelog from the
  Conventional Commits the commit-msg gate already enforces — reading git history
  only, so it never publishes or runs `cargo package`.
- **cargo-dist** builds four target binaries plus shell and PowerShell
  installers and publishes the GitHub Release, triggered by the version tag.

### Cross-build toolchain

Release CI uses native cargo-dist runners for macOS, Linux, and Windows so each
binary is linked with the platform SDK and can be exercised there. For an
earlier host-agnostic target lint and build check, the repository also provides
Cargo aliases:

```sh
cargo install --locked cargo-xwin --version 0.23.0
cargo install --locked cargo-zigbuild --version 0.23.0
rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
# Install the current stable Zig from https://ziglang.org/download/ or the
# host package manager, then record `zig version` with the release evidence.

cargo cross-check-windows
cargo cross-build-windows
cargo cross-check-linux       # requires Zig on PATH
cargo cross-build-linux       # requires Zig on PATH
```

`cargo-xwin` acquires the Windows CRT/SDK inputs needed to lint and build MSVC
targets from macOS or Linux. `cargo-zigbuild` uses Zig as the linker for a Linux
GNU binary with a glibc 2.17 floor. macOS artifacts still build on macOS because
Apple SDK redistribution/licensing prevents a generic bundled cross toolchain.
Cross-build success proves compilation and linking only; it never replaces a
native Windows/Linux/macOS test and installer canary.

### Presentation renderer assets

The `cf-present` browser distribution is a release input, not an install-time
build. Use the exact Node/npm versions declared in
`crates/codeflow-present/web/package.json`; from that directory run:

```sh
npm ci
npm run supply-chain
npm run check
npm run check:browser
```

`supply-chain` refreshes the committed audit, CycloneDX SBOM, and license
inventory. `check` proves two clean builds are byte-identical and enforces the
raw/Brotli/export budgets and integrity manifest. `check:browser` exercises the
representative accessible renderer and mode/review behavior in a task-owned
browser, including a dense bounded multi-diagram corpus and long-task envelope.
Review the generated diff; do not hand-edit the distribution or its evidence
files. Release builds consume only the committed assets, and consumer machines
do not need Node.

Repeat the runtime journey on every claimed native platform. Windows evidence
must cover Unicode known-folder/profile paths, creation-time ACL hardening,
read-only rejection of weakened owner/protected-DACL/trustee/inheritance state,
trusted system tools, exact quoted command-line identity, file URLs, and
process-tree cleanup. All browser and auxiliary tool routes must exclude
provider-secret environment canaries through the shared restricted environment. Linux/WSL2
evidence must cover bounded no-follow `/proc` identity and process-group
cleanup; macOS must prove its equivalent ownership boundary. Cross-compilation
is useful adapter-shape evidence, but it does not satisfy these native
qualification cases.

Measure the stripped release binary against the recorded pre-presentation
reference build, and record the embedded service/export payload contribution
using the procedure captured for the release. Enforce the per-payload and
combined limits in `codeflow_present::limits`; a debug binary, cross-build, or
compressed archive size is not equivalent evidence.

### Portal ownership migration

The bundled portal's ownership lifecycle is defined by ADR-0058 and SPC-008.
Starter 2.0.0 and adoption schema v2 do not change evidence schema v1. This is a
breaking change to managed portal update behavior, not to general scaffold
merging or Markdown authority. No publishing is implied by a successful build.

| Existing state | Safe next step |
|---|---|
| No adoption | Remain unchanged; adopt explicitly only when useful |
| Managed, unchanged runtime | Setup/update installs the coherent release and migrates state |
| Missing managed runtime file | Managed setup/update repairs it |
| Local runtime edits or incoming collision | Preserve work; choose supported customization, reviewed restoration or explicit transfer |
| Unknown or changed legacy baseline content | Stop and preserve; resolve journal recovery and inspect before manual recovery |
| Transferred runtime | Project owns maintenance; setup/update preserves edits and intentional deletions |

`codeflow portal transfer --confirm` operates on the whole adopted runtime. It
does not install the new embedded release, repair missing runtime/configuration,
or waive integrity checks. It freezes the release actually adopted, not the
release in the current binary. After transfer, update the actual generator and
its state declaration together for a genuine fork; evidence still must pass
`codeflow validate --portal <dir>`. A green verifier is not visual, security,
accessibility or runtime-provenance attestation.

Legacy journals are recovered by the normal portal command under its lease
before full adoption parsing and migration. Never delete journal, stage or
lease data to make an error disappear. If a baseline-integrity failure remains,
the new migration/transfer did not publish; previously journaled recovery may
have completed and must be reported separately. Preserve and inspect the
questionable content and repository history. Once
pending recovery is settled, an explicit reviewed operation may relocate a
verified preserved copy outside the reserved baseline directory or restore
exact reviewed bytes. Retain a restore path; do not normalize line endings or
delete unknown files as a shortcut. Retry the intended command afterwards.
Absent baselines are benign; they do not waive managed runtime-drift checks.

Release evidence includes fresh minimal/standard/full consumers, brownfield and
repeated updates, project-owned byte preservation, generator/state agreement,
before/after-transfer locked builds and real browser journeys, current dependency
and secret scans, and measured unpacked/archive/release-binary size changes.
Keep native macOS, Linux and Windows execution claims separate from cross-target
type checking. Missing native platform or installer evidence remains explicit
and blocks claiming that platform's release qualification.

### The runbook

Prerequisite: install the release-pinned git-cliff version
(`cargo install git-cliff --version 2.13.1 --locked`, or the matching prebuilt
binary from <https://github.com/orhun/git-cliff/releases>).

```sh
# 1. Refresh remote truth and choose an explicit remote-tracking base: latest
#    main for a standalone release, or the accepted integration candidate after
#    every required task has landed. Verify it is the reviewed commit.
git fetch --prune --tags origin
BASE=origin/main  # or origin/integration/EPC-NNN-<slug>
EXPECTED="<reviewed-commit-sha>"
test "$(git rev-parse "$BASE^{commit}")" = "$EXPECTED"
git switch -c chore/release "$BASE"

# 2. Compute the next version from the conventional commits since the last tag:
NEXT=$(git cliff --bumped-version)        # current history resolves to v3.0.0
echo "$NEXT"

# 3. Bump the single workspace version (all three crates inherit it):
#    edit Cargo.toml -> [workspace.package] version = "<NEXT without the leading v>"
#    then refresh the lockfile:
cargo build

# 4. Generate a review aid; do not write it over the curated changelog:
git cliff --unreleased --tag "$NEXT" > /tmp/codeflow-release-notes.md

# 5. Promote the curated Unreleased body in CHANGELOG.md to a dated release,
#    restore an empty Unreleased section above it, and reconcile the draft.
#    Preserve human-written migrations and comparison links; rerunning this
#    procedure must not duplicate a release section.

# 6. Commit and open a PR:
git commit -am "chore(release): $NEXT"
git push -u origin chore/release   # then open the PR
```

- A **human merges** the release PR when required checks are evidenced green
  (ADR-0006/0007; an infra-killed duplicate job is not a failed check) — agents never
  merge to `main`.
- **Before tagging, re-verify the harness-parity claims** against the
  currently installed harness versions — these surfaces move fast, and
  ADR-0008/ADR-0013/ADR-0014 and docs/adoption.md's cross-harness section pin
  a version that decays:
  - The PreToolUse payload contract (`git-guard`/`exec-guard`) still matches
    what Claude Code and an interactive Codex session send.
  - The Codex `hooks.json` events still fire as documented, and the `cf-guard`
    permission-profile keys in `.codex/config.toml` still validate — run
    `codex --strict-config doctor` from a checkout with the shipped
    `.codex/config.toml` in place; `--strict-config` errors out on any field
    the installed Codex no longer recognizes.
  - The Claude settings/hook schema (`.claude/settings.json`) still matches
    what the installed Claude Code expects.
  - The host-neutral duo contract test passes, and both native interactive
    lanes complete a scoped canary with the task's required MCP tools:
    Claude Code → Codex through the enabled official plugin, and Codex →
    Claude through task-scoped tmux with Stop/StopFailure hook completion.
    Record versions, exact commands, and observed tool access. Do not accept
    auth status output in place of a working interactive session.
  - The current ensemble record names only bindings qualified for this release;
    every `capability-supported` harness catalog entry still proves the full
    capability contract without being mistaken for concrete binding
    qualification.
    Run `codeflow doctor --check model-bindings` for retained local promotion
    records and resolve requested/observed, harness-version, or declared
    settings drift. Confirm the repository's project selection is absent/empty
    or resolves atomically to exact stable-role binding IDs; a diagnostic pack
    or parseable harness name is not promotion evidence.
  - Before the repository is public, confirm no committed CodeQL workflow has
    entered the scaffold. After it is public, enable GitHub CodeQL default setup
    for Rust with `security-extended`, verify intended file coverage and zero
    tool-status errors, and collect five healthy applicable PR runs before
    considering the check required. Roll back branch-protection requirements
    before disabling the setup.
  - `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` passes
    locally; CI billing/availability never substitutes for this evidence.
  - `cargo dist plan --output-format=json` lists all four archives, both
    installers, and native runner rows. Canary the shell installer on each
    macOS/Linux architecture and the PowerShell installer on Windows; confirm
    WSL2 selects the Linux archive and native Windows installs `codeflow.exe`.

  Record new verification in a current ADR/release note and update
  docs/adoption.md if anything drifted; historical ADR bodies remain append-only.
- After merge, tag the release; the tag drives cargo-dist:

  ```sh
  git tag -a "$NEXT" -m "CodeFlow $NEXT" && git push origin "$NEXT"
  ```

  A real tag push triggers `.github/workflows/release.yml` — **no PAT or bot
  needed**.

### Why not release-plz / a bot?

release-plz was evaluated and removed (ADR-0012): it is publish-centric and
cannot `cargo package` this unpublished binary workspace. If full PR-bot
automation (a bot opens the release PR for you) is wanted later, **release-please**
fits — it has no crates.io coupling, unlike release-plz — but it needs a Personal
Access Token so its tag triggers cargo-dist. Until then, the runbook above is the
reliable, verifiable path.

## Versioning in a project that consumes codeflow

codeflow gives your repo the *substrate* for clean releases — the commit-msg gate
enforces Conventional Commits, so your history is SemVer-derivable — but it does
**not** scaffold a release pipeline. Release/version/changelog tooling is
stack-specific and stays yours to choose. Because your commits are already
conventional, any of these has clean input:

| Situation | Reasonable choice |
|---|---|
| Rust crate, published to crates.io | `release-plz` (the standard) or `cargo-release` |
| Rust binaries, not published | `git-cliff` + `cargo-dist` — what codeflow itself uses |
| Any language, PR-based automation | `release-please` |
| Changelog only | `git-cliff` or `conventional-changelog` |

codeflow's job is the discipline; the release mechanism is yours. This split is
deliberate — release tooling is as stack-specific as a test runner, so codeflow
records standards and enforces commit hygiene rather than prescribing one
release tool for every consumer.
