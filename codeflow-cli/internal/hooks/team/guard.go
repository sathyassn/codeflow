package team

import (
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
	"github.com/codeflow/codeflow-cli/internal/pathflow"
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

	// Check if PathFlow is active using session status file.
	if !session.IsPathflowActive(sessionDir) {
		// No active session — allow.
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

// HandlePostTeamDelete performs PathFlow cleanup after a successful TeamDelete.
// This is the PostToolUse counterpart to the PreToolUse guard.
//
// Cleanup operations (in order):
//  1. Update session status to "pf-complete"
//  2. Remove the sentinel directory (.state/sentinels/pathflow/{sessionID}/)
//  3. Remove pathflow-team.json from the session pathflow dir
//  4. Reset pathflow-phase-tasks.json to fresh state via ResetAllPhases
//
// All operations are non-fatal: errors are logged but do not block TeamDelete.
func HandlePostTeamDelete(sessionDir, projectDir, sessionID string) error {
	logger := slog.Default()

	// 1. Update session status to "pf-complete".
	if err := session.UpdatePathflowSessionStatus(sessionDir, func() time.Time { return time.Now().UTC() }, func(s *session.PathflowSessionStatus) {
		s.Status = "pf-complete"
	}); err != nil {
		logger.Warn("post-team-delete: failed to update session status",
			"path", sessionDir, "error", err)
	}

	// 2. Remove sentinel directory.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.RemoveAll(sentinelDir); err != nil {
		logger.Warn("post-team-delete: failed to remove sentinel directory",
			"path", sentinelDir, "error", err)
	}

	// 3. Remove pathflow-team.json.
	teamFilePath := filepath.Join(sessionDir, "pathflow-team.json")
	if err := os.Remove(teamFilePath); err != nil && !os.IsNotExist(err) {
		logger.Warn("post-team-delete: failed to remove pathflow-team.json",
			"path", teamFilePath, "error", err)
	}

	// 4. Reset pathflow-phase-tasks.json to fresh state.
	checkpointPath := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	configPath := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")

	cp := pathflow.NewCheckpoint()
	if err := cp.ResetAllPhases(checkpointPath, configPath); err != nil {
		logger.Warn("post-team-delete: failed to reset checkpoint",
			"path", checkpointPath, "error", err)
	}

	return nil
}
