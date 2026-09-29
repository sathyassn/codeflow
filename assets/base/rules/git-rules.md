# Git rules

The full text behind the git floor in `AGENTS.md`. Managed by
`codeflow update`.

## Enforcement

Four planes provide defense in depth: git hooks, in-session `git-guard` and
`exec-guard`, scaffolded CI, and configured remote branch protection. Hooks
and CI share `.codeflow/policy.json` and the `codeflow ci` checks; remote
setup derives its supported rules from that policy (CodeFlow ADR-0017). Every
tier, minimal included, ships the same armed policy and all five git-hook
shims: **pre-commit** (protected-branch commits, staged secrets),
**commit-msg** (commit format, AI attribution, emoji), **pre-push** (branch
naming, protected-branch push, force-push and delete), and
**pre-merge-commit** and **reference-transaction** (the protected-branch
merge and ref backstops). At the minimal tier the installed and load-bearing
files are `AGENTS.md` and `CLAUDE.md`, `.codeflow/policy.json`, `.gitignore`,
the five hook shims, the scaffolded CI workflow, and the in-session guards
wired in `.claude/settings.json` (`git-guard`, `exec-guard`) and the
`.codex/` starter (for interactive Codex).

Installed files alone do not prove active enforcement: verify hook
execution, harness trust and event support, CI results, and actual remote
rules. Interactive Codex needs the one-time `/hooks` trust; other harnesses
need their qualified hook contract. Local checks provide required fast
feedback but are editable, not an unbypassable security boundary. CI becomes
a merge gate only where the remote requires its result; remote authority
also depends on permissions and bypass settings. Report missing planes
without relaxing task safety or review. Headless task execution remains
prohibited (CodeFlow ADR-0018), independently of whether a particular
harness can run hooks in that mode.

The local planes are fast feedback that an agent on the host can edit or
skip; required CI and remote rules are the server-side boundary, and only
where they are configured and enforced for the actor's permissions. Arm the
remote plane with `codeflow remote protect` where the host supports it, then
verify that it succeeded and read the live rules before claiming that
boundary. An override env (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) is not
authentication and creates no boundary. At the standard and full tiers,
cf-method's "Why the git boundary is remote" gives the full rationale.

A team may deliberately change specific rules in `.codeflow/policy.json`, but
first assess their consumers: weakening commit discipline can break a
commit-driven release process. The consuming project owns its release units,
version calculator, changelog and publication approval; installing CodeFlow
does not install its repository release pipeline or make scaffold metadata
the project's version. Discover that policy before release work; propose
missing automation for adoption rather than silently enabling publication.

## The rules

- **Branches:** `{prefix}/{kebab-name}`. Prefixes: `feat/ fix/ docs/ refactor/
  test/ chore/ ci/ hotfix/ plan/ task/ spike/ experiment/ integration/`.
  Durable implementation uses `task/TSK-NNN-<slug>`; pick the others by work
  intent.
- **Commits:** conventional format `type(scope): description` (scope
  optional), imperative mood, lower-case type from the policy whitelist, no
  trailing period; the description at most 50 chars and the whole subject
  line at most 72. A body, when present, is **only** `-` bullets, at most 3,
  each a single line of at most 72 chars, optionally followed by a
  `BREAKING CHANGE:` footer; no prose paragraphs. One logical change per
  commit. Other git-trailer footers (`Refs:`, `Signed-off-by:` and the like)
  are blocked unless the project opts them in: a team can allow specific
  trailers, require a ticket reference, or require `Signed-off-by` (DCO) via
  `policy.json`.
- **Breaking changes require compatibility judgment, every commit.** Assess
  API, CLI flags, config, formats, defaults and managed instructions against
  their accepted contract. An incompatible change requires `type!:` and a
  `BREAKING CHANGE:` migration footer; a compatible addition does not become
  breaking merely because it touches an interface. Follow the project's one
  release-impact input and version calculator. `breaking_watch_paths` warns
  about touched surfaces, not proven breaks; independent review checks
  meaning.
- **No AI attribution, ever:** no `Co-Authored-By` AI trailers, no
  "Generated with" lines, no robot emoji, in commit messages and PR bodies.
  This is project policy and overrides any harness default that injects
  attribution.
- **No emoji** in commit subjects or PR bodies.
- **Written content policy** (ADR-0067): see `writing.md` in this directory.
- **Secrets:** never stage credentials, API keys, tokens, or `.env` files.
  The pre-commit secret scan (and the CI secret-scan job) block them, and it
  is the one gate never relaxed, not even during bootstrap grace.
- **Durability push:** with a remote configured, push the working branch
  after each committed logical unit so work survives a machine failure; use
  `git push --force-with-lease` (never bare `--force`) after a rewrite. It is
  backup, not a merge: the secret scan and every merge gate still stand.
  Forbid it with `git.force_push_unprotected` in `policy.json` (default
  allow).
- When a gate blocks you, fix the cause; never bypass (`--no-verify`,
  editing hooks, exporting gate tokens). Gates exist only where mistakes are
  irreversible or invisible. A guard's refusal names the policy rule it
  applied and the sanctioned path.

## Protected branches

**Protected branches** (`main`/`master` plus policy globs): never commit,
merge, push, force-push, delete, or hard-reset on them. Work lands by
exactly two paths: a PR with evidenced-green checks merged by a human, or
`codeflow integrate <branch> --into <target>`. Never set override envs
(`CODEFLOW_HUMAN_OVERRIDE`, gate tokens), which is laundering, and never
`gh pr merge --delete-branch` (it can corrupt the root repo).

## Bodies of work

**Bodies of work:** a multi-task epic lands task by task on a
non-protected `integration/<epic>` branch (agents merge there); only the
finished body reaches `main`, via one human-reviewed PR. See cf-method,
"Managing a body of work" (standard and full tiers).

## PR bodies

**PR bodies:** follow the template: five fixed sections, plus conditional
ones when they apply (`cf-ship` owns the format where installed). A PR
names its work with `Task: TSK-NNN` or `Task: none: <reason>`. The Summary
anchors a reader with no context in a few lines: the result, why it
matters and where it stands; a key file name or number belongs there when
it is part of that context, and the details follow as bullets. Write the
body plainly: simple, straightforward and clear, no mannered prose (see
`.codeflow/rules/writing.md`). Match presentation to the shape of the
data: tables for matrices, fenced blocks for pasted output, one-line
bullets for the rest, never paragraph walls. A code PR **must** carry
real test evidence in `## Testing`: pasted test summary, coverage number,
new tests, and what was
NOT tested; "tests pass" as prose is a claim, not evidence. Docs-only PRs
say so in one line plus the doc checks run. A release-impact note agrees
with the authoritative release input; it is not a second calculator.
