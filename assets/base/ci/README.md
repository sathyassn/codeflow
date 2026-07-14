# CI templates — one binary, thin wrappers

The verification checks live in the **`codeflow` binary**, not in the CI files.
`codeflow ci` verifies a commit range and branch name against
`.codeflow/policy.json` — commit format, no-AI-attribution, no-emoji,
breaking-change footer, branch naming, and (when a PR/MR body is provided)
the PR-body structure — reusing the exact same functions the
git-client hooks and the Claude git-guard use. That makes the binary the
**single source of truth**: the CI plane can no longer drift from the hooks the
way inline shell regex did (ADR-0017).

Every file in this directory is therefore a **thin wrapper**: install the
`codeflow` binary, then run

```text
codeflow ci && codeflow test --strict && codeflow validate --docs
```

Each wrapper only differs in how it discovers the commit range and branch, which
it reads from that platform's CI variables (`codeflow ci` auto-detects them):

| File | Platform | Range source (verified variables) |
|---|---|---|
| `codeflow-ci.yml` | GitHub Actions | `github.event.pull_request.base.sha` / `.head.sha` / `.head.ref`, body via `CODEFLOW_PR_BODY` |
| `.gitlab-ci.yml` | GitLab CI | `CI_MERGE_REQUEST_DIFF_BASE_SHA`, `CI_COMMIT_SHA`, `CI_MERGE_REQUEST_SOURCE_BRANCH_NAME`, body via `CI_MERGE_REQUEST_DESCRIPTION` |
| `bitbucket-pipelines.yml` | Bitbucket Pipelines | `BITBUCKET_PR_DESTINATION_COMMIT`, `BITBUCKET_COMMIT`, `BITBUCKET_BRANCH` (no PR-body variable — body scan skipped) |
| `ci-generic.sh` | anything (pre-receive hook, Makefile, other CI) | `$1 $2` args, or `BASE`/`HEAD` env, or auto-detect; body via `CODEFLOW_PR_BODY` |

On a host `codeflow ci` does not recognize, the range fallback (when no
explicit base/head is given) is: `CODEFLOW_DEFAULT_BRANCH` (export it to name
the base branch), then the policy's `git.protected_branches` tried in order
(`origin/main`, `main`, `origin/master`, `master` by default). When no base
resolves, the commit checks are skipped with a warning and `codeflow ci` exits
non-zero — pass `--base`/`--head` explicitly to fix the setup.

## What `codeflow init` scaffolds

`codeflow init` scaffolds the **GitHub** workflow (`codeflow-ci.yml`) today. The
other files here are **available to copy in** for GitLab, Bitbucket, or a generic
runner — copy the one you need to your repo root (`.gitlab-ci.yml`,
`bitbucket-pipelines.yml`) or your scripts dir (`ci-generic.sh`).

> **Follow-up:** a `codeflow init` platform picker (scaffold the right wrapper
> per host) and wiring the alt templates into `scaffold-manifest.toml` are
> deferred — for now they ship as available files only.

## The Install-codeflow placeholder

Each wrapper's install step is a **PLACEHOLDER**. Until you replace it with the
release installer, the gate **fails RED** — a missing binary is an unarmed
perimeter, not a pass. Do not "fix" the red by skipping when the binary is
absent; that hands branch protection a job that ran nothing.

## Two planes, deliberately

`codeflow ci` is the **portable, host-agnostic** verification plane. The
separate **remote branch-protection** plane (`codeflow remote`) stays
host-API-specific because branch protection is configured through each host's
API — see ADR-0017.

## Optional external add-ons

The GitHub workflow also runs two pinned external tools; add them to any wrapper
as extra steps when your stack warrants:

- **gitleaks** — secret scan: `gitleaks detect --source . --redact --no-banner --exit-code 1`
- **osv-scanner** — dependency/supply-chain audit: `osv-scanner scan -r .`
