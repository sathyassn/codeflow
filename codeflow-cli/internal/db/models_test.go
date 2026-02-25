package db

import (
	"database/sql"
	"encoding/json"
	"testing"
)

func TestStructSerialization(t *testing.T) {
	tests := []struct {
		name string
		task Task
	}{
		{
			name: "full task with all fields",
			task: Task{
				ID:       "task-01BRZ4PDFLUTW5SSGG70H6GBW",
				FormatID: "INF-TSK-015-004",
				EpicID:   "epic-01ARZ3NDEKTSV4RRFFQ69G5FAV",
				Title:    "Database core package",
				Description: sql.NullString{
					String: "Implement DB connection singleton",
					Valid:  true,
				},
				Status:   "in_progress",
				AreaType: "INF",
				WorkType: "FEAT",
				Domain:   "GENL",
				Origin:   "planned",
				Priority: "normal",
				Branch: sql.NullString{
					String: "feat/go-cli-db-package",
					Valid:  true,
				},
				StageHistory: json.RawMessage(`[{"stage":"dev","status":"in_progress"}]`),
				CreatedAt:    "2024-01-15T10:00:00Z",
				UpdatedAt:    "2024-01-15T12:00:00Z",
			},
		},
		{
			name: "minimal task with null fields",
			task: Task{
				ID:        "task-minimal",
				FormatID:  "INF-TSK-001-001",
				EpicID:    "epic-minimal",
				Title:     "Minimal task",
				Status:    "todo",
				AreaType:  "INF",
				WorkType:  "FEAT",
				Domain:    "GENL",
				Origin:    "planned",
				Priority:  "normal",
				CreatedAt: "2024-01-15T10:00:00Z",
				UpdatedAt: "2024-01-15T10:00:00Z",
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Marshal to JSON.
			data, err := json.Marshal(tt.task)
			if err != nil {
				t.Fatalf("Marshal: %v", err)
			}

			// Unmarshal back.
			var got Task
			if err := json.Unmarshal(data, &got); err != nil {
				t.Fatalf("Unmarshal: %v", err)
			}

			// Verify key fields round-trip correctly.
			if got.ID != tt.task.ID {
				t.Errorf("ID = %q, want %q", got.ID, tt.task.ID)
			}
			if got.Title != tt.task.Title {
				t.Errorf("Title = %q, want %q", got.Title, tt.task.Title)
			}
			if got.Status != tt.task.Status {
				t.Errorf("Status = %q, want %q", got.Status, tt.task.Status)
			}
		})
	}
}

func TestJSONFieldHandling(t *testing.T) {
	tests := []struct {
		name      string
		input     json.RawMessage
		wantValid bool
	}{
		{
			name:      "valid JSON array",
			input:     json.RawMessage(`["file1.go","file2.go"]`),
			wantValid: true,
		},
		{
			name:      "valid JSON object",
			input:     json.RawMessage(`{"key":"value"}`),
			wantValid: true,
		},
		{
			name:      "empty JSON array",
			input:     json.RawMessage(`[]`),
			wantValid: true,
		},
		{
			name:      "nil JSON becomes null after round-trip",
			input:     nil,
			wantValid: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			task := Task{
				ID:        "task-json-test",
				FormatID:  "INF-TSK-001-001",
				EpicID:    "epic-test",
				Title:     "JSON test",
				Status:    "todo",
				AreaType:  "INF",
				WorkType:  "FEAT",
				Domain:    "GENL",
				Origin:    "planned",
				Priority:  "normal",
				FileScope: tt.input,
				CreatedAt: "2024-01-15T10:00:00Z",
				UpdatedAt: "2024-01-15T10:00:00Z",
			}

			data, err := json.Marshal(task)
			if err != nil {
				t.Fatalf("Marshal: %v", err)
			}

			var got Task
			if err := json.Unmarshal(data, &got); err != nil {
				t.Fatalf("Unmarshal: %v", err)
			}

			if tt.wantValid {
				if !json.Valid(got.FileScope) {
					t.Errorf("FileScope is not valid JSON after round-trip: %s", got.FileScope)
				}
				// Verify content matches.
				if string(got.FileScope) != string(tt.input) {
					t.Errorf("FileScope = %s, want %s", got.FileScope, tt.input)
				}
			} else {
				// A nil json.RawMessage marshals as "null" and unmarshals back
				// as json.RawMessage("null"), not nil. This is standard Go behavior.
				if string(got.FileScope) != "null" {
					t.Errorf("FileScope = %s, want null", got.FileScope)
				}
			}
		})
	}
}

func TestNilSafeAccessors(t *testing.T) {
	tests := []struct {
		name string
		fn   func(t *testing.T)
	}{
		{
			name: "zero-value ActiveWork",
			fn: func(t *testing.T) {
				t.Helper()
				var aw ActiveWork
				data, err := json.Marshal(aw)
				if err != nil {
					t.Fatalf("Marshal zero ActiveWork: %v", err)
				}
				var got ActiveWork
				if err := json.Unmarshal(data, &got); err != nil {
					t.Fatalf("Unmarshal zero ActiveWork: %v", err)
				}
				if got.TaskID.Valid {
					t.Error("zero-value TaskID should not be valid")
				}
				if got.SessionID.Valid {
					t.Error("zero-value SessionID should not be valid")
				}
			},
		},
		{
			name: "zero-value Session",
			fn: func(t *testing.T) {
				t.Helper()
				var s Session
				data, err := json.Marshal(s)
				if err != nil {
					t.Fatalf("Marshal zero Session: %v", err)
				}
				var got Session
				if err := json.Unmarshal(data, &got); err != nil {
					t.Fatalf("Unmarshal zero Session: %v", err)
				}
				if got.EndedAt.Valid {
					t.Error("zero-value EndedAt should not be valid")
				}
				if got.DurationSeconds.Valid {
					t.Error("zero-value DurationSeconds should not be valid")
				}
			},
		},
		{
			name: "zero-value MemoryEvent",
			fn: func(t *testing.T) {
				t.Helper()
				var me MemoryEvent
				data, err := json.Marshal(me)
				if err != nil {
					t.Fatalf("Marshal zero MemoryEvent: %v", err)
				}
				var got MemoryEvent
				if err := json.Unmarshal(data, &got); err != nil {
					t.Fatalf("Unmarshal zero MemoryEvent: %v", err)
				}
				if got.WorkID.Valid {
					t.Error("zero-value WorkID should not be valid")
				}
				if got.MemoryType.Valid {
					t.Error("zero-value MemoryType should not be valid")
				}
			},
		},
		{
			name: "zero-value AutorunWorker",
			fn: func(t *testing.T) {
				t.Helper()
				var aw AutorunWorker
				data, err := json.Marshal(aw)
				if err != nil {
					t.Fatalf("Marshal zero AutorunWorker: %v", err)
				}
				var got AutorunWorker
				if err := json.Unmarshal(data, &got); err != nil {
					t.Fatalf("Unmarshal zero AutorunWorker: %v", err)
				}
				if got.TmuxSession.Valid {
					t.Error("zero-value TmuxSession should not be valid")
				}
				if got.PRNumber.Valid {
					t.Error("zero-value PRNumber should not be valid")
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, tt.fn)
	}
}

func TestEpicSerialization(t *testing.T) {
	epic := Epic{
		ID:       "epic-01ARZ3NDEKTSV4RRFFQ69G5FAV",
		FormatID: "INF-EPC-015",
		Title:    "Go CLI Infrastructure",
		Summary:  sql.NullString{String: "Build the Go CLI", Valid: true},
		Status:   "in_progress",
		AreaType: "INF",
		WorkType: "FEAT",
		Domain:   "GENL",
		Priority: "high",
		FileScope: json.RawMessage(`["codeflow-cli/"]`),
		CreatedAt: "2024-01-15T10:00:00Z",
		UpdatedAt: "2024-01-15T12:00:00Z",
	}

	data, err := json.Marshal(epic)
	if err != nil {
		t.Fatalf("Marshal Epic: %v", err)
	}

	var got Epic
	if err := json.Unmarshal(data, &got); err != nil {
		t.Fatalf("Unmarshal Epic: %v", err)
	}

	if got.ID != epic.ID {
		t.Errorf("ID = %q, want %q", got.ID, epic.ID)
	}
	if got.Title != epic.Title {
		t.Errorf("Title = %q, want %q", got.Title, epic.Title)
	}
	if string(got.FileScope) != string(epic.FileScope) {
		t.Errorf("FileScope = %s, want %s", got.FileScope, epic.FileScope)
	}
}
