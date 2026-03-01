// Package session provides the Go implementation of the cf-session-start-init.sh
// hook handler.
//
// It consolidates all session initialization into a single binary call:
//
//   - Parse stdin JSON for session metadata (Claude's per-agent UUID and source)
//   - Detect stale sessions via PID-based cleanup (crashed lead detection)
//   - Generate or load a CODEFLOW_SESSION_ID (ses-{timestamp}{hex} format)
//   - Create required .state/ directories
//   - Detect and warn about stale sessions and teams
//   - Clean up expired sentinels and orphan sentinel directories
//   - Create the pathflow-active flag (is-pathflow-active JSON file)
//   - Initialize the checkpoint file for all 7 PathFlow phases
//   - Write session metadata and current-session-id files
//   - Create the project temp directory
//
// The entry point is [StartInit], which reads hook stdin and performs all
// initialization steps. It returns an [InitResult] containing the session ID,
// environment variables, and any warnings generated during initialization.
//
// This package replaces the 619-line cf-session-start-init.sh shell script
// with a type-safe, testable Go implementation.
package session
