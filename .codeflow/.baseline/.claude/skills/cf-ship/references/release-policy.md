# Project-owned releases

Read when customizing a project's release process, assessing a change's release
impact, reviewing a release candidate, or preparing publication. Ordinary
shipping is a reviewed code/documentation merge; it is not permission to publish
an artifact or deploy it.

## Establish one authority

Inspect the project's manifests, workflows, published releases, changelog,
maintainer instructions and package boundaries before proposing changes. Reuse
a coherent existing process. CodeFlow does not install its own repository's
release pipeline into consumers, require git-cliff, or own their versions.
`.codeflow/manifest.json` records installed scaffold content, never the
consuming application's version.

Record the following in the project's existing release guide or project-owned
AGENTS section; link it from other entry points instead of copying it:

- Release units and deliberately coupled groups; authoritative version files.
- Compatibility/impact rules, including what is shipped and what counts as
  `none`; the one input and calculator that determine each next version.
- Curated release-note source, migration and security-disclosure rules.
- Candidate preparation and verification commands, exact publication approval,
  identity and credential boundaries, plus retry/recovery behavior.
- Supported prerelease/backport rules, or an explicit unsupported boundary;
  independently authorized installation and deployment.

A package is not automatically an independent release unit. A monorepo may
release all packages together, selected groups together, or each separately.
Keep the existing package graph and tool authoritative; never add a second
Markdown, JSON or database release ledger to mirror it. Calendar versions and
deployment identifiers follow their own policy, not forced SemVer arithmetic.
Skill, schema, evaluation and runtime versions need not change together unless
the project deliberately couples them.

## Offer a complete starting process

If no credible process exists, `cf-customize` proposes one against the actual
project and asks for adoption before installing tooling, changing workflows,
creating settings or enabling remote permissions. An accepted setup request is
not automatic consent to a release policy or publication. A project may decline.

For a new, single stable-SemVer product, offer reviewed change intent, curated
notes and warranted version updates **in the ordinary work PR**. Publication
is a separate deliberate action, not another automatically created PR:

```text
work + impact + notes + version -> review/check -> merge
                                                    |
                                  explicit publication request
                                                    |
                                   verify/publish exact source
                                                    |
                                     separate consumer upgrade
```

Use a maintained stack-appropriate tool where it fits. Existing Changesets or
Release Please release-PR workflows remain valid project choices; do not replace
them merely to match this starter. A commit-driven calculator is another valid
choice when expressly adopted. These are alternatives, not layers. Verify the
installed tool's actual workflow, workspace support and hosting permissions;
never invent dispatch inputs or automation it does not provide.

Complete adoption includes the actual commands/workflow, an owner, version and
note sources, and disposable-fixture tests for ordinary, breaking, no-release,
concurrent, post-publication and repeated runs. The local preparation command
leaves a reviewable diff; CI validates without creating repair commits or PRs.
Where the project intentionally uses bot PRs, verify their check triggers and
notification behavior. Prefer a small extension to existing tooling over a
custom release framework; prose alone is not enforcement.

## Keep pending metadata in the work PR

For the same-PR starter, calculate the next pending version from the **verified
last published version** and highest remaining reviewed pending impact—not the
previous source version or PR count. From published `1.4.0`, two pending fixes
still target `1.4.1`; adding a compatible feature targets `1.5.0`; another fix
keeps `1.5.0`. Once `1.5.0` is actually published, the next fix targets `1.5.1`.
No shipped impact means no manufactured bump or empty version section.

Keep impact beside the curated entries (or in the project's existing input).
An undated `## [X.Y.Z]` section describes source state without claiming
publication. It stays undated in Git after publication: the public release owns
availability and date. Publishing reads the reviewed source; it does not date
the source heading, move Unreleased notes, or create a bookkeeping commit/PR.
State that convention explicitly. Preserve an existing project's different
convention under its own adopted release process.

Before opening/updating the PR, reconcile against the current target, preserve
other pending work, and update only coupled stamps. Recheck the proposed merge
result: even a clean merge can contain a stale version. Integration tasks do
not count twice when their final main PR is assessed. A newly published section
is frozen; stale work must start the next pending section rather than amend it.
Without strict target-freshness enforcement, describe the check-to-merge race
honestly and reject inconsistent state before publication.

Removing or changing a pending entry needs a truthful explanation: a wording
clarification is not a withdrawn feature. A real withdrawal is assessed against
the remaining net contract and may lower an unpublished target. Resolve an
existing tag/draft attempt first. A tag or public release reserves that version
for its original content; abandoning an attempt does not permit repurposing it.
An untagged draft may be resumed unchanged or explicitly abandoned by its owner,
not silently rewritten by a later work PR. These checks
cover actual state, not a second release ledger or a mandatory extra approval
role. The independent reviewer still judges the meaning of the change.

## Keep pending entries checkable

Give each pending entry a stable identity, such as a unique bold label, so a
checker can tell an addition, an edit and a withdrawal apart. An edit under
an existing label is assessed at its impact like an addition, whatever the
PR declares: a checker cannot prove that changed words keep their meaning, so
only rewrapping prose is no edit; where an entry holds code or nested
structure, its whitespace is meaning. The entry is the whole
rendered bullet, continuation lines included. Renaming a label withdraws the
old entry and adds a new one.

Check release state as early as it is cheap, and keep the pull request check
authoritative:

- Before push, warn when behavior paths change with no entry added or edited
  and no declared `none`; block only a push that breaks state its base kept
  valid, so work in progress can still be backed up.
- On an integration line, check the structure of the release state before
  each landing and say what was not checked against the host.
- When the base itself fails its release state, accept only a repair that
  changes the changelog and coupled version stamps, judged by the base's
  configuration, keeps every existing entry's words, and leaves each stamp,
  baseline and recorded hash consistent; refuse other work until it lands.
  A pull request runs the checker in its own merge tree, so ship this repair
  path before the state can break: a checker without it cannot pass a repair.

A published section stays byte-identical. Correct it with a dated erratum
that names the version, never by editing the section. Before the tag, render
the notes from the final source and read them twice: as a new user and as a
user upgrading from the last release, in the order they would act.

Release jobs belong to the project, outside any CI file a tool manages for
its adopters, so an update never installs or removes another project's
release process.

## Assess each change

Judge compatibility against the released contract and actual target-relative
diff, including an integration PR's whole body of work. Commit subjects, file
extensions and coverage scores are not semantic proof. In stable SemVer:

- **Major:** incompatible removal or change to an accepted contract, such as a
  removed CLI flag, changed persisted format, or reversed shipped guarantee.
  Identify affected consumers and a real migration path.
- **Minor:** compatible added behavior or capability.
- **Patch:** compatible defect correction or clarification of shipped behavior.
- **None:** no shipped impact under the project's stated policy, with a reason.

Touching an API, config, default or managed instruction is a reason to assess,
not an automatic major bump. Conversely, a `docs:` label cannot hide a changed
instruction that consumers execute or a doctrine guarantee that they rely on.
An internal refactor with unchanged observable contracts can legitimately be
`none` where the project permits it. Test changes need review, but do not by
themselves prove either a release or an exemption is needed. Pre-1.0 and other
version schemes use the adopted project's explicit rules.

Use the project's required PR declaration, fragments or annotations; never
create a competing input. The PR's Release impact block states the change in
four fields, then the project's own fields (unit, changelog entry, evidence):

- `Impact`: the change level a consumer sees.
- `Breaking`: `yes` or `no`, the plain compatibility statement. It replaces
  the older three-state `Contract` field. On a watched contract path,
  `Breaking: no` is the explicit compatibility claim, so it is never
  prefilled.
- `Rationale`: the consumer-visible effect and the evidence for the level.
- `Migration`: always present. It is normally `none` for nonbreaking work.
  When Breaking is yes, give steps or a pointer to a Breaking change section.
  A PR that refines or reconciles a pending breaking entry carries that
  entry's migration reference; a checker that assesses edits at the entry's
  impact also requires the break to be declared.
- A value is chosen, never left as the template's alternatives.

In stable SemVer, as CodeFlow uses it, Breaking is yes if and only if Impact
is major, and its checker enforces both directions. Pre-1.0 and other
schemes name the level a break takes in their adopted policy. Compare a PR's
declaration with the entries it adds, never with the cumulative pending
version: an additive task declares minor and Breaking no even when earlier
work already made the pending release major.
For a commit-driven calculator, the commits that will actually land must retain
the reviewed markers: a corrected PR title alone is insufficient for merge or
rebase workflows. For a fragment-driven tool, review its authoritative entries.
Assess combined effects without counting task and integration PRs twice.

The independent other-lineage reviewer challenges the classification against
the diff and accepted guarantees. Syntax/path checks can reject missing or
contradictory intent; they cannot discover every semantic break. Resolve a real
ambiguity with evidence or a consequential maintainer decision, not an invented
guarantee of automatic detection.

## Verify and publish deliberately

Preparation preserves curated meaning and touches only coupled version fields.
It distinguishes source builds from published releases, handles no-change and
reverted work under the adopted calculator, and is idempotent on unchanged
input. Preserve or flag manual edits rather than silently discarding them.
Recompute and re-review when the source changes.

Before publication, verify the authorized source identity, candidate versions,
notes/migration, package contents, checksums and required platform evidence
together. A moving branch, tag-shaped string, green syntax check, old review or
ordinary merged PR is not release authorization. Revalidate the exact source
that will be published using the project's supported merge strategy. Keep
untrusted PR code/text away from publication credentials and shell interpolation.
For a multi-platform binary or installer release, keep native Windows and
WSL2/Linux evidence separate: the native Windows installer must select its
Windows binary, while WSL2 uses the Linux installer and binary. Cross-build
success proves compilation and linking only; it never replaces native
macOS/Linux/Windows tests or installer canaries. Missing platform evidence
blocks publication rather than becoming an inferred pass.

Stage assets before declaring success. Retry only the same candidate; compare
existing tag targets and asset hashes, and reject conflicts rather than
overwriting a published version. A partial upload, stale approval or unknown
source fails visibly with the smallest safe recovery action. Never move a
historical tag to make metadata agree: distinguish a Git commit, tree hash,
archive checksum and release timestamp, preserve evidence, and obtain an
explicit baseline disposition when they disagree.

Publication does not install an update into users' systems. Stable bundle users
pin an exact release and digest; following a moving branch is an explicit
preview/contributor choice. Respect the project's independent rollout, backup
and rollback authority.

Reference standards: [Semantic Versioning](https://semver.org/),
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
These define useful conventions, not proof of a project's implemented gates.
