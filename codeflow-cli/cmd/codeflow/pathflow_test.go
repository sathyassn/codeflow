package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/pathflow"
)

// writeTestConfig writes a minimal pathflow-config.json for CLI tests.
func writeTestConfig(t *testing.T, dir string) string {
	t.Helper()
	config := map[string]any{
		"phases": map[string]any{
			"PF1-INIT": map[string]any{
				"required_tasks": []string{"PF1-TSK-01", "PF1-TSK-02"},
				"tasks": []any{
					map[string]any{"id": "PF1-TSK-01"},
					map[string]any{"id": "PF1-TSK-02"},
				},
			},
			"PF2-CONTEXT": map[string]any{
				"required_tasks": []string{"PF2-TSK-01"},
				"tasks": []any{
					map[string]any{"id": "PF2-TSK-01"},
				},
			},
			"PF3-CLASSIFY": map[string]any{
				"required_tasks": []string{"PF3-TSK-01"},
				"tasks":          []any{map[string]any{"id": "PF3-TSK-01"}},
			},
			"PF4-EXECUTE": map[string]any{
				"required_tasks": []string{"PF4-TSK-01"},
				"tasks":          []any{map[string]any{"id": "PF4-TSK-01"}},
			},
			"PF5-VERIFY": map[string]any{
				"required_tasks": []string{"PF5-TSK-01"},
				"tasks":          []any{map[string]any{"id": "PF5-TSK-01"}},
			},
			"PF6-COMPLETE": map[string]any{
				"required_tasks": []string{"PF6-TSK-01"},
				"tasks":          []any{map[string]any{"id": "PF6-TSK-01"}},
			},
			"PF7-END": map[string]any{
				"required_tasks": []string{"PF7-TSK-01"},
				"tasks":          []any{map[string]any{"id": "PF7-TSK-01"}},
			},
		},
	}
	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		t.Fatalf("marshaling config: %v", err)
	}
	path := filepath.Join(dir, "pathflow-config.json")
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("writing config: %v", err)
	}
	return path
}

func TestRunCheckpointInit(t *testing.T) {
	t.Parallel()

	t.Run("initializes checkpoint file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")
		if err := os.MkdirAll(sessionDir, 0o755); err != nil {
			t.Fatalf("setup: %v", err)
		}

		var buf bytes.Buffer
		err := runCheckpointInit(&buf, sessionDir, configPath)
		if err != nil {
			t.Fatalf("runCheckpointInit() error: %v", err)
		}

		if !strings.Contains(buf.String(), "initialized") {
			t.Errorf("output = %q, want 'initialized'", buf.String())
		}

		// Verify file was created.
		cpPath := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
		if _, err := os.Stat(cpPath); err != nil {
			t.Fatalf("checkpoint file not found: %v", err)
		}
	})

	t.Run("returns error for missing config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		sessionDir := filepath.Join(dir, "session")

		var buf bytes.Buffer
		err := runCheckpointInit(&buf, sessionDir, filepath.Join(dir, "missing.json"))
		if err == nil {
			t.Fatal("runCheckpointInit() expected error for missing config")
		}
	})
}

func TestRunCheckpointRegister(t *testing.T) {
	t.Parallel()

	t.Run("registers task successfully", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")
		sentDir := filepath.Join(dir, "sentinels")

		// Init first.
		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		buf.Reset()
		err := runCheckpointRegister(&buf, sessionDir, sentDir, "PF1", "PF1-TSK-01")
		if err != nil {
			t.Fatalf("runCheckpointRegister() error: %v", err)
		}

		if !strings.Contains(buf.String(), "PF1-TSK-01 registered") {
			t.Errorf("output = %q, want 'registered'", buf.String())
		}
	})

	t.Run("returns exit code 2 for cross-phase block", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")
		sentDir := filepath.Join(dir, "sentinels")

		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		buf.Reset()
		err := runCheckpointRegister(&buf, sessionDir, sentDir, "PF2", "PF2-TSK-01")
		if err == nil {
			t.Fatal("runCheckpointRegister() expected error for cross-phase block")
		}

		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d (ExitConfigError)", code, ExitConfigError)
		}
	})
}

func TestRunCheckpointComplete(t *testing.T) {
	t.Parallel()

	t.Run("completes task and creates sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")
		sentDir := filepath.Join(dir, "sentinels")

		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		// Complete both PF1 tasks.
		for _, tid := range []string{"PF1-TSK-01", "PF1-TSK-02"} {
			buf.Reset()
			if err := runCheckpointComplete(&buf, sessionDir, sentDir, "PF1", tid); err != nil {
				t.Fatalf("runCheckpointComplete(%s) error: %v", tid, err)
			}
			if !strings.Contains(buf.String(), "completed") {
				t.Errorf("output for %s = %q, want 'completed'", tid, buf.String())
			}
		}

		// Verify sentinel.
		if !pathflow.Exists(sentDir, "pf-1") {
			t.Error("pf-1 sentinel not created after completing all PF1 tasks")
		}
	})
}

func TestRunCheckpointSkip(t *testing.T) {
	t.Parallel()

	t.Run("skips task successfully", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")
		sentDir := filepath.Join(dir, "sentinels")

		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		buf.Reset()
		err := runCheckpointSkip(&buf, sessionDir, sentDir, "PF1", "PF1-TSK-01")
		if err != nil {
			t.Fatalf("runCheckpointSkip() error: %v", err)
		}

		if !strings.Contains(buf.String(), "skipped") {
			t.Errorf("output = %q, want 'skipped'", buf.String())
		}
	})
}

func TestRunCheckpointStatus(t *testing.T) {
	t.Parallel()

	t.Run("outputs JSON status", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")

		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		buf.Reset()
		err := runCheckpointStatus(&buf, sessionDir, "PF1")
		if err != nil {
			t.Fatalf("runCheckpointStatus() error: %v", err)
		}

		// Verify output is valid JSON.
		var pc pathflow.PhaseCheckpoint
		if err := json.Unmarshal(buf.Bytes(), &pc); err != nil {
			t.Fatalf("output is not valid JSON: %v\nOutput: %s", err, buf.String())
		}

		if len(pc.Expected) != 2 {
			t.Errorf("Expected tasks = %d, want 2", len(pc.Expected))
		}
	})

	t.Run("returns error for unknown phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeTestConfig(t, dir)
		sessionDir := filepath.Join(dir, "session")

		var buf bytes.Buffer
		if err := runCheckpointInit(&buf, sessionDir, configPath); err != nil {
			t.Fatalf("init: %v", err)
		}

		buf.Reset()
		err := runCheckpointStatus(&buf, sessionDir, "PF99")
		if err == nil {
			t.Fatal("runCheckpointStatus() expected error for unknown phase")
		}
	})
}

func TestRunPhaseTransition(t *testing.T) {
	t.Parallel()

	t.Run("records valid phase transition", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runPhaseTransition(t.Context(), &buf, dir, "ses-test-001", "PF1-INIT", "entered")
		if err != nil {
			t.Fatalf("runPhaseTransition() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"status":"recorded"`) {
			t.Errorf("output = %q, want recorded status", buf.String())
		}
		if !strings.Contains(buf.String(), `"phase":"PF1-INIT"`) {
			t.Errorf("output = %q, want phase PF1-INIT", buf.String())
		}
	})

	t.Run("returns error for invalid phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runPhaseTransition(t.Context(), &buf, dir, "ses-test-002", "PF9-INVALID", "entered")
		if err == nil {
			t.Fatal("expected error for invalid phase")
		}
		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d", code, ExitConfigError)
		}
	})

	t.Run("returns error for bad logs dir", func(t *testing.T) {
		t.Parallel()
		var buf bytes.Buffer
		err := runPhaseTransition(t.Context(), &buf, "/dev/null/bad", "ses-test-003", "PF1-INIT", "entered")
		if err == nil {
			t.Fatal("expected error for bad logs dir")
		}
		code := exitCode(err)
		if code != ExitRuntimeError {
			t.Errorf("exit code = %d, want %d", code, ExitRuntimeError)
		}
	})
}

func TestRunStageTransition(t *testing.T) {
	t.Parallel()

	t.Run("records valid stage transition without verdict", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runStageTransition(t.Context(), &buf, dir, "ses-test-001", "WS-DEV", "in_progress", 1, "")
		if err != nil {
			t.Fatalf("runStageTransition() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"stage":"WS-DEV"`) {
			t.Errorf("output = %q, want stage WS-DEV", buf.String())
		}
	})

	t.Run("records stage transition with verdict", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runStageTransition(t.Context(), &buf, dir, "ses-test-002", "WS-REV", "complete", 2, "approved")
		if err != nil {
			t.Fatalf("runStageTransition() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"iteration":2`) {
			t.Errorf("output = %q, want iteration 2", buf.String())
		}
	})

	t.Run("returns error for invalid stage", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runStageTransition(t.Context(), &buf, dir, "ses-test-003", "WS-INVALID", "in_progress", 1, "")
		if err == nil {
			t.Fatal("expected error for invalid stage")
		}
		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d", code, ExitConfigError)
		}
	})

	t.Run("returns error for bad logs dir", func(t *testing.T) {
		t.Parallel()
		var buf bytes.Buffer
		err := runStageTransition(t.Context(), &buf, "/dev/null/bad", "ses-test-004", "WS-DEV", "in_progress", 1, "")
		if err == nil {
			t.Fatal("expected error for bad logs dir")
		}
	})
}

func TestRunSessionRegister(t *testing.T) {
	t.Parallel()

	t.Run("registers session with interactive mode", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runSessionRegister(t.Context(), &buf, dir, "ses-test-001", "interactive")
		if err != nil {
			t.Fatalf("runSessionRegister() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"status":"registered"`) {
			t.Errorf("output = %q, want registered status", buf.String())
		}
		if !strings.Contains(buf.String(), `"tracking_level":"pending"`) {
			t.Errorf("output = %q, want tracking_level pending", buf.String())
		}
		if !strings.Contains(buf.String(), `"interaction_mode":"interactive"`) {
			t.Errorf("output = %q, want interaction_mode interactive", buf.String())
		}
	})

	t.Run("registers session with autorun mode", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runSessionRegister(t.Context(), &buf, dir, "ses-test-002", "autorun")
		if err != nil {
			t.Fatalf("runSessionRegister() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"interaction_mode":"autorun"`) {
			t.Errorf("output = %q, want interaction_mode autorun", buf.String())
		}
	})

	t.Run("returns error for invalid mode", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runSessionRegister(t.Context(), &buf, dir, "ses-test-003", "batch")
		if err == nil {
			t.Fatal("expected error for invalid mode")
		}
		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d", code, ExitConfigError)
		}
	})

	t.Run("returns error for bad logs dir", func(t *testing.T) {
		t.Parallel()
		var buf bytes.Buffer
		err := runSessionRegister(t.Context(), &buf, "/dev/null/bad", "ses-test-004", "interactive")
		if err == nil {
			t.Fatal("expected error for bad logs dir")
		}
	})
}

func TestRunTaskUpdate(t *testing.T) {
	t.Parallel()

	t.Run("records valid task update", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runTaskUpdate(t.Context(), &buf, dir, "ses-test-001", "PF3-TSK-01", "completed")
		if err != nil {
			t.Fatalf("runTaskUpdate() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"task_id":"PF3-TSK-01"`) {
			t.Errorf("output = %q, want task_id PF3-TSK-01", buf.String())
		}
		if !strings.Contains(buf.String(), `"task_status":"completed"`) {
			t.Errorf("output = %q, want task_status completed", buf.String())
		}
	})

	t.Run("returns error for invalid task_id", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runTaskUpdate(t.Context(), &buf, dir, "ses-test-002", "INVALID-ID", "completed")
		if err == nil {
			t.Fatal("expected error for invalid task_id")
		}
		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d", code, ExitConfigError)
		}
	})

	t.Run("returns error for bad logs dir", func(t *testing.T) {
		t.Parallel()
		var buf bytes.Buffer
		err := runTaskUpdate(t.Context(), &buf, "/dev/null/bad", "ses-test-003", "PF3-TSK-01", "completed")
		if err == nil {
			t.Fatal("expected error for bad logs dir")
		}
	})
}

func TestRunSessionMetadata(t *testing.T) {
	t.Parallel()

	t.Run("records valid session metadata", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runSessionMetadata(t.Context(), &buf, dir, "ses-test-001", "work_type", "FEAT")
		if err != nil {
			t.Fatalf("runSessionMetadata() error: %v", err)
		}
		if !strings.Contains(buf.String(), `"key":"work_type"`) {
			t.Errorf("output = %q, want key work_type", buf.String())
		}
		if !strings.Contains(buf.String(), `"value":"FEAT"`) {
			t.Errorf("output = %q, want value FEAT", buf.String())
		}
	})

	t.Run("returns error for empty key", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		var buf bytes.Buffer
		err := runSessionMetadata(t.Context(), &buf, dir, "ses-test-002", "", "FEAT")
		if err == nil {
			t.Fatal("expected error for empty key")
		}
		code := exitCode(err)
		if code != ExitConfigError {
			t.Errorf("exit code = %d, want %d", code, ExitConfigError)
		}
	})

	t.Run("returns error for bad logs dir", func(t *testing.T) {
		t.Parallel()
		var buf bytes.Buffer
		err := runSessionMetadata(t.Context(), &buf, "/dev/null/bad", "ses-test-003", "key", "val")
		if err == nil {
			t.Fatal("expected error for bad logs dir")
		}
	})
}

func TestNewPathflowCmd(t *testing.T) {
	t.Parallel()

	cmd := newPathflowCmd()
	if cmd.Use != "pathflow" {
		t.Errorf("cmd.Use = %q, want %q", cmd.Use, "pathflow")
	}

	// Verify checkpoint subcommand exists.
	found := false
	for _, sub := range cmd.Commands() {
		if sub.Use == "checkpoint" {
			found = true
			break
		}
	}
	if !found {
		t.Error("checkpoint subcommand not found")
	}
}

func TestCheckpointPath(t *testing.T) {
	t.Parallel()

	got := checkpointPath("/tmp/session")
	want := filepath.Join("/tmp/session", "pathflow-phase-tasks.json")
	if got != want {
		t.Errorf("checkpointPath() = %q, want %q", got, want)
	}
}

func TestIsCrossPhaseBlock(t *testing.T) {
	t.Parallel()

	t.Run("returns true for cross-phase error", func(t *testing.T) {
		t.Parallel()
		err := pathflow.ErrCrossPhaseBlock
		if !isCrossPhaseBlock(err) {
			t.Error("isCrossPhaseBlock() = false for ErrCrossPhaseBlock")
		}
	})

	t.Run("returns false for nil", func(t *testing.T) {
		t.Parallel()
		if isCrossPhaseBlock(nil) {
			t.Error("isCrossPhaseBlock(nil) = true")
		}
	})

	t.Run("returns false for other errors", func(t *testing.T) {
		t.Parallel()
		if isCrossPhaseBlock(os.ErrNotExist) {
			t.Error("isCrossPhaseBlock(ErrNotExist) = true")
		}
	})
}
