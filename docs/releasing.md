# Releasing codeflow

How a codeflow release is cut, and how a project that *consumes* codeflow should
think about its own versioning. Decision record: ADR-0012 (supersedes ADR-0010).
Use [the release checklist](release-checklist.md) as the evidence-bearing
approval record for every run of this procedure. ADR-0061 automates the
candidate and exact-source dispatch while preserving the human decision.

## codeflow's own releases

Version source of truth: `Cargo.toml [workspace.package] version` (all three
crates inherit it via `version.workspace = true`). Releases are conventional-commit
driven and human-gated. Two tools do the work:

- **git-cliff** is the sole next-SemVer calculator. It reads the Conventional
  Commits the commit-msg gate already enforces, but never replaces the curated
  Unreleased prose, publishes, or runs `cargo package`.
- **cargo-dist** builds four target binaries plus shell and PowerShell
  installers and is the only tag, release, and artifact publisher. Its
  generated workflow runs only by explicit dispatch from an authorized
  candidate.

PRs carry one `Release impact` section. `scripts/release.py check-pr` compares
the declaration with the actual base-relative changes and landed conventional
markers. It checks known contradictions and demands an assessment for watched
contracts; it does not infer compatibility. git-cliff alone calculates the
version after merge, from reviewed commit markers.

Use a plain `revert:` only when the resulting change has no shipped release
impact. A revert that changes supported behavior or a public contract must use
the `fix:`, `feat:`, or breaking marker that describes the resulting release,
with matching PR impact and curated Unreleased notes.

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

### Automated candidate and authorization

1. A push to `main` runs `release-candidate.yml`. It verifies any existing
   candidate's recorded hashes, installs git-cliff 2.13.1, and asks it for the
   next version. An empty Unreleased section or a range containing only
   `none`-impact/revert commits is a no-op before git-cliff runs; this is
   important because pinned git-cliff 2.13.1 retains a patch floor even when
   repository policy assigns no release impact. A merged candidate with no
   corresponding tag is held for publication, not regenerated. A tag ends this
   local pending guard; it is not by itself evidence that hosted publication or
   installer verification completed, which the release evidence must establish.
2. The preparer promotes the curated Unreleased body without replacing prose.
   It preserves the staged v3 date as a candidate-cut date, never a publication
   claim. It updates the root workspace version only when git-cliff advances it.
   Cargo refreshes the lock, the release CLI runs `codeflow update`, and the
   finalizer verifies every coupled version stamp and records hashes for the
   bounded generated diff.
3. A pinned create-pull-request action maintains the draft
   `chore/release-codeflow` PR. Do not edit generated candidate files directly.
   Apply an accepted note correction to `CHANGELOG.md` on `main`, then restore
   or close the stale candidate so the guarded workflow can regenerate it.
   The repository Actions setting must permit `GITHUB_TOKEN` to create PRs.
   Current GitHub behavior creates approval-required PR workflow runs for token-
   generated opened/synchronize/reopened events; a human must approve and observe
   the checks on the exact head. Editing the impact declaration reruns its check.
4. A **human merges** the exact candidate when every release check is complete.
   The merge—not a label, branch name, model verdict, or moving ref—is the
   authorization. `release-authorize.yml` proves the human event and current
   merge shape, recreates the exact reviewed candidate ref if repository auto-
   deletion removed it, then explicitly dispatches the cargo-dist workflow.
5. cargo-dist's custom local-artifact job re-checks the candidate head, merge parents,
   equal trees, tag target, and existing release state before global packaging or hosting. The
   current repository permits merge commits and disables squash/rebase; a
   different strategy is unsupported until this guard is reviewed. A wrong tag,
   public release, foreign draft, or draft with any asset stops without overwrite.
   The authority prepares an exact empty draft containing the reviewed curated
   notes; a retry may reuse only that same candidate-bound empty draft. With no
   external package publisher, cargo-dist uploads without `--clobber` and uses its
   supported announce phase to create the tag and make the release public only
   after every artifact is available. A provider mutation after the plan check is
   a disclosed race and causes the generated commands to fail rather than replace
   an asset or public release.

A failed or cancelled authority job blocks cargo-dist host and announce. The
supported local-artifact extension lets platform compilation run concurrently,
but global packaging and all hosting wait for authority. A `dry-run` validates
the cargo-dist plan and may rehearse configured artifact builds, but never
creates a draft, hosts, or announces. After announcement, a supported
post-announce job compares the exact tag/source and public asset names, sizes,
and SHA-256 digests with the same-run staged files. cargo-dist 0.32's generated
jobs still receive the workflow's write-scoped token even though every checkout
uses `persist-credentials: false`; this upstream permission breadth is a recorded
limitation, not a claim that untrusted build code is least-privileged.

If publication fails after a valid candidate merge, first resolve the recorded
host collision and rerun the generated workflow against the preserved exact
candidate branch and tag input. To abandon an unpublishable or stale merge, use
a separate reviewed recovery PR: restore every recorded generated file from the
candidate record's `source_commit`, move the promoted notes back under
Unreleased, delete `.release/candidate.json`, and remove any owned empty draft.
Never delete the record alone or let automation guess whether reviewed notes may
be discarded. The next main run then calculates from the restored reviewed state.

Before merging the candidate, re-verify the harness-parity claims against the
currently installed harness versions—these surfaces move fast, and
ADR-0008/ADR-0013/ADR-0014 and docs/adoption.md's cross-harness section pin a
version that decays:
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

The candidate workflow uses the repository's scoped `GITHUB_TOKEN`; it does not
provision a PAT or publication credential. Hosted settings can still prevent
token-created PRs, approval-required exact-head checks, workflow dispatch, or
releases. Treat a zero-step or permission failure as absent evidence and repair
the repository setting—never bypass the source and publication guards.

### Historical bridge into v3

The live `v2.1.0` tag remains at
`d70c6f17d4bf199a545c843856b3ded4681aff20` and is git-cliff's reachable
calculation baseline. The published `source.tar.gz` independently identifies
commit `3c3efdb91009361e18b0fabad699b5e875d4e4dd` and has SHA-256
`1501e0d81716dadd3aa4dc1c56348dd7321abd9cdca90b8f5deb89ea20d54beb`.
The release target and published source agree with each other, not with the
current tag. The automation records all three facts, does not move the tag, and
expects the already-staged `3.0.0` as the first candidate.

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
