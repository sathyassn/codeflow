package logging

import (
	"crypto/sha256"
	"fmt"
	"io"
	"strings"
)

// promptInput represents stdin JSON from Claude Code UserPromptSubmit hooks.
type promptInput struct {
	SessionID  string `json:"session_id"`
	UserPrompt string `json:"user_prompt"`
}

// LogPrompt reads stdin JSON and writes a prompt_submitted event to
// prompts-{date}.jsonl. Always returns nil.
func LogPrompt(w *ActivityWriter, stdin io.Reader, projectDir string, cfg *Config) error {
	if !cfg.UserPrompt.Enabled {
		return nil
	}

	input := parseJSON[promptInput](stdin)
	sessionID := ResolveSessionID(projectDir)

	promptLen := len(input.UserPrompt)
	promptHash := ""
	if input.UserPrompt != "" {
		h := sha256.Sum256([]byte(input.UserPrompt))
		promptHash = fmt.Sprintf("%x", h)
	}

	promptType := "general"
	if cfg.UserPrompt.DetectIntent && input.UserPrompt != "" {
		promptType = detectPromptType(input.UserPrompt)
	}

	record := map[string]any{
		"ts":            w.Timestamp(),
		"session_id":    sessionID,
		"event":         "prompt_submitted",
		"prompt_length": promptLen,
		"prompt_hash":   promptHash,
		"prompt_type":   promptType,
	}

	// In privacy mode, never include full text.
	if !cfg.UserPrompt.PrivacyMode && cfg.UserPrompt.CaptureFullText && input.UserPrompt != "" {
		record["prompt_text"] = input.UserPrompt
	}

	return w.Append("prompts", record)
}

// detectPromptType classifies a user prompt into a type category.
func detectPromptType(prompt string) string {
	if strings.HasPrefix(prompt, "/") {
		return "command"
	}
	lower := strings.ToLower(prompt)
	if strings.Contains(prompt, "?") {
		return "question"
	}
	switch {
	case containsAny(lower, "fix", "bug", "error", "issue"):
		return "debugging"
	case containsAny(lower, "create", "add", "implement", "build"):
		return "creation"
	case containsAny(lower, "update", "change", "modify", "edit"):
		return "modification"
	case containsAny(lower, "review", "check", "verify", "test"):
		return "review"
	case containsAny(lower, "find", "search", "locate", "where"):
		return "navigation"
	}
	return "general"
}

// containsAny returns true if text contains any of the given words.
func containsAny(text string, words ...string) bool {
	for _, w := range words {
		if strings.Contains(text, w) {
			return true
		}
	}
	return false
}
