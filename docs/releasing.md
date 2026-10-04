# Releasing CodeFlow

## Concept

**A CodeFlow release is one deliberate decision by a named human, taken after
the evidence for it already exists.**

Humans merge the normal work PR that carries the release state and dispatch
the publishing workflow, and no agent merges, tags or publishes.

This runbook covers two things:

- how a CodeFlow release is cut
- how a project that consumes CodeFlow can version its own releases

[The release checklist](release-checklist.md) is the approval record for each
run, with a link or pasted output for every item. A green job, model agreement
or peer approval counts there as evidence. It is never the named human release
decision.

## Architecture

Each release stage has one actor and one gate, and no later stage can
reinterpret an earlier decision.

- Architecture decision record ADR-0062 (release state lives in the normal
  work PR) is the one operative record for release state.
- It superseded ADR-0061 (a maintained candidate PR), which superseded
  ADR-0012 (git-cliff calculates the version). Read those two as history,
  never as current procedure.

The stages, in order, with the actor for each:

```text
 PR author        human          human            workflow       cargo-dist
 work PR   --->   merge   --->   dispatch  --->   authority  --> upload,
 + notes                         dry run, then    job, builds,   announce,
                                 tag              recheck        verify
```

| Stage | Who acts | Gate it must satisfy |
|---|---|---|
| Pending notes and impact | The author of the normal work PR | One `Release impact` section per PR, and one `codeflow:release-impact none\|patch\|minor\|major` HTML marker directly before each new pending entry |
| Release-state check | `scripts/release.py check-pr` | Compares the declaration with the current target, the actual proposed merge tree, pending annotations, coupled stamps and the conventional-marker floor |
| Merge | A human | PR CI checks the actual proposed merge tree, and the human merger requires the fresh check |
| Dispatch | A human with current write, maintain or admin permission | A dry run passes, then a dispatch with the `vX.Y.Z` tag |
| Local-artifact authority job | The generated workflow | Checks the dispatch rules, source, version, notes and prior results, then creates or resumes an empty draft |
| Global-artifact recheck | The generated workflow | Rechecks main after platform builds. Failed or cancelled guards block hosting and announcing |
| Upload and announce | cargo-dist | Uploads without `--clobber`, so a later host conflict is never overwritten, and announces last |
| Post-announce verification | cargo-dist's verifier | Compares tag and source, and every asset name, size and SHA-256 digest, with the same-run files |

### Stage details

Release-state check (`scripts/release.py check-pr`):

- It checks known contradictions and watched contracts. It does not infer
  compatibility.
- It refuses a pending section the publication could not carry. Two cases
  qualify:
  - The section would push the plan job output past 1 MB. That output carries
    the section twice, and GitHub counts it in UTF-16.
  - The notes exceed GitHub's release body limit even with each entry cut to
    its label.
- The main-push state check and `codeflow integrate` apply the same refusal.

Merge:

- PR CI checks the actual proposed merge tree, which is more than the absence
  of conflicts.
- Main-push and integration-line CI repeat the state check without writing.
- These release jobs live in `codeflow-release.yml`, outside the managed
  `codeflow-ci.yml` that every adopter receives. No scaffold installs them.
- Without strict branch protection a stale clean merge is still possible, so
  the human merger must require the fresh check.

Dispatch:

- First dispatch with `--ref main` and `tag=dry-run`. It builds every artifact
  and publishes nothing. Wait for it, and for current main's push runs of
  `codeflow-ci` and `codeflow-release`, to pass.
- Then dispatch with `--ref main` and the `vX.Y.Z` tag.
- The actor and any rerunning actor must both be GitHub Users with effective
  permission.
- `GITHUB_SHA` must still equal current main and be the result of an ordinary
  PR human-merged into this repository's main.
- Contributor forks remain valid. No static allowlist or second-human role is
  implied.

Local-artifact authority job:

- It records `GITHUB_SHA` on main and checks the dispatch rules above, the
  source, the version and the notes. The notes are the curated section, or each
  entry cut to its label with a link to the full section when the section
  exceeds GitHub's 125,000-character release body.
- It checks the latest exact-source GitHub Actions main-push results of
  `codeflow-ci` and `codeflow-release` (`publication_workflows`) for
  `release state`, `codeflow gates`, `windows`, secret scan and security review.
  - `codeflow gates` is the one verdict over the gate's parallel parts.
  - `windows` is the one verdict over the Windows checks, test partitions and
    journeys.
- It checks write-visible host collisions. Its write-scoped token can see draft
  releases.
- It fails closed on a wrong tag, source or public release, a foreign draft, or
  draft assets.
- It then creates or resumes only the exact source-bound empty draft.

Global-artifact recheck:

- First it reads every platform archive. It refuses one whose binary does not
  identify as `X.Y.Z source=<release commit> dirty=false` in
  `codeflow --version`. This check also runs on a dry run.

Concurrency:

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
  target binaries: macOS arm64 and x86-64, Linux x86-64 and Windows x86-64. It
  also builds shell and PowerShell installers. 3.0.0 published no Windows
  target.
- Its generated workflow runs only by explicit human dispatch on `main`.
- The generated workflow uses the repository's scoped `GITHUB_TOKEN`. It
  provisions no personal access token (PAT) or publication credential.
- Hosted settings can still prevent exact-source checks, workflow dispatch,
  drafts, uploads or releases. Treat a zero-step or permission failure as absent
  evidence and repair the repository setting. Never bypass the source and
  publication guards.

Release impact rules for each PR:

- The PR carries one `Release impact` section with `Impact`, `Breaking`,
  `Rationale`, `Migration`, `Unit` and `Evidence`.
- `Breaking: yes` holds if and only if `Impact: major`, and the checkers
  enforce both directions. A break needs substantive migration guidance.
- An edit of a pending major entry is assessed at major, so it declares the
  break and keeps its migration guidance.
- A field left at the template's alternatives fails.
- `Breaking` replaces the older three-state `Contract` field. The checkers
  accept the legacy `Contract` field during the transition:
  - With `Breaking`, the two must agree.
  - Alone, `not-applicable` and `compatible` mean `Breaking: no`, and
    `breaking` means `Breaking: yes`. `Migration` is then needed only for a
    break.
  - `codeflow ci` and `release.py` read it alike.
- `check-pr` reads the PR body with the `codeflow` binary its caller names
  (`--codeflow-bin`, or `CODEFLOW_BIN`). This is the same reader `codeflow ci`
  uses, and `check-pr` never takes one from `PATH`.
  - CI builds the binary from the checked-out tree, so CI's reader is current.
  - The pre-push preflight passes the `codeflow` running the hook, so its
    reader is as current as the installed `codeflow` enforcing the push.
  - The answer carries a protocol version, and `release.py` refuses a binary
    that answers another. A binary answering the same version is trusted to
    read with its semantics.
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

Release CI uses native cargo-dist runners for macOS, Linux and Windows. Each
binary links against its platform SDK and can be exercised there. For an
earlier host-agnostic target lint and build check, the repository also provides
Cargo aliases.

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
| `npm run check:browser` | Exercises the renderer's accessibility, display modes, saved appearance, static export and review behavior (selection, comments and submit) in a task-owned browser whose network stays on loopback |

Review the generated diff. Never hand-edit the distribution or its evidence
files. Release builds consume only the committed assets, and consumer machines
do not need Node.

Repeat the runtime journey on every claimed native platform. Cross-compilation
is useful adapter-shape evidence but does not satisfy these native cases.

| Platform | Native evidence the journey must cover |
|---|---|
| Windows | Unicode known-folder and profile paths |
| Windows | Creation-time access control list (ACL) hardening |
| Windows | Read-only rejection of a weakened owner, or of a protected discretionary access control list (DACL), trustee or inheritance state |
| Windows | Trusted system tools, exact quoted command-line identity, file URLs, and process-tree cleanup |
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
  exact reviewed bytes. Keep a restore path.
- Never normalize line endings or delete unknown files as a shortcut. Retry the
  intended command afterwards.
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
`PATH`. A binary older than the key rejects the policy file, so every commit
fails at `commit-msg`.

A release that adds a policy key tells users in its notes to:

1. Upgrade the `codeflow` on `PATH`.
2. Run `codeflow update`.

| Policy file | Hook binary | Result |
|---|---|---|
| Without the new key | older | passes, rule not checked |
| Without the new key | newer | rule checked at its default |
| With the new key | newer | rule checked as set |
| With the new key | older | every commit blocked: unknown key |

To recover from the last row, upgrade the binary on `PATH`. Never delete the
key to make the hook pass.

### Pending entries, local checks and repairs

`check-pr` matches pending entries by label.

| Case | How `check-pr` treats it |
|---|---|
| Identity | A pending entry is identified by its bold label (`- **Label.** text`), which is unique among pending entries. A duplicate or an unlabelled entry blocks |
| Legacy group | The bounded legacy group keeps its explicit identity (`legacy:pre-policy-v3`) |
| Extent | An entry is the whole bullet as Markdown renders it, including unindented lines that continue its paragraph |
| Comparison | The checker cannot prove that a change keeps an entry's meaning, so entries compare byte for byte and a rewrap is an edit too. In code and nested Markdown, whitespace carries meaning |
| A new label | An addition |
| A missing label | A withdrawal, which needs the `Withdrawal` field |
| A changed body or impact under a kept label | An edit of that item, assessed at its impact like an addition, whatever the declaration |
| The entry moves under another heading with its bytes unchanged | Not an edit |
| A lowered impact | Needs `Withdrawal` |
| A renamed label | A withdrawal plus an addition |
| A note outside entries, such as the upgrade steps | Carries no impact and is judged in review |

Three planes check release state, from cheapest to authoritative:

| Plane | What runs | Blocks |
|---|---|---|
| Pre-push | `release.py preflight` for each pushed branch | only a push that breaks a release tree its base kept valid |
| `codeflow integrate` | `release.py check-state --structural` in the test stage | an invalid tree |
| CI, `codeflow-release.yml` | `release impact` (full `check-pr`) on pull requests; `release state` on pushes to `main` and `integration/**` | yes |

The first two planes are the local planes:

| Local plane rule | Detail |
|---|---|
| Where they run | Only in a project whose `.codeflow/project.toml` sets `release.backend = "codeflow"` and that carries `scripts/release.py` |
| What they judge against | The recorded bootstrap and the local stable tags. They say "not checked against the host" |
| Published tags | Every local stable tag counts as published, so a pending version never reuses one |
| Preflight range | The branch against its pull request target: a task branch's `integration_target`, else `main` |
| Missing entry warning | The preflight warns when that range touches behavior paths with no pending entry added or edited and no `Impact: none` in the local PR draft named by `CODEFLOW_PR_DRAFT` |
| Work in progress | A missing entry never blocks a work-in-progress push |
| Base already invalid | The preflight only warns, so it does not catch every new break there. The pull request job does |
| Behavior paths | Every path outside `docs/`, `project-management/` and the record templates, with the skill trees always included |
| Path table | `crates/codeflow-core/src/workgraph/path_sets.toml`, which `codeflow ci` also reads for the adopter-facing set |

**Typed repair.** When the base fails its own release state and the proposed
merge passes, `check-pr` accepts a PR that changes only `CHANGELOG.md` and the
version stamps of the coupled files. Any other PR onto a broken base is
refused until the repair lands.

| Typed repair rule | Detail |
|---|---|
| Coupled files | `Cargo.toml`, `Cargo.lock`, `.codeflow/project.toml`, `.codeflow/manifest.json`, `AGENTS.md`, `CLAUDE.md`, and the managed baselines of the last two with their manifest hashes, which `sync` writes together |
| Configuration | Comes from the base, so a repair that changes `.release/config.json` is refused. The output names the invariant repaired |
| Pending entries | A repair keeps every existing pending entry byte for byte. An edit waits for its own PR |
| Baselines and manifest | When a repair touches a managed baseline or the manifest, each baseline carries the one managed stamp of the release version and the manifest records its exact hash |
| Still enforced | Published sections are held to their exact public source. Version non-reuse and the impact floors still apply |
| Authority for history | The base is always judged by the configuration it carries. A PR never supplies the authority for the history it is judged against |
| Older shape | One older shape is read: a bootstrap record without its comparison tree, as `main` carries |
| Older shape tree | Its tree is derived from the recorded comparison commit when the tag carries the same tree, and the output says so |
| Other configuration | Any other configuration the checker cannot read refuses the PR |
| Checker version | A PR runs the checker in its own merge tree, so a line whose checker predates the typed repair cannot take a green repair PR. The TSK-106 review record replays that case |

**Errata.** Published sections stay byte-frozen. A correction is a dated
note in the `## Errata` block before the first version section, written as
`- YYYY-MM-DD, X.Y.Z: note` and naming a published version.

### Integration after an epic-line landing

The repository's `codeflow-release-integration.yml` merges verified epic lines
into the release branch. It reports its result after the landing, and task
pull requests do not depend on it.

| Aspect | Rule |
|---|---|
| Trigger | Runs after a `codeflow-release` push run completes on an epic line, and daily at 03:17 UTC |
| Source of the job's YAML | The `workflow_run` trigger loads the privileged job's YAML from the default branch, including for an older line without the integration job. See [GitHub's workflow_run contract](https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_run) |
| Source of the job's code | The job checks out that event's default-branch commit and builds its runner and judge there |
| Triggering line | Passed through an environment variable for reporting. Its workflow, artifacts and scripts do not define or run the privileged job |
| Release branch | `RELEASE_BRANCH` in the integration job selects an existing branch that matches the default target's release pattern |
| Lines checked | Every run checks all verified epic lines, not only the triggering line |
| Replaced pending job | GitHub can replace a pending concurrency job. The surviving run catches up those landings too, and running jobs are not cancelled. See [GitHub's concurrency rules](https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency) |
| Checks before the push | The runner verifies each epic line with the core line check and makes clean merges in a disposable clone. It runs `codeflow ci` under R-120 and the reading-structure checks on the combined result before one normal push |
| Failure | A batch with any failure pushes nothing |
| Reading checks | Reading sizes remain guidelines. Structural reading faults block integration |
| Inspection | Select the run with `gh run list --workflow codeflow-release-integration.yml` and inspect it with `gh run view <run-id> --log-failed`. For a replaced pending run, inspect the later surviving run's result for the line tip |
| Failure owner | A failure names the open task carrying `role: release-integration` at the destination's fetched default-branch commit, even when the release branch predates its assignment, or reports that none owns it |
| Resolution | TSK-010 resolves conflicts and findings in the release pull request. Automation never resolves conflicts or changes a task's status |
| Prerequisites | The integration workflow must first land on the default branch. Per-landing notifications need the epic line's existing read-only `codeflow-release` push workflow |
| Line without that workflow | The next surviving integration run or the daily backstop includes it |
| Upstream result | Upstream completion is a notification and never an approval. The runner checks the current tips itself even when the notifying run failed |

To reproduce the current integration locally without pushing, put the current
`codeflow` binary on PATH and run:

```sh
cargo run -p codeflow-cli --example release_integration -- --release integration/release-3-0-0-r3
```

| Option or limit | Effect |
|---|---|
| No `--line` | Reproduces the workflow's complete catch-up |
| `--line integration/EPC-NNN-slug` | Narrows a local investigation to one line; use it only for that |
| `--push` | Passed by the workflow alone; the runner's default is a check |
| No configured release argument and no repository workflow | Reports no configured integration and does nothing |
| Adopters | The runner and workflow are repository-owned and absent from adopter scaffolds |
| Proof | Hosted authentication and scheduling still need a live workflow run; fixture tests prove the local merge, judge, failure and push paths |

#### Rotating the release branch

When the next release uses a new branch, a maintainer coordinates these steps.
Changing the YAML alone does not verify the rotation.

| Step | What the maintainer does |
|---|---|
| 1 | Disable `codeflow-release-integration.yml` in Actions and let running jobs finish or cancel them. Confirm no running or pending integration job remains. An already queued job still carries its old `RELEASE_BRANCH` value |
| 2 | Create the next release branch through the reviewed release process and check that the default target's `git.release_branch_pattern` covers it. Land the planning record for exactly one open task with `role: release-integration` and that branch as its `integration_target` on the default branch. Keep the preceding release's completed task as history and use the new release's task |
| 3 | In one reviewed PR to the default branch, change the integration workflow's `RELEASE_BRANCH` and this runbook's reproduction command to that same branch. Keep the shared concurrency group. Confirm the branch exists at the remote and the open owner record agrees with it before enabling automation |
| 4 | From the updated default checkout, build the runner and current `codeflow`, put that binary on PATH, and run the reproduction command without `--push` or `--line`. Resolve any finding in the release PR, then repeat the preview |
| 5 | Re-enable the integration workflow. Inspect the next eligible completion or daily run, verify its reported release branch, owner and integrated line tips, and record that run in the release checklist |

### Same-PR preparation and deliberate publication

1. Add curated notes and adjacent impact markers to the undated pending version
   section in the normal work PR. Then run
   `python3 scripts/release.py sync --repository sathyassn/codeflow`.
   - It discovers public state read-only and calculates from the latest
     verified public release.
   - It updates coupled stamps through Cargo and `codeflow update` only when
     needed.
   - It never pushes, opens a PR, tags or publishes.
   - Read-scoped PR and main checks cannot see GitHub draft releases and do not
     claim to. The publisher guards check draft absence later with write scope,
     while behaving read-only.
2. Refresh against the current target before merge. The human merger requires
   the fresh check described in the Merge row of Architecture.
3. Before the tag, render the notes from the final assembled source:

   ```sh
   python3 scripts/release.py release-notes --ref <source> --source <source> \
     --tag vX.Y.Z --repository sathyassn/codeflow --output notes.md
   ```

   Read them twice, as a new user (what the release does) and as a user
   upgrading from the last release (what to do, in order). Record both reads in
   the release checklist.
4. When the evidence is complete, a human dispatches cargo-dist's generated
   Release workflow as the Dispatch row describes.
5. The authority job, the recheck, and cargo-dist's upload, announcement and
   verification run as the remaining Architecture rows describe.

### Public version baseline

A baseline advances automatically only from a stable public release with an
exact-source marker and asset digests. A stable-looking prerelease, a draft, or
a tag without a verified public release is never a baseline.

3.0.0 is published, but `.release/config.json` still records the bounded
v2.1.0 bootstrap as historical provenance. The automatic baseline is the
highest verified public release, 3.0.0. It keeps v2.1.0's public source and tag
mismatch as recorded facts.

| Fact | Value |
|---|---|
| Live `v2.1.0` tag, the immutable comparison point | `d70c6f17d4bf199a545c843856b3ded4681aff20` |
| Commit identified by the published `source.tar.gz` | `3c3efdb91009361e18b0fabad699b5e875d4e4dd` |
| SHA-256 of that `source.tar.gz` | `1501e0d81716dadd3aa4dc1c56348dd7321abd9cdca90b8f5deb89ea20d54beb` |
| Tree of the `v2.1.0` tag in both repositories | `c00d62202df12d8aab2caf4ff90a81491f294a8f` |

- The release target and the published source agree with each other and
  differ from the tag. Never move the `v2.1.0` tag.
- The commit ids above belong to the original repository. The public
  repository's history is a path-filtered copy, so its `v2.1.0` tag has a
  different commit id with the same tree.
- The check therefore requires the tag to resolve to that tree, the host tag
  to match the local tag, and the public release to carry `source.tar.gz` with
  the recorded SHA-256. A tag moved to other content still fails.
- The bootstrap accepts the already-staged 3.0.0 pending section.
- The verified public 3.0.0 release replaces the bootstrap record as the
  automatic baseline.

### When publication stops

Never move or delete the tag of a stopped publication, and never overwrite its
assets.

| State after the stop | Next step |
|---|---|
| No tag and no release | Reverify current main and redispatch |
| Exact empty draft, or exact tag-only attempt | Resume, only for the same source and notes |
| Draft with assets, or a mismatched draft, and no tag | Delete the draft as described below, then reverify current main and redispatch |
| Public release whose post-announce verification failed | Rerun only the failed job in the same run, which still holds the built files; never edit the release or its tag |
| Tag with another source | Leave the tag as it is; the operator decides the recovery |

A draft is not public and has no tag yet, so it can be deleted:

1. Confirm `gh api repos/sathyassn/codeflow/git/ref/tags/vX.Y.Z` answers 404.
2. Delete the draft with `gh release delete vX.Y.Z --yes`.
3. Reverify current main and redispatch.

- A public version is spent forever. No tag or version is repurposed.
- Material work and withdrawals stay blocked while an attempt is unresolved.

### Critical issues

The issue-handling reference
(`.agents/skills/cf-method/references/issue-handling.md`) defines a critical
defect and the order of responses for any project. This section gives
CodeFlow's own routes, so the release step is not argued again for each
issue. The operator set them on 2026-10-04.

- **Marking:** the primary applies the `critical` label at intake and names
  the criterion that holds. The bug report template asks the reporter for
  the same criteria.
- **Interim guidance:** a workaround that is true now, tested before it is
  posted, goes in the issue the same day, and in the release plan when
  adopters are affected. It needs no release and changes no guard.
- **Prioritized fix:** the fix moves ahead of planned work in its own epic
  on the primary's call. Moving another epic's planned work, or work the
  operator ordered, is the operator's call.
- **Release:** CodeFlow publishes only from the current tip of `main`, so
  every pending change rides with the fix. The critical fix lands first and
  `main` stays green and releasable. Issue 48 is handled this way: the
  3.0.0 managed secret scan covers every branch, so one branch's finding
  turns an adopter's pull requests red, and its fix is planned for 3.1.0
  rather than a 3.0.1 patch.
- **Adopters told:** the issue comment, the `CHANGELOG.md` entry with its
  `Migration` line and the release notes. A security defect goes through
  [private vulnerability reporting](SECURITY.md), never a public issue.

There is no maintenance branch, such as a `release/3.0` line for a 3.0.1
patch. Adding one would change these guards and costs:

| What | Where | What a maintenance branch needs |
|---|---|---|
| The dispatch must run on `main` | `.github/workflows/release-plan-authority.yml:41`; `scripts/release.py:1839` | accept a second ref |
| The source must be the current `main` tip | `.github/workflows/release-plan-authority.yml:43`; `scripts/release.py:1822` | accept a source that is not `main` |
| The source must be a PR merged into `main` | `scripts/release.py:1860` | accept a merge into the branch |
| One pending section, bumped once from the published baseline | `scripts/release.py:817`, `scripts/release.py:830` | a second live `CHANGELOG.md` section and target |
| Branch protection | repository settings | rules for the new branch |
| Every fix | each pull request | landed twice, once per line |

The first three rows are publication guards. Loosening them is a
security-relevant change and an operator decision, taken with this cost
stated; it is never a step taken under pressure.

### Re-verification before tagging

Before tagging, re-verify these claims against the currently installed harness
versions. These surfaces move fast, and the versions pinned in ADR-0008,
ADR-0013, ADR-0014 and the parity section of
[the harness posture](harness-posture.md) decay.

| Surface | What must still hold |
|---|---|
| PreToolUse payload contract | The `git-guard` and `exec-guard` contract still matches what Claude Code and an interactive Codex session send |
| Codex hooks | The Codex `hooks.json` events still fire as documented |
| Codex permission profile | The `cf-guard` permission-profile keys in `.codex/config.toml` still validate |
| Codex strict config | Run `codex --strict-config doctor` from a checkout with the shipped `.codex/config.toml` in place. `--strict-config` errors on any field the installed Codex no longer recognizes |
| Claude settings and hook schema | `.claude/settings.json` still matches what the installed Claude Code expects |
| Duo contract and interactive lanes | The host-neutral duo contract test passes. Both native interactive lanes complete a scoped canary with the task's required Model Context Protocol (MCP) tools: Claude Code to Codex through the interactive Codex CLI in a Herdr tab (the plugin only as a recorded fallback), and Codex to Claude through task-scoped Herdr (tmux as the last fallback) with the qualified tracked Stop/StopFailure lifecycle, as `cf-model-orchestrator/resources/routing/transport.md` states. Record versions, effort, exact commands, observed tool access and graceful degradation. Auth status output never replaces a working interactive session |
| Ensemble and model bindings | The current ensemble record, selectors and effort and worker policy name only bindings qualified for this release. Every `capability-supported` harness catalog entry still proves the full capability contract, and catalog support is never taken as concrete binding qualification. Any changed concrete binding has an approved full native result, not only a diagnostic pack. `codeflow doctor --check model-bindings` passes for each retained local promotion record, or the exact non-probeable native canary needed is recorded; requested and observed identity, harness version and declared settings drift are resolved. The repository's project selection is absent or empty, or resolves atomically to exact stable-role binding IDs; a diagnostic pack or parseable harness name is not promotion evidence |
| CodeQL | Before the repository is public, no committed CodeQL workflow has entered the portable scaffold and the CodeQL state stays pending. After it is public, enable GitHub CodeQL default setup for Rust with `security-extended`, and confirm tool status shows the intended files analyzed with zero extraction or configuration errors. Treat it as advisory until five consecutive applicable PR runs are healthy, then decide separately whether branch protection should require it. Roll back branch-protection requirements before disabling the setup |
| Coverage | The one instrumented run, `cargo llvm-cov nextest --workspace --no-fail-fast --fail-under-lines 90 --profile codeflow` (the `rust-coverage` target of the full gate), passes locally. CI billing or availability never substitutes for this evidence |
| Distribution plan | `cargo dist plan --output-format=json` lists all four archives, both installers and the native runner rows |
| Source archive | `source.tar.gz` comes from `git archive`, so it leaves out `docs/verification/` (`export-ignore` in `.gitattributes`). A contract test compares the real archive with the tracked files |
| Installers | Canary the shell installer on each macOS and Linux architecture and the PowerShell installer on Windows. Confirm WSL2 selects the Linux archive and native Windows installs `codeflow.exe` |

Record new verification in a current ADR or release note, and update
[the harness posture](harness-posture.md) if parity drifted. Historical ADR
bodies stay append-only.

### Ceremony check before a release

Before the tag, run the ceremony report over the release's window, from a
clone that has fetched every epic line:

```sh
codeflow report ceremony --since <the previous release's date>
```

- Compare it with the recorded baseline in
  `docs/verification/ceremony-baseline-2026-09-28.md` (pull requests per
  logical change, the record status count, review rounds and refusals).
- Paste the output with a one-line comparison into the release checklist.
- The report informs the release decision and never blocks it.

| Report field | When it prints `unknown` |
|---|---|
| Review rounds | The host cannot answer or holds no review |
| Refusals | Any part of the window before this clone began recording them |

Neither value is estimated.

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

Release tooling is as stack-specific as a test runner. CodeFlow records
standards and enforces commit hygiene, and each consumer chooses the release
tool.
