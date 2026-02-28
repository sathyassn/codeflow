package security

import (
	"encoding/json"
	"fmt"
	"io"
)

// HookInput represents the JSON structure sent by Claude Code on stdin
// to PreToolUse hooks.
type HookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// BashToolInput represents the tool_input for Bash tool calls.
type BashToolInput struct {
	Command                  string `json:"command"`
	DangerouslyDisableSandbox bool   `json:"dangerouslyDisableSandbox"`
}

// ParseHookInput reads and parses the Claude Code hook stdin JSON.
// Returns the tool name, command, and sandbox bypass flag.
func ParseHookInput(r io.Reader) (toolName, command string, sandboxBypass bool, err error) {
	data, err := io.ReadAll(r)
	if err != nil {
		return "", "", false, fmt.Errorf("read stdin: %w", err)
	}

	if len(data) == 0 {
		return "", "", false, nil
	}

	var input HookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return "", "", false, fmt.Errorf("parse hook input: %w", err)
	}

	toolName = input.ToolName
	if toolName != "Bash" {
		return toolName, "", false, nil
	}

	if len(input.ToolInput) == 0 {
		return toolName, "", false, nil
	}

	var bashInput BashToolInput
	if err := json.Unmarshal(input.ToolInput, &bashInput); err != nil {
		return toolName, "", false, fmt.Errorf("parse tool input: %w", err)
	}

	return toolName, bashInput.Command, bashInput.DangerouslyDisableSandbox, nil
}
