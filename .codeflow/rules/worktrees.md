# Worktrees and branches

The full text behind the start, landing and cleanup rows of the map in
`AGENTS.md`. Managed by `codeflow update`.

## Where work happens

Develop in a worktree per session: a linked worktree on a feature branch,
under `.worktrees/<slug>` (gitignored) or in the folder a harness manages
for its own worktrees. Never use a sibling folder.

The root checkout, the repository's main working tree, stays on its root
branch and takes no task work. The root branch is the repository's default
branch unless `git.root_branch` in `.codeflow/policy.json` names another.
Change it only for an umbrella repository whose root is a working checkout
(workspace mode): its convention is `integration/workspace`, and
`codeflow init --workspace` sets it up.

In an umbrella, a change lands this way:

- The umbrella's own files: a small edit is a commit on
  `integration/workspace` at the root checkout. Larger or parallel
  work uses a short-lived branch in the umbrella's own
  `.worktrees/<slug>`, cut from the root branch and merged back with
  `codeflow integrate`.
- With no remote, the root branch is the landing line and `main` is a
  protected checkpoint: at a milestone the operator moves it forward with
  `codeflow integrate integration/workspace --into main`; agents
  never do. With a remote the same holds, the root branch is pushed, and a
  change into `main` is a pull request a human merges.
- A nested repository: every change goes through that repository's own
  flow, a worktree under its own `.worktrees/<slug>` and a pull request
  into its integration branch or its `main`. The umbrella never commits
  nested files, which it ignores, and agents never merge into any
  repository's `main`.

git-guard refuses an agent's commit at the root checkout on any other branch
(`git.root_checkout_commits`). The git hooks refuse it too when a harness
marks the session, and only warn a human at their own terminal. `codeflow
doctor` reports the root branch, a root checkout off it or holding task
edits, and a linked worktree outside `git.worktree_locations`.

## Work-start check

Before a task's first branch, worktree, commit, merge, rebase, push, or
delete mutation, make three ordered work-start assertions:

1. **IDENTITY:** establish where you actually are: the worktree path and
   checked-out branch, read from `git worktree list` and
   `git branch --show-current`, never from memory or the prompt.
2. **INTENT-MATCH:** confirm this is the worktree and branch *this task* was
   assigned. On a mismatch, stop and surface it; never silently adapt to
   where you happen to be, and never accept name resemblance as a match.
   Wrong-branch is the failure to prevent; it outranks stale-branch. If the
   assignment explicitly calls for creating a worktree that does not exist
   yet, the protected root may only create that exact named branch and
   worktree after the currency check; do no task edits there, then rerun all
   three assertions inside the new worktree. Absence alone never authorizes
   repair.
3. **CURRENCY:** fetch, then check the base against the repository's
   configured target branch (normally `origin/main` or `origin/master`, or
   the task's integration target). New work starts in a fresh worktree off
   its current tip; build on an older base only when the task explicitly pins
   it.

## Cleanup

This section is the one home of the cleanup rules; the skills point here.

At orientation and after a landing, use `codeflow status` to inventory
linked worktrees and unattached local branches. Before any removal, read
`CODEFLOW_STATUS.txt` when supplied, then verify its paths and merge evidence
against current Git state. A saved inventory may be stale. Treat the
removable/dirty/unproven classification from `codeflow status` as local Git evidence, not ownership
authorization: confirm the owner is inactive before promptly closing a
proven-landed resource. Retain active, dirty, and unproven work with an
owner, reason, and recheck event; never use age, name resemblance, or
`git worktree prune` as merge proof.

Cleanup runs after every landing, onto an integration branch or `main`, in
the same step as the landing, by whoever landed it: the primary after it
lands a batch or a PR on the integration line, and the session that
confirms the landed state after a human merge into `main`. It is part of
the landing, never deferred to a later sweep.

A landed task ends with cleanup that proves the merge and inspects worktree
state first. Ancestry never proves a squash merge: require PR state `MERGED`
and match the branch tip to its recorded head SHA, or show that `git cherry`
against the updated target has no unapplied `+` entry, before any branch
force-delete; ancestry proves only a normal merge. Never force-remove a
dirty worktree; preserve or harvest dirty or untracked work first. Unproven
work is retained, never guessed safe.

With that proof, cleanup removes each of these that the task made:

- the clean task worktree, with `git worktree remove` and never `--force`;
- the local branch, with `git branch -d`, or `-D` only after the squash
  proof;
- the pushed branch, with `git push origin --delete <branch>` from the
  repo root, only when the PR state is `MERGED` and the remote tip
  (`git ls-remote origin refs/heads/<branch>`) equals the PR's recorded
  head SHA. The merge command never deletes it: never
  `gh pr merge --delete-branch`;
- the task's build output: the worktree's `target/`, a `CARGO_TARGET_DIR`
  or other build folder made for the task outside the worktree, and the
  builds made to review it.

Some harness sandboxes deny writes that `git worktree remove` needs:
Claude Code's denies writes to its protected paths, which include parts of
`.claude/` and `.git/`. Inside such a sandbox the removal deletes part of the
worktree, then stops with `Operation not permitted` and leaves it
half-removed. Where the effective sandbox denies those writes, make the proof
first (merge, clean worktree, inactive owner), then run the removal once
through the harness's sanctioned unsandboxed path, such as Claude Code's
unsandboxed retry under its normal permission check. Do not make a first
attempt inside the sandbox. If that path is disabled, refused or unavailable,
keep the worktree and hand the proven removal to the operator with the proof
and the exact command. Report a removal that already stopped partway the same
way. Never use `--force`, and never change sandbox or permission settings to
get past the denial.

A folder another active task still uses stays, and so does a remote branch
whose tip moved after the recorded head.

- The cargo target directory of a worktree is `<worktree>/target`, reused
  across that worktree's runs and deleted with it; never one per run and
  never one shared between worktrees. Scratch evidence lives in one
  directory per task and leaves with it.
- Before a worktree or its `target/` is removed, every run artifact and
  review reference the PR cites must already resolve in its durable home:
  the CodeFlow home (`~/.codeflow/gate-runs/<repo>/<run-id>/`) for a gate
  run, the PR itself for the review verdict. Cleanup checks that the
  citations resolve before it removes anything.

## Parallel work and integration

Parallelize independent work when it shortens the critical path, but make
the settled task graph, file ownership, and integration order explicit
first. Use the orchestrator's canonical node and edge notation for
multi-task work (standard and full tiers); a change of outcome, cross-task
interface, dependency graph or safety boundary goes through one reviewed
epic amendment, while ordinary in-node detail does not. Each parallel task
gets one owner, branch, and worktree; never let two sessions write the same
worktree or concurrently edit a shared contract, schema, migration, or other
merge hotspot. The host sets a bounded
concurrency cap from available CPU, memory, disk, and tool limits, monitors
pressure, and reduces fan-out before swapping, duplicate heavyweight builds,
or context sprawl degrade quality. Dependent work stays sequential.

For a multi-task body, integrate through `integration/<epic>` in small
batches, and serialize each landing: one candidate at a time. Before review,
each builder merges the current line into the task branch and resolves the
conflicts there. The primary assembles reviewed task heads on a candidate
from the line tip in dependency order, resolves any remaining conflicts
there, and reviews the resolved hunks and integration seams on product
paths; the other lineage reviews those integration effects only when the
primary hand-resolved a product hunk or two tasks touched one hotspot. Unit
reviews are not repeated. Then one full gate runs on the exact candidate
and is attached to the PRs it covers; a standalone PR is its own
candidate. A red gate is diagnosed first: a member and its dependents are
dropped only when evidence attributes the failure to it, and a shared
runner or environment defect is fixed at its owner and the candidate
regated. On green the line moves to a tree equal to the gated candidate.
Generated files are regenerated, never hand-merged. Rebase task branches,
never a shared integration branch; use `codeflow integrate` where no PR
path exists.
