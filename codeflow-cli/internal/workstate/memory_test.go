package workstate

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/ledger"
)

func TestRecordMemoryEvent(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	w, err := ledger.NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter() error = %v", err)
	}

	event := ledger.Event{
		EventType: "progress",
		SessionID: "ses-test",
		Data: map[string]any{
			"id":      "mem-001",
			"task_id": "task-001",
			"detail":  "implementation started",
		},
	}

	if err := RecordMemoryEvent(w, event); err != nil {
		t.Fatalf("RecordMemoryEvent() error = %v", err)
	}

	// Verify event was written to memory-events.jsonl.
	data, err := os.ReadFile(filepath.Join(dir, ledger.FileMemoryEvents))
	if err != nil {
		t.Fatalf("reading memory events file: %v", err)
	}

	var got map[string]any
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("unmarshaling event: %v", err)
	}

	if got["event"] != "progress" {
		t.Errorf("event type = %v, want %q", got["event"], "progress")
	}
	if got["session_id"] != "ses-test" {
		t.Errorf("session_id = %v, want %q", got["session_id"], "ses-test")
	}
}

func TestRecordMemoryEvent_EmptyEventType(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	w, err := ledger.NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter() error = %v", err)
	}

	event := ledger.Event{
		Data: map[string]any{"key": "value"},
	}

	if err := RecordMemoryEvent(w, event); err == nil {
		t.Fatal("RecordMemoryEvent() expected error for empty event type, got nil")
	}
}

func TestRecordMemoryEvent_MultipleTypes(t *testing.T) {
	t.Parallel()

	memoryTypes := []string{"progress", "decision", "milestone", "finding", "blocker", "memory_store"}

	for _, eventType := range memoryTypes {
		t.Run(eventType, func(t *testing.T) {
			t.Parallel()
			dir := t.TempDir()
			w, err := ledger.NewWriter(dir)
			if err != nil {
				t.Fatalf("NewWriter() error = %v", err)
			}

			event := ledger.Event{
				EventType: eventType,
				SessionID: "ses-multi",
				Data:      map[string]any{"id": "mem-" + eventType, "detail": "test " + eventType},
			}

			if err := RecordMemoryEvent(w, event); err != nil {
				t.Fatalf("RecordMemoryEvent(%q) error = %v", eventType, err)
			}

			// Verify routed to memory-events.jsonl.
			if _, err := os.Stat(filepath.Join(dir, ledger.FileMemoryEvents)); err != nil {
				t.Fatalf("memory-events.jsonl not found for event type %q: %v", eventType, err)
			}
		})
	}
}

func TestRecordMemoryEvent_UnknownEventType(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	w, err := ledger.NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter() error = %v", err)
	}

	// Unknown event type should fail schema validation.
	event := ledger.Event{
		EventType: "totally_unknown_type",
		Data:      map[string]any{"id": "mem-unknown"},
	}

	err = RecordMemoryEvent(w, event)
	if err == nil {
		t.Fatal("RecordMemoryEvent() expected error for unknown event type, got nil")
	}
}
