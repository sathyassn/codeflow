package doctor

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/codeflow/codeflow-cli/internal/db"
)

// fakeLookPath returns a LookPath function that succeeds for the given names.
func fakeLookPath(available ...string) func(string) (string, error) {
	set := make(map[string]bool, len(available))
	for _, name := range available {
		set[name] = true
	}
	return func(file string) (string, error) {
		if set[file] {
			return "/usr/bin/" + file, nil
		}
		return "", fmt.Errorf("%s not found", file)
	}
}

// fakeExecCommand returns an ExecCommand function with configurable behavior.
func fakeExecCommand(output string, err error) func(string, ...string) ([]byte, error) {
	return func(_ string, _ ...string) ([]byte, error) {
		return []byte(output), err
	}
}

func TestCheckNames(t *testing.T) {
	t.Parallel()
	names := CheckNames()
	if len(names) != 16 {
		t.Fatalf("expected 16 check names, got %d", len(names))
	}

	// Verify the list is independent (modifying it doesn't affect the original).
	names[0] = "modified"
	origNames := CheckNames()
	if origNames[0] == "modified" {
		t.Fatal("CheckNames returned a reference to the internal slice, not a copy")
	}
}

func TestCheckDatabase_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	// Create a real SQLite database using the db package.
	dbDir := t.TempDir()
	dbPath := filepath.Join(dbDir, "test.db")

	// Create a minimal valid SQLite database.
	createTestDB(t, dbPath)

	opts := &Options{
		DBPath: dbPath,
	}
	opts.applyDefaults()

	result := checkDatabase(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
	if result.Name != "database" {
		t.Errorf("expected name 'database', got %q", result.Name)
	}
}

func TestCheckDatabase_NotFound(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		DBPath: filepath.Join(t.TempDir(), "nonexistent.db"),
	}
	opts.applyDefaults()

	result := checkDatabase(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckDatabase_NoDB(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		DBPath: "",
	}
	opts.applyDefaults()

	result := checkDatabase(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail for empty path, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckJSONL_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	ledgerDir := t.TempDir()
	for _, f := range canonicalJSONLFiles {
		path := filepath.Join(ledgerDir, f)
		content := `{"event":"test","ts":"2026-01-01T00:00:00Z"}` + "\n"
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{LedgerDir: ledgerDir}
	opts.applyDefaults()

	result := checkJSONL(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckJSONL_MissingFiles(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	ledgerDir := t.TempDir()
	// Only create 2 of 4 files.
	for _, f := range canonicalJSONLFiles[:2] {
		path := filepath.Join(ledgerDir, f)
		if err := os.WriteFile(path, []byte("{}\n"), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{LedgerDir: ledgerDir}
	opts.applyDefaults()

	result := checkJSONL(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckJSONL_InvalidJSON(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	ledgerDir := t.TempDir()
	for i, f := range canonicalJSONLFiles {
		path := filepath.Join(ledgerDir, f)
		content := `{"valid":"json"}` + "\n"
		if i == 0 {
			content = "not valid json\n"
		}
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{LedgerDir: ledgerDir}
	opts.applyDefaults()

	result := checkJSONL(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckJSONL_EmptyDir(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{LedgerDir: ""}
	opts.applyDefaults()

	result := checkJSONL(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail for empty ledger dir, got %s", result.Status)
	}
}

func TestCheckCRDT_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	for _, dir := range []string{"db", "ledger", "logs", "runtime", "sentinels"} {
		if err := os.MkdirAll(filepath.Join(stateDir, dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{StateDir: stateDir}
	opts.applyDefaults()

	result := checkCRDT(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckCRDT_MissingDirs(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	// Only create some of the required directories.
	os.MkdirAll(filepath.Join(stateDir, "db"), 0o755)

	opts := &Options{StateDir: stateDir}
	opts.applyDefaults()

	result := checkCRDT(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPython_Available(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		LookPath: fakeLookPath("python3"),
	}
	opts.applyDefaults()

	result := checkPython(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPython_Missing(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		LookPath: fakeLookPath(), // No executables available.
	}
	opts.applyDefaults()

	result := checkPython(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckHooks_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	scriptPath := filepath.Join(hooksDir, "test-hook.sh")
	if err := os.WriteFile(scriptPath, []byte("#!/bin/bash\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkHooks(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckHooks_NoDir(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{ProjectDir: t.TempDir()}
	opts.applyDefaults()

	result := checkHooks(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckHooks_NonExecutable(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a non-executable script.
	scriptPath := filepath.Join(hooksDir, "test-hook.sh")
	if err := os.WriteFile(scriptPath, []byte("#!/bin/bash\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkHooks(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn for non-executable script, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckClaude_Available(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		LookPath: fakeLookPath("claude"),
	}
	opts.applyDefaults()

	result := checkClaude(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckClaude_Missing(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		LookPath: fakeLookPath(), // No executables.
	}
	opts.applyDefaults()

	result := checkClaude(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckAuth_Authenticated(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		ExecCommand: fakeExecCommand("Logged in as user@example.com\n", nil),
	}
	opts.applyDefaults()

	result := checkAuth(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckAuth_NotAuthenticated(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		ExecCommand: fakeExecCommand("Not authenticated", nil),
	}
	opts.applyDefaults()

	result := checkAuth(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckAuth_CommandFailed(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		ExecCommand: fakeExecCommand("", fmt.Errorf("command not found")),
	}
	opts.applyDefaults()

	result := checkAuth(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckConfig_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	data, _ := json.MarshalIndent(map[string]string{"key": "value"}, "", "  ")
	if err := os.WriteFile(filepath.Join(configDir, "test.json"), data, 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkConfig(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckConfig_InvalidJSON(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, ".codeflow", "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	if err := os.WriteFile(filepath.Join(configDir, "bad.json"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkConfig(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckConfig_NoDir(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{ProjectDir: t.TempDir()}
	opts.applyDefaults()

	result := checkConfig(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckEmbedding(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{}
	opts.applyDefaults()

	result := checkEmbedding(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckVector(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{}
	opts.applyDefaults()

	result := checkVector(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPermissions_Pass(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	stateDir := filepath.Join(projectDir, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir, StateDir: stateDir}
	opts.applyDefaults()

	result := checkPermissions(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPermissions_NonExecutableHooks(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	stateDir := filepath.Join(projectDir, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create hook scripts without executable permission.
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(hooksDir, "test.sh"), []byte("#!/bin/bash\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir, StateDir: stateDir}
	opts.applyDefaults()

	result := checkPermissions(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail for non-executable hooks, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPermissions_StateDirFallback(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	// Create .state under project dir so the fallback path works.
	stateDir := filepath.Join(projectDir, ".state")
	if err := os.MkdirAll(stateDir, 0o755); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir, StateDir: ""} // Empty StateDir triggers fallback.
	opts.applyDefaults()

	result := checkPermissions(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass with fallback StateDir, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckCRDT_StateDirFallback(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	stateDir := filepath.Join(projectDir, ".state")
	for _, dir := range []string{"db", "ledger", "logs", "runtime", "sentinels"} {
		if err := os.MkdirAll(filepath.Join(stateDir, dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{ProjectDir: projectDir, StateDir: ""} // Empty triggers fallback.
	opts.applyDefaults()

	result := checkCRDT(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass with fallback StateDir, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckHooks_EmptySubdirs(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	// Directory exists but no subdirectories.

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkHooks(ctx, opts)
	if result.Status != StatusFail {
		t.Errorf("expected fail for empty hooks directory, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckConfig_MixedFiles(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	configDir := filepath.Join(projectDir, ".codeflow", "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}

	// Create a valid JSON file.
	if err := os.WriteFile(filepath.Join(configDir, "good.json"), []byte(`{"key":"value"}`), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create a non-JSON file (should be ignored).
	if err := os.WriteFile(filepath.Join(configDir, "readme.txt"), []byte("not json"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{ProjectDir: projectDir}
	opts.applyDefaults()

	result := checkConfig(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass (non-json files ignored), got %s: %s", result.Status, result.Message)
	}
}

func TestCheckVersion(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{}
	opts.applyDefaults()

	result := checkVersion(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckNetwork_Online(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		ExecCommand: fakeExecCommand("github.com has address 140.82.121.4\n", nil),
	}
	opts.applyDefaults()

	result := checkNetwork(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckNetwork_Offline(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		ExecCommand: fakeExecCommand("", fmt.Errorf("host: network unreachable")),
	}
	opts.applyDefaults()

	result := checkNetwork(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn, got %s: %s", result.Status, result.Message)
	}
}

func TestRunAll(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	projectDir := t.TempDir()
	setupFullFixture(t, projectDir)

	opts := &Options{
		DBPath:     filepath.Join(projectDir, ".state", "db", "test.db"),
		LedgerDir:  filepath.Join(projectDir, ".state", "ledger"),
		ProjectDir: projectDir,
		StateDir:   filepath.Join(projectDir, ".state"),
		LookPath:   fakeLookPath("python3", "claude"),
		ExecCommand: func(name string, args ...string) ([]byte, error) {
			if name == "claude" {
				return []byte("Logged in"), nil
			}
			if name == "host" {
				return []byte("github.com has address\n"), nil
			}
			return []byte(""), nil
		},
	}

	// Create a valid DB.
	createTestDB(t, opts.DBPath)

	results := RunAll(ctx, opts)
	if len(results) != 16 {
		t.Fatalf("expected 16 results, got %d", len(results))
	}

	// Verify results are in the canonical order.
	for i, name := range checkNames {
		if results[i].Name != name {
			t.Errorf("result[%d] expected name %q, got %q", i, name, results[i].Name)
		}
	}

	// All checks should have non-empty messages and valid status.
	for _, r := range results {
		if r.Message == "" {
			t.Errorf("check %q returned empty message", r.Name)
		}
		if r.Status != StatusPass && r.Status != StatusFail && r.Status != StatusWarn {
			t.Errorf("check %q returned invalid status %q", r.Name, r.Status)
		}
	}
}

func TestRunCheck_KnownCheck(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		LookPath: fakeLookPath("python3"),
	}

	result, err := RunCheck(ctx, "python", opts)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if result.Name != "python" {
		t.Errorf("expected name 'python', got %q", result.Name)
	}
	if result.Status != StatusPass {
		t.Errorf("expected pass, got %s", result.Status)
	}
}

func TestRunCheck_UnknownCheck(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	_, err := RunCheck(ctx, "nonexistent", nil)
	if err == nil {
		t.Fatal("expected error for unknown check name")
	}
}

// ---------------------------------------------------------------------------
// pathflow-stuck tests
// ---------------------------------------------------------------------------

func TestCheckPathflowStuck_NotActive(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	// No pathflow-active flag exists.
	opts := &Options{
		StateDir:  stateDir,
		SessionID: "ses-test-session",
	}
	opts.applyDefaults()

	result := checkPathflowStuck(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass when pathflow not active, got %s: %s", result.Status, result.Message)
	}
	if result.Name != "pathflow-stuck" {
		t.Errorf("expected name 'pathflow-stuck', got %q", result.Name)
	}
}

func TestCheckPathflowStuck_StuckPhase(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	sessionID := "ses-test-stuck"

	// Create pathflow-active flag.
	flagDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-events.jsonl with a phase_transition 45 minutes ago.
	logsDir := filepath.Join(stateDir, "logs")
	if err := os.MkdirAll(logsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	stuckTime := time.Now().Add(-45 * time.Minute).Format(time.RFC3339)
	event := fmt.Sprintf(`{"event_type":"phase_transition","phase":"PF4-EXECUTE","timestamp":"%s"}`, stuckTime)
	if err := os.WriteFile(filepath.Join(logsDir, "pathflow-events.jsonl"), []byte(event+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		StateDir:  stateDir,
		SessionID: sessionID,
	}
	opts.applyDefaults()

	result := checkPathflowStuck(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn for stuck phase, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPathflowStuck_RecentPhase(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	sessionID := "ses-test-recent"

	// Create pathflow-active flag.
	flagDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create pathflow-events.jsonl with a phase_transition 5 minutes ago.
	logsDir := filepath.Join(stateDir, "logs")
	if err := os.MkdirAll(logsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	recentTime := time.Now().Add(-5 * time.Minute).Format(time.RFC3339)
	event := fmt.Sprintf(`{"event_type":"phase_transition","phase":"PF3-CLASSIFY","timestamp":"%s"}`, recentTime)
	if err := os.WriteFile(filepath.Join(logsDir, "pathflow-events.jsonl"), []byte(event+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		StateDir:  stateDir,
		SessionID: sessionID,
	}
	opts.applyDefaults()

	result := checkPathflowStuck(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass for recent phase, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckPathflowStuck_EmptySessionID(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		StateDir:  t.TempDir(),
		SessionID: "",
	}
	opts.applyDefaults()

	result := checkPathflowStuck(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass for empty session ID, got %s: %s", result.Status, result.Message)
	}
}

// ---------------------------------------------------------------------------
// team-health tests
// ---------------------------------------------------------------------------

func TestCheckTeamHealth_NoTeam(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	fakeHome := t.TempDir()
	// No team config anywhere — empty home dir prevents fallback discovery.
	opts := &Options{
		StateDir:  stateDir,
		SessionID: "ses-no-team",
		HomeDir:   fakeHome,
	}
	opts.applyDefaults()

	result := checkTeamHealth(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass when no team, got %s: %s", result.Status, result.Message)
	}
	if result.Name != "team-health" {
		t.Errorf("expected name 'team-health', got %q", result.Name)
	}
}

func TestCheckTeamHealth_TmuxNotAvailable(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	fakeHome := t.TempDir()
	sessionID := "ses-tmux-fail"

	// Create session-scoped team config so a team is discovered.
	teamDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamJSON := `{"team_name":"test-team","lead_pid":"12345","codeflow_session_id":"ses-tmux-fail"}`
	if err := os.WriteFile(filepath.Join(teamDir, "pathflow-team.json"), []byte(teamJSON), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create team config under fake home.
	teamConfigDir := filepath.Join(fakeHome, ".claude", "teams", "test-team")
	if err := os.MkdirAll(teamConfigDir, 0o755); err != nil {
		t.Fatal(err)
	}
	configData := `{"members":[{"name":"cf-development","tmuxPaneId":"%42"}]}`
	if err := os.WriteFile(filepath.Join(teamConfigDir, "config.json"), []byte(configData), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		StateDir:    stateDir,
		SessionID:   sessionID,
		HomeDir:     fakeHome,
		ExecCommand: fakeExecCommand("", fmt.Errorf("tmux: no server running")),
	}
	opts.applyDefaults()

	result := checkTeamHealth(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn when tmux unavailable, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckTeamHealth_AllPanesAlive(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	fakeHome := t.TempDir()
	sessionID := "ses-alive"

	// Create session-scoped team config.
	teamDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(teamDir, 0o755); err != nil {
		t.Fatal(err)
	}
	teamJSON := `{"team_name":"alive-team","lead_pid":"12345","codeflow_session_id":"ses-alive"}`
	if err := os.WriteFile(filepath.Join(teamDir, "pathflow-team.json"), []byte(teamJSON), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create team config under fake home.
	teamConfigDir := filepath.Join(fakeHome, ".claude", "teams", "alive-team")
	if err := os.MkdirAll(teamConfigDir, 0o755); err != nil {
		t.Fatal(err)
	}
	configData := `{"members":[{"name":"cf-dev","tmuxPaneId":"%42"},{"name":"cf-git","tmuxPaneId":"%43"}]}`
	if err := os.WriteFile(filepath.Join(teamConfigDir, "config.json"), []byte(configData), 0o644); err != nil {
		t.Fatal(err)
	}

	// tmux output includes both pane IDs.
	opts := &Options{
		StateDir:    stateDir,
		SessionID:   sessionID,
		HomeDir:     fakeHome,
		ExecCommand: fakeExecCommand("session:0.0: [200x50] %42 (active)\nsession:0.1: [200x50] %43\n", nil),
	}
	opts.applyDefaults()

	result := checkTeamHealth(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass when all panes alive, got %s: %s", result.Status, result.Message)
	}
}

// ---------------------------------------------------------------------------
// sentinel-drift tests
// ---------------------------------------------------------------------------

func TestCheckSentinelDrift_NoSession(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	opts := &Options{
		StateDir:  t.TempDir(),
		SessionID: "",
	}
	opts.applyDefaults()

	result := checkSentinelDrift(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass for no session, got %s: %s", result.Status, result.Message)
	}
	if result.Name != "sentinel-drift" {
		t.Errorf("expected name 'sentinel-drift', got %q", result.Name)
	}
}

func TestCheckSentinelDrift_DriftDetected(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	sessionID := "ses-drift"

	// Create pathflow-active flag.
	flagDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create events showing PF4-EXECUTE phase.
	logsDir := filepath.Join(stateDir, "logs")
	if err := os.MkdirAll(logsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	recentTime := time.Now().Add(-2 * time.Minute).Format(time.RFC3339)
	event := fmt.Sprintf(`{"event_type":"phase_transition","phase":"PF4-EXECUTE","timestamp":"%s"}`, recentTime)
	if err := os.WriteFile(filepath.Join(logsDir, "pathflow-events.jsonl"), []byte(event+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create sentinel directory but only with pf-1 (missing pf-2, pf-3).
	sentinelDir := filepath.Join(stateDir, "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sentinelDir, "pathflow-pf-1"), []byte(""), 0o644); err != nil {
		t.Fatal(err)
	}

	opts := &Options{
		StateDir:  stateDir,
		SessionID: sessionID,
	}
	opts.applyDefaults()

	result := checkSentinelDrift(ctx, opts)
	if result.Status != StatusWarn {
		t.Errorf("expected warn for drift, got %s: %s", result.Status, result.Message)
	}
}

func TestCheckSentinelDrift_AllPresent(t *testing.T) {
	t.Parallel()
	ctx := t.Context()

	stateDir := t.TempDir()
	sessionID := "ses-ok"

	// Create pathflow-active flag.
	flagDir := filepath.Join(stateDir, "session", sessionID, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create events showing PF3-CLASSIFY phase.
	logsDir := filepath.Join(stateDir, "logs")
	if err := os.MkdirAll(logsDir, 0o755); err != nil {
		t.Fatal(err)
	}
	recentTime := time.Now().Add(-1 * time.Minute).Format(time.RFC3339)
	event := fmt.Sprintf(`{"event_type":"phase_transition","phase":"PF3-CLASSIFY","timestamp":"%s"}`, recentTime)
	if err := os.WriteFile(filepath.Join(logsDir, "pathflow-events.jsonl"), []byte(event+"\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// Create all expected sentinels for PF3-CLASSIFY.
	sentinelDir := filepath.Join(stateDir, "sentinels", "pathflow", sessionID)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, s := range []string{"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"} {
		if err := os.WriteFile(filepath.Join(sentinelDir, s), []byte(""), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	opts := &Options{
		StateDir:  stateDir,
		SessionID: sessionID,
	}
	opts.applyDefaults()

	result := checkSentinelDrift(ctx, opts)
	if result.Status != StatusPass {
		t.Errorf("expected pass when all sentinels present, got %s: %s", result.Status, result.Message)
	}
}

// createTestDB creates a minimal valid SQLite database using the db package.
func createTestDB(t *testing.T, path string) {
	t.Helper()

	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatalf("creating db directory: %v", err)
	}

	// Import the db package to create a proper database.
	d, err := newTestDB(path)
	if err != nil {
		t.Fatalf("creating test database: %v", err)
	}
	d.Close()
}

// newTestDB creates a proper SQLite database using the db package.
func newTestDB(path string) (interface{ Close() error }, error) {
	return db.NewDB(path)
}

// setupFullFixture creates a complete project fixture for RunAll testing.
func setupFullFixture(t *testing.T, projectDir string) {
	t.Helper()

	// Create .state/ structure.
	for _, dir := range []string{"db", "ledger", "logs", "runtime", "sentinels"} {
		if err := os.MkdirAll(filepath.Join(projectDir, ".state", dir), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	// Create JSONL files.
	for _, f := range canonicalJSONLFiles {
		path := filepath.Join(projectDir, ".state", "ledger", f)
		content := `{"event":"test","ts":"2026-01-01T00:00:00Z"}` + "\n"
		if err := os.WriteFile(path, []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}

	// Create hooks directory with executable script.
	hooksDir := filepath.Join(projectDir, ".claude", "hooks", "codeflow", "pre-tool-use")
	if err := os.MkdirAll(hooksDir, 0o755); err != nil {
		t.Fatal(err)
	}
	scriptPath := filepath.Join(hooksDir, "test.sh")
	if err := os.WriteFile(scriptPath, []byte("#!/bin/bash\n"), 0o755); err != nil {
		t.Fatal(err)
	}

	// Create config directory with valid JSON.
	configDir := filepath.Join(projectDir, ".codeflow", "config", "pathflow")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatal(err)
	}
	configData, _ := json.MarshalIndent(map[string]string{"key": "value"}, "", "  ")
	if err := os.WriteFile(filepath.Join(configDir, "test.json"), configData, 0o644); err != nil {
		t.Fatal(err)
	}
}
