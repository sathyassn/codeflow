// Package sentinel provides PathFlow sentinel pipeline management for CodeFlow hooks.
//
// It implements five hook handlers that manage PathFlow stage and phase progression:
//
//   - CheckAndCreateStageSentinel: PostToolUse handler that pattern-matches
//     "STAGE-COMPLETE: WS-{STAGE}" in SendMessage content and creates stage
//     sentinel files (pathflow-ws-dev, pathflow-ws-rev, etc.). Enforces stage
//     ordering: ws-rev requires a prior primary stage, ws-qa requires ws-dev
//     or ws-test.
//
//   - HandleTeamCreate: PostToolUse handler that creates pathflow-team.json
//     in the session pathflow directory when a TeamCreate event fires.
//
//   - HandleTeammateSpawn: PostToolUse handler that updates pathflow-team.json
//     with teammate_spawned=true and last_spawn_name when a Task event fires.
//
//   - RegisterCheckpointTask: PostToolUse handler that registers PF{N}-TSK-{NN}
//     tasks in the checkpoint file when TaskCreate fires. Blocks cross-phase
//     registration (exit 2) if the previous phase sentinel is missing.
//
//   - CompleteCheckpointTask: TaskCompleted handler that marks PF{N}-TSK-{NN}
//     tasks complete in the checkpoint file. Creates phase sentinels
//     (pathflow-pf-{N}) when all tasks in a phase are done, skipped, or
//     auto-skipped by condition.
//
// Sentinel files are created in {projectDir}/.state/sentinels/pathflow/{sessionID}/
// and named "pathflow-{name}" (e.g., "pathflow-ws-dev", "pathflow-pf-3").
//
// Team state is stored in {projectDir}/.state/session/{sessionID}/pathflow/pathflow-team.json.
//
// Checkpoint state is stored in {projectDir}/.state/session/{sessionID}/pathflow/pathflow-phase-tasks.json.
package sentinel
