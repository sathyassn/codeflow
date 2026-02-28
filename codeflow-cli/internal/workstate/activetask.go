// Package workstate manages the active task bridge file and memory event
// recording for the CodeFlow CLI.
package workstate

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

// ActiveTaskFile is the filename for the active task bridge file.
const ActiveTaskFile = "active-task.json"

// ErrNoActiveTask is returned when the active task file does not exist.
var ErrNoActiveTask = errors.New("workstate: no active task")

// ActiveTask represents the current task context used by hooks and agents.
// JSON tags match the existing active-task.json schema written by cf-work-state.sh.
type ActiveTask struct {
	TaskID       string `json:"task_id"`
	EpicID       string `json:"epic_id,omitempty"`
	FormatID     string `json:"task_format_id,omitempty"`
	EpicFormatID string `json:"epic_format_id,omitempty"`
	Title        string `json:"title,omitempty"`
	Status       string `json:"status,omitempty"`
	Branch       string `json:"branch,omitempty"`
	SessionID    string `json:"session_id,omitempty"`
	CreatedAt    string `json:"created_at,omitempty"`
	UpdatedAt    string `json:"updated_at,omitempty"`
	CurrentStage string `json:"current_stage,omitempty"`
	TeamName     string `json:"team_name,omitempty"`
}

// SetActiveTask writes the active task as JSON to the runtime directory.
// The write is atomic: data is written to a temporary file and then renamed.
// The directory is created if it does not exist.
func SetActiveTask(runtimeDir string, task ActiveTask) error {
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		return fmt.Errorf("workstate: creating directory: %w", err)
	}

	data, err := json.MarshalIndent(task, "", "  ")
	if err != nil {
		return fmt.Errorf("workstate: marshaling task: %w", err)
	}
	data = append(data, '\n')

	target := filepath.Join(runtimeDir, ActiveTaskFile)
	tmpPath := target + ".tmp"

	if err := os.WriteFile(tmpPath, data, 0o644); err != nil {
		return fmt.Errorf("workstate: writing temp file: %w", err)
	}

	if err := os.Rename(tmpPath, target); err != nil {
		_ = os.Remove(tmpPath)
		return fmt.Errorf("workstate: renaming temp file: %w", err)
	}

	return nil
}

// GetActiveTask reads the active task from the runtime directory.
// Returns ErrNoActiveTask if the file does not exist.
func GetActiveTask(runtimeDir string) (*ActiveTask, error) {
	target := filepath.Join(runtimeDir, ActiveTaskFile)

	data, err := os.ReadFile(target)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, ErrNoActiveTask
		}
		return nil, fmt.Errorf("workstate: reading task file: %w", err)
	}

	var task ActiveTask
	if err := json.Unmarshal(data, &task); err != nil {
		return nil, fmt.Errorf("workstate: unmarshaling task: %w", err)
	}

	return &task, nil
}

// ClearActiveTask removes the active task bridge file.
// Clearing a non-existent file is a no-op and returns nil.
func ClearActiveTask(runtimeDir string) error {
	target := filepath.Join(runtimeDir, ActiveTaskFile)

	if err := os.Remove(target); err != nil && !os.IsNotExist(err) {
		return fmt.Errorf("workstate: removing task file: %w", err)
	}

	return nil
}
