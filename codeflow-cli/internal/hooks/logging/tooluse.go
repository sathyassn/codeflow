package logging

import (
	"encoding/json"
	"io"
	"regexp"
	"strings"
)

// toolUseInput represents stdin JSON from Claude Code PostToolUse hooks.
type toolUseInput struct {
	ToolName   string          `json:"tool_name"`
	ToolInput  json.RawMessage `json:"tool_input"`
	ToolResult json.RawMessage `json:"tool_result"`
}

// LogToolUse reads stdin JSON and writes a tool_completed event to
// tool-use-{date}.jsonl. This is the highest-frequency hook -- it
// minimizes allocations. Always returns nil.
func LogToolUse(w *ActivityWriter, stdin io.Reader, projectDir string, cfg *Config) error {
	if !cfg.PostToolUse.Enabled {
		return nil
	}

	input := parseJSON[toolUseInput](stdin)
	if input.ToolName == "" {
		return nil
	}

	// Check tool filter.
	if len(cfg.PostToolUse.ToolsToLog) > 0 && !isToolLogged(input.ToolName, cfg.PostToolUse.ToolsToLog) {
		return nil
	}

	sessionID := ResolveSessionID(projectDir)

	// Summarize tool input.
	toolInputStr := summarizeJSON(input.ToolInput)

	// Truncate and optionally redact tool result.
	resultStr := summarizeJSON(input.ToolResult)
	truncated := false
	if len(resultStr) > cfg.PostToolUse.MaxResultSize {
		resultStr = resultStr[:cfg.PostToolUse.MaxResultSize] + "...[truncated]"
		truncated = true
	}

	if cfg.PostToolUse.RedactSensit {
		toolInputStr = redactSensitive(toolInputStr)
		resultStr = redactSensitive(resultStr)
	}

	record := map[string]any{
		"ts":               w.Timestamp(),
		"session_id":       sessionID,
		"event":            "tool_completed",
		"tool_name":        input.ToolName,
		"tool_input":       toolInputStr,
		"tool_result":      resultStr,
		"result_truncated": truncated,
	}

	return w.Append("tool-use", record)
}

// isToolLogged checks whether a tool name is in the configured log list.
func isToolLogged(name string, allowed []string) bool {
	for _, a := range allowed {
		if a == name {
			return true
		}
	}
	return false
}

// summarizeJSON converts raw JSON to a compact string representation.
func summarizeJSON(raw json.RawMessage) string {
	if raw == nil || len(raw) == 0 {
		return ""
	}
	return strings.TrimSpace(string(raw))
}

// sensitivePatterns matches common secret patterns for redaction.
var sensitivePatterns = []*regexp.Regexp{
	regexp.MustCompile(`(?i)(password\s*[:=]\s*)\S+`),
	regexp.MustCompile(`(?i)(token\s*[:=]\s*)\S+`),
	regexp.MustCompile(`(?i)(key\s*[:=]\s*)\S+`),
	regexp.MustCompile(`(?i)(secret\s*[:=]\s*)\S+`),
	regexp.MustCompile(`Bearer\s+[A-Za-z0-9_-]+`),
	regexp.MustCompile(`[A-Za-z0-9]{40,}`),
	regexp.MustCompile(`[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}`),
}

// sensitiveReplacements provides the replacement string for each pattern.
var sensitiveReplacements = []string{
	"${1}[REDACTED]",
	"${1}[REDACTED]",
	"${1}[REDACTED]",
	"${1}[REDACTED]",
	"Bearer [REDACTED]",
	"[LONG_TOKEN_REDACTED]",
	"[EMAIL_REDACTED]",
}

// redactSensitive removes common secret patterns from text.
func redactSensitive(text string) string {
	for i, pat := range sensitivePatterns {
		text = pat.ReplaceAllString(text, sensitiveReplacements[i])
	}
	return text
}
