package gate

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/codeflow/codeflow-cli/internal/hooks/session"
)

// setupGateProjectEnv creates a full project directory structure for
// config-driven gate tests. Returns sentinelDir, sessionDir, configDir.
func setupGateProjectEnv(t *testing.T) (sentinelDir, sessionDir, configDir string) {
	t.Helper()
	projectDir := t.TempDir()
	sid := "ses-gatetest"
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

func writeGateTestConfig(t *testing.T, configDir string, pipelines map[string][]string) {
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

func writeGateSessionStatus(t *testing.T, sessionDir, workType string) {
	t.Helper()
	status := &session.PathflowSessionStatus{
		SessionID: "ses-gatetest",
		Status:    "pf-in-progress",
		WorkType:  workType,
	}
	if err := session.WritePathflowSessionStatus(sessionDir, status); err != nil {
		t.Fatal(err)
	}
}

func TestCheckCumulativePushPRGate_ConfigLoadFailure(t *testing.T) {
	t.Parallel()

	// Sentinel dir with proper structure but NO config file.
	sentinelDir, sessionDir, _ := setupGateProjectEnv(t)

	// Create all phase sentinels so phase check passes.
	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	// Write session status with work_type but no config file.
	writeGateSessionStatus(t, sessionDir, "FEAT")

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	// Should allow through (graceful degradation when config can't load).
	if !v.Allow {
		t.Fatalf("expected allow on config load failure (graceful degradation); reason: %s", v.Reason)
	}
}

func TestCheckCumulativePushPRGate_EmptyWorkType(t *testing.T) {
	t.Parallel()

	sentinelDir, sessionDir, configDir := setupGateProjectEnv(t)

	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	writeGateTestConfig(t, configDir, map[string][]string{
		"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
	})

	// Write session status WITHOUT work_type.
	status := &session.PathflowSessionStatus{
		SessionID: "ses-gatetest",
		Status:    "pf-in-progress",
	}
	if err := session.WritePathflowSessionStatus(sessionDir, status); err != nil {
		t.Fatal(err)
	}

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	// Should allow through (no work_type means no pipeline check).
	if !v.Allow {
		t.Fatalf("expected allow when work_type is empty; reason: %s", v.Reason)
	}
}

func TestCheckCumulativePushPRGate_UnknownWorkType(t *testing.T) {
	t.Parallel()

	sentinelDir, sessionDir, configDir := setupGateProjectEnv(t)

	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	writeGateTestConfig(t, configDir, map[string][]string{
		"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
	})

	// Work type not in config pipelines.
	writeGateSessionStatus(t, sessionDir, "UNKNOWN_TYPE")

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	// Should allow through (unknown work_type has no pipeline to check).
	if !v.Allow {
		t.Fatalf("expected allow for unknown work_type; reason: %s", v.Reason)
	}
}

func TestCheckCumulativePushPRGate_MissingStageSentinel(t *testing.T) {
	t.Parallel()

	sentinelDir, sessionDir, configDir := setupGateProjectEnv(t)

	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	writeGateTestConfig(t, configDir, map[string][]string{
		"FEAT": {"WS-DEV", "WS-REV", "WS-QA"},
	})
	writeGateSessionStatus(t, sessionDir, "FEAT")

	// Only ws-dev exists, ws-rev and ws-qa missing.
	createSentinel(t, sentinelDir, "ws-dev")

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	if v.Allow {
		t.Fatal("expected block: ws-rev missing in pipeline")
	}
	if !strings.Contains(v.Reason, "ws-rev") {
		t.Errorf("Reason should mention ws-rev, got: %s", v.Reason)
	}
}

func TestCheckCumulativePushPRGate_AllSentinelsPresent(t *testing.T) {
	t.Parallel()

	sentinelDir, sessionDir, configDir := setupGateProjectEnv(t)

	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	writeGateTestConfig(t, configDir, map[string][]string{
		"FIX": {"WS-DEV", "WS-REV", "WS-QA"},
	})
	writeGateSessionStatus(t, sessionDir, "FIX")

	createSentinel(t, sentinelDir, "ws-dev")
	createSentinel(t, sentinelDir, "ws-rev")
	createSentinel(t, sentinelDir, "ws-qa")

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	if !v.Allow {
		t.Fatalf("expected allow with all sentinels; reason: %s", v.Reason)
	}
}

func TestCheckCumulativePushPRGate_SessionStatusUnreadable(t *testing.T) {
	t.Parallel()

	sentinelDir, sessionDir, _ := setupGateProjectEnv(t)

	for i := 1; i <= 5; i++ {
		createSentinel(t, sentinelDir, "pf-"+string(rune('0'+i)))
	}

	// Write invalid JSON to session status.
	statusPath := filepath.Join(sessionDir, "pathflow-session-status.json")
	if err := os.WriteFile(statusPath, []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	checker := &GateChecker{SentinelDir: sentinelDir, SessionID: "ses-gatetest"}
	v := checker.Check("Bash", json.RawMessage(`{"command":"git push origin main"}`))

	// Should allow through (unreadable status = empty work_type = no pipeline check).
	if !v.Allow {
		t.Fatalf("expected allow when session status is unreadable; reason: %s", v.Reason)
	}
}

func TestDeriveSessionDirFromSentinelDir(t *testing.T) {
	t.Parallel()

	sentinelDir := "/project/.state/sentinels/pathflow/ses-123"
	got := deriveSessionDirFromSentinelDir(sentinelDir)
	want := filepath.Clean("/project/.state/session/ses-123/pathflow")
	if filepath.Clean(got) != want {
		t.Errorf("deriveSessionDirFromSentinelDir() = %q, want %q", got, want)
	}
}

func TestDeriveConfigDirFromSentinelDir(t *testing.T) {
	t.Parallel()

	sentinelDir := "/project/.state/sentinels/pathflow/ses-123"
	got := deriveConfigDirFromSentinelDir(sentinelDir)
	want := filepath.Clean("/project/.codeflow/config/pathflow")
	if filepath.Clean(got) != want {
		t.Errorf("deriveConfigDirFromSentinelDir() = %q, want %q", got, want)
	}
}
