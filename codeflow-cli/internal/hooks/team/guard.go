package team

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// Verdict represents the result of a team guard check.
type Verdict struct {
	// Allow is true if the operation is permitted.
	Allow bool

	// Reason is a human-readable explanation when the operation is blocked.
	Reason string
}

// hookInput represents the JSON structure sent by Claude Code on stdin.
type hookInput struct {
	ToolName  string          `json:"tool_name"`
	ToolInput json.RawMessage `json:"tool_input"`
}

// teammateInput represents the tool_input for Teammate tool calls.
type teammateInput struct {
	Operation string `json:"operation"`
}

// CheckTeamDelete reads Claude Code hook JSON from stdin and determines
// whether a TeamDelete or Teammate cleanup operation should be allowed.
//
// Logic:
//   - If tool is not TeamDelete or Teammate, allow.
//   - If Teammate but operation is not "cleanup", allow.
//   - If pathflow-active flag does not exist, allow (no active session).
//   - If TeamDelete and pf-6 sentinel exists, allow (PF7-END gate passed).
//   - Otherwise, block (PathFlow active, team resources in use).
//
// sessionDir is the path to the session directory, e.g.,
// {projectDir}/.state/session/{sessionID}/pathflow/
//
// sentinelDir is the path to the sentinel directory, e.g.,
// {projectDir}/.state/sentinels/pathflow/{sessionID}/
func CheckTeamDelete(stdin io.Reader, sessionDir, sentinelDir string) (*Verdict, error) {
	data, err := io.ReadAll(stdin)
	if err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("read stdin: %w", err)
	}

	if len(data) == 0 {
		return &Verdict{Allow: true}, nil
	}

	var input hookInput
	if err := json.Unmarshal(data, &input); err != nil {
		return &Verdict{Allow: true}, fmt.Errorf("parse hook input: %w", err)
	}

	isTeamDelete := input.ToolName == "TeamDelete"

	// For non-Teammate and non-TeamDelete tools, allow through.
	if input.ToolName != "Teammate" && !isTeamDelete {
		return &Verdict{Allow: true}, nil
	}

	// For Teammate tool, only gate "cleanup" operations.
	if !isTeamDelete {
		if len(input.ToolInput) == 0 {
			return &Verdict{Allow: true}, nil
		}

		var ti teammateInput
		if err := json.Unmarshal(input.ToolInput, &ti); err != nil {
			return &Verdict{Allow: true}, nil
		}

		if ti.Operation != "cleanup" {
			return &Verdict{Allow: true}, nil
		}
	}

	// Check if PathFlow is active.
	flagPath := filepath.Join(sessionDir, "is-pathflow-active")
	if _, err := os.Stat(flagPath); err != nil {
		// No pathflow-active flag — no active session, allow.
		return &Verdict{Allow: true}, nil
	}

	// PF7-END gate: if pf-6 sentinel exists, allow TeamDelete.
	if isTeamDelete {
		pf6Path := filepath.Join(sentinelDir, "pathflow-pf-6")
		if _, err := os.Stat(pf6Path); err == nil {
			return &Verdict{Allow: true}, nil
		}
	}

	// PathFlow is active, block the operation.
	blockedOp := "cleanup"
	if isTeamDelete {
		blockedOp = "TeamDelete"
	}

	return &Verdict{
		Allow: false,
		Reason: fmt.Sprintf(
			"BLOCKED: Team cleanup/deletion not allowed during active PathFlow\n"+
				"Reason: PathFlow is active - team resources are still in use\n"+
				"Operation: %s\n\n"+
				"Complete the PathFlow workflow (PF7-END) before cleaning up team resources.\n",
			blockedOp,
		),
	}, nil
}

// HandlePostTeamDelete removes the pathflow-active flag after a successful
// TeamDelete. This is the PostToolUse counterpart to the PreToolUse guard.
//
// sessionDir is the path to the session pathflow directory, e.g.,
// {projectDir}/.state/session/{sessionID}/pathflow/
func HandlePostTeamDelete(sessionDir string) error {
	flagPath := filepath.Join(sessionDir, "is-pathflow-active")
	if _, err := os.Stat(flagPath); err != nil {
		// Flag does not exist — nothing to remove.
		return nil
	}
	return os.Remove(flagPath)
}
