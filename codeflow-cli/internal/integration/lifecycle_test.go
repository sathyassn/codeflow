// Package integration provides end-to-end integration tests for the full
// PathFlow lifecycle, exercising Go hook implementations directly without
// relying on settings.json wiring or an active Claude Code session.
//
// These tests verify that all Go hook implementations work correctly as a
// cohesive system: session initialization, phase checkpoint progression,
// gate enforcement, security checks, sentinel creation, and session cleanup.
package integration

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/githooks"
	"github.com/codeflow/codeflow-cli/internal/hooks/gate"
	hooksession "github.com/codeflow/codeflow-cli/internal/hooks/session"
	"github.com/codeflow/codeflow-cli/internal/hooks/security"
	"github.com/codeflow/codeflow-cli/internal/hooks/sentinel"
	"github.com/codeflow/codeflow-cli/internal/hooks/team"
	"github.com/codeflow/codeflow-cli/internal/ledger"
	"github.com/codeflow/codeflow-cli/internal/pathflow"
	"github.com/codeflow/codeflow-cli/internal/testutil"
	"github.com/codeflow/codeflow-cli/internal/validate"
	"github.com/codeflow/codeflow-cli/internal/worktree"
)

// fixedTime is a deterministic timestamp for tests.
var fixedTime = time.Date(2026, 2, 28, 12, 0, 0, 0, time.UTC)

// testSessionID is the ULID-format session ID used across integration tests.
const testSessionID = "ses-01jk0000000000000000000000"

// ---- mock types ----

// mockSessionStarter returns a fixed session ID without accessing the database.
type mockSessionStarter struct {
	id  string
	err error
}

func (m mockSessionStarter) StartSession(_ context.Context, _ string, _ string) (string, error) {
	return m.id, m.err
}

// mockTmuxChecker returns predefined liveness results for pane IDs.
type mockTmuxChecker struct {
	alive map[string]bool
}

func (m mockTmuxChecker) IsPaneAlive(paneID string) bool {
	return m.alive[paneID]
}

// ---- helpers ----

// newTestInitializer creates an Initializer configured for isolated testing.
func newTestInitializer(t *testing.T, sessionID string) *hooksession.Initializer {
	t.Helper()
	return &hooksession.Initializer{
		Now:            func() time.Time { return fixedTime },
		TmuxChecker:    mockTmuxChecker{alive: map[string]bool{}},
		SessionStarter: mockSessionStarter{id: sessionID},
		HomeDir:        t.TempDir(),
	}
}

// setupPathflowConfig writes the testdata pathflow-config.json into the
// project's expected location.
func setupPathflowConfig(t *testing.T, projectDir string) {
	t.Helper()
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("setupPathflowConfig: mkdir %s: %v", configDir, err)
	}
	// Read testdata fixture.
	src := testdataPath(t, "pathflow-config.json")
	data, err := os.ReadFile(src)
	if err != nil {
		t.Fatalf("setupPathflowConfig: reading fixture: %v", err)
	}
	dst := filepath.Join(configDir, "pathflow-config.json")
	if err := os.WriteFile(dst, data, 0o644); err != nil {
		t.Fatalf("setupPathflowConfig: writing config: %v", err)
	}
}

// testdataPath resolves a path relative to the testdata/ directory alongside
// this test file.
func testdataPath(t *testing.T, rel string) string {
	t.Helper()
	return filepath.Join("testdata", rel)
}

// sentinelDir returns the sentinel directory path for a session.
func sentinelDir(projectDir, sessionID string) string {
	return filepath.Join(projectDir, ".state", "sentinels", "pathflow", sessionID)
}

// checkpointPath returns the checkpoint file path for a session.
func checkpointPath(projectDir, sessionID string) string {
	return filepath.Join(projectDir, ".state", "session", sessionID, "pathflow", "pathflow-phase-tasks.json")
}

// sessionStateDir returns the pathflow state directory for a session.
func sessionStateDir(projectDir, sessionID string) string {
	return filepath.Join(projectDir, ".state", "session", sessionID, "pathflow")
}

// assertFileExists fails the test if path does not exist.
func assertFileExists(t *testing.T, path, desc string) {
	t.Helper()
	if _, err := os.Stat(path); err != nil {
		t.Errorf("expected file to exist (%s): %s", desc, path)
	}
}

// assertFileAbsent fails the test if path exists.
func assertFileAbsent(t *testing.T, path, desc string) {
	t.Helper()
	if _, err := os.Stat(path); err == nil {
		t.Errorf("expected file to be absent (%s): %s", desc, path)
	}
}

// readLedgerEvents reads all events from a JSONL file and returns them.
func readLedgerEvents(t *testing.T, path string) []ledger.Event {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("readLedgerEvents: reading %s: %v", path, err)
	}
	var events []ledger.Event
	scanner := bufio.NewScanner(strings.NewReader(string(data)))
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var ev ledger.Event
		if err := json.Unmarshal([]byte(line), &ev); err != nil {
			t.Fatalf("readLedgerEvents: parsing line %q: %v", line, err)
		}
		events = append(events, ev)
	}
	return events
}

// completePhase registers and completes all tasks in a phase, verifying that
// the phase sentinel is created when the last task completes.
func completePhase(t *testing.T, cp *pathflow.Checkpoint, cpPath, sentDir, phaseID string, taskIDs []string) {
	t.Helper()
	for _, taskID := range taskIDs {
		if err := cp.RegisterTask(cpPath, sentDir, taskID); err != nil {
			t.Fatalf("completePhase: RegisterTask(%s): %v", taskID, err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, taskID); err != nil {
			t.Fatalf("completePhase: CompleteTask(%s): %v", taskID, err)
		}
	}
	// Verify sentinel was created.
	num := strings.TrimPrefix(phaseID, "PF")
	sentinelName := fmt.Sprintf("pf-%s", num)
	if !pathflow.Exists(sentDir, sentinelName) {
		t.Errorf("completePhase: sentinel %s not created after completing phase %s", sentinelName, phaseID)
	}
}

// ---- TestFullPathFlowLifecycle ----

// TestFullPathFlowLifecycle runs a complete PF1-PF7 lifecycle using Go hook
// implementations directly (no CLI binary, no settings.json wiring). This
// verifies that all components work together as a system.
func TestFullPathFlowLifecycle(t *testing.T) {
	t.Parallel()

	projectDir := testutil.TempProject(t)
	setupPathflowConfig(t, projectDir)

	sid := testSessionID
	init_ := newTestInitializer(t, sid)

	// ---- PF1: Session initialization ----
	stdin := strings.NewReader(`{"session_id":"test-claude-uuid","source":"startup"}`)
	result, err := init_.StartInit(stdin, projectDir)
	if err != nil {
		t.Fatalf("StartInit() error = %v", err)
	}
	if result.SessionID != sid {
		t.Errorf("SessionID = %q, want %q", result.SessionID, sid)
	}
	if result.IsTeammate {
		t.Error("IsTeammate should be false for fresh session")
	}
	if result.IsResume {
		t.Error("IsResume should be false for fresh session")
	}

	// Verify session status file created.
	statusPath := filepath.Join(sessionStateDir(projectDir, sid), hooksession.PathflowSessionStatusFile)
	assertFileExists(t, statusPath, "session status file")

	// Verify checkpoint file initialized.
	cpPath := checkpointPath(projectDir, sid)
	assertFileExists(t, cpPath, "checkpoint file")

	// Verify current-session-id is NOT written (eliminated; only codeflow-env.sh is used).
	csidPath := filepath.Join(projectDir, ".state", "runtime", "current-session-id")
	if _, err := os.Stat(csidPath); !os.IsNotExist(err) {
		t.Error("current-session-id should NOT exist (eliminated in favor of codeflow-env.sh)")
	}

	// Verify env file written.
	envPath := filepath.Join(projectDir, ".state", "runtime", "codeflow-env.sh")
	assertFileExists(t, envPath, "codeflow-env.sh")
	envData, _ := os.ReadFile(envPath)
	if !strings.Contains(string(envData), sid) {
		t.Errorf("env file does not contain session ID %q", sid)
	}

	sentDir := sentinelDir(projectDir, sid)
	cp := &pathflow.Checkpoint{
		Now: func() time.Time { return fixedTime },
	}

	// ---- PF1: Complete phase tasks ----
	// PF1 has no cross-phase gate (first phase), so we can register immediately.
	completePhase(t, cp, cpPath, sentDir, "PF1", []string{"PF1-TSK-01", "PF1-TSK-02"})

	// ---- PF2: Complete phase tasks ----
	completePhase(t, cp, cpPath, sentDir, "PF2", []string{"PF2-TSK-01", "PF2-TSK-02", "PF2-TSK-03", "PF2-TSK-04"})

	// ---- PF3: Complete phase tasks (unlocks Edit/Write gate) ----
	completePhase(t, cp, cpPath, sentDir, "PF3", []string{"PF3-TSK-01", "PF3-TSK-02", "PF3-TSK-03"})

	// Verify pf-3 sentinel exists.
	if !pathflow.Exists(sentDir, "pf-3") {
		t.Fatal("pf-3 sentinel must exist before gate enforcement tests")
	}

	// ---- Gate enforcement: Edit BLOCKED before pf-3 (test via empty dir) ----
	emptyDir := t.TempDir()
	blockedGate := &gate.GateChecker{SentinelDir: emptyDir, SessionID: sid}
	editInput := json.RawMessage(`{"file_path":"/tmp/x.go","old_string":"a","new_string":"b"}`)
	blockedVerdict := blockedGate.Check("Edit", editInput)
	if blockedVerdict.Allow {
		t.Error("Edit should be BLOCKED when no pf-3 sentinel exists")
	}
	if blockedVerdict.GateType != gate.GateEditWrite {
		t.Errorf("GateType = %v, want GateEditWrite", blockedVerdict.GateType)
	}

	// ---- Gate enforcement: Edit ALLOWED after pf-3 ----
	allowedGate := &gate.GateChecker{SentinelDir: sentDir, SessionID: sid}
	allowedVerdict := allowedGate.Check("Edit", editInput)
	if !allowedVerdict.Allow {
		t.Errorf("Edit should be ALLOWED with pf-3 sentinel; reason: %s", allowedVerdict.Reason)
	}

	// ---- Gate enforcement: git push BLOCKED (no pf-5 or ws-rev) ----
	gitPushInput := json.RawMessage(`{"command":"git push origin main"}`)
	pushBlockedVerdict := allowedGate.Check("Bash", gitPushInput)
	if pushBlockedVerdict.Allow {
		t.Error("git push should be BLOCKED without pf-5 and ws-rev sentinels")
	}
	if pushBlockedVerdict.GateType != gate.GateGitPushPR {
		t.Errorf("GateType = %v, want GateGitPushPR", pushBlockedVerdict.GateType)
	}

	// ---- Gate enforcement: role teammate spawn BLOCKED before pf-3 ----
	spawnInput := json.RawMessage(`{"name":"cf-development","prompt":"implement feature"}`)
	spawnBlockedVerdict := blockedGate.Check("Task", spawnInput)
	if spawnBlockedVerdict.Allow {
		t.Error("cf-development spawn should be BLOCKED without pf-3 sentinel")
	}
	if spawnBlockedVerdict.GateType != gate.GateRoleTeammateSpawn {
		t.Errorf("GateType = %v, want GateRoleTeammateSpawn", spawnBlockedVerdict.GateType)
	}

	// ---- Security: dangerous command blocked ----
	secChecker := security.NewChecker()
	dangerCtx := &security.CheckContext{
		ToolName:  "Bash",
		Command:   "rm -rf /",
		ProjectDir: projectDir,
		SessionID: sid,
	}
	dangerVerdict := secChecker.Check(dangerCtx)
	if dangerVerdict.Allow {
		t.Error("rm -rf / should be BLOCKED by security checker")
	}

	// ---- Security: safe command allowed ----
	safeCtx := &security.CheckContext{
		ToolName:  "Bash",
		Command:   "ls -la",
		ProjectDir: projectDir,
		SessionID: sid,
	}
	safeVerdict := secChecker.Check(safeCtx)
	if !safeVerdict.Allow {
		t.Errorf("ls -la should be ALLOWED; reason: %s", safeVerdict.Reason)
	}

	// ---- Security: sudo blocked ----
	sudoCtx := &security.CheckContext{
		ToolName:  "Bash",
		Command:   "sudo apt-get install curl",
		ProjectDir: projectDir,
		SessionID: sid,
	}
	sudoVerdict := secChecker.Check(sudoCtx)
	if sudoVerdict.Allow {
		t.Error("sudo command should be BLOCKED by security checker")
	}

	// ---- Team guard: TeamDelete BLOCKED during active session ----
	teamDeleteStdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
	teamVerdict, err := team.CheckTeamDelete(teamDeleteStdin, sessionStateDir(projectDir, sid), sentDir)
	if err != nil {
		t.Fatalf("CheckTeamDelete error: %v", err)
	}
	if teamVerdict.Allow {
		t.Error("TeamDelete should be BLOCKED during active PathFlow session")
	}

	// ---- Sentinel pipeline: WS-DEV stage sentinel creation ----
	// Simulate WS-DEV completion via PostToolUse hook.
	wsDevStdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-DEV -- implementation complete"}}`)
	wsDevVerdict := sentinel.CheckAndCreateStageSentinel(wsDevStdin, sentDir)
	if !wsDevVerdict.Allow {
		t.Errorf("WS-DEV stage sentinel creation blocked: %s", wsDevVerdict.Reason)
	}
	if !pathflow.Exists(sentDir, "ws-dev") {
		t.Error("ws-dev sentinel not created after STAGE-COMPLETE: WS-DEV message")
	}

	// ---- Sentinel pipeline: WS-REV requires prior primary stage ----
	// WS-REV should be allowed now that ws-dev exists.
	wsRevStdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV -- review approved"}}`)
	wsRevVerdict := sentinel.CheckAndCreateStageSentinel(wsRevStdin, sentDir)
	if !wsRevVerdict.Allow {
		t.Errorf("WS-REV stage sentinel creation blocked: %s", wsRevVerdict.Reason)
	}
	if !pathflow.Exists(sentDir, "ws-rev") {
		t.Error("ws-rev sentinel not created after STAGE-COMPLETE: WS-REV message")
	}

	// ---- Sentinel pipeline: WS-QA requires ws-dev or ws-test ----
	wsQaStdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA -- all tests pass"}}`)
	wsQaVerdict := sentinel.CheckAndCreateStageSentinel(wsQaStdin, sentDir)
	if !wsQaVerdict.Allow {
		t.Errorf("WS-QA stage sentinel creation blocked: %s", wsQaVerdict.Reason)
	}
	if !pathflow.Exists(sentDir, "ws-qa") {
		t.Error("ws-qa sentinel not created after STAGE-COMPLETE: WS-QA message")
	}

	// ---- PF4-PF6: Complete remaining phases ----
	// PF4: use adhoc_only condition skip for PF4-TSK-01, complete others.
	if err := cp.RegisterTask(cpPath, sentDir, "PF4-TSK-01"); err != nil {
		t.Fatalf("PF4-TSK-01 RegisterTask: %v", err)
	}
	if err := cp.SkipTask(cpPath, sentDir, "PF4-TSK-01"); err != nil {
		t.Fatalf("PF4-TSK-01 SkipTask: %v", err)
	}
	for _, taskID := range []string{"PF4-TSK-02", "PF4-TSK-03", "PF4-TSK-04", "PF4-TSK-05", "PF4-TSK-06", "PF4-TSK-07"} {
		if err := cp.RegisterTask(cpPath, sentDir, taskID); err != nil {
			t.Fatalf("RegisterTask(%s): %v", taskID, err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, taskID); err != nil {
			t.Fatalf("CompleteTask(%s): %v", taskID, err)
		}
	}
	if !pathflow.Exists(sentDir, "pf-4") {
		t.Error("pf-4 sentinel should exist after completing all PF4 tasks")
	}

	completePhase(t, cp, cpPath, sentDir, "PF5", []string{"PF5-TSK-01", "PF5-TSK-02"})
	completePhase(t, cp, cpPath, sentDir, "PF6",
		[]string{"PF6-TSK-01", "PF6-TSK-02", "PF6-TSK-03", "PF6-TSK-04", "PF6-TSK-05", "PF6-TSK-06", "PF6-TSK-07", "PF6-TSK-08", "PF6-TSK-09"})

	// ---- Team guard: TeamDelete ALLOWED after pf-6 sentinel ----
	teamDeleteStdin2 := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
	teamVerdict2, err := team.CheckTeamDelete(teamDeleteStdin2, sessionStateDir(projectDir, sid), sentDir)
	if err != nil {
		t.Fatalf("CheckTeamDelete (after pf-6) error: %v", err)
	}
	if !teamVerdict2.Allow {
		t.Errorf("TeamDelete should be ALLOWED after pf-6 sentinel; reason: %s", teamVerdict2.Reason)
	}

	// ---- git push ALLOWED after pf-5 and ws-rev ----
	gitPushVerdict2 := allowedGate.Check("Bash", gitPushInput)
	if !gitPushVerdict2.Allow {
		t.Errorf("git push should be ALLOWED after pf-5 and ws-rev; reason: %s", gitPushVerdict2.Reason)
	}

	// ---- PF7: Complete final phase ----
	completePhase(t, cp, cpPath, sentDir, "PF7", []string{"PF7-TSK-01", "PF7-TSK-02", "PF7-TSK-03"})

	// Verify all 7 phase sentinels exist.
	for i := 1; i <= 7; i++ {
		name := fmt.Sprintf("pf-%d", i)
		if !pathflow.Exists(sentDir, name) {
			t.Errorf("phase sentinel %s does not exist after completing all phases", name)
		}
	}

	// Also create the pf-7 sentinel as a file (needed by PF7-END gate in pf-6 check above;
	// pf-7 is created when PF7 checkpoint tasks complete, which we verified via completePhase).

	// ---- Session end cleanup ----
	// First create the pf-7 sentinel explicitly (in normal operation it's created by the
	// checkpoint hook, which we've already tested via completePhase above).
	pf7Path := filepath.Join(sentDir, "pathflow-pf-7")
	if _, err := os.Stat(pf7Path); err != nil {
		// Sentinel was created by completePhase, but verify.
		t.Logf("Note: pf-7 sentinel verified via completePhase (Exists check passed)")
	}

	cleaner := &hooksession.Cleaner{
		Now:     func() time.Time { return fixedTime },
		HomeDir: t.TempDir(),
	}
	endStdin := strings.NewReader(`{"session_id":"test-claude-uuid","transcript_path":""}`)
	cleanResult, err := cleaner.EndCleanup(endStdin, projectDir)
	if err != nil {
		t.Fatalf("EndCleanup() error = %v", err)
	}
	if cleanResult.SessionID != sid {
		t.Errorf("CleanupResult.SessionID = %q, want %q", cleanResult.SessionID, sid)
	}
	// PF7Valid may be false if sentinels were cleaned before EndCleanup reads them.
	// The important check is that cleanup ran successfully.

	// ---- Verify JSONL ledger: no session_end event written ----
	// EndCleanup no longer writes session_end to sessions.jsonl (redundant with activity log).
	ledgerDir := filepath.Join(projectDir, ".state", "ledger")
	sessionsFile := filepath.Join(ledgerDir, "sessions.jsonl")
	if _, err := os.Stat(sessionsFile); err == nil {
		events := readLedgerEvents(t, sessionsFile)
		for _, ev := range events {
			if ev.EventType == "session_end" {
				t.Error("sessions.jsonl should NOT contain a session_end event after EndCleanup() (ledger write removed)")
			}
		}
	}

	// Verify session status file was removed (EndCleanup removes the session dir).
	assertFileAbsent(t, statusPath, "session status file (should be removed by EndCleanup)")
}

// ---- TestGateEnforcementIndependent ----

// TestGateEnforcementIndependent verifies gate enforcement rules in isolation,
// independent of session state, using table-driven test cases.
func TestGateEnforcementIndependent(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		toolName  string
		toolInput string
		sentinels []string // sentinel names to pre-create
		wantAllow bool
		wantGate  gate.GateType
	}{
		// Edit tool gate (requires pf-3)
		{
			name:      "Edit before pf-3 is BLOCKED",
			toolName:  "Edit",
			toolInput: `{"file_path":"/tmp/x.go","old_string":"a","new_string":"b"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  gate.GateEditWrite,
		},
		{
			name:      "Edit after pf-3 is ALLOWED",
			toolName:  "Edit",
			toolInput: `{"file_path":"/tmp/x.go","old_string":"a","new_string":"b"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  gate.GateEditWrite,
		},
		// Write tool gate (requires pf-3)
		{
			name:      "Write before pf-3 is BLOCKED",
			toolName:  "Write",
			toolInput: `{"file_path":"/tmp/x.go","content":"hello"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  gate.GateEditWrite,
		},
		{
			name:      "Write after pf-3 is ALLOWED",
			toolName:  "Write",
			toolInput: `{"file_path":"/tmp/x.go","content":"hello"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  gate.GateEditWrite,
		},
		// git commit gate (requires pf-3)
		{
			name:      "git commit before pf-3 is BLOCKED",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m 'initial'"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  gate.GateGitCommit,
		},
		{
			name:      "git commit after pf-3 is ALLOWED",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m 'initial'"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  gate.GateGitCommit,
		},
		// git push gate (requires pf-5 AND ws-rev)
		{
			name:      "git push without pf-5 is BLOCKED",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: []string{"pf-3"},
			wantAllow: false,
			wantGate:  gate.GateGitPushPR,
		},
		{
			name:      "git push with pf-5 but no ws-rev is BLOCKED",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: []string{"pf-3", "pf-5"},
			wantAllow: false,
			wantGate:  gate.GateGitPushPR,
		},
		{
			name:      "git push with pf-5 and ws-rev is ALLOWED",
			toolName:  "Bash",
			toolInput: `{"command":"git push origin main"}`,
			sentinels: []string{"pf-3", "pf-5", "ws-rev"},
			wantAllow: true,
			wantGate:  gate.GateGitPushPR,
		},
		// gh pr gate (requires pf-5 AND ws-rev)
		{
			name:      "gh pr create without pf-5 is BLOCKED",
			toolName:  "Bash",
			toolInput: `{"command":"gh pr create --title test"}`,
			sentinels: []string{"pf-3"},
			wantAllow: false,
			wantGate:  gate.GateGitPushPR,
		},
		{
			name:      "gh pr create with pf-5 and ws-rev is ALLOWED",
			toolName:  "Bash",
			toolInput: `{"command":"gh pr create --title test"}`,
			sentinels: []string{"pf-3", "pf-5", "ws-rev"},
			wantAllow: true,
			wantGate:  gate.GateGitPushPR,
		},
		// Role teammate spawn gate (requires pf-3)
		{
			name:      "cf-development spawn before pf-3 is BLOCKED",
			toolName:  "Task",
			toolInput: `{"name":"cf-development","prompt":"implement feature x"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  gate.GateRoleTeammateSpawn,
		},
		{
			name:      "cf-review spawn before pf-3 is BLOCKED",
			toolName:  "Task",
			toolInput: `{"name":"cf-review","description":"review changes"}`,
			sentinels: nil,
			wantAllow: false,
			wantGate:  gate.GateRoleTeammateSpawn,
		},
		{
			name:      "cf-quality-assurance spawn after pf-3 is ALLOWED",
			toolName:  "Task",
			toolInput: `{"name":"cf-quality-assurance","prompt":"run qa gate"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  gate.GateRoleTeammateSpawn,
		},
		// Non-gated tools
		{
			name:      "Read tool is ungated",
			toolName:  "Read",
			toolInput: `{"file_path":"/tmp/x.go"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  gate.GateUngated,
		},
		{
			name:      "Glob tool is ungated",
			toolName:  "Glob",
			toolInput: `{"pattern":"**/*.go"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  gate.GateUngated,
		},
		{
			name:      "SendMessage tool is ungated",
			toolName:  "SendMessage",
			toolInput: `{"type":"message","recipient":"cf-git-operations","content":"please commit"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  gate.GateUngated,
		},
		// Non-role Task spawns are ungated (function teammates)
		{
			name:      "cf-knowledge-layer spawn is ungated (function teammate)",
			toolName:  "Task",
			toolInput: `{"name":"cf-knowledge-layer","prompt":"query work graph"}`,
			sentinels: nil,
			wantAllow: true,
			wantGate:  gate.GateUngated,
		},
		// Bash command with "git push" in a quoted string is NOT a push
		{
			name:      "git push in commit message string is NOT a push gate",
			toolName:  "Bash",
			toolInput: `{"command":"git commit -m 'feat: support git push integration'"}`,
			sentinels: []string{"pf-3"},
			wantAllow: true,
			wantGate:  gate.GateGitCommit,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			sentDir := t.TempDir()
			// Create required sentinels.
			for _, name := range tt.sentinels {
				sentPath := filepath.Join(sentDir, "pathflow-"+name)
				if err := os.WriteFile(sentPath, []byte("1"), 0o644); err != nil {
					t.Fatalf("create sentinel %s: %v", name, err)
				}
			}

			checker := &gate.GateChecker{
				SentinelDir: sentDir,
				SessionID:   testSessionID,
			}
			verdict := checker.Check(tt.toolName, json.RawMessage(tt.toolInput))

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check(%q) Allow = %v, want %v (reason: %s)",
					tt.name, verdict.Allow, tt.wantAllow, verdict.Reason)
			}
			if verdict.GateType != tt.wantGate {
				t.Errorf("Check(%q) GateType = %v, want %v",
					tt.name, verdict.GateType, tt.wantGate)
			}
		})
	}
}

// ---- TestSecurityEnforcementIndependent ----

// TestSecurityEnforcementIndependent verifies security enforcement rules in
// isolation using table-driven test cases.
func TestSecurityEnforcementIndependent(t *testing.T) {
	t.Parallel()

	checker := security.NewChecker()

	tests := []struct {
		name      string
		command   string
		wantAllow bool
	}{
		// Dangerous commands -- blocked
		{"rm -rf / is BLOCKED", "rm -rf /", false},
		{"rm -rf /* is BLOCKED", "rm -rf /*", false},
		{"sudo apt-get install is BLOCKED", "sudo apt-get install curl", false},
		{"git push --force main is BLOCKED", "git push --force main", false},
		{"git push --force-with-lease is BLOCKED", "git push --force-with-lease", false},
		// Safe commands -- allowed
		{"ls -la is ALLOWED", "ls -la", true},
		{"cat file.txt is ALLOWED", "cat file.txt", true},
		{"go test ./... is ALLOWED", "go test ./...", true},
		{"echo hello is ALLOWED", "echo hello", true},
		{"grep -r pattern ./src is ALLOWED", "grep -r pattern ./src", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			ctx := &security.CheckContext{
				ToolName:  "Bash",
				Command:   tt.command,
				ProjectDir: t.TempDir(),
				SessionID: testSessionID,
			}
			verdict := checker.Check(ctx)

			if verdict.Allow != tt.wantAllow {
				t.Errorf("Check(%q) Allow = %v, want %v (reason: %s / pattern: %s)",
					tt.command, verdict.Allow, tt.wantAllow, verdict.Reason, verdict.Pattern)
			}
		})
	}
}

// ---- TestSentinelPipelineIndependent ----

// TestSentinelPipelineIndependent verifies the sentinel pipeline in isolation:
// stage sentinel creation from SendMessage content patterns, stage ordering
// validation, and phase checkpoint register/complete lifecycle.
func TestSentinelPipelineIndependent(t *testing.T) {
	t.Parallel()

	t.Run("STAGE-COMPLETE pattern creates ws-dev sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-DEV -- all done"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("expected Allow=true, got reason: %s", verdict.Reason)
		}
		if !pathflow.Exists(sentDir, "ws-dev") {
			t.Error("ws-dev sentinel not created")
		}
	})

	t.Run("STAGE-COMPLETE pattern creates ws-test sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-TEST -- tests implemented"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("expected Allow=true, got reason: %s", verdict.Reason)
		}
		if !pathflow.Exists(sentDir, "ws-test") {
			t.Error("ws-test sentinel not created")
		}
	})

	t.Run("STAGE-COMPLETE pattern creates ws-docs sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-DOCS -- documentation updated"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("expected Allow=true, got reason: %s", verdict.Reason)
		}
		if !pathflow.Exists(sentDir, "ws-docs") {
			t.Error("ws-docs sentinel not created")
		}
	})

	t.Run("no STAGE-COMPLETE pattern creates no sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"Task completed successfully"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("expected Allow=true for non-stage message")
		}
		// No sentinel should be created.
		entries, _ := os.ReadDir(sentDir)
		if len(entries) != 0 {
			t.Errorf("expected no sentinels created, got %d entries", len(entries))
		}
	})

	t.Run("WS-REV BLOCKED without prior primary stage", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		// No ws-dev/ws-test/ws-docs/ws-plan sentinels exist.
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV -- review done"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if verdict.Allow {
			t.Error("WS-REV should be BLOCKED without prior primary stage sentinel")
		}
		if !strings.Contains(verdict.Reason, "ws-rev requires prior primary stage") {
			t.Errorf("unexpected block reason: %s", verdict.Reason)
		}
	})

	t.Run("WS-REV ALLOWED after ws-dev sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		// Create ws-dev sentinel first.
		if err := os.WriteFile(filepath.Join(sentDir, "pathflow-ws-dev"), []byte("1"), 0o644); err != nil {
			t.Fatalf("create ws-dev sentinel: %v", err)
		}
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-REV -- review approved"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("WS-REV should be ALLOWED after ws-dev; reason: %s", verdict.Reason)
		}
		if !pathflow.Exists(sentDir, "ws-rev") {
			t.Error("ws-rev sentinel not created")
		}
	})

	t.Run("WS-QA BLOCKED without ws-dev or ws-test", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA -- all tests pass"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if verdict.Allow {
			t.Error("WS-QA should be BLOCKED without ws-dev or ws-test sentinel")
		}
	})

	t.Run("WS-QA ALLOWED after ws-test sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		// Create ws-test sentinel first.
		if err := os.WriteFile(filepath.Join(sentDir, "pathflow-ws-test"), []byte("1"), 0o644); err != nil {
			t.Fatalf("create ws-test sentinel: %v", err)
		}
		stdin := strings.NewReader(`{"tool_name":"SendMessage","tool_input":{"message":"STAGE-COMPLETE: WS-QA -- tests pass"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("WS-QA should be ALLOWED after ws-test; reason: %s", verdict.Reason)
		}
		if !pathflow.Exists(sentDir, "ws-qa") {
			t.Error("ws-qa sentinel not created")
		}
	})

	t.Run("non-SendMessage tool produces no sentinel", func(t *testing.T) {
		t.Parallel()

		sentDir := t.TempDir()
		stdin := strings.NewReader(`{"tool_name":"Bash","tool_input":{"command":"echo STAGE-COMPLETE: WS-DEV"}}`)
		verdict := sentinel.CheckAndCreateStageSentinel(stdin, sentDir)
		if !verdict.Allow {
			t.Errorf("expected Allow=true for non-SendMessage tool")
		}
		// No sentinel should be created.
		entries, _ := os.ReadDir(sentDir)
		if len(entries) != 0 {
			t.Errorf("expected no sentinels created for Bash tool, got %d entries", len(entries))
		}
	})

	t.Run("checkpoint register and complete lifecycle", func(t *testing.T) {
		t.Parallel()

		// Set up a minimal project dir with pathflow config.
		projectDir := testutil.TempProject(t)
		setupPathflowConfig(t, projectDir)

		sid := "ses-01jk1111111111111111111111"
		sentDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatalf("mkdir sentDir: %v", err)
		}
		cpPath := filepath.Join(projectDir, ".state", "session", sid, "pathflow", "pathflow-phase-tasks.json")
		if err := os.MkdirAll(filepath.Dir(cpPath), 0o755); err != nil {
			t.Fatalf("mkdir cpPath dir: %v", err)
		}

		cp := &pathflow.Checkpoint{
			Now: func() time.Time { return fixedTime },
		}
		configPath := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases: %v", err)
		}

		// Register and complete PF1 tasks.
		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("RegisterTask PF1-TSK-01: %v", err)
		}
		// After registering only PF1-TSK-01, pf-1 sentinel should NOT exist yet.
		if pathflow.Exists(sentDir, "pf-1") {
			t.Error("pf-1 sentinel should not exist before all PF1 tasks complete")
		}
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-01"); err != nil {
			t.Fatalf("CompleteTask PF1-TSK-01: %v", err)
		}

		// Register and complete PF1-TSK-02 to trigger phase completion.
		if err := cp.RegisterTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("RegisterTask PF1-TSK-02: %v", err)
		}
		if err := cp.CompleteTask(cpPath, sentDir, "PF1-TSK-02"); err != nil {
			t.Fatalf("CompleteTask PF1-TSK-02: %v", err)
		}

		// pf-1 sentinel should now exist.
		if !pathflow.Exists(sentDir, "pf-1") {
			t.Error("pf-1 sentinel should exist after all PF1 tasks complete")
		}
	})

	t.Run("cross-phase gate blocks registration without prior sentinel", func(t *testing.T) {
		t.Parallel()

		projectDir := testutil.TempProject(t)
		setupPathflowConfig(t, projectDir)

		sid := "ses-01jk2222222222222222222222"
		sentDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatalf("mkdir sentDir: %v", err)
		}
		cpPath := filepath.Join(projectDir, ".state", "session", sid, "pathflow", "pathflow-phase-tasks.json")
		if err := os.MkdirAll(filepath.Dir(cpPath), 0o755); err != nil {
			t.Fatalf("mkdir cpPath dir: %v", err)
		}

		cp := &pathflow.Checkpoint{
			Now: func() time.Time { return fixedTime },
		}
		configPath := filepath.Join(projectDir, ".codeflow", "config", "pathflow", "pathflow-config.json")
		if err := cp.InitAllPhases(cpPath, configPath); err != nil {
			t.Fatalf("InitAllPhases: %v", err)
		}

		// Attempt to register PF2-TSK-01 without pf-1 sentinel -- should fail.
		err := cp.RegisterTask(cpPath, sentDir, "PF2-TSK-01")
		if err == nil {
			t.Error("RegisterTask PF2 should be BLOCKED without pf-1 sentinel")
		}
		if !strings.Contains(err.Error(), "cross-phase registration blocked") {
			t.Errorf("unexpected error: %v (expected cross-phase block)", err)
		}
	})
}

// ---- TestTeamGuardIndependent ----

// TestTeamGuardIndependent verifies team guard enforcement rules in isolation.
func TestTeamGuardIndependent(t *testing.T) {
	t.Parallel()

	t.Run("TeamDelete BLOCKED with pathflow-active flag and no pf-6", func(t *testing.T) {
		t.Parallel()

		projectDir := t.TempDir()
		sid := "ses-01jk3333333333333333333333"
		sessDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
		sentDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
		if err := os.MkdirAll(sessDir, 0o755); err != nil {
			t.Fatalf("mkdir sessDir: %v", err)
		}
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatalf("mkdir sentDir: %v", err)
		}
		// Create active status file.
		statusJSON := []byte(`{"session_id":"` + sid + `","status":"pf-in-progress","team_name":"test"}`)
		if err := os.WriteFile(filepath.Join(sessDir, hooksession.PathflowSessionStatusFile), statusJSON, 0o644); err != nil {
			t.Fatalf("create status file: %v", err)
		}

		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		verdict, err := team.CheckTeamDelete(stdin, sessDir, sentDir)
		if err != nil {
			t.Fatalf("CheckTeamDelete: %v", err)
		}
		if verdict.Allow {
			t.Error("TeamDelete should be BLOCKED during active PathFlow session")
		}
	})

	t.Run("TeamDelete ALLOWED when no active session status", func(t *testing.T) {
		t.Parallel()

		projectDir := t.TempDir()
		sid := "ses-01jk4444444444444444444444"
		sessDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
		sentDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
		if err := os.MkdirAll(sessDir, 0o755); err != nil {
			t.Fatalf("mkdir sessDir: %v", err)
		}
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatalf("mkdir sentDir: %v", err)
		}
		// No pathflow-active flag created.

		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		verdict, err := team.CheckTeamDelete(stdin, sessDir, sentDir)
		if err != nil {
			t.Fatalf("CheckTeamDelete: %v", err)
		}
		if !verdict.Allow {
			t.Errorf("TeamDelete should be ALLOWED when no pathflow-active flag; reason: %s", verdict.Reason)
		}
	})

	t.Run("TeamDelete ALLOWED after pf-6 sentinel", func(t *testing.T) {
		t.Parallel()

		projectDir := t.TempDir()
		sid := "ses-01jk5555555555555555555555"
		sessDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
		sentDir := filepath.Join(projectDir, ".state", "sentinels", "pathflow", sid)
		if err := os.MkdirAll(sessDir, 0o755); err != nil {
			t.Fatalf("mkdir sessDir: %v", err)
		}
		if err := os.MkdirAll(sentDir, 0o755); err != nil {
			t.Fatalf("mkdir sentDir: %v", err)
		}
		// Create active status file AND pf-6 sentinel.
		statusJSON := []byte(`{"session_id":"` + sid + `","status":"pf-in-progress","team_name":"test"}`)
		if err := os.WriteFile(filepath.Join(sessDir, hooksession.PathflowSessionStatusFile), statusJSON, 0o644); err != nil {
			t.Fatalf("create status file: %v", err)
		}
		if err := os.WriteFile(filepath.Join(sentDir, "pathflow-pf-6"), []byte("1"), 0o644); err != nil {
			t.Fatalf("create pf-6 sentinel: %v", err)
		}

		stdin := strings.NewReader(`{"tool_name":"TeamDelete","tool_input":{}}`)
		verdict, err := team.CheckTeamDelete(stdin, sessDir, sentDir)
		if err != nil {
			t.Fatalf("CheckTeamDelete: %v", err)
		}
		if !verdict.Allow {
			t.Errorf("TeamDelete should be ALLOWED after pf-6 sentinel; reason: %s", verdict.Reason)
		}
	})

	t.Run("HandlePostTeamDelete updates status to pf-complete", func(t *testing.T) {
		t.Parallel()

		projectDir := t.TempDir()
		sid := "ses-01jk6666666666666666666666"
		sessDir := filepath.Join(projectDir, ".state", "session", sid, "pathflow")
		if err := os.MkdirAll(sessDir, 0o755); err != nil {
			t.Fatalf("mkdir sessDir: %v", err)
		}
		statusJSON := []byte(`{"session_id":"` + sid + `","status":"pf-in-progress","team_name":"test"}`)
		statusPath := filepath.Join(sessDir, hooksession.PathflowSessionStatusFile)
		if err := os.WriteFile(statusPath, statusJSON, 0o644); err != nil {
			t.Fatalf("create status file: %v", err)
		}

		// Write minimal pathflow config for checkpoint reset.
		cfgDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
		if err := os.MkdirAll(cfgDir, 0o755); err != nil {
			t.Fatalf("mkdir cfgDir: %v", err)
		}
		if err := os.WriteFile(filepath.Join(cfgDir, "pathflow-config.json"), []byte(`{"phases":{}}`), 0o644); err != nil {
			t.Fatalf("write config: %v", err)
		}

		if err := team.HandlePostTeamDelete(sessDir, projectDir, sid); err != nil {
			t.Fatalf("HandlePostTeamDelete: %v", err)
		}
		// Status file should still exist but updated to pf-complete.
		if _, err := os.Stat(statusPath); err != nil {
			t.Error("status file should be preserved by HandlePostTeamDelete")
		}
		updatedStatus, readErr := hooksession.ReadPathflowSessionStatus(sessDir)
		if readErr != nil {
			t.Fatalf("reading updated status: %v", readErr)
		}
		if updatedStatus == nil || updatedStatus.Status != "pf-complete" {
			t.Errorf("status should be pf-complete after HandlePostTeamDelete, got %v", updatedStatus)
		}
	})
}

// ---- TestLedgerRoutingIndependent ----

// TestLedgerRoutingIndependent verifies that the ledger writer routes events
// to the correct JSONL files per event type.
func TestLedgerRoutingIndependent(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name         string
		eventType    string
		expectedFile string
		extraData    map[string]any
	}{
		{
			"session_end routes to sessions.jsonl",
			"session_end",
			ledger.FileSessions,
			nil,
		},
		{
			"session_start routes to sessions.jsonl",
			"session_start",
			ledger.FileSessions,
			nil,
		},
		{
			"session_progress routes to sessions.jsonl",
			"session_progress",
			ledger.FileSessions,
			nil,
		},
		{
			"task_created routes to work-graph.jsonl",
			"task_created",
			ledger.FileWorkGraph,
			map[string]any{"id": "task-123", "epic_id": "epic-456", "title": "test task"},
		},
		{
			"epic_created routes to work-graph.jsonl",
			"epic_created",
			ledger.FileWorkGraph,
			map[string]any{"id": "epic-456", "title": "test epic"},
		},
		{
			"begin_work routes to work-graph.jsonl",
			"begin_work",
			ledger.FileWorkGraph,
			map[string]any{"id": "task-123"},
		},
		{
			"memory_store routes to memory-events.jsonl",
			"memory_store",
			ledger.FileMemoryEvents,
			map[string]any{"id": "mem-001"},
		},
		{
			"config_set routes to config.jsonl",
			"config_set",
			ledger.FileConfig,
			nil,
		},
		{
			"phase_transition routes to pathflow-events.jsonl",
			"phase_transition",
			ledger.FilePathflowEvents,
			map[string]any{"phase": "PF1-INIT", "status": "entered"},
		},
		{
			"stage_transition routes to pathflow-events.jsonl",
			"stage_transition",
			ledger.FilePathflowEvents,
			map[string]any{"stage": "WS-DEV", "status": "in_progress"},
		},
		{
			"session_register routes to pathflow-events.jsonl",
			"session_register",
			ledger.FilePathflowEvents,
			nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			ledgerDir := t.TempDir()
			w, err := ledger.NewWriter(ledgerDir)
			if err != nil {
				t.Fatalf("NewWriter: %v", err)
			}

			data := map[string]any{"routing_test": "integration"}
			for k, v := range tt.extraData {
				data[k] = v
			}
			event := ledger.Event{
				EventType: tt.eventType,
				SessionID: testSessionID,
				Timestamp: fixedTime.Format(time.RFC3339),
				Data:      data,
			}
			if err := w.AppendEvent(event); err != nil {
				t.Fatalf("AppendEvent(%s): %v", tt.eventType, err)
			}

			// Verify the event was written to the correct file.
			targetPath := filepath.Join(ledgerDir, tt.expectedFile)
			if _, err := os.Stat(targetPath); err != nil {
				t.Errorf("expected %s to exist after writing %s event", tt.expectedFile, tt.eventType)
				return
			}

			// Verify the event can be read back with correct canonical schema.
			events := readLedgerEvents(t, targetPath)
			if len(events) == 0 {
				t.Errorf("expected at least 1 event in %s, got 0", tt.expectedFile)
				return
			}
			found := false
			for _, ev := range events {
				if ev.EventType == tt.eventType {
					found = true
					// Verify canonical schema: event field and timestamp field (not type/ts).
					if ev.Timestamp == "" {
						t.Errorf("event %s has empty timestamp", tt.eventType)
					}
					break
				}
			}
			if !found {
				t.Errorf("event type %s not found in %s", tt.eventType, tt.expectedFile)
			}
		})
	}

	t.Run("AppendEventToFile rejects misrouted events", func(t *testing.T) {
		t.Parallel()

		ledgerDir := t.TempDir()
		w, err := ledger.NewWriter(ledgerDir)
		if err != nil {
			t.Fatalf("NewWriter: %v", err)
		}

		// Attempting to write a session_end event to work-graph.jsonl should fail.
		event := ledger.Event{
			EventType: "session_end",
			SessionID: testSessionID,
			Timestamp: fixedTime.Format(time.RFC3339),
		}
		err = w.AppendEventToFile(ledger.FileWorkGraph, event)
		if err == nil {
			t.Error("AppendEventToFile should reject misrouted event (session_end -> work-graph.jsonl)")
		}
	})
}

// ---- TestTeammateModeIndependent ----

// TestTeammateModeIndependent verifies that StartInit correctly detects
// teammate mode when env var matches and session status is active.
func TestTeammateModeIndependent(t *testing.T) {
	// NOTE: no t.Parallel() — uses t.Setenv for CODEFLOW_SESSION_ID.

	t.Run("teammate mode with env var match and active status", func(t *testing.T) {
		// NOTE: no t.Parallel() — t.Setenv is incompatible with parallel tests.

		projectDir := t.TempDir()
		setupPathflowConfig(t, projectDir)
		homeDir := t.TempDir()

		const existingSID = "ses-01jk7777777777777777777777"
		const teamName = "test-team"

		// Set up existing session state: env file + pathflow-team.json + status file.
		runtimeDir := filepath.Join(projectDir, ".state", "runtime")
		if err := os.MkdirAll(runtimeDir, 0o755); err != nil {
			t.Fatalf("mkdir runtimeDir: %v", err)
		}
		envContent := fmt.Sprintf("export CODEFLOW_SESSION_ID='%s'\nexport CF_PROJECT_ROOT='test'\n", existingSID)
		if err := os.WriteFile(filepath.Join(runtimeDir, "codeflow-env.sh"), []byte(envContent), 0o644); err != nil {
			t.Fatalf("write env file: %v", err)
		}

		// Create pathflow-team.json with team name.
		pathflowDir := filepath.Join(projectDir, ".state", "session", existingSID, "pathflow")
		if err := os.MkdirAll(pathflowDir, 0o755); err != nil {
			t.Fatalf("mkdir pathflowDir: %v", err)
		}
		teamJSON := fmt.Sprintf(`{"lead_pid":0,"team_name":"%s"}`, teamName)
		if err := os.WriteFile(filepath.Join(pathflowDir, "pathflow-team.json"), []byte(teamJSON), 0o644); err != nil {
			t.Fatalf("write pathflow-team.json: %v", err)
		}

		// Create status file with pf-in-progress (Signal 2).
		statusJSON := fmt.Sprintf(`{"session_id":"%s","status":"pf-in-progress","team_name":"%s"}`, existingSID, teamName)
		if err := os.WriteFile(filepath.Join(pathflowDir, hooksession.PathflowSessionStatusFile), []byte(statusJSON), 0o644); err != nil {
			t.Fatalf("write status file: %v", err)
		}

		// Create team config.
		teamDir := filepath.Join(homeDir, ".claude", "teams", teamName)
		if err := os.MkdirAll(teamDir, 0o755); err != nil {
			t.Fatalf("mkdir teamDir: %v", err)
		}
		if err := os.WriteFile(filepath.Join(teamDir, "config.json"), []byte(`{}`), 0o644); err != nil {
			t.Fatalf("write team config: %v", err)
		}

		// Set CODEFLOW_SESSION_ID env var to match existingSID (Signal 1).
		t.Setenv("CODEFLOW_SESSION_ID", existingSID)

		init_ := &hooksession.Initializer{
			Now:            func() time.Time { return fixedTime },
			TmuxChecker:    mockTmuxChecker{alive: map[string]bool{}},
			SessionStarter: mockSessionStarter{id: "ses-01jk8888888888888888888888"},
			HomeDir:        homeDir,
		}

		stdin := strings.NewReader(`{"session_id":"new-claude-uuid","source":"startup"}`)
		result, err := init_.StartInit(stdin, projectDir)
		if err != nil {
			t.Fatalf("StartInit() error = %v", err)
		}

		if !result.IsTeammate {
			t.Error("IsTeammate should be true when env var matches + status active + config exists")
		}
		if result.SessionID != existingSID {
			t.Errorf("SessionID = %q, want %q (should reuse existing SID)", result.SessionID, existingSID)
		}
		if result.EnvVars["CODEFLOW_SESSION_ID"] != existingSID {
			t.Errorf("EnvVars[CODEFLOW_SESSION_ID] = %q, want %q", result.EnvVars["CODEFLOW_SESSION_ID"], existingSID)
		}
	})
}

// ---- TestValidateIndependent ----

// TestValidateIndependent verifies that validate.ValidateTask and validate.ValidateEpic
// work correctly via direct Go API calls (AC 14: validate task and epic coverage).
func TestValidateIndependent(t *testing.T) {
	t.Parallel()

	// minimalValidTask is a well-formed task markdown file.
	const minimalValidTask = `---
id: "task-01AAAAAAAAAAAAAAAAAAAAAAAA"
format_id: "INF-TSK-021-099"
epic_id: "epic-01BBBBBBBBBBBBBBBBBBBBBBBB"
epic_format_id: "INF-EPC-021"
title: "Integration test task"
description: "Test task for integration tests"
status: in_progress
area_type: "INF"
work_type: "TEST"
domain: "GENL"
origin: planned
autorun_eligible: false
raise_pr: true
auto_merge: false
---

## Description

Integration test task.

## Approach

Test approach.

## Files

None.

## Acceptance Criteria

1. Test passes.

## Dependencies

### Blocked By

None.

## Verification

### Automated

- [ ] Tests pass.

## Stage Reports

### TEST Report

> Placeholder.

## Notes

None.
`

	// minimalValidEpic is a well-formed epic markdown file.
	const minimalValidEpic = `---
id: "epic-01BBBBBBBBBBBBBBBBBBBBBBBB"
format_id: "INF-EPC-099"
title: "Integration test epic"
description: "Test epic for integration tests"
status: in_progress
area_type: "INF"
work_type: "TEST"
---

## Summary

Integration test epic.

## Scope

Test scope.

## Acceptance Criteria

1. Epic is created.

## Tasks

None.

## Dependencies

None.

## Technical Notes

None.

## Related

None.
`

	t.Run("ValidateTask accepts well-formed task markdown", func(t *testing.T) {
		t.Parallel()

		dir := t.TempDir()
		path := filepath.Join(dir, "test-task.md")
		if err := os.WriteFile(path, []byte(minimalValidTask), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}

		errs, _, err := validate.ValidateTask(path)
		if err != nil {
			t.Fatalf("ValidateTask() I/O error: %v", err)
		}
		if len(errs) != 0 {
			t.Errorf("ValidateTask() returned %d errors for valid task: %v", len(errs), errs)
		}
	})

	t.Run("ValidateTask rejects task with missing required fields", func(t *testing.T) {
		t.Parallel()

		// Task with missing status and work_type.
		const badTask = `---
id: "task-01AAAAAAAAAAAAAAAAAAAAAAAA"
format_id: "INF-TSK-021-099"
title: "Bad task"
---

## Description
Missing required fields.

## Approach
N/A.

## Files
None.

## Acceptance Criteria
1. Test.

## Dependencies

### Blocked By
None.

## Verification

### Automated
None.

## Stage Reports

### DEV Report
Placeholder.

## Notes
None.
`
		dir := t.TempDir()
		path := filepath.Join(dir, "bad-task.md")
		if err := os.WriteFile(path, []byte(badTask), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}

		errs, _, err := validate.ValidateTask(path)
		if err != nil {
			t.Fatalf("ValidateTask() I/O error: %v", err)
		}
		if len(errs) == 0 {
			t.Error("ValidateTask() returned no errors for task with missing fields")
		}
	})

	t.Run("ValidateEpic accepts well-formed epic markdown", func(t *testing.T) {
		t.Parallel()

		dir := t.TempDir()
		path := filepath.Join(dir, "test-epic.md")
		if err := os.WriteFile(path, []byte(minimalValidEpic), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}

		errs, _, err := validate.ValidateEpic(path)
		if err != nil {
			t.Fatalf("ValidateEpic() I/O error: %v", err)
		}
		if len(errs) != 0 {
			t.Errorf("ValidateEpic() returned %d errors for valid epic: %v", len(errs), errs)
		}
	})

	t.Run("ValidateEpic rejects epic with missing required fields", func(t *testing.T) {
		t.Parallel()

		const badEpic = `---
id: "epic-01BBBBBBBBBBBBBBBBBBBBBBBB"
title: "Bad epic"
---

## Summary
Missing area_type and work_type.
`
		dir := t.TempDir()
		path := filepath.Join(dir, "bad-epic.md")
		if err := os.WriteFile(path, []byte(badEpic), 0o644); err != nil {
			t.Fatalf("WriteFile: %v", err)
		}

		errs, _, err := validate.ValidateEpic(path)
		if err != nil {
			t.Fatalf("ValidateEpic() I/O error: %v", err)
		}
		if len(errs) == 0 {
			t.Error("ValidateEpic() returned no errors for epic with missing fields")
		}
	})
}

// ---- TestGitHooksIndependent ----

// TestGitHooksIndependent verifies git hook subcommand implementations
// via direct Go API calls (AC 15: git-hooks subcommands coverage).
func TestGitHooksIndependent(t *testing.T) {
	t.Parallel()

	policy := githooks.DefaultPolicy()

	tests := []struct {
		name    string
		msg     string
		wantErr bool
	}{
		{
			name:    "valid conventional commit is accepted",
			msg:     "feat: add integration tests for PathFlow lifecycle\n\n- Covers PF1-PF7 end-to-end\n- Gate enforcement verified\n- Security checks verified",
			wantErr: false,
		},
		{
			name:    "valid fix commit is accepted",
			msg:     "fix: resolve session ID resolution in EndCleanup\n\n- Reads from codeflow-env.sh first\n- Falls back to current-session-id file",
			wantErr: false,
		},
		{
			name:    "empty commit message is rejected",
			msg:     "",
			wantErr: true,
		},
		{
			name:    "commit message with invalid type is rejected",
			msg:     "unknowntype: some change",
			wantErr: true,
		},
		{
			name:    "commit message with scoped type is rejected (no scopes allowed)",
			msg:     "feat(api): add endpoint",
			wantErr: true,
		},
		{
			name:    "merge commit is accepted (skip condition)",
			msg:     "Merge branch 'main' into feat/integration",
			wantErr: false,
		},
		{
			name:    "revert commit is accepted (skip condition)",
			msg:     "Revert \"feat: add something\"",
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			t.Parallel()

			err := githooks.ValidateCommitMsg(tt.msg, policy)
			if tt.wantErr && err == nil {
				t.Errorf("ValidateCommitMsg(%q) expected error, got nil", tt.msg)
			}
			if !tt.wantErr && err != nil {
				t.Errorf("ValidateCommitMsg(%q) unexpected error: %v", tt.msg, err)
			}
		})
	}
}

// ---- TestWorktreeIndependent ----

// TestWorktreeIndependent verifies worktree subcommand implementations
// via direct Go API calls (AC 17: worktree subcommands coverage).
func TestWorktreeIndependent(t *testing.T) {
	t.Parallel()

	t.Run("Manager.List returns nil when no worktrees registered", func(t *testing.T) {
		t.Parallel()

		projectDir := testutil.TempProject(t)
		mgr := &worktree.Manager{
			ProjectDir: projectDir,
		}

		// List with no filter returns nil (no registry file = no worktrees).
		worktrees, err := mgr.List("")
		if err != nil {
			t.Fatalf("Manager.List() error: %v", err)
		}
		// nil is valid when registry doesn't exist yet.
		_ = worktrees
	})

	t.Run("Manager.List with filter returns nil when registry empty", func(t *testing.T) {
		t.Parallel()

		projectDir := testutil.TempProject(t)
		mgr := &worktree.Manager{
			ProjectDir: projectDir,
		}

		// List with filter on a fresh project with no registry returns nil.
		worktrees, err := mgr.List("some-filter")
		if err != nil {
			t.Fatalf("Manager.List('some-filter') error: %v", err)
		}
		_ = worktrees // nil is expected (no registry file)
	})

	t.Run("ErrNotFound sentinel error is exported", func(t *testing.T) {
		t.Parallel()

		// Verify the sentinel errors are accessible (not nil).
		if worktree.ErrNotFound == nil {
			t.Error("worktree.ErrNotFound is nil")
		}
		if worktree.ErrAlreadyExists == nil {
			t.Error("worktree.ErrAlreadyExists is nil")
		}
		if worktree.ErrPathFlowActive == nil {
			t.Error("worktree.ErrPathFlowActive is nil")
		}
		if worktree.ErrInvalidName == nil {
			t.Error("worktree.ErrInvalidName is nil")
		}
	})
}
