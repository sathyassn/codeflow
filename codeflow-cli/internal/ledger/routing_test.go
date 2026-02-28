package ledger

import (
	"errors"
	"testing"
)

func TestRouteEvent(t *testing.T) {
	t.Parallel()

	tests := []struct {
		eventType string
		wantFile  string
		wantErr   bool
	}{
		{"session_start", FileSessions, false},
		{"session_end", FileSessions, false},
		{"session_progress", FileSessions, false},
		{"work_claimed", FileSessions, false},
		{"epic_created", FileWorkGraph, false},
		{"epic_status_changed", FileWorkGraph, false},
		{"task_created", FileWorkGraph, false},
		{"task_status_changed", FileWorkGraph, false},
		{"begin_work", FileWorkGraph, false},
		{"complete_work", FileWorkGraph, false},
		{"work_complete", FileWorkGraph, false},
		{"pr_created", FileWorkGraph, false},
		{"pr_merged", FileWorkGraph, false},
		{"memory_store", FileMemoryEvents, false},
		{"memory_stored", FileMemoryEvents, false},
		{"milestone", FileMemoryEvents, false},
		{"progress", FileMemoryEvents, false},
		{"finding", FileMemoryEvents, false},
		{"decision", FileMemoryEvents, false},
		{"blocker", FileMemoryEvents, false},
		{"config_set", FileConfig, false},
		{"config_updated", FileConfig, false},
		{"nonexistent_event", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.eventType, func(t *testing.T) {
			t.Parallel()
			file, err := RouteEvent(tt.eventType)
			if tt.wantErr {
				if err == nil {
					t.Fatalf("expected error for %q", tt.eventType)
				}
				if !errors.Is(err, ErrUnknownEventType) {
					t.Errorf("error should wrap ErrUnknownEventType, got: %v", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("RouteEvent(%q): %v", tt.eventType, err)
			}
			if file != tt.wantFile {
				t.Errorf("RouteEvent(%q) = %q, want %q", tt.eventType, file, tt.wantFile)
			}
		})
	}
}

func TestValidateRoute(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		eventType string
		file      string
		wantErr   error
	}{
		{"correct route", "session_start", FileSessions, nil},
		{"misrouted event", "session_start", FileWorkGraph, ErrMisroutedEvent},
		{"unknown event type", "fake_event", FileSessions, ErrUnknownEventType},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := ValidateRoute(tt.eventType, tt.file)
			if tt.wantErr == nil {
				if err != nil {
					t.Fatalf("unexpected error: %v", err)
				}
				return
			}
			if err == nil {
				t.Fatal("expected error, got nil")
			}
			if !errors.Is(err, tt.wantErr) {
				t.Errorf("error should wrap %v, got: %v", tt.wantErr, err)
			}
		})
	}
}

func TestCanonicalFiles(t *testing.T) {
	t.Parallel()

	files := CanonicalFiles()
	if len(files) != 4 {
		t.Fatalf("CanonicalFiles() returned %d files, want 4", len(files))
	}

	expected := map[string]bool{
		FileWorkGraph:    true,
		FileMemoryEvents: true,
		FileSessions:     true,
		FileConfig:       true,
	}
	for _, f := range files {
		if !expected[f] {
			t.Errorf("unexpected file in CanonicalFiles: %q", f)
		}
	}
}

func TestCanonicalFileConstants(t *testing.T) {
	t.Parallel()

	// Verify constants match expected values.
	if FileWorkGraph != "work-graph.jsonl" {
		t.Errorf("FileWorkGraph = %q, want work-graph.jsonl", FileWorkGraph)
	}
	if FileMemoryEvents != "memory-events.jsonl" {
		t.Errorf("FileMemoryEvents = %q, want memory-events.jsonl", FileMemoryEvents)
	}
	if FileSessions != "sessions.jsonl" {
		t.Errorf("FileSessions = %q, want sessions.jsonl", FileSessions)
	}
	if FileConfig != "config.jsonl" {
		t.Errorf("FileConfig = %q, want config.jsonl", FileConfig)
	}
}

func TestRouteEvent_AllRoutesMapToCanonicalFiles(t *testing.T) {
	t.Parallel()

	canonical := make(map[string]bool)
	for _, f := range CanonicalFiles() {
		canonical[f] = true
	}

	// Every event type in eventRoutes should map to a canonical file.
	for eventType, file := range eventRoutes {
		if !canonical[file] {
			t.Errorf("event type %q maps to non-canonical file %q", eventType, file)
		}
	}
}
