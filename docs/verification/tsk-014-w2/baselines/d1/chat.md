# D1 ordinary-chat baseline

What a competent assistant types when a newcomer asks how to start working on
this repository. Authored before any candidate.

---

CodeFlow is the AI-development discipline layer you install into a repo: one Rust
binary that scaffolds, enforces, verifies and remembers, while the harness does
the developing. Thirteen capabilities are shipped and three are building. It
deliberately is not a runtime harness, an agent framework, a process-enforcement
engine or a GUI product — gates exist only where a mistake is irreversible or
invisible.

Knowledge lives in six layers: `docs/product.md` for why, this contract plus the
skills for the rules, `docs/capabilities.md` for what the system does,
`docs/architecture.md` and `docs/decisions/` for how, `project-management/` for
planned and active work, and the ledger for what happened.

To start a task you need a few things to already be true. Work happens on a
branch named `task/TSK-NNN-<slug>` in its own worktree, and that branch has to
resolve to a task record that is actually visible in the checkout. The visible
workgraph has to validate — `codeflow validate --docs`.

The important one is less obvious. Planning is a seal in Git history: the task
record and everything it points at must already exist at the merge-base between
your branch and the task's declared `integration_target`. That target must be a
real local or remote-tracking branch and it must not be a task branch, because a
task branch cannot authorize its own planning record. At that merge-base the task
must be present, its status must be `todo` or `in_progress`, its epic must
resolve or it must carry a `standalone_reason`, every spec it and its epic require
must be `approved` or `implemented`, and every dependency must be `complete`.

You check that with `codeflow work start TSK-NNN`. The same read-only check runs
again in the `pre-commit` hook, which will block the commit, and in `codeflow ci`,
which is the authoritative one. In CI it reports `work.stable_planning_anchor` for
an anchoring failure, `work.task_record` if the branch names no visible record,
and `work.valid_graph` if the workgraph is invalid.

Separately from that: `git.branch_naming` refuses a prefix that is not in the
whitelist, `git.secret_scan` refuses staged credentials and is never relaxed,
`git.commit_format`, `git.commit_body`, `git.ai_attribution` and
`git.commit_emoji` refuse a bad message, and the protected-branch rules refuse
committing, merging, pushing, deleting or hard-resetting `main`. On this
repository remote branch protection is unavailable and not pursued, so the
authoritative perimeter is CI plus human-merged PRs, not an armed remote.
