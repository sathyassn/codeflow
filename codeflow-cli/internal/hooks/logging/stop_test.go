package logging

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestLogStop(t *testing.T) {
	t.Parallel()
	fixedTime := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)

	t.Run("writes stop event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-stop-test")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid","stop_reason":"end_turn"}`))
		if err := LogStop(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("LogStop() error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["event"] != "stop" {
			t.Errorf("event = %v, want stop", record["event"])
		}
		if record["stop_reason"] != "end_turn" {
			t.Errorf("stop_reason = %v, want end_turn", record["stop_reason"])
		}
		if record["session_id"] != "ses-stop-test" {
			t.Errorf("session_id = %v, want ses-stop-test", record["session_id"])
		}
	})

	t.Run("disabled skips logging", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.Stop.Enabled = false

		if err := LogStop(w, nil, dir, cfg); err != nil {
			t.Fatalf("error = %v", err)
		}
		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no files when disabled")
		}
	})

	t.Run("default stop reason when missing", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-no-reason")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid"}`))
		_ = LogStop(w, stdin, dir, DefaultConfig())

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["stop_reason"] != "unknown" {
			t.Errorf("stop_reason = %v, want unknown", record["stop_reason"])
		}
	})

	t.Run("includes task context when enabled", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-task-ctx")

		// Create active-task.json.
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(filepath.Join(runtimeDir, "active-task.json"),
			[]byte(`{"task_id":"INF-TSK-021-017","status":"in_progress"}`), 0o644)

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"stop_reason":"end_turn"}`))
		_ = LogStop(w, stdin, dir, DefaultConfig())

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		taskCtx, ok := record["task_context"].(map[string]any)
		if !ok {
			t.Fatal("task_context missing or not a map")
		}
		if taskCtx["task_id"] != "INF-TSK-021-017" {
			t.Errorf("task_id = %v, want INF-TSK-021-017", taskCtx["task_id"])
		}
	})

	t.Run("no task context when disabled", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-no-task")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.Stop.CaptureTaskContext = false

		stdin := bytes.NewReader([]byte(`{"stop_reason":"end_turn"}`))
		_ = LogStop(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["task_context"] != nil {
			t.Error("task_context should be absent when CaptureTaskContext is false")
		}
	})
}
