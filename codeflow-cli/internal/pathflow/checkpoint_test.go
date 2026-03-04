package pathflow

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"testing"
	"time"
)

// testCheckpoint creates a Checkpoint with a fixed clock for deterministic tests.
func testCheckpoint(t *testing.T) *Checkpoint {
	t.Helper()
	return &Checkpoint{
		Now: func() time.Time {
			return time.Date(2026, 2, 28, 12, 0, 0, 0, time.UTC)
		},
	}
}

// writeMinimalConfig writes a minimal pathflow-config.json for testing.
func writeMinimalConfig(t *testing.T, dir string) string {
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
				"required_tasks": []string{"PF2-TSK-01", "PF2-TSK-02"},
				"tasks": []any{
					map[string]any{"id": "PF2-TSK-01"},
					map[string]any{"id": "PF2-TSK-02"},
				},
			},
			"PF3-CLASSIFY": map[string]any{
				"required_tasks": []string{"PF3-TSK-01", "PF3-TSK-02", "PF3-TSK-03"},
				"tasks": []any{
					map[string]any{"id": "PF3-TSK-01"},
					map[string]any{"id": "PF3-TSK-02"},
					map[string]any{"id": "PF3-TSK-03"},
				},
			},
			"PF4-EXECUTE": map[string]any{
				"required_tasks": []string{"PF4-TSK-01", "PF4-TSK-02", "PF4-TSK-07"},
				"tasks": []any{
					map[string]any{"id": "PF4-TSK-01", "condition": "adhoc_only"},
					map[string]any{"id": "PF4-TSK-02"},
					map[string]any{"id": "PF4-TSK-07", "condition": "if_pipeline_includes_qa"},
				},
			},
			"PF5-VERIFY": map[string]any{
				"required_tasks": []string{"PF5-TSK-01"},
				"tasks": []any{
					map[string]any{"id": "PF5-TSK-01"},
				},
			},
			"PF6-COMPLETE": map[string]any{
				"required_tasks": []string{"PF6-TSK-01"},
				"tasks": []any{
					map[string]any{"id": "PF6-TSK-01"},
				},
			},
			"PF7-END": map[string]any{
				"required_tasks": []string{"PF7-TSK-01"},
				"tasks": []any{
					map[string]any{"id": "PF7-TSK-01"},
				},
			},
		},
	}
	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		t.Fatalf("marshaling test config: %v", err)
	}
	path := filepath.Join(dir, "pathflow-config.json")
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("writing test config: %v", err)
	}
	return path
}

func TestCheckpoint_InitAllPhases(t *testing.T) {
	t.Parallel()

	t.Run("initializes all 7 phases", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases() error: %v", err)
		}

		// Verify all 7 phases exist.
		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		for i := 1; i <= 7; i++ {
			phase := phaseKey(i)
			pc, ok := cf.Phases[phase]
			if !ok {
				t.Errorf("phase %s not found in checkpoint", phase)
				continue
			}
			if len(pc.Expected) == 0 {
				t.Errorf("phase %s has empty expected tasks", phase)
			}
		}
	})

	t.Run("idempotent on re-init", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases() first call: %v", err)
		}

		// Register a task in PF1 to verify it's preserved.
		sentDir := filepath.Join(dir, "sentinels")
		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("RegisterTask(): %v", err)
		}

		// Re-init should not overwrite PF1.
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases() second call: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}
		if _, ok := cf.Phases["PF1"].Registered["PF1-TSK-01"]; !ok {
			t.Error("re-init overwrote PF1 registered tasks")
		}
	})

	t.Run("preserves conditions from config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		// PF4-TSK-01 has condition "adhoc_only".
		if got := cf.Phases["PF4"].Conditions["PF4-TSK-01"]; got != "adhoc_only" {
			t.Errorf("PF4-TSK-01 condition = %q, want %q", got, "adhoc_only")
		}

		// PF4-TSK-07 has condition "if_pipeline_includes_qa".
		if got := cf.Phases["PF4"].Conditions["PF4-TSK-07"]; got != "if_pipeline_includes_qa" {
			t.Errorf("PF4-TSK-07 condition = %q, want %q", got, "if_pipeline_includes_qa")
		}
	})

	t.Run("returns error for missing config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		err := cp.InitAllPhases(cpPath, filepath.Join(dir, "missing.json"))
		if err == nil {
			t.Fatal("InitAllPhases() expected error for missing config")
		}
	})

	t.Run("returns error for invalid config JSON", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		badConfig := filepath.Join(dir, "bad.json")
		if err := os.WriteFile(badConfig, []byte("not json"), 0o644); err != nil {
			t.Fatalf("setup: %v", err)
		}

		cp := testCheckpoint(t)
		err := cp.InitAllPhases(cpPath, badConfig)
		if err == nil {
			t.Fatal("InitAllPhases() expected error for invalid JSON")
		}
	})
}

func TestCheckpoint_RegisterTask(t *testing.T) {
	t.Parallel()

	t.Run("registers task in correct phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("RegisterTask() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		ts, ok := cf.Phases["PF1"].Registered["PF1-TSK-01"]
		if !ok {
			t.Fatal("PF1-TSK-01 not found in registered")
		}
		if ts == "" {
			t.Error("PF1-TSK-01 timestamp is empty")
		}
	})

	t.Run("idempotent on duplicate registration", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("RegisterTask() first call: %v", err)
		}
		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("RegisterTask() second call: %v", err)
		}
	})

	t.Run("blocks cross-phase without sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// PF2 requires PF1 sentinel.
		err := cp.RegisterTask(cpPath, sentDir, "PF2-TSK-01")
		if err == nil {
			t.Fatal("RegisterTask(PF2) expected cross-phase error")
		}
		if !errors.Is(err, ErrCrossPhaseBlock) {
			t.Errorf("RegisterTask(PF2) error = %v, want ErrCrossPhaseBlock", err)
		}
	})

	t.Run("allows PF2 after PF1 sentinel exists", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Create PF1 sentinel.
		if err := Create(sentDir, "pf-1"); err != nil {
			t.Fatalf("creating sentinel: %v", err)
		}

		if err := cp.RegisterTask(cpPath, sentDir, "PF2-TSK-01"); err != nil {
			t.Fatalf("RegisterTask(PF2) after sentinel: %v", err)
		}
	})

	t.Run("rejects invalid task ID format", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		err := cp.RegisterTask(cpPath, sentDir, "INVALID")
		if err == nil {
			t.Fatal("RegisterTask(INVALID) expected error")
		}
	})

	t.Run("returns error for uninitialized phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		// Create empty checkpoint file.
		if err := os.MkdirAll(filepath.Dir(cpPath), 0o755); err != nil {
			t.Fatalf("setup: %v", err)
		}
		if err := os.WriteFile(cpPath, []byte("{}"), 0o644); err != nil {
			t.Fatalf("setup: %v", err)
		}

		cp := testCheckpoint(t)
		err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01")
		if err == nil {
			t.Fatal("RegisterTask() expected error for uninitialized phase")
		}
	})
}

func TestCheckpoint_CompleteTask(t *testing.T) {
	t.Parallel()

	t.Run("marks task completed", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		if _, ok := cf.Phases["PF1"].Completed["PF1-TSK-01"]; !ok {
			t.Error("PF1-TSK-01 not in completed map")
		}
	})

	t.Run("creates sentinel when all tasks complete", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Complete both PF1 tasks.
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask(01) error: %v", err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("CompleteTask(02) error: %v", err)
		}

		// Verify sentinel was created.
		if !Exists(sentDir, "pf-1") {
			t.Error("sentinel pf-1 not created after completing all PF1 tasks")
		}

		// Verify sentinel_created flag.
		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}
		if !cf.Phases["PF1"].SentinelCreated {
			t.Error("PF1.SentinelCreated = false, want true")
		}
	})

	t.Run("idempotent on already completed task", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask() first: %v", err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask() second: %v", err)
		}
	})

	t.Run("no sentinel when partial completion", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Complete only one of two PF1 tasks.
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask() error: %v", err)
		}

		if Exists(sentDir, "pf-1") {
			t.Error("sentinel pf-1 should not exist with only 1 of 2 tasks complete")
		}
	})

	t.Run("idempotent after sentinel already created", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Complete all and trigger sentinel.
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask(01): %v", err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("CompleteTask(02): %v", err)
		}

		// Complete again after sentinel: should be no-op.
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask() after sentinel: %v", err)
		}
	})
}

func TestCheckpoint_SkipTask(t *testing.T) {
	t.Parallel()

	t.Run("marks task skipped", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.SkipTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("SkipTask() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		if _, ok := cf.Phases["PF1"].Skipped["PF1-TSK-02"]; !ok {
			t.Error("PF1-TSK-02 not in skipped map")
		}
	})

	t.Run("creates sentinel when skip completes phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Complete one, skip the other.
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask(): %v", err)
		}
		if err := cp.SkipTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("SkipTask(): %v", err)
		}

		if !Exists(sentDir, "pf-1") {
			t.Error("sentinel pf-1 not created after complete + skip = all done")
		}
	})

	t.Run("idempotent on duplicate skip", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.SkipTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("SkipTask() first: %v", err)
		}
		if err := cp.SkipTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("SkipTask() second: %v", err)
		}
	})
}

func TestCheckpoint_GetStatus(t *testing.T) {
	t.Parallel()

	t.Run("returns phase status", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		pc, err := cp.GetStatus(cpPath, "PF1")
		if err != nil {
			t.Fatalf("GetStatus() error: %v", err)
		}

		if len(pc.Expected) != 2 {
			t.Errorf("GetStatus() expected %d tasks, got %d", 2, len(pc.Expected))
		}
	})

	t.Run("returns error for unknown phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		_, err := cp.GetStatus(cpPath, "PF99")
		if err == nil {
			t.Fatal("GetStatus(PF99) expected error")
		}
	})

	t.Run("returns error for missing checkpoint file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		cp := testCheckpoint(t)
		_, err := cp.GetStatus(filepath.Join(dir, "missing.json"), "PF1")
		if err == nil {
			t.Fatal("GetStatus() expected error for missing file")
		}
	})

	t.Run("status is JSON serializable", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}
		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("register: %v", err)
		}

		pc, err := cp.GetStatus(cpPath, "PF1")
		if err != nil {
			t.Fatalf("GetStatus(): %v", err)
		}

		data, err := json.Marshal(pc)
		if err != nil {
			t.Fatalf("json.Marshal() error: %v", err)
		}
		if len(data) == 0 {
			t.Error("json.Marshal() produced empty output")
		}
	})
}

func TestIsPhaseComplete(t *testing.T) {
	t.Parallel()

	t.Run("empty expected is not complete", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{Expected: nil}
		if IsPhaseComplete(pc, nil) {
			t.Error("IsPhaseComplete() = true for empty expected, want false")
		}
	})

	t.Run("all completed is complete", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:  []string{"PF1-TSK-01", "PF1-TSK-02"},
			Completed: map[string]string{"PF1-TSK-01": "t1", "PF1-TSK-02": "t2"},
			Skipped:   map[string]string{},
		}
		if !IsPhaseComplete(pc, nil) {
			t.Error("IsPhaseComplete() = false, want true")
		}
	})

	t.Run("mixed completed and skipped is complete", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:  []string{"PF1-TSK-01", "PF1-TSK-02"},
			Completed: map[string]string{"PF1-TSK-01": "t1"},
			Skipped:   map[string]string{"PF1-TSK-02": "t2"},
		}
		if !IsPhaseComplete(pc, nil) {
			t.Error("IsPhaseComplete() = false for mixed complete+skip, want true")
		}
	})

	t.Run("auto-skip adhoc_only when origin=planned", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF3-TSK-01", "PF3-TSK-02", "PF3-TSK-03"},
			Conditions: map[string]string{"PF3-TSK-03": "adhoc_only"},
			Completed:  map[string]string{"PF3-TSK-01": "t1", "PF3-TSK-02": "t2"},
			Skipped:    map[string]string{},
		}
		ctx := map[string]string{"origin": "planned"}
		if !IsPhaseComplete(pc, ctx) {
			t.Error("IsPhaseComplete() = false with adhoc_only auto-skip, want true")
		}
	})

	t.Run("no auto-skip adhoc_only when origin=adhoc", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF3-TSK-01", "PF3-TSK-02", "PF3-TSK-03"},
			Conditions: map[string]string{"PF3-TSK-03": "adhoc_only"},
			Completed:  map[string]string{"PF3-TSK-01": "t1", "PF3-TSK-02": "t2"},
			Skipped:    map[string]string{},
		}
		ctx := map[string]string{"origin": "adhoc"}
		if IsPhaseComplete(pc, ctx) {
			t.Error("IsPhaseComplete() = true when adhoc_only should not auto-skip")
		}
	})

	t.Run("auto-skip if_pipeline_includes_qa for DOCS", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF4-TSK-05", "PF4-TSK-07"},
			Conditions: map[string]string{"PF4-TSK-07": "if_pipeline_includes_qa"},
			Completed:  map[string]string{"PF4-TSK-05": "t1"},
			Skipped:    map[string]string{},
		}

		for _, wt := range []string{"DOCS", "PLAN", "SPKE"} {
			ctx := map[string]string{"work_type": wt}
			if !IsPhaseComplete(pc, ctx) {
				t.Errorf("IsPhaseComplete() = false for work_type=%s, want true (QA auto-skip)", wt)
			}
		}
	})

	t.Run("no auto-skip if_pipeline_includes_qa for FEAT", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF4-TSK-05", "PF4-TSK-07"},
			Conditions: map[string]string{"PF4-TSK-07": "if_pipeline_includes_qa"},
			Completed:  map[string]string{"PF4-TSK-05": "t1"},
			Skipped:    map[string]string{},
		}
		ctx := map[string]string{"work_type": "FEAT"}
		if IsPhaseComplete(pc, ctx) {
			t.Error("IsPhaseComplete() = true for FEAT, want false (QA should not auto-skip)")
		}
	})

	t.Run("no auto-skip without context", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF4-TSK-01", "PF4-TSK-02"},
			Conditions: map[string]string{"PF4-TSK-01": "adhoc_only"},
			Completed:  map[string]string{"PF4-TSK-02": "t1"},
			Skipped:    map[string]string{},
		}
		if IsPhaseComplete(pc, nil) {
			t.Error("IsPhaseComplete() = true without context, want false")
		}
	})

	t.Run("unknown condition is not auto-skipped", func(t *testing.T) {
		t.Parallel()
		pc := &PhaseCheckpoint{
			Expected:   []string{"PF1-TSK-01"},
			Conditions: map[string]string{"PF1-TSK-01": "unknown_condition"},
			Completed:  map[string]string{},
			Skipped:    map[string]string{},
		}
		if IsPhaseComplete(pc, map[string]string{"origin": "planned"}) {
			t.Error("IsPhaseComplete() = true for unknown condition")
		}
	})
}

func TestCheckpoint_ResetAllPhases(t *testing.T) {
	t.Parallel()

	t.Run("clears completed state", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)

		// Init and complete PF1.
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup InitAllPhases: %v", err)
		}
		for _, tid := range []string{"PF1-TSK-01", "PF1-TSK-02"} {
			if err := cp.RegisterTask(cpPath, sentDir, tid); err != nil {
				t.Fatalf("setup RegisterTask(%s): %v", tid, err)
			}
			if err := cp.CompleteTask(cpPath, sentDir, tid); err != nil {
				t.Fatalf("setup CompleteTask(%s): %v", tid, err)
			}
		}

		// Verify PF1 is completed before reset.
		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}
		if !cf.Phases["PF1"].SentinelCreated {
			t.Fatal("PF1 should have SentinelCreated=true before reset")
		}

		// Reset.
		if err := cp.ResetAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("ResetAllPhases() error: %v", err)
		}

		// Verify reset state.
		cf, err = readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint after reset: %v", err)
		}

		pf1, ok := cf.Phases["PF1"]
		if !ok {
			t.Fatal("PF1 should exist after reset")
		}
		if len(pf1.Registered) != 0 {
			t.Errorf("PF1.Registered should be empty after reset, got %d entries", len(pf1.Registered))
		}
		if len(pf1.Completed) != 0 {
			t.Errorf("PF1.Completed should be empty after reset, got %d entries", len(pf1.Completed))
		}
		if pf1.SentinelCreated {
			t.Error("PF1.SentinelCreated should be false after reset")
		}
	})

	t.Run("preserves expected tasks from config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)

		// Init, modify, then reset.
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}
		if err := cp.ResetAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("ResetAllPhases() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("reading checkpoint: %v", err)
		}

		// All 7 phases should be present with their expected tasks.
		for i := 1; i <= 7; i++ {
			phase := phaseKey(i)
			pc, ok := cf.Phases[phase]
			if !ok {
				t.Errorf("phase %s not found after reset", phase)
				continue
			}
			if len(pc.Expected) == 0 {
				t.Errorf("phase %s has empty expected tasks after reset", phase)
			}
		}
	})

	t.Run("works when no checkpoint file exists", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)

		// Reset without prior init should create fresh file.
		if err := cp.ResetAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("ResetAllPhases() error: %v", err)
		}

		if _, err := os.Stat(cpPath); err != nil {
			t.Errorf("checkpoint file should be created by ResetAllPhases: %v", err)
		}
	})

	t.Run("returns error for missing config", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		err := cp.ResetAllPhases(cpPath, filepath.Join(dir, "missing.json"))
		if err == nil {
			t.Fatal("ResetAllPhases() expected error for missing config")
		}
	})
}

func TestCheckpoint_FullLifecycle(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	configPath := writeMinimalConfig(t, dir)
	cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
	sentDir := filepath.Join(dir, "sentinels")

	cp := testCheckpoint(t)

	// Step 1: Initialize all phases.
	if err := cp.InitAllPhases(cpPath, configPath); err != nil {
		t.Fatalf("InitAllPhases(): %v", err)
	}

	// Step 2: Register and complete PF1 tasks.
	for _, tid := range []string{"PF1-TSK-01", "PF1-TSK-02"} {
		if err := cp.RegisterTask(cpPath, sentDir, tid); err != nil {
			t.Fatalf("RegisterTask(%s): %v", tid, err)
		}
	}
	for _, tid := range []string{"PF1-TSK-01", "PF1-TSK-02"} {
		if err := cp.CompleteTask(cpPath, sentDir, tid); err != nil {
			t.Fatalf("CompleteTask(%s): %v", tid, err)
		}
	}
	if !Exists(sentDir, "pf-1") {
		t.Fatal("pf-1 sentinel missing after completing PF1")
	}

	// Step 3: PF2 should now be registerable.
	if err := cp.RegisterTask(cpPath, sentDir, "PF2-TSK-01"); err != nil {
		t.Fatalf("RegisterTask(PF2-TSK-01): %v", err)
	}
	if err := cp.RegisterTask(cpPath, sentDir, "PF2-TSK-02"); err != nil {
		t.Fatalf("RegisterTask(PF2-TSK-02): %v", err)
	}

	// Step 4: PF3 should be blocked.
	err := cp.RegisterTask(cpPath, sentDir, "PF3-TSK-01")
	if !errors.Is(err, ErrCrossPhaseBlock) {
		t.Fatalf("RegisterTask(PF3) expected ErrCrossPhaseBlock, got: %v", err)
	}

	// Step 5: Complete PF2.
	if err := cp.CompleteTask(cpPath, sentDir, "PF2-TSK-01"); err != nil {
		t.Fatalf("CompleteTask(PF2-TSK-01): %v", err)
	}
	if err := cp.CompleteTask(cpPath, sentDir, "PF2-TSK-02"); err != nil {
		t.Fatalf("CompleteTask(PF2-TSK-02): %v", err)
	}
	if !Exists(sentDir, "pf-2") {
		t.Fatal("pf-2 sentinel missing after completing PF2")
	}

	// Step 6: PF3 should now work.
	if err := cp.RegisterTask(cpPath, sentDir, "PF3-TSK-01"); err != nil {
		t.Fatalf("RegisterTask(PF3-TSK-01) after PF2 complete: %v", err)
	}

	// Verify final state.
	sentinels, err := List(sentDir)
	if err != nil {
		t.Fatalf("List(): %v", err)
	}
	if len(sentinels) < 2 {
		t.Errorf("expected at least 2 sentinels, got %d: %v", len(sentinels), sentinels)
	}
}

func TestCheckpoint_ConcurrentAccess(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	configPath := writeMinimalConfig(t, dir)
	cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
	sentDir := filepath.Join(dir, "sentinels")

	cp := testCheckpoint(t)
	if err := cp.InitAllPhases(cpPath, configPath); err != nil {
		t.Fatalf("setup: %v", err)
	}

	// Concurrently register the same PF1 tasks from multiple goroutines.
	var wg sync.WaitGroup
	errs := make([]error, 10)

	for i := range 10 {
		wg.Add(1)
		go func(idx int) {
			defer wg.Done()
			taskID := "PF1-TSK-01"
			if idx%2 == 1 {
				taskID = "PF1-TSK-02"
			}
			errs[idx] = cp.RegisterTask(cpPath, sentDir, taskID)
		}(i)
	}
	wg.Wait()

	for i, err := range errs {
		if err != nil {
			t.Errorf("concurrent RegisterTask[%d] error: %v", i, err)
		}
	}

	// Verify checkpoint state is consistent.
	cf, err := readCheckpointFile(cpPath)
	if err != nil {
		t.Fatalf("reading checkpoint: %v", err)
	}

	if _, ok := cf.Phases["PF1"].Registered["PF1-TSK-01"]; !ok {
		t.Error("PF1-TSK-01 not registered after concurrent access")
	}
	if _, ok := cf.Phases["PF1"].Registered["PF1-TSK-02"]; !ok {
		t.Error("PF1-TSK-02 not registered after concurrent access")
	}
}

func TestCheckpoint_ConcurrentComplete(t *testing.T) {
	t.Parallel()

	dir := t.TempDir()
	configPath := writeMinimalConfig(t, dir)
	cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
	sentDir := filepath.Join(dir, "sentinels")

	cp := testCheckpoint(t)
	if err := cp.InitAllPhases(cpPath, configPath); err != nil {
		t.Fatalf("setup: %v", err)
	}

	// Concurrently complete both PF1 tasks.
	var wg sync.WaitGroup
	errs := make([]error, 2)

	wg.Add(2)
	go func() {
		defer wg.Done()
		errs[0] = cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01")
	}()
	go func() {
		defer wg.Done()
		errs[1] = cp.CompleteTask(cpPath, sentDir, "PF1-TSK-02")
	}()
	wg.Wait()

	for i, err := range errs {
		if err != nil {
			t.Errorf("concurrent CompleteTask[%d] error: %v", i, err)
		}
	}

	// Exactly one sentinel should exist.
	if !Exists(sentDir, "pf-1") {
		t.Error("pf-1 sentinel not created after concurrent completion")
	}

	cf, err := readCheckpointFile(cpPath)
	if err != nil {
		t.Fatalf("reading checkpoint: %v", err)
	}
	if !cf.Phases["PF1"].SentinelCreated {
		t.Error("SentinelCreated = false after concurrent completion")
	}
}

func TestCheckpointFile_MarshalUnmarshal(t *testing.T) {
	t.Parallel()

	t.Run("round-trips context and phases", func(t *testing.T) {
		t.Parallel()
		cf := &CheckpointFile{
			Context: map[string]string{"origin": "planned", "work_type": "FEAT"},
			Phases: map[string]*PhaseCheckpoint{
				"PF1": {
					Expected:   []string{"PF1-TSK-01"},
					Conditions: map[string]string{},
					Registered: map[string]string{"PF1-TSK-01": "2026-02-28T12:00:00Z"},
					Completed:  map[string]string{},
					Skipped:    map[string]string{},
				},
			},
		}

		data, err := json.Marshal(cf)
		if err != nil {
			t.Fatalf("Marshal() error: %v", err)
		}

		var cf2 CheckpointFile
		if err := json.Unmarshal(data, &cf2); err != nil {
			t.Fatalf("Unmarshal() error: %v", err)
		}

		if cf2.Context["origin"] != "planned" {
			t.Errorf("Context[origin] = %q, want %q", cf2.Context["origin"], "planned")
		}
		if cf2.Context["work_type"] != "FEAT" {
			t.Errorf("Context[work_type] = %q, want %q", cf2.Context["work_type"], "FEAT")
		}

		pc, ok := cf2.Phases["PF1"]
		if !ok {
			t.Fatal("PF1 not found in unmarshaled data")
		}
		if len(pc.Expected) != 1 || pc.Expected[0] != "PF1-TSK-01" {
			t.Errorf("PF1.Expected = %v, want [PF1-TSK-01]", pc.Expected)
		}
		if _, ok := pc.Registered["PF1-TSK-01"]; !ok {
			t.Error("PF1-TSK-01 not in registered after round-trip")
		}
	})

	t.Run("handles empty context", func(t *testing.T) {
		t.Parallel()
		cf := &CheckpointFile{
			Phases: map[string]*PhaseCheckpoint{
				"PF1": {
					Expected:   []string{"PF1-TSK-01"},
					Conditions: map[string]string{},
					Registered: map[string]string{},
					Completed:  map[string]string{},
					Skipped:    map[string]string{},
				},
			},
		}

		data, err := json.Marshal(cf)
		if err != nil {
			t.Fatalf("Marshal() error: %v", err)
		}

		var cf2 CheckpointFile
		if err := json.Unmarshal(data, &cf2); err != nil {
			t.Fatalf("Unmarshal() error: %v", err)
		}

		if len(cf2.Context) != 0 {
			t.Errorf("Context = %v, want empty", cf2.Context)
		}
	})
}

func TestExtractPhase(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		taskID  string
		want    string
		wantErr bool
	}{
		{"valid PF1-TSK-01", "PF1-TSK-01", "PF1", false},
		{"valid PF7-TSK-03", "PF7-TSK-03", "PF7", false},
		{"valid PF12-TSK-99", "PF12-TSK-99", "PF12", false},
		{"invalid no prefix", "TSK-01", "", true},
		{"invalid empty", "", "", true},
		{"invalid wrong format", "PF1-01", "", true},
		{"invalid extra suffix", "PF1-TSK-01-extra", "", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got, err := extractPhase(tt.taskID)
			if tt.wantErr {
				if err == nil {
					t.Errorf("extractPhase(%q) expected error", tt.taskID)
				}
				return
			}
			if err != nil {
				t.Errorf("extractPhase(%q) unexpected error: %v", tt.taskID, err)
				return
			}
			if got != tt.want {
				t.Errorf("extractPhase(%q) = %q, want %q", tt.taskID, got, tt.want)
			}
		})
	}
}

func TestPhaseSentinelName(t *testing.T) {
	t.Parallel()

	tests := []struct {
		phaseID string
		want    string
	}{
		{"PF1", "pf-1"},
		{"PF7", "pf-7"},
		{"PF12", "pf-12"},
	}

	for _, tt := range tests {
		t.Run(tt.phaseID, func(t *testing.T) {
			t.Parallel()
			if got := phaseSentinelName(tt.phaseID); got != tt.want {
				t.Errorf("phaseSentinelName(%q) = %q, want %q", tt.phaseID, got, tt.want)
			}
		})
	}
}

// phaseKey is a test helper that produces "PF{n}" strings.
func phaseKey(n int) string {
	return "PF" + strconv.Itoa(n)
}
