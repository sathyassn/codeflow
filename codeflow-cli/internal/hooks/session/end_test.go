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
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{}},
		HomeDir:        t.TempDir(),
		PPID:           99999,
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

func TestEndCleanup_NormalCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Create pathflow sentinels.
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

	// Verify session ID resolved.
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}

	// Verify PF7 was valid (sentinel existed before cleanup).
	if !result.PF7Valid {
		t.Error("PF7Valid = false, want true")
	}

	// Verify sentinels were cleaned.
	if result.SentinelsCleaned < 1 {
		t.Errorf("SentinelsCleaned = %d, want >= 1", result.SentinelsCleaned)
	}

	// Verify pathflow sentinel directory was removed.
	if _, err := os.Stat(pfSentinelDir); !os.IsNotExist(err) {
		t.Error("pathflow sentinel dir still exists after cleanup")
	}

	// Verify session state directory was removed.
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists after cleanup")
	}

	// Verify runtime files were removed.
	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if _, err := os.Stat(envFile); !os.IsNotExist(err) {
		t.Error("env file still exists after cleanup")
	}

	// Verify session_end event was written to ledger.
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

	// Verify no warnings.
	if len(result.Warnings) > 0 {
		t.Errorf("unexpected warnings: %v", result.Warnings)
	}
}

func TestEndCleanup_StalePathflowFlag(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Create pathflow-active flag (simulates TeamDelete PostToolUse not running).
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	flag := pathflowFlag{
		SessionID:     sessionID,
		CreatedAt:     fixedTime.Format("2006-01-02T15:04:05.000Z"),
		TrackingLevel: "tracked",
	}
	flagData, _ := json.Marshal(flag)
	if err := os.WriteFile(flagPath, flagData, 0o644); err != nil {
		t.Fatal(err)
	}

	// No pathflow-team.json -- simulates pre-TeamCreate scenario.

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should proceed with cleanup (no team file = no lead to protect).
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}

	// Verify session state was cleaned up (including the stale flag).
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists after cleanup (stale flag should be removed)")
	}
}

func TestEndCleanup_TeammateShutdownSkip(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "test-team"

	// Create cleaner with a live tmux pane for the team.
	cleaner := &Cleaner{
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{"%100": true}},
		HomeDir:        homeDir,
		PPID:           99999,
	}

	// Create team config with a live pane.
	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{
		"members": []map[string]string{
			{"tmuxPaneId": "%100"},
		},
	})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-active flag.
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-team.json with team name.
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 12345, TeamName: teamName})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should skip cleanup (teammate shutdown -- live tmux panes detected).
	// Session state should still exist.
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); os.IsNotExist(err) {
		t.Error("session state dir was removed during teammate shutdown (should be skipped)")
	}

	// Env file should still exist.
	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if _, err := os.Stat(envFile); os.IsNotExist(err) {
		t.Error("env file was removed during teammate shutdown (should be skipped)")
	}

	// Verify the skip message was logged.
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

func TestEndCleanup_NoLivePanesCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "test-team"

	// Create cleaner with NO live tmux panes.
	cleaner := &Cleaner{
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{"%200": false}},
		HomeDir:        homeDir,
		PPID:           55555,
	}

	// Create team config with a dead pane.
	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{
		"members": []map[string]string{
			{"tmuxPaneId": "%200"},
		},
	})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-active flag.
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-team.json with team name.
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 55555, TeamName: teamName})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should proceed with cleanup (no live panes = session ended).
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (no live panes should trigger cleanup)")
	}

	// Verify the cleanup message.
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "no live tmux panes") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no 'no live tmux panes' message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_NoTeamConfigCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()

	// No team config file at all -- hasLiveTeamPanes returns false.
	cleaner := &Cleaner{
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{}},
		HomeDir:        homeDir,
		PPID:           99999,
	}

	// Create pathflow-active flag and team file with a team name but no config.
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 88888, TeamName: "orphan-team"})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should proceed with cleanup (no team config = no live panes).
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); !os.IsNotExist(err) {
		t.Error("session state dir still exists (missing team config should trigger cleanup)")
	}

	// Verify cleanup message.
	foundMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "no live tmux panes") {
			foundMsg = true
			break
		}
	}
	if !foundMsg {
		t.Errorf("no 'no live tmux panes' message found in Messages; got: %v", result.Messages)
	}
}

func TestEndCleanup_MissingFiles(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	cleaner := newTestCleaner(t)

	// No runtime dir, no env file, no session state -- should gracefully handle.
	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should warn about missing session ID and return early.
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

	// nil stdin should not crash.
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

	// Create some sentinels but NOT pf-7.
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

	// PF7 should be invalid.
	if result.PF7Valid {
		t.Error("PF7Valid = true, want false (pf-7 sentinel missing)")
	}

	// Should still complete cleanup successfully.
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}

	// Verify warning message about incomplete PF7.
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

	// Create active task with in_progress status.
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

	// Active task file should still exist.
	if _, err := os.Stat(taskPath); os.IsNotExist(err) {
		t.Error("active-task.json was removed (should be preserved for in_progress tasks)")
	}
}

func TestEndCleanup_CompletedTaskRemoved(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Create active task with completed status.
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

	// Active task file should be removed.
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

	// Create pathflow-team.json with team name.
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(map[string]any{
		"team_name": teamName,
		"lead_pid":  0,
	})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create team config and task list directories.
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

	// Team directories should be removed.
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

	// Create pf-7 sentinel for a clean session.
	pfSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
	if err := os.WriteFile(filepath.Join(pfSentinelDir, "pathflow-pf-7"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	_, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Read and parse the ledger event.
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
	// Cannot use t.Parallel() with t.Setenv.

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := t.TempDir()
	cleaner := newTestCleaner(t)

	// No env file -- only CODEFLOW_SESSION_ID env var is set.
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create ledger dir.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "ledger"), 0o755); err != nil {
		t.Fatal(err)
	}

	t.Setenv("CODEFLOW_SESSION_ID", sessionID)

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should resolve session ID from CODEFLOW_SESSION_ID env var.
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
}

func TestEndCleanup_EnvFileCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Verify the env file exists before cleanup.
	envFile := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	if _, err := os.Stat(envFile); err != nil {
		t.Fatalf("codeflow-env.sh should exist before cleanup: %v", err)
	}

	stdin := strings.NewReader(`{}`)
	_, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// codeflow-env.sh should be cleaned up.
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
	// Create some other sentinels but not pf-7.
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

	// Should mention incomplete PF7.
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

	// Should list existing sentinels.
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
		{
			name:  "empty input",
			input: "",
		},
		{
			name:  "invalid JSON",
			input: "not-json",
		},
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
	if cleaner.ProcessChecker == nil {
		t.Error("ProcessChecker is nil")
	}
	if cleaner.TmuxChecker == nil {
		t.Error("TmuxChecker is nil")
	}
	if cleaner.HomeDir == "" {
		t.Error("HomeDir is empty")
	}
	if cleaner.PPID <= 0 {
		t.Errorf("PPID = %d, want > 0", cleaner.PPID)
	}
}

func TestEndCleanup_IdempotentFlagRemoval(t *testing.T) {
	t.Parallel()

	// Test that cleanup works even when pathflow-active flag is already removed
	// (TeamDelete PostToolUse hook may have already removed it).
	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Do NOT create pathflow-active flag (already removed by TeamDelete hook).
	// Session state dir exists but no flag.

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should complete without errors.
	if result.SessionID != sessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sessionID)
	}
	if len(result.Warnings) > 0 {
		t.Errorf("unexpected warnings: %v", result.Warnings)
	}
}

func TestEndCleanup_EmptyTeamNameCleanup(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	cleaner := newTestCleaner(t)

	// Create pathflow-active flag.
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-team.json with EMPTY team name.
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 12345, TeamName: ""})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should proceed with cleanup (no team name = can't check panes).
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

func TestEndCleanup_MultiplePanesPartiallyAlive(t *testing.T) {
	t.Parallel()

	sessionID := "ses-1234567890123abcdef012345"
	projectDir := setupCleanupFixture(t, sessionID)
	homeDir := t.TempDir()
	teamName := "multi-pane-team"

	// One pane alive, one dead -- should skip cleanup (any alive = session active).
	cleaner := &Cleaner{
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{"%300": false, "%301": true}},
		HomeDir:        homeDir,
		PPID:           99999,
	}

	// Create team config with mixed panes.
	teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	cfgData, _ := json.Marshal(map[string]any{
		"members": []map[string]string{
			{"tmuxPaneId": "%300"},
			{"tmuxPaneId": "%301"},
		},
	})
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-active flag and team file.
	flagPath := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "is-pathflow-active")
	if err := os.WriteFile(flagPath, []byte(`{"session_id":"`+sessionID+`"}`), 0o644); err != nil {
		t.Fatal(err)
	}
	teamFile := filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-team.json")
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 12345, TeamName: teamName})
	if err := os.WriteFile(teamFile, teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	stdin := strings.NewReader(`{}`)
	result, err := cleaner.EndCleanup(stdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}

	// Should skip cleanup (at least one pane is alive).
	sessionStateDir := filepath.Join(projectDir, ".state", "session", sessionID)
	if _, err := os.Stat(sessionStateDir); os.IsNotExist(err) {
		t.Error("session state dir was removed (should skip cleanup when any pane is alive)")
	}

	foundSkipMsg := false
	for _, m := range result.Messages {
		if strings.Contains(m, "skipping cleanup") {
			foundSkipMsg = true
			break
		}
	}
	if !foundSkipMsg {
		t.Errorf("no skip message found; got: %v", result.Messages)
	}
}

func TestHasLiveTeamPanes_Cleaner(t *testing.T) {
	t.Parallel()

	t.Run("config_missing", func(t *testing.T) {
		t.Parallel()
		cleaner := newTestCleaner(t)
		// No team config at all.
		if cleaner.hasLiveTeamPanes("nonexistent-team") {
			t.Error("hasLiveTeamPanes should return false when config is missing")
		}
	})

	t.Run("config_invalid_json", func(t *testing.T) {
		t.Parallel()
		cleaner := newTestCleaner(t)
		teamDir := filepath.Join(cleaner.HomeDir, ".claude", "teams", "bad-json")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), []byte("not-json"), 0o644); err != nil {
			t.Fatal(err)
		}
		if cleaner.hasLiveTeamPanes("bad-json") {
			t.Error("hasLiveTeamPanes should return false for invalid JSON")
		}
	})

	t.Run("empty_members", func(t *testing.T) {
		t.Parallel()
		cleaner := newTestCleaner(t)
		teamDir := filepath.Join(cleaner.HomeDir, ".claude", "teams", "empty-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{"members": []any{}})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}
		if cleaner.hasLiveTeamPanes("empty-team") {
			t.Error("hasLiveTeamPanes should return false for empty members")
		}
	})

	t.Run("all_panes_dead", func(t *testing.T) {
		t.Parallel()
		cleaner := newTestCleaner(t)
		cleaner.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%dead1": false, "%dead2": false}}

		teamDir := filepath.Join(cleaner.HomeDir, ".claude", "teams", "dead-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%dead1"},
				{"tmuxPaneId": "%dead2"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}
		if cleaner.hasLiveTeamPanes("dead-team") {
			t.Error("hasLiveTeamPanes should return false when all panes are dead")
		}
	})

	t.Run("one_pane_alive", func(t *testing.T) {
		t.Parallel()
		cleaner := newTestCleaner(t)
		cleaner.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%alive": true, "%dead": false}}

		teamDir := filepath.Join(cleaner.HomeDir, ".claude", "teams", "partial-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%alive"},
				{"tmuxPaneId": "%dead"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}
		if !cleaner.hasLiveTeamPanes("partial-team") {
			t.Error("hasLiveTeamPanes should return true when any pane is alive")
		}
	})
}
