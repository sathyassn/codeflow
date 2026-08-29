# Starting work on codeflow

Ordinary Markdown treatment of the same content. `baseline.html` is a faithful
hand-authored HTML rendering of this file.

## What this repository is

One Rust binary plus an embedded scaffold that scaffolds, enforces, verifies and
remembers, while the harness does the developing. 13 capabilities shipped, 3
building. Not a runtime harness, agent framework, process-enforcement engine, or
GUI product.

## The six layers

| Layer | Lives in |
|---|---|
| WHY — purpose, users, scope, non-goals | `docs/product.md` |
| RULES — how we work | `AGENTS.md` + the agent skills |
| WHAT — what the system does | `docs/capabilities.md` |
| HOW — structure and decisions | `docs/architecture.md`, `docs/decisions/` |
| WORK — planned and active work | `project-management/` |
| TRACE — what happened and why | ledger + `codeflow recall` |

## Preconditions for starting a task

1. The branch prefix is in the policy whitelist.
2. The branch is `task/TSK-NNN-<slug>` and resolves to a visible task record.
3. The visible durable workgraph validates.
4. The declared `integration_target` is a stable, resolvable, non-task branch and
   matches the record's declaration.
5. At the merge-base with that target, the task record exists.
6. Its status is `todo` or `in_progress`.
7. Its epic resolves, or it carries a `standalone_reason`.
8. Every spec it and its epic require is `approved` or `implemented`.
9. Every dependency is `complete`.

## Which gate refuses you

| Rule | Plane | Refuses |
|---|---|---|
| `work.stable_planning_anchor` | `codeflow work start`, pre-commit, `codeflow ci` | preconditions 4–9 |
| `work.task_record` | pre-commit, `codeflow ci` | precondition 2 |
| `work.valid_graph` | pre-commit, `codeflow ci` | precondition 3 |
| `git.branch_naming` | pre-push, `codeflow ci` | precondition 1 |
| `git.secret_scan` | pre-commit | staged credentials; never relaxed |
| `git.commit_format`, `git.commit_body`, `git.ai_attribution`, `git.commit_emoji` | commit-msg, `codeflow ci` | the commit message |
| `git.commit_to_protected`, `git.merge_to_protected`, `git.push_to_protected`, `git.force_push_protected`, `git.delete_protected`, `git.local_ref_protection` | pre-commit, pre-merge-commit, pre-push, reference-transaction | mutating a protected branch |
| `git.hard_reset_protected`, `git.no_verify_bypass`, `git.override_token_laundering`, `git.hook_integrity` | `git-guard` (PreToolUse, in-session) | the command, before it runs |

## On this repository

Remote branch protection is unavailable and not pursued. The authoritative
perimeter is server-side CI plus human-merged PRs; the local hooks and the
in-session guard are fast feedback, not the boundary.
