# Releasing CodeFlow

## Concept

**A CodeFlow release is one deliberate decision by a named human, taken after
the evidence for it already exists.**

Humans merge the normal work PR that carries the release state and dispatch
the publishing workflow, and no agent merges, tags or publishes. This runbook
says how a CodeFlow release is cut and how a project that consumes CodeFlow can
version its own releases. [The release checklist](release-checklist.md)
is the approval record for each run, with a link or pasted output for every
item. A green job, model agreement or peer approval counts there as evidence,
never as the named human release decision.

## Architecture

Each release stage has one actor and one gate, and no later stage can
reinterpret an earlier decision.

- Architecture decision record ADR-0062 (release state lives in the normal
  work PR) is the one operative record for release state.
- It superseded ADR-0061 (a maintained candidate PR), which superseded
  ADR-0012 (git-cliff calculates the version). Read those two as history,
  never as current procedure.

| Stage | Who acts | Gate it must satisfy |
|---|---|---|
| Pending notes and impact | The author of the normal work PR | One `Release impact` section per PR, and one `codeflow:release-impact none\|patch\|minor\|major` HTML marker directly before each new pending entry |
| Release-state check | `scripts/release.py check-pr` | Compares the declaration with the current target, the actual proposed merge tree, pending annotations, coupled stamps and the conventional-marker floor. It checks known contradictions and watched contracts; it does not infer compatibility |
| Merge | A human | PR CI checks the actual proposed merge tree, which is more than the absence of conflicts. Main-push CI repeats the state check without writing. Without strict branch protection a stale clean merge is still possible, so the human merger must require the fresh check |
| Dispatch | A human with current write, maintain or admin permission | Dispatches with `--ref main` and the `vX.Y.Z` tag. The actor and any rerunning actor must both be GitHub Users with effective permission. `GITHUB_SHA` must still equal current main and be the result of an ordinary PR human-merged into this repository's main. Contributor forks remain valid. No static allowlist or second-human role is implied |
| Local-artifact authority job | The generated workflow | Records `GITHUB_SHA` on main and checks the dispatch rules above, source, version and notes, the latest exact-source GitHub Actions main-push results for `release state`, `codeflow gates`, Rust, Windows, secret scan and security review, and write-visible host collisions. Its write-scoped token can see draft releases. It fails closed on a wrong tag, source or public release, a foreign draft, or draft assets. It then creates or resumes only the exact source-bound empty draft |
| Global-artifact recheck | The generated workflow | Rechecks main after platform builds. Failed or cancelled guards block hosting and announcing |
| Upload and announce | cargo-dist | Uploads without `--clobber`, so a later host conflict is never overwritten, and announces last |
| Post-announce verification | cargo-dist's verifier | Compares tag and source, and every asset name, size and SHA-256 digest, with the same-run files |

- The authority job narrows the small scheduler and API race before hosting,
  but cannot close it atomically. A changed main fails the run and needs a
  deliberate redispatch.
- Its concurrency group covers the authority job only, not the whole generated
  workflow. Operate one deliberate publication at a time. If runs overlap, the
  no-clobber and partial-attempt checks remain the safety boundary.

### codeflow's own releases

- The version source of truth is the reviewed impact annotations next to the
  entries in the one undated pending CHANGELOG section.
- The cumulative target is the latest verified public version, bumped once by
  the highest remaining pending impact.
- `Cargo.toml [workspace.package] version` and the lock and scaffold stamps
  must match it.
- Conventional markers are conservative mismatch tripwires. They do not
  calculate a second version.
- cargo-dist is the only tag, release and artifact publisher. It builds four
  target binaries plus shell and PowerShell installers. Its generated workflow
  runs only by explicit human dispatch on `main`.
- The generated workflow uses the repository's scoped `GITHUB_TOKEN` and
  provisions no personal access token (PAT) or publication credential. Hosted settings can still
  prevent exact-source checks, workflow dispatch, drafts, uploads or releases.
  Treat a zero-step or permission failure as absent evidence and repair the
  repository setting. Never bypass the source and publication guards.

Release impact rules for each PR:

- The PR carries one `Release impact` section with `Impact`, `Breaking`,
  `Rationale`, `Migration`, `Unit` and `Evidence`.
- `Breaking: yes` holds if and only if `Impact: major`. A break needs
  substantive migration guidance.
- A nonbreaking refinement of a pending major entry still carries its
  migration reference.
- A field left at the template's alternatives fails.
- A `Contract` field is still accepted. When it appears with `Breaking`, the
  two must agree.
- Withdrawing a pending entry before release removes that entry and its impact
  marker. The PR body explains why the remaining net contract permits the lower
  target.
- Use a plain `revert:` only when the resulting change has no shipped release
  impact. A revert that changes supported behavior or a public contract uses
  the `fix:`, `feat:` or breaking marker that describes the resulting release,
  with matching PR impact and curated pending notes.

## Technical

Work through each section below that your release touches.

### Cross-build toolchain

Release CI uses native cargo-dist runners for macOS, Linux and Windows, so each
binary links against its platform SDK and can be exercised there. For an
earlier host-agnostic target lint and build check, the repository also
provides Cargo aliases.

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

- `cargo-xwin` acquires the Windows CRT and SDK inputs needed to lint and
  build MSVC targets from macOS or Linux.
- `cargo-zigbuild` uses Zig as the linker for a Linux GNU binary with a glibc
  2.17 floor.
- macOS artifacts still build on macOS, because Apple SDK redistribution and
  licensing prevent a generic bundled cross toolchain.
- A cross-build proves compilation and linking only. It never replaces a native
  Windows, Linux or macOS test and installer canary.

### Presentation renderer assets

The `cf-present` browser distribution is a release input and is never built at
install time. Use the exact Node and npm versions declared in
`crates/codeflow-present/web/package.json`, and run these from that directory.

```sh
npm ci
npm run supply-chain
npm run check
npm run check:browser
```

| Command | What it proves |
|---|---|
| `npm run supply-chain` | Refreshes the committed audit, CycloneDX software bill of materials (SBOM), and license inventory |
| `npm run check` | Proves two clean builds are byte-identical and enforces the raw, Brotli and export budgets and the integrity manifest |
| `npm run check:browser` | Exercises the representative accessible renderer and mode and review behavior in a task-owned browser, including a dense bounded multi-diagram corpus and the long-task envelope |

Review the generated diff. Never hand-edit the distribution or its evidence
files. Release builds consume only the committed assets, and consumer machines
do not need Node.

Repeat the runtime journey on every claimed native platform. Cross-compilation
is useful adapter-shape evidence but does not satisfy these native cases.

| Platform | Native evidence the journey must cover |
|---|---|
| Windows | Unicode known-folder and profile paths, creation-time access control list (ACL) hardening, read-only rejection of a weakened owner or of a protected discretionary access control list (DACL), trustee or inheritance state, trusted system tools, exact quoted command-line identity, file URLs, and process-tree cleanup |
| Linux and WSL2 | Bounded no-follow `/proc` identity and process-group cleanup |
| macOS | Its equivalent ownership boundary |
| Every platform | Every browser route, auxiliary tool route and other external child excludes provider-secret environment canaries through the shared restricted environment |

Measure the stripped release binary against the recorded pre-presentation
reference build. Record the embedded service and export payload contribution
using the procedure captured for the release, and enforce the per-payload and
combined limits in `codeflow_present::limits`. A debug binary, cross-build or
compressed archive size is not equivalent evidence.

### Portal ownership migration

Architecture decision record ADR-0058 and spec SPC-008 define the bundled
portal's ownership lifecycle. The migration breaks managed portal update
behavior only. General scaffold merging and Markdown authority do not change,
and a successful build implies no publishing.

| Existing state | Safe next step |
|---|---|
| No adoption | Remain unchanged; adopt explicitly only when useful |
| Managed, unchanged runtime | Setup or update installs the coherent release and migrates state |
| Missing managed runtime file | Managed setup or update repairs it |
| Local runtime edits or incoming collision | Preserve the work; choose a supported customization, a reviewed restoration or an explicit transfer |
| Unknown or changed baseline content from an earlier portal version | Stop and preserve; resolve journal recovery and inspect before manual recovery |
| Transferred runtime | The project owns maintenance; setup and update preserve edits and intentional deletions |

Transfer rules:

- `codeflow portal transfer --confirm` operates on the whole adopted runtime.
  It does not install the new embedded release, repair missing runtime or
  configuration, or waive integrity checks.
- It freezes the release actually adopted, which may differ from the release in
  the current binary.
- For a genuine fork after transfer, update the actual generator and its state
  declaration together. Its evidence must still pass
  `codeflow validate --portal <dir>`.
- A green verifier does not attest visual quality, security, accessibility or
  runtime provenance.

Recovery rules:

- The normal portal command recovers any pending journal from an earlier portal
  version under its lease, before full adoption parsing and migration.
- Never delete journal, stage or lease data to make an error disappear.
- If a baseline-integrity failure remains, the new migration or transfer did
  not publish. Earlier journaled recovery may have completed, and you report it
  separately.
- Preserve and inspect the questionable content and the repository history.
- Once pending recovery is settled, an explicit reviewed operation may move a
  verified preserved copy outside the reserved baseline directory or restore
  exact reviewed bytes. Keep a restore path, and never normalize line endings
  or delete unknown files as a shortcut. Retry the intended command afterwards.
- Absent baselines are benign. They do not waive managed runtime-drift checks.

Portal release evidence covers:

- fresh minimal, standard and full consumers
- brownfield and repeated updates
- preservation of project-owned bytes
- agreement between the generator and its state declaration
- locked builds and real browser journeys before and after transfer
- current dependency and secret scans
- measured unpacked, archive and release-binary size changes

Keep native macOS, Linux and Windows execution claims separate from
cross-target type checking. Missing native platform or installer evidence stays
explicit and blocks claiming that platform's release qualification.

### Policy key upgrade order

`codeflow update` adds each new `.codeflow/policy.json` key to an existing
consumer with its default. The git-hook shims run whichever `codeflow` is on
`PATH`, and a binary older than the key rejects the policy file, so every
commit fails at `commit-msg`. A release that adds a policy key tells users in
its notes to upgrade the `codeflow` on `PATH` first and then run
`codeflow update`.

| Policy file | Hook binary | Result |
|---|---|---|
| Without the new key | older | passes, rule not checked |
| Without the new key | newer | rule checked at its default |
| With the new key | newer | rule checked as set |
| With the new key | older | every commit blocked: unknown key |

To recover from the last row, upgrade the binary on `PATH`. Never delete the
key to make the hook pass.

### Same-PR preparation and deliberate publication

1. Add curated notes and adjacent impact markers to the undated pending version
   section in the normal work PR. Run
   `python3 scripts/release.py sync --repository sathyassn/codeflow`. It
   discovers public state read-only, calculates from the latest verified public
   release, and updates coupled stamps through Cargo and `codeflow update` only
   when needed. It never pushes, opens a PR, tags or publishes.
   Read-scoped PR and main checks cannot see GitHub draft releases and do not
   claim to. The publisher guards check draft absence later with write scope,
   while behaving read-only.
2. Refresh against the current target before merge. The human merger requires
   the fresh check described in the Merge row of Architecture.
3. When the evidence is complete, a human dispatches cargo-dist's generated
   Release workflow as the Dispatch row describes.
4. The local-artifact authority job and the global-artifact recheck run as
   their Architecture rows describe.
5. cargo-dist uploads, announces and verifies as the last two Architecture rows
   describe.

### Public version baseline

A baseline advances automatically only from a stable public release with an
exact-source marker and asset digests. A stable-looking prerelease, a draft, or
a tag without a verified public release is never a baseline.

Until 3.0.0 is published, the baseline is the bounded v2.1.0 bootstrap recorded
in `.release/config.json`. It keeps v2.1.0's public source and tag mismatch as
recorded facts.

| Fact | Value |
|---|---|
| Live `v2.1.0` tag, the immutable comparison point | `d70c6f17d4bf199a545c843856b3ded4681aff20` |
| Commit identified by the published `source.tar.gz` | `3c3efdb91009361e18b0fabad699b5e875d4e4dd` |
| SHA-256 of that `source.tar.gz` | `1501e0d81716dadd3aa4dc1c56348dd7321abd9cdca90b8f5deb89ea20d54beb` |

- The release target and the published source agree with each other and
  differ from the tag. Never move the `v2.1.0` tag.
- The bootstrap accepts the already-staged 3.0.0 pending section.
- Once 3.0.0 is published, that verified public release replaces the bootstrap
  record as the automatic baseline.

### When publication stops

Never move or delete the tag of a stopped publication, and never overwrite its
assets.

| State after the stop | Next step |
|---|---|
| No tag and no release | Reverify current main and redispatch |
| Exact empty draft, or exact tag-only attempt | Resume, only for the same source and notes |
| Draft with assets, mismatched draft, or tag with another source | Explicit recovery |

- A public version is spent forever. No tag or version is repurposed.
- Material work and withdrawals stay blocked while an attempt is unresolved.

### Re-verification before tagging

Before tagging, re-verify these claims against the currently installed harness
versions. These surfaces move fast, and the versions pinned in ADR-0008,
ADR-0013, ADR-0014 and the parity section of
[the harness posture](harness-posture.md) decay.

| Surface | What must still hold |
|---|---|
| PreToolUse payload contract | The `git-guard` and `exec-guard` contract still matches what Claude Code and an interactive Codex session send |
| Codex hooks and permission profile | The Codex `hooks.json` events still fire as documented, and the `cf-guard` permission-profile keys in `.codex/config.toml` still validate. Run `codex --strict-config doctor` from a checkout with the shipped `.codex/config.toml` in place; `--strict-config` errors on any field the installed Codex no longer recognizes |
| Claude settings and hook schema | `.claude/settings.json` still matches what the installed Claude Code expects |
| Duo contract and interactive lanes | The host-neutral duo contract test passes. Both native interactive lanes complete a scoped canary with the task's required Model Context Protocol (MCP) tools: Claude Code to Codex through the enabled official plugin or qualified native fallback, and Codex to Claude through task-scoped Herdr (tmux degraded) with the qualified tracked Stop/StopFailure lifecycle. Record versions, effort, exact commands, observed tool access and graceful degradation. Auth status output never replaces a working interactive session |
| Ensemble and model bindings | The current ensemble record, selectors and effort and worker policy name only bindings qualified for this release. Every `capability-supported` harness catalog entry still proves the full capability contract, and catalog support is never taken as concrete binding qualification. Any changed concrete binding has an approved full native result, not only a diagnostic pack. `codeflow doctor --check model-bindings` passes for each retained local promotion record, or the exact non-probeable native canary needed is recorded; requested and observed identity, harness version and declared settings drift are resolved. The repository's project selection is absent or empty, or resolves atomically to exact stable-role binding IDs; a diagnostic pack or parseable harness name is not promotion evidence |
| CodeQL | Before the repository is public, no committed CodeQL workflow has entered the portable scaffold and the CodeQL state stays pending. After it is public, enable GitHub CodeQL default setup for Rust with `security-extended`, and confirm tool status shows the intended files analyzed with zero extraction or configuration errors. Treat it as advisory until five consecutive applicable PR runs are healthy, then decide separately whether branch protection should require it. Roll back branch-protection requirements before disabling the setup |
| Coverage | `cargo llvm-cov --workspace --summary-only --fail-under-lines 90` passes locally. CI billing or availability never substitutes for this evidence |
| Distribution plan and installers | `cargo dist plan --output-format=json` lists all four archives, both installers and the native runner rows. Canary the shell installer on each macOS and Linux architecture and the PowerShell installer on Windows. Confirm WSL2 selects the Linux archive and native Windows installs `codeflow.exe` |

Record new verification in a current ADR or release note, and update
[the harness posture](harness-posture.md) if parity drifted. Historical ADR
bodies stay append-only.

### Versioning in a project that consumes CodeFlow

CodeFlow gives your repository the groundwork for clean releases. The
commit-msg gate enforces Conventional Commits, so your history supports SemVer
derivation. CodeFlow does **not** scaffold a release pipeline. Release, version
and changelog tooling is stack-specific and stays yours to choose. Because your
commits are already conventional, each of these tools gets clean input.

| Situation | Reasonable choice |
|---|---|
| Rust crate, published to crates.io | `release-plz` (the standard) or `cargo-release` |
| Rust binaries, not published | `git-cliff` + `cargo-dist`, or an equivalent reviewed policy |
| Any language, PR-based automation | `release-please` |
| Changelog only | `git-cliff` or `conventional-changelog` |

This split is deliberate. Release tooling is as stack-specific as a test
runner, so CodeFlow records standards and enforces commit hygiene and leaves
the choice of release tool to each consumer.
