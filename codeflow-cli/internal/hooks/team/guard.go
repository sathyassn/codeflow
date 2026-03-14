package team

import (
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"time"

	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
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
//   - If TeamDelete and pf-6 sentinel exists AND all pipeline stage sentinels exist, allow.
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

	// PF7-END gate: if pf-6 sentinel exists, check pipeline stages then allow TeamDelete.
	if isTeamDelete {
		pf6Path := filepath.Join(sentinelDir, "pathflow-pf-6")
		if _, err := os.Stat(pf6Path); err == nil {
			// Also verify ALL pipeline stage sentinels exist.
			if v := checkPipelineStageSentinels(sessionDir, sentinelDir); v != nil {
				return v, nil
			}
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

// checkPipelineStageSentinels verifies that all pipeline stage sentinels exist
// for the current work type. Returns a blocking verdict if any are missing.
func checkPipelineStageSentinels(sessionDir, sentinelDir string) *Verdict {
	workType := sentinel.ReadWorkTypeFromSessionStatus(sessionDir)
	if workType == "" {
		// No work type available — allow through.
		return nil
	}

	projectDir := filepath.Join(sentinelDir, "..", "..", "..", "..")
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")

	pipelines, err := sentinel.LoadPipelines(configDir)
	if err != nil {
		// Config load failure — graceful degradation, allow through.
		slog.Warn("team-guard: cannot load pipelines for stage check", "error", err)
		return nil
	}

	pipeline, exists := pipelines[workType]
	if !exists {
		// Unknown work type — allow through.
		return nil
	}

	ok, missing := sentinel.VerifyCumulativeStageSentinels(sentinelDir, pipeline, len(pipeline)-1)
	if !ok {
		return &Verdict{
			Allow: false,
			Reason: fmt.Sprintf(
				"BLOCKED: TeamDelete blocked — stage sentinel missing: %s\n"+
					"All pipeline stages must complete before team deletion.\n"+
					"Pipeline for %s: %v\n",
				missing, workType, pipeline,
			),
		}
	}

	return nil
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

	// 5. Log team delete completion to cleanup JSONL.
	logTeamDeleteCompleted(projectDir, sessionID)

	return nil
}

// logTeamDeleteCompleted writes a team_delete_completed event to the cleanup JSONL log.
func logTeamDeleteCompleted(projectDir, sessionID string) {
	logDir := filepath.Join(projectDir, ".state", "logs", "sessions")
	if err := os.MkdirAll(logDir, 0o755); err != nil {
		return
	}

	now := time.Now().UTC()
	entry := map[string]any{
		"event":      "team_delete_completed",
		"session_id": sessionID,
		"timestamp":  now.Format(time.RFC3339),
	}
	data, err := json.Marshal(entry)
	if err != nil {
		return
	}

	date := now.Format("2006-01-02")
	logFile := filepath.Join(logDir, "cleanup-"+date+".jsonl")
	f, err := os.OpenFile(logFile, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		return
	}
	defer f.Close()
	_, _ = f.Write(append(data, '\n'))
}
