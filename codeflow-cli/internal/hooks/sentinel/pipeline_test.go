package sentinel

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

// writeTestConfig writes a minimal pathflow-config.json to configDir.
func writeTestConfig(t *testing.T, configDir string, pipelines map[string][]string) {
	t.Helper()
	config := map[string]any{
		"phases": map[string]any{
			"PF1-INIT":     map[string]any{"phase_order": 1},
			"PF2-CONTEXT":  map[string]any{"phase_order": 2},
			"PF3-CLASSIFY": map[string]any{"phase_order": 3},
			"PF4-EXECUTE":  map[string]any{"phase_order": 4},
			"PF5-VERIFY":   map[string]any{"phase_order": 5},
			"PF6-COMPLETE": map[string]any{"phase_order": 6},
			"PF7-END":      map[string]any{"phase_order": 7},
		},
		"pipelines": pipelines,
	}
	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestLoadPipelines(t *testing.T) {
	t.Parallel()

	t.Run("loads all pipelines from config", func(t *testing.T) {
		t.Parallel()
		configDir := filepath.Join(t.TempDir(), "config")
		pipelines := map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
			"DOCS": {"WS-DOCS", "WS-REV"},
			"PLAN": {"WS-PLAN", "WS-REV"},
		}
		writeTestConfig(t, configDir, pipelines)

		result, err := LoadPipelines(configDir)
		if err != nil {
			t.Fatalf("LoadPipelines() error: %v", err)
		}

		if len(result) != 3 {
			t.Errorf("got %d pipelines, want 3", len(result))
		}
		if got := result["FEAT"]; len(got) != 3 || got[0] != "WS-DEV" || got[1] != "WS-REV" || got[2] != "WS-QA" {
			t.Errorf("FEAT pipeline = %v, want [WS-DEV WS-REV WS-QA]", got)
		}
		if got := result["DOCS"]; len(got) != 2 || got[0] != "WS-DOCS" || got[1] != "WS-REV" {
			t.Errorf("DOCS pipeline = %v, want [WS-DOCS WS-REV]", got)
		}
	})

	t.Run("error on missing config file", func(t *testing.T) {
		t.Parallel()
		_, err := LoadPipelines(filepath.Join(t.TempDir(), "nonexistent"))
		if err == nil {
			t.Error("expected error for missing config file")
		}
	})

	t.Run("error on invalid JSON", func(t *testing.T) {
		t.Parallel()
		configDir := t.TempDir()
		if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), []byte("not json"), 0o644); err != nil {
			t.Fatal(err)
		}
		_, err := LoadPipelines(configDir)
		if err == nil {
			t.Error("expected error for invalid JSON")
		}
	})

	t.Run("error on missing pipelines section", func(t *testing.T) {
		t.Parallel()
		configDir := t.TempDir()
		data, _ := json.Marshal(map[string]any{"phases": map[string]any{}})
		if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
			t.Fatal(err)
		}
		_, err := LoadPipelines(configDir)
		if err == nil {
			t.Error("expected error for missing pipelines section")
		}
	})
}

func TestLoadPhaseOrder(t *testing.T) {
	t.Parallel()

	t.Run("returns phases in order", func(t *testing.T) {
		t.Parallel()
		configDir := filepath.Join(t.TempDir(), "config")
		writeTestConfig(t, configDir, map[string][]string{"FEAT": {"WS-DEV"}})

		phases, err := LoadPhaseOrder(configDir)
		if err != nil {
			t.Fatalf("LoadPhaseOrder() error: %v", err)
		}

		want := []string{"PF1", "PF2", "PF3", "PF4", "PF5", "PF6", "PF7"}
		if len(phases) != len(want) {
			t.Fatalf("got %d phases, want %d", len(phases), len(want))
		}
		for i, p := range phases {
			if p != want[i] {
				t.Errorf("phases[%d] = %q, want %q", i, p, want[i])
			}
		}
	})

	t.Run("error on missing config", func(t *testing.T) {
		t.Parallel()
		_, err := LoadPhaseOrder(filepath.Join(t.TempDir(), "nonexistent"))
		if err == nil {
			t.Error("expected error for missing config")
		}
	})

	t.Run("error on missing phases section", func(t *testing.T) {
		t.Parallel()
		configDir := t.TempDir()
		data, _ := json.Marshal(map[string]any{"pipelines": map[string]any{}})
		if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
			t.Fatal(err)
		}
		_, err := LoadPhaseOrder(configDir)
		if err == nil {
			t.Error("expected error for missing phases section")
		}
	})
}

func TestVerifyCumulativeStageSentinels(t *testing.T) {
	t.Parallel()

	pipeline := []string{"WS-DEV", "WS-REV", "WS-QA"}

	t.Run("all present pass", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		createTestSentinel(t, dir, "ws-dev")
		createTestSentinel(t, dir, "ws-rev")
		createTestSentinel(t, dir, "ws-qa")

		ok, missing := VerifyCumulativeStageSentinels(dir, pipeline, 2)
		if !ok {
			t.Errorf("expected all present, got missing: %s", missing)
		}
	})

	t.Run("first missing reports correct sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		// ws-dev missing, ws-rev present
		createTestSentinel(t, dir, "ws-rev")

		ok, missing := VerifyCumulativeStageSentinels(dir, pipeline, 1)
		if ok {
			t.Error("expected failure, got success")
		}
		if missing != "ws-dev" {
			t.Errorf("missing = %q, want %q", missing, "ws-dev")
		}
	})

	t.Run("middle missing reports correct sentinel", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		createTestSentinel(t, dir, "ws-dev")
		// ws-rev missing
		createTestSentinel(t, dir, "ws-qa")

		ok, missing := VerifyCumulativeStageSentinels(dir, pipeline, 2)
		if ok {
			t.Error("expected failure, got success")
		}
		if missing != "ws-rev" {
			t.Errorf("missing = %q, want %q", missing, "ws-rev")
		}
	})

	t.Run("check only up to index 0", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		createTestSentinel(t, dir, "ws-dev")

		ok, _ := VerifyCumulativeStageSentinels(dir, pipeline, 0)
		if !ok {
			t.Error("expected success for index 0 with ws-dev present")
		}
	})

	t.Run("empty pipeline passes", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		ok, _ := VerifyCumulativeStageSentinels(dir, []string{}, 0)
		if !ok {
			t.Error("expected success for empty pipeline")
		}
	})
}

func TestVerifyCumulativePhaseSentinels(t *testing.T) {
	t.Parallel()

	t.Run("all present pass", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		for i := 1; i <= 5; i++ {
			createTestSentinel(t, dir, fmt.Sprintf("pf-%d", i))
		}

		ok, missing := VerifyCumulativePhaseSentinels(dir, 5)
		if !ok {
			t.Errorf("expected all present, got missing: %s", missing)
		}
	})

	t.Run("gap in middle reports correct phase", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		createTestSentinel(t, dir, "pf-1")
		// pf-2 missing
		createTestSentinel(t, dir, "pf-3")
		createTestSentinel(t, dir, "pf-4")

		ok, missing := VerifyCumulativePhaseSentinels(dir, 4)
		if ok {
			t.Error("expected failure, got success")
		}
		if missing != "pf-2" {
			t.Errorf("missing = %q, want %q", missing, "pf-2")
		}
	})

	t.Run("first phase missing", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		for i := 2; i <= 5; i++ {
			createTestSentinel(t, dir, fmt.Sprintf("pf-%d", i))
		}

		ok, missing := VerifyCumulativePhaseSentinels(dir, 5)
		if ok {
			t.Error("expected failure, got success")
		}
		if missing != "pf-1" {
			t.Errorf("missing = %q, want %q", missing, "pf-1")
		}
	})

	t.Run("upToPhase 0 always passes", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()

		ok, _ := VerifyCumulativePhaseSentinels(dir, 0)
		if !ok {
			t.Error("expected success for upToPhase 0")
		}
	})
}

func TestInferWorkTypeFromBranch(t *testing.T) {
	t.Parallel()

	tests := []struct {
		branch string
		want   string
	}{
		{"feat/add-login", "FEAT"},
		{"fix/auth-bug", "FIX"},
		{"refactor/cleanup", "RFCT"},
		{"docs/readme", "DOCS"},
		{"test/coverage", "TEST"},
		{"chore/deps", "CHOR"},
		{"cicd/pipeline", "CICD"},
		{"plan/design", "PLAN"},
		{"spike/prototype", "SPKE"},
		{"hotfix/urgent", "HTFX"},
		{"main", ""},
		{"unknown/branch", ""},
		{"", ""},
	}

	for _, tt := range tests {
		t.Run(tt.branch, func(t *testing.T) {
			t.Parallel()
			got := InferWorkTypeFromBranch(tt.branch)
			if got != tt.want {
				t.Errorf("InferWorkTypeFromBranch(%q) = %q, want %q", tt.branch, got, tt.want)
			}
		})
	}
}

func TestStageSentinelKey(t *testing.T) {
	t.Parallel()

	tests := []struct {
		input string
		want  string
	}{
		{"WS-DEV", "ws-dev"},
		{"WS-REV", "ws-rev"},
		{"WS-QA", "ws-qa"},
		{"WS-DOCS", "ws-docs"},
		{"WS-PLAN", "ws-plan"},
		{"WS-TEST", "ws-test"},
	}

	for _, tt := range tests {
		t.Run(tt.input, func(t *testing.T) {
			t.Parallel()
			got := StageSentinelKey(tt.input)
			if got != tt.want {
				t.Errorf("StageSentinelKey(%q) = %q, want %q", tt.input, got, tt.want)
			}
		})
	}
}

func TestReadWorkTypeFromSessionStatus(t *testing.T) {
	t.Parallel()

	t.Run("reads work_type from status file", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		status := map[string]string{"work_type": "FEAT", "status": "pf-in-progress"}
		data, _ := json.Marshal(status)
		if err := os.WriteFile(filepath.Join(dir, "pathflow-session-status.json"), data, 0o644); err != nil {
			t.Fatal(err)
		}

		got := ReadWorkTypeFromSessionStatus(dir)
		if got != "FEAT" {
			t.Errorf("ReadWorkTypeFromSessionStatus() = %q, want %q", got, "FEAT")
		}
	})

	t.Run("returns empty for missing file", func(t *testing.T) {
		t.Parallel()
		got := ReadWorkTypeFromSessionStatus(t.TempDir())
		if got != "" {
			t.Errorf("ReadWorkTypeFromSessionStatus() = %q, want empty", got)
		}
	})

	t.Run("returns empty for invalid JSON", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		if err := os.WriteFile(filepath.Join(dir, "pathflow-session-status.json"), []byte("not json"), 0o644); err != nil {
			t.Fatal(err)
		}
		got := ReadWorkTypeFromSessionStatus(dir)
		if got != "" {
			t.Errorf("ReadWorkTypeFromSessionStatus() = %q, want empty", got)
		}
	})
}

func TestDeriveProjectDir(t *testing.T) {
	t.Parallel()

	sentinelDir := "/project/.state/sentinels/pathflow/ses-123"
	got := deriveProjectDir(sentinelDir)
	want := filepath.Clean("/project")
	if filepath.Clean(got) != want {
		t.Errorf("deriveProjectDir() = %q, want %q", got, want)
	}
}

func TestDeriveSessionDir(t *testing.T) {
	t.Parallel()

	sentinelDir := "/project/.state/sentinels/pathflow/ses-123"
	got := deriveSessionDir(sentinelDir)
	want := filepath.Clean("/project/.state/session/ses-123/pathflow")
	if filepath.Clean(got) != want {
		t.Errorf("deriveSessionDir() = %q, want %q", got, want)
	}
}

func TestDeriveConfigDir(t *testing.T) {
	t.Parallel()

	sentinelDir := "/project/.state/sentinels/pathflow/ses-123"
	got := deriveConfigDir(sentinelDir)
	want := filepath.Clean("/project/.codeflow/config/pathflow")
	if filepath.Clean(got) != want {
		t.Errorf("deriveConfigDir() = %q, want %q", got, want)
	}
}
