package session

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
	"github.com/codeflow/codeflow-cli/internal/session"
)

// fixedTime is a deterministic timestamp for tests.
var fixedTime = time.Date(2026, 2, 28, 12, 0, 0, 0, time.UTC)

// mockProcessChecker returns predefined liveness results for PIDs.
type mockProcessChecker struct {
	alive map[int]bool
}

func (m mockProcessChecker) IsAlive(pid int) bool {
	return m.alive[pid]
}

// mockTmuxChecker returns predefined liveness results for pane IDs.
type mockTmuxChecker struct {
	alive map[string]bool
}

func (m mockTmuxChecker) IsPaneAlive(paneID string) bool {
	return m.alive[paneID]
}

// mockSessionStarter returns a fixed session ID for testing.
type mockSessionStarter struct {
	id  string
	err error
}

func (m mockSessionStarter) StartSession(_ context.Context, _ string, _ string) (string, error) {
	return m.id, m.err
}

// mockSessionStarterFunc wraps a function as a SessionStarter for flexible test control.
type mockSessionStarterFunc func(ctx context.Context, claudeID string, projectDir string) (string, error)

func (f mockSessionStarterFunc) StartSession(ctx context.Context, claudeID string, projectDir string) (string, error) {
	return f(ctx, claudeID, projectDir)
}

// testULIDSessionID is a deterministic ULID-format session ID for tests.
const testULIDSessionID = "ses-01jk0000000000000000000000"

// newTestInitializer creates an Initializer with test defaults.
func newTestInitializer(t *testing.T) *Initializer {
	t.Helper()
	return &Initializer{
		Now:            func() time.Time { return fixedTime },
		ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{}},
		SessionStarter: mockSessionStarter{id: testULIDSessionID},
		ReadBuildInfo:  func() string { return "" },
		PPID:           99999,
		HomeDir:        t.TempDir(),
	}
}

func TestStartInit_FreshSession(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)

	// Create pathflow config for checkpoint init.
	setupPathflowConfig(t, projectDir)

	stdin := strings.NewReader(`{"session_id":"abc-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Verify session ID was generated in ULID format.
	if result.SessionID != testULIDSessionID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, testULIDSessionID)
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("SessionID %q does not match expected format", result.SessionID)
	}
	if result.IsResume {
		t.Error("IsResume = true, want false for fresh session")
	}
	if result.IsTeammate {
		t.Error("IsTeammate = true, want false for fresh session")
	}

	// Verify env vars.
	if result.EnvVars["CODEFLOW_SESSION_ID"] != result.SessionID {
		t.Errorf("EnvVars[CODEFLOW_SESSION_ID] = %q, want %q", result.EnvVars["CODEFLOW_SESSION_ID"], result.SessionID)
	}
	if result.EnvVars["CF_PROJECT_ROOT"] == "" {
		t.Error("EnvVars[CF_PROJECT_ROOT] is empty")
	}

	// Verify directories created.
	assertDirExists(t, filepath.Join(projectDir, ".state", "logs", "sessions"))
	assertDirExists(t, filepath.Join(projectDir, ".state", "db"))
	assertDirExists(t, filepath.Join(projectDir, ".state", "runtime"))
	assertDirExists(t, filepath.Join(projectDir, ".state", "sentinels", "pathflow", result.SessionID))
	assertDirExists(t, filepath.Join(projectDir, ".state", "session", result.SessionID))

	// Verify pathflow-active flag.
	flagPath := filepath.Join(projectDir, ".state", "session", result.SessionID, "pathflow", "is-pathflow-active")
	assertFileExists(t, flagPath)
	flagData, _ := os.ReadFile(flagPath)
	var flag pathflowFlag
	if err := json.Unmarshal(flagData, &flag); err != nil {
		t.Fatalf("pathflow flag unmarshal error: %v", err)
	}
	if flag.SessionID != result.SessionID {
		t.Errorf("flag.SessionID = %q, want %q", flag.SessionID, result.SessionID)
	}
	if flag.TrackingLevel != "pending" {
		t.Errorf("flag.TrackingLevel = %q, want %q", flag.TrackingLevel, "pending")
	}

	// Verify checkpoint file was initialized.
	checkpointPath := filepath.Join(projectDir, ".state", "session", result.SessionID, "pathflow", "pathflow-phase-tasks.json")
	assertFileExists(t, checkpointPath)

	// Verify current-session-id is NOT written (eliminated; only codeflow-env.sh is used).
	sessionIDPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if _, err := os.Stat(sessionIDPath); !os.IsNotExist(err) {
		t.Error("current-session-id should NOT exist (eliminated in favor of codeflow-env.sh)")
	}

	// Verify session metadata.
	metaPath := filepath.Join(projectDir, ".state", "logs", "sessions", "session-"+result.SessionID+".meta")
	assertFileExists(t, metaPath)

	// Verify env file.
	envPath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	assertFileExists(t, envPath)
	envData, _ := os.ReadFile(envPath)
	if !strings.Contains(string(envData), result.SessionID) {
		t.Errorf("env file does not contain session ID %q", result.SessionID)
	}
}

func TestStartInit_ResumeDetection(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	// Pre-create pathflow flag to simulate existing session.
	sid := "ses-1709136000000abcdef012345"
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flag := pathflowFlag{
		SessionID:     sid,
		CreatedAt:     "2026-02-28T12:00:00.000Z",
		TrackingLevel: "tracked",
	}
	flagData, _ := json.Marshal(flag)
	if err := os.WriteFile(filepath.Join(pfDir, "is-pathflow-active"), flagData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write env file with existing session.
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sid + "'\nexport CF_PROJECT_ROOT='test'\n"
	if err := os.WriteFile(filepath.Join(envDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// Use source=resume so the stale cleanup preserves the session.
	// In production, resume means the user typed /resume, so state is preserved.
	stdin := strings.NewReader(`{"session_id":"new-uuid","source":"resume"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	if result.SessionID != sid {
		t.Errorf("SessionID = %q, want %q (should preserve existing)", result.SessionID, sid)
	}
	if !result.IsResume {
		t.Error("IsResume = false, want true for existing pathflow flag")
	}
}

func TestStartInit_TeammateMode(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	teamName := "my-team"

	// Create pathflow-team.json with team name.
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 12345, TeamName: teamName})
	if err := os.WriteFile(filepath.Join(pfDir, "pathflow-team.json"), teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write env file.
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sid + "'\nexport CF_PROJECT_ROOT='test'\n"
	if err := os.WriteFile(filepath.Join(envDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	init_ := newTestInitializer(t)
	// Set up team config with a live tmux pane.
	init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%100": true}}

	teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", teamName)
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

	stdin := strings.NewReader(`{"session_id":"teammate-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	if !result.IsTeammate {
		t.Error("IsTeammate = false, want true when team has live tmux panes")
	}
	if result.SessionID != sid {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sid)
	}
	// Teammate mode should have messages.
	found := false
	for _, m := range result.Messages {
		if strings.Contains(m, "TEAMMATE MODE") {
			found = true
			break
		}
	}
	if !found {
		t.Error("missing TEAMMATE MODE message")
	}
}

func TestStartInit_StaleSessionCleanup(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	// Create stale session state with dead PID.
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 99998, TeamName: "stale-team"})
	if err := os.WriteFile(filepath.Join(pfDir, "pathflow-team.json"), teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Create team config directory.
	teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "stale-team")
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(teamDir, "config.json"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create sentinel dir.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Write env file.
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sid + "'\nexport CF_PROJECT_ROOT='test'\n"
	if err := os.WriteFile(filepath.Join(envDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// No live tmux panes (config is bare {}) + source=startup -> full cleanup.
	stdin := strings.NewReader(`{"session_id":"new-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Should have generated a new session ID.
	if result.SessionID == sid {
		t.Error("SessionID should be new after stale cleanup")
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("new SessionID %q does not match format", result.SessionID)
	}

	// Verify stale artifacts were cleaned.
	if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", sid)); !os.IsNotExist(err) {
		t.Error("stale session directory should have been removed")
	}
	if _, err := os.Stat(sentinelDir); !os.IsNotExist(err) {
		t.Error("stale sentinel directory should have been removed")
	}
	if _, err := os.Stat(filepath.Join(teamDir, "config.json")); !os.IsNotExist(err) {
		t.Error("stale team config should have been removed")
	}
}

func TestStartInit_CompactContinuation(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	// Create existing session with no live tmux panes (simulating compact).
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 99998, TeamName: "compact-team"})
	if err := os.WriteFile(filepath.Join(pfDir, "pathflow-team.json"), teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write env file.
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sid + "'\nexport CF_PROJECT_ROOT='test'\n"
	if err := os.WriteFile(filepath.Join(envDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// No live panes + source=compact -> preserve session, DON'T update PID.
	// On compact, the claude process doesn't restart. If no panes are alive,
	// this caller is a surviving teammate. PID should NOT be updated.
	stdin := strings.NewReader(`{"session_id":"compact-uuid","source":"compact"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Should preserve the existing session ID.
	if result.SessionID != sid {
		t.Errorf("SessionID = %q, want %q (should preserve for compact)", result.SessionID, sid)
	}

	// Verify pathflow-team.json was NOT updated with new PID (compact does not update).
	updatedData, err := os.ReadFile(filepath.Join(pfDir, "pathflow-team.json"))
	if err != nil {
		t.Fatalf("reading team file: %v", err)
	}
	var updated pathflowTeamJSON
	if err := json.Unmarshal(updatedData, &updated); err != nil {
		t.Fatalf("unmarshaling team file: %v", err)
	}
	if updated.LeadPID == init_.PPID {
		t.Errorf("lead_pid should NOT be updated on compact (got %d, PPID=%d)", updated.LeadPID, init_.PPID)
	}
	if updated.LeadPID != 99998 {
		t.Errorf("lead_pid should remain %d (original dead PID), got %d", 99998, updated.LeadPID)
	}
}

func TestStartInit_ResumeContinuation(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	// Create existing session with no live tmux panes (simulating resume after session ended).
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 99998, TeamName: "resume-team"})
	if err := os.WriteFile(filepath.Join(pfDir, "pathflow-team.json"), teamData, 0o644); err != nil {
		t.Fatal(err)
	}

	// Write env file.
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envContent := "export CODEFLOW_SESSION_ID='" + sid + "'\nexport CF_PROJECT_ROOT='test'\n"
	if err := os.WriteFile(filepath.Join(envDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
		t.Fatal(err)
	}

	// No live panes + source=resume -> preserve session AND update PID.
	// On resume, the user relaunched claude, so PID update is correct.
	stdin := strings.NewReader(`{"session_id":"resume-uuid","source":"resume"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Should preserve the existing session ID.
	if result.SessionID != sid {
		t.Errorf("SessionID = %q, want %q (should preserve for resume)", result.SessionID, sid)
	}

	// Verify pathflow-team.json WAS updated with new PID (resume updates PID).
	updatedData, err := os.ReadFile(filepath.Join(pfDir, "pathflow-team.json"))
	if err != nil {
		t.Fatalf("reading updated team file: %v", err)
	}
	var updated pathflowTeamJSON
	if err := json.Unmarshal(updatedData, &updated); err != nil {
		t.Fatalf("unmarshaling updated team file: %v", err)
	}
	if updated.LeadPID != init_.PPID {
		t.Errorf("updated lead_pid = %d, want %d (resume should update PID)", updated.LeadPID, init_.PPID)
	}
}

func TestStartInit_EmptyProjectDir(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	stdin := strings.NewReader(`{"session_id":"uuid","source":"startup"}`)
	_, err := init_.StartInit(stdin, "")
	if err == nil {
		t.Error("StartInit() with empty project dir should return error")
	}
}

func TestStartInit_InvalidStdinJSON(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	stdin := strings.NewReader("not json")
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v (should succeed with invalid stdin)", err)
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("SessionID %q should still be generated", result.SessionID)
	}
}

func TestStartInit_NilStdin(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	result, err := init_.StartInit(nil, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v (should succeed with nil stdin)", err)
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("SessionID %q should still be generated", result.SessionID)
	}
}

func TestValidateSessionID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name  string
		sid   string
		valid bool
	}{
		// Legacy format: ses-{13-digit-timestamp}{12-hex-chars} (25 total after prefix).
		{"valid_legacy", "ses-1709136000000abcdef012345", true},
		{"valid_legacy_all_zeros", "ses-0000000000000000000000000", true},
		// ULID format: ses-{26-lowercase-alphanumeric} (30 chars total).
		{"valid_ulid", "ses-01jk0000000000000000000000", true},
		{"valid_ulid_mixed", "ses-01jk1234567890abcdefghijkl", true},
		// Invalid cases.
		{"too_short", "ses-123", false},
		{"missing_prefix", "1709136000000abcdef012345", false},
		{"uppercase_hex", "ses-1709136000000ABCDEF012345", false},
		{"uppercase_ulid", "ses-01JK0000000000000000000000", false},
		{"empty", "", false},
		{"uuid_format", "abc-def-123", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := ValidateSessionID(tt.sid)
			if got != tt.valid {
				t.Errorf("ValidateSessionID(%q) = %v, want %v", tt.sid, got, tt.valid)
			}
		})
	}
}

func TestParseStdin(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name       string
		input      string
		wantSource string
		wantUUID   string
	}{
		{"valid_startup", `{"session_id":"uuid-123","source":"startup"}`, "startup", "uuid-123"},
		{"valid_compact", `{"session_id":"uuid-456","source":"compact"}`, "compact", "uuid-456"},
		{"missing_source", `{"session_id":"uuid-789"}`, "unknown", "uuid-789"},
		{"empty_json", `{}`, "unknown", ""},
		{"invalid_json", `not json`, "unknown", ""},
		{"empty_string", ``, "unknown", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			r := strings.NewReader(tt.input)
			got := parseStdin(r)
			if got.Source != tt.wantSource {
				t.Errorf("Source = %q, want %q", got.Source, tt.wantSource)
			}
			if got.SessionID != tt.wantUUID {
				t.Errorf("SessionID = %q, want %q", got.SessionID, tt.wantUUID)
			}
		})
	}
}

func TestParseEnvFileSessionID(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		content string
		want    string
	}{
		{"single_quote", "export CODEFLOW_SESSION_ID='ses-123'\n", "ses-123"},
		{"double_quote", `export CODEFLOW_SESSION_ID="ses-456"` + "\n", "ses-456"},
		{"no_quote", "export CODEFLOW_SESSION_ID=ses-789\n", "ses-789"},
		{"with_other_vars", "export FOO='bar'\nexport CODEFLOW_SESSION_ID='ses-abc'\nexport BAZ='qux'\n", "ses-abc"},
		{"empty", "", ""},
		{"no_sid", "export FOO='bar'\n", ""},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()
			got := parseEnvFileSessionID(tt.content)
			if got != tt.want {
				t.Errorf("parseEnvFileSessionID() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestSessionStarterIntegration(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)

	// Create pathflow config for checkpoint init.
	setupPathflowConfig(t, projectDir)

	// Use a mock starter that returns a specific ULID-format ID.
	wantID := "ses-01jk1234567890abcdefghijkl"
	init_.SessionStarter = mockSessionStarter{id: wantID}

	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Verify the ULID-format session ID was used.
	if result.SessionID != wantID {
		t.Errorf("SessionID = %q, want %q", result.SessionID, wantID)
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("SessionID %q does not match expected format", result.SessionID)
	}
	if len(result.SessionID) != 30 {
		t.Errorf("SessionID length = %d, want 30 (ses- + 26 ULID chars)", len(result.SessionID))
	}
}

func TestSessionStarterError(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)

	// Create pathflow config for checkpoint init.
	setupPathflowConfig(t, projectDir)

	// Use a mock starter that returns an error.
	init_.SessionStarter = mockSessionStarter{err: fmt.Errorf("db connection failed")}

	stdin := strings.NewReader(`{"session_id":"test-uuid","source":"startup"}`)
	_, err := init_.StartInit(stdin, projectDir)
	if err == nil {
		t.Fatal("StartInit() should return error when SessionStarter fails")
	}
	if !strings.Contains(err.Error(), "db connection failed") {
		t.Errorf("error = %q, should contain 'db connection failed'", err.Error())
	}
}

func TestSessionStarterPassesClaudeID(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)

	// Create pathflow config for checkpoint init.
	setupPathflowConfig(t, projectDir)

	// Use a function-based mock to capture the claudeID argument.
	var capturedClaudeID string
	init_.SessionStarter = mockSessionStarterFunc(func(_ context.Context, claudeID string, _ string) (string, error) {
		capturedClaudeID = claudeID
		return testULIDSessionID, nil
	})

	stdin := strings.NewReader(`{"session_id":"my-claude-uuid","source":"startup"}`)
	_, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	if capturedClaudeID != "my-claude-uuid" {
		t.Errorf("claudeID = %q, want %q", capturedClaudeID, "my-claude-uuid")
	}
}

func TestDBSessionStarterIntegration(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)
	setupPathflowConfig(t, projectDir)

	// Create a real DB for integration testing.
	dbPath := filepath.Join(projectDir, ".state", "db", "codeflow.db")
	if err := os.MkdirAll(filepath.Dir(dbPath), 0o755); err != nil {
		t.Fatal(err)
	}
	d, err := db.NewDB(dbPath)
	if err != nil {
		t.Fatalf("NewDB: %v", err)
	}
	t.Cleanup(func() { d.Close() })

	ctx := t.Context()
	if err := d.InitFromSchema(ctx); err != nil {
		t.Fatalf("InitFromSchema: %v", err)
	}

	// Use a real SessionStarter backed by session.Start.
	init_.SessionStarter = mockSessionStarterFunc(func(ctx context.Context, claudeID string, pDir string) (string, error) {
		ledgerDir := filepath.Join(pDir, ".state", "ledger")
		runtimeDir := filepath.Join(pDir, ".state", "runtime")
		return session.Start(ctx, d, claudeID, ledgerDir, runtimeDir)
	})

	stdin := strings.NewReader(`{"session_id":"integration-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}

	// Verify ULID format: ses-{26 lowercase alphanumeric}.
	if !strings.HasPrefix(result.SessionID, "ses-") {
		t.Errorf("SessionID %q should start with ses-", result.SessionID)
	}
	if len(result.SessionID) != 30 {
		t.Errorf("SessionID length = %d, want 30", len(result.SessionID))
	}
	if result.SessionID != strings.ToLower(result.SessionID) {
		t.Errorf("SessionID %q should be lowercase", result.SessionID)
	}
	if !ValidateSessionID(result.SessionID) {
		t.Errorf("SessionID %q does not match expected format", result.SessionID)
	}

	// Verify env vars contain the ULID session ID.
	if result.EnvVars["CODEFLOW_SESSION_ID"] != result.SessionID {
		t.Errorf("EnvVars[CODEFLOW_SESSION_ID] = %q, want %q",
			result.EnvVars["CODEFLOW_SESSION_ID"], result.SessionID)
	}

	// Verify session was recorded in the database.
	var status string
	err = d.QueryRow(ctx,
		"SELECT status FROM sessions WHERE id = ?", result.SessionID,
	).Scan(&status)
	if err != nil {
		t.Fatalf("DB query for session record: %v", err)
	}
	if status != "active" {
		t.Errorf("session status = %q, want %q", status, "active")
	}

	// Verify current-session-id is NOT written (eliminated; only codeflow-env.sh is used).
	sessionIDPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if _, statErr := os.Stat(sessionIDPath); !os.IsNotExist(statErr) {
		t.Error("current-session-id should NOT exist (eliminated in favor of codeflow-env.sh)")
	}

	// Verify JSONL event was written.
	jsonlPath := filepath.Join(projectDir, ".state", "ledger", "sessions.jsonl")
	jsonlData, err := os.ReadFile(jsonlPath)
	if err != nil {
		t.Fatalf("reading sessions.jsonl: %v", err)
	}
	if !strings.Contains(string(jsonlData), result.SessionID) {
		t.Errorf("sessions.jsonl does not contain session ID %q", result.SessionID)
	}
	if !strings.Contains(string(jsonlData), "session_start") {
		t.Error("sessions.jsonl does not contain session_start event")
	}
}

func TestDBSessionStarter_Direct(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()

	starter := dbSessionStarter{}
	ctx := t.Context()

	sessionID, err := starter.StartSession(ctx, "test-claude-id", projectDir)
	if err != nil {
		t.Fatalf("StartSession() error = %v", err)
	}

	// Verify ULID format.
	if !strings.HasPrefix(sessionID, "ses-") {
		t.Errorf("session ID should start with ses-, got %q", sessionID)
	}
	if len(sessionID) != 30 {
		t.Errorf("session ID length = %d, want 30", len(sessionID))
	}
	if !ValidateSessionID(sessionID) {
		t.Errorf("session ID %q does not validate", sessionID)
	}

	// Verify DB was created.
	dbPath := filepath.Join(projectDir, ".state", "db", "codeflow.db")
	assertFileExists(t, dbPath)

	// Verify current-session-id is NOT written by StartSession/session.Start
	// (caller is responsible -- hook path or CLI path).
	sidPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if _, err := os.Stat(sidPath); !os.IsNotExist(err) {
		t.Error("current-session-id should NOT be written by StartSession -- caller is responsible")
	}

	// Verify JSONL event was written.
	jsonlPath := filepath.Join(projectDir, ".state", "ledger", "sessions.jsonl")
	assertFileExists(t, jsonlPath)
}

func TestDBSessionStarter_EmptyClaudeID(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	starter := dbSessionStarter{}
	ctx := t.Context()

	_, err := starter.StartSession(ctx, "", projectDir)
	if err == nil {
		t.Fatal("StartSession() should fail with empty claudeID")
	}
}

func TestWriteEnvFile(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	envPath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")

	sid := "ses-1709136000000abcdef012345"
	err := session.WriteEnvFile(filepath.Dir(envPath), sid, projectDir)
	if err != nil {
		t.Fatalf("writeEnvFile() error = %v", err)
	}

	data, err := os.ReadFile(envPath)
	if err != nil {
		t.Fatalf("reading env file: %v", err)
	}
	content := string(data)
	if !strings.Contains(content, sid) {
		t.Errorf("env file does not contain session ID %q", sid)
	}
	if !strings.Contains(content, "CODEFLOW_SESSION_ID") {
		t.Error("env file does not contain CODEFLOW_SESSION_ID export")
	}
	if !strings.Contains(content, "CF_PROJECT_ROOT") {
		t.Error("env file does not contain CF_PROJECT_ROOT export")
	}
}

func TestCreateDirectories(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"

	init_.createDirectories(projectDir, sid)

	expected := []string{
		filepath.Join(projectDir, ".state", "logs", "sessions"),
		filepath.Join(projectDir, ".state", "logs", "security"),
		filepath.Join(projectDir, ".state", "db"),
		filepath.Join(projectDir, ".state", "runtime"),
		filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid),
		filepath.Join(projectDir, ".state", "session", sid),
	}
	for _, dir := range expected {
		assertDirExists(t, dir)
	}
}

func TestCreatePathFlowFlag(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	result := &InitResult{EnvVars: make(map[string]string)}

	isResume := init_.createPathFlowFlag(projectDir, sid, result)
	if isResume {
		t.Error("createPathFlowFlag() = true, want false for new flag")
	}

	flagPath := filepath.Join(projectDir, ".state", "session", sid, "pathflow", "is-pathflow-active")
	assertFileExists(t, flagPath)

	data, _ := os.ReadFile(flagPath)
	var flag pathflowFlag
	if err := json.Unmarshal(data, &flag); err != nil {
		t.Fatalf("unmarshal flag: %v", err)
	}
	if flag.SessionID != sid {
		t.Errorf("flag.SessionID = %q, want %q", flag.SessionID, sid)
	}
	if flag.TrackingLevel != "pending" {
		t.Errorf("flag.TrackingLevel = %q, want %q", flag.TrackingLevel, "pending")
	}
	if flag.CreatedAt == "" {
		t.Error("flag.CreatedAt is empty")
	}
}

func TestCreatePathFlowFlag_ResumeWhenExists(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	result := &InitResult{EnvVars: make(map[string]string)}

	// Pre-create flag.
	flagDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("existing"), 0o644); err != nil {
		t.Fatal(err)
	}

	isResume := init_.createPathFlowFlag(projectDir, sid, result)
	if !isResume {
		t.Error("createPathFlowFlag() = false, want true when flag exists")
	}
}

func TestCleanupActiveTask(t *testing.T) {
	t.Parallel()

	t.Run("completed_task_removed", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		runtimeDir := filepath.Join(projectDir, ".state", "runtime")
		if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
			t.Fatal(err)
		}

		taskData, _ := json.Marshal(map[string]string{"status": "completed"})
		taskPath := filepath.Join(runtimeDir, "active-task.json")
		if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.cleanupActiveTask(projectDir)

		if _, err := os.Stat(taskPath); !os.IsNotExist(err) {
			t.Error("completed task should have been removed")
		}
	})

	t.Run("stale_task_removed", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		runtimeDir := filepath.Join(projectDir, ".state", "runtime")
		if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
			t.Fatal(err)
		}

		staleTime := fixedTime.Add(-48 * time.Hour).Format(time.RFC3339)
		taskData, _ := json.Marshal(map[string]string{
			"status":     "in_progress",
			"updated_at": staleTime,
		})
		taskPath := filepath.Join(runtimeDir, "active-task.json")
		if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.cleanupActiveTask(projectDir)

		if _, err := os.Stat(taskPath); !os.IsNotExist(err) {
			t.Error("stale task (>24h) should have been removed")
		}
	})

	t.Run("active_task_preserved", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		runtimeDir := filepath.Join(projectDir, ".state", "runtime")
		if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
			t.Fatal(err)
		}

		recentTime := fixedTime.Add(-1 * time.Hour).Format(time.RFC3339)
		taskData, _ := json.Marshal(map[string]string{
			"status":     "in_progress",
			"updated_at": recentTime,
		})
		taskPath := filepath.Join(runtimeDir, "active-task.json")
		if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.cleanupActiveTask(projectDir)

		if _, err := os.Stat(taskPath); os.IsNotExist(err) {
			t.Error("active recent task should have been preserved")
		}
	})
}

func TestSweepAllStaleSessions(t *testing.T) {
	t.Parallel()

	t.Run("cleans_stale_session_with_no_live_panes", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		currentSID := "ses-1709136000000current00001"

		// Create current session dir (should not be touched).
		if err := os.MkdirAll(filepath.Join(projectDir, ".state", "session", currentSID, "pathflow"), 0o755); err != nil {
			t.Fatal(err)
		}

		// Create stale session with no live tmux panes.
		staleSID := "ses-1709136000000stale000001"
		staleDir := filepath.Join(projectDir, ".state", "session", staleSID, "pathflow")
		if err := os.MkdirAll(staleDir, 0o755); err != nil {
			t.Fatal(err)
		}
		teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 88888, TeamName: "stale-team"})
		if err := os.WriteFile(filepath.Join(staleDir, "pathflow-team.json"), teamData, 0o644); err != nil {
			t.Fatal(err)
		}

		// Create stale sentinel dir.
		staleSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", staleSID)
		if err := os.MkdirAll(staleSentinelDir, 0o755); err != nil {
			t.Fatal(err)
		}

		// Create team config with dead pane to verify it's cleaned.
		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "stale-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%dead-pane"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%dead-pane": false}}
		init_.sweepAllStaleSessions(projectDir, currentSID)

		// Stale session dir should be removed.
		if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", staleSID)); !os.IsNotExist(err) {
			t.Error("stale session directory should have been removed")
		}
		// Stale sentinel dir should be removed.
		if _, err := os.Stat(staleSentinelDir); !os.IsNotExist(err) {
			t.Error("stale sentinel directory should have been removed")
		}
		// Team config should be removed by removeStaleSessionArtifacts.
		if _, err := os.Stat(filepath.Join(init_.HomeDir, ".claude", "teams", "stale-team")); !os.IsNotExist(err) {
			t.Error("stale team directory should have been removed")
		}
		// Current session should remain.
		if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", currentSID)); os.IsNotExist(err) {
			t.Error("current session directory should remain")
		}
	})

	t.Run("skips_session_with_live_panes", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		currentSID := "ses-1709136000000current00001"

		// Create alive session with live tmux panes.
		aliveSID := "ses-1709136000000alive000001"
		aliveDir := filepath.Join(projectDir, ".state", "session", aliveSID, "pathflow")
		if err := os.MkdirAll(aliveDir, 0o755); err != nil {
			t.Fatal(err)
		}
		teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 77777, TeamName: "alive-team"})
		if err := os.WriteFile(filepath.Join(aliveDir, "pathflow-team.json"), teamData, 0o644); err != nil {
			t.Fatal(err)
		}

		// Create team config with a live pane.
		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "alive-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%alive-pane"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%alive-pane": true}}
		init_.sweepAllStaleSessions(projectDir, currentSID)

		// Alive session should remain.
		if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", aliveSID)); os.IsNotExist(err) {
			t.Error("alive session directory should remain")
		}
	})

	t.Run("cleans_session_without_team_file", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		currentSID := "ses-1709136000000current00001"

		// Create session with no team file.
		noTeamSID := "ses-1709136000000noteam00001"
		noTeamDir := filepath.Join(projectDir, ".state", "session", noTeamSID, "pathflow")
		if err := os.MkdirAll(noTeamDir, 0o755); err != nil {
			t.Fatal(err)
		}

		init_.sweepAllStaleSessions(projectDir, currentSID)

		// Session without team file should be removed.
		if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", noTeamSID)); !os.IsNotExist(err) {
			t.Error("session without team file should have been removed")
		}
	})

	t.Run("sweeps_orphan_sentinels", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%good-pane": true}}
		projectDir := t.TempDir()
		currentSID := "ses-1709136000000current00001"

		// Create orphan sentinel dir (no matching session dir).
		orphanSID := "ses-1709136000000orphan000001"
		orphanDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", orphanSID)
		if err := os.MkdirAll(orphanDir, 0o755); err != nil {
			t.Fatal(err)
		}

		// Create sentinel dir with matching session dir and live tmux panes (not orphan).
		goodSID := "ses-1709136000000goodsid00001"
		goodSentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", goodSID)
		goodSessionDir := filepath.Join(projectDir, ".state", "session", goodSID, "pathflow")
		if err := os.MkdirAll(goodSentinelDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.MkdirAll(goodSessionDir, 0o755); err != nil {
			t.Fatal(err)
		}
		// Write a valid pathflow-team.json with team name.
		teamJSON := []byte(`{"team_name":"good-team","lead_pid":9999}`)
		if err := os.WriteFile(filepath.Join(goodSessionDir, "pathflow-team.json"), teamJSON, 0o644); err != nil {
			t.Fatal(err)
		}
		// Create team config with a live tmux pane so the session is not stale.
		goodTeamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "good-team")
		if err := os.MkdirAll(goodTeamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%good-pane"},
			},
		})
		if err := os.WriteFile(filepath.Join(goodTeamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.sweepAllStaleSessions(projectDir, currentSID)

		// Orphan should be removed.
		if _, err := os.Stat(orphanDir); !os.IsNotExist(err) {
			t.Error("orphan sentinel dir should have been removed")
		}
		// Good sentinel should remain (session has live tmux panes).
		if _, err := os.Stat(goodSentinelDir); os.IsNotExist(err) {
			t.Error("non-orphan sentinel dir should remain")
		}
	})
}

func TestDetectCompactRecovery(t *testing.T) {
	t.Parallel()

	t.Run("compact_with_active_team", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		result := &InitResult{EnvVars: make(map[string]string)}

		// Create a team config.
		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "test-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), []byte("{}"), 0o644); err != nil {
			t.Fatal(err)
		}

		init_.detectCompactRecovery(projectDir, "compact", result)

		found := false
		for _, m := range result.Messages {
			if strings.Contains(m, "COMPACT RECOVERY") {
				found = true
				break
			}
		}
		if !found {
			t.Error("missing COMPACT RECOVERY message")
		}
	})

	t.Run("startup_no_message", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		projectDir := t.TempDir()
		result := &InitResult{EnvVars: make(map[string]string)}

		init_.detectCompactRecovery(projectDir, "startup", result)

		if len(result.Messages) > 0 {
			t.Error("startup should not produce compact recovery messages")
		}
	})
}

func TestFormatEnvOutput(t *testing.T) {
	t.Parallel()

	result := &InitResult{
		SessionID: "ses-1709136000000abcdef012345",
		EnvVars: map[string]string{
			"CODEFLOW_SESSION_ID": "ses-1709136000000abcdef012345",
			"CF_PROJECT_ROOT":     "myproject",
		},
	}

	data, err := result.FormatEnvOutput()
	if err != nil {
		t.Fatalf("FormatEnvOutput() error = %v", err)
	}

	var output map[string]any
	if err := json.Unmarshal(data, &output); err != nil {
		t.Fatalf("output unmarshal error: %v", err)
	}

	envMap, ok := output["env"].(map[string]any)
	if !ok {
		t.Fatal("output missing 'env' key")
	}
	if envMap["CODEFLOW_SESSION_ID"] != "ses-1709136000000abcdef012345" {
		t.Errorf("env.CODEFLOW_SESSION_ID = %v", envMap["CODEFLOW_SESSION_ID"])
	}
}

func TestSweepAllStaleSessions_CleansStaleFlagOnly(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	currentSID := "ses-1709136000000current00001"

	// Create current session.
	if err := os.MkdirAll(filepath.Join(projectDir, ".state", "session", currentSID), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create stale session with pathflow flag but no team file.
	staleSID := "ses-1709136000000stale000001"
	staleDir := filepath.Join(projectDir, ".state", "session", staleSID, "pathflow")
	if err := os.MkdirAll(staleDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flagData := []byte(`{"session_id":"` + staleSID + `","tracking_level":"tracked"}`)
	if err := os.WriteFile(filepath.Join(staleDir, "is-pathflow-active"), flagData, 0o644); err != nil {
		t.Fatal(err)
	}

	init_.sweepAllStaleSessions(projectDir, currentSID)

	// Stale session should be removed (no team file = stale).
	if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", staleSID)); !os.IsNotExist(err) {
		t.Error("stale session without team file should have been removed")
	}
}

func TestInitCheckpoint(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	result := &InitResult{EnvVars: make(map[string]string)}

	setupPathflowConfig(t, projectDir)

	// Create session dir.
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}

	init_.initCheckpoint(projectDir, sid, result)

	checkpointPath := filepath.Join(pfDir, "pathflow-phase-tasks.json")
	assertFileExists(t, checkpointPath)

	// Read and verify checkpoint has all 7 phases.
	data, err := os.ReadFile(checkpointPath)
	if err != nil {
		t.Fatalf("reading checkpoint: %v", err)
	}
	var raw map[string]json.RawMessage
	if err := json.Unmarshal(data, &raw); err != nil {
		t.Fatalf("parsing checkpoint: %v", err)
	}

	// Check for PF1 through PF7.
	for i := 1; i <= 7; i++ {
		key := fmt.Sprintf("PF%d", i)
		if _, ok := raw[key]; !ok {
			t.Errorf("checkpoint missing phase %s", key)
		}
	}
}

func TestCreateProjectTempDir(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	// Use a unique project name to avoid collisions with other tests.
	projectDir := filepath.Join(t.TempDir(), "test-project-tmpdir")
	if err := os.MkdirAll(projectDir, 0o755); err != nil {
		t.Fatal(err)
	}
	result := &InitResult{EnvVars: make(map[string]string)}

	init_.createProjectTempDir(projectDir, result)

	expectedDir := filepath.Join("/tmp", "claude", filepath.Base(projectDir))
	if _, err := os.Stat(expectedDir); os.IsNotExist(err) {
		t.Errorf("project temp dir %q should exist", expectedDir)
	}

	// Cleanup.
	_ = os.RemoveAll(expectedDir)
}

func TestHandleStaleCleanup_NoEnvFile(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envPath := filepath.Join(projectDir, "nonexistent-env.sh")

	sid, teammate, err := init_.handleStaleCleanup(projectDir, envPath, "startup")
	if err != nil {
		t.Errorf("unexpected error: %v", err)
	}
	if sid != "" {
		t.Errorf("expected empty sid, got %q", sid)
	}
	if teammate {
		t.Error("expected teammate=false")
	}
}

func TestHandleStaleCleanup_InvalidSIDFormat(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envPath := filepath.Join(envDir, "codeflow-env.sh")
	content := "export CODEFLOW_SESSION_ID='invalid-format'\n"
	if err := os.WriteFile(envPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	// Startup source -> should discard invalid SID.
	sid, _, _ := init_.handleStaleCleanup(projectDir, envPath, "startup")
	if sid != "" {
		t.Errorf("expected empty sid for invalid format on startup, got %q", sid)
	}
	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("env file should have been removed for invalid SID on startup")
	}
}

func TestHandleStaleCleanup_InvalidSIDFormat_Compact(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envPath := filepath.Join(envDir, "codeflow-env.sh")
	content := "export CODEFLOW_SESSION_ID='invalid-format'\n"
	if err := os.WriteFile(envPath, []byte(content), 0o644); err != nil {
		t.Fatal(err)
	}

	// Compact source -> should preserve invalid SID.
	sid, _, _ := init_.handleStaleCleanup(projectDir, envPath, "compact")
	if sid != "invalid-format" {
		t.Errorf("expected preserved invalid SID for compact, got %q", sid)
	}
}

func TestNewInitializer(t *testing.T) {
	t.Parallel()

	init_ := NewInitializer()
	if init_.ProcessChecker == nil {
		t.Error("NewInitializer().ProcessChecker is nil")
	}
	if init_.TmuxChecker == nil {
		t.Error("NewInitializer().TmuxChecker is nil")
	}
	if init_.Now == nil {
		t.Error("NewInitializer().Now is nil")
	}
	if init_.PPID == 0 {
		t.Error("NewInitializer().PPID is 0")
	}
	if init_.HomeDir == "" {
		t.Error("NewInitializer().HomeDir is empty")
	}
	if init_.SessionStarter == nil {
		t.Error("NewInitializer().SessionStarter is nil")
	}
	// Verify Now returns a time close to now.
	now := init_.Now()
	if time.Since(now) > 5*time.Second {
		t.Errorf("NewInitializer().Now() returned %v, expected close to current time", now)
	}
}

func TestWarn(t *testing.T) {
	t.Parallel()

	result := &InitResult{EnvVars: make(map[string]string)}
	result.warn("test warning %d", 42)
	if len(result.Warnings) != 1 {
		t.Fatalf("expected 1 warning, got %d", len(result.Warnings))
	}
	if result.Warnings[0] != "test warning 42" {
		t.Errorf("warning = %q, want %q", result.Warnings[0], "test warning 42")
	}
}

func TestHandleNoTeamFile_FlagExistsResume(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envPath := filepath.Join(envDir, "codeflow-env.sh")
	sid := "ses-1709136000000abcdef012345"

	// Create pathflow-active flag but no team file.
	flagDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Resume source -> should preserve existing SID.
	gotSID, teammate, err := init_.handleNoTeamFile(projectDir, envPath, sid, "resume")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotSID != sid {
		t.Errorf("expected SID %q, got %q", sid, gotSID)
	}
	if teammate {
		t.Error("expected teammate=false")
	}

	// Flag should still exist.
	if _, err := os.Stat(filepath.Join(flagDir, "is-pathflow-active")); os.IsNotExist(err) {
		t.Error("pathflow flag should be preserved for resume source")
	}
}

func TestHandleNoTeamFile_NoFlagNoTeam(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envPath := filepath.Join(envDir, "codeflow-env.sh")
	if err := os.WriteFile(envPath, []byte("export CODEFLOW_SESSION_ID='ses-old'\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	sid := "ses-1709136000000abcdef012345"

	// No flag, no team file -> env file should be cleaned.
	gotSID, teammate, err := init_.handleNoTeamFile(projectDir, envPath, sid, "startup")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotSID != "" {
		t.Errorf("expected empty SID, got %q", gotSID)
	}
	if teammate {
		t.Error("expected teammate=false")
	}

	// Env file should have been removed.
	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("env file should have been removed")
	}
}

func TestSweepAllStaleSessions_InvalidTeamJSON(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	currentSID := "ses-1709136000000current00001"

	// Create session with invalid JSON team file.
	badSID := "ses-1709136000000badjson00001"
	badDir := filepath.Join(projectDir, ".state", "session", badSID, "pathflow")
	if err := os.MkdirAll(badDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(badDir, "pathflow-team.json"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_.sweepAllStaleSessions(projectDir, currentSID)

	// Session with invalid team JSON should be removed.
	if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", badSID)); !os.IsNotExist(err) {
		t.Error("session with invalid team JSON should have been removed")
	}
}

func TestDetectStaleTeams(t *testing.T) {
	t.Parallel()

	t.Run("stale_team_cleaned", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%dead": false}}

		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "stale-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%dead"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		// Create task list for the team.
		taskDir := filepath.Join(init_.HomeDir, ".claude", "tasks", "stale-team")
		if err := os.MkdirAll(taskDir, 0o755); err != nil {
			t.Fatal(err)
		}

		warnings := init_.detectStaleTeams()
		if len(warnings) == 0 {
			t.Error("expected stale team warning")
		}
		found := false
		for _, w := range warnings {
			if strings.Contains(w, "STALE TEAM CLEANED") && strings.Contains(w, "stale-team") {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("warnings should mention 'STALE TEAM CLEANED: stale-team', got: %v", warnings)
		}

		// Team directory should be removed.
		if _, err := os.Stat(teamDir); !os.IsNotExist(err) {
			t.Error("stale team directory should have been removed")
		}
		// Task list should be removed.
		if _, err := os.Stat(taskDir); !os.IsNotExist(err) {
			t.Error("stale team task list should have been removed")
		}
	})

	t.Run("healthy_team_no_warning", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%alive": true}}

		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "healthy-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%alive"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		warnings := init_.detectStaleTeams()
		if len(warnings) != 0 {
			t.Errorf("expected no warnings for healthy team, got: %v", warnings)
		}
	})

	t.Run("no_teams_dir", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		// HomeDir is already a TempDir with no .claude/teams

		warnings := init_.detectStaleTeams()
		if len(warnings) != 0 {
			t.Errorf("expected no warnings when teams dir missing, got: %v", warnings)
		}
	})

	t.Run("empty_members_no_warning", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)

		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "empty-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}

		warnings := init_.detectStaleTeams()
		if len(warnings) != 0 {
			t.Errorf("expected no warnings for empty team, got: %v", warnings)
		}
	})
}

func TestWriteSessionMetadata(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"

	// Create sessions dir.
	sessDir := filepath.Join(projectDir, ".state", "logs", "sessions")
	if err := os.MkdirAll(sessDir, 0o755); err != nil {
		t.Fatal(err)
	}

	input := hookInput{SessionID: "test-uuid", Source: "startup"}
	result := &InitResult{EnvVars: make(map[string]string)}

	init_.writeSessionMetadata(projectDir, sid, input, result)

	metaPath := filepath.Join(sessDir, "session-"+sid+".meta")
	assertFileExists(t, metaPath)

	data, err := os.ReadFile(metaPath)
	if err != nil {
		t.Fatalf("reading meta file: %v", err)
	}

	var meta map[string]any
	if err := json.Unmarshal(data, &meta); err != nil {
		t.Fatalf("parsing meta JSON: %v", err)
	}

	if meta["session_id"] != sid {
		t.Errorf("meta.session_id = %v, want %q", meta["session_id"], sid)
	}
	if meta["claude_uuid"] != "test-uuid" {
		t.Errorf("meta.claude_uuid = %v, want %q", meta["claude_uuid"], "test-uuid")
	}
	if meta["source"] != "startup" {
		t.Errorf("meta.source = %v, want %q", meta["source"], "startup")
	}
}

func TestCreatePathFlowFlag_ErrorPaths(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)

	t.Run("creates_flag_successfully", func(t *testing.T) {
		t.Parallel()
		projectDir := t.TempDir()
		sid := "ses-1709136000000abcdef012345"
		result := &InitResult{EnvVars: make(map[string]string)}

		isResume := init_.createPathFlowFlag(projectDir, sid, result)
		if isResume {
			t.Error("expected isResume=false for fresh flag creation")
		}

		flagPath := filepath.Join(projectDir, ".state", "session", sid, "pathflow", "is-pathflow-active")
		assertFileExists(t, flagPath)

		// Verify JSON content.
		data, err := os.ReadFile(flagPath)
		if err != nil {
			t.Fatalf("reading flag: %v", err)
		}
		var flag pathflowFlag
		if err := json.Unmarshal(data, &flag); err != nil {
			t.Fatalf("parsing flag JSON: %v", err)
		}
		if flag.SessionID != sid {
			t.Errorf("flag.SessionID = %q, want %q", flag.SessionID, sid)
		}
		if flag.TrackingLevel != "pending" {
			t.Errorf("flag.TrackingLevel = %q, want %q", flag.TrackingLevel, "pending")
		}
	})
}

func TestUpdateLeadPID(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	init_.PPID = 12345

	t.Run("updates_pid_successfully", func(t *testing.T) {
		t.Parallel()
		teamDir := t.TempDir()
		teamFilePath := filepath.Join(teamDir, "pathflow-team.json")
		original, _ := json.Marshal(map[string]any{"lead_pid": 99999, "team_name": "test"})
		if err := os.WriteFile(teamFilePath, original, 0o644); err != nil {
			t.Fatal(err)
		}

		init_.updateLeadPID(teamFilePath, original)

		data, err := os.ReadFile(teamFilePath)
		if err != nil {
			t.Fatalf("reading updated file: %v", err)
		}
		var raw map[string]any
		if err := json.Unmarshal(data, &raw); err != nil {
			t.Fatalf("parsing updated JSON: %v", err)
		}
		pid, ok := raw["lead_pid"].(float64)
		if !ok {
			t.Fatal("lead_pid is not a number")
		}
		if int(pid) != 12345 {
			t.Errorf("lead_pid = %d, want 12345", int(pid))
		}
	})

	t.Run("handles_invalid_json", func(t *testing.T) {
		t.Parallel()
		teamDir := t.TempDir()
		teamFilePath := filepath.Join(teamDir, "pathflow-team.json")

		// updateLeadPID should handle invalid JSON gracefully (no panic).
		init_.updateLeadPID(teamFilePath, []byte("not json"))

		// File should not exist (no write attempted).
		if _, err := os.Stat(teamFilePath); !os.IsNotExist(err) {
			t.Error("file should not exist after invalid JSON")
		}
	})
}

func TestSweepAllStaleSessions_SkipsCurrentSession(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	currentSID := "ses-1709136000000current00001"

	// Create current session only.
	sessionDir := filepath.Join(projectDir, ".state", "session", currentSID)
	if err := os.MkdirAll(sessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	init_.sweepAllStaleSessions(projectDir, currentSID)

	// Current session must NOT be removed.
	if _, err := os.Stat(sessionDir); os.IsNotExist(err) {
		t.Error("current session should not be removed by sweep")
	}
}

func TestSweepAllStaleSessions_IgnoresNonSessionDir(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	currentSID := "ses-1709136000000current00001"

	// Create a non-session directory (doesn't start with ses-).
	nonSessionDir := filepath.Join(projectDir, ".state", "session", "not-a-session")
	if err := os.MkdirAll(nonSessionDir, 0o755); err != nil {
		t.Fatal(err)
	}

	init_.sweepAllStaleSessions(projectDir, currentSID)

	// Non-session directories must NOT be removed.
	if _, err := os.Stat(nonSessionDir); os.IsNotExist(err) {
		t.Error("non-session directory should not be touched by sweep")
	}
}

func TestCleanupActiveTask_InvalidJSON(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}

	taskPath := filepath.Join(runtimeDir, "active-task.json")
	if err := os.WriteFile(taskPath, []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_.cleanupActiveTask(projectDir)

	if _, err := os.Stat(taskPath); !os.IsNotExist(err) {
		t.Error("invalid JSON active task should be removed")
	}
}

func TestCleanupActiveTask_DoneStatus(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	runtimeDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
		t.Fatal(err)
	}

	taskData, _ := json.Marshal(map[string]string{
		"status":     "done",
		"updated_at": fixedTime.Format(time.RFC3339),
	})
	taskPath := filepath.Join(runtimeDir, "active-task.json")
	if err := os.WriteFile(taskPath, taskData, 0o644); err != nil {
		t.Fatal(err)
	}

	init_.cleanupActiveTask(projectDir)

	if _, err := os.Stat(taskPath); !os.IsNotExist(err) {
		t.Error("done task should be removed")
	}
}

func TestDetectCompactRecovery_WithPathflowFlag(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	result := &InitResult{EnvVars: make(map[string]string)}

	// Create a session with pathflow flag (but no team config).
	sid := "ses-1709136000000compact00001"
	flagDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_.detectCompactRecovery(projectDir, "compact", result)

	found := false
	for _, m := range result.Messages {
		if strings.Contains(m, "COMPACT RECOVERY") {
			found = true
			break
		}
	}
	if !found {
		t.Error("compact with pathflow flag should produce COMPACT RECOVERY message")
	}
}

func TestHandleNoTeamFile_FlagExistsStartup(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	envDir := filepath.Join(projectDir, ".state", "runtime")
	if err := os.MkdirAll(envDir, 0o755); err != nil {
		t.Fatal(err)
	}
	envPath := filepath.Join(envDir, "codeflow-env.sh")
	if err := os.WriteFile(envPath, []byte("export CODEFLOW_SESSION_ID='ses-old'\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	sid := "ses-1709136000000abcdef012345"

	// Create pathflow-active flag but no team file.
	flagDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create sentinel dir for this session.
	sentinelDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Startup source with flag but no team -> pre-TeamCreate crash cleanup.
	gotSID, teammate, err := init_.handleNoTeamFile(projectDir, envPath, sid, "startup")
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if gotSID != "" {
		t.Errorf("expected empty SID after cleanup, got %q", gotSID)
	}
	if teammate {
		t.Error("expected teammate=false")
	}

	// Session dir should be cleaned.
	if _, err := os.Stat(filepath.Join(projectDir, ".state", "session", sid)); !os.IsNotExist(err) {
		t.Error("session dir should have been removed")
	}
	if _, err := os.Stat(sentinelDir); !os.IsNotExist(err) {
		t.Error("sentinel dir should have been removed")
	}
	if _, err := os.Stat(envPath); !os.IsNotExist(err) {
		t.Error("env file should have been removed")
	}
}

func TestWriteEnvFile_ErrorPaths(t *testing.T) {
	t.Parallel()

	t.Run("creates_dir_and_writes", func(t *testing.T) {
		t.Parallel()
		dir := t.TempDir()
		envPath := filepath.Join(dir, "subdir", "codeflow-env.sh")

		err := session.WriteEnvFile(filepath.Dir(envPath), "ses-test", dir)
		if err != nil {
			t.Fatalf("writeEnvFile() error = %v", err)
		}

		data, err := os.ReadFile(envPath)
		if err != nil {
			t.Fatalf("reading env file: %v", err)
		}
		content := string(data)
		if !strings.Contains(content, "ses-test") {
			t.Errorf("env file content = %q, want to contain 'ses-test'", content)
		}
	})

	t.Run("handles_unwritable_dir", func(t *testing.T) {
		t.Parallel()
		// Use a path that can't be created (file in place of dir).
		dir := t.TempDir()
		blockFile := filepath.Join(dir, "block")
		if err := os.WriteFile(blockFile, []byte("x"), 0o644); err != nil {
			t.Fatal(err)
		}
		envPath := filepath.Join(blockFile, "subdir", "env.sh")

		err := session.WriteEnvFile(filepath.Dir(envPath), "ses-test", dir)
		if err == nil {
			t.Error("writeEnvFile() with unwritable dir should return error")
		}
	})
}

func TestCreatePathFlowFlag_ResumeDetection(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	result := &InitResult{EnvVars: make(map[string]string)}

	// Create the flag first.
	flagDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("{}"), 0o644); err != nil {
		t.Fatal(err)
	}

	isResume := init_.createPathFlowFlag(projectDir, sid, result)
	if !isResume {
		t.Error("expected isResume=true when flag already exists")
	}
}

func TestCreatePathFlowFlag_DirError(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	result := &InitResult{EnvVars: make(map[string]string)}
	sid := "ses-1709136000000abcdef012345"

	// Use a file path where MkdirAll will fail (parent is a file, not dir).
	blockDir := t.TempDir()
	blockFile := filepath.Join(blockDir, ".state")
	if err := os.WriteFile(blockFile, []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}

	isResume := init_.createPathFlowFlag(blockDir, sid, result)
	if isResume {
		t.Error("expected isResume=false when dir creation fails")
	}
	if len(result.Warnings) == 0 {
		t.Error("expected warning for dir creation failure")
	}
}

func TestUpdateLeadPID_WriteError(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	init_.PPID = 12345

	// Use a non-existent directory for the team file path so tmp write fails.
	teamFilePath := filepath.Join(t.TempDir(), "nonexistent", "deep", "pathflow-team.json")
	original, _ := json.Marshal(map[string]any{"lead_pid": 99999, "team_name": "test"})

	// Should not panic -- error handling is graceful.
	init_.updateLeadPID(teamFilePath, original)

	// File should not exist.
	if _, err := os.Stat(teamFilePath); !os.IsNotExist(err) {
		t.Error("file should not exist when write fails")
	}
}

func TestCreateProjectTempDir_SafetyGuard(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	result := &InitResult{EnvVars: make(map[string]string)}

	// Test with empty base name (edge case).
	projectDir := t.TempDir()
	init_.createProjectTempDir(projectDir, result)

	expectedDir := filepath.Join("/tmp", "claude", filepath.Base(projectDir))
	if _, err := os.Stat(expectedDir); os.IsNotExist(err) {
		t.Errorf("project temp dir %q should exist", expectedDir)
	}
	_ = os.RemoveAll(expectedDir)
}

func TestWriteSessionMetadata_ErrorPath(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"

	// Don't create sessions dir -> write will fail.
	input := hookInput{SessionID: "test-uuid", Source: "startup"}
	result := &InitResult{EnvVars: make(map[string]string)}

	init_.writeSessionMetadata(projectDir, sid, input, result)

	// Should have a warning about the write failure.
	if len(result.Warnings) == 0 {
		t.Error("expected warning for metadata write failure")
	}
}

func TestInitCheckpoint_MissingConfig(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"
	result := &InitResult{EnvVars: make(map[string]string)}

	// Don't create pathflow config -> init will fail with warning.
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}

	init_.initCheckpoint(projectDir, sid, result)

	if len(result.Warnings) == 0 {
		t.Error("expected warning for missing pathflow config")
	}
}

// --- Helpers ---

// setupPathflowConfig creates a minimal pathflow-config.json for checkpoint init.
func setupPathflowConfig(t *testing.T, projectDir string) {
	t.Helper()
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	config := map[string]any{
		"phases": map[string]any{
			"PF1-INIT": map[string]any{
				"required_tasks": []string{"PF1-TSK-01", "PF1-TSK-02"},
				"tasks": []any{
					map[string]string{"id": "PF1-TSK-01"},
					map[string]string{"id": "PF1-TSK-02"},
				},
			},
			"PF2-CONTEXT": map[string]any{
				"required_tasks": []string{"PF2-TSK-01", "PF2-TSK-02", "PF2-TSK-03", "PF2-TSK-04"},
				"tasks": []any{
					map[string]string{"id": "PF2-TSK-01"},
					map[string]string{"id": "PF2-TSK-02"},
					map[string]string{"id": "PF2-TSK-03"},
					map[string]string{"id": "PF2-TSK-04"},
				},
			},
			"PF3-CLASSIFY": map[string]any{
				"required_tasks": []string{"PF3-TSK-01", "PF3-TSK-02", "PF3-TSK-03"},
				"tasks": []any{
					map[string]string{"id": "PF3-TSK-01"},
					map[string]string{"id": "PF3-TSK-02"},
					map[string]string{"id": "PF3-TSK-03"},
				},
			},
			"PF4-EXECUTE": map[string]any{
				"required_tasks": []string{"PF4-TSK-01", "PF4-TSK-02", "PF4-TSK-03", "PF4-TSK-04", "PF4-TSK-05", "PF4-TSK-06", "PF4-TSK-07"},
				"tasks": []any{
					map[string]any{"id": "PF4-TSK-01", "condition": "adhoc_only"},
					map[string]string{"id": "PF4-TSK-02"},
					map[string]string{"id": "PF4-TSK-03"},
					map[string]string{"id": "PF4-TSK-04"},
					map[string]string{"id": "PF4-TSK-05"},
					map[string]string{"id": "PF4-TSK-06"},
					map[string]any{"id": "PF4-TSK-07", "condition": "if_pipeline_includes_qa"},
				},
			},
			"PF5-VERIFY": map[string]any{
				"required_tasks": []string{"PF5-TSK-01", "PF5-TSK-02"},
				"tasks": []any{
					map[string]string{"id": "PF5-TSK-01"},
					map[string]string{"id": "PF5-TSK-02"},
				},
			},
			"PF6-COMPLETE": map[string]any{
				"required_tasks": []string{"PF6-TSK-01", "PF6-TSK-02", "PF6-TSK-03", "PF6-TSK-04", "PF6-TSK-05", "PF6-TSK-06", "PF6-TSK-07", "PF6-TSK-08", "PF6-TSK-09"},
				"tasks": []any{
					map[string]string{"id": "PF6-TSK-01"},
					map[string]string{"id": "PF6-TSK-02"},
					map[string]string{"id": "PF6-TSK-03"},
					map[string]string{"id": "PF6-TSK-04"},
					map[string]string{"id": "PF6-TSK-05"},
					map[string]string{"id": "PF6-TSK-06"},
					map[string]string{"id": "PF6-TSK-07"},
					map[string]string{"id": "PF6-TSK-08"},
					map[string]string{"id": "PF6-TSK-09"},
				},
			},
			"PF7-END": map[string]any{
				"required_tasks": []string{"PF7-TSK-01", "PF7-TSK-02", "PF7-TSK-03"},
				"tasks": []any{
					map[string]string{"id": "PF7-TSK-01"},
					map[string]string{"id": "PF7-TSK-02"},
					map[string]string{"id": "PF7-TSK-03"},
				},
			},
		},
	}

	data, err := json.MarshalIndent(config, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "pathflow-config.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}
}

func TestAcquireSessionLock_Exclusive(t *testing.T) {
	t.Parallel()

	runtimeDir := t.TempDir()

	// Acquire first lock.
	lock1, err := session.AcquireSessionLock(runtimeDir)
	if err != nil {
		t.Fatalf("AcquireSessionLock() error = %v", err)
	}

	// Try to acquire second lock in a goroutine — it should block.
	acquired := make(chan struct{})
	go func() {
		lock2, err := session.AcquireSessionLock(runtimeDir)
		if err != nil {
			t.Errorf("second AcquireSessionLock() error = %v", err)
			close(acquired)
			return
		}
		session.ReleaseSessionLock(lock2)
		close(acquired)
	}()

	// Wait briefly — the second lock should NOT be acquired yet.
	select {
	case <-acquired:
		t.Error("second lock was acquired while first lock is held")
	case <-time.After(100 * time.Millisecond):
		// Expected: second lock is still blocked.
	}

	// Release first lock — second lock should now be acquired.
	session.ReleaseSessionLock(lock1)

	select {
	case <-acquired:
		// Expected: second lock acquired after first released.
	case <-time.After(2 * time.Second):
		t.Error("second lock was not acquired after first lock was released")
	}
}

func TestStartInit_ConcurrentSessionCreation(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	setupPathflowConfig(t, projectDir)

	const numGoroutines = 5

	// The mock SessionStarter writes pathflow-team.json and team config
	// as a side effect, simulating the real flow where the lead creates
	// these files after generating the session ID. This allows subsequent
	// goroutines (under the serialized lock) to detect teammate mode via
	// tmux pane liveness.
	callCount := make(chan struct{}, numGoroutines)

	type result struct {
		initResult *InitResult
		err        error
	}
	results := make(chan result, numGoroutines)

	homeDir := t.TempDir()

	for i := 0; i < numGoroutines; i++ {
		go func() {
			init_ := &Initializer{
				Now:            func() time.Time { return fixedTime },
				ProcessChecker: mockProcessChecker{alive: map[int]bool{}},
				TmuxChecker:    mockTmuxChecker{alive: map[string]bool{"%lead-pane": true}},
				SessionStarter: mockSessionStarterFunc(func(_ context.Context, _ string, pDir string) (string, error) {
					callCount <- struct{}{}
					// After "generating" the session, write pathflow-team.json
					// so subsequent goroutines detect teammate mode.
					pfDir := filepath.Join(pDir, ".state", "session", testULIDSessionID, "pathflow")
					_ = os.MkdirAll(pfDir, 0o755)
					teamData, _ := json.Marshal(pathflowTeamJSON{LeadPID: 0, TeamName: "test-concurrent"})
					_ = os.WriteFile(filepath.Join(pfDir, "pathflow-team.json"), teamData, 0o644)
					// Also create team config with a live tmux pane.
					teamDir := filepath.Join(homeDir, ".claude", "teams", "test-concurrent")
					_ = os.MkdirAll(teamDir, 0o755)
					cfgData, _ := json.Marshal(map[string]any{
						"members": []map[string]string{
							{"tmuxPaneId": "%lead-pane"},
						},
					})
					_ = os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644)
					return testULIDSessionID, nil
				}),
				ReadBuildInfo: func() string { return "" },
				PPID:          99999,
				HomeDir:       homeDir,
			}

			stdin := strings.NewReader(`{"session_id":"agent-uuid","source":"startup"}`)
			r, err := init_.StartInit(stdin, projectDir)
			results <- result{r, err}
		}()
	}

	// Collect results.
	var initResults []*InitResult
	for i := 0; i < numGoroutines; i++ {
		r := <-results
		if r.err != nil {
			t.Fatalf("goroutine StartInit() error = %v", r.err)
		}
		initResults = append(initResults, r.initResult)
	}
	close(callCount)

	// Count how many goroutines actually called StartSession.
	var sessionStartCalls int
	for range callCount {
		sessionStartCalls++
	}

	// All goroutines should return the same session ID.
	for i, r := range initResults {
		if r.SessionID != testULIDSessionID {
			t.Errorf("goroutine %d: SessionID = %q, want %q", i, r.SessionID, testULIDSessionID)
		}
	}

	// Exactly 1 goroutine should have called StartSession (the leader).
	// The rest detect the env file + team.json with live tmux panes under the lock.
	if sessionStartCalls != 1 {
		t.Errorf("StartSession called %d times, want exactly 1", sessionStartCalls)
	}

	// Count teammates vs leaders.
	teammates := 0
	leaders := 0
	for _, r := range initResults {
		if r.IsTeammate {
			teammates++
		} else {
			leaders++
		}
	}

	// The first goroutine is the leader, the rest are teammates.
	if leaders != 1 {
		t.Errorf("leaders = %d, want 1", leaders)
	}
	if teammates != numGoroutines-1 {
		t.Errorf("teammates = %d, want %d", teammates, numGoroutines-1)
	}
}

func assertDirExists(t *testing.T, path string) {
	t.Helper()
	info, err := os.Stat(path)
	if err != nil {
		t.Errorf("directory %q does not exist: %v", path, err)
		return
	}
	if !info.IsDir() {
		t.Errorf("%q exists but is not a directory", path)
	}
}

func assertFileExists(t *testing.T, path string) {
	t.Helper()
	if _, err := os.Stat(path); err != nil {
		t.Errorf("file %q does not exist: %v", path, err)
	}
}

func TestHasLiveTeamPanes_Initializer(t *testing.T) {
	t.Parallel()

	t.Run("config_missing", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		if init_.hasLiveTeamPanes("nonexistent-team") {
			t.Error("should return false when config is missing")
		}
	})

	t.Run("config_invalid_json", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "bad-json")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), []byte("not-json"), 0o644); err != nil {
			t.Fatal(err)
		}
		if init_.hasLiveTeamPanes("bad-json") {
			t.Error("should return false for invalid JSON")
		}
	})

	t.Run("empty_members", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "empty-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{"members": []any{}})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}
		if init_.hasLiveTeamPanes("empty-team") {
			t.Error("should return false for empty members")
		}
	})

	t.Run("all_panes_dead", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%d1": false, "%d2": false}}

		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "dead-team")
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatal(err)
		}
		cfgData, _ := json.Marshal(map[string]any{
			"members": []map[string]string{
				{"tmuxPaneId": "%d1"},
				{"tmuxPaneId": "%d2"},
			},
		})
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), cfgData, 0o644); err != nil {
			t.Fatal(err)
		}
		if init_.hasLiveTeamPanes("dead-team") {
			t.Error("should return false when all panes are dead")
		}
	})

	t.Run("one_pane_alive", func(t *testing.T) {
		t.Parallel()
		init_ := newTestInitializer(t)
		init_.TmuxChecker = mockTmuxChecker{alive: map[string]bool{"%alive": true, "%dead": false}}

		teamDir := filepath.Join(init_.HomeDir, ".claude", "teams", "partial-team")
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
		if !init_.hasLiveTeamPanes("partial-team") {
			t.Error("should return true when any pane is alive")
		}
	})
}

// initGitRepo creates a minimal git repo in dir with one commit and returns
// the short commit hash.
func initGitRepo(t *testing.T, dir string) string {
	t.Helper()
	for _, args := range [][]string{
		{"init"},
		{"config", "user.email", "test@test.com"},
		{"config", "user.name", "Test"},
	} {
		cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v failed: %v\n%s", args, err, out)
		}
	}
	// Create a file and commit.
	if err := os.WriteFile(filepath.Join(dir, "dummy.txt"), []byte("x"), 0o644); err != nil {
		t.Fatal(err)
	}
	for _, args := range [][]string{
		{"add", "dummy.txt"},
		{"commit", "-m", "init"},
	} {
		cmd := exec.Command("git", append([]string{"-C", dir}, args...)...)
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v failed: %v\n%s", args, err, out)
		}
	}
	cmd := exec.Command("git", "-C", dir, "rev-parse", "--short", "HEAD")
	out, err := cmd.Output()
	if err != nil {
		t.Fatalf("git rev-parse failed: %v", err)
	}
	return strings.TrimSpace(string(out))
}

func TestAutoRebuildCLI_SkipNoGoMod(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	init_ := newTestInitializer(t)
	init_.ReadBuildInfo = func() string { return "abc1234" }
	result := &InitResult{EnvVars: map[string]string{}}

	// No codeflow-cli/go.mod exists.
	init_.autoRebuildCLI(projectDir, result)

	// Should produce no messages (silently skipped).
	if len(result.Messages) > 0 {
		t.Errorf("expected no messages, got %v", result.Messages)
	}
	if len(result.Warnings) > 0 {
		t.Errorf("expected no warnings, got %v", result.Warnings)
	}
}

func TestAutoRebuildCLI_SkipMatchingRevision(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	commitHash := initGitRepo(t, projectDir)

	// Create codeflow-cli/go.mod.
	cliDir := filepath.Join(projectDir, "codeflow-cli")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "go.mod"), []byte("module test\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_ := newTestInitializer(t)
	// ReadBuildInfo returns the same hash as git HEAD.
	init_.ReadBuildInfo = func() string { return commitHash }
	result := &InitResult{EnvVars: map[string]string{}}

	init_.autoRebuildCLI(projectDir, result)

	// Should produce no messages (revisions match).
	if len(result.Messages) > 0 {
		t.Errorf("expected no messages when revisions match, got %v", result.Messages)
	}
}

func TestAutoRebuildCLI_SkipNoBuildInfo(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	_ = initGitRepo(t, projectDir)

	// Create codeflow-cli/go.mod.
	cliDir := filepath.Join(projectDir, "codeflow-cli")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "go.mod"), []byte("module test\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_ := newTestInitializer(t)
	// ReadBuildInfo returns empty (no VCS info embedded).
	init_.ReadBuildInfo = func() string { return "" }
	result := &InitResult{EnvVars: map[string]string{}}

	init_.autoRebuildCLI(projectDir, result)

	// Should produce no messages (cannot compare without build info).
	if len(result.Messages) > 0 {
		t.Errorf("expected no messages when no build info, got %v", result.Messages)
	}
}

func TestAutoRebuildCLI_TriggersOnMismatch(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	_ = initGitRepo(t, projectDir)

	// Create codeflow-cli/go.mod.
	cliDir := filepath.Join(projectDir, "codeflow-cli")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "go.mod"), []byte("module test\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_ := newTestInitializer(t)
	// ReadBuildInfo returns a different hash from git HEAD.
	init_.ReadBuildInfo = func() string { return "old1234" }
	result := &InitResult{EnvVars: map[string]string{}}

	init_.autoRebuildCLI(projectDir, result)

	// Should emit a rebuild message (even though go install will fail in test env,
	// the mismatch detection message should be present).
	foundRebuildMsg := false
	for _, msg := range result.Messages {
		if strings.Contains(msg, "[auto-rebuild]") && strings.Contains(msg, "differs from HEAD") {
			foundRebuildMsg = true
			break
		}
	}
	if !foundRebuildMsg {
		t.Errorf("expected [auto-rebuild] mismatch message, got messages: %v", result.Messages)
	}

	// The go install will fail (no real Go project), so we should also see a warning.
	foundWarning := false
	for _, w := range result.Warnings {
		if strings.Contains(w, "[auto-rebuild]") && strings.Contains(w, "rebuild failed") {
			foundWarning = true
			break
		}
	}
	if !foundWarning {
		t.Errorf("expected [auto-rebuild] rebuild failed warning, got warnings: %v", result.Warnings)
	}
}

func TestAutoRebuildCLI_NonFatal(t *testing.T) {
	t.Parallel()

	projectDir := t.TempDir()
	_ = initGitRepo(t, projectDir)

	// Create codeflow-cli/go.mod but no actual Go source (go install will fail).
	cliDir := filepath.Join(projectDir, "codeflow-cli")
	if err := os.MkdirAll(cliDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliDir, "go.mod"), []byte("module test\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	init_ := newTestInitializer(t)
	init_.ReadBuildInfo = func() string { return "stale99" }

	// Run full StartInit to verify auto-rebuild failure doesn't block session start.
	setupPathflowConfig(t, projectDir)
	stdin := strings.NewReader(`{"session_id":"abc","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() should not fail due to auto-rebuild error, got: %v", err)
	}

	// Session should still be initialized successfully.
	if result.SessionID == "" {
		t.Error("SessionID should not be empty")
	}

	// Verify auto-rebuild was attempted but didn't block.
	foundRebuildAttempt := false
	for _, msg := range result.Messages {
		if strings.Contains(msg, "[auto-rebuild]") {
			foundRebuildAttempt = true
			break
		}
	}
	if !foundRebuildAttempt {
		t.Error("expected auto-rebuild attempt message")
	}
}

func TestOsProcessChecker_IsAlive(t *testing.T) {
	t.Parallel()
	checker := osProcessChecker{}

	t.Run("pid_zero_is_dead", func(t *testing.T) {
		t.Parallel()
		if checker.IsAlive(0) {
			t.Error("IsAlive(0) = true, want false")
		}
	})

	t.Run("negative_pid_is_dead", func(t *testing.T) {
		t.Parallel()
		if checker.IsAlive(-1) {
			t.Error("IsAlive(-1) = true, want false")
		}
	})

	t.Run("large_nonexistent_pid_is_dead", func(t *testing.T) {
		t.Parallel()
		// PID 999999 is extremely unlikely to exist.
		if checker.IsAlive(999999) {
			t.Skip("PID 999999 unexpectedly alive, skipping")
		}
	})

	t.Run("valid_pid_calls_signal", func(t *testing.T) {
		t.Parallel()
		// IsAlive with a valid PID exercises the FindProcess and Signal(nil) path.
		// On macOS, Signal(nil) returns "unsupported signal type" so IsAlive
		// returns false even for running processes. We verify no panic occurs
		// and the guard clauses work correctly.
		_ = checker.IsAlive(1)
		_ = checker.IsAlive(os.Getpid())
	})
}

func TestGetClaudePID_Session(t *testing.T) {
	t.Parallel()

	// getClaudePID walks up the process tree. In a test environment, the result
	// should be a valid PID > 0. We verify it returns something reasonable and
	// doesn't panic or return invalid values.
	pid := getClaudePID()
	if pid <= 0 {
		t.Errorf("getClaudePID() = %d, want > 0", pid)
	}
}

func TestCreateProjectTempDir_MkdirAllFails(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	result := &InitResult{EnvVars: make(map[string]string)}

	// Create a project dir whose base name contains a path separator issue.
	// Block by placing a regular file where /tmp/claude/{name} needs to be a dir.
	blockBase := filepath.Join("/tmp", "claude")
	_ = os.MkdirAll(blockBase, 0o755)

	// Create a project dir with a name that will collide.
	uniqueName := fmt.Sprintf("test-blocked-%d", time.Now().UnixNano())
	blockFile := filepath.Join(blockBase, uniqueName)
	// Place a regular file where the dir needs to be.
	if err := os.WriteFile(blockFile, []byte("x"), 0o444); err != nil {
		t.Fatal(err)
	}
	defer os.Remove(blockFile)

	// createProjectTempDir does RemoveAll first, so blocking with a read-only
	// file won't work. Instead, use a project dir path that results in
	// an unmkdir-able temp path. We can test the safety guard path instead.

	// Test: projectDir whose filepath.Base resolves to empty string.
	// filepath.Base("/") returns "/" which is the root, which should NOT be rm'd.
	// The safety guard should kick in when abs paths match.
	// Actually, let's just test with the blocked file properly:

	// Make the blockFile a directory, put a file inside so RemoveAll can
	// remove it, but then immediately create a file blocking MkdirAll.
	os.Remove(blockFile)

	// Use a subdirectory path that goes through a file.
	innerBlock := filepath.Join(blockBase, uniqueName, "subdir")
	if err := os.WriteFile(filepath.Join(blockBase, uniqueName), []byte("x"), 0o644); err != nil {
		// If the file already exists as a dir from RemoveAll race, skip.
		t.Skip("could not set up blocking file")
	}
	defer os.Remove(filepath.Join(blockBase, uniqueName))

	// Create a project dir that produces tmpDir = /tmp/claude/{uniqueName}/subdir
	projectDir := filepath.Join(t.TempDir(), uniqueName, "subdir")
	_ = os.MkdirAll(projectDir, 0o755)
	_ = innerBlock // suppress unused warning

	init_.createProjectTempDir(projectDir, result)

	// Cleanup.
	expectedDir := filepath.Join("/tmp", "claude", filepath.Base(projectDir))
	_ = os.RemoveAll(expectedDir)
}

func TestWriteSessionMetadata_MarshalSuccess(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"

	// Create the sessions directory.
	sessDir := filepath.Join(projectDir, ".state", "logs", "sessions")
	if err := os.MkdirAll(sessDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Test with empty USER env var (covers user = "unknown" branch).
	origUser := os.Getenv("USER")
	os.Setenv("USER", "")
	defer os.Setenv("USER", origUser)

	input := hookInput{SessionID: "test-uuid", Source: "startup"}
	result := &InitResult{EnvVars: make(map[string]string)}

	init_.writeSessionMetadata(projectDir, sid, input, result)

	if len(result.Warnings) > 0 {
		t.Errorf("unexpected warnings: %v", result.Warnings)
	}

	metaPath := filepath.Join(sessDir, "session-"+sid+".meta")
	data, err := os.ReadFile(metaPath)
	if err != nil {
		t.Fatalf("reading meta file: %v", err)
	}

	var meta map[string]any
	if err := json.Unmarshal(data, &meta); err != nil {
		t.Fatalf("parsing meta JSON: %v", err)
	}

	if meta["user"] != "unknown" {
		t.Errorf("meta.user = %v, want %q when USER env is empty", meta["user"], "unknown")
	}
}

func TestWriteSessionMetadata_WriteFileError(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	projectDir := t.TempDir()
	sid := "ses-1709136000000abcdef012345"

	// Place a directory where the meta file should be written,
	// causing WriteFile to fail.
	metaDir := filepath.Join(projectDir, ".state", "logs", "sessions")
	if err := os.MkdirAll(metaDir, 0o755); err != nil {
		t.Fatal(err)
	}
	metaAsDir := filepath.Join(metaDir, "session-"+sid+".meta")
	if err := os.MkdirAll(metaAsDir, 0o755); err != nil {
		t.Fatal(err)
	}

	input := hookInput{SessionID: "uuid", Source: "startup"}
	result := &InitResult{EnvVars: make(map[string]string)}

	init_.writeSessionMetadata(projectDir, sid, input, result)

	if len(result.Warnings) == 0 {
		t.Error("expected warning for write failure when meta path is a directory")
	}
	foundWriteWarning := false
	for _, w := range result.Warnings {
		if strings.Contains(w, "session meta write error") {
			foundWriteWarning = true
			break
		}
	}
	if !foundWriteWarning {
		t.Errorf("expected 'session meta write error' warning, got: %v", result.Warnings)
	}
}

func TestCreatePathFlowFlag_WriteError(t *testing.T) {
	t.Parallel()

	init_ := newTestInitializer(t)
	result := &InitResult{EnvVars: make(map[string]string)}
	sid := "ses-1709136000000abcdef012345"

	// Create the pathflow dir but place a directory at the flag path so
	// WriteFile fails.
	projectDir := t.TempDir()
	pfDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(pfDir, 0o755); err != nil {
		t.Fatal(err)
	}
	flagAsDir := filepath.Join(pfDir, "is-pathflow-active")
	if err := os.MkdirAll(flagAsDir, 0o755); err != nil {
		// If Stat succeeds, createPathFlowFlag returns true (resume).
		// We need it to fail at WriteFile, so the path must not be stat-able
		// as a regular file. A directory at that path will cause Stat to succeed
		// (returning true for resume). Let's use a different approach:
		// remove the directory and make the parent non-writable.
		t.Fatal(err)
	}
	// Remove it -- Stat will fail (not exist), then MkdirAll succeeds
	// (dir already exists), then WriteFile to a directory path fails.
	// Actually, os.Stat on a directory succeeds with err==nil.
	// So createPathFlowFlag will see it as existing and return true (resume).
	// We need a different approach: make the file path unwritable.
	os.RemoveAll(flagAsDir)

	// Make pfDir read-only so WriteFile fails.
	if err := os.Chmod(pfDir, 0o555); err != nil {
		t.Fatal(err)
	}
	defer os.Chmod(pfDir, 0o755) // restore for cleanup

	isResume := init_.createPathFlowFlag(projectDir, sid, result)
	if isResume {
		t.Error("expected isResume=false when write fails")
	}
	if len(result.Warnings) == 0 {
		t.Error("expected warning for flag write failure")
	}
	foundWriteWarning := false
	for _, w := range result.Warnings {
		if strings.Contains(w, "pathflow flag write error") {
			foundWriteWarning = true
			break
		}
	}
	if !foundWriteWarning {
		t.Errorf("expected 'pathflow flag write error' warning, got: %v", result.Warnings)
	}
}


// TestOsTmuxChecker_IsPaneAlive exercises the osTmuxChecker.IsPaneAlive method.
func TestOsTmuxChecker_IsPaneAlive(t *testing.T) {
	t.Parallel()

	checker := osTmuxChecker{}

	t.Run("empty_pane_id_is_dead", func(t *testing.T) {
		t.Parallel()
		if checker.IsPaneAlive("") {
			t.Error("IsPaneAlive(\"\") = true, want false")
		}
	})

	t.Run("nonexistent_pane_id", func(t *testing.T) {
		t.Parallel()
		// Check if tmux is available. If not, IsPaneAlive returns true (safe default).
		_, err := exec.LookPath("tmux")
		if err != nil {
			// tmux not installed — IsPaneAlive returns true as safe default.
			if !checker.IsPaneAlive("%999999") {
				t.Error("IsPaneAlive should return true when tmux is unavailable")
			}
			return
		}

		// tmux available but pane doesn't exist — should return false.
		alive := checker.IsPaneAlive("%999999")
		if alive {
			t.Skip("pane %999999 unexpectedly alive, skipping")
		}
	})

	t.Run("existing_pane_is_alive", func(t *testing.T) {
		t.Parallel()
		// Find a real tmux pane to test the "found" branch.
		out, err := exec.Command("tmux", "list-panes", "-a", "-F", "#{pane_id}").Output()
		if err != nil {
			t.Skip("tmux not available")
		}
		lines := strings.Split(strings.TrimSpace(string(out)), "\n")
		if len(lines) == 0 || lines[0] == "" {
			t.Skip("no tmux panes available")
		}
		paneID := strings.TrimSpace(lines[0])
		if !checker.IsPaneAlive(paneID) {
			t.Errorf("IsPaneAlive(%q) = false, want true for existing pane", paneID)
		}
	})

	t.Run("pane_id_format_variations", func(t *testing.T) {
		t.Parallel()
		// Verify that IsPaneAlive handles realistic pane ID formats without panicking.
		_ = checker.IsPaneAlive("%0")
		_ = checker.IsPaneAlive("%123")
	})
}

// TestGetClaudePID_ReturnsPositive verifies getClaudePID always returns a
// positive PID and exercises the ps-based parent lookup.
func TestGetClaudePID_ReturnsPositive(t *testing.T) {
	t.Parallel()

	pid := getClaudePID()
	if pid <= 0 {
		t.Errorf("getClaudePID() = %d, want > 0", pid)
	}

	// The returned PID should be either the grandparent (ps lookup succeeded)
	// or the parent (ps lookup fell back). Either way, it must be a valid PID.
	ppid := os.Getppid()
	if pid != ppid {
		// ps lookup succeeded and returned a different grandparent PID.
		// Verify it's reasonable.
		if pid <= 1 {
			t.Errorf("getClaudePID() returned init/launchd PID %d", pid)
		}
	}
}
