## Parallel execution contract

Parallelize only workstreams whose inputs and outputs can be isolated. This is
the one field list for parallel work; record it in the plan:

```text
PARALLEL_TASKS:        <from the approved task graph; never a second copy>
OWNER_BRANCH_WORKTREE: <one writer, branch and worktree per task>
HOTSPOT_OWNER:         <one owner, or sequential work, per shared hotspot>
CONCURRENCY_CAP:       <from observed CPU, memory, disk and tool limits>
UI_RESOURCES:          <per concurrent UI task: browser profile or context,
                        endpoints, test-data namespace, artifact directory,
                        retention and teardown check>
```

Parallel eligibility comes from graph topology, but fan-out still requires a
critical-path benefit and safe isolation; do not parallelize a short task when
coordination costs more than it saves. Shared schemas, migrations, lockfiles,
generated registries, and other conflict hotspots have a single owner or run
sequentially. The coordinator caps concurrent heavy builds, browsers, and
model sessions from observed capacity and preserves headroom; reduce fan-out
before swap pressure, duplicate caches or builds, or context dilution affects
evidence quality. Never use concurrent writers in one worktree, and never
rebase a shared integration branch.

Reviewed task heads land together as a batch candidate in dependency order,
and one full gate runs on that exact candidate before the integration line
moves. Passing task-local checks does not prove the integration.
