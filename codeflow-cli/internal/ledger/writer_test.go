package ledger

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"
)

// fixedTime returns a deterministic time.Time for tests.
func fixedTime() time.Time {
	return time.Date(2026, 2, 28, 14, 0, 0, 0, time.UTC)
}

// newTestWriter creates a Writer pointing at a temp dir with a fixed clock.
func newTestWriter(t *testing.T) *Writer {
	t.Helper()
	dir := filepath.Join(t.TempDir(), "ledger")
	w, err := NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter: %v", err)
	}
	w.Now = fixedTime
	return w
}

func TestNewWriter(t *testing.T) {
	t.Parallel()

	dir := filepath.Join(t.TempDir(), "ledger")
	w, err := NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter: %v", err)
	}
	if w.Dir() != dir {
		t.Errorf("Dir() = %q, want %q", w.Dir(), dir)
	}

	// Verify directory was created.
	info, err := os.Stat(dir)
	if err != nil {
		t.Fatalf("Stat(%q): %v", dir, err)
	}
	if !info.IsDir() {
		t.Errorf("%q is not a directory", dir)
	}
}

func TestNewWriter_DefaultNow(t *testing.T) {
	t.Parallel()

	dir := filepath.Join(t.TempDir(), "ledger")
	w, err := NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter: %v", err)
	}

	// Default Now should return a time close to now.
	got := w.Now()
	if time.Since(got) > 5*time.Second {
		t.Errorf("default Now() returned %v, expected close to current time", got)
	}
}

func TestAppendEventHappyPath(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-test123",
		Data: map[string]any{
			"user_host": "laptop",
		},
	}

	if err := w.AppendEvent(event); err != nil {
		t.Fatalf("AppendEvent: %v", err)
	}

	// Read the file and verify contents.
	data, err := os.ReadFile(filepath.Join(w.Dir(), FileSessions))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 1 {
		t.Fatalf("expected 1 line, got %d", len(lines))
	}

	var m map[string]any
	if err := json.Unmarshal([]byte(lines[0]), &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if m["event"] != "session_start" {
		t.Errorf("event = %v, want session_start", m["event"])
	}
	if m["session_id"] != "ses-test123" {
		t.Errorf("session_id = %v, want ses-test123", m["session_id"])
	}
	if m["timestamp"] != "2026-02-28T14:00:00Z" {
		t.Errorf("timestamp = %v, want 2026-02-28T14:00:00Z", m["timestamp"])
	}
}

func TestAppendEventToAllFourFiles(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	events := []Event{
		{EventType: "session_start", Timestamp: "2026-02-28T14:00:00Z", SessionID: "ses-1"},
		{EventType: "task_created", Timestamp: "2026-02-28T14:00:01Z", Data: map[string]any{"id": "t-1", "epic_id": "e-1", "title": "T1"}},
		{EventType: "memory_store", Timestamp: "2026-02-28T14:00:02Z", Data: map[string]any{"id": "m-1"}},
		{EventType: "config_set", Timestamp: "2026-02-28T14:00:03Z"},
	}

	expectedFiles := []string{FileSessions, FileWorkGraph, FileMemoryEvents, FileConfig}

	for i, event := range events {
		if err := w.AppendEvent(event); err != nil {
			t.Fatalf("AppendEvent[%d]: %v", i, err)
		}
	}

	// Verify each canonical file has exactly one line.
	for _, filename := range expectedFiles {
		data, err := os.ReadFile(filepath.Join(w.Dir(), filename))
		if err != nil {
			t.Fatalf("ReadFile(%q): %v", filename, err)
		}
		lines := strings.Split(strings.TrimSpace(string(data)), "\n")
		if len(lines) != 1 {
			t.Errorf("%s: expected 1 line, got %d", filename, len(lines))
		}
	}
}

func TestAppendEventCreatesFileIfMissing(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// Verify file doesn't exist yet.
	filePath := filepath.Join(w.Dir(), FileSessions)
	if _, err := os.Stat(filePath); !os.IsNotExist(err) {
		t.Fatalf("expected file to not exist, got err: %v", err)
	}

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-test",
	}
	if err := w.AppendEvent(event); err != nil {
		t.Fatalf("AppendEvent: %v", err)
	}

	// Verify file now exists.
	if _, err := os.Stat(filePath); err != nil {
		t.Fatalf("file should exist after append: %v", err)
	}
}

func TestAppendEventSchemaRejection(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// Missing required field (session_id for session_start).
	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
	}
	err := w.AppendEvent(event)
	if err == nil {
		t.Fatal("expected schema validation error, got nil")
	}
	if !strings.Contains(err.Error(), "missing required fields") {
		t.Errorf("error should mention missing fields, got: %v", err)
	}

	// Verify no file was created.
	if _, statErr := os.Stat(filepath.Join(w.Dir(), FileSessions)); !os.IsNotExist(statErr) {
		t.Error("file should not have been created for invalid event")
	}
}

func TestAppendEventUnknownType(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "totally_unknown_event",
		Timestamp: "2026-02-28T14:00:00Z",
	}
	err := w.AppendEvent(event)
	if err == nil {
		t.Fatal("expected error for unknown event type")
	}
	if !strings.Contains(err.Error(), "unknown event type") {
		t.Errorf("error should mention unknown event type, got: %v", err)
	}
}

func TestAppendEventToFile(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-test",
	}

	// Correct file.
	if err := w.AppendEventToFile(FileSessions, event); err != nil {
		t.Fatalf("AppendEventToFile: %v", err)
	}

	// Wrong file.
	err := w.AppendEventToFile(FileWorkGraph, event)
	if err == nil {
		t.Fatal("expected misrouted error")
	}
	if !strings.Contains(err.Error(), "does not belong") {
		t.Errorf("error should mention misrouting, got: %v", err)
	}
}

func TestAppendMultipleEventsToSameFile(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	for i := range 5 {
		event := Event{
			EventType: "session_start",
			Timestamp: "2026-02-28T14:00:00Z",
			SessionID: "ses-" + string(rune('a'+i)),
		}
		if err := w.AppendEvent(event); err != nil {
			t.Fatalf("AppendEvent[%d]: %v", i, err)
		}
	}

	data, err := os.ReadFile(filepath.Join(w.Dir(), FileSessions))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != 5 {
		t.Errorf("expected 5 lines, got %d", len(lines))
	}

	// Verify each line is valid JSON.
	for i, line := range lines {
		var m map[string]any
		if err := json.Unmarshal([]byte(line), &m); err != nil {
			t.Errorf("line %d: invalid JSON: %v", i, err)
		}
	}
}

func TestConcurrentAppend(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	const numWriters = 10
	var wg sync.WaitGroup
	errs := make(chan error, numWriters)

	for i := range numWriters {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			event := Event{
				EventType: "session_start",
				Timestamp: "2026-02-28T14:00:00Z",
				SessionID: "ses-concurrent",
				Data: map[string]any{
					"writer_idx": idx,
				},
			}
			if appendErr := w.AppendEvent(event); appendErr != nil {
				errs <- appendErr
			}
		}(i)
	}

	wg.Wait()
	close(errs)

	for err := range errs {
		t.Errorf("concurrent append error: %v", err)
	}

	// Verify all lines were written.
	data, err := os.ReadFile(filepath.Join(w.Dir(), FileSessions))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) != numWriters {
		t.Errorf("expected %d lines, got %d", numWriters, len(lines))
	}

	// Verify each line is valid JSON (no interleaving).
	for i, line := range lines {
		var m map[string]any
		if err := json.Unmarshal([]byte(line), &m); err != nil {
			t.Errorf("line %d: invalid JSON (possible interleaving): %v", i, err)
		}
	}
}

func TestAppendEventAutoTimestamp(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)
	// Writer.Now is already set to fixedTime (2026-02-28T14:00:00Z).

	event := Event{
		EventType: "session_start",
		SessionID: "ses-auto-ts",
		// No Timestamp -- should be auto-generated by Writer.
	}
	if err := w.AppendEvent(event); err != nil {
		t.Fatalf("AppendEvent: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(w.Dir(), FileSessions))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal([]byte(strings.TrimSpace(string(data))), &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	ts, ok := m["timestamp"].(string)
	if !ok {
		t.Fatal("timestamp should be present")
	}
	if ts != "2026-02-28T14:00:00Z" {
		t.Errorf("timestamp = %q, want 2026-02-28T14:00:00Z", ts)
	}
}

func TestAppendEventEmptyEventType(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		Timestamp: "2026-02-28T14:00:00Z",
		// No EventType.
	}
	err := w.AppendEvent(event)
	if err == nil {
		t.Fatal("expected error for empty event type")
	}
}

func TestAppendEventWithLargeData(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// Create a large data payload.
	largeValue := strings.Repeat("x", 10000)
	event := Event{
		EventType: "memory_store",
		Timestamp: "2026-02-28T14:00:00Z",
		Data: map[string]any{
			"id":      "mem-large",
			"content": largeValue,
		},
	}

	if err := w.AppendEvent(event); err != nil {
		t.Fatalf("AppendEvent: %v", err)
	}

	// Verify the file contains the large value.
	data, err := os.ReadFile(filepath.Join(w.Dir(), FileMemoryEvents))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if !strings.Contains(string(data), largeValue) {
		t.Error("file should contain the large data value")
	}
}

func TestAppendEventPreservesExistingTimestamp(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "session_start",
		Timestamp: "2025-01-01T00:00:00Z",
		SessionID: "ses-preserved",
	}
	if err := w.AppendEvent(event); err != nil {
		t.Fatalf("AppendEvent: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(w.Dir(), FileSessions))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal([]byte(strings.TrimSpace(string(data))), &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	// Should preserve the original timestamp, not replace with fixedTime.
	if m["timestamp"] != "2025-01-01T00:00:00Z" {
		t.Errorf("timestamp = %v, want 2025-01-01T00:00:00Z (should preserve original)", m["timestamp"])
	}
}

func TestNewWriter_MkdirAllError(t *testing.T) {
	t.Parallel()

	// Point at a path nested under a file (not a directory) to trigger MkdirAll error.
	tmpDir := t.TempDir()
	blocker := filepath.Join(tmpDir, "not-a-dir")
	if err := os.WriteFile(blocker, []byte("file"), 0o644); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	// Attempt to create a writer with a dir nested under the regular file.
	_, err := NewWriter(filepath.Join(blocker, "ledger"))
	if err == nil {
		t.Fatal("expected error when directory creation fails")
	}
	if !strings.Contains(err.Error(), "creating directory") {
		t.Errorf("error should mention creating directory, got: %v", err)
	}
}

func TestAppendEventToFile_SchemaRejection(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// session_start requires session_id — omit it to trigger schema error.
	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
	}
	err := w.AppendEventToFile(FileSessions, event)
	if err == nil {
		t.Fatal("expected schema validation error, got nil")
	}
	if !strings.Contains(err.Error(), "missing required fields") {
		t.Errorf("error should mention missing fields, got: %v", err)
	}
}

func TestAppendToFile_LockFileOpenError(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// Create a directory where the lock file would go, so OpenFile fails.
	lockPath := filepath.Join(w.Dir(), FileSessions+".lock")
	if err := os.MkdirAll(lockPath, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-lockfail",
	}
	err := w.AppendEvent(event)
	if err == nil {
		t.Fatal("expected error when lock file cannot be opened")
	}
	if !strings.Contains(err.Error(), "opening lock file") {
		t.Errorf("error should mention opening lock file, got: %v", err)
	}
}

func TestAppendToFile_DataFileOpenError(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// Create a directory at the data file path so OpenFile fails.
	dataPath := filepath.Join(w.Dir(), FileSessions)
	if err := os.MkdirAll(dataPath, 0o755); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-openfail",
	}
	err := w.AppendEvent(event)
	if err == nil {
		t.Fatal("expected error when data file cannot be opened")
	}
	if !strings.Contains(err.Error(), "opening file") {
		t.Errorf("error should mention opening file, got: %v", err)
	}
}

func TestAppendToFile_ReadOnlyDir(t *testing.T) {
	t.Parallel()

	// Create a writer in a directory, then make it read-only to trigger write error.
	dir := filepath.Join(t.TempDir(), "ledger")
	w, err := NewWriter(dir)
	if err != nil {
		t.Fatalf("NewWriter: %v", err)
	}
	w.Now = fixedTime

	// Create the lock file first (writable), then make the data file unwritable.
	dataPath := filepath.Join(dir, FileSessions)
	// Create data file as read-only to trigger write permission error.
	if err := os.WriteFile(dataPath, nil, 0o444); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-readonly",
	}
	err = w.AppendEvent(event)
	if err == nil {
		// On some systems the OS may allow root to write to read-only files.
		// Skip instead of failing.
		t.Skip("OS allowed write to read-only file (possibly running as root)")
	}
	// Verify we get either an opening or writing error.
	if !strings.Contains(err.Error(), "opening file") && !strings.Contains(err.Error(), "writing event") {
		t.Errorf("error should mention file operation failure, got: %v", err)
	}
}

func TestAppendEventToFile_MisrouteError(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	// session_start belongs to sessions.jsonl, not work-graph.jsonl.
	event := Event{
		EventType: "session_start",
		Timestamp: "2026-02-28T14:00:00Z",
		SessionID: "ses-misroute",
	}
	err := w.AppendEventToFile(FileWorkGraph, event)
	if err == nil {
		t.Fatal("expected misrouted error")
	}
	if !strings.Contains(err.Error(), "does not belong") {
		t.Errorf("error should mention misrouting, got: %v", err)
	}
}

func TestAppendEventToFile_UnknownEventType(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "totally_unknown",
		Timestamp: "2026-02-28T14:00:00Z",
	}
	err := w.AppendEventToFile(FileSessions, event)
	if err == nil {
		t.Fatal("expected unknown event type error")
	}
	if !strings.Contains(err.Error(), "unknown event type") {
		t.Errorf("error should mention unknown event type, got: %v", err)
	}
}

func TestAppendEventToFile_HappyPath(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "task_created",
		Timestamp: "2026-02-28T14:00:00Z",
		Data: map[string]any{
			"id":      "t-1",
			"epic_id": "e-1",
			"title":   "Test task",
		},
	}

	if err := w.AppendEventToFile(FileWorkGraph, event); err != nil {
		t.Fatalf("AppendEventToFile: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(w.Dir(), FileWorkGraph))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal([]byte(strings.TrimSpace(string(data))), &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}
	if m["event"] != "task_created" {
		t.Errorf("event = %v, want task_created", m["event"])
	}
}

func TestAppendEventToFile_AutoTimestamp(t *testing.T) {
	t.Parallel()

	w := newTestWriter(t)

	event := Event{
		EventType: "config_set",
		// No Timestamp — should be auto-generated.
	}

	if err := w.AppendEventToFile(FileConfig, event); err != nil {
		t.Fatalf("AppendEventToFile: %v", err)
	}

	data, err := os.ReadFile(filepath.Join(w.Dir(), FileConfig))
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}

	var m map[string]any
	if err := json.Unmarshal([]byte(strings.TrimSpace(string(data))), &m); err != nil {
		t.Fatalf("Unmarshal: %v", err)
	}

	ts, ok := m["timestamp"].(string)
	if !ok || ts == "" {
		t.Fatal("timestamp should be auto-generated")
	}
	if ts != "2026-02-28T14:00:00Z" {
		t.Errorf("timestamp = %q, want 2026-02-28T14:00:00Z (from fixedTime)", ts)
	}
}
