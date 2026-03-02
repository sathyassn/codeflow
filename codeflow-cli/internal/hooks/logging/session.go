package logging

import (
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// sessionStartInput represents stdin JSON from Claude Code SessionStart hooks.
type sessionStartInput struct {
	SessionID      string `json:"session_id"`
	Source         string `json:"source"`
	PermissionMode string `json:"permission_mode"`
}

// LogSessionStart reads stdin JSON, resolves session ID, and writes a
// session_start event to session-{date}.jsonl. Always returns nil (logging
// never blocks).
func LogSessionStart(w *ActivityWriter, stdin io.Reader, projectDir string, cfg *Config) error {
	if !cfg.SessionStart.Enabled {
		return nil
	}

	input := parseJSON[sessionStartInput](stdin)
	sessionID := ResolveSessionID(projectDir)

	record := map[string]any{
		"ts":         w.Timestamp(),
		"session_id": sessionID,
		"event":      "session_start",
	}

	if cfg.SessionStart.CaptureMetadata {
		metadata := gatherSessionMetadata(projectDir, input.PermissionMode)
		record["metadata"] = metadata
	}

	return w.Append("session", record)
}

// sessionEndInput represents stdin JSON from Claude Code SessionEnd hooks.
type sessionEndInput struct {
	SessionID      string `json:"session_id"`
	TranscriptPath string `json:"transcript_path"`
}

// LogSessionEnd reads stdin JSON, resolves session ID, and writes a
// session_end event to session-{date}.jsonl. Always returns nil.
func LogSessionEnd(w *ActivityWriter, stdin io.Reader, projectDir string, cfg *Config) error {
	if !cfg.SessionEnd.Enabled {
		return nil
	}

	_ = parseJSON[sessionEndInput](stdin)
	sessionID := ResolveSessionID(projectDir)

	record := map[string]any{
		"ts":               w.Timestamp(),
		"session_id":       sessionID,
		"event":            "session_end",
		"duration_seconds": calculateDuration(w, projectDir, sessionID),
	}

	return w.Append("session", record)
}

// calculateDuration attempts to compute session duration from the session
// meta file. Returns 0 if no metadata is available.
func calculateDuration(w *ActivityWriter, projectDir, sessionID string) int64 {
	metaFile := filepath.Join(w.Dir(), fmt.Sprintf("session-%s.meta", sessionID))
	data, err := os.ReadFile(metaFile)
	if err != nil {
		return 0
	}

	var meta struct {
		StartedEpoch int64 `json:"started_epoch"`
	}
	if err := json.Unmarshal(data, &meta); err != nil || meta.StartedEpoch == 0 {
		return 0
	}

	return w.Now().Unix() - meta.StartedEpoch
}

// gatherSessionMetadata collects git and task context for session-start logs.
func gatherSessionMetadata(projectDir, approvalMode string) map[string]any {
	meta := map[string]any{
		"cwd": projectDir,
	}

	if branch := gitBranch(projectDir); branch != "" {
		meta["git_branch"] = branch
	}
	if commit := gitShortCommit(projectDir); commit != "" {
		meta["git_commit"] = commit
	}

	if approvalMode != "" {
		meta["approval_mode"] = approvalMode
	} else {
		meta["approval_mode"] = "standard"
	}

	if taskID := readActiveTaskID(projectDir); taskID != "" {
		meta["active_task"] = taskID
	}

	return meta
}

// gitBranch returns the current git branch or empty string.
func gitBranch(projectDir string) string {
	out, err := exec.Command("git", "-C", projectDir, "branch", "--show-current").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// gitShortCommit returns the short HEAD commit hash or empty string.
func gitShortCommit(projectDir string) string {
	out, err := exec.Command("git", "-C", projectDir, "rev-parse", "--short", "HEAD").Output()
	if err != nil {
		return ""
	}
	return strings.TrimSpace(string(out))
}

// readActiveTaskID reads the active task ID from active-task.json.
func readActiveTaskID(projectDir string) string {
	path := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return ""
	}
	var task struct {
		TaskID string `json:"task_id"`
	}
	if err := json.Unmarshal(data, &task); err != nil {
		return ""
	}
	return task.TaskID
}

// parseJSON reads stdin and unmarshals JSON into T. Returns zero value on error.
func parseJSON[T any](r io.Reader) T {
	var result T
	if r == nil {
		return result
	}
	data, err := io.ReadAll(r)
	if err != nil || len(data) == 0 {
		return result
	}
	_ = json.Unmarshal(data, &result)
	return result
}
