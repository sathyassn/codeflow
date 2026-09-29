# Git rules

The full text behind the git floor in `AGENTS.md`. Managed by
`codeflow update`.

## Enforcement

Four planes provide defense in depth: git hooks, the in-session `git-guard`
and `exec-guard`, scaffolded CI, and configured remote branch protection.
Hooks and CI share `.codeflow/policy.json` and the `codeflow ci` checks
(CodeFlow ADR-0017). Every tier ships the same armed policy and all five
git-hook shims: **pre-commit** refuses protected-branch commits, commits at
the root checkout off its root branch, staged secrets and unresolved
conflict markers, **commit-msg** checks the commit format, AI attribution
and emoji, **pre-push** checks branch naming and refuses protected-branch
pushes, force-pushes and deletes, and
**pre-merge-commit** and **reference-transaction** are the protected-branch
merge and ref backstops.

Installed files do not prove active enforcement: verify hook execution,
harness trust and event support (interactive Codex needs the one-time
`/hooks` trust), CI results and the actual remote rules, and report a
missing plane without relaxing task safety or review. The local planes are
fast feedback that an agent on the host can edit or skip; required CI and
remote rules are the server-side boundary, and only where they are
configured for the actor's permissions. Arm the remote plane with
`codeflow remote protect` where the host supports it, then read the live
rules before claiming that boundary. An override env (`CODEFLOW_HUMAN_OVERRIDE`, gate tokens) is not
authentication and creates no boundary. Headless task execution remains
prohibited (CodeFlow ADR-0018). At the standard and full tiers, cf-method's
"Why the git boundary is remote" gives the full rationale.

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
`gh pr merge --delete-branch` (it can corrupt the root repo). The pushed
branch is deleted later, in cleanup after merge proof (`worktrees.md`
"Cleanup").

**Root checkout:** task work happens in a linked worktree; the root
checkout stays on its root branch (`git.root_branch`, by default the
repository's default branch). An agent's commit there on any other branch
is refused; a human at their own terminal is warned. See `worktrees.md`.

## Bodies of work

**Bodies of work:** a multi-task epic lands on a non-protected
`integration/<epic>` branch (agents merge there) in small batches of
reviewed task heads, each batch gated once as one candidate (see
`worktrees.md`, "Parallel work and integration"). Only the finished body
reaches `main`, via one human-reviewed PR that the operator merges. See
cf-method, "Managing a body of work" (standard and full tiers).

## PR bodies

**PR bodies:** follow the template and the sections `codeflow ci` requires
for the PR's class (`cf-ship` owns the format where installed). Every PR
names its work on one `Task:` line. Where durable tracking is active (the
project keeps its work records in `project-management/`), the line names the
task, `Task: TSK-NNN`, or the epic, `Task: EPC-NNN`, for the breakdown PR
and the PR to `main`; where it is not, it names the harness's tracked unit,
any non-empty name. A missing, empty, malformed, repeated or mismatched
`Task:` line is refused, and there is no unrecorded form.

The Summary anchors a reader with no context in a few lines: the result, why
it matters and where it stands; a key file name or number belongs there when
it is part of that context, and the details follow as bullets. Write the
body plainly: simple, straightforward and clear, no mannered prose (see
`.codeflow/rules/writing.md`). Match presentation to the shape of the data:
tables for matrices, fenced blocks for pasted output, one-line bullets for
the rest, never paragraph walls. A code PR **must** carry real test evidence
in `## Testing`: the tested revision, the commands and their pasted results
(targeted tests and the quick gate from the builder), new tests, and what
was NOT tested; "tests pass" as prose is a claim, not evidence. The full
gate on the landing candidate, with its coverage, is linked by the primary
when the batch lands; a standalone PR runs it as its own candidate. The PR
stays draft until its required evidence exists. Docs-only PRs say so in one
line plus the doc checks run. A release-impact note is required on a PR into
a protected branch or one that carries a breaking commit; it agrees with the
authoritative release input and is not a second calculator.
