# Parallel execution contract

Parallelize only workstreams whose inputs and outputs can be isolated. Record:

```text
SETTLED_TASK_GRAPH:
PARALLEL_TASKS:
TASK_BRANCH_WORKTREE_OWNER:
SHARED_FILE_OWNER:
INTEGRATION_BRANCH_AND_ORDER:
HOST_RESOURCE_BUDGET:
PER_TASK_GATES:
POST_MERGE_GATES:
```

`SETTLED_TASK_GRAPH` references the exact graph already approved in Plan vN; it
is not a divergent second copy. Parallel eligibility comes from graph topology,
but fan-out still requires a critical-path benefit and safe isolation. Each
implementation task has one writer, branch, and worktree. Shared schemas,
migrations, lockfiles, generated registries, and other conflict hotspots have a
single integration owner or run sequentially. The coordinator caps concurrent
heavy builds, browsers, and model sessions from observed CPU, memory, disk, and
tool limits and preserves headroom; reduce fan-out before swap pressure,
duplicate caches/builds, or context dilution affects evidence quality. Never
use concurrent writers in one worktree, and never rebase a shared integration
branch.

Merge task branches in the recorded order through serialized integration,
running the affected gates after each merge and the aggregate gates on the final
combined diff. Passing task-local checks does not prove the integration.
