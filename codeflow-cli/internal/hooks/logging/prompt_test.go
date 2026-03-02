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

func TestLogPrompt(t *testing.T) {
	t.Parallel()
	fixedTime := time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)

	t.Run("writes prompt_submitted event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-prompt")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{"user_prompt":"hello world"}`))
		if err := LogPrompt(w, stdin, dir, DefaultConfig()); err != nil {
			t.Fatalf("LogPrompt() error = %v", err)
		}

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "prompts-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["event"] != "prompt_submitted" {
			t.Errorf("event = %v, want prompt_submitted", record["event"])
		}
		if record["session_id"] != "ses-prompt" {
			t.Errorf("session_id = %v, want ses-prompt", record["session_id"])
		}
		promptLen, _ := record["prompt_length"].(float64)
		if int(promptLen) != len("hello world") {
			t.Errorf("prompt_length = %v, want %d", promptLen, len("hello world"))
		}
		if record["prompt_hash"] == "" {
			t.Error("prompt_hash should not be empty")
		}
	})

	t.Run("disabled skips logging", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		w, _ := NewActivityWriter(dir, "logs")

		cfg := DefaultConfig()
		cfg.UserPrompt.Enabled = false

		if err := LogPrompt(w, bytes.NewReader([]byte(`{"user_prompt":"test"}`)), dir, cfg); err != nil {
			t.Fatalf("error = %v", err)
		}
		files, _ := filepath.Glob(filepath.Join(dir, "logs", "*.jsonl"))
		if len(files) > 0 {
			t.Error("expected no files when disabled")
		}
	})

	t.Run("privacy mode excludes full text", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-privacy")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.UserPrompt.PrivacyMode = true
		cfg.UserPrompt.CaptureFullText = true

		stdin := bytes.NewReader([]byte(`{"user_prompt":"my secret prompt"}`))
		_ = LogPrompt(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "prompts-2026-03-01.jsonl"))
		content := string(data)

		if strings.Contains(content, "my secret prompt") {
			t.Error("privacy mode should exclude full text")
		}
		if !strings.Contains(content, "prompt_hash") {
			t.Error("should still include prompt_hash")
		}
	})

	t.Run("capture full text when not privacy", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-fulltext")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.UserPrompt.PrivacyMode = false
		cfg.UserPrompt.CaptureFullText = true

		stdin := bytes.NewReader([]byte(`{"user_prompt":"show me the code"}`))
		_ = LogPrompt(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "prompts-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["prompt_text"] != "show me the code" {
			t.Errorf("prompt_text = %v, want 'show me the code'", record["prompt_text"])
		}
	})

	t.Run("detect intent classifies prompt type", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-intent")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		cfg := DefaultConfig()
		cfg.UserPrompt.DetectIntent = true

		stdin := bytes.NewReader([]byte(`{"user_prompt":"fix the bug in auth"}`))
		_ = LogPrompt(w, stdin, dir, cfg)

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "prompts-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		if record["prompt_type"] != "debugging" {
			t.Errorf("prompt_type = %v, want debugging", record["prompt_type"])
		}
	})

	t.Run("empty prompt", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeTestEnvFile(t, dir, "ses-empty-prompt")

		w, _ := NewActivityWriter(dir, "logs")
		w.Now = func() time.Time { return fixedTime }

		stdin := bytes.NewReader([]byte(`{}`))
		_ = LogPrompt(w, stdin, dir, DefaultConfig())

		data, _ := os.ReadFile(filepath.Join(dir, "logs", "prompts-2026-03-01.jsonl"))
		var record map[string]any
		_ = json.Unmarshal(data, &record)

		promptLen, _ := record["prompt_length"].(float64)
		if int(promptLen) != 0 {
			t.Errorf("prompt_length = %v, want 0", promptLen)
		}
		if record["prompt_hash"] != "" {
			t.Errorf("prompt_hash = %v, want empty for empty prompt", record["prompt_hash"])
		}
	})
}

func TestDetectPromptType(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		prompt string
		want   string
	}{
		{"command", "/cf-develop", "command"},
		{"question", "what is this?", "question"},
		{"debugging", "fix the bug", "debugging"},
		{"creation", "create a new feature", "creation"},
		{"modification", "update the config", "modification"},
		{"review", "review the code", "review"},
		{"navigation", "find the function", "navigation"},
		{"general", "hello there", "general"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := detectPromptType(tt.prompt)
			if got != tt.want {
				t.Errorf("detectPromptType(%q) = %q, want %q", tt.prompt, got, tt.want)
			}
		})
	}
}

func TestContainsAny(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		text  string
		words []string
		want  bool
	}{
		{"match first", "fix the bug", []string{"fix", "error"}, true},
		{"match second", "got an error", []string{"fix", "error"}, true},
		{"no match", "hello world", []string{"fix", "error"}, false},
		{"empty words", "hello", []string{}, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := containsAny(tt.text, tt.words...)
			if got != tt.want {
				t.Errorf("containsAny(%q, %v) = %v, want %v", tt.text, tt.words, got, tt.want)
			}
		})
	}
}
