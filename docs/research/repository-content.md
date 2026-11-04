# What belongs in a repository

The question for TSK-1030 (issue #279): what should a repository under
CodeFlow hold long term, what should stay out, and where should the evidence
of a change live instead. The answer sets the instruction text TSK-1030
ships in 3.2.0 and the content classes TSK-428's gate later enforces.

Researched 2026-10-27 at line tip `3114b4529`. Sources were read on that
date.

## Findings

The sources below share a working test, though none states it as a universal
law: a repository holds what people review, build from and need again, and
leaves out what a tool can regenerate on demand or what only one run needed.
The exceptions are deliberate: a project may version an output when its
consumers take it straight from the repository, as a GitHub JavaScript action
does with its bundled `dist/index.js`. So the line runs between disposable
output and an output the project chooses to version, not between generated and
hand-written files.

Revised 2026-10-28 after review: the first version stated two practices as
universal (review records and generated code); both rows are now qualified,
and a distribution-artifact row and the project shapes below were added.

| Class | Current practice | Rule for CodeFlow |
|---|---|---|
| Build and cache output | Every stack template ignores it: `dist`, `build/`, `target/`, `__pycache__/`, `.venv`, tool caches ([github/gitignore][gi], [Node][gi-node], [Python][gi-py]). OpenSSF Scorecard rates committed binaries a high risk because they cannot be reviewed and drift from source ([Scorecard checks][scorecard]). | Stays out. |
| Run output and per-run logs | `*.log`, coverage reports and test output are ignored by the same templates; CI keeps them as workflow artifacts, deleted after 90 days by default ([GitHub retention][gh-retention], [artifacts][gh-artifacts]). | Stays out. The PR body carries a summary; the full output stays in the run's own home. |
| Review records and verdict files | None of the sources here recommends committing per-round review verdicts; GitHub keeps review on the pull request. Some teams keep an audit or decision log in the repository by policy, which is an authority record the project chose, not a per-run file. CodeFlow already says the review verdict lives on the PR (`cf-method/references/delivery-process.md:255`). | Per-round verdict files stay out; a record the project keeps as authority belongs. |
| Lockfiles | Committed: npm `package-lock.json` "is intended to be committed" ([npm][npm-lock]); `uv.lock` "should be checked into version control" ([uv][uv]); Terraform's lock file belongs in version control ([Terraform][tf-lock]); Cargo now leaves the choice to the project and `cargo new` stops ignoring `Cargo.lock` for libraries ([Cargo FAQ][cargo-faq], [Rust blog][rust-lock]). | Belongs, as the stack's tools and the project decide. |
| Generated code | Go marks generated files with `// Code generated ... DO NOT EDIT.` and `go build` never runs `go generate`, which makes committing the output the usual Go choice, though the docs do not prescribe it ([go generate][go-gen]). Protobuf stubs and API clients are committed in some projects and generated in the build in others. | Belongs when the project needs it checked in, marked as generated; otherwise it is build output. |
| Distribution artifacts | A GitHub JavaScript action bundles its code with rollup or ncc into `dist/index.js` and commits it, because the runner executes the repository as checked out; the same guide says not to commit `node_modules` ([GitHub action guide][gh-js-action]). | Belongs when it is the shipped product consumers take from the repository; an admitted use outranks the build-output exclusion. |
| Vendored code | Go vendoring copies what the build needs and verifies it against `go.mod` ([Go modules][go-mod]); GitHub recommends a package manager over storing dependencies in the repository ([GitHub large files][gh-large]). The adopter on #279 kept runtime vendoring through its dependency rule and banned copies kept for reference (comment of 2026-10-27 22:46Z). | One rule: a copy a runtime or build needs is a dependency and joins through the project's dependency rule; a copy kept for reference stays out. |
| Fixtures and snapshots | Committed and reviewed as part of the test: "all snapshot files should be committed alongside the modules they are covering" ([Jest][jest]). | Belongs, as test content. |
| Large binaries | GitHub warns at 50 MiB, blocks 100 MiB and recommends repositories stay under 1 GB; large product files go to Git LFS or release assets ([GitHub large files][gh-large], [Git LFS][lfs]). pre-commit's `check-added-large-files` refuses additions over 500 kB by default ([pre-commit-hooks][pre-commit]). | A product asset belongs in its smallest form; size alone is a review question, and a size check is TSK-428's warn tier. |
| Secrets | Deleting a committed secret does not remove it from history; rotate it first ([GitHub sensitive data][gh-secrets]). | Already covered: the managed `.gitignore` and the git floor's secret scan. No change here. |
| Research and decision records | CodeFlow keeps research under `docs/research/`, one file per question, and decisions as ADRs (`cf-method/references/project-organization.md:173`). | Belongs: authority content. A superseded draft stays out; history keeps it. |
| Evidence for a change | Pasted summaries in the PR body; full output in CI artifacts with a retention limit ([GitHub retention][gh-retention]). Git history keeps every removed file, so a clean-up loses nothing a commit pin cannot recover. | The PR body or the task record. A file only when a live record cites it and the body cannot carry it, in summary form. |

## Project shapes

The same file class can fall on either side depending on what the project
ships.

| Project shape | Kept that a generic rule might refuse | Still stays out |
|---|---|---|
| Application or service | lockfile; migrations; generated clients the team edits against | build output, logs, coverage |
| Library | lockfile when the project chooses it (Cargo, uv) | build output, test run output |
| GitHub JavaScript action | bundled `dist/index.js` | `node_modules`, run logs |
| Infrastructure (Terraform, OpenTofu) | `.terraform.lock.hcl`, policy fixtures | `.terraform/`, plan output, state |
| Docs site or portal | sources and figure data | the rendered site when a pipeline builds it |
| Monorepo | each package's admitted content | per-task evidence folders, reference copies |

## CodeFlow guidance and checks this touches

- The one shipped row that names a home for evidence files sends it to a
  per-task folder on protected branches:
  `assets/base/claude/skills/cf-method/references/project-organization.md:670`
  ("Retained comparison evidence | `docs/verification/<task>/` | Yes").
  Repository A on #279 copied it into its own rules and grew 492 files
  (26 MB) in 61 per-task folders on `main`.
- Examples present CodeFlow's own `docs/verification/` as an ordinary
  folder: `cf-present` and `cf-docs-portal`
  `resources/figure-grammar.md:153` and
  `resources/utility-presentation-system.md:29`; `rules/writing.md:287` and
  `:334`; `cf-model-orchestrator/resources/grok-host.md:56`.
- Defaults treat the folder as standard: the docs-portal starter
  `portal.config.json:11` and three `cf-evaluate-model` fixtures exclude
  `docs/verification` from every new portal.
- Nothing asks whether a file belongs. The nearest rules are about code
  (`rules/workflow-discipline.md:145` "Write only what earns its keep"),
  the PR body's evidence with no rule on files
  (`cf-ship/references/pr-evidence.md`), and evidence quality with no home
  (`cf-model-orchestrator/resources/quality/evidence.md`). `cf-reviewer`
  has no question on added files, and its materiality list
  (`rules/workflow-discipline.md:353`) does not cover them.
- Where evidence should go already exists: the PR body, the task record's
  acceptance block, the gate's durable home (`~/.codeflow/gate-runs/`) and
  the task's scratch directory, which "leaves with it"
  (`rules/worktrees.md:134`).
- The managed `.gitignore` block covers secrets, editor files, worktrees and
  portal lock state only. Stack build and cache output is the project's own
  `.gitignore`, which the stack templates above cover.

## This repository

`docs/verification/` holds 806 tracked files (91 MB) in 60 entries. Citing
paths in it: 81 records under `project-management/`, 14 docs outside it, 17
crate sources and tests (among them `manifest_consistency.rs`, which pins
the `cf-present` board path, and `orchestration_contract.rs`, which reads a
Grok canary record there) and the changelog. Removing it needs the citation
pins TSK-428 builds, so its clean-up stays with TSK-428.

## Chosen scope

- The rule has one home, the discipline rules' "Write only what earns its
  keep", which ships at every tier. It states what belongs, what stays out,
  where evidence goes, the evidence-file condition, that history keeps
  removed evidence (cite a removed file at a commit that holds it), and the
  vendoring rule. An admitted use outranks the exclusions, so a generated
  file the project ships, such as an action's bundled `dist/index.js`, is
  not refused as build output.
- `cf-develop` (build), `cf-ship` (PR), `pr-evidence.md` (the body),
  `quality/evidence.md` and the `project-organization.md` row point to it.
  The row names no per-task folder and points to `pr-evidence.md`.
- `cf-reviewer` asks of every added file whether it is needed long term and
  in the smallest form that serves its use, reports the count in its
  verdict, and an unneeded or oversized addition is a finding. The
  materiality list gains that case, so such a finding blocks: the issue's
  cause was that every such file passed review. Oversized means shown
  unnecessary content or avoidable bulk, never a wording or structure
  preference.
- The examples and defaults above stop naming `docs/verification/`: the
  board is named as CodeFlow's own and unshipped, the writing examples take
  durable sources, and the portal starter and fixtures exclude nothing.
- Two `cf-evaluate-model` cases: a builder with run output in hand puts its
  summary in the PR body and commits none of it; a reviewer refuses an
  unneeded file while it keeps fixtures, a lockfile and an action's bundled
  `dist/index.js`.
- Left out, for TSK-428: the gate and its tiers, citation checks, exception
  handling, a managed `.gitignore` entry for run output (a gate class, and a
  stack-neutral ignore would hide files some projects track), and this
  repository's `docs/verification/` clean-up. CodeFlow's managed-copy
  footprint is TSK-429's.

[gi]: https://github.com/github/gitignore
[gi-node]: https://raw.githubusercontent.com/github/gitignore/main/Node.gitignore
[gi-py]: https://raw.githubusercontent.com/github/gitignore/main/Python.gitignore
[scorecard]: https://github.com/ossf/scorecard/blob/main/docs/checks.md
[gh-retention]: https://docs.github.com/en/organizations/managing-organization-settings/configuring-the-retention-period-for-github-actions-artifacts-and-logs-in-your-organization
[gh-artifacts]: https://docs.github.com/en/actions/using-workflows/storing-workflow-data-as-artifacts
[npm-lock]: https://docs.npmjs.com/cli/v10/configuring-npm/package-lock-json
[uv]: https://docs.astral.sh/uv/concepts/projects/layout/
[tf-lock]: https://developer.hashicorp.com/terraform/language/files/dependency-lock
[cargo-faq]: https://doc.rust-lang.org/cargo/faq.html
[rust-lock]: https://blog.rust-lang.org/2023/08/29/committing-lockfiles/
[go-gen]: https://pkg.go.dev/cmd/go#hdr-Generate_Go_files_by_processing_source
[go-mod]: https://go.dev/ref/mod
[gh-large]: https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-large-files-on-github
[gh-js-action]: https://docs.github.com/en/actions/tutorials/create-actions/create-a-javascript-action
[lfs]: https://git-lfs.com/
[jest]: https://jestjs.io/docs/snapshot-testing
[pre-commit]: https://github.com/pre-commit/pre-commit-hooks
[gh-secrets]: https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/removing-sensitive-data-from-a-repository
