package cliutil

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadPathflowConfig_ValidFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	content := `{
		"version": "1.0.0",
		"phases": {
			"PF1-INIT": {
				"name": "Session Start",
				"phase_order": 1,
				"execution_order": "sequential",
				"subject": "PF1-INIT: Initialize",
				"description": "Init phase",
				"activeForm": "Initializing",
				"tasks": [
					{
						"id": "PF1-TSK-01",
						"task_order": 1,
						"description": "First task",
						"assigned_to": "team-lead",
						"operation": "TeamCreate"
					}
				],
				"required_tasks": ["PF1-TSK-01"]
			}
		},
		"stages": {
			"WS-DEV": {
				"name": "Development",
				"teammate": "cf-development",
				"subject": "WS-DEV: Dev",
				"description": "Code impl",
				"activeForm": "Implementing",
				"max_rework_iterations": 3,
				"max_parallel": 3,
				"batch_size": 2
			}
		},
		"pipelines": {
			"FEAT": ["WS-DEV", "WS-REV", "WS-QA"],
			"DOCS": ["WS-DOCS", "WS-REV"]
		},
		"rework": {
			"max_rework_iterations": 3,
			"max_qa_retries": 3,
			"stage_timeout_minutes": 30,
			"escalation": "lead"
		}
	}`

	if err := os.WriteFile(filepath.Join(cfgDir, "pathflow-config.json"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := LoadPathflowConfig(dir)
	if err != nil {
		t.Fatalf("LoadPathflowConfig() error: %v", err)
	}

	if cfg.Version != "1.0.0" {
		t.Errorf("Version = %q, want %q", cfg.Version, "1.0.0")
	}

	// Verify phases.
	phase, ok := cfg.Phases["PF1-INIT"]
	if !ok {
		t.Fatal("Phases missing PF1-INIT")
	}
	if phase.Name != "Session Start" {
		t.Errorf("Phase name = %q, want %q", phase.Name, "Session Start")
	}
	if phase.PhaseOrder != 1 {
		t.Errorf("Phase order = %d, want %d", phase.PhaseOrder, 1)
	}
	if len(phase.Tasks) != 1 {
		t.Fatalf("Phase tasks count = %d, want 1", len(phase.Tasks))
	}
	if phase.Tasks[0].ID != "PF1-TSK-01" {
		t.Errorf("Task ID = %q, want %q", phase.Tasks[0].ID, "PF1-TSK-01")
	}

	// Verify stages.
	stage, ok := cfg.Stages["WS-DEV"]
	if !ok {
		t.Fatal("Stages missing WS-DEV")
	}
	if stage.MaxParallel != 3 {
		t.Errorf("Stage MaxParallel = %d, want %d", stage.MaxParallel, 3)
	}

	// Verify pipelines.
	pipeline, ok := cfg.Pipelines["FEAT"]
	if !ok {
		t.Fatal("Pipelines missing FEAT")
	}
	if len(pipeline) != 3 {
		t.Errorf("FEAT pipeline length = %d, want 3", len(pipeline))
	}

	// Verify rework.
	if cfg.Rework.MaxReworkIterations != 3 {
		t.Errorf("Rework.MaxReworkIterations = %d, want 3", cfg.Rework.MaxReworkIterations)
	}
	if cfg.Rework.StageTimeoutMinutes != 30 {
		t.Errorf("Rework.StageTimeoutMinutes = %d, want 30", cfg.Rework.StageTimeoutMinutes)
	}
}

func TestLoadPathflowConfig_FileNotFound(t *testing.T) {
	t.Parallel()

	_, err := LoadPathflowConfig(t.TempDir())
	if err == nil {
		t.Fatal("LoadPathflowConfig() expected error for missing file, got nil")
	}
}

func TestLoadPathflowConfig_InvalidJSON(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(filepath.Join(cfgDir, "pathflow-config.json"), []byte("{invalid json}"), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := LoadPathflowConfig(dir)
	if err == nil {
		t.Fatal("LoadPathflowConfig() expected error for invalid JSON, got nil")
	}
}

func TestLoadPathflowConfig_TaskBlockedBy(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	content := `{
		"version": "1.0.0",
		"phases": {
			"PF2-CONTEXT": {
				"name": "Context",
				"phase_order": 2,
				"tasks": [
					{
						"id": "PF2-TSK-01",
						"task_order": 1,
						"blocked_by": ["PF1-TSK-02"],
						"description": "Spawn KL",
						"assigned_to": "team-lead",
						"operation": "spawn"
					}
				],
				"required_tasks": ["PF2-TSK-01"]
			}
		},
		"stages": {},
		"pipelines": {},
		"rework": {}
	}`

	if err := os.WriteFile(filepath.Join(cfgDir, "pathflow-config.json"), []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := LoadPathflowConfig(dir)
	if err != nil {
		t.Fatalf("LoadPathflowConfig() error: %v", err)
	}

	phase := cfg.Phases["PF2-CONTEXT"]
	if len(phase.Tasks[0].BlockedBy) != 1 {
		t.Fatalf("BlockedBy length = %d, want 1", len(phase.Tasks[0].BlockedBy))
	}
	if phase.Tasks[0].BlockedBy[0] != "PF1-TSK-02" {
		t.Errorf("BlockedBy[0] = %q, want %q", phase.Tasks[0].BlockedBy[0], "PF1-TSK-02")
	}
}

func TestLoadPathflowConfig_EmptyFile(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	cfgDir := filepath.Join(dir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(filepath.Join(cfgDir, "pathflow-config.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	cfg, err := LoadPathflowConfig(dir)
	if err != nil {
		t.Fatalf("LoadPathflowConfig() error: %v", err)
	}
	if cfg.Version != "" {
		t.Errorf("Version = %q, want empty", cfg.Version)
	}
	if len(cfg.Phases) != 0 {
		t.Errorf("Phases length = %d, want 0", len(cfg.Phases))
	}
}
