package main

import (
	"encoding/json"
	"io"
	"strings"

	"github.com/spf13/cobra"
)

// readStdinBytes reads all bytes from the command's stdin.
func readStdinBytes(cmd *cobra.Command) ([]byte, error) {
	return io.ReadAll(cmd.InOrStdin())
}

// isSettingsTemplateEdit checks if the hook input JSON is an Edit/Write
// operation targeting a settings-templates/*.json file.
func isSettingsTemplateEdit(data []byte) bool {
	if len(data) == 0 {
		return false
	}

	var input struct {
		ToolName  string `json:"tool_name"`
		ToolInput struct {
			FilePath string `json:"file_path"`
		} `json:"tool_input"`
	}

	if err := json.Unmarshal(data, &input); err != nil {
		return false
	}

	if input.ToolName != "Edit" && input.ToolName != "Write" {
		return false
	}

	if !strings.Contains(input.ToolInput.FilePath, ".claude/settings-templates/") {
		return false
	}

	if !strings.HasSuffix(input.ToolInput.FilePath, ".json") {
		return false
	}

	return true
}
