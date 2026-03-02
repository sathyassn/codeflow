package main

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

// writeEnvFile creates a codeflow-env.sh in dir so ResolveSessionID picks up
// the session ID without t.Setenv (which is incompatible with t.Parallel).
func writeEnvFile(t *testing.T, dir, sessionID string) {
	t.Helper()
	runtimeDir := filepath.Join(dir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatalf("MkdirAll for env file: %v", err)
	}
	content := "export CODEFLOW_SESSION_ID='" + sessionID + "'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(content), 0o644); err != nil {
		t.Fatalf("WriteFile for env file: %v", err)
	}
}

func TestNewStopCmd(t *testing.T) {
	t.Parallel()

	cmd := newStopCmd()
	if cmd.Use != "stop" {
		t.Errorf("newStopCmd().Use = %q, want %q", cmd.Use, "stop")
	}
	if !cmd.HasSubCommands() {
		t.Error("newStopCmd() has no subcommands, want logging")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["logging"] {
		t.Error("newStopCmd() missing subcommand 'logging'")
	}
}

func TestNewUserPromptSubmitCmd(t *testing.T) {
	t.Parallel()

	cmd := newUserPromptSubmitCmd()
	if cmd.Use != "user-prompt-submit" {
		t.Errorf("newUserPromptSubmitCmd().Use = %q, want %q", cmd.Use, "user-prompt-submit")
	}
	if !cmd.HasSubCommands() {
		t.Error("newUserPromptSubmitCmd() has no subcommands, want logging")
	}

	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["logging"] {
		t.Error("newUserPromptSubmitCmd() missing subcommand 'logging'")
	}
}

func TestNewSessionStartLogCmd(t *testing.T) {
	t.Parallel()

	cmd := newSessionStartLogCmd()
	if cmd.Use != "logging" {
		t.Errorf("Use = %q, want %q", cmd.Use, "logging")
	}
}

func TestNewSessionEndLogCmd(t *testing.T) {
	t.Parallel()

	cmd := newSessionEndLogCmd()
	if cmd.Use != "logging" {
		t.Errorf("Use = %q, want %q", cmd.Use, "logging")
	}
}

func TestNewStopLogCmd(t *testing.T) {
	t.Parallel()

	cmd := newStopLogCmd()
	if cmd.Use != "logging" {
		t.Errorf("Use = %q, want %q", cmd.Use, "logging")
	}
}

func TestNewPostToolUseLogCmd(t *testing.T) {
	t.Parallel()

	cmd := newPostToolUseLogCmd()
	if cmd.Use != "logging" {
		t.Errorf("Use = %q, want %q", cmd.Use, "logging")
	}
}

func TestNewUserPromptSubmitLogCmd(t *testing.T) {
	t.Parallel()

	cmd := newUserPromptSubmitLogCmd()
	if cmd.Use != "logging" {
		t.Errorf("Use = %q, want %q", cmd.Use, "logging")
	}
}

func TestRunSessionStartLog(t *testing.T) {
	t.Parallel()
	t.Run("logs session start event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-start")

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid","source":"startup"}`))
		var errBuf bytes.Buffer

		if err := runSessionStartLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runSessionStartLog() error = %v", err)
		}
		if errBuf.Len() > 0 {
			t.Errorf("unexpected stderr: %s", errBuf.String())
		}
	})

	t.Run("empty stdin does not error", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-start-empty")

		stdin := bytes.NewReader([]byte{})
		var errBuf bytes.Buffer

		if err := runSessionStartLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runSessionStartLog() error = %v", err)
		}
	})
}

func TestRunSessionEndLog(t *testing.T) {
	t.Parallel()
	t.Run("logs session end event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-end")

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid"}`))
		var errBuf bytes.Buffer

		if err := runSessionEndLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runSessionEndLog() error = %v", err)
		}
	})
}

func TestRunStopLog(t *testing.T) {
	t.Parallel()
	t.Run("logs stop event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-stop")

		stdin := bytes.NewReader([]byte(`{"stop_reason":"end_turn"}`))
		var errBuf bytes.Buffer

		if err := runStopLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runStopLog() error = %v", err)
		}
	})
}

func TestRunPostToolUseLog(t *testing.T) {
	t.Parallel()
	t.Run("logs tool use event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-tool")

		stdin := bytes.NewReader([]byte(`{"tool_name":"Bash","tool_input":{"command":"ls"}}`))
		var errBuf bytes.Buffer

		if err := runPostToolUseLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runPostToolUseLog() error = %v", err)
		}
	})
}

func TestRunUserPromptSubmitLog(t *testing.T) {
	t.Parallel()
	t.Run("logs prompt event", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-cmd-prompt")

		stdin := bytes.NewReader([]byte(`{"user_prompt":"hello"}`))
		var errBuf bytes.Buffer

		if err := runUserPromptSubmitLog(stdin, &errBuf, dir); err != nil {
			t.Fatalf("runUserPromptSubmitLog() error = %v", err)
		}
	})
}

func TestHooksCmd_HasAllEventGroups(t *testing.T) {
	t.Parallel()

	cmd := newHooksCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}

	expected := []string{
		"pre-tool-use",
		"post-tool-use",
		"task-completed",
		"session-start",
		"session-end",
		"stop",
		"user-prompt-submit",
	}
	for _, name := range expected {
		if !names[name] {
			t.Errorf("newHooksCmd() missing subcommand %q", name)
		}
	}
}

func TestSessionStartCmd_HasLogging(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionStartCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["logging"] {
		t.Error("session-start missing 'logging' subcommand")
	}
	if !names["init"] {
		t.Error("session-start missing 'init' subcommand")
	}
}

func TestSessionEndCmd_HasLogging(t *testing.T) {
	t.Parallel()

	cmd := newHookSessionEndCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["logging"] {
		t.Error("session-end missing 'logging' subcommand")
	}
	if !names["cleanup"] {
		t.Error("session-end missing 'cleanup' subcommand")
	}
}

func TestPostToolUseCmd_HasLogging(t *testing.T) {
	t.Parallel()

	cmd := newPostToolUseCmd()
	subs := cmd.Commands()
	names := make(map[string]bool)
	for _, sub := range subs {
		names[sub.Name()] = true
	}
	if !names["logging"] {
		t.Error("post-tool-use missing 'logging' subcommand")
	}
}

func TestRunSessionStartLog_WriterError(t *testing.T) {
	t.Parallel()
	t.Run("writer creation failure logs to stderr", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-err-start")

		// Create config that points to a log directory that cannot be created
		// (a file blocks the parent path, so MkdirAll fails).
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		blocker := filepath.Join(dir, "blocked")
		_ = os.WriteFile(blocker, []byte("x"), 0o644)
		logDir := filepath.Join(blocker, "subdir")
		policy := `{"logging":{"session_start":{"enabled":true,"log_directory":"` + logDir + `"}}}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		stdin := bytes.NewReader([]byte(`{}`))
		var errBuf bytes.Buffer

		err := runSessionStartLog(stdin, &errBuf, dir)
		if err != nil {
			t.Errorf("should return nil even on error, got %v", err)
		}
		if errBuf.Len() == 0 {
			t.Error("expected stderr output on writer creation failure")
		}
	})
}

func TestRunSessionEndLog_WriterError(t *testing.T) {
	t.Parallel()
	t.Run("writer creation failure logs to stderr", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-err-end")
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		blocker := filepath.Join(dir, "blocked")
		_ = os.WriteFile(blocker, []byte("x"), 0o644)
		logDir := filepath.Join(blocker, "subdir")
		policy := `{"logging":{"session_end":{"enabled":true,"log_directory":"` + logDir + `"}}}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		stdin := bytes.NewReader([]byte(`{}`))
		var errBuf bytes.Buffer

		err := runSessionEndLog(stdin, &errBuf, dir)
		if err != nil {
			t.Errorf("should return nil even on error, got %v", err)
		}
		if errBuf.Len() == 0 {
			t.Error("expected stderr output on writer creation failure")
		}
	})
}

func TestRunStopLog_WriterError(t *testing.T) {
	t.Parallel()
	t.Run("writer creation failure logs to stderr", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-err-stop")
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		blocker := filepath.Join(dir, "blocked")
		_ = os.WriteFile(blocker, []byte("x"), 0o644)
		logDir := filepath.Join(blocker, "subdir")
		// Stop uses its own LogDirectory.
		policy := `{"logging":{"stop":{"enabled":true,"log_directory":"` + logDir + `"}}}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		stdin := bytes.NewReader([]byte(`{"stop_reason":"end_turn"}`))
		var errBuf bytes.Buffer

		err := runStopLog(stdin, &errBuf, dir)
		if err != nil {
			t.Errorf("should return nil even on error, got %v", err)
		}
		if errBuf.Len() == 0 {
			t.Error("expected stderr output on writer creation failure")
		}
	})
}

func TestRunPostToolUseLog_WriterError(t *testing.T) {
	t.Parallel()
	t.Run("writer creation failure logs to stderr", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-err-tool")
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		blocker := filepath.Join(dir, "blocked")
		_ = os.WriteFile(blocker, []byte("x"), 0o644)
		logDir := filepath.Join(blocker, "subdir")
		policy := `{"logging":{"post_tool_use":{"enabled":true,"log_directory":"` + logDir + `"}}}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		stdin := bytes.NewReader([]byte(`{"tool_name":"Bash"}`))
		var errBuf bytes.Buffer

		err := runPostToolUseLog(stdin, &errBuf, dir)
		if err != nil {
			t.Errorf("should return nil even on error, got %v", err)
		}
		if errBuf.Len() == 0 {
			t.Error("expected stderr output on writer creation failure")
		}
	})
}

func TestRunUserPromptSubmitLog_WriterError(t *testing.T) {
	t.Parallel()
	t.Run("writer creation failure logs to stderr", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-err-prompt")
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		blocker := filepath.Join(dir, "blocked")
		_ = os.WriteFile(blocker, []byte("x"), 0o644)
		logDir := filepath.Join(blocker, "subdir")
		// UserPromptSubmit uses its own LogDirectory.
		policy := `{"logging":{"user_prompt":{"enabled":true,"log_directory":"` + logDir + `"}}}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		stdin := bytes.NewReader([]byte(`{"user_prompt":"hello"}`))
		var errBuf bytes.Buffer

		err := runUserPromptSubmitLog(stdin, &errBuf, dir)
		if err != nil {
			t.Errorf("should return nil even on error, got %v", err)
		}
		if errBuf.Len() == 0 {
			t.Error("expected stderr output on writer creation failure")
		}
	})
}

func TestSessionStartLogCmd_Execute(t *testing.T) {
	t.Parallel()
	t.Run("executes via cobra command", func(t *testing.T) {
		t.Parallel()
		cmd := newSessionStartLogCmd()
		cmd.SetIn(bytes.NewReader([]byte(`{"session_id":"uuid"}`)))
		cmd.SetErr(&bytes.Buffer{})

		if err := cmd.Execute(); err != nil {
			t.Fatalf("cmd.Execute() error = %v", err)
		}
	})
}

func TestSessionEndLogCmd_Execute(t *testing.T) {
	t.Parallel()
	t.Run("executes via cobra command", func(t *testing.T) {
		t.Parallel()
		cmd := newSessionEndLogCmd()
		cmd.SetIn(bytes.NewReader([]byte(`{"session_id":"uuid"}`)))
		cmd.SetErr(&bytes.Buffer{})

		if err := cmd.Execute(); err != nil {
			t.Fatalf("cmd.Execute() error = %v", err)
		}
	})
}

func TestStopLogCmd_Execute(t *testing.T) {
	t.Parallel()
	t.Run("executes via cobra command", func(t *testing.T) {
		t.Parallel()
		cmd := newStopLogCmd()
		cmd.SetIn(bytes.NewReader([]byte(`{"stop_reason":"end_turn"}`)))
		cmd.SetErr(&bytes.Buffer{})

		if err := cmd.Execute(); err != nil {
			t.Fatalf("cmd.Execute() error = %v", err)
		}
	})
}

func TestPostToolUseLogCmd_Execute(t *testing.T) {
	t.Parallel()
	t.Run("executes via cobra command", func(t *testing.T) {
		t.Parallel()
		cmd := newPostToolUseLogCmd()
		cmd.SetIn(bytes.NewReader([]byte(`{"tool_name":"Bash"}`)))
		cmd.SetErr(&bytes.Buffer{})

		if err := cmd.Execute(); err != nil {
			t.Fatalf("cmd.Execute() error = %v", err)
		}
	})
}

func TestUserPromptSubmitLogCmd_Execute(t *testing.T) {
	t.Parallel()
	t.Run("executes via cobra command", func(t *testing.T) {
		t.Parallel()
		cmd := newUserPromptSubmitLogCmd()
		cmd.SetIn(bytes.NewReader([]byte(`{"user_prompt":"hello"}`)))
		cmd.SetErr(&bytes.Buffer{})

		if err := cmd.Execute(); err != nil {
			t.Fatalf("cmd.Execute() error = %v", err)
		}
	})
}

func TestLogCmdCreatesFiles(t *testing.T) {
	t.Parallel()
	t.Run("session-start creates log file in controlled dir", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		writeEnvFile(t, dir, "ses-file-check")

		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		logsDir := filepath.Join(dir, "logs")
		_ = os.WriteFile(
			filepath.Join(configDir, "enforcement-policy.json"),
			[]byte(`{"logging":{"session_start":{"enabled":true,"log_directory":"`+logsDir+`"}}}`),
			0o644,
		)

		stdin := bytes.NewReader([]byte(`{"session_id":"uuid","source":"startup"}`))
		var errBuf bytes.Buffer
		_ = runSessionStartLog(stdin, &errBuf, dir)

		// Verify a session log file was created.
		files, _ := filepath.Glob(filepath.Join(logsDir, "session-*.jsonl"))
		if len(files) == 0 {
			t.Error("expected session log file to be created")
		}
		if errBuf.Len() > 0 {
			t.Logf("stderr (non-fatal): %s", errBuf.String())
		}
	})
}
