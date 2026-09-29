# Workspace mode

Workspace mode is for an umbrella repository that holds several projects,
each its own git repository (CodeFlow projects or plain repositories), in
folders inside it. The umbrella's
root checkout is a working checkout: sessions start there, load their
instructions and settings from it, and read the shared records it holds. So
its root stays on a designated working branch, `integration/workspace` by
convention, instead of the default branch.

## The layout

```text
umbrella/                        root checkout, on integration/workspace
|-- AGENTS.md, shared records    read by every session started here
|-- .claude/ .codex/ .grok/      harness settings for sessions started here
|-- .codeflow/policy.json        git.root_branch: "integration/workspace"
|-- .gitignore                   /project-a/  /project-b/  .worktrees/
|-- .worktrees/<slug>/           umbrella worktrees for larger or parallel work
|-- project-a/                   its own repository, ignored by the umbrella
|   |-- .codeflow/policy.json    its own policy; its root stays on main
|   `-- .worktrees/<slug>/       project-a's task worktrees
`-- project-b/                   a plain git repository, also ignored
```

- The umbrella's root checkout stays on `integration/workspace`. Never
  switch it to another branch.
- Each nested project keeps its root checkout on its own default branch and
  does all task work in worktrees under its own `.worktrees/`.
- Worktree folders that a harness manages count as valid locations too: the
  Claude desktop app's `.claude/worktrees/`, and Codex's and Grok's folders
  under their home directories (`git.worktree_locations`).

## How a change lands

```text
the umbrella's own files

  small edit: a commit at the root checkout ------------+
                                                        |
  larger or parallel work: a branch in                  |
  umbrella/.worktrees/<slug>, cut from the root         |
  branch, merged back with codeflow integrate ----------+
                                                        |
                                                        v
                                              integration/workspace
                                              (the landing line)
                                                        |
                  at a milestone, the operator runs     |
                  codeflow integrate                    |
                  integration/workspace --into main     |
                                                        v
                                                      main
                                              (protected checkpoint;
                                               agents never move it)

a nested project's files

  project-a/.worktrees/<slug> --- pull request ---> project-a's integration
  (a task branch)                                   branch or its main
                                                    (agents never merge
                                                     into main)
```

- The umbrella's own files: a small edit is a commit on
  `integration/workspace` at the root checkout. Larger or parallel work uses
  a short-lived branch in the umbrella's own `.worktrees/<slug>`, cut from
  the root branch and merged back with `codeflow integrate`.
- With no remote, the root branch is the landing line and `main` is a
  protected checkpoint: at a milestone the operator moves it forward with
  `codeflow integrate integration/workspace --into main`; agents never do.
  With a remote the same holds, the root branch is pushed, and a change into
  `main` is a pull request a human merges.
- A nested repository: every change goes through that repository's own
  flow, a worktree under its own `.worktrees/<slug>` and a pull request into
  its integration branch or its `main`. The umbrella never commits nested
  files, which it ignores, and agents never merge into any repository's
  `main`.

## Where settings come from

A session runs the harness settings of the folder it starts in. A session
started at `umbrella/` runs the umbrella's hooks, permissions and sandbox,
even while it edits files in `project-a/`. Git works per repository: a
commit in `project-a/` runs `project-a`'s git hooks and policy, and
git-guard judges each git command by the policy of the repository it
targets. Keep the umbrella's settings at least as strict as every
project's.

## When to use it, and when not

Use workspace mode when one repository coordinates several projects that
each have their own repository, and its root is where sessions start and
shared records live.

Do not use it for:

- a single project: its root checkout stays on the default branch, and all
  task work happens in worktrees;
- a monorepo whose packages share one repository: there are no nested
  repositories to ignore;
- projects added as registered submodules: they are tracked on purpose, and
  workspace mode leaves them alone.

## Set it up

1. At the umbrella's root, with no uncommitted changes to tracked files,
   run `codeflow init --workspace`. It creates `integration/workspace` from
   the default branch (or reuses it), puts the root checkout on it, writes
   `git.root_branch`, and adds every nested repository to `.gitignore`,
   saying which are CodeFlow projects and which are plain git
   repositories. It refuses over uncommitted changes and names them.
2. Commit the files init wrote, including `.gitignore` and
   `.codeflow/policy.json`, on `integration/workspace`.
3. Run `codeflow doctor`. It reports workspace mode, the root branch, and
   any nested repository the tracked `.gitignore` does not cover.
4. When the harness permissions work ships its nested-repository inventory,
   bind the nested repositories with it, so each one is adopted or
   excluded on purpose.

Running `codeflow init --workspace` again changes nothing. Plain `codeflow
init` or `codeflow update` in a folder that holds nested repositories
switches nothing; it says the folder looks like a workspace and names
`codeflow init --workspace`.

## What is enforced

- An agent's commit at the umbrella's root checkout on any branch other
  than `integration/workspace` is refused (`git.root_checkout_commits`). A
  human at their own terminal gets a warning instead.
- `codeflow doctor` warns when the root checkout is on another branch, when
  a nested repository is not ignored by a tracked `.gitignore`, and when a
  linked worktree sits outside `git.worktree_locations`.
- `codeflow ci` classifies a range headed by the root branch as the
  workspace root branch, as it accepts a verified epic line: it needs no
  `Task:` line and may change task criteria. It reads `git.root_branch`
  from the policy on the target, so a pull request cannot name its own
  branch as the root. With durable tracking, a pull request from any other
  `integration/*` branch that is not a verified epic line or a release
  branch is refused whatever its `Task:` line.
