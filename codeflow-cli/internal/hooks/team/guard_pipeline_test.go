package team

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// setupGuardProjectEnv creates a full project directory structure for
// pipeline-aware TeamDelete guard tests. Returns sessionDir, sentinelDir, projectDir.
func setupGuardProjectEnv(t *testing.T) (sessionDir, sentinelDir, projectDir string) {
	t.Helper()
	projectDir = t.TempDir()
	sid := "ses-guardtest"
	sessionDir = filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	sentinelDir = filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	for _, d := range []string{sessionDir, sentinelDir, configDir} {
		if err := os.MkdirAll(d, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	return sessionDir, sentinelDir, projectDir
}

// writeGuardTestConfig writes a pathflow-config.json with pipelines.
func writeGuardTestConfig(t *testing.T, projectDir string, pipelines map[string][]string) {
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
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

// writeGuardSessionStatus writes pathflow-session-status.json with work_type.
func writeGuardSessionStatus(t *testing.T, sessionDir, workType string) {
	t.Helper()
	status := &session.PathflowSessionStatus{
		SessionID: "ses-guardtest",
		Status:    "pf-in-progress",
		TeamName:  "test-team",
		WorkType:  workType,
	}
	data, err := json.Marshal(status)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sessionDir, session.PathflowSessionStatusFile), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestCheckTeamDelete_PipelineStageSentinels(t *testing.T) {
	t.Parallel()

	t.Run("TeamDelete blocked when ws-qa missing for FEAT pipeline", func(t *testing.T) {
		t.Parallel()
		sessionDir, sentinelDir, projectDir := setupGuardProjectEnv(t)

		writeGuardTestConfig(t, projectDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		writeGuardSessionStatus(t, sessionDir, "FEAT")

		// pf-6 exists (PF7-END gate passes)
		createSentinel(t, sentinelDir, "pf-6")
		// ws-dev and ws-rev exist but ws-qa missing
		createSentinel(t, sentinelDir, "ws-dev")
		createSentinel(t, sentinelDir, "ws-rev")

		verdict, err := CheckTeamDelete(
			strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
			sessionDir,
			sentinelDir,
		)
		if err != nil {
			t.Fatalf("CheckTeamDelete() error: %v", err)
		}

		if verdict.Allow {
			t.Fatal("expected block: ws-qa missing for FEAT pipeline")
		}
		if !strings.Contains(verdict.Reason, "ws-qa") {
			t.Errorf("Reason should mention ws-qa, got: %s", verdict.Reason)
		}
	})

	t.Run("TeamDelete allowed when all pipeline sentinels exist", func(t *testing.T) {
		t.Parallel()
		sessionDir, sentinelDir, projectDir := setupGuardProjectEnv(t)

		writeGuardTestConfig(t, projectDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		writeGuardSessionStatus(t, sessionDir, "FEAT")

		// pf-6 + all pipeline stages
		createSentinel(t, sentinelDir, "pf-6")
		createSentinel(t, sentinelDir, "ws-dev")
		createSentinel(t, sentinelDir, "ws-rev")
		createSentinel(t, sentinelDir, "ws-qa")

		verdict, err := CheckTeamDelete(
			strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
			sessionDir,
			sentinelDir,
		)
		if err != nil {
			t.Fatalf("CheckTeamDelete() error: %v", err)
		}

		if !verdict.Allow {
			t.Fatalf("expected allow: all pipeline sentinels exist; reason: %s", verdict.Reason)
		}
	})

	t.Run("TeamDelete allowed for DOCS pipeline with ws-docs and ws-rev", func(t *testing.T) {
		t.Parallel()
		sessionDir, sentinelDir, projectDir := setupGuardProjectEnv(t)

		writeGuardTestConfig(t, projectDir, map[string][]string{
			"DOCS": {"WS-DOCS", "WS-REV"},
		})
		writeGuardSessionStatus(t, sessionDir, "DOCS")

		createSentinel(t, sentinelDir, "pf-6")
		createSentinel(t, sentinelDir, "ws-docs")
		createSentinel(t, sentinelDir, "ws-rev")

		verdict, err := CheckTeamDelete(
			strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
			sessionDir,
			sentinelDir,
		)
		if err != nil {
			t.Fatalf("CheckTeamDelete() error: %v", err)
		}

		if !verdict.Allow {
			t.Fatalf("expected allow for DOCS pipeline; reason: %s", verdict.Reason)
		}
	})

	t.Run("TeamDelete allowed when no work_type (graceful degradation)", func(t *testing.T) {
		t.Parallel()
		sessionDir, sentinelDir, projectDir := setupGuardProjectEnv(t)

		writeGuardTestConfig(t, projectDir, map[string][]string{
			"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
		})
		// Write status without work_type
		status := &session.PathflowSessionStatus{
			SessionID: "ses-guardtest",
			Status:    "pf-in-progress",
			TeamName:  "test-team",
		}
		data, _ := json.Marshal(status)
		if err := os.WriteFile(filepath.Join(sessionDir, session.PathflowSessionStatusFile), data, 0o644); err != nil {
			t.Fatal(err)
		}

		createSentinel(t, sentinelDir, "pf-6")

		verdict, err := CheckTeamDelete(
			strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
			sessionDir,
			sentinelDir,
		)
		if err != nil {
			t.Fatalf("CheckTeamDelete() error: %v", err)
		}

		if !verdict.Allow {
			t.Fatalf("expected allow when no work_type (graceful degradation); reason: %s", verdict.Reason)
		}
	})
}
