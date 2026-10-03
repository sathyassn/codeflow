# CI templates — one binary, thin wrappers

The verification checks live in the **`codeflow` binary**, not in the CI files.
`codeflow ci` verifies a commit range and branch name against
`.codeflow/policy.json` — commit format, no-AI-attribution, no-emoji,
breaking-change footer, branch naming, and PR/MR body structure and release
claims, reusing the exact same functions the
git-client hooks and the Claude git-guard use. That makes the binary the
**single source of truth**: the CI plane can no longer drift from the hooks the
way inline shell regex did (CodeFlow ADR-0017).

Every file in this directory is therefore a **thin wrapper**: install the
`codeflow` binary the target pins, then run

```text
codeflow ci && codeflow test --strict && codeflow validate --docs
```

Each wrapper only differs in how it discovers the commit range and branch, which
it reads from that platform's CI variables (`codeflow ci` auto-detects them):

| File | Platform | Range source (verified variables) |
|---|---|---|
| `codeflow-ci.yml` | GitHub Actions | `github.event.pull_request.base.sha` / `.head.sha` / `.head.ref`, body via `CODEFLOW_PR_BODY` |
| `.gitlab-ci.yml` | GitLab CI | target: `CI_MERGE_REQUEST_TARGET_BRANCH_SHA`, else `CI_MERGE_REQUEST_TARGET_BRANCH_NAME` fetched from the merge request's project; `CI_COMMIT_SHA`, `CI_MERGE_REQUEST_SOURCE_BRANCH_NAME`, body via `CI_MERGE_REQUEST_DESCRIPTION` |
| `bitbucket-pipelines.yml` | Bitbucket Pipelines | target: `BITBUCKET_PR_DESTINATION_COMMIT`, else `BITBUCKET_PR_DESTINATION_BRANCH` fetched from `origin`; `BITBUCKET_COMMIT`, `BITBUCKET_BRANCH` (supply the body via `CODEFLOW_PR_BODY` or `--pr-body-file`) |
| `ci-generic.sh` | anything with a working tree and full history (Makefile, other CI) | the target commit as `$1` (required) and the head as `$2` (default `HEAD`), or `BASE`/`HEAD` env; body via `CODEFLOW_PR_BODY` |

The GitLab, Bitbucket and generic wrappers pass the target and head to
`codeflow ci` explicitly. `ci-generic.sh` refuses to run without the target
commit: it never guesses it, because a pin read from the change itself would
let the change choose the binary that judges it.

## PR body checks

On recognized GitHub PR and GitLab MR events, a missing or empty body fails
under the default `git.pr_sections: block` policy. Local and push runs can omit
it. Bitbucket's [default variables](https://support.atlassian.com/bitbucket-cloud/docs/variables-and-secrets/)
include a PR ID but no description channel. Without `CODEFLOW_PR_BODY`, its
copy-in wrapper warns that body checks were skipped and explains how to supply
one. A project-owned retrieval step can export that variable or pass
`--pr-body-file`. An explicitly empty body still fails the default policy.

The parser recognizes unindented ATX headings outside closed HTML block
containers, keeps nested evidence, and rejects duplicate required sections at
the preferred depth. Inline HTML in prose, such as `Vec<String>`, never hides a
heading. A block container that never closes hides nothing: later headings
still count, and a warning names the unclosed tag. Depth two takes precedence
over same-name depth-three subsections; setext underlines do not end a section.
Fresh installs require Summary, Changes, Reviews and Release impact, plus
Testing for ranges that touch code. Without an explicit list, the built-in
default stays Summary and Changes. Existing section lists and enforcement
levels stay unchanged on update. The PR template
ships at minimal, standard and full tiers through the usual managed-file merge.

Presentation warnings cover Summary length, code spans and paths, missing
`Not tested:`, fences over twelve lines, long prose lines, template remnants,
and roughly 65 wrapped rows at 100 columns (90 for an integration branch into
main). They are advisory and follow `pr_sections`; off/allow disables them.

`git.pr_release_impact` defaults to warn independently. It checks Impact
(none/patch/minor/major), Breaking (yes/no), Rationale and Migration, allowing
extra project fields. Value tokens are case-insensitive. Breaking yes requires
Impact at or above `git.pr_breaking_level` (default major) and substantive
migration guidance. A breaking commit marker requires both Breaking yes and
that impact floor. Pre-1.0 projects declare their own minimum breaking level. No release calculator, changelog, task tracker or language
is assumed. Upgrade the local and CI binaries before committing the new keys
from `codeflow update`: an older binary fails every `codeflow ci` run with exit
2 and `unknown key git.pr_release_impact`.

## What `codeflow init` scaffolds

`codeflow init` scaffolds the **GitHub** workflow (`codeflow-ci.yml`) today. The
other files here are **available to copy in** for GitLab, Bitbucket, or a generic
runner — copy the one you need to your repo root (`.gitlab-ci.yml`,
`bitbucket-pipelines.yml`) or your scripts dir (`ci-generic.sh`).

> **Follow-up:** a `codeflow init` platform picker (scaffold the right wrapper
> per host) and wiring the alt templates into `scaffold-manifest.toml` are
> deferred — for now they ship as available files only.

## The pinned install

`codeflow-ci.yml` and `codeflow-policy.yml` install
the `codeflow` release named by `scaffold_version` in the target branch's
`.codeflow/project.toml`, verified against the release's published
`sha256.sum`. A missing checksum file, a missing entry or a mismatch fails the
job; nothing unverified is installed. The enforcing jobs run on
`pull_request_target`, which takes the workflow from the default branch; they
check out the pull request's base commit, so the target's pin and policy
apply, and read the pull request head only as git data.

The GitLab, Bitbucket and generic wrappers run one shared script (the text
between the `codeflow pinned run` markers is the same in all three). It reads
the pin from the target branch's current commit (on GitLab the merged
results pipeline's `CI_MERGE_REQUEST_TARGET_BRANCH_SHA`, otherwise the target
branch fetched from the merge request's project, never the diff base
`CI_MERGE_REQUEST_DIFF_BASE_SHA`; on Bitbucket
`BITBUCKET_PR_DESTINATION_COMMIT`, otherwise the destination branch fetched
from `origin`, failing when it cannot; or the generic script's first
argument),
installs that release with the same checksum verification, and runs
`codeflow ci` from a checkout of the target, so the target's policy judges
the head as git data; `codeflow test` and `validate --docs` run on the head
with the same binary. When the head raises the pin, the head's release is
installed separately and only tested (`--version`, `validate --docs`). When
the head lowers it, the target's binary still judges the change and the job
then fails; a head that kept the pin it branched from lowers nothing. An upgrade takes two pull requests, in order: raise only
`scaffold_version`, land it, then run `codeflow update`; a head that carries
new policy keys before the raise lands fails with a message naming that
order.

A Bitbucket pull request pipeline "merges the destination branch into your
working branch before it runs"
([pipeline start conditions](https://support.atlassian.com/bitbucket-cloud/docs/pipeline-start-conditions/)),
so there `codeflow test` and `validate --docs` run on that merge, while
`codeflow ci` judges `BITBUCKET_COMMIT`, the source commit, against the
target.

On GitLab and Bitbucket the job file runs from the merge or pull request
itself, as a GitHub `pull_request` workflow does, so a change can edit its
own install step. The pin does not defend that edit: require review of the
CI file and `.codeflow/` in the host's rules.

## Two planes, deliberately

`codeflow ci` is the **portable, host-agnostic** verification plane. The
separate **remote branch-protection** plane (`codeflow remote`) stays
host-API-specific because branch protection is configured through each host's
API — see CodeFlow ADR-0017.

## Optional external add-ons

The GitHub workflow also runs two pinned external tools; add them to any wrapper
as extra steps when your stack warrants:

- **gitleaks** — secret scan: `gitleaks detect --source . --redact --no-banner --exit-code 1`.
  The GitHub workflow reads every exemption from the trusted commit: the
  pull request's base, or the pushed commit on a push. A pull request
  cannot exempt the leak it adds, so a new exemption takes effect once its
  own pull request merges. The step deletes the checkout's
  `.gitleaksignore`, `.gitleaks.toml` and `.gitleaks.json` and gives
  gitleaks the trusted commit's copies, choosing your configuration as
  gitleaks does (`GITLEAKS_CONFIG`, `GITLEAKS_CONFIG_TOML`, a
  `.gitleaks.json` beside `.gitleaks.toml`, `.gitleaks.toml`, else its
  default rules). A file your configuration extends by `[extend] path`, and
  a `GITLEAKS_CONFIG` file in the repository, come from the trusted commit
  too; the step fails when that commit does not hold the file, and refuses
  an absolute path into the checkout. An inline `gitleaks:allow` comment
  counts only on a commit the trusted commit already holds. The step fails
  before it scans when the trusted commit is not in the checkout, and it
  downloads gitleaks under the runner's temp directory. Nothing from the
  checkout runs or steers the scan: its Python helpers run isolated (they
  need Python 3.11 or later), git reads `.gitattributes` from the trusted
  commit (git 2.41 or later), and no step before the scan runs code from
  the checkout, since such a step could set the scan's environment. Add
  any new step to that job after the scan. If you customised the workflow
  before this release and the secret-scan job runs a step of yours before
  the gitleaks step, `codeflow update` keeps it through the merge and warns
  about it on every run: move it after the gitleaks step or into another
  job. Update recognises the scan step by its shipped name, `gitleaks`,
  with `TRUSTED_SHA` in its env; if you renamed it, update cannot check the
  order and says on every run that the job's step order needs your review.
  gitleaks reads the whole history of HEAD, the base's included (on a
  pull request, the pull request merged into its base; on a push, the
  pushed commit), with what each merge adds beyond its automatic result,
  files whose type changes and files git judges binary, which gitleaks'
  default history scan leaves out. That scope is narrower on purpose: a
  branch with no pull request, or a tag, is not scanned by this workflow
  unless its commits become reachable from a scanned HEAD, so a
  repository-wide audit needs a scan of its own. The refusals below check
  only the commits a pull request brings, those HEAD holds and the trusted
  commit does not; on a push that range is empty, since the pushed commit
  is the trusted commit, so a push scan reads its history without them.
  In those commits, git cannot show what an octopus merge adds, so the
  step refuses one; merge the branches one at a time. It also refuses a path with a
  backslash, a double quote or a control character that one of those
  commits changes, or that either side of a merge among them changes,
  since gitleaks cannot read such names reliably. Each refusal names the
  commit and the refs that hold it. Renaming the file in a later commit
  leaves the name in the earlier one, so rewrite the pull request's
  commits, or rebase onto the base when the change is on the base's side.
  Names with spaces or non-ASCII letters pass. The scan pins git's patch
  format, so git configuration on the runner, such as `diff.noprefix`,
  cannot move a finding to another path. It then drops one
  known false positive from the report: the security-stage prose CodeFlow
  3.0.0 seeded on line 209 of `.claude/workflows/pipeline.workflow.js` and
  its baseline copy, which the `generic-api-key` rule mistakes for a key.
  Only a `generic-api-key` finding whose value and matched text are exactly
  that prose, in one of those two paths, is dropped; every other finding
  fails the job, as does a scan that logs an error or reads no commit. A
  wrapper that runs gitleaks on the change's own checkout reads the
  change's own exemptions; read them from the target branch instead. A
  wrapper that runs gitleaks itself on a repository scaffolded by 3.0.0 can
  allow the same prose in its own configuration.
  With no `.gitleaks.toml` yet, create one that keeps the default rules:

  ```toml
  [extend]
  useDefault = true

  [[allowlists]]
  description = "CodeFlow 3.0.0 seeded pipeline workflow: security-stage prose, not a credential"
  condition = "AND"
  paths = ['''^(?:\.codeflow/\.baseline/)?\.claude/workflows/pipeline\.workflow\.js$''']
  regexTarget = "match"
  regexes = ['''^authz gaps, vulnerable[/]malicious $''']
  ```

  With a `.gitleaks.toml` already, add only the `[[allowlists]]` block to
  that file; leave its `[extend]` section as it is.
- **osv-scanner** — dependency/supply-chain audit: `osv-scanner scan -r .`
