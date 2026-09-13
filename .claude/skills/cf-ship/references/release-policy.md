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

For a new, single stable-SemVer product, a practical starting design is reviewed
change intent plus curated Unreleased notes, one version calculator, a maintained
release-candidate PR, and human-approved exact-source publication:

```text
change PR -> impact review/check -> merge
                                     |
                          refresh one candidate PR
                                     |
                         verify + human approval
                                     |
                        publish exact source/assets
                                     |
                         separate consumer upgrade
```

Choose a maintained stack-appropriate tool after verifying its fit: an existing
Changesets package graph remains Changesets; a compatible Release Please setup
may own release PRs and versions; git-cliff may calculate from reviewed commits
where the project adopts that input. These are alternatives, not layers to run
together. Check the installed version's behavior, private-registry requirements,
workspace support and hosting permissions before promising automation.

Complete adoption includes the actual commands/workflow, an owner, version and
note sources, and disposable-fixture tests for ordinary, breaking, no-release
and repeated runs. Verify how automatic PR creation triggers checks on the exact
candidate revision with the chosen credential. Unknown hosting permissions are
a setup prerequisite, not a reason to pretend a prose policy is enforced.
Prefer a small extension to the existing tool over a custom release framework.

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
create a competing input. A useful release-impact explanation names the unit,
impact, evidence-based rationale, changelog entry and migration where needed.
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

Candidate preparation preserves curated meaning and touches only coupled
version fields. It distinguishes proposed/source builds from published releases,
handles no-change and reverted work under the adopted calculator, and converges
on one candidate on refresh. Preserve or flag manual candidate edits rather
than silently discarding them. Recompute and re-review when the source changes.

Before publication, verify the authorized source identity, candidate versions,
notes/migration, package contents, checksums and required platform evidence
together. A moving branch, tag-shaped string, green syntax check, old review or
ordinary merged PR is not release authorization. Revalidate the exact source
that will be published using the project's supported merge strategy. Keep
untrusted PR code/text away from publication credentials and shell interpolation.

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
