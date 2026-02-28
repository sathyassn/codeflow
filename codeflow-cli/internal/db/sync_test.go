package db

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNormalizeEvent(t *testing.T) {
	t.Parallel()
	t.Run("Pattern 1: canonical event key", func(t *testing.T) {
		raw := map[string]any{
			"event":     "epic_created",
			"timestamp": "2024-01-01T00:00:00Z",
			"id":        "E-001",
		}

		result := normalizeEvent(raw)

		if result.Event != "epic_created" {
			t.Errorf("event = %q, want epic_created", result.Event)
		}
		if result.Timestamp != "2024-01-01T00:00:00Z" {
			t.Errorf("timestamp = %q, want 2024-01-01T00:00:00Z", result.Timestamp)
		}
	})

	t.Run("Pattern 2: type key renamed to event", func(t *testing.T) {
		raw := map[string]any{
			"type":      "task_created",
			"timestamp": "2024-01-01T00:00:00Z",
		}

		result := normalizeEvent(raw)

		if result.Event != "task_created" {
			t.Errorf("event = %q, want task_created", result.Event)
		}
		// "type" key should be removed, "event" key should be set.
		if _, exists := result.Raw["type"]; exists {
			t.Error("expected 'type' key to be removed")
		}
		if result.Raw["event"] != "task_created" {
			t.Errorf("raw['event'] = %v, want task_created", result.Raw["event"])
		}
	})

	t.Run("Pattern 2: memory_stored normalized to memory_store", func(t *testing.T) {
		raw := map[string]any{
			"type": "memory_stored",
		}

		result := normalizeEvent(raw)

		if result.Event != "memory_store" {
			t.Errorf("event = %q, want memory_store", result.Event)
		}
	})

	t.Run("Pattern 3: shorthand e key", func(t *testing.T) {
		raw := map[string]any{
			"e":   "session_start",
			"ts":  "2024-01-01T00:00:00Z",
			"sid": "ses-123",
		}

		result := normalizeEvent(raw)

		if result.Event != "session_start" {
			t.Errorf("event = %q, want session_start", result.Event)
		}
		if result.Timestamp != "2024-01-01T00:00:00Z" {
			t.Errorf("timestamp = %q, want 2024-01-01T00:00:00Z", result.Timestamp)
		}
		// "e" key removed, "ts" renamed to "timestamp", "sid" renamed to "session_id".
		if _, exists := result.Raw["e"]; exists {
			t.Error("expected 'e' key removed")
		}
		if _, exists := result.Raw["ts"]; exists {
			t.Error("expected 'ts' key removed")
		}
		if result.Raw["session_id"] != "ses-123" {
			t.Errorf("raw['session_id'] = %v, want ses-123", result.Raw["session_id"])
		}
	})

	t.Run("Pattern 4: op+table mapped to event", func(t *testing.T) {
		raw := map[string]any{
			"op":    "INSERT",
			"table": "epics",
			"id":    "E-001",
		}

		result := normalizeEvent(raw)

		if result.Event != "epic_created" {
			t.Errorf("event = %q, want epic_created", result.Event)
		}
		// "op" and "table" keys removed.
		if _, exists := result.Raw["op"]; exists {
			t.Error("expected 'op' key removed")
		}
		if _, exists := result.Raw["table"]; exists {
			t.Error("expected 'table' key removed")
		}
	})

	t.Run("field renaming: from_status to old_status", func(t *testing.T) {
		raw := map[string]any{
			"event":       "task_status_changed",
			"from_status": "todo",
			"to_status":   "in_progress",
		}

		result := normalizeEvent(raw)

		if result.Raw["old_status"] != "todo" {
			t.Errorf("raw['old_status'] = %v, want todo", result.Raw["old_status"])
		}
		if result.Raw["new_status"] != "in_progress" {
			t.Errorf("raw['new_status'] = %v, want in_progress", result.Raw["new_status"])
		}
	})

	t.Run("no event type returns empty event", func(t *testing.T) {
		raw := map[string]any{
			"id": "unknown",
		}

		result := normalizeEvent(raw)

		if result.Event != "" {
			t.Errorf("event = %q, want empty", result.Event)
		}
	})

	t.Run("wid renamed to work_id", func(t *testing.T) {
		raw := map[string]any{
			"event": "begin_work",
			"wid":   "W-001",
		}

		result := normalizeEvent(raw)

		if result.Raw["work_id"] != "W-001" {
			t.Errorf("raw['work_id'] = %v, want W-001", result.Raw["work_id"])
		}
		if _, exists := result.Raw["wid"]; exists {
			t.Error("expected 'wid' key removed")
		}
	})
}

func TestMapOpToEvent(t *testing.T) {
	t.Parallel()
	tests := []struct {
		op    string
		table string
		want  string
	}{
		{"INSERT", "memory_events", "memory_store"},
		{"INSERT", "sessions", "session_start"},
		{"INSERT", "epics", "epic_created"},
		{"INSERT", "tasks", "task_created"},
		{"UPDATE", "tasks", "task_status_changed"},
		{"UPDATE", "epics", "epic_status_changed"},
		{"INSERT", "active_work", "begin_work"},
		{"INSERT", "other_table", "other_table_created"},
		{"UPDATE", "other_table", "other_table_updated"},
		{"DELETE", "items", "delete_items"},
		{"insert", "epics", "epic_created"}, // lowercase op
	}

	for _, tt := range tests {
		t.Run(tt.op+"_"+tt.table, func(t *testing.T) {
			got := mapOpToEvent(tt.op, tt.table)
			if got != tt.want {
				t.Errorf("mapOpToEvent(%q, %q) = %q, want %q", tt.op, tt.table, got, tt.want)
			}
		})
	}
}

func TestNormalizeEventName(t *testing.T) {
	t.Parallel()
	tests := []struct {
		input string
		want  string
	}{
		{"memory_stored", "memory_store"},
		{"epic_created", "epic_created"},
		{"session_start", "session_start"},
		{"unknown_event", "unknown_event"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			got := normalizeEventName(tt.input)
			if got != tt.want {
				t.Errorf("normalizeEventName(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestRenameField(t *testing.T) {
	t.Parallel()
	t.Run("renames existing field", func(t *testing.T) {
		m := map[string]any{"old": "value"}
		renameField(m, "old", "new")

		if _, exists := m["old"]; exists {
			t.Error("old key should not exist")
		}
		if m["new"] != "value" {
			t.Errorf("new = %v, want value", m["new"])
		}
	})

	t.Run("no-op if old key missing", func(t *testing.T) {
		m := map[string]any{"other": "value"}
		renameField(m, "old", "new")

		if _, exists := m["new"]; exists {
			t.Error("new key should not be created from nothing")
		}
	})

	t.Run("does not overwrite existing new key", func(t *testing.T) {
		m := map[string]any{"old": "original", "new": "existing"}
		renameField(m, "old", "new")

		if m["new"] != "existing" {
			t.Errorf("new = %v, want existing (should not overwrite)", m["new"])
		}
		// Old key should remain since rename was blocked.
		if m["old"] != "original" {
			t.Errorf("old = %v, want original (should remain when rename blocked)", m["old"])
		}
	})
}

func TestReadAndNormalize(t *testing.T) {
	t.Parallel()
	t.Run("reads and normalizes valid JSONL file", func(t *testing.T) {
		dir := t.TempDir()
		path := filepath.Join(dir, "events.jsonl")

		content := strings.Join([]string{
			`{"event":"epic_created","id":"E-001","timestamp":"2024-01-01T00:00:00Z"}`,
			`{"type":"task_created","id":"T-001"}`,
			`{"e":"session_start","sid":"ses-123","ts":"2024-01-01T00:00:00Z"}`,
		}, "\n")

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		events, errs := ReadAndNormalize(path)
		if len(errs) != 0 {
			t.Errorf("unexpected errors: %v", errs)
		}
		if len(events) != 3 {
			t.Fatalf("got %d events, want 3", len(events))
		}

		if events[0].Event != "epic_created" {
			t.Errorf("event[0] = %q, want epic_created", events[0].Event)
		}
		if events[1].Event != "task_created" {
			t.Errorf("event[1] = %q, want task_created", events[1].Event)
		}
		if events[2].Event != "session_start" {
			t.Errorf("event[2] = %q, want session_start", events[2].Event)
		}
	})

	t.Run("returns nil for nonexistent file", func(t *testing.T) {
		events, errs := ReadAndNormalize("/nonexistent/file.jsonl")
		if events != nil {
			t.Error("expected nil events for missing file")
		}
		if errs != nil {
			t.Error("expected nil errors for missing file")
		}
	})

	t.Run("skips blank lines", func(t *testing.T) {
		dir := t.TempDir()
		path := filepath.Join(dir, "sparse.jsonl")
		content := `{"event":"a"}` + "\n\n" + `{"event":"b"}` + "\n\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		events, errs := ReadAndNormalize(path)
		if len(errs) != 0 {
			t.Errorf("unexpected errors: %v", errs)
		}
		if len(events) != 2 {
			t.Errorf("got %d events, want 2", len(events))
		}
	})

	t.Run("reports invalid JSON lines", func(t *testing.T) {
		dir := t.TempDir()
		path := filepath.Join(dir, "bad.jsonl")
		content := `{"event":"good"}` + "\n" + `not json` + "\n" + `{"event":"also_good"}` + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		events, errs := ReadAndNormalize(path)
		if len(events) != 2 {
			t.Errorf("got %d events, want 2 (skipping invalid)", len(events))
		}
		if len(errs) != 1 {
			t.Errorf("got %d errors, want 1", len(errs))
		}
	})

	t.Run("reports lines with no event type", func(t *testing.T) {
		dir := t.TempDir()
		path := filepath.Join(dir, "noevt.jsonl")
		content := `{"id":"orphan","data":"test"}` + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		events, errs := ReadAndNormalize(path)
		if len(events) != 0 {
			t.Errorf("got %d events, want 0", len(events))
		}
		if len(errs) != 1 {
			t.Errorf("got %d errors, want 1 (no event type)", len(errs))
		}
	})
}

func TestReadAndNormalizeReader(t *testing.T) {
	t.Parallel()
	t.Run("all 4 patterns in one stream", func(t *testing.T) {
		input := strings.NewReader(strings.Join([]string{
			`{"event":"epic_created","id":"E-001"}`,                      // Pattern 1
			`{"type":"memory_stored","id":"M-001"}`,                      // Pattern 2
			`{"e":"session_start","sid":"ses-1","ts":"2024-01-01T00:00:00Z"}`, // Pattern 3
			`{"op":"INSERT","table":"tasks","id":"T-001"}`,               // Pattern 4
		}, "\n"))

		events, errs := readAndNormalizeReader(input)
		if len(errs) != 0 {
			t.Errorf("unexpected errors: %v", errs)
		}
		if len(events) != 4 {
			t.Fatalf("got %d events, want 4", len(events))
		}

		// Pattern 1: canonical.
		if events[0].Event != "epic_created" {
			t.Errorf("pattern1: event = %q, want epic_created", events[0].Event)
		}

		// Pattern 2: type -> event, memory_stored -> memory_store.
		if events[1].Event != "memory_store" {
			t.Errorf("pattern2: event = %q, want memory_store", events[1].Event)
		}

		// Pattern 3: e -> event, ts -> timestamp, sid -> session_id.
		if events[2].Event != "session_start" {
			t.Errorf("pattern3: event = %q, want session_start", events[2].Event)
		}
		if events[2].Timestamp != "2024-01-01T00:00:00Z" {
			t.Errorf("pattern3: timestamp = %q", events[2].Timestamp)
		}

		// Pattern 4: op+table -> event.
		if events[3].Event != "task_created" {
			t.Errorf("pattern4: event = %q, want task_created", events[3].Event)
		}
	})
}

func TestSyncFromJSONL(t *testing.T) {
	t.Parallel()
	t.Run("syncs events from JSONL files", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		// Initialize schema (creates tables and seeds area_types, work_types, domains).
		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		// Insert a user record since sessions table has FK to users(id).
		_, err := d.Execute(ctx,
			"INSERT INTO users (id, email, display_name) VALUES ('user-test', 'test@test.com', 'Test User')")
		if err != nil {
			t.Fatalf("inserting user: %v", err)
		}

		ledgerDir := t.TempDir()

		// Write work-graph.jsonl with epic and task.
		// Use valid FK values: area_type=INF (seeded), work_type=FEAT (seeded), domain=GENL (seeded).
		wgContent := strings.Join([]string{
			`{"event":"epic_created","id":"E-001","format_id":"INF-EPC-001","title":"Test Epic","status":"draft","area_type":"INF","work_type":"FEAT","domain":"GENL","timestamp":"2024-01-01T00:00:00Z"}`,
			`{"event":"task_created","id":"T-001","format_id":"INF-TSK-001-001","epic_id":"E-001","title":"Test Task","status":"todo","area_type":"INF","work_type":"FEAT","domain":"GENL","origin":"planned","timestamp":"2024-01-01T00:00:00Z"}`,
		}, "\n")
		if err := os.WriteFile(filepath.Join(ledgerDir, FileWorkGraph), []byte(wgContent), 0o644); err != nil {
			t.Fatalf("writing work-graph.jsonl: %v", err)
		}

		// Write sessions.jsonl with valid user_id FK.
		sessContent := `{"event":"session_start","session_id":"ses-test-123","user_id":"user-test","user_host":"localhost","timestamp":"2024-01-01T00:00:00Z"}` + "\n"
		if err := os.WriteFile(filepath.Join(ledgerDir, FileSessions), []byte(sessContent), 0o644); err != nil {
			t.Fatalf("writing sessions.jsonl: %v", err)
		}

		// Create empty files for the other canonical files.
		for _, name := range []string{FileMemoryEvents, FileConfig} {
			if err := os.WriteFile(filepath.Join(ledgerDir, name), []byte(""), 0o644); err != nil {
				t.Fatalf("writing %s: %v", name, err)
			}
		}

		result, err := d.SyncFromJSONL(ctx, ledgerDir, nil)
		if err != nil {
			t.Fatalf("SyncFromJSONL: %v", err)
		}

		if result.FilesProcessed != 4 {
			t.Errorf("FilesProcessed = %d, want 4", result.FilesProcessed)
		}
		if result.EventsApplied < 3 {
			t.Errorf("EventsApplied = %d, want >= 3", result.EventsApplied)
		}

		// Verify epic was created.
		epics, err := d.QueryToMaps(ctx, "SELECT id, title FROM epics WHERE id = 'E-001'")
		if err != nil {
			t.Fatalf("querying epics: %v", err)
		}
		if len(epics) != 1 {
			t.Errorf("got %d epics, want 1", len(epics))
		}
	})

	t.Run("handles missing ledger directory gracefully", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		result, err := d.SyncFromJSONL(ctx, "/nonexistent/ledger", nil)
		if err != nil {
			t.Fatalf("SyncFromJSONL should not error for missing dir: %v", err)
		}

		// All 4 files attempted, none found.
		if result.FilesProcessed != 4 {
			t.Errorf("FilesProcessed = %d, want 4", result.FilesProcessed)
		}
		if result.EventsRead != 0 {
			t.Errorf("EventsRead = %d, want 0", result.EventsRead)
		}
	})

	t.Run("records errors for malformed events", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		ledgerDir := t.TempDir()

		// Write malformed content to work-graph.jsonl.
		content := "not json\n{\"event\":\"epic_created\"}\n"
		if err := os.WriteFile(filepath.Join(ledgerDir, FileWorkGraph), []byte(content), 0o644); err != nil {
			t.Fatalf("writing: %v", err)
		}

		// Create empty files for the other canonical files.
		for _, name := range []string{FileMemoryEvents, FileSessions, FileConfig} {
			if err := os.WriteFile(filepath.Join(ledgerDir, name), []byte(""), 0o644); err != nil {
				t.Fatalf("writing %s: %v", name, err)
			}
		}

		result, err := d.SyncFromJSONL(ctx, ledgerDir, nil)
		if err != nil {
			t.Fatalf("SyncFromJSONL: %v", err)
		}

		if len(result.Errors) == 0 {
			t.Error("expected at least one error for malformed JSONL")
		}
		if result.EventsSkipped == 0 {
			t.Error("expected at least one skipped event")
		}
	})

	t.Run("unknown events are silently skipped", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		ledgerDir := t.TempDir()
		content := `{"event":"future_event_type","data":"something"}` + "\n"
		if err := os.WriteFile(filepath.Join(ledgerDir, FileWorkGraph), []byte(content), 0o644); err != nil {
			t.Fatalf("writing: %v", err)
		}
		for _, name := range []string{FileMemoryEvents, FileSessions, FileConfig} {
			if err := os.WriteFile(filepath.Join(ledgerDir, name), []byte(""), 0o644); err != nil {
				t.Fatalf("writing %s: %v", name, err)
			}
		}

		result, err := d.SyncFromJSONL(ctx, ledgerDir, nil)
		if err != nil {
			t.Fatalf("SyncFromJSONL: %v", err)
		}

		// Unknown events are applied without error (forward compatible).
		if result.EventsApplied != 1 {
			t.Errorf("EventsApplied = %d, want 1", result.EventsApplied)
		}
	})

	t.Run("config and work_claimed events accepted", func(t *testing.T) {
		d := newTestDB(t)
		ctx := t.Context()

		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}

		ledgerDir := t.TempDir()
		configContent := `{"event":"config_set","key":"test","value":"val"}` + "\n"
		if err := os.WriteFile(filepath.Join(ledgerDir, FileConfig), []byte(configContent), 0o644); err != nil {
			t.Fatalf("writing: %v", err)
		}
		sessContent := `{"event":"work_claimed","session_id":"ses-1"}` + "\n"
		if err := os.WriteFile(filepath.Join(ledgerDir, FileSessions), []byte(sessContent), 0o644); err != nil {
			t.Fatalf("writing: %v", err)
		}
		for _, name := range []string{FileWorkGraph, FileMemoryEvents} {
			if err := os.WriteFile(filepath.Join(ledgerDir, name), []byte(""), 0o644); err != nil {
				t.Fatalf("writing %s: %v", name, err)
			}
		}

		result, err := d.SyncFromJSONL(ctx, ledgerDir, nil)
		if err != nil {
			t.Fatalf("SyncFromJSONL: %v", err)
		}

		if result.EventsApplied != 2 {
			t.Errorf("EventsApplied = %d, want 2", result.EventsApplied)
		}
	})
}

func TestApplyEventRouting(t *testing.T) {
	t.Parallel()
	// Helper to create a test DB with schema initialized and a user for FK constraints.
	setupDB := func(t *testing.T) *DB {
		t.Helper()
		d := newTestDB(t)
		ctx := t.Context()
		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}
		// Create user for sessions FK.
		_, err := d.Execute(ctx,
			"INSERT INTO users (id, email, display_name) VALUES ('user-1', 'u@test.com', 'User')")
		if err != nil {
			t.Fatalf("inserting user: %v", err)
		}
		return d
	}

	t.Run("task_status_changed updates task status", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Create epic and task first (satisfying FK constraints).
		_, err := d.Execute(ctx,
			`INSERT INTO epics (id, format_id, title, area_type, work_type, domain)
			 VALUES ('E-1', 'INF-EPC-001', 'Epic', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
		_, err = d.Execute(ctx,
			`INSERT INTO tasks (id, format_id, epic_id, title, status, area_type, work_type, domain)
			 VALUES ('T-1', 'INF-TSK-001-001', 'E-1', 'Task', 'todo', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting task: %v", err)
		}

		event := NormalizedEvent{
			Event: "task_status_changed",
			Raw: map[string]any{
				"task_id":    "T-1",
				"new_status": "in_progress",
				"timestamp":  "2024-06-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		// Verify status updated.
		var status string
		if err := d.QueryRow(ctx, "SELECT status FROM tasks WHERE id = 'T-1'").Scan(&status); err != nil {
			t.Fatalf("querying task: %v", err)
		}
		if status != "in_progress" {
			t.Errorf("status = %q, want in_progress", status)
		}
	})

	t.Run("epic_status_changed updates epic status", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		_, err := d.Execute(ctx,
			`INSERT INTO epics (id, format_id, title, area_type, work_type, domain)
			 VALUES ('E-2', 'INF-EPC-002', 'Epic', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}

		event := NormalizedEvent{
			Event: "epic_status_changed",
			Raw: map[string]any{
				"epic_id":    "E-2",
				"new_status": "complete",
				"timestamp":  "2024-06-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var status string
		if err := d.QueryRow(ctx, "SELECT status FROM epics WHERE id = 'E-2'").Scan(&status); err != nil {
			t.Fatalf("querying epic: %v", err)
		}
		if status != "complete" {
			t.Errorf("status = %q, want complete", status)
		}
	})

	t.Run("session_end updates session record", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Create session first.
		_, err := d.Execute(ctx,
			`INSERT INTO sessions (id, user_id, user_host, started_at, status)
			 VALUES ('ses-1', 'user-1', 'localhost', '2024-01-01', 'active')`)
		if err != nil {
			t.Fatalf("inserting session: %v", err)
		}

		event := NormalizedEvent{
			Event: "session_end",
			Raw: map[string]any{
				"session_id": "ses-1",
				"status":     "completed",
				"timestamp":  "2024-01-01T01:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileSessions, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var status string
		if err := d.QueryRow(ctx, "SELECT status FROM sessions WHERE id = 'ses-1'").Scan(&status); err != nil {
			t.Fatalf("querying session: %v", err)
		}
		if status != "completed" {
			t.Errorf("status = %q, want completed", status)
		}
	})

	t.Run("progress inserts memory event", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Use event type "progress" which is routed to applyMemoryEvent
		// and also satisfies the CHECK constraint on memory_events.event_type.
		event := NormalizedEvent{
			Event: "progress",
			Raw: map[string]any{
				"event":       "progress",
				"id":          "mem-1",
				"event_type":  "progress",
				"domain":      "development",
				"data":        map[string]any{"note": "test memory"},
				"memory_type": "episodic",
				"timestamp":   "2024-01-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileMemoryEvents, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		count, err := d.CountRows(ctx, "memory_events")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}
		if count != 1 {
			t.Errorf("memory_events count = %d, want 1", count)
		}
	})

	t.Run("begin_work inserts active_work", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// begin_work with no task_id FK (task_id is nullable).
		event := NormalizedEvent{
			Event: "begin_work",
			Raw: map[string]any{
				"event":      "begin_work",
				"id":         "work-1",
				"topic":      "Test feature",
				"branch":     "feat/test",
				"session_id": "ses-1",
				"timestamp":  "2024-01-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		count, err := d.CountRows(ctx, "active_work")
		if err != nil {
			t.Fatalf("CountRows: %v", err)
		}
		if count != 1 {
			t.Errorf("active_work count = %d, want 1", count)
		}
	})

	t.Run("complete_work updates active_work status", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Create active_work first (topic is NOT NULL).
		_, err := d.Execute(ctx,
			`INSERT INTO active_work (id, topic, status, created_at, updated_at)
			 VALUES ('work-2', 'Test work', 'in_progress', '2024-01-01', '2024-01-01')`)
		if err != nil {
			t.Fatalf("inserting active_work: %v", err)
		}

		event := NormalizedEvent{
			Event: "complete_work",
			Raw: map[string]any{
				"event":     "complete_work",
				"id":        "work-2",
				"timestamp": "2024-01-01T01:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var status string
		if err := d.QueryRow(ctx, "SELECT status FROM active_work WHERE id = 'work-2'").Scan(&status); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if status != "complete" {
			t.Errorf("status = %q, want complete", status)
		}
	})

	t.Run("work_complete alternative event name", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		_, err := d.Execute(ctx,
			`INSERT INTO active_work (id, topic, status, created_at, updated_at)
			 VALUES ('work-3', 'Test work', 'in_progress', '2024-01-01', '2024-01-01')`)
		if err != nil {
			t.Fatalf("inserting active_work: %v", err)
		}

		event := NormalizedEvent{
			Event: "work_complete",
			Raw:   map[string]any{"work_id": "work-3", "timestamp": "2024-01-01T01:00:00Z"},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var status string
		if err := d.QueryRow(ctx, "SELECT status FROM active_work WHERE id = 'work-3'").Scan(&status); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if status != "complete" {
			t.Errorf("status = %q, want complete", status)
		}
	})

	t.Run("begin_work with work_id fallback", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "begin_work",
			Raw: map[string]any{
				"event":     "begin_work",
				"work_id":   "work-via-wid",
				"topic":     "Fallback ID test",
				"timestamp": "2024-01-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var id string
		if err := d.QueryRow(ctx, "SELECT id FROM active_work WHERE id = 'work-via-wid'").Scan(&id); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if id != "work-via-wid" {
			t.Errorf("id = %q, want work-via-wid", id)
		}
	})

	t.Run("session_end without session_id is no-op", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "session_end",
			Raw:   map[string]any{"timestamp": "2024-01-01T00:00:00Z"},
		}

		if err := d.applyEvent(ctx, FileSessions, event); err != nil {
			t.Fatalf("applyEvent should not error: %v", err)
		}
	})

	t.Run("memory event without id is no-op", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "progress",
			Raw:   map[string]any{"event": "progress", "data": "test"},
		}

		if err := d.applyEvent(ctx, FileMemoryEvents, event); err != nil {
			t.Fatalf("applyEvent should not error: %v", err)
		}

		count, _ := d.CountRows(ctx, "memory_events")
		if count != 0 {
			t.Errorf("memory_events count = %d, want 0 (skipped)", count)
		}
	})

	t.Run("begin_work without id is no-op", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "begin_work",
			Raw:   map[string]any{"event": "begin_work", "topic": "no id"},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent should not error: %v", err)
		}

		count, _ := d.CountRows(ctx, "active_work")
		if count != 0 {
			t.Errorf("active_work count = %d, want 0", count)
		}
	})

	t.Run("task_status_changed missing task_id returns error", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "task_status_changed",
			Raw:   map[string]any{"new_status": "done"},
		}

		err := d.applyEvent(ctx, FileWorkGraph, event)
		if err == nil {
			t.Error("expected error for missing task_id")
		}
	})

	t.Run("epic_status_changed missing epic_id returns error", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "epic_status_changed",
			Raw:   map[string]any{"new_status": "complete"},
		}

		err := d.applyEvent(ctx, FileWorkGraph, event)
		if err == nil {
			t.Error("expected error for missing epic_id")
		}
	})

	t.Run("pr_created updates task PR fields", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Create epic and task.
		_, err := d.Execute(ctx,
			`INSERT INTO epics (id, format_id, title, area_type, work_type, domain)
			 VALUES ('E-PR', 'INF-EPC-099', 'PR Epic', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
		_, err = d.Execute(ctx,
			`INSERT INTO tasks (id, format_id, epic_id, title, status, area_type, work_type, domain)
			 VALUES ('T-PR', 'INF-TSK-099-001', 'E-PR', 'PR Task', 'in_progress', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting task: %v", err)
		}

		event := NormalizedEvent{
			Event: "pr_created",
			Raw: map[string]any{
				"task_format_id": "INF-TSK-099-001",
				"pr_number":      float64(42),
				"pr_url":         "https://github.com/test/repo/pull/42",
				"timestamp":      "2024-06-01T00:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var prNumber int
		var externalURL string
		if err := d.QueryRow(ctx,
			"SELECT pr_number, external_url FROM tasks WHERE format_id = 'INF-TSK-099-001'",
		).Scan(&prNumber, &externalURL); err != nil {
			t.Fatalf("querying task: %v", err)
		}
		if prNumber != 42 {
			t.Errorf("pr_number = %d, want 42", prNumber)
		}
		if externalURL != "https://github.com/test/repo/pull/42" {
			t.Errorf("external_url = %q, want PR URL", externalURL)
		}
	})

	t.Run("pr_created without task_format_id is no-op", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "pr_created",
			Raw:   map[string]any{"pr_number": float64(1)},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent should not error: %v", err)
		}
	})

	t.Run("pr_merged marks task complete", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		// Create epic and task.
		_, err := d.Execute(ctx,
			`INSERT INTO epics (id, format_id, title, area_type, work_type, domain)
			 VALUES ('E-MRG', 'INF-EPC-098', 'Merge Epic', 'INF', 'FIX', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
		_, err = d.Execute(ctx,
			`INSERT INTO tasks (id, format_id, epic_id, title, status, area_type, work_type, domain)
			 VALUES ('T-MRG', 'INF-TSK-098-001', 'E-MRG', 'Merge Task', 'in_progress', 'INF', 'FIX', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting task: %v", err)
		}

		event := NormalizedEvent{
			Event: "pr_merged",
			Raw: map[string]any{
				"task_format_id": "INF-TSK-098-001",
				"timestamp":      "2024-06-01T12:00:00Z",
			},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}

		var status, completedAt string
		if err := d.QueryRow(ctx,
			"SELECT status, completed_at FROM tasks WHERE format_id = 'INF-TSK-098-001'",
		).Scan(&status, &completedAt); err != nil {
			t.Fatalf("querying task: %v", err)
		}
		if status != "complete" {
			t.Errorf("status = %q, want complete", status)
		}
		if completedAt != "2024-06-01T12:00:00Z" {
			t.Errorf("completed_at = %q, want 2024-06-01T12:00:00Z", completedAt)
		}
	})

	t.Run("pr_merged without task_format_id is no-op", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "pr_merged",
			Raw:   map[string]any{"timestamp": "2024-06-01T00:00:00Z"},
		}

		if err := d.applyEvent(ctx, FileWorkGraph, event); err != nil {
			t.Fatalf("applyEvent should not error: %v", err)
		}
	})

	t.Run("config_set event is accepted silently", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		event := NormalizedEvent{
			Event: "config_set",
			Raw:   map[string]any{"key": "test", "value": "val"},
		}

		if err := d.applyEvent(ctx, FileConfig, event); err != nil {
			t.Fatalf("applyEvent: %v", err)
		}
	})
}

func TestCanonicalFiles(t *testing.T) {
	t.Parallel()
	files := CanonicalFiles()

	if len(files) != 4 {
		t.Fatalf("got %d files, want 4", len(files))
	}

	expected := map[string]bool{
		FileWorkGraph:    false,
		FileMemoryEvents: false,
		FileSessions:     false,
		FileConfig:       false,
	}

	for _, f := range files {
		if _, ok := expected[f]; !ok {
			t.Errorf("unexpected file: %s", f)
		}
		expected[f] = true
	}

	for name, found := range expected {
		if !found {
			t.Errorf("missing expected file: %s", name)
		}
	}
}

func TestHelperFunctions(t *testing.T) {
	t.Parallel()
	t.Run("getString returns value for existing key", func(t *testing.T) {
		m := map[string]any{"key": "value"}
		if got := getString(m, "key"); got != "value" {
			t.Errorf("getString = %q, want value", got)
		}
	})

	t.Run("getString returns empty for missing key", func(t *testing.T) {
		m := map[string]any{}
		if got := getString(m, "key"); got != "" {
			t.Errorf("getString = %q, want empty", got)
		}
	})

	t.Run("getString returns empty for non-string value", func(t *testing.T) {
		m := map[string]any{"key": 42}
		if got := getString(m, "key"); got != "" {
			t.Errorf("getString = %q, want empty for int", got)
		}
	})

	t.Run("getStringDefault returns default for missing key", func(t *testing.T) {
		m := map[string]any{}
		if got := getStringDefault(m, "key", "fallback"); got != "fallback" {
			t.Errorf("getStringDefault = %q, want fallback", got)
		}
	})

	t.Run("getStringDefault returns value when present", func(t *testing.T) {
		m := map[string]any{"key": "real"}
		if got := getStringDefault(m, "key", "fallback"); got != "real" {
			t.Errorf("getStringDefault = %q, want real", got)
		}
	})

	t.Run("getBool handles bool type", func(t *testing.T) {
		m := map[string]any{"flag": true}
		if got := getBool(m, "flag"); !got {
			t.Error("getBool = false, want true")
		}
	})

	t.Run("getBool handles string true", func(t *testing.T) {
		m := map[string]any{"flag": "true"}
		if got := getBool(m, "flag"); !got {
			t.Error("getBool = false for string 'true', want true")
		}
	})

	t.Run("getBool returns false for missing key", func(t *testing.T) {
		m := map[string]any{}
		if got := getBool(m, "flag"); got {
			t.Error("getBool = true for missing key, want false")
		}
	})

	t.Run("getJSONString returns string value", func(t *testing.T) {
		m := map[string]any{"key": "simple"}
		if got := getJSONString(m, "key"); got != "simple" {
			t.Errorf("getJSONString = %q, want simple", got)
		}
	})

	t.Run("getJSONString marshals non-string", func(t *testing.T) {
		m := map[string]any{"key": []string{"a", "b"}}
		got := getJSONString(m, "key")
		if got != `["a","b"]` {
			t.Errorf("getJSONString = %q, want [\"a\",\"b\"]", got)
		}
	})

	t.Run("getJSONString returns empty for missing key", func(t *testing.T) {
		m := map[string]any{}
		if got := getJSONString(m, "key"); got != "" {
			t.Errorf("getJSONString = %q, want empty", got)
		}
	})
}

func TestApplyTaskCreated(t *testing.T) {
	t.Parallel()
	setupDB := func(t *testing.T) *DB {
		t.Helper()
		d := newTestDB(t)
		ctx := t.Context()
		if err := d.InitFromSchema(ctx); err != nil {
			t.Fatalf("InitFromSchema: %v", err)
		}
		// Create domain to satisfy FK constraint on tasks.domain.
		_, err := d.Execute(ctx,
			"INSERT OR IGNORE INTO domains (code, name) VALUES ('GENL', 'General')")
		if err != nil {
			t.Fatalf("inserting domain: %v", err)
		}
		// Create an epic to satisfy FK constraint on tasks.epic_id.
		_, err = d.Execute(ctx,
			`INSERT INTO epics (id, format_id, title, area_type, work_type, domain)
			 VALUES ('E-TC', 'INF-EPC-100', 'Test Epic', 'INF', 'FEAT', 'GENL')`)
		if err != nil {
			t.Fatalf("inserting epic: %v", err)
		}
		return d
	}

	t.Run("missing id returns error", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		err := d.applyTaskCreated(ctx, map[string]any{
			"epic_id": "E-TC",
			"title":   "No ID task",
		})
		if err == nil {
			t.Fatal("expected error for missing id")
		}
		if !strings.Contains(err.Error(), "missing id") {
			t.Errorf("error = %v, want 'missing id'", err)
		}
	})

	t.Run("missing epic_id returns error", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		err := d.applyTaskCreated(ctx, map[string]any{
			"id":    "T-TC-1",
			"title": "No epic task",
		})
		if err == nil {
			t.Fatal("expected error for missing epic_id")
		}
		if !strings.Contains(err.Error(), "missing epic_id") {
			t.Errorf("error = %v, want 'missing epic_id'", err)
		}
	})

	t.Run("format_id falls back to id", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		err := d.applyTaskCreated(ctx, map[string]any{
			"id":        "T-TC-2",
			"epic_id":   "E-TC",
			"title":     "Fallback format_id",
			"area_type": "INF",
			"work_type": "FEAT",
			"domain":    "GENL",
			"timestamp": "2024-01-01T00:00:00Z",
		})
		if err != nil {
			t.Fatalf("applyTaskCreated: %v", err)
		}

		var formatID string
		if err := d.QueryRow(ctx, "SELECT format_id FROM tasks WHERE id = 'T-TC-2'").Scan(&formatID); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if formatID != "T-TC-2" {
			t.Errorf("format_id = %q, want T-TC-2 (fallback to id)", formatID)
		}
	})

	t.Run("explicit format_id is used", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		err := d.applyTaskCreated(ctx, map[string]any{
			"id":        "T-TC-3",
			"format_id": "INF-TSK-100-003",
			"epic_id":   "E-TC",
			"title":     "Explicit format_id",
			"area_type": "INF",
			"work_type": "FEAT",
			"domain":    "GENL",
			"timestamp": "2024-01-01T00:00:00Z",
		})
		if err != nil {
			t.Fatalf("applyTaskCreated: %v", err)
		}

		var formatID string
		if err := d.QueryRow(ctx, "SELECT format_id FROM tasks WHERE id = 'T-TC-3'").Scan(&formatID); err != nil {
			t.Fatalf("querying: %v", err)
		}
		if formatID != "INF-TSK-100-003" {
			t.Errorf("format_id = %q, want INF-TSK-100-003", formatID)
		}
	})

	t.Run("default values applied for optional fields", func(t *testing.T) {
		d := setupDB(t)
		ctx := t.Context()

		err := d.applyTaskCreated(ctx, map[string]any{
			"id":        "T-TC-4",
			"epic_id":   "E-TC",
			"title":     "Defaults test",
			"area_type": "INF",
			"work_type": "FEAT",
			"domain":    "GENL",
			"timestamp": "2024-01-01T00:00:00Z",
		})
		if err != nil {
			t.Fatalf("applyTaskCreated: %v", err)
		}

		var status, origin, priority string
		err = d.QueryRow(ctx,
			"SELECT status, origin, priority FROM tasks WHERE id = 'T-TC-4'",
		).Scan(&status, &origin, &priority)
		if err != nil {
			t.Fatalf("querying: %v", err)
		}
		if status != "todo" {
			t.Errorf("status = %q, want todo", status)
		}
		if origin != "planned" {
			t.Errorf("origin = %q, want planned", origin)
		}
		if priority != "normal" {
			t.Errorf("priority = %q, want normal", priority)
		}
	})
}
