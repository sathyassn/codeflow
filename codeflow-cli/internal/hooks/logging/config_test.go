package logging

import (
	"os"
	"path/filepath"
	"testing"
)

func TestDefaultConfig(t *testing.T) {
	t.Parallel()

	cfg := DefaultConfig()

	if !cfg.SessionStart.Enabled {
		t.Error("SessionStart.Enabled = false, want true")
	}
	if cfg.SessionStart.LogDirectory != ".state/logs/sessions" {
		t.Errorf("SessionStart.LogDirectory = %q, want %q", cfg.SessionStart.LogDirectory, ".state/logs/sessions")
	}
	if !cfg.SessionStart.CaptureMetadata {
		t.Error("SessionStart.CaptureMetadata = false, want true")
	}
	if !cfg.SessionEnd.Enabled {
		t.Error("SessionEnd.Enabled = false, want true")
	}
	if !cfg.PostToolUse.Enabled {
		t.Error("PostToolUse.Enabled = false, want true")
	}
	if cfg.PostToolUse.MaxResultSize != 2000 {
		t.Errorf("PostToolUse.MaxResultSize = %d, want 2000", cfg.PostToolUse.MaxResultSize)
	}
	if !cfg.Stop.Enabled {
		t.Error("Stop.Enabled = false, want true")
	}
	if cfg.Stop.LogDirectory != ".state/logs/sessions" {
		t.Errorf("Stop.LogDirectory = %q, want %q", cfg.Stop.LogDirectory, ".state/logs/sessions")
	}
	if !cfg.UserPrompt.Enabled {
		t.Error("UserPrompt.Enabled = false, want true")
	}
	if cfg.UserPrompt.LogDirectory != ".state/logs/sessions" {
		t.Errorf("UserPrompt.LogDirectory = %q, want %q", cfg.UserPrompt.LogDirectory, ".state/logs/sessions")
	}
	if cfg.UserPrompt.PrivacyMode {
		t.Error("UserPrompt.PrivacyMode = true, want false")
	}
}

func TestReadConfig(t *testing.T) {
	t.Parallel()

	t.Run("reads from enforcement-policy.json", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)

		policy := `{
			"logging": {
				"session_start": {
					"enabled": false,
					"log_directory": ".state/logs/custom",
					"capture_metadata": false
				},
				"post_tool_use": {
					"max_result_size": 500,
					"redact_sensitive": false,
					"tools_to_log": ["Bash", "Edit"]
				},
				"user_prompt": {
					"privacy_mode": true
				}
			}
		}`
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

		cfg := ReadConfig(dir)

		if cfg.SessionStart.Enabled {
			t.Error("SessionStart.Enabled = true, want false")
		}
		if cfg.SessionStart.LogDirectory != ".state/logs/custom" {
			t.Errorf("LogDirectory = %q, want %q", cfg.SessionStart.LogDirectory, ".state/logs/custom")
		}
		if cfg.SessionStart.CaptureMetadata {
			t.Error("CaptureMetadata = true, want false")
		}
		if cfg.PostToolUse.MaxResultSize != 500 {
			t.Errorf("MaxResultSize = %d, want 500", cfg.PostToolUse.MaxResultSize)
		}
		if cfg.PostToolUse.RedactSensit {
			t.Error("RedactSensit = true, want false")
		}
		if len(cfg.PostToolUse.ToolsToLog) != 2 {
			t.Errorf("ToolsToLog len = %d, want 2", len(cfg.PostToolUse.ToolsToLog))
		}
		if !cfg.UserPrompt.PrivacyMode {
			t.Error("PrivacyMode = false, want true")
		}
	})

	t.Run("returns defaults when file missing", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		cfg := ReadConfig(dir)

		if !cfg.SessionStart.Enabled {
			t.Error("expected default SessionStart.Enabled = true")
		}
		if cfg.PostToolUse.MaxResultSize != 2000 {
			t.Errorf("expected default MaxResultSize = 2000, got %d", cfg.PostToolUse.MaxResultSize)
		}
	})

	t.Run("returns defaults on malformed JSON", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte("{invalid"), 0o644)

		cfg := ReadConfig(dir)

		if !cfg.SessionStart.Enabled {
			t.Error("expected default enabled on malformed JSON")
		}
	})

	t.Run("preserves defaults for missing fields", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
		_ = os.MkdirAll(configDir, 0o755)
		_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"),
			[]byte(`{"logging": {"stop": {"enabled": false}}}`), 0o644)

		cfg := ReadConfig(dir)

		if cfg.Stop.Enabled {
			t.Error("Stop.Enabled should be false")
		}
		// Other sections should retain defaults.
		if !cfg.SessionStart.Enabled {
			t.Error("SessionStart.Enabled should still be true (default)")
		}
	})
}

func TestReadConfig_ZeroMaxResultSize(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	_ = os.MkdirAll(configDir, 0o755)

	policy := `{
		"logging": {
			"post_tool_use": {
				"max_result_size": 0
			}
		}
	}`
	_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

	cfg := ReadConfig(dir)

	if cfg.PostToolUse.MaxResultSize != 2000 {
		t.Errorf("MaxResultSize = %d, want 2000 (reset from zero)", cfg.PostToolUse.MaxResultSize)
	}
}

func TestReadConfig_NegativeMaxResultSize(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	_ = os.MkdirAll(configDir, 0o755)

	policy := `{
		"logging": {
			"post_tool_use": {
				"max_result_size": -5
			}
		}
	}`
	_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

	cfg := ReadConfig(dir)

	if cfg.PostToolUse.MaxResultSize != 2000 {
		t.Errorf("MaxResultSize = %d, want 2000 (reset from negative)", cfg.PostToolUse.MaxResultSize)
	}
}

func TestReadConfig_EmptyLogDirectories(t *testing.T) {
	t.Parallel()
	dir := t.TempDir()
	configDir := filepath.Join(dir, ".codeflow", "config", "enforcement")
	_ = os.MkdirAll(configDir, 0o755)

	// Explicitly set log directories to empty strings.
	policy := `{
		"logging": {
			"session_start": {
				"log_directory": ""
			},
			"session_end": {
				"log_directory": ""
			},
			"post_tool_use": {
				"log_directory": ""
			},
			"stop": {
				"log_directory": ""
			},
			"user_prompt": {
				"log_directory": ""
			}
		}
	}`
	_ = os.WriteFile(filepath.Join(configDir, "enforcement-policy.json"), []byte(policy), 0o644)

	cfg := ReadConfig(dir)

	if cfg.SessionStart.LogDirectory != ".state/logs/sessions" {
		t.Errorf("SessionStart.LogDirectory = %q, want default %q", cfg.SessionStart.LogDirectory, ".state/logs/sessions")
	}
	if cfg.SessionEnd.LogDirectory != ".state/logs/sessions" {
		t.Errorf("SessionEnd.LogDirectory = %q, want default %q", cfg.SessionEnd.LogDirectory, ".state/logs/sessions")
	}
	if cfg.PostToolUse.LogDirectory != ".state/logs/sessions" {
		t.Errorf("PostToolUse.LogDirectory = %q, want default %q", cfg.PostToolUse.LogDirectory, ".state/logs/sessions")
	}
	if cfg.Stop.LogDirectory != ".state/logs/sessions" {
		t.Errorf("Stop.LogDirectory = %q, want default %q", cfg.Stop.LogDirectory, ".state/logs/sessions")
	}
	if cfg.UserPrompt.LogDirectory != ".state/logs/sessions" {
		t.Errorf("UserPrompt.LogDirectory = %q, want default %q", cfg.UserPrompt.LogDirectory, ".state/logs/sessions")
	}
}

func TestResolveLogDir(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		projectDir string
		logDir     string
		want       string
	}{
		{"relative path", "/project", ".state/logs/sessions", "/project/.state/logs/sessions"},
		{"absolute path", "/project", "/var/log/codeflow", "/var/log/codeflow"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := resolveLogDir(tt.projectDir, tt.logDir)
			if got != tt.want {
				t.Errorf("resolveLogDir() = %q, want %q", got, tt.want)
			}
		})
	}
}
