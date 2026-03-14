package session

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// newTestCleaner creates a Cleaner with test defaults.
func newTestCleaner(t *testing.T) *Cleaner {
	t.Helper()
	return &Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: t.TempDir(),
	}
}

// setupCleanupFixture creates the standard directory structure for cleanup tests.
// Returns projectDir.
func setupCleanupFixture(t *testing.T, sessionID string) string {
	t.Helper()
	projectDir := t.TempDir()

	// Create session state directory with pathflow subdirectory.
	sessionDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create runtime directory with env file (sole source of session ID).
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sessionID + "'\nexport CF_PROJECT_ROOT='testproject'\n"
	if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create ledger directory.
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	if err := os.MkdirAll(ledgerDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create sentinel directories.
	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(pfSentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	return projectDir
}

// writeSessionStatus creates a pathflow-session-status.json file for testing.
func writeSessionStatus(t *testing.T, projectDir, sessionID string, status *PathflowSessionStatus) {
	t.Helper()
	pathflowDir := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
	if err := os.MkdirAll(pathflowDir, 0o755); err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(status)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pathflowDir, PathflowSessionStatusFile), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestEndCleanup_NormalCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-3"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-7"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{"session_id":"test-uuid","transcript_path":"/tmp/transcript"}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	if !result.PF7Valid {
		t.Error("PF7Valid = false, want true")
	}
	if result.SentinelsCleaned < 1 {
		t.Errorf("SentinelsCleaned = %d, want >= 1", result.SentinelsCleaned)
	}
	if _, err := os.Stat(pfSentinelDir); !os.IsNotExist(err) {
		t.Error("pathflow sentinel dir still exists after cleanup")
	}
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists after cleanup")
	}
	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if _, err := os.Stat(envFile); !os.IsNotExist(err) {
		t.Error("env file still exists after cleanup")
	}
	ledgerPath := filepath.Join(projectDir, ".state", "ledger", "sessions.jsonl")
	data, err := os.ReadFile(ledgerPath)
	if err != nil {
		t.Fatalf("reading ledger file: %v", err)
	}
	if !strings.Contains(string(data), "session_end") {
		t.Error("ledger file does not contain session_end event")
	}
	if !strings.Contains(string(data), sessionID) {
		t.Error("ledger file does not contain session ID")
	}
	if len(result.Warnings) > 0 {
		t.Errorf("unexpected warnings: %v", result.Warnings)
	}
}

func TestEndCleanup_NoStatusFile(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists after cleanup (no status file should allow cleanup)")
	}
}

func TestEndCleanup_StatusPfComplete(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID: sessionID,
		Status:    "pf-complete",
		TeamName:  "test-team",
	})

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (pf-complete should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "pf-complete") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no pf-complete message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_StatusCreated(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID: sessionID,
		Status:    "created",
	})

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (created should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "'created'") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no 'created' status message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_ActiveSessionWithTeamConfig(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "test-team"

	cleaner := &Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: homeDir,
	}

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID:          sessionID,
		Status:             "pf-started",
		TeamName:           teamName,
		LastCompletedPhase: "pf-3",
	})

	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{"members": []any{}})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); os.IsNotExist(err) {
		t.Error("session state dir was removed (should skip cleanup for active session)")
	}
	foundSkipMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "skipping cleanup") {
			foundSkipMsg = true
			break
		}
	}
	if !foundSkipMsg {
		t.Errorf("no skip message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_PfInProgressWithTeamConfig(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "pf-started-team"

	cleaner := &Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: homeDir,
	}

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID:          sessionID,
		Status:             "pf-in-progress",
		TeamName:           teamName,
		LastCompletedPhase: "pf-4",
		LastCompletedStage: "ws-dev",
	})

	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{"members": []any{}})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); os.IsNotExist(err) {
		t.Error("session state dir was removed (should skip cleanup for pf-in-progress session)")
	}
	foundSkipMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "skipping cleanup") {
			foundSkipMsg = true
			break
		}
	}
	if !foundSkipMsg {
		t.Errorf("no skip message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_ActiveSessionPF7Completed(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "test-team"

	cleaner := &Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: homeDir,
	}

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID:          sessionID,
		Status:             "pf-started",
		TeamName:           teamName,
		LastCompletedPhase: "pf-7",
	})

	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{"members": []any{}})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (PF7 completed should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "PF7 already completed") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no PF7 completed message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_ActiveSessionNoTeamConfig(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "dissolved-team"

	cleaner := &Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: homeDir,
	}

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID:          sessionID,
		Status:             "pf-started",
		TeamName:           teamName,
		LastCompletedPhase: "pf-3",
	})

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (missing team config should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "config missing") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no config missing message found; got: %v", result.Messages)
	}
}

func TestEndCleanup_OldFlagNoStatusFile(t *testing.T) {
	t.Parallel()

	// Old is-pathflow-active flag exists but no status file.
	// With legacy fallback removed, shouldSkipCleanup returns false (allow cleanup).
	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// No status file means cleanup proceeds (old flag is ignored).
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (old flag without status file should trigger cleanup)")
	}
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
}

func TestEndCleanup_EmptyTeamNameInStatus(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID: sessionID,
		Status:    "pf-started",
		TeamName:  "",
	})

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (empty team name should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "no team name") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no 'no team name' message found; got: %v", result.Messages)
	}
}

func TestEndCleanup_MissingFiles(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	cleaner := newTestCleaner(t)

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.SessionID != "" {
		t.Errorf("SessionID = %q, want empty", result.SessionID)
	}
	foundWarning := false
	for _, w := range result.Warnings {
		if strings.Contains(w, "no session ID") {
			foundWarning = true
			break
		}
	}
	if !foundWarning {
		t.Error("no 'no session ID' warning found")
	}
}

func TestEndCleanup_EmptyProjectDir(t *testing.T) {
	t.Parallel()

	cleaner := newTestCleaner(t)
	stdin := strings.NewReader(`{}`)

	_, err := cleaner.EndCleanup(stdin, "")
	if err == nil {
		t.Fatal("EndCleanup() expected error for empty project dir")
	}
	if !strings.Contains(err.Error(), "empty project directory") {
		t.Errorf("error = %q, want to contain 'empty project directory'", err.Error())
	}
}

func TestEndCleanup_NilStdin(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	result, err := cleaner.EndCleanup(nil, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
}

func TestEndCleanup_IncompletePF7(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-3"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-ws-dev"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.PF7Valid {
		t.Error("PF7Valid = true, want false (pf-7 sentinel missing)")
	}
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	foundPF7Msg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "Incomplete PF7") {
			foundPF7Msg = true
			break
		}
	}
	if !foundPF7Msg {
		t.Error("no incomplete PF7 message found")
	}
}

func TestEndCleanup_ActiveTaskPreserved(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	taskPath := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	taskData, _ := json.Marshal(map[string]string{
		"task_id": "INF-TSK-021-011",
		"status":  "in_progress",
	})
	if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if !result.TaskPreserved {
		t.Error("TaskPreserved = false, want true")
	}
	if _, err := os.Stat(taskPath); os.IsNotExist(err) {
		t.Error("active-task.json was removed (should be preserved for in_progress tasks)")
	}
}

func TestEndCleanup_CompletedTaskRemoved(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	taskPath := filepath.Join(projectDir, ".state", "runtime", "active-task.json")
	taskData, _ := json.Marshal(map[string]string{
		"task_id": "INF-TSK-021-011",
		"status":  "completed",
	})
	if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.TaskPreserved {
		t.Error("TaskPreserved = true, want false (completed task should be removed)")
	}
	if _, err := os.Stat(taskPath); !os.IsNotExist(err) {
		t.Error("active-task.json still exists (completed task should be removed)")
	}
}

func TestEndCleanup_TeamArtifactCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	teamName := "codeflow-test-team"

	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(map[string]any{
		"team_name": teamName,
		"lead_pid":  0,
	})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	teamDir := filepath.Join(cleaner.HomeDir, ".claude", "teams", teamName)
	taskDir := filepath.Join(cleaner.HomeDir, ".claude", "tasks", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(taskDir, 0o755); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.TeamName != teamName {
		t.Errorf("TeamName = %q, want %q", result.TeamName, teamName)
	}
	if _, err := os.Stat(teamDir); !os.IsNotExist(err) {
		t.Error("team config dir still exists after cleanup")
	}
	if _, err := os.Stat(taskDir); !os.IsNotExist(err) {
		t.Error("task list dir still exists after cleanup")
	}
}

func TestEndCleanup_LedgerEventContent(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-7"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	_, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	ledgerPath := filepath.Join(projectDir, ".state", "ledger", "sessions.jsonl")
	data, err := os.ReadFile(ledgerPath)
	if err != nil {
		t.Fatalf("reading ledger: %v", err)
	}

	lines := strings.Split(strings.TrimSpace(string(data)), "\n")
	if len(lines) == 0 {
		t.Fatal("no events in ledger")
	}

	var event map[string]any
	if err := json.Unmarshal([]byte(lines[len(lines)-1]), &event); err != nil {
		t.Fatalf("parsing ledger event: %v", err)
	}

	if event["event"] != "session_end" {
		t.Errorf("event type = %v, want session_end", event["event"])
	}
	if event["session_id"] != sessionID {
		t.Errorf("session_id = %v, want %s", event["session_id"], sessionID)
	}
	if event["pf7_valid"] != true {
		t.Errorf("pf7_valid = %v, want true", event["pf7_valid"])
	}
	if event["cleanup_completed"] != true {
		t.Errorf("cleanup_completed = %v, want true", event["cleanup_completed"])
	}
}

func TestEndCleanup_SessionIDFromEnvVar(t *testing.T) {
	// NOTE: no t.Parallel() — t.Setenv is incompatible with parallel tests.
	sessionID := "ses-1234567890123abcdef012345"
	projectDir := t.TempDir()
	cleaner := newTestCleaner(t)

	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "ledger"), 0o755); err != nil {
		t.Fatal(err)
	}

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
}

func TestEndCleanup_EnvFileCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if _, err := os.Stat(envFile); err != nil {
		t.Fatalf("codeflow-env.sh should exist before cleanup: %v", err)
	}

	stdin := strings.NewReader(`{}`)
	_, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if _, err := os.Stat(envFile); !os.IsNotExist(err) {
		t.Error("codeflow-env.sh should have been removed after cleanup")
	}
}

func TestValidatePF7_SentinelExists(t *testing.T) {
	t.Parallel()

	sentinelDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-7"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	valid, warnings := ValidatePF7(sentinelDir)
	if !valid {
		t.Error("ValidatePF7() = false, want true")
	}
	if len(warnings) > 0 {
		t.Errorf("unexpected warnings: %v", warnings)
	}
}

func TestValidatePF7_SentinelMissing(t *testing.T) {
	t.Parallel()

	sentinelDir := t.TempDir()
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-3"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-ws-dev"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	valid, warnings := ValidatePF7(sentinelDir)
	if valid {
		t.Error("ValidatePF7() = true, want false")
	}
	if len(warnings) == 0 {
		t.Error("expected warnings for missing pf-7 sentinel")
	}
	foundIncomplete := false
	for _, w := range warnings {
		if strings.Contains(w, "Incomplete PF7") {
			foundIncomplete = true
			break
		}
	}
	if !foundIncomplete {
		t.Error("no 'Incomplete PF7' warning found")
	}
	foundExisting := false
	for _, w := range warnings {
		if strings.Contains(w, "existing sentinels") {
			foundExisting = true
			break
		}
	}
	if !foundExisting {
		t.Error("no existing sentinels diagnostic found")
	}
}

func TestValidatePF7_EmptyDir(t *testing.T) {
	t.Parallel()

	sentinelDir := t.TempDir()
	valid, warnings := ValidatePF7(sentinelDir)
	if valid {
		t.Error("ValidatePF7() = true, want false")
	}
	if len(warnings) == 0 {
		t.Error("expected warnings for missing pf-7")
	}
}

func TestValidatePF7_NonexistentDir(t *testing.T) {
	t.Parallel()

	valid, warnings := ValidatePF7("/nonexistent/path/to/sentinels")
	if valid {
		t.Error("ValidatePF7() = true, want false")
	}
	if len(warnings) < 2 {
		t.Errorf("expected at least 2 warnings, got %d", len(warnings))
	}
}

func TestParseEndStdin(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name           string
		input          string
		wantSessionID  string
		wantTranscript string
	}{
		{
			name:           "full input",
			input:          `{"session_id":"test-uuid","transcript_path":"/tmp/tx"}`,
			wantSessionID:  "test-uuid",
			wantTranscript: "/tmp/tx",
		},
		{name: "empty input", input: ""},
		{name: "invalid JSON", input: "not-json"},
		{
			name:          "partial input",
			input:         `{"session_id":"partial-uuid"}`,
			wantSessionID: "partial-uuid",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			var r io.Reader
			if tt.input != "" {
				r = strings.NewReader(tt.input)
			}
			result := parseEndStdin(r)
			if result.SessionID != tt.wantSessionID {
				t.Errorf("SessionID = %q, want %q", result.SessionID, tt.wantSessionID)
			}
			if result.TranscriptPath != tt.wantTranscript {
				t.Errorf("TranscriptPath = %q, want %q", result.TranscriptPath, tt.wantTranscript)
			}
		})
	}
}

func TestNewCleaner(t *testing.T) {
	t.Parallel()

	cleaner := NewCleaner()
	if cleaner.Now == nil {
		t.Error("Now is nil")
	}
	if cleaner.HomeDir == "" {
		t.Error("HomeDir is empty")
	}
}

func TestEndCleanup_IdempotentNoStatusFile(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	if len(result.Warnings) > 0 {
		t.Errorf("unexpected warnings: %v", result.Warnings)
	}
}

func TestEndCleanup_UnknownStatusAllowsCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	writeSessionStatus(t, projectDir, sessionID, &PathflowSessionStatus{
		SessionID: sessionID,
		Status:    "some-unknown-status",
	})

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (unknown status should trigger cleanup)")
	}
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "Unknown session status") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no unknown status message found; got: %v", result.Messages)
	}
}
