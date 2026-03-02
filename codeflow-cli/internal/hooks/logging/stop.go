package logging

import (
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

// stopInput represents stdin JSON from Claude Code Stop hooks.
type stopInput struct {
	SessionID  string          `json:"session_id"`
	StopReason string          `json:"stop_reason"`
	Usage      json.RawMessage `json:"usage"`
}

// LogStop reads stdin JSON, resolves session ID, and writes a stop event
// to session-{date}.jsonl. Always returns nil.
func LogStop(w *ActivityWriter, stdin io.Reader, projectDir string, cfg *Config) error {
	if !cfg.Stop.Enabled {
		return nil
	}

	input := parseJSON[stopInput](stdin)
	sessionID := ResolveSessionID(projectDir)

	stopReason := input.StopReason
	if stopReason == "" {
		stopReason = "unknown"
	}

	record := map[string]any{
		"ts":          w.Timestamp(),
		"session_id":  sessionID,
		"event":       "stop",
		"stop_reason": stopReason,
	}

	if cfg.Stop.CaptureTaskContext {
		if taskCtx := readTaskContext(projectDir); taskCtx != nil {
			record["task_context"] = taskCtx
		}
	}

	if branch := gitBranch(projectDir); branch != "" {
		record["git_branch"] = branch
	}
	record["has_uncommitted_changes"] = hasUncommittedChanges(projectDir)

	return w.Append("session", record)
}

// readTaskContext reads task context from active-task.json.
func readTaskContext(projectDir string) map[string]any {
	path := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var task struct {
		TaskID string `json:"task_id"`
		Status string `json:"status"`
	}
	if err := json.Unmarshal(data, &task); err != nil || task.TaskID == "" {
		return nil
	}
	return map[string]any{
		"task_id": task.TaskID,
		"status":  task.Status,
	}
}

// hasUncommittedChanges returns true if git reports uncommitted changes.
func hasUncommittedChanges(projectDir string) bool {
	out, err := exec.Command("git", "-C", projectDir, "status", "--porcelain").Output()
	if err != nil {
		return false
	}
	return len(strings.TrimSpace(string(out))) > 0
}
