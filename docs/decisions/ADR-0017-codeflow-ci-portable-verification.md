---
id: ADR-0017
title: codeflow ci — CI-portable, binary-sourced verification
date: 2026-07-11
status: accepted
superseded_by: null
architecture_impact: docs/architecture.md — the CI enforcement plane stops re-implementing the git standards inline and instead runs the `codeflow ci` binary (one source of truth), and becomes host-portable via thin per-platform wrappers; `main.rs` gains the `ci` subcommand (15 total). Those edits land in this same PR.
---

<!-- ADRs are append-only: written at the moment of decision, never edited
     afterwards except to set superseded_by. -->

# ADR-0017 — codeflow ci: CI-portable, binary-sourced verification

## Context

The git/commit/PR standards (charter §6.4) are checked by three planes that read
`.codeflow/policy.json` through shared Rust functions
(`hooks/standards.rs`, `hooks/policy.rs`): the git-client `commit-msg`/`pre-push`
hooks and the Claude `git-guard`. The fourth plane — CI, the authoritative
perimeter — did NOT: the scaffolded GitHub workflow re-implemented the same
checks as **inline bash** (a `types` regex, `grep -niE` for attribution, a
`grep -P` emoji range) over the PR commit range and the PR body. That is exactly
the duplication AGENTS.md warns about ("the scaffolded CI re-implements the git
standards inline … keep it in step with policy.json"): two encodings of one rule
that drift silently — the bash `types` list is hardcoded, ignores the policy
whitelist and per-rule block/warn/off levels, and never sees `check_breaking_footer`
or branch naming. The CI plane was also **GitHub-only**: the checks were welded
into GitHub Actions YAML, so a GitLab or Bitbucket user got nothing.

## Decision

Add a `codeflow ci` subcommand that IS the CI plane's check, and reduce every CI
file to a thin wrapper that calls it.

`codeflow ci` verifies a commit range and a branch name against
`.codeflow/policy.json`, reusing the exact functions the hooks use — it calls
`git_hook::commit_msg` per non-merge commit (so commit format, breaking-footer,
AI attribution, and emoji are the *same* code the `commit-msg` hook runs),
`GitPolicy::branch_name_ok` for the head branch, and `standards::find_attribution`
/ `find_emoji` for the PR/MR body. Each finding is leveled by the policy field
that governs it (`commit_format` / `branch_naming` / `ai_attribution` /
`commit_emoji`): a Block violation exits non-zero, a Warn is printed and passes,
an Off is skipped. There is no second encoding of any rule — drift is now
structurally impossible, closing the AGENTS.md risk.

The command **auto-detects the commit range** from the CI platform's own
environment, so a wrapper can call it with no arguments: GitLab
(`CI_MERGE_REQUEST_DIFF_BASE_SHA`, `CI_COMMIT_SHA`,
`CI_MERGE_REQUEST_SOURCE_BRANCH_NAME`), Bitbucket
(`BITBUCKET_PR_DESTINATION_COMMIT`, `BITBUCKET_COMMIT`, `BITBUCKET_BRANCH`), and
GitHub (`GITHUB_BASE_REF`/`GITHUB_HEAD_REF`), falling back to
`origin/<default>..HEAD` and stating which base it used. Explicit `--base`/`--head`
/`--branch`/`--pr-body[-file]` override the detection, and `CODEFLOW_PR_BODY`
carries the body via `env:` (never interpolated into a shell line, so an
untrusted PR body cannot inject).

The CI plane becomes **host-portable**: thin wrappers ship under `assets/base/ci/`
for GitHub (`codeflow-ci.yml`, refactored), GitLab (`.gitlab-ci.yml`), Bitbucket
(`bitbucket-pipelines.yml`), and anything else (`ci-generic.sh`, for a
pre-receive hook / Makefile / other runner), each ~10–30 lines that install the
binary then run `codeflow ci && codeflow test && codeflow validate --docs`. This
is deliberately the **portable** plane; the separate remote branch-protection
plane (`codeflow remote`) stays host-API-specific, because configuring branch
protection is inherently a per-host API operation, not a per-checkout command.

## Consequences

- One source of truth: the CI checks can no longer drift from the hooks, and they
  now honor the policy whitelist, the per-rule levels, and the breaking-footer and
  branch-naming rules the inline bash never covered. Fixing a check in
  `standards.rs`/`policy.rs` fixes all four planes at once.
- The CI plane now **depends on the `codeflow` binary being installed** in CI —
  the same dependency the `test`/`validate` gates already have. The install step
  stays a loud PLACEHOLDER that fails RED until wired (a missing binary is an
  unarmed perimeter, not a pass); this is unchanged posture, now extended to the
  commit-standards job.
- Portability is real but the wrappers are **copy-in** today: `codeflow init`
  still scaffolds only the GitHub workflow. An init platform-picker and wiring the
  alt templates into `scaffold-manifest.toml` are a deliberate follow-up — the alt
  files ship as available templates, not managed artifacts, so the
  `manifest_consistency` test (scoped to skills/agents) is unaffected.
- Range auto-detection is best-effort per host: a shallow CI checkout that lacks
  the base commit degrades to a stated fallback and skips the commit walk rather
  than failing opaquely (the wrappers set full-depth to avoid this). CI env var
  names were verified against the GitLab and Bitbucket docs, not assumed.
- CI stays the authoritative perimeter only where a required status check is
  actually enforced before merge (ADR-0006); on this repo that is a human merging
  on green, since remote protection is unavailable (private + GitHub Free).

## Note (2026-09-05)

A CI *job* may restack `codeflow test` / coverage / validate targets already
run in sibling jobs. The perimeter is those checks, not the job name. If the
umbrella job dies from runner loss, OOM, timeout, or billing cutoff with no
assertion result, that is missing job evidence, not an assertion-red of the
checks that already completed green elsewhere. Model consensus still cannot
override a check that ran and failed.

## Architecture impact

`docs/architecture.md` is updated in this PR: the four-planes paragraph now states
the CI plane runs `codeflow ci` (one source of truth, no inline drift, portable
via per-platform wrappers) instead of re-implementing the standards inline, and
the subcommand surface count goes from 14 to 15 with `ci` listed after
`validate`. No new external dependency is introduced (the wrappers reuse the
already-pinned gitleaks/osv-scanner as optional add-ons).
