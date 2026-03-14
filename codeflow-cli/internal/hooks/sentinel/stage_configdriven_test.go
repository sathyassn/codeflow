package sentinel

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// setupStageProjectEnv creates a full project directory structure for config-driven
// stage ordering tests. sentinelDir is at {project}/.state/sentinels/pathflow/{SID}/
// so that deriveSessionDir and deriveConfigDir can resolve paths correctly.
func setupStageProjectEnv(t *testing.T) (sentinelDir string, sessionDir string, configDir string) {
	t.Helper()
	projectDir := t.TempDir()
	sid := "ses-stagetest"
	sentinelDir = filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
	sessionDir = filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	configDir = filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	for _, d := range []string{sentinelDir, sessionDir, configDir} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	return sentinelDir, sessionDir, configDir
}

// writeStageTestConfig writes a pathflow-config.json with phases and pipelines.
func writeStageTestConfig(t *testing.T, configDir string, pipelines map[string][]string) {
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
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

// writeStageSessionStatus writes a pathflow-session-status.json with the given work_type.
func writeStageSessionStatus(t *testing.T, sessionDir, workType string) {
	t.Helper()
	status := &session.PathflowSessionStatus{
		SessionID: "ses-stagetest",
		Status:    "pf-in-progress",
		WorkType:  workType,
	}
	if err := session.WritePathflowSessionStatus(sessionDir, status); err != nil {
		t.Fatal(err)
	}
}

func TestConfigDrivenStageOrdering(t *testing.T) {
	t.Parallel()

	t.Run("ws-rev blocked without ws-dev for FEAT pipeline", func(t *testing.T) {
		t.Parallel()
		sentinelDir, sessionDir, configDir := setupStageProjectEnv(t)

		writeStageTestConfig(t, configDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		writeStageSessionStatus(t, sessionDir, "FEAT")

		stdin := `{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV"}}`
		verdict := CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)

		if verdict.Allow {
			t.Fatal("expected block: ws-rev without ws-dev for FEAT pipeline")
		}
		if !strings.Contains(verdict.Reason, "ws-dev") {
			t.Errorf("Reason should mention ws-dev, got: %s", verdict.Reason)
		}
	})

	t.Run("ws-qa blocked without ws-dev AND ws-rev cumulative", func(t *testing.T) {
		t.Parallel()
		sentinelDir, sessionDir, configDir := setupStageProjectEnv(t)

		writeStageTestConfig(t, configDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		writeStageSessionStatus(t, sessionDir, "FEAT")

		// Only ws-dev, missing ws-rev
		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-ws-dev"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}

		stdin := `{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA"}}`
		verdict := CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)

		if verdict.Allow {
			t.Fatal("expected block: ws-qa without ws-rev (cumulative)")
		}
		if !strings.Contains(verdict.Reason, "ws-rev") {
			t.Errorf("Reason should mention ws-rev, got: %s", verdict.Reason)
		}
	})

	t.Run("ws-rev allowed with ws-docs for DOCS pipeline", func(t *testing.T) {
		t.Parallel()
		sentinelDir, sessionDir, configDir := setupStageProjectEnv(t)

		writeStageTestConfig(t, configDir, map[string][]string{
			"DOCS": {"WS-DOCS", "WS-REV"},
		})
		writeStageSessionStatus(t, sessionDir, "DOCS")

		if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-ws-docs"), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}

		stdin := `{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV"}}`
		verdict := CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)

		if !verdict.Allow {
			t.Fatalf("expected allow: ws-rev with ws-docs for DOCS pipeline; reason: %s", verdict.Reason)
		}

		if _, err := os.Stat(filepath.Join(sentinelDir, "pathflow-ws-rev")); os.IsNotExist(err) {
			t.Error("ws-rev sentinel not created for DOCS pipeline")
		}
	})

	t.Run("ws-qa allowed with all prior stages for FEAT pipeline", func(t *testing.T) {
		t.Parallel()
		sentinelDir, sessionDir, configDir := setupStageProjectEnv(t)

		writeStageTestConfig(t, configDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		writeStageSessionStatus(t, sessionDir, "FEAT")

		for _, s := range []string{"ws-dev", "ws-rev"} {
			if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-"+s), []byte("1"), 0o644); err != nil {
				t.Fatal(err)
			}
		}

		stdin := `{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA"}}`
		verdict := CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)

		if !verdict.Allow {
			t.Fatalf("expected allow: ws-qa with all prior stages; reason: %s", verdict.Reason)
		}

		if _, err := os.Stat(filepath.Join(sentinelDir, "pathflow-ws-qa")); os.IsNotExist(err) {
			t.Error("ws-qa sentinel not created")
		}
	})

	t.Run("falls back to hardcoded rules when no work_type", func(t *testing.T) {
		t.Parallel()
		sentinelDir, sessionDir, configDir := setupStageProjectEnv(t)

		writeStageTestConfig(t, configDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		// Write status WITHOUT work_type
		status := &session.PathflowSessionStatus{
			SessionID: "ses-stagetest",
			Status:    "pf-in-progress",
		}
		if err := session.WritePathflowSessionStatus(sessionDir, status); err != nil {
			t.Fatal(err)
		}

		stdin := `{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV"}}`
		verdict := CheckAndCreateStageSentinel(strings.NewReader(stdin), sentinelDir)

		if verdict.Allow {
			t.Fatal("expected block: ws-rev without primary stage (fallback rules)")
		}
		if !strings.Contains(verdict.Reason, "ws-rev requires prior primary stage") {
			t.Errorf("Reason should mention fallback rule, got: %s", verdict.Reason)
		}
	})
}
