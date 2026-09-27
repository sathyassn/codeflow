# Parallel tasks

Add these to the detailed tasking when implementation has independent tasks.

- for concurrent UI work, the owner and run-scoped browser profile/context,
  service/application endpoints, test-data namespace, artifact directory,
  retention, and teardown verification required by the quality contract;

If implementation has independent tasks, add an explicit execution graph:

- the settled task graph and a valid integration order;
- one file/component owner, branch, and worktree per parallel task;
- shared or conflict-prone files reserved to one integration owner;
- a host resource budget and maximum concurrent heavyweight builds/browsers,
  with non-overlapping browser resources for every parallel UI task;
- the `integration/<epic>` branch and serialized `codeflow integrate` order;
- focused checks per task and combined checks after each landing.

Do not parallelize a short task when coordination costs more than it saves.
Never use concurrent writers in one worktree or rebase a shared integration
branch.
