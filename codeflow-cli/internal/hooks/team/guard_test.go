package team

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// createPathflowActive creates the is-pathflow-active flag file in the given dir.
func createPathflowActive(t *testing.T, sessionDir string) {
	t.Helper()
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatalf("create session dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(sessionDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatalf("create pathflow-active flag: %v", err)
	}
}

// createSentinel creates a sentinel file in the given directory.
func createSentinel(t *testing.T, sentinelDir, name string) {
	t.Helper()
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatalf("create sentinel dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-"+name), []byte("1"), 0o644); err != nil {
		t.Fatalf("create sentinel %s: %v", name, err)
	}
}

func TestCheckTeamDelete(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name             string
		stdin            string
		pathflowActive   bool
		pf6Exists        bool
		wantAllow        bool
		wantReasonSubstr string
	}{
		// --- Non-target tools ---
		{
			name:      "Edit tool allowed (not TeamDelete/Teammate)",
			stdin:     `{"tool_name":"Edit","tool_input":{"file_path":"/tmp/x"}}`,
			wantAllow: true,
		},
		{
			name:      "Bash tool allowed (not TeamDelete/Teammate)",
			stdin:     `{"tool_name":"Bash","tool_input":{"command":"ls"}}`,
			wantAllow: true,
		},

		// --- Teammate non-cleanup ---
		{
			name:      "Teammate spawn operation allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{"operation":"spawn","name":"cf-dev"}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate message operation allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{"operation":"message","name":"cf-dev"}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate empty input allowed",
			stdin:     `{"tool_name":"Teammate","tool_input":{}}`,
			wantAllow: true,
		},
		{
			name:      "Teammate null input allowed",
			stdin:     `{"tool_name":"Teammate"}`,
			wantAllow: true,
		},

		// --- No PathFlow active ---
		{
			name:           "TeamDelete allowed when no PathFlow active",
			stdin:          `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive: false,
			wantAllow:      true,
		},
		{
			name:           "Teammate cleanup allowed when no PathFlow active",
			stdin:          `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive: false,
			wantAllow:      true,
		},

		// --- PathFlow active, no pf-6 sentinel ---
		{
			name:             "TeamDelete blocked when PathFlow active without pf-6",
			stdin:            `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive:   true,
			pf6Exists:        false,
			wantAllow:        false,
			wantReasonSubstr: "TeamDelete",
		},
		{
			name:             "Teammate cleanup blocked when PathFlow active",
			stdin:            `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive:   true,
			pf6Exists:        false,
			wantAllow:        false,
			wantReasonSubstr: "cleanup",
		},

		// --- PathFlow active, pf-6 sentinel exists (PF7-END gate) ---
		{
			name:           "TeamDelete allowed when pf-6 sentinel exists",
			stdin:          `{"tool_name":"TeamDelete","tool_input":{}}`,
			pathflowActive: true,
			pf6Exists:      true,
			wantAllow:      true,
		},
		{
			name:             "Teammate cleanup blocked even with pf-6 (only TeamDelete gets gate exception)",
			stdin:            `{"tool_name":"Teammate","tool_input":{"operation":"cleanup"}}`,
			pathflowActive:   true,
			pf6Exists:        true,
			wantAllow:        false,
			wantReasonSubstr: "cleanup",
		},

		// --- Edge cases ---
		{
			name:      "empty stdin allowed",
			stdin:     "",
			wantAllow: true,
		},
		{
			name:      "invalid JSON allowed (graceful degradation)",
			stdin:     "not json",
			wantAllow: true,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			tmpDir := t.TempDir()
			sessionDir := filepath.Join(tmpDir, "session", "pathflow")
			sentinelDir := filepath.Join(tmpDir, "sentinels")

			if tt.pathflowActive {
				createPathflowActive(t, sessionDir)
			}
			if tt.pf6Exists {
				createSentinel(t, sentinelDir, "pf-6")
			}

			verdict, _ := CheckTeamDelete(
				strings.NewReader(tt.stdin),
				sessionDir,
				sentinelDir,
			)

			if verdict.Allow != tt.wantAllow {
				t.Errorf("CheckTeamDelete() Allow = %v, want %v; reason: %s",
					verdict.Allow, tt.wantAllow, verdict.Reason)
			}

			if !tt.wantAllow && verdict.Reason == "" {
				t.Error("blocked verdict has empty Reason")
			}

			if tt.wantReasonSubstr != "" && !strings.Contains(verdict.Reason, tt.wantReasonSubstr) {
				t.Errorf("Reason = %q, want substring %q", verdict.Reason, tt.wantReasonSubstr)
			}
		})
	}
}

func TestHandlePostTeamDelete_Legacy(t *testing.T) {
	t.Parallel()

	t.Run("no error when session dir does not exist", func(t *testing.T) {
		t.Parallel()

		projectDir := t.TempDir()
		writeMinimalPathflowConfig(t, projectDir)

		err := HandlePostTeamDelete("/nonexistent/path/to/session/pathflow", projectDir, "ses-nonexistent")
		if err != nil {
			t.Errorf("HandlePostTeamDelete() error = %v, want nil (dir does not exist)", err)
		}
	})
}

// writeMinimalPathflowConfig writes a minimal pathflow-config.json for testing.
func writeMinimalPathflowConfig(t *testing.T, dir string) string {
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
		},
	}
	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		t.Fatalf("marshaling test config: %v", err)
	}
	cfgDir := filepath.Join(dir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(cfgDir, 0o755); err != nil {
		t.Fatalf("creating config dir: %v", err)
	}
	path := filepath.Join(cfgDir, "pathflow-config.json")
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatalf("writing test config: %v", err)
	}
	return path
}

func TestHandlePostTeamDelete_RemovesSentinels(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-test123"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create sentinel directory with files.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"pathflow-pf-1", "pathflow-pf-2", "pathflow-ws-dev"} {
		if err := os.WriteFile(filepath.Join(sentinelDir, name), []byte("1"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	// Write config for checkpoint reset.
	writeMinimalPathflowConfig(t, projectDir)

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Fatalf("HandlePostTeamDelete() error = %v", err)
	}

	// Verify sentinel directory was removed.
	if _, err := os.Stat(sentinelDir); !os.IsNotExist(err) {
		t.Error("sentinel directory should be removed after HandlePostTeamDelete")
	}
}

func TestHandlePostTeamDelete_RemovesTeamFile(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-test456"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-team.json.
	teamFile := filepath.Join(sessionDir, "pathflow-team.json")
	if err := os.WriteFile(teamFile, []byte(`{"team_name":"test"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Write config for checkpoint reset.
	writeMinimalPathflowConfig(t, projectDir)

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Fatalf("HandlePostTeamDelete() error = %v", err)
	}

	// Verify pathflow-team.json was removed.
	if _, err := os.Stat(teamFile); !os.IsNotExist(err) {
		t.Error("pathflow-team.json should be removed after HandlePostTeamDelete")
	}
}

func TestHandlePostTeamDelete_ResetsCheckpoint(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-test789"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Write config for checkpoint reset.
	writeMinimalPathflowConfig(t, projectDir)

	// Create a checkpoint with completed phases.
	checkpointPath := filepath.Join(sessionDir, "pathflow-phase-tasks.json")
	completedCheckpoint := map[string]any{
		"PF1": map[string]any{
			"expected":         []string{"PF1-TSK-01", "PF1-TSK-02"},
			"conditions":       map[string]string{},
			"registered":       map[string]string{"PF1-TSK-01": "2026-01-01T00:00:00Z", "PF1-TSK-02": "2026-01-01T00:00:00Z"},
			"completed":        map[string]string{"PF1-TSK-01": "2026-01-01T00:01:00Z", "PF1-TSK-02": "2026-01-01T00:01:00Z"},
			"skipped":          map[string]string{},
			"sentinel_created": true,
		},
	}
	data, _ := json.MarshalIndent(completedCheckpoint, "", "  ")
	if err := os.WriteFile(checkpointPath, data, 0o644); err != nil {
		t.Fatal(err)
	}

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Fatalf("HandlePostTeamDelete() error = %v", err)
	}

	// Verify checkpoint was reset (file should exist with fresh state).
	freshData, err := os.ReadFile(checkpointPath)
	if err != nil {
		t.Fatalf("checkpoint file should still exist after reset: %v", err)
	}

	var freshCheckpoint map[string]json.RawMessage
	if err := json.Unmarshal(freshData, &freshCheckpoint); err != nil {
		t.Fatalf("checkpoint file should be valid JSON: %v", err)
	}

	// PF1 should exist with empty completed/registered maps.
	pf1Data, ok := freshCheckpoint["PF1"]
	if !ok {
		t.Fatal("PF1 should exist in reset checkpoint")
	}
	var pf1 struct {
		Registered      map[string]string `json:"registered"`
		Completed       map[string]string `json:"completed"`
		SentinelCreated bool              `json:"sentinel_created"`
	}
	if err := json.Unmarshal(pf1Data, &pf1); err != nil {
		t.Fatalf("parsing PF1: %v", err)
	}
	if len(pf1.Registered) != 0 {
		t.Errorf("PF1.Registered should be empty after reset, got %v", pf1.Registered)
	}
	if len(pf1.Completed) != 0 {
		t.Errorf("PF1.Completed should be empty after reset, got %v", pf1.Completed)
	}
	if pf1.SentinelCreated {
		t.Error("PF1.SentinelCreated should be false after reset")
	}
}

func TestHandlePostTeamDelete_DoesNotRemoveActiveFlag(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-flag-test"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create is-pathflow-active flag.
	flagPath := filepath.Join(sessionDir, "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Write config for checkpoint reset.
	writeMinimalPathflowConfig(t, projectDir)

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Fatalf("HandlePostTeamDelete() error = %v", err)
	}

	// Verify is-pathflow-active was NOT removed.
	if _, err := os.Stat(flagPath); err != nil {
		t.Error("is-pathflow-active flag should survive HandlePostTeamDelete")
	}
}

func TestHandlePostTeamDelete_IdempotentWhenFilesAbsent(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-idempotent"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	// Intentionally do NOT create sessionDir or any files.

	// Write config for checkpoint reset.
	writeMinimalPathflowConfig(t, projectDir)

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Errorf("HandlePostTeamDelete() error = %v, want nil when no files exist", err)
	}
}

func TestHandlePostTeamDelete_NonFatalOnPartialFailure(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sessionID := "ses-partial"
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a team file that can be removed.
	teamFile := filepath.Join(sessionDir, "pathflow-team.json")
	if err := os.WriteFile(teamFile, []byte(`{"team_name":"test"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Do NOT write pathflow-config.json — checkpoint reset will fail,
	// but the other operations should still succeed.

	err := HandlePostTeamDelete(sessionDir, projectDir, sessionID)
	if err != nil {
		t.Errorf("HandlePostTeamDelete() error = %v, want nil (non-fatal)", err)
	}

	// Team file should still be removed despite checkpoint reset failure.
	if _, err := os.Stat(teamFile); !os.IsNotExist(err) {
		t.Error("pathflow-team.json should be removed even when checkpoint reset fails")
	}
}

func TestCheckTeamDelete_BlockedReasonFormat(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	sessionDir := filepath.Join(tmpDir, "session", "pathflow")
	sentinelDir := filepath.Join(tmpDir, "sentinels")
	createPathflowActive(t, sessionDir)

	verdict, _ := CheckTeamDelete(
		strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`),
		sessionDir,
		sentinelDir,
	)

	if verdict.Allow {
		t.Fatal("expected block")
	}

	// Verify the block message contains key information.
	expectedPhrases := []string{
		"BLOCKED",
		"PathFlow is active",
		"TeamDelete",
		"PF7-END",
	}
	for _, phrase := range expectedPhrases {
		if !strings.Contains(verdict.Reason, phrase) {
			t.Errorf("Reason missing phrase %q; got: %s", phrase, verdict.Reason)
		}
	}
}
