package ledger

import (
	"strings"
	"testing"
)

func TestValidateEvent(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		event   Event
		wantErr bool
	}{
		{
			name: "valid session_start",
			event: Event{
				EventType: "session_start",
				SessionID: "ses-abc",
			},
			wantErr: false,
		},
		{
			name: "valid task_created",
			event: Event{
				EventType: "task_created",
				Data: map[string]any{
					"id":      "task-001",
					"epic_id": "epic-001",
					"title":   "Test",
				},
			},
			wantErr: false,
		},
		{
			name: "valid config_set with no extra fields",
			event: Event{
				EventType: "config_set",
			},
			wantErr: false,
		},
		{
			name:    "empty event type",
			event:   Event{},
			wantErr: true,
		},
		{
			name: "unknown event type",
			event: Event{
				EventType: "completely_unknown",
			},
			wantErr: true,
		},
		{
			name: "session_start missing session_id",
			event: Event{
				EventType: "session_start",
			},
			wantErr: true,
		},
		{
			name: "task_created missing required fields",
			event: Event{
				EventType: "task_created",
				Data: map[string]any{
					"id": "task-001",
					// missing epic_id, title
				},
			},
			wantErr: true,
		},
		{
			name: "begin_work with id in data",
			event: Event{
				EventType: "begin_work",
				Data: map[string]any{
					"id": "work-001",
				},
			},
			wantErr: false,
		},
		{
			name: "pr_created with required fields",
			event: Event{
				EventType: "pr_created",
				Data: map[string]any{
					"task_format_id": "INF-TSK-021-001",
					"pr_number":      42,
				},
			},
			wantErr: false,
		},
		{
			name: "pr_created missing pr_number",
			event: Event{
				EventType: "pr_created",
				Data: map[string]any{
					"task_format_id": "INF-TSK-021-001",
				},
			},
			wantErr: true,
		},
		{
			name: "empty string value treated as missing",
			event: Event{
				EventType: "session_start",
				SessionID: "",
				Data: map[string]any{
					"session_id": "",
				},
			},
			wantErr: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			err := ValidateEvent(tt.event)
			if tt.wantErr && err == nil {
				t.Fatal("expected error, got nil")
			}
			if !tt.wantErr && err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
		})
	}
}

func TestValidateEvent_MissingFieldsErrorMessage(t *testing.T) {
	t.Parallel()

	event := Event{
		EventType: "task_created",
		Data: map[string]any{
			"id": "task-001",
			// missing epic_id and title
		},
	}
	err := ValidateEvent(event)
	if err == nil {
		t.Fatal("expected error")
	}
	msg := err.Error()
	if !strings.Contains(msg, "epic_id") {
		t.Errorf("error should mention epic_id: %v", err)
	}
	if !strings.Contains(msg, "title") {
		t.Errorf("error should mention title: %v", err)
	}
}

func TestHasFieldChecksStructAndData(t *testing.T) {
	t.Parallel()

	// session_id in struct field.
	e1 := Event{EventType: "session_start", SessionID: "ses-1"}
	if !hasField(e1, "session_id") {
		t.Error("hasField should find session_id in struct")
	}

	// session_id in Data map.
	e2 := Event{EventType: "session_start", Data: map[string]any{"session_id": "ses-2"}}
	if !hasField(e2, "session_id") {
		t.Error("hasField should find session_id in Data")
	}

	// Missing field.
	e3 := Event{EventType: "session_start"}
	if hasField(e3, "session_id") {
		t.Error("hasField should not find missing session_id")
	}

	// Empty string treated as missing.
	e4 := Event{EventType: "session_start", Data: map[string]any{"session_id": ""}}
	if hasField(e4, "session_id") {
		t.Error("hasField should treat empty string as missing")
	}

	// nil Data map.
	e5 := Event{EventType: "session_start"}
	if hasField(e5, "some_field") {
		t.Error("hasField should handle nil Data")
	}
}

func TestHasField_EventField(t *testing.T) {
	t.Parallel()

	e := Event{EventType: "session_start"}
	if !hasField(e, "event") {
		t.Error("hasField should find event field")
	}

	empty := Event{}
	if hasField(empty, "event") {
		t.Error("hasField should not find empty event field")
	}
}

func TestHasField_TimestampField(t *testing.T) {
	t.Parallel()

	e := Event{EventType: "session_start", Timestamp: "2026-02-28T14:00:00Z"}
	if !hasField(e, "timestamp") {
		t.Error("hasField should find timestamp field")
	}

	noTs := Event{EventType: "session_start"}
	if hasField(noTs, "timestamp") {
		t.Error("hasField should not find empty timestamp")
	}
}

func TestHasField_NonStringDataValue(t *testing.T) {
	t.Parallel()

	// Non-string values (int, bool) should be found.
	e := Event{
		EventType: "pr_created",
		Data: map[string]any{
			"pr_number": 42,
			"is_draft":  false,
		},
	}
	if !hasField(e, "pr_number") {
		t.Error("hasField should find int value in Data")
	}
	if !hasField(e, "is_draft") {
		t.Error("hasField should find bool value in Data")
	}
}

func TestValidateEvent_AllConfigEvents(t *testing.T) {
	t.Parallel()

	// config_set and config_updated have no required fields beyond event/timestamp.
	for _, eventType := range []string{"config_set", "config_updated"} {
		t.Run(eventType, func(t *testing.T) {
			t.Parallel()
			err := ValidateEvent(Event{EventType: eventType})
			if err != nil {
				t.Errorf("ValidateEvent(%q) should pass with no extra fields: %v", eventType, err)
			}
		})
	}
}
