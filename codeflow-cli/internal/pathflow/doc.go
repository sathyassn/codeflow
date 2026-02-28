// Package pathflow provides checkpoint and sentinel management for PathFlow
// phase ordering.
//
// The checkpoint engine tracks phase task registration, completion, and
// automatic sentinel creation. It replaces the shell-based implementation
// in cf-pathflow-state.sh with proper JSON marshaling, file locking, and
// type safety.
//
// Checkpoint operations use sidecar .lock files with syscall.Flock for
// safe concurrent access. All mutations follow an atomic read-modify-write
// pattern: read JSON, modify in memory, write to temp file, os.Rename.
//
// Sentinels are empty files whose existence signals phase or stage
// completion. They are created automatically when all expected tasks in
// a phase are completed or skipped.
package pathflow
