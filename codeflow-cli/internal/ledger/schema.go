package ledger

import (
	"fmt"
	"strings"
)

// requiredFields maps event types to their required fields beyond the
// universal "event" and "timestamp" fields. These are checked by ValidateEvent.
var requiredFields = map[string][]string{
	// sessions.jsonl
	"session_start":    {"session_id"},
	"session_end":      {"session_id"},
	"session_progress": {"session_id"},
	"work_claimed":     {"session_id"},
	"claim_created":    {"id"},
	"claim_released":   {"claim_id"},
	"claim_renewed":    {"claim_id"},

	// work-graph.jsonl
	"epic_created":        {"id", "title"},
	"epic_status_changed": {"epic_id", "new_status"},
	"task_created":        {"id", "epic_id", "title"},
	"task_status_changed": {"task_id", "new_status"},
	"begin_work":          {"id"},
	"complete_work":       {"id"},
	"work_complete":       {"id"},
	"pr_created":          {"task_format_id", "pr_number"},
	"pr_merged":           {"task_format_id"},

	// memory-events.jsonl
	"memory_store":  {"id"},
	"memory_stored": {"id"},
	"milestone":     {"id"},
	"progress":      {"id"},
	"finding":       {"id"},
	"decision":      {"id"},
	"blocker":       {"id"},

	// config.jsonl
	"config_set":     {},
	"config_updated": {},

	// pathflow-events.jsonl
	"phase_transition":     {"session_id", "phase", "status"},
	"stage_transition":     {"session_id", "stage", "status"},
	"session_register":     {"session_id"},
	"session_metadata":     {"session_id", "key", "value"},
	"pathflow_task_update": {"session_id", "task_id", "task_status"},
}

// ValidateEvent checks that an event has all required fields for its type.
// The event must have a non-empty EventType. Required fields are checked
// in both the top-level Event fields (SessionID) and the Data map.
func ValidateEvent(e Event) error {
	if e.EventType == "" {
		return fmt.Errorf("ledger: event type must not be empty")
	}

	fields, ok := requiredFields[e.EventType]
	if !ok {
		return fmt.Errorf("%w: %q", ErrUnknownEventType, e.EventType)
	}

	var missing []string
	for _, field := range fields {
		if !hasField(e, field) {
			missing = append(missing, field)
		}
	}

	if len(missing) > 0 {
		return fmt.Errorf("ledger: %s: missing required fields: %s",
			e.EventType, strings.Join(missing, ", "))
	}

	return nil
}

// hasField checks whether the event has a non-empty value for the given field.
// It checks the Event struct's named fields first, then falls back to Data.
func hasField(e Event, field string) bool {
	// Check named struct fields.
	switch field {
	case "session_id":
		if e.SessionID != "" {
			return true
		}
	case "event":
		return e.EventType != ""
	case "timestamp":
		return e.Timestamp != ""
	}

	// Check Data map.
	if e.Data == nil {
		return false
	}
	v, ok := e.Data[field]
	if !ok {
		return false
	}

	// Reject empty string values.
	if s, isStr := v.(string); isStr && s == "" {
		return false
	}

	return true
}
