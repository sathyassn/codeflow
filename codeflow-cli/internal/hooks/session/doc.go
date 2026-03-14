// Package session provides the Go implementation of the session lifecycle hooks:
// session-start initialization and session-end cleanup.
//
// # Session Start
//
// The [Initializer.StartInit] entry point consolidates all session initialization
// into a single binary call:
//
//   - Parse stdin JSON for session metadata (Claude's per-agent UUID and source)
//   - Detect stale sessions via PID-based cleanup (crashed lead detection)
//   - Generate or load a CODEFLOW_SESSION_ID (ses-{ULID} format via session.Start())
//   - Create required .state/ directories
//   - Detect and warn about stale sessions and teams
//   - Clean up orphan sentinel directories
//   - Create the session status file (pathflow-session-status.json)
//   - Initialize the checkpoint file for all 7 PathFlow phases
//   - Write session metadata and codeflow-env.sh files
//   - Create the project temp directory
//
// It returns an [InitResult] containing the session ID, environment variables,
// and any warnings generated during initialization.
//
// # Session End
//
// The [Cleaner.EndCleanup] entry point consolidates all session cleanup into a
// single binary call:
//
//   - Parse stdin JSON for session metadata (Claude's per-agent UUID and transcript path)
//   - Resolve CODEFLOW_SESSION_ID from env file or environment variable
//   - PathFlow guard: skip cleanup for teammate shutdowns while session is active
//   - Validate PF7 completion (check for pathflow-pf-7 sentinel)
//   - Clean up PathFlow sentinels for the session
//   - Preserve in-progress tasks, remove completed ones
//   - Remove team config and task list directories (backstop cleanup)
//   - Remove session state directory
//   - Remove runtime files (codeflow-env.sh)
//   - Remove project temp directory
//
// It returns a [CleanupResult] containing the session ID, PF7 validity,
// sentinel counts, and any warnings generated during cleanup.
//
// [ValidatePF7] is exported for use by other packages that need to check
// PF7 completion status independently.
package session
