package sentinel

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// setupCoverageProjectEnv creates a full project directory structure for
// config-driven coverage tests. Returns sessionDir, sentinelDir, configDir.
func setupCoverageProjectEnv(t *testing.T) (string, string, string) {
	t.Helper()
	projectDir := t.TempDir()
	sid := "ses-covtest"
	sessionDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	for _, d := range []string{sessionDir, sentinelDir, configDir} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	return sessionDir, sentinelDir, configDir
}

func TestSetCheckpointContext(t *testing.T) {
	t.Parallel()

	t.Run("sets work_type", func(t *testing.T) {
		t.Parallel()
		sessionDir, _ := setupCheckpointEnv(t)

		writeTestCheckpoint(t, sessionDir, map[string]any{
			"PF1": map[string]any{
				"expected":         []string{"PF1-TSK-01"},
				"conditions":       map[string]any{},
				"registered":       map[string]any{},
				"completed":        map[string]any{},
				"skipped":          map[string]any{},
				"sentinel_created": false,
			},
		})

		checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
		if err := SetCheckpointContext(checkpointFile, "work_type", "FEAT"); err != nil {
			t.Fatalf("SetCheckpointContext() error: %v", err)
		}

		data, err := readCheckpoint(checkpointFile)
		if err != nil {
			t.Fatalf("readCheckpoint() error: %v", err)
		}
		if data.Context == nil || data.Context.WorkType != "FEAT" {
			t.Errorf("work_type = %q, want %q", data.Context.WorkType, "FEAT")
		}
	})

	t.Run("sets origin", func(t *testing.T) {
		t.Parallel()
		sessionDir, _ := setupCheckpointEnv(t)

		writeTestCheckpoint(t, sessionDir, map[string]any{
			"PF1": map[string]any{
				"expected":         []string{"PF1-TSK-01"},
				"conditions":       map[string]any{},
				"registered":       map[string]any{},
				"completed":        map[string]any{},
				"skipped":          map[string]any{},
				"sentinel_created": false,
			},
		})

		checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
		if err := SetCheckpointContext(checkpointFile, "origin", "planned"); err != nil {
			t.Fatalf("SetCheckpointContext() error: %v", err)
		}

		data, err := readCheckpoint(checkpointFile)
		if err != nil {
			t.Fatalf("readCheckpoint() error: %v", err)
		}
		if data.Context == nil || data.Context.Origin != "planned" {
			t.Errorf("origin = %q, want %q", data.Context.Origin, "planned")
		}
	})

	t.Run("rejects unknown key", func(t *testing.T) {
		t.Parallel()
		sessionDir, _ := setupCheckpointEnv(t)

		writeTestCheckpoint(t, sessionDir, map[string]any{
			"PF1": map[string]any{
				"expected":         []string{"PF1-TSK-01"},
				"conditions":       map[string]any{},
				"registered":       map[string]any{},
				"completed":        map[string]any{},
				"skipped":          map[string]any{},
				"sentinel_created": false,
			},
		})

		checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
		err := SetCheckpointContext(checkpointFile, "unknown_key", "value")
		if err == nil {
			t.Fatal("expected error for unknown key")
		}
		if !strings.Contains(err.Error(), "unknown key") {
			t.Errorf("error = %q, want substring %q", err.Error(), "unknown key")
		}
	})

	t.Run("creates context when nil", func(t *testing.T) {
		t.Parallel()
		sessionDir, _ := setupCheckpointEnv(t)

		// Write checkpoint without context.
		writeTestCheckpoint(t, sessionDir, map[string]any{
			"PF1": map[string]any{
				"expected":         []string{"PF1-TSK-01"},
				"conditions":       map[string]any{},
				"registered":       map[string]any{},
				"completed":        map[string]any{},
				"skipped":          map[string]any{},
				"sentinel_created": false,
			},
		})

		checkpointFile := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
		if err := SetCheckpointContext(checkpointFile, "work_type", "FIX"); err != nil {
			t.Fatalf("SetCheckpointContext() error: %v", err)
		}

		data, err := readCheckpoint(checkpointFile)
		if err != nil {
			t.Fatalf("readCheckpoint() error: %v", err)
		}
		if data.Context == nil {
			t.Fatal("context should be created")
		}
		if data.Context.WorkType != "FIX" {
			t.Errorf("work_type = %q, want %q", data.Context.WorkType, "FIX")
		}
	})
}

func TestExtractTaskNum(t *testing.T) {
	t.Parallel()

	tests := []struct {
		taskID  string
		want    int
		wantErr bool
	}{
		{"PF4-TSK-05", 5, false},
		{"PF4-TSK-06", 6, false},
		{"PF4-TSK-07", 7, false},
		{"PF1-TSK-01", 1, false},
		{"invalid", 0, true},
		{"PF-TSK-01", 0, true},
	}

	for _, tt := range tests {
		t.Run(tt.taskID, func(t *testing.T) {
			t.Parallel()
			got, err := extractTaskNum(tt.taskID)
			if tt.wantErr {
				if err == nil {
					t.Error("expected error")
				}
				return
			}
			if err != nil {
				t.Fatalf("extractTaskNum() error: %v", err)
			}
			if got != tt.want {
				t.Errorf("extractTaskNum(%q) = %d, want %d", tt.taskID, got, tt.want)
			}
		})
	}
}

func TestCheckPF4StageSentinels_NoWorkType(t *testing.T) {
	t.Parallel()

	sessionDir, sentinelDir := setupCheckpointEnv(t)

	writeTestCheckpoint(t, sessionDir, map[string]any{
		"PF4": map[string]any{
			"expected":         []string{"PF4-TSK-05"},
			"conditions":       map[string]any{},
			"registered":       map[string]any{"PF4-TSK-05": "t1"},
			"completed":        map[string]any{},
			"skipped":          map[string]any{},
			"sentinel_created": false,
		},
		// No context -- work_type is empty.
	})

	createTestSentinel(t, sentinelDir, "pf-1")
	createTestSentinel(t, sentinelDir, "pf-2")
	createTestSentinel(t, sentinelDir, "pf-3")

	stdin := `{"task_subject":"PF4-TSK-05: Execute primary stage"}`
	verdict := CompleteCheckpointTask(strings.NewReader(stdin), sessionDir, sentinelDir)

	// Should allow through (no work_type = no pipeline check).
	if !verdict.Allow {
		t.Fatalf("expected allow when no work_type; reason: %s", verdict.Reason)
	}
}

func TestCheckPF4StageSentinels_TaskNumBelow5(t *testing.T) {
	t.Parallel()

	sessionDir, sentinelDir, configDir := setupCoverageProjectEnv(t)

	writeTestConfig(t, configDir, map[string][]string{
		"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
	})

	writeTestCheckpoint(t, sessionDir, map[string]any{
		"PF4": map[string]any{
			"expected":         []string{"PF4-TSK-01", "PF4-TSK-02"},
			"conditions":       map[string]any{},
			"registered":       map[string]any{"PF4-TSK-01": "t1", "PF4-TSK-02": "t2"},
			"completed":        map[string]any{"PF4-TSK-01": "t3"},
			"skipped":          map[string]any{},
			"sentinel_created": false,
		},
		"context": map[string]any{"work_type": "FEAT"},
	})

	createTestSentinel(t, sentinelDir, "pf-1")
	createTestSentinel(t, sentinelDir, "pf-2")
	createTestSentinel(t, sentinelDir, "pf-3")

	// PF4-TSK-02 has task_num=2 which is < 5, so no stage sentinel check.
	stdin := `{"task_subject":"PF4-TSK-02: Begin work session"}`
	verdict := CompleteCheckpointTask(strings.NewReader(stdin), sessionDir, sentinelDir)

	if !verdict.Allow {
		t.Fatalf("expected allow for task_num < 5; reason: %s", verdict.Reason)
	}
}

func TestCheckPF4StageSentinels_PipelineIndexOutOfRange(t *testing.T) {
	t.Parallel()

	sessionDir, sentinelDir, configDir := setupCoverageProjectEnv(t)

	// DOCS pipeline has only 2 entries, so PF4-TSK-07 (index 2) is out of range.
	writeTestConfig(t, configDir, map[string][]string{
		"DOCS": {"WS-DOCS", "WS-REV"},
	})

	writeTestCheckpoint(t, sessionDir, map[string]any{
		"PF4": map[string]any{
			"expected":         []string{"PF4-TSK-07"},
			"conditions":       map[string]any{},
			"registered":       map[string]any{"PF4-TSK-07": "t1"},
			"completed":        map[string]any{},
			"skipped":          map[string]any{},
			"sentinel_created": false,
		},
		"context": map[string]any{"work_type": "DOCS"},
	})

	createTestSentinel(t, sentinelDir, "pf-1")
	createTestSentinel(t, sentinelDir, "pf-2")
	createTestSentinel(t, sentinelDir, "pf-3")

	stdin := `{"task_subject":"PF4-TSK-07: Execute WS-QA stage"}`
	verdict := CompleteCheckpointTask(strings.NewReader(stdin), sessionDir, sentinelDir)

	// Should allow -- pipeline_index >= len(pipeline) means auto-skip.
	if !verdict.Allow {
		t.Fatalf("expected allow for pipeline index out of range; reason: %s", verdict.Reason)
	}
}

func TestIsPhaseComplete_PF4WithSentinels(t *testing.T) {
	t.Parallel()

	// Test the PF4 pipeline stage sentinel check inside isPhaseComplete.
	// This requires a proper project structure.
	_, sentinelDir, configDir := setupCoverageProjectEnv(t)

	writeTestConfig(t, configDir, map[string][]string{
		"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
	})

	phase := &CheckpointPhase{
		Expected:   []string{"PF4-TSK-05", "PF4-TSK-06", "PF4-TSK-07"},
		Conditions: map[string]string{},
		Completed:  map[string]string{"PF4-TSK-05": "t", "PF4-TSK-06": "t", "PF4-TSK-07": "t"},
		Skipped:    map[string]string{},
	}
	ctx := &CheckpointContext{WorkType: "FEAT"}

	// All tasks completed but ws-dev sentinel missing.
	got := isPhaseComplete(phase, ctx, sentinelDir, 4)
	if got {
		t.Error("isPhaseComplete() = true, want false (ws-dev sentinel missing for PF4)")
	}

	// Add all stage sentinels.
	createTestSentinel(t, sentinelDir, "ws-dev")
	createTestSentinel(t, sentinelDir, "ws-rev")
	createTestSentinel(t, sentinelDir, "ws-qa")

	got = isPhaseComplete(phase, ctx, sentinelDir, 4)
	if !got {
		t.Error("isPhaseComplete() = false, want true (all sentinels present for PF4)")
	}
}

func TestWriteCheckpoint_AtomicRename(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	path := filepath.Join(dir, "checkpoint.json")

	data := &CheckpointData{
		Phases: map[string]*CheckpointPhase{
			"PF1": {
				Expected:   []string{"PF1-TSK-01"},
				Conditions: map[string]string{},
				Registered: map[string]string{},
				Completed:  map[string]string{},
				Skipped:    map[string]string{},
			},
		},
		Context: &CheckpointContext{WorkType: "FEAT", Origin: "planned"},
	}

	if err := writeCheckpoint(path, data); err != nil {
		t.Fatalf("writeCheckpoint() error: %v", err)
	}

	// Verify no tmp file remains.
	if _, err := os.Stat(path + ".tmp"); !os.IsNotExist(err) {
		t.Error("tmp file should not remain after successful write")
	}

	// Verify content.
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var parsed map[string]json.RawMessage
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatal(err)
	}
	if _, ok := parsed["PF1"]; !ok {
		t.Error("PF1 missing in written checkpoint")
	}
	if _, ok := parsed["context"]; !ok {
		t.Error("context missing in written checkpoint")
	}
}
