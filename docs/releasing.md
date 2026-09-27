# Releasing codeflow

How a codeflow release is cut, and how a project that *consumes* codeflow should
think about its own versioning. Decision record: ADR-0062 (supersedes the
release-state portions of ADR-0012 and ADR-0061).
Use [the release checklist](release-checklist.md) as the evidence-bearing
approval record for every run of this procedure.

## codeflow's own releases

Version source of truth: the reviewed impact annotations adjacent to entries in
the one undated pending CHANGELOG section. The cumulative target is the latest
verified public version bumped once by the highest remaining pending impact.
`Cargo.toml [workspace.package] version` and the lock/scaffold stamps must
match. Conventional markers are conservative mismatch tripwires, not another
calculator.

One publisher remains:

- **cargo-dist** builds four target binaries plus shell and PowerShell
  installers and is the only tag, release, and artifact publisher. Its
  generated workflow runs only by explicit human dispatch on `main`.

PRs carry one `Release impact` section with `Impact`, `Breaking`,
`Rationale`, `Migration`, `Unit` and `Evidence`. `Breaking: yes` holds if and
only if `Impact: major`, and a break needs substantive migration guidance.
An edit of a pending major entry is assessed at major, so it declares the
break and keeps its migration guidance, and a field left at the template's
alternatives fails.
The legacy `Contract` field is accepted during the transition and must agree
with `Breaking` when both appear. `scripts/release.py check-pr` compares
the declaration with the current target, actual proposed merge tree, pending
annotations, coupled stamps, and conventional-marker floor. It checks known
contradictions and watched contracts; it does not infer compatibility. Put one
`codeflow:release-impact patch|minor|major` HTML marker directly before
each new pending entry. A withdrawal removes the affected entry/marker and
explains in the PR body why the remaining net contract permits the lower target.

### Pending entries, local checks and repairs

A pending entry is identified by its bold label (`- **Label.** text`), which
is unique among pending entries; a duplicate or an unlabelled entry blocks.
The bounded legacy group keeps its explicit identity (`legacy:pre-policy-v3`).
`check-pr` matches entries by label: a new label is an addition, a missing
label is a withdrawal (it needs the `Withdrawal` field), and a changed body
or impact under a kept label is an edit of that item. An entry is the whole
bullet as Markdown renders it, including unindented lines that continue its
paragraph. An edit is assessed at its impact like an addition, whatever the
declaration: the checker cannot prove that changed words keep their meaning,
so only rewrapping, which keeps every word, the label and the impact, is not
an edit. Moving an entry under another heading is not an edit either.
Lowering an entry's impact needs `Withdrawal`, and a renamed label is a
withdrawal plus an addition. Notes outside entries, such as the upgrade steps,
carry no impact and are judged in review.

Three planes check release state, from cheapest to authoritative:

| Plane | What runs | Blocks |
|---|---|---|
| Pre-push | `release.py preflight` for each pushed branch | only a push that breaks a release tree its base kept valid |
| `codeflow integrate` | `release.py check-state --structural` in the test stage | an invalid tree |
| CI, `codeflow-release.yml` | `release impact` (full `check-pr`) on pull requests; `release state` on pushes to `main` and `integration/**` | yes |

The first two run only in a project whose `.codeflow/project.toml` sets
`release.backend = "codeflow"` and that carries `scripts/release.py`. They
judge the tree against the recorded bootstrap and the local stable tags and
say "not checked against the host"; every local stable tag counts as
published, so a pending version never reuses one. The preflight compares the
branch with its pull request target (a task branch's `integration_target`,
else `main`) and warns when that range touches behaviour paths with no
pending entry added or edited and no `Impact: none` in the local PR draft
named by `CODEFLOW_PR_DRAFT`. A work-in-progress push is never blocked by a
missing entry. On a base whose release tree is already invalid the
preflight only warns, so it does not catch every new break there; the pull
request job does. Behaviour is every path outside `docs/`,
`project-management/` and the record templates, with the skill trees always
included; the table is `crates/codeflow-core/src/workgraph/path_sets.toml`,
which `codeflow ci` reads for the adopter-facing set as well.

**Typed repair.** When the base fails its own release state and the
proposed merge passes, `check-pr` accepts a PR that changes only
`CHANGELOG.md` and the version stamps of the coupled files (`Cargo.toml`,
`Cargo.lock`, `.codeflow/project.toml`, `.codeflow/manifest.json`,
`AGENTS.md`, `CLAUDE.md`, the managed baselines of the last two and their
manifest hashes, which `sync` writes together). The configuration comes from
the base, so a repair that changes `.release/config.json` is refused; the
output names the invariant repaired. A repair keeps every existing pending
entry's words and impact (rewrapping aside); an edit waits for its own PR.
When a repair touches a managed baseline or the manifest, each baseline must
carry the one managed stamp of the release version and the manifest must
record its exact hash. Published sections are held to their exact public
source, and version non-reuse and the impact floors still apply. Any other
PR onto a broken base is refused until the repair lands. The checker a PR
runs is the one in its own merge tree, so a line whose checker predates the
typed repair cannot take a green repair PR; see the TSK-106 review record
for how that line was repaired.

**Errata.** Published sections stay byte-frozen. A correction is a dated
note in the `## Errata` block before the first version section:
`- YYYY-MM-DD, X.Y.Z: note`, naming a published version.

Use a plain `revert:` only when the resulting change has no shipped release
impact. A revert that changes supported behavior or a public contract must use
the `fix:`, `feat:`, or breaking marker that describes the resulting release,
with matching PR impact and curated pending notes.

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

### Policy key upgrade order

`codeflow update` adds each new `.codeflow/policy.json` key to an existing
consumer with its default. The git-hook shims run whichever `codeflow` is on
`PATH`, and a binary older than the key rejects the policy file, so every
commit fails at `commit-msg`. A release that adds a policy key says in its
notes: upgrade the `codeflow` on `PATH` first, then run `codeflow update`.

| Policy file | Hook binary | Result |
|---|---|---|
| Without the new key | older | passes, rule not checked |
| Without the new key | newer | rule checked at its default |
| With the new key | newer | rule checked as set |
| With the new key | older | every commit blocked: unknown key |

Recovery from the last row is to upgrade the binary on `PATH`; do not delete
the key to make the hook pass.

### Same-PR preparation and deliberate publication

1. Add curated notes and adjacent impact markers to the normal work PR's
   undated pending version section. Run
   `python3 scripts/release.py sync --repository sathyassn/codeflow`. It
   discovers public state read-only, calculates from the latest verified public
   release, and updates coupled stamps through Cargo and `codeflow update`
   only when needed. It never pushes, opens a PR, tags, or publishes.
   Read-scoped PR/main checks cannot see GitHub draft releases and do not claim
   that they can; draft absence is checked later inside the write-scoped,
   read-only-in-behavior publisher guards.
2. Refresh against the current target before merge. PR CI checks the actual
   proposed merge tree, not conflict absence. Main-push and integration-line
   CI repeat the state check without writing. These release jobs live in
   `codeflow-release.yml`, outside the managed `codeflow-ci.yml` that every
   adopter receives; no scaffold installs them. Without strict branch protection a stale clean merge
   remains possible, so the human merger must require the fresh check.
3. Before the tag, render the notes from the final assembled source with
   `python3 scripts/release.py release-notes --ref <source> --source <source>
   --tag vX.Y.Z --output notes.md` and read them twice: as a new user (what
   the release does) and as a user upgrading from the last release (what
   to do, in order). Record both reads in the release checklist.
4. When evidence is complete, a human with current write, maintain, or admin
   permission explicitly dispatches cargo-dist's generated Release workflow
   with `--ref main` and the `vX.Y.Z` tag. The actor and rerunning actor
   must both be GitHub Users with effective permission. `GITHUB_SHA` must
   still equal current main and be the result of an ordinary PR human-merged
   into this repository's main. Contributor forks remain valid. No static
   allowlist or second-human role is implied.
5. The supported local-artifact job checks source/version/notes, the latest
   exact-source GitHub Actions main-push results of `codeflow-ci` and
   `codeflow-release` (`publication_workflows`) for `release state`, `codeflow gates`,
   Rust, Windows, secret scan, and security review, plus write-visible host collisions,
   then creates or resumes only an exact empty draft.
   The supported global-artifact job rechecks main after platform builds.
   Failed or cancelled guards block host and announce. This narrows but cannot
   atomically close the small scheduler/API race before hosting; changed main
   fails and requires deliberate redispatch. Its concurrency group covers the
   authority job, not the whole generated workflow, so operate one deliberate
   publication at a time; no-clobber and partial-attempt checks remain the
   safety boundary if runs overlap.
6. cargo-dist uploads without `--clobber` and announces last. Its
   post-announce verifier compares tag/source and every asset name, size, and
   SHA-256 digest to the same-run files.

The bootstrap preserves v2.1's historical public-source/tag mismatch as two
facts. Later baselines advance automatically only from a stable public release
with an exact-source marker and asset digests. A stable-looking prerelease,
draft, or tag without a verified public release is not a baseline.

If publication stops, never move/delete its tag or overwrite assets. No tag or
release means reverify current main and redispatch. An exact empty draft or
exact tag-only attempt may resume. A draft with assets, mismatched draft, or tag
with another source needs explicit recovery. A public version is spent forever.
Material work and withdrawals remain blocked while an attempt is unresolved.

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
    Claude Code → Codex through the enabled official plugin or qualified native
    fallback, and Codex → Claude through task-scoped Herdr (tmux degraded)
    with the qualified tracked Stop/StopFailure lifecycle.
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

The generated release workflow uses the repository's scoped `GITHUB_TOKEN`; it
does not provision a PAT or publication credential. Hosted settings can still
prevent exact-source checks, workflow dispatch, drafts, uploads, or releases.
Treat a zero-step or permission failure as absent evidence and repair the
repository setting—never bypass the source and publication guards.

### Historical bridge into v3

The live `v2.1.0` tag remains at
`d70c6f17d4bf199a545c843856b3ded4681aff20` and is the immutable historical
comparison point. The published `source.tar.gz` independently identifies
commit `3c3efdb91009361e18b0fabad699b5e875d4e4dd` and has SHA-256
`1501e0d81716dadd3aa4dc1c56348dd7321abd9cdca90b8f5deb89ea20d54beb`.
The release target and published source agree with each other, not with the
current tag. The bootstrap records all three facts, does not move the tag, and
accepts the already-staged `3.0.0` pending section. Those commit ids belong to
the original repository. The public repository's history is a path-filtered
copy, so its `v2.1.0` tag has a different commit id with the same tree
`c00d62202df12d8aab2caf4ff90a81491f294a8f`. The check therefore requires the
tag to resolve to that tree, the host tag to match the local tag, and the
public release to carry `source.tar.gz` with the recorded SHA-256. A tag moved
to other content still fails. After that version is
published, the verified public release—not this bootstrap record—becomes the
automatic baseline.

## Versioning in a project that consumes codeflow

codeflow gives your repo the *substrate* for clean releases — the commit-msg gate
enforces Conventional Commits, so your history is SemVer-derivable — but it does
**not** scaffold a release pipeline. Release/version/changelog tooling is
stack-specific and stays yours to choose. Because your commits are already
conventional, any of these has clean input:

| Situation | Reasonable choice |
|---|---|
| Rust crate, published to crates.io | `release-plz` (the standard) or `cargo-release` |
| Rust binaries, not published | `git-cliff` + `cargo-dist`, or an equivalent reviewed policy |
| Any language, PR-based automation | `release-please` |
| Changelog only | `git-cliff` or `conventional-changelog` |

codeflow's job is the discipline; the release mechanism is yours. This split is
deliberate — release tooling is as stack-specific as a test runner, so codeflow
records standards and enforces commit hygiene rather than prescribing one
release tool for every consumer.
