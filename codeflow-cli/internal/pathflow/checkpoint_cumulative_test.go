package pathflow

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestCheckpoint_CumulativePhaseRegistration(t *testing.T) {
	t.Parallel()

	t.Run("PF5 registration blocked when pf-1 missing even if pf-2 pf-3 pf-4 exist", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Create pf-2, pf-3, pf-4 but NOT pf-1
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, n := range []string{"pf-2", "pf-3", "pf-4"} {
			if err := Create(sentDir, n); err != nil {
				t.Fatal(err)
			}
		}

		err := cp.RegisterTask(cpPath, sentDir, "PF5-TSK-01")
		if err == nil {
			t.Fatal("RegisterTask(PF5) expected error: pf-1 missing in cumulative check")
		}
		if !errors.Is(err, ErrCrossPhaseBlock) {
			t.Errorf("error = %v, want ErrCrossPhaseBlock", err)
		}
	})

	t.Run("PF4 registration blocked when pf-2 missing even if pf-1 and pf-3 exist", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Create pf-1 and pf-3 but NOT pf-2
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, n := range []string{"pf-1", "pf-3"} {
			if err := Create(sentDir, n); err != nil {
				t.Fatal(err)
			}
		}

		err := cp.RegisterTask(cpPath, sentDir, "PF4-TSK-01")
		if err == nil {
			t.Fatal("RegisterTask(PF4) expected error: pf-2 missing in cumulative check")
		}
		if !errors.Is(err, ErrCrossPhaseBlock) {
			t.Errorf("error = %v, want ErrCrossPhaseBlock", err)
		}
	})

	t.Run("PF4 registration succeeds with all prior sentinels pf-1 pf-2 pf-3", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")
		sentDir := filepath.Join(dir, "sentinels")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		// Create all prior sentinels
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, n := range []string{"pf-1", "pf-2", "pf-3"} {
			if err := Create(sentDir, n); err != nil {
				t.Fatal(err)
			}
		}

		err := cp.RegisterTask(cpPath, sentDir, "PF4-TSK-01")
		if err != nil {
			t.Fatalf("RegisterTask(PF4) with all prior sentinels: %v", err)
		}
	})
}

func TestCheckpoint_SetContext(t *testing.T) {
	t.Parallel()

	t.Run("sets work_type in checkpoint context", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.SetContext(cpPath, "work_type", "FEAT"); err != nil {
			t.Fatalf("SetContext() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("read checkpoint: %v", err)
		}

		if cf.Context["work_type"] != "FEAT" {
			t.Errorf("work_type = %q, want %q", cf.Context["work_type"], "FEAT")
		}
	})

	t.Run("sets origin in checkpoint context", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		configPath := writeMinimalConfig(t, dir)
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		cp := testCheckpoint(t)
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("setup: %v", err)
		}

		if err := cp.SetContext(cpPath, "origin", "planned"); err != nil {
			t.Fatalf("SetContext() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("read checkpoint: %v", err)
		}

		if cf.Context["origin"] != "planned" {
			t.Errorf("origin = %q, want %q", cf.Context["origin"], "planned")
		}
	})

	t.Run("creates context when it does not exist", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		cpPath := filepath.Join(dir, "pathflow-phase-tasks.json")

		// Write minimal checkpoint without context
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(cpPath, []byte(`{"PF1":{"expected":["PF1-TSK-01"],"conditions":{},"registered":{},"completed":{},"skipped":{},"sentinel_created":false}}`), 0o644); err != nil {
			t.Fatal(err)
		}

		cp := testCheckpoint(t)
		if err := cp.SetContext(cpPath, "work_type", "FIX"); err != nil {
			t.Fatalf("SetContext() error: %v", err)
		}

		cf, err := readCheckpointFile(cpPath)
		if err != nil {
			t.Fatalf("read checkpoint: %v", err)
		}

		if cf.Context == nil {
			t.Fatal("context should be created")
		}
		if cf.Context["work_type"] != "FIX" {
			t.Errorf("work_type = %q, want %q", cf.Context["work_type"], "FIX")
		}
	})
}
