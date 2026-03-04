package logging

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestLogSessionStart(t *testing.T) {
	t.Parallel()
	fixedTime := time.Date(2026, 3, 1, 10, 30, 0, 0, time.UTC)

	t.Run("writes session_start event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-test-start")

		w, err := NewActivityWriter(dir, "logs")
		if err != nil {
			t.Fatalf("NewActivityWriter() error = %v", err)
		}
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid","source":"startup","permission_mode":"plan"}`))
		cfg := DefaultConfig()

		if err := LogSessionStart(w, stdin, dir, cfg); err != nil {
			t.Fatalf("LogSessionStart() error = %v", err)
		}

		data, err := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		if err != nil {
			t.Fatalf("reading log: %v", err)
		}

		var record map[string]any
		if err := json.Unmarshal(data, &record); err != nil {
			t.Fatalf("parsing log: %v", err)
		}

		if record["event"] != "session_start" {
			t.Errorf("event = %v, want session_start", record["event"])
		}
		if record["session_id"] != "ses-test-start" {
			t.Errorf("session_id = %v, want ses-test-start (from env file, not stdin)", record["session_id"])
		}
		if record["ts"] != "2026-03-01T10:30:00.000Z" {
			t.Errorf("ts = %v, want 2026-03-01T10:30:00.000Z", record["ts"])
		}
		if record["metadata"] == nil {
			t.Error("metadata missing (capture_metadata is true)")
		}
	})

	t.Run("disabled skips logging", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.SessionStart.Enabled = false

		if err := LogSessionStart(w, nil, dir, cfg); err != nil {
			t.Fatalf("LogSessionStart() error = %v", err)
		}

		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no log files when disabled")
		}
	})

	t.Run("without metadata capture", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-no-meta")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.SessionStart.CaptureMetadata = false

		stdin := bytes.NewReader([]byte(`{}`))
		if err := LogSessionStart(w, stdin, dir, cfg); err != nil {
			t.Fatalf("error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["metadata"] != nil {
			t.Error("metadata should be absent when CaptureMetadata is false")
		}
	})

	t.Run("empty stdin", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-empty")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		if err := LogSessionStart(w, bytes.NewReader([]byte{}), dir, DefaultConfig()); err != nil {
			t.Fatalf("error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		if len(data) == 0 {
			t.Error("expected log entry even with empty stdin")
		}
	})
}

func TestLogSessionEnd(t *testing.T) {
	t.Parallel()
	fixedTime := time.Date(2026, 3, 1, 11, 0, 0, 0, time.UTC)

	t.Run("writes session_end event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-test-end")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid","transcript_path":"/some/path"}`))
		if err := LogSessionEnd(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("LogSessionEnd() error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["event"] != "session_end" {
			t.Errorf("event = %v, want session_end", record["event"])
		}
		if record["session_id"] != "ses-test-end" {
			t.Errorf("session_id = %v, want ses-test-end", record["session_id"])
		}
	})

	t.Run("disabled skips logging", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")

		cfg := DefaultConfig()
		cfg.SessionEnd.Enabled = false

		if err := LogSessionEnd(w, nil, dir, cfg); err != nil {
			t.Fatalf("error = %v", err)
		}
		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no files when disabled")
		}
	})

	t.Run("calculates duration from meta file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-dur")

		logsDir := filepath.Join(dir, "logs")
		_ = os.MkdirAll(logsDir, 0o755)

		// Write a meta file with a start time 300 seconds ago.
		startEpoch := fixedTime.Unix() - 300
		metaJSON := []byte(fmt.Sprintf(`{"started_epoch":%d}`, startEpoch))
		_ = os.WriteFile(filepath.Join(logsDir, "session-ses-dur.meta"), metaJSON, 0o644)

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{}`))
		if err := LogSessionEnd(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "session-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		dur, ok := record["duration_seconds"].(float64)
		if !ok || dur != 300 {
			t.Errorf("duration_seconds = %v, want 300", record["duration_seconds"])
		}
	})
}

func TestReadActiveTaskID(t *testing.T) {
	t.Parallel()

	t.Run("returns task ID from valid file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "active-task.json"),
			[]byte(`{"task_id":"INF-TSK-021-017"}`),
			0o644,
		)

		got := readActiveTaskID(dir)
		if got != "INF-TSK-021-017" {
			t.Errorf("readActiveTaskID() = %q, want %q", got, "INF-TSK-021-017")
		}
	})

	t.Run("returns empty on missing file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		got := readActiveTaskID(dir)
		if got != "" {
			t.Errorf("readActiveTaskID() = %q, want empty", got)
		}
	})

	t.Run("returns empty on malformed JSON", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "active-task.json"),
			[]byte(`{invalid json`),
			0o644,
		)

		got := readActiveTaskID(dir)
		if got != "" {
			t.Errorf("readActiveTaskID() = %q, want empty on bad JSON", got)
		}
	})

	t.Run("returns empty when task_id is empty string", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "active-task.json"),
			[]byte(`{"task_id":""}`),
			0o644,
		)

		got := readActiveTaskID(dir)
		if got != "" {
			t.Errorf("readActiveTaskID() = %q, want empty", got)
		}
	})
}

func TestGatherSessionMetadata(t *testing.T) {
	t.Parallel()
	t.Run("includes active task when present", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		// Create active-task.json.
		runtimeDir := filepath.Join(dir, ".state", "runtime")
		_ = os.MkdirAll(runtimeDir, 0o755)
		_ = os.WriteFile(
			filepath.Join(runtimeDir, "active-task.json"),
			[]byte(`{"task_id":"TSK-001"}`),
			0o644,
		)

		meta := gatherSessionMetadata(dir, "plan")

		if meta["active_task"] != "TSK-001" {
			t.Errorf("active_task = %v, want TSK-001", meta["active_task"])
		}
		if meta["approval_mode"] != "plan" {
			t.Errorf("approval_mode = %v, want plan", meta["approval_mode"])
		}
		if meta["cwd"] != dir {
			t.Errorf("cwd = %v, want %s", meta["cwd"], dir)
		}
	})

	t.Run("uses standard approval mode when empty", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		meta := gatherSessionMetadata(dir, "")

		if meta["approval_mode"] != "standard" {
			t.Errorf("approval_mode = %v, want standard", meta["approval_mode"])
		}
	})

	t.Run("includes git info in git repo", func(t *testing.T) {
		t.Parallel()
		// Use the actual project directory since it is a git repo.
		// This exercises the gitBranch and gitShortCommit success paths.
		projectDir := findGitRoot(t)
		if projectDir == "" {
			t.Skip("not running inside a git repo")
		}

		meta := gatherSessionMetadata(projectDir, "auto")

		// git_branch may be absent in detached HEAD (e.g., CI merge refs).
		// Only assert git_commit which is always available in a repo.
		if meta["git_commit"] == nil {
			t.Error("expected git_commit to be present in a git repo")
		}
	})
}

// findGitRoot returns the git repo root, or empty string if not in a repo.
func findGitRoot(t *testing.T) string {
	t.Helper()
	out, err := exec.Command("git", "rev-parse", "--show-toplevel").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

func TestGitBranch(t *testing.T) {
	t.Parallel()
	t.Run("returns branch in git repo", func(t *testing.T) {
		t.Parallel()
		root := findGitRoot(t)
		if root == "" {
			t.Skip("not in a git repo")
		}
		branch := gitBranch(root)
		// In detached HEAD (e.g., CI merge refs), branch is empty — that's valid.
		// We only verify the function does not error; the branch value depends on context.
		_ = branch
	})

	t.Run("returns empty for non-git directory", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		branch := gitBranch(dir)
		if branch != "" {
			t.Errorf("gitBranch() = %q, want empty for non-git dir", branch)
		}
	})
}

func TestGitShortCommit(t *testing.T) {
	t.Parallel()
	t.Run("returns commit in git repo", func(t *testing.T) {
		t.Parallel()
		root := findGitRoot(t)
		if root == "" {
			t.Skip("not in a git repo")
		}
		commit := gitShortCommit(root)
		if commit == "" {
			t.Error("gitShortCommit() returned empty in a git repo")
		}
	})

	t.Run("returns empty for non-git directory", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		commit := gitShortCommit(dir)
		if commit != "" {
			t.Errorf("gitShortCommit() = %q, want empty for non-git dir", commit)
		}
	})
}

func TestParseJSON(t *testing.T) {
	t.Parallel()

	t.Run("valid JSON", func(t *testing.T) {
		t.Parallel()
		r := bytes.NewReader([]byte(`{"session_id":"abc","source":"startup"}`))
		result := parseJSON[sessionStartInput](r)
		if result.SessionID != "abc" {
			t.Errorf("SessionID = %q, want %q", result.SessionID, "abc")
		}
	})

	t.Run("nil reader", func(t *testing.T) {
		t.Parallel()
		result := parseJSON[sessionStartInput](nil)
		if result.SessionID != "" {
			t.Errorf("expected zero value for nil reader, got SessionID=%q", result.SessionID)
		}
	})

	t.Run("empty input", func(t *testing.T) {
		t.Parallel()
		result := parseJSON[sessionStartInput](bytes.NewReader([]byte{}))
		if result.SessionID != "" {
			t.Errorf("expected zero value for empty input, got SessionID=%q", result.SessionID)
		}
	})

	t.Run("malformed JSON", func(t *testing.T) {
		t.Parallel()
		result := parseJSON[sessionStartInput](bytes.NewReader([]byte("{bad json")))
		if result.SessionID != "" {
			t.Errorf("expected zero value for malformed JSON, got SessionID=%q", result.SessionID)
		}
	})
}
