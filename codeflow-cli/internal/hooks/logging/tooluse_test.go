package logging

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestLogToolUse(t *testing.T) {
	t.Parallel()
	fixedTime := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)

	t.Run("writes tool_completed event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-tool")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"tool_name":"Bash","tool_input":{"command":"ls"},"tool_result":"file1\nfile2"}`))
		if err := LogToolUse(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("LogToolUse() error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "tool-use-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["event"] != "tool_completed" {
			t.Errorf("event = %v, want tool_completed", record["event"])
		}
		if record["tool_name"] != "Bash" {
			t.Errorf("tool_name = %v, want Bash", record["tool_name"])
		}
		if record["session_id"] != "ses-tool" {
			t.Errorf("session_id = %v, want ses-tool", record["session_id"])
		}
	})

	t.Run("disabled skips logging", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")

		cfg := DefaultConfig()
		cfg.PostToolUse.Enabled = false

		if err := LogToolUse(w, bytes.NewReader([]byte(`{"tool_name":"Bash"}`)), dir, cfg); err != nil {
			t.Fatalf("error = %v", err)
		}
		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no files when disabled")
		}
	})

	t.Run("empty tool name skips", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{}`))
		if err := LogToolUse(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("error = %v", err)
		}
		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no files for empty tool name")
		}
	})

	t.Run("tool filter excludes unmatched tools", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.PostToolUse.ToolsToLog = []string{"Bash", "Edit"}

		stdin := bytes.NewReader([]byte(`{"tool_name":"Read"}`))
		_ = LogToolUse(w, stdin, dir, cfg)

		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("Read should be filtered out")
		}
	})

	t.Run("tool filter includes matched tools", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-filter")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.PostToolUse.ToolsToLog = []string{"Bash", "Edit"}

		stdin := bytes.NewReader([]byte(`{"tool_name":"Edit","tool_input":{"file_path":"foo.go"}}`))
		_ = LogToolUse(w, stdin, dir, cfg)

		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) == 0 {
			t.Error("Edit should be logged when in ToolsToLog list")
		}
	})

	t.Run("truncates large results", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-trunc")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.PostToolUse.MaxResultSize = 50
		cfg.PostToolUse.RedactSensit = false

		largeResult := `"` + strings.Repeat("x", 200) + `"`
		stdin := bytes.NewReader([]byte(`{"tool_name":"Bash","tool_result":` + largeResult + `}`))
		_ = LogToolUse(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "tool-use-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		result, _ := record["tool_result"].(string)
		if !strings.HasSuffix(result, "...[truncated]") {
			t.Errorf("expected truncated result, got %q", result)
		}
		if record["result_truncated"] != true {
			t.Error("result_truncated should be true")
		}
	})

	t.Run("redacts sensitive data", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-redact")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.PostToolUse.RedactSensit = true

		stdin := bytes.NewReader([]byte(`{"tool_name":"Bash","tool_input":{"command":"password= secret123"},"tool_result":"Bearer abcdef123"}`))
		_ = LogToolUse(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "tool-use-2026-03-01.jsonl"))
		content := string(data)

		if strings.Contains(content, "secret123") {
			t.Error("password should be redacted")
		}
		if strings.Contains(content, "abcdef123") {
			t.Error("bearer token should be redacted")
		}
	})
}

func TestIsToolLogged(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		tool    string
		allowed []string
		want    bool
	}{
		{"in list", "Bash", []string{"Bash", "Edit"}, true},
		{"not in list", "Read", []string{"Bash", "Edit"}, false},
		{"empty list allows all", "Read", []string{}, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := isToolLogged(tt.tool, tt.allowed)
			if got != tt.want {
				t.Errorf("isToolLogged(%q) = %v, want %v", tt.tool, got, tt.want)
			}
		})
	}
}

func TestRedactSensitive(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		input string
		check func(string) bool
	}{
		{"password", "password= mysecret", func(s string) bool { return !strings.Contains(s, "mysecret") }},
		{"bearer", "Bearer abc123token", func(s string) bool { return !strings.Contains(s, "abc123token") }},
		{"email", "user@example.com", func(s string) bool { return !strings.Contains(s, "user@example.com") }},
		{"no secrets", "hello world", func(s string) bool { return s == "hello world" }},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := redactSensitive(tt.input)
			if !tt.check(got) {
				t.Errorf("redactSensitive(%q) = %q, failed check", tt.input, got)
			}
		})
	}
}
