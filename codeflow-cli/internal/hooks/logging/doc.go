// Package logging provides activity log writers for CodeFlow hook events.
//
// It handles five event types (session-start, session-end, stop,
// post-tool-use, user-prompt-submit) by reading Claude Code hook JSON
// from stdin, extracting relevant fields, and appending structured
// JSONL entries to date-rotated log files under .state/logs/sessions/.
//
// This package is distinct from the Tier 0 ledger (internal/ledger):
//
//   - Ledger: schema-validated, event-type-routed WorkGraph rebuild authority
//   - Activity logs: operational debugging/audit trail, no schema validation
//
// The ActivityWriter uses flock-based file locking and O_APPEND writes
// to ensure safe concurrent appends from multiple hook processes.
//
// Session ID resolution follows the canonical priority order:
//
//  1. Parse codeflow-env.sh at .state/runtime/codeflow-env.sh
//  2. CODEFLOW_SESSION_ID environment variable
//  3. Fallback to "unknown"
//
// Usage:
//
//	cfg := logging.ReadConfig(projectDir)
//	w, _ := logging.NewActivityWriter(projectDir, cfg)
//	_ = logging.LogSessionStart(w, stdin, projectDir)
package logging
