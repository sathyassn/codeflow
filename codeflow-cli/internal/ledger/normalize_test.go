package ledger

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestNormalizeRecord(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		input       map[string]any
		wantEvent   string
		wantRenamed int
		wantKeys    map[string]string // key -> expected value (string)
		wantAbsent  []string          // keys that must NOT exist
	}{
		{
			name:        "canonical event unchanged",
			input:       map[string]any{"event": "epic_created", "timestamp": "2024-01-01T00:00:00Z", "id": "E-001"},
			wantEvent:   "epic_created",
			wantRenamed: 0,
			wantKeys:    map[string]string{"event": "epic_created", "timestamp": "2024-01-01T00:00:00Z"},
		},
		{
			name:        "type renamed to event",
			input:       map[string]any{"type": "task_created", "timestamp": "2024-01-01T00:00:00Z"},
			wantEvent:   "task_created",
			wantRenamed: 1,
			wantKeys:    map[string]string{"event": "task_created"},
			wantAbsent:  []string{"type"},
		},
		{
			name:        "type memory_stored normalized to memory_store",
			input:       map[string]any{"type": "memory_stored", "id": "M-001"},
			wantEvent:   "memory_store",
			wantRenamed: 1,
			wantKeys:    map[string]string{"event": "memory_store"},
			wantAbsent:  []string{"type"},
		},
		{
			name:        "e shorthand renamed to event",
			input:       map[string]any{"e": "session_start", "ts": "2024-01-01T00:00:00Z", "sid": "ses-123"},
			wantEvent:   "session_start",
			wantRenamed: 3, // e->event, ts->timestamp, sid->session_id
			wantKeys:    map[string]string{"event": "session_start", "timestamp": "2024-01-01T00:00:00Z", "session_id": "ses-123"},
			wantAbsent:  []string{"e", "ts", "sid"},
		},
		{
			name:        "ts renamed to timestamp",
			input:       map[string]any{"event": "task_created", "ts": "2024-06-01T00:00:00Z"},
			wantEvent:   "task_created",
			wantRenamed: 1,
			wantKeys:    map[string]string{"timestamp": "2024-06-01T00:00:00Z"},
			wantAbsent:  []string{"ts"},
		},
		{
			name:        "sid renamed to session_id",
			input:       map[string]any{"event": "session_start", "sid": "ses-456"},
			wantEvent:   "session_start",
			wantRenamed: 1,
			wantKeys:    map[string]string{"session_id": "ses-456"},
			wantAbsent:  []string{"sid"},
		},
		{
			name:        "wid renamed to work_id",
			input:       map[string]any{"event": "begin_work", "wid": "W-001"},
			wantEvent:   "begin_work",
			wantRenamed: 1,
			wantKeys:    map[string]string{"work_id": "W-001"},
			wantAbsent:  []string{"wid"},
		},
		{
			name:        "from_status and to_status renamed",
			input:       map[string]any{"event": "task_status_changed", "from_status": "todo", "to_status": "in_progress"},
			wantEvent:   "task_status_changed",
			wantRenamed: 2,
			wantKeys:    map[string]string{"old_status": "todo", "new_status": "in_progress"},
			wantAbsent:  []string{"from_status", "to_status"},
		},
		{
			name:        "op+table mapped to semantic event",
			input:       map[string]any{"op": "INSERT", "table": "epics", "id": "E-001"},
			wantEvent:   "epic_created",
			wantRenamed: 1,
			wantKeys:    map[string]string{"event": "epic_created"},
			wantAbsent:  []string{"op", "table"},
		},
		{
			name:        "op+table INSERT memory_events mapped to memory_store",
			input:       map[string]any{"op": "INSERT", "table": "memory_events", "id": "M-001"},
			wantEvent:   "memory_store",
			wantRenamed: 1,
			wantKeys:    map[string]string{"event": "memory_store"},
			wantAbsent:  []string{"op", "table"},
		},
		{
			name:        "no event type returns empty",
			input:       map[string]any{"id": "orphan", "data": "test"},
			wantEvent:   "",
			wantRenamed: 0,
		},
		{
			name:        "does not overwrite existing canonical fields",
			input:       map[string]any{"event": "epic_created", "sid": "ses-1", "session_id": "ses-existing"},
			wantEvent:   "epic_created",
			wantRenamed: 0, // sid rename blocked because session_id exists
			wantKeys:    map[string]string{"session_id": "ses-existing"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			// Make a copy to avoid mutating test data.
			raw := make(map[string]any, len(tt.input))
			for k, v := range tt.input {
				raw[k] = v
			}

			renamed, event := normalizeRecord(raw)

			if event != tt.wantEvent {
				t.Errorf("event = %q, want %q", event, tt.wantEvent)
			}
			if renamed != tt.wantRenamed {
				t.Errorf("renamed = %d, want %d", renamed, tt.wantRenamed)
			}

			for key, wantVal := range tt.wantKeys {
				got, ok := raw[key].(string)
				if !ok {
					t.Errorf("key %q not found or not string in result", key)
					continue
				}
				if got != wantVal {
					t.Errorf("raw[%q] = %q, want %q", key, got, wantVal)
				}
			}

			for _, key := range tt.wantAbsent {
				if _, exists := raw[key]; exists {
					t.Errorf("key %q should not exist in result", key)
				}
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
			t.Parallel()
			got := normalizeEventName(tt.input)
			if got != tt.want {
				t.Errorf("normalizeEventName(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestMapOpToEventName(t *testing.T) {
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
			t.Parallel()
			got := mapOpToEventName(tt.op, tt.table)
			if got != tt.want {
				t.Errorf("mapOpToEventName(%q, %q) = %q, want %q", tt.op, tt.table, got, tt.want)
			}
		})
	}
}

func TestNormalizeFile(t *testing.T) {
	t.Parallel()

	t.Run("renames type to event and ts to timestamp", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "memory-events.jsonl")

		content := strings.Join([]string{
			`{"type":"memory_stored","ts":"2024-01-01T00:00:00Z","id":"M-001","data":{"note":"test"}}`,
			`{"type":"progress","ts":"2024-01-02T00:00:00Z","id":"M-002"}`,
		}, "\n") + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.RecordsRead != 2 {
			t.Errorf("RecordsRead = %d, want 2", result.RecordsRead)
		}
		if result.RecordsWritten != 2 {
			t.Errorf("RecordsWritten = %d, want 2", result.RecordsWritten)
		}
		if result.FieldsRenamed < 4 {
			t.Errorf("FieldsRenamed = %d, want >= 4 (2 type + 2 ts)", result.FieldsRenamed)
		}

		// Verify file contents.
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading result: %v", err)
		}
		normalized := string(data)
		if strings.Contains(normalized, `"type"`) {
			t.Error("output still contains 'type' key")
		}
		if strings.Contains(normalized, `"ts"`) {
			t.Error("output still contains 'ts' key")
		}
		if !strings.Contains(normalized, `"event"`) {
			t.Error("output missing 'event' key")
		}
		if !strings.Contains(normalized, `"timestamp"`) {
			t.Error("output missing 'timestamp' key")
		}
		if !strings.Contains(normalized, `"memory_store"`) {
			t.Error("output missing normalized event name 'memory_store'")
		}
	})

	t.Run("drops records with no resolvable event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "memory-events.jsonl")

		content := strings.Join([]string{
			`{"event":"memory_store","id":"M-001","timestamp":"2024-01-01T00:00:00Z"}`,
			`{"id":"orphan","data":"no event type"}`,
			`{"event":"progress","id":"M-002","timestamp":"2024-01-02T00:00:00Z"}`,
		}, "\n") + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.RecordsRead != 3 {
			t.Errorf("RecordsRead = %d, want 3", result.RecordsRead)
		}
		if result.RecordsWritten != 2 {
			t.Errorf("RecordsWritten = %d, want 2", result.RecordsWritten)
		}
		if result.RecordsDropped != 1 {
			t.Errorf("RecordsDropped = %d, want 1", result.RecordsDropped)
		}
	})

	t.Run("maps op+table to semantic event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "memory-events.jsonl")

		content := `{"op":"INSERT","table":"memory_events","id":"M-001","ts":"2024-01-01T00:00:00Z"}` + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.RecordsWritten != 1 {
			t.Errorf("RecordsWritten = %d, want 1", result.RecordsWritten)
		}

		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading result: %v", err)
		}

		var m map[string]any
		if err := json.Unmarshal(bytes.TrimSpace(data), &m); err != nil {
			t.Fatalf("parsing output: %v", err)
		}
		if m["event"] != "memory_store" {
			t.Errorf("event = %v, want memory_store", m["event"])
		}
		if _, exists := m["op"]; exists {
			t.Error("output still contains 'op' key")
		}
		if _, exists := m["table"]; exists {
			t.Error("output still contains 'table' key")
		}
	})

	t.Run("empty file is no-op", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "empty.jsonl")

		if err := os.WriteFile(path, []byte(""), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.RecordsRead != 0 {
			t.Errorf("RecordsRead = %d, want 0", result.RecordsRead)
		}
		if result.RecordsWritten != 0 {
			t.Errorf("RecordsWritten = %d, want 0", result.RecordsWritten)
		}
	})

	t.Run("dry-run does not modify file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "memory-events.jsonl")

		original := `{"type":"memory_stored","ts":"2024-01-01T00:00:00Z","id":"M-001"}` + "\n"
		if err := os.WriteFile(path, []byte(original), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{DryRun: true})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.FieldsRenamed == 0 {
			t.Error("expected fields to be reported as renamed in dry-run")
		}

		// File should be unchanged.
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading file: %v", err)
		}
		if string(data) != original {
			t.Errorf("file was modified in dry-run mode")
		}
	})

	t.Run("idempotent (normalize twice = same output)", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "memory-events.jsonl")

		content := strings.Join([]string{
			`{"type":"memory_stored","ts":"2024-01-01T00:00:00Z","id":"M-001"}`,
			`{"event":"progress","timestamp":"2024-01-02T00:00:00Z","id":"M-002"}`,
			`{"e":"session_start","sid":"ses-123","ts":"2024-01-03T00:00:00Z"}`,
		}, "\n") + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		// First normalization.
		_, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("first NormalizeFile: %v", err)
		}

		first, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading after first: %v", err)
		}

		// Second normalization.
		result2, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("second NormalizeFile: %v", err)
		}

		second, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading after second: %v", err)
		}

		if string(first) != string(second) {
			t.Errorf("idempotency failed: output differs between runs\nfirst:\n%s\nsecond:\n%s", first, second)
		}

		// Second run should report zero renames since everything is already canonical.
		if result2.FieldsRenamed != 0 {
			t.Errorf("second run FieldsRenamed = %d, want 0", result2.FieldsRenamed)
		}
		if result2.RecordsDropped != 0 {
			t.Errorf("second run RecordsDropped = %d, want 0", result2.RecordsDropped)
		}
	})

	t.Run("malformed JSON preserved with warning", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "events.jsonl")

		content := strings.Join([]string{
			`{"event":"good","id":"1","timestamp":"2024-01-01T00:00:00Z"}`,
			`not valid json at all`,
			`{"event":"also_good","id":"2","timestamp":"2024-01-02T00:00:00Z"}`,
		}, "\n") + "\n"

		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		result, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		if result.RecordsRead != 3 {
			t.Errorf("RecordsRead = %d, want 3", result.RecordsRead)
		}
		if result.RecordsWritten != 3 {
			t.Errorf("RecordsWritten = %d, want 3 (including malformed)", result.RecordsWritten)
		}
		if result.MalformedLines != 1 {
			t.Errorf("MalformedLines = %d, want 1", result.MalformedLines)
		}

		// Verify malformed line is preserved.
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("reading result: %v", err)
		}
		if !strings.Contains(string(data), "not valid json at all") {
			t.Error("malformed line was not preserved in output")
		}
	})

	t.Run("nonexistent file returns error", func(t *testing.T) {
		t.Parallel()
		_, err := NormalizeFile("/nonexistent/path.jsonl", NormalizeConfig{})
		if err == nil {
			t.Fatal("expected error for nonexistent file")
		}
	})

	t.Run("preserves file permissions", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		path := filepath.Join(dir, "events.jsonl")

		content := `{"event":"test","id":"1","timestamp":"2024-01-01T00:00:00Z"}` + "\n"
		if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
			t.Fatalf("writing file: %v", err)
		}

		_, err := NormalizeFile(path, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeFile: %v", err)
		}

		info, err := os.Stat(path)
		if err != nil {
			t.Fatalf("stat: %v", err)
		}
		if info.Mode().Perm() != 0o600 {
			t.Errorf("permissions = %o, want 600", info.Mode().Perm())
		}
	})
}

func TestNormalizeFile_BlankLinesPreserved(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	path := filepath.Join(dir, "events.jsonl")

	content := strings.Join([]string{
		`{"event":"epic_created","id":"E-001","timestamp":"2024-01-01T00:00:00Z"}`,
		"",
		`{"event":"task_created","id":"T-001","timestamp":"2024-01-02T00:00:00Z","epic_id":"E-001","title":"T1"}`,
		"",
	}, "\n") + "\n"

	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writing file: %v", err)
	}

	result, err := NormalizeFile(path, NormalizeConfig{})
	if err != nil {
		t.Fatalf("NormalizeFile: %v", err)
	}

	// Blank lines are not counted as records.
	if result.RecordsRead != 2 {
		t.Errorf("RecordsRead = %d, want 2", result.RecordsRead)
	}
	if result.RecordsWritten != 2 {
		t.Errorf("RecordsWritten = %d, want 2", result.RecordsWritten)
	}

	// Verify blank lines are preserved in output.
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("reading result: %v", err)
	}
	lines := strings.Split(strings.TrimSuffix(string(data), "\n"), "\n")
	// Expect: record, blank, record, blank = 4 lines.
	if len(lines) != 4 {
		t.Errorf("got %d lines, want 4 (2 records + 2 blank)", len(lines))
	}
	if lines[1] != "" {
		t.Errorf("line 2 should be blank, got: %q", lines[1])
	}
	if lines[3] != "" {
		t.Errorf("line 4 should be blank, got: %q", lines[3])
	}
}

func TestNormalizeFile_ReadOnlyDir(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	path := filepath.Join(dir, "events.jsonl")

	content := `{"event":"test","id":"1","timestamp":"2024-01-01T00:00:00Z"}` + "\n"
	if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
		t.Fatalf("writing file: %v", err)
	}

	// Make directory read-only to prevent temp file creation.
	if err := os.Chmod(dir, 0o555); err != nil {
		t.Fatalf("chmod: %v", err)
	}
	t.Cleanup(func() { os.Chmod(dir, 0o755) })

	_, err := NormalizeFile(path, NormalizeConfig{})
	if err == nil {
		t.Fatal("expected error when directory is read-only")
	}
	if !strings.Contains(err.Error(), "creating temp file") {
		t.Errorf("error should mention creating temp file: %v", err)
	}
}

func TestNormalizeAll_ErrorPropagation(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()

	// Create a directory with the same name as a canonical file to cause an open error.
	if err := os.MkdirAll(filepath.Join(dir, FileWorkGraph), 0o755); err != nil {
		t.Fatalf("creating directory: %v", err)
	}

	results, err := NormalizeAll(dir, NormalizeConfig{})
	if err == nil {
		t.Fatal("expected error when canonical file is a directory")
	}
	if !strings.Contains(err.Error(), "normalizing") {
		t.Errorf("error should mention normalizing: %v", err)
	}
	// Partial results may be returned before the error.
	_ = results
}

func TestNormalizeAll(t *testing.T) {
	t.Parallel()

	t.Run("normalizes all canonical files", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		// Create all 4 canonical files.
		files := map[string]string{
			FileWorkGraph:    `{"event":"epic_created","id":"E-001","timestamp":"2024-01-01T00:00:00Z"}` + "\n",
			FileMemoryEvents: `{"type":"memory_stored","ts":"2024-01-01T00:00:00Z","id":"M-001"}` + "\n",
			FileSessions:     `{"event":"session_start","session_id":"ses-1","timestamp":"2024-01-01T00:00:00Z"}` + "\n",
			FileConfig:       `{"event":"config_set","timestamp":"2024-01-01T00:00:00Z","key":"k","value":"v"}` + "\n",
		}

		for name, content := range files {
			if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
				t.Fatalf("writing %s: %v", name, err)
			}
		}

		results, err := NormalizeAll(dir, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeAll: %v", err)
		}

		if len(results) != 4 {
			t.Errorf("got %d results, want 4", len(results))
		}

		// Verify memory-events.jsonl was normalized.
		data, err := os.ReadFile(filepath.Join(dir, FileMemoryEvents))
		if err != nil {
			t.Fatalf("reading memory-events: %v", err)
		}
		if strings.Contains(string(data), `"type"`) {
			t.Error("memory-events still contains 'type' key")
		}
	})

	t.Run("skips missing files", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		// Create only one file.
		content := `{"event":"epic_created","id":"E-001","timestamp":"2024-01-01T00:00:00Z"}` + "\n"
		if err := os.WriteFile(filepath.Join(dir, FileWorkGraph), []byte(content), 0o644); err != nil {
			t.Fatalf("writing: %v", err)
		}

		results, err := NormalizeAll(dir, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeAll: %v", err)
		}

		if len(results) != 1 {
			t.Errorf("got %d results, want 1 (only existing file)", len(results))
		}
	})

	t.Run("empty directory returns no results", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		results, err := NormalizeAll(dir, NormalizeConfig{})
		if err != nil {
			t.Fatalf("NormalizeAll: %v", err)
		}

		if len(results) != 0 {
			t.Errorf("got %d results, want 0", len(results))
		}
	})
}

func TestPrintResults(t *testing.T) {
	t.Parallel()

	t.Run("prints summary for dry-run", func(t *testing.T) {
		t.Parallel()
		results := []*NormalizeResult{
			{File: "memory-events.jsonl", RecordsRead: 10, RecordsWritten: 8, RecordsDropped: 2, FieldsRenamed: 5},
			{File: "work-graph.jsonl", RecordsRead: 5, RecordsWritten: 5, RecordsDropped: 0, FieldsRenamed: 0},
		}

		var buf bytes.Buffer
		PrintResults(&buf, results, true)
		output := buf.String()

		if !strings.Contains(output, "DRY RUN") {
			t.Error("dry-run output missing DRY RUN header")
		}
		if !strings.Contains(output, "memory-events.jsonl") {
			t.Error("output missing file name")
		}
		if !strings.Contains(output, "Total: 2 files") {
			t.Error("output missing total summary")
		}
	})

	t.Run("prints summary for normal run", func(t *testing.T) {
		t.Parallel()
		results := []*NormalizeResult{
			{File: "test.jsonl", RecordsRead: 3, RecordsWritten: 3, RecordsDropped: 0, FieldsRenamed: 2, MalformedLines: 1, Warnings: []string{"line 2: malformed"}},
		}

		var buf bytes.Buffer
		PrintResults(&buf, results, false)
		output := buf.String()

		if strings.Contains(output, "DRY RUN") {
			t.Error("normal run should not contain DRY RUN header")
		}
		if !strings.Contains(output, "1 malformed") {
			t.Error("output missing malformed count")
		}
		if !strings.Contains(output, "WARN: line 2: malformed") {
			t.Error("output missing warning")
		}
	})
}

func TestRenameFieldIfPresent(t *testing.T) {
	t.Parallel()

	t.Run("renames existing field", func(t *testing.T) {
		t.Parallel()
		m := map[string]any{"old": "value"}
		got := renameFieldIfPresent(m, "old", "new")

		if got != 1 {
			t.Errorf("got %d, want 1", got)
		}
		if _, exists := m["old"]; exists {
			t.Error("old key should not exist")
		}
		if m["new"] != "value" {
			t.Errorf("new = %v, want value", m["new"])
		}
	})

	t.Run("no-op when old key absent", func(t *testing.T) {
		t.Parallel()
		m := map[string]any{"other": "value"}
		got := renameFieldIfPresent(m, "old", "new")

		if got != 0 {
			t.Errorf("got %d, want 0", got)
		}
	})

	t.Run("no-op when new key exists", func(t *testing.T) {
		t.Parallel()
		m := map[string]any{"old": "original", "new": "existing"}
		got := renameFieldIfPresent(m, "old", "new")

		if got != 0 {
			t.Errorf("got %d, want 0", got)
		}
		if m["new"] != "existing" {
			t.Errorf("new = %v, want existing", m["new"])
		}
	})
}
