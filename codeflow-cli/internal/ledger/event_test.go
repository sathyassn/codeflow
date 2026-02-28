package ledger

import (
	"encoding/json"
	"testing"
	"time"
)

func TestEventMarshalJSON(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		event   Event
		wantErr bool
		check   func(t *testing.T, data []byte)
	}{
		{
			name: "basic event with timestamp",
			event: Event{
				EventType: "session_start",
				Timestamp: "2026-02-28T12:00:00Z",
				SessionID: "ses-abc123",
			},
			check: func(t *testing.T, data []byte) {
				t.Helper()
				var m map[string]any
				if err := json.Unmarshal(data, &m); err != nil {
					t.Fatalf("unmarshal: %v", err)
				}
				if m["event"] != "session_start" {
					t.Errorf("event = %v, want session_start", m["event"])
				}
				if m["timestamp"] != "2026-02-28T12:00:00Z" {
					t.Errorf("timestamp = %v, want 2026-02-28T12:00:00Z", m["timestamp"])
				}
				if m["session_id"] != "ses-abc123" {
					t.Errorf("session_id = %v, want ses-abc123", m["session_id"])
				}
			},
		},
		{
			name: "auto-generates timestamp when empty",
			event: Event{
				EventType: "config_set",
				// Timestamp left empty -- marshalFlat fills it from time.Now.
			},
			check: func(t *testing.T, data []byte) {
				t.Helper()
				var m map[string]any
				if err := json.Unmarshal(data, &m); err != nil {
					t.Fatalf("unmarshal: %v", err)
				}
				ts, ok := m["timestamp"].(string)
				if !ok || ts == "" {
					t.Error("timestamp should be auto-generated")
				}
				// Verify it parses as RFC 3339.
				if _, err := time.Parse(time.RFC3339, ts); err != nil {
					t.Errorf("timestamp %q is not valid RFC 3339: %v", ts, err)
				}
			},
		},
		{
			name: "merges data fields into flat JSON",
			event: Event{
				EventType: "task_created",
				Timestamp: "2026-02-28T12:00:00Z",
				Data: map[string]any{
					"id":      "task-001",
					"epic_id": "epic-001",
					"title":   "Test task",
				},
			},
			check: func(t *testing.T, data []byte) {
				t.Helper()
				var m map[string]any
				if err := json.Unmarshal(data, &m); err != nil {
					t.Fatalf("unmarshal: %v", err)
				}
				if m["id"] != "task-001" {
					t.Errorf("id = %v, want task-001", m["id"])
				}
				if m["epic_id"] != "epic-001" {
					t.Errorf("epic_id = %v, want epic-001", m["epic_id"])
				}
			},
		},
		{
			name: "empty event type produces error",
			event: Event{
				Timestamp: "2026-02-28T12:00:00Z",
			},
			wantErr: true,
		},
		{
			name: "session_id omitted when empty",
			event: Event{
				EventType: "config_set",
				Timestamp: "2026-02-28T12:00:00Z",
			},
			check: func(t *testing.T, data []byte) {
				t.Helper()
				var m map[string]any
				if err := json.Unmarshal(data, &m); err != nil {
					t.Fatalf("unmarshal: %v", err)
				}
				if _, exists := m["session_id"]; exists {
					t.Error("session_id should be omitted when empty")
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			data, err := json.Marshal(tt.event)
			if tt.wantErr {
				if err == nil {
					t.Fatal("expected error, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("Marshal: %v", err)
			}
			if tt.check != nil {
				tt.check(t, data)
			}
		})
	}
}

func TestEventUnmarshalJSON(t *testing.T) {
	t.Parallel()

	input := `{"event":"session_start","timestamp":"2026-02-28T12:00:00Z","session_id":"ses-abc","user_host":"laptop"}`
	var e Event
	if err := json.Unmarshal([]byte(input), &e); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if e.EventType != "session_start" {
		t.Errorf("EventType = %q, want session_start", e.EventType)
	}
	if e.Timestamp != "2026-02-28T12:00:00Z" {
		t.Errorf("Timestamp = %q, want 2026-02-28T12:00:00Z", e.Timestamp)
	}
	if e.SessionID != "ses-abc" {
		t.Errorf("SessionID = %q, want ses-abc", e.SessionID)
	}
	if e.Data["user_host"] != "laptop" {
		t.Errorf("Data[user_host] = %v, want laptop", e.Data["user_host"])
	}
}

func TestEventUnmarshalJSON_EmptyData(t *testing.T) {
	t.Parallel()

	input := `{"event":"config_set","timestamp":"2026-02-28T12:00:00Z"}`
	var e Event
	if err := json.Unmarshal([]byte(input), &e); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if e.EventType != "config_set" {
		t.Errorf("EventType = %q, want config_set", e.EventType)
	}
	if e.Data != nil {
		t.Errorf("Data should be nil for event with no extra fields, got %v", e.Data)
	}
}

func TestEventUnmarshalJSON_InvalidJSON(t *testing.T) {
	t.Parallel()

	var e Event
	err := json.Unmarshal([]byte("not-json"), &e)
	if err == nil {
		t.Fatal("expected error for invalid JSON")
	}
}

func TestEventMarshalJSON_DataFieldPrecedence(t *testing.T) {
	t.Parallel()

	// Top-level fields should take precedence over Data keys with same name.
	e := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T12:00:00Z",
		SessionID: "ses-real",
		Data: map[string]any{
			"event":      "should-be-overridden",
			"session_id": "should-be-overridden",
		},
	}

	data, err := json.Marshal(e)
	if err != nil {
		t.Fatalf("Marshal: %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal(data, &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if m["event"] != "session_start" {
		t.Errorf("event = %v, want session_start (top-level should win)", m["event"])
	}
	if m["session_id"] != "ses-real" {
		t.Errorf("session_id = %v, want ses-real (top-level should win)", m["session_id"])
	}
}
