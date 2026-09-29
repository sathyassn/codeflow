# Worktrees and branches

The full text behind the branch row of the moment table in `AGENTS.md`.
Managed by `codeflow update`.

## Where work happens

Develop in a worktree per session: a linked worktree on a feature branch,
under `.worktrees/<slug>` (gitignored) or in the folder a harness manages
for its own worktrees. Never use a sibling folder.

The root checkout, the repository's main working tree, stays on its root
branch and takes no task work. The root branch is the repository's default
branch unless `git.root_branch` in `.codeflow/policy.json` names another.
Change it only for an umbrella repository whose root is a working checkout
(workspace mode): its convention is `{{WORKSPACE_ROOT_BRANCH}}`, and
`codeflow init --workspace` sets it up.

In an umbrella, a change lands this way:

- The umbrella's own files: a small edit is a commit on
  `{{WORKSPACE_ROOT_BRANCH}}` at the root checkout. Larger or parallel
  work uses a short-lived branch in the umbrella's own
  `.worktrees/<slug>`, cut from the root branch and merged back with
  `codeflow integrate`.
- With no remote, the root branch is the landing line and `main` is a
  protected checkpoint: at a milestone the operator moves it forward with
  `codeflow integrate {{WORKSPACE_ROOT_BRANCH}} --into main`; agents
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

At orientation and after a landing, use `codeflow status` to inventory
linked worktrees and unattached local branches. Treat its
removable/dirty/unproven classification as local Git evidence, not ownership
authorization: confirm the owner is inactive before promptly closing a
proven-landed resource. Retain active, dirty, and unproven work with an
owner, reason, and recheck event; never use age, name resemblance, or
`git worktree prune` as merge proof.

A landed task ends with cleanup that proves the merge and inspects worktree
state first. Ancestry never proves a squash merge: require PR state `MERGED`
and match the branch tip to its recorded head SHA, or show that `git cherry`
against the updated target has no unapplied `+` entry, before any branch
force-delete. Never force-remove a dirty worktree; preserve or harvest dirty
or untracked work first. Unproven work is retained, never guessed safe.

## Parallel work and integration

Parallelize independent work when it shortens the critical path, but make
the settled task graph, file ownership, and integration order explicit
first. Use the orchestrator's canonical node and edge notation for
multi-task work (standard and full tiers); a material graph mutation
requires a newly dual-approved plan version, while ordinary in-node detail
does not. Each parallel task gets one owner, branch, and worktree; never let
two sessions write the same worktree or concurrently edit a shared contract,
schema, migration, or other merge hotspot. The host sets a bounded
concurrency cap from available CPU, memory, disk, and tool limits, monitors
pressure, and reduces fan-out before swapping, duplicate heavyweight builds,
or context sprawl degrade quality. Dependent work stays sequential.

For a multi-task body, integrate through `integration/<epic>` and serialize
each landing with `codeflow integrate`; rerun the affected and aggregate
gates after every merge. Rebase task branches, never a shared integration
branch. Parallel output is not complete until the integration worktree is
green and the combined diff has received the same executor verification,
primary acceptance, cross-lineage unit review, and integrated
Claude-judgment-primary review as a serial change.
