package logging

import (
	"encoding/json"
	"os"
	"path/filepath"
)

// Config holds all logging configuration parsed from enforcement-policy.json.
type Config struct {
	SessionStart SessionStartConfig `json:"session_start"`
	SessionEnd   SessionEndConfig   `json:"session_end"`
	PostToolUse  PostToolUseConfig  `json:"post_tool_use"`
	Stop         StopConfig         `json:"stop"`
	UserPrompt   UserPromptConfig   `json:"user_prompt"`
}

// SessionStartConfig holds session-start logging settings.
type SessionStartConfig struct {
	Enabled         bool   `json:"enabled"`
	LogDirectory    string `json:"log_directory"`
	CaptureMetadata bool   `json:"capture_metadata"`
}

// SessionEndConfig holds session-end logging settings.
type SessionEndConfig struct {
	Enabled         bool   `json:"enabled"`
	GenerateSummary bool   `json:"generate_summary"`
	TriggerRotation bool   `json:"trigger_rotation"`
	LogDirectory    string `json:"log_directory"`
}

// PostToolUseConfig holds post-tool-use logging settings.
type PostToolUseConfig struct {
	Enabled       bool     `json:"enabled"`
	LogDirectory  string   `json:"log_directory"`
	MaxResultSize int      `json:"max_result_size"`
	RedactSensit  bool     `json:"redact_sensitive"`
	ToolsToLog    []string `json:"tools_to_log"`
}

// StopConfig holds stop logging settings.
type StopConfig struct {
	Enabled            bool   `json:"enabled"`
	LogDirectory       string `json:"log_directory"`
	CaptureTaskContext bool   `json:"capture_task_context"`
}

// UserPromptConfig holds user-prompt-submit logging settings.
type UserPromptConfig struct {
	Enabled         bool   `json:"enabled"`
	LogDirectory    string `json:"log_directory"`
	CaptureFullText bool   `json:"capture_full_text"`
	DetectIntent    bool   `json:"detect_intent"`
	PrivacyMode     bool   `json:"privacy_mode"`
}

// DefaultConfig returns a Config with production default values.
func DefaultConfig() *Config {
	return &Config{
		SessionStart: SessionStartConfig{
			Enabled:         true,
			LogDirectory:    ".state/logs/sessions",
			CaptureMetadata: true,
		},
		SessionEnd: SessionEndConfig{
			Enabled:         true,
			GenerateSummary: true,
			TriggerRotation: true,
			LogDirectory:    ".state/logs/sessions",
		},
		PostToolUse: PostToolUseConfig{
			Enabled:       true,
			LogDirectory:  ".state/logs/sessions",
			MaxResultSize: 2000,
			RedactSensit:  true,
		},
		Stop: StopConfig{
			Enabled:            true,
			LogDirectory:       ".state/logs/sessions",
			CaptureTaskContext: true,
		},
		UserPrompt: UserPromptConfig{
			Enabled:         true,
			LogDirectory:    ".state/logs/sessions",
			CaptureFullText: true,
			DetectIntent:    true,
			PrivacyMode:     false,
		},
	}
}

// ReadConfig reads logging configuration from enforcement-policy.json.
// Returns DefaultConfig on any error.
func ReadConfig(projectDir string) *Config {
	cfg := DefaultConfig()

	path := filepath.Join(projectDir, ".codeflow", "config", "enforcement", "enforcement-policy.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return cfg
	}

	var raw struct {
		Logging json.RawMessage `json:"logging"`
	}
	if err := json.Unmarshal(data, &raw); err != nil || raw.Logging == nil {
		return cfg
	}

	// Overlay parsed values onto defaults.
	_ = json.Unmarshal(raw.Logging, cfg)

	// Ensure non-zero defaults for critical fields.
	if cfg.PostToolUse.MaxResultSize <= 0 {
		cfg.PostToolUse.MaxResultSize = 2000
	}
	if cfg.SessionStart.LogDirectory == "" {
		cfg.SessionStart.LogDirectory = ".state/logs/sessions"
	}
	if cfg.SessionEnd.LogDirectory == "" {
		cfg.SessionEnd.LogDirectory = ".state/logs/sessions"
	}
	if cfg.PostToolUse.LogDirectory == "" {
		cfg.PostToolUse.LogDirectory = ".state/logs/sessions"
	}
	if cfg.Stop.LogDirectory == "" {
		cfg.Stop.LogDirectory = ".state/logs/sessions"
	}
	if cfg.UserPrompt.LogDirectory == "" {
		cfg.UserPrompt.LogDirectory = ".state/logs/sessions"
	}

	return cfg
}

// resolveLogDir returns the absolute log directory path.
// If the configured directory is relative, it is resolved relative to projectDir.
func resolveLogDir(projectDir, logDir string) string {
	if filepath.IsAbs(logDir) {
		return logDir
	}
	return filepath.Join(projectDir, logDir)
}
