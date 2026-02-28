package ledger

import (
	"errors"
	"fmt"
)

// Canonical JSONL ledger file names. The ledger package is the write-side
// authority for these constants. The db package (internal/db/sync.go) defines
// identical constants for the read/sync side; a cross-package test in
// routing_crosscheck_test.go asserts they never diverge.
const (
	FileWorkGraph    = "work-graph.jsonl"
	FileMemoryEvents = "memory-events.jsonl"
	FileSessions     = "sessions.jsonl"
	FileConfig       = "config.jsonl"
)

// Sentinel errors for routing validation.
var (
	// ErrUnknownEventType indicates an event type not present in the routing table.
	ErrUnknownEventType = errors.New("ledger: unknown event type")

	// ErrMisroutedEvent indicates the event type does not belong to the target file.
	ErrMisroutedEvent = errors.New("ledger: event type does not belong to target file")
)

// eventRoutes maps each known event type to its canonical ledger file.
var eventRoutes = map[string]string{
	// sessions.jsonl
	"session_start":    FileSessions,
	"session_end":      FileSessions,
	"session_progress": FileSessions,
	"work_claimed":     FileSessions,

	// work-graph.jsonl
	"epic_created":        FileWorkGraph,
	"epic_status_changed": FileWorkGraph,
	"task_created":        FileWorkGraph,
	"task_status_changed": FileWorkGraph,
	"begin_work":          FileWorkGraph,
	"complete_work":       FileWorkGraph,
	"work_complete":       FileWorkGraph,
	"pr_created":          FileWorkGraph,
	"pr_merged":           FileWorkGraph,

	// memory-events.jsonl
	"memory_store":  FileMemoryEvents,
	"memory_stored": FileMemoryEvents,
	"milestone":     FileMemoryEvents,
	"progress":      FileMemoryEvents,
	"finding":       FileMemoryEvents,
	"decision":      FileMemoryEvents,
	"blocker":       FileMemoryEvents,

	// config.jsonl
	"config_set":     FileConfig,
	"config_updated": FileConfig,
}

// CanonicalFiles returns the list of all 4 canonical JSONL file names.
func CanonicalFiles() []string {
	return []string{FileWorkGraph, FileMemoryEvents, FileSessions, FileConfig}
}

// RouteEvent returns the canonical ledger file for the given event type.
// Returns ErrUnknownEventType if the event type is not in the routing table.
func RouteEvent(eventType string) (string, error) {
	file, ok := eventRoutes[eventType]
	if !ok {
		return "", fmt.Errorf("%w: %q", ErrUnknownEventType, eventType)
	}
	return file, nil
}

// ValidateRoute checks that the given event type belongs to the specified file.
// Returns nil if the routing is correct, ErrUnknownEventType if the event type
// is unknown, or ErrMisroutedEvent if the event type does not match the file.
func ValidateRoute(eventType, targetFile string) error {
	expected, err := RouteEvent(eventType)
	if err != nil {
		return err
	}
	if expected != targetFile {
		return fmt.Errorf("%w: %q belongs to %q, not %q", ErrMisroutedEvent, eventType, expected, targetFile)
	}
	return nil
}
