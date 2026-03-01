package cliutil

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

// PathflowConfig represents the parsed .codeflow/config/pathflow/pathflow-config.json.
type PathflowConfig struct {
	Version string                    `json:"version"`
	Phases  map[string]PathflowPhase  `json:"phases"`
	Stages  map[string]PathflowStage  `json:"stages"`
	Pipelines map[string][]string     `json:"pipelines"`
	Rework  PathflowRework            `json:"rework"`
}

// PathflowPhase represents a single phase in the PathFlow lifecycle.
type PathflowPhase struct {
	Name           string          `json:"name"`
	PhaseOrder     int             `json:"phase_order"`
	ExecutionOrder string          `json:"execution_order"`
	Subject        string          `json:"subject"`
	Description    string          `json:"description"`
	ActiveForm     string          `json:"activeForm"`
	Tasks          []PathflowTask  `json:"tasks"`
	RequiredTasks  []string        `json:"required_tasks"`
}

// PathflowTask represents a single task within a phase.
type PathflowTask struct {
	ID          string   `json:"id"`
	TaskOrder   int      `json:"task_order"`
	BlockedBy   []string `json:"blocked_by,omitempty"`
	Description string   `json:"description"`
	AssignedTo  string   `json:"assigned_to"`
	Operation   string   `json:"operation"`
	Condition   string   `json:"condition,omitempty"`
}

// PathflowStage represents a work stage configuration.
type PathflowStage struct {
	Name                 string `json:"name"`
	Teammate             string `json:"teammate"`
	Subject              string `json:"subject"`
	Description          string `json:"description"`
	ActiveForm           string `json:"activeForm"`
	MaxReworkIterations  int    `json:"max_rework_iterations,omitempty"`
	MaxQARetries         int    `json:"max_qa_retries,omitempty"`
	MaxParallel          int    `json:"max_parallel,omitempty"`
	BatchSize            int    `json:"batch_size,omitempty"`
}

// PathflowRework holds global rework limits.
type PathflowRework struct {
	MaxReworkIterations  int    `json:"max_rework_iterations"`
	MaxQARetries         int    `json:"max_qa_retries"`
	StageTimeoutMinutes  int    `json:"stage_timeout_minutes"`
	Escalation           string `json:"escalation"`
}

// LoadPathflowConfig reads and unmarshals the pathflow-config.json file
// from the project directory.
func LoadPathflowConfig(projectDir string) (*PathflowConfig, error) {
	path := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read pathflow config: %w", err)
	}

	var cfg PathflowConfig
	if err := json.Unmarshal(data, &cfg); err != nil {
		return nil, fmt.Errorf("parse pathflow config: %w", err)
	}

	return &cfg, nil
}
