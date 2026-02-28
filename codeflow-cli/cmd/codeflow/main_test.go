package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

// setupPathFlowFixtures creates a temporary directory tree for PathFlow state
// testing. Returns the temp dir root path. The caller must chdir into the
// returned path before calling functions that use relative ".state/" paths.
func setupPathFlowFixtures(t *testing.T, sid string) string {
	t.Helper()
	root := t.TempDir()

	// Create pathflow-active flag.
	flagDir := filepath.Join(root, ".state", "session", sid, "pathflow")
	if err := os.MkdirAll(flagDir, 0o755); err != nil {
		t.Fatalf("creating flag dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(flagDir, "is-pathflow-active"), []byte("1"), 0o644); err != nil {
		t.Fatalf("creating flag file: %v", err)
	}

	// Create sentinel directory.
	sentinelDir := filepath.Join(root, ".state", "sentinels", "pathflow", sid)
	if err := os.MkdirAll(sentinelDir, 0o755); err != nil {
		t.Fatalf("creating sentinel dir: %v", err)
	}

	// Create logs directory.
	logsDir := filepath.Join(root, ".state", "logs")
	if err := os.MkdirAll(logsDir, 0o755); err != nil {
		t.Fatalf("creating logs dir: %v", err)
	}

	return root
}

func TestVersionVariable(t *testing.T) {
	t.Parallel()
	if version == "" {
		t.Fatal("version should have a default value")
	}
	if version != "dev" {
		t.Errorf("version = %q, want %q", version, "dev")
	}
}

func TestNewRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	if cmd.Use != "codeflow" {
		t.Errorf("Use = %q, want %q", cmd.Use, "codeflow")
	}
	if cmd.Version != version {
		t.Errorf("Version = %q, want %q", cmd.Version, version)
	}

	// Verify subcommands are registered.
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	for _, name := range []string{"version", "uninstall"} {
		if !subcommands[name] {
			t.Errorf("expected subcommand %q to be registered", name)
		}
	}
}

func TestRootCmd_VersionFlag(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"--version"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("--version returned error: %v", err)
	}

	got := buf.String()
	expected := "codeflow " + version + "\n"
	if got != expected {
		t.Errorf("--version output: got %q, want %q", got, expected)
	}
}

func TestRootCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("--help returned error: %v", err)
	}

	got := buf.String()

	// Verify help contains key sections.
	for _, want := range []string{"codeflow", "Available Commands:", "version", "uninstall", "Flags:"} {
		if !strings.Contains(got, want) {
			t.Errorf("help output missing %q:\n%s", want, got)
		}
	}
}

func TestRootCmd_NoArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("no-args returned error: %v", err)
	}

	// No args should show help.
	got := buf.String()
	if !strings.Contains(got, "Available Commands:") {
		t.Errorf("no-args output should show help, got:\n%s", got)
	}
}

func TestRootCmd_UnknownCommand(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var errBuf bytes.Buffer
	cmd.SetErr(&errBuf)
	cmd.SetArgs([]string{"nonexistent"})

	err := cmd.Execute()
	if err == nil {
		t.Fatal("expected error for unknown command, got nil")
	}
	if !strings.Contains(err.Error(), "unknown command") {
		t.Errorf("error = %v, want it to contain %q", err, "unknown command")
	}
}

func TestVersionCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"version"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version command returned error: %v", err)
	}

	expected := "codeflow " + version + "\n"
	if buf.String() != expected {
		t.Errorf("version output: got %q, want %q", buf.String(), expected)
	}
}

func TestVersionCmd_ExtraArgs(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	cmd.SetArgs([]string{"version", "extra"})

	err := cmd.Execute()
	if err == nil {
		t.Error("expected error for version with extra args")
	}
}

func TestVersionCmd_JSONOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"version", "--json"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version --json returned error: %v", err)
	}

	var info versionInfo
	if err := json.Unmarshal(buf.Bytes(), &info); err != nil {
		t.Fatalf("failed to parse JSON output: %v\nraw: %s", err, buf.String())
	}

	if info.CodeflowVersion != version {
		t.Errorf("codeflow_version = %q, want %q", info.CodeflowVersion, version)
	}
	if info.GoVersion != runtime.Version() {
		t.Errorf("go_version = %q, want %q", info.GoVersion, runtime.Version())
	}
	// Claude Code version may be "unknown" in test env -- just verify the field exists.
	if info.ClaudeCodeVersion == "" {
		t.Error("claude_code_version should not be empty")
	}
}

func TestVersionCmd_CheckFlag(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"version", "--check"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("version --check returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "not yet implemented") {
		t.Errorf("output missing %q: %q", "not yet implemented", got)
	}
	if !strings.Contains(got, version) {
		t.Errorf("output missing version %q: %q", version, got)
	}
}

func TestVersionCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newVersionCmd()

	jsonFlag := cmd.Flags().Lookup("json")
	if jsonFlag == nil {
		t.Fatal("version command should have --json flag")
	}
	if jsonFlag.DefValue != "false" {
		t.Errorf("--json default should be 'false', got %q", jsonFlag.DefValue)
	}

	checkFlag := cmd.Flags().Lookup("check")
	if checkFlag == nil {
		t.Fatal("version command should have --check flag")
	}
	if checkFlag.DefValue != "false" {
		t.Errorf("--check default should be 'false', got %q", checkFlag.DefValue)
	}
}

func TestDetectClaudeCodeVersionWith_Success(t *testing.T) {
	t.Parallel()

	fakeRunner := func() ([]byte, error) {
		return []byte("1.0.18 (Claude Code)\n"), nil
	}

	got := detectClaudeCodeVersionWith(fakeRunner)
	if got != "1.0.18 (Claude Code)" {
		t.Errorf("detectClaudeCodeVersionWith = %q, want %q", got, "1.0.18 (Claude Code)")
	}
}

func TestDetectClaudeCodeVersionWith_NotFound(t *testing.T) {
	t.Parallel()

	fakeRunner := func() ([]byte, error) {
		return nil, &os.PathError{Op: "exec", Path: "claude", Err: os.ErrNotExist}
	}

	got := detectClaudeCodeVersionWith(fakeRunner)
	if got != "unknown" {
		t.Errorf("detectClaudeCodeVersionWith = %q, want %q", got, "unknown")
	}
}

func TestDetectClaudeCodeVersionWith_EmptyOutput(t *testing.T) {
	t.Parallel()

	fakeRunner := func() ([]byte, error) {
		return []byte("  \n"), nil
	}

	got := detectClaudeCodeVersionWith(fakeRunner)
	if got != "unknown" {
		t.Errorf("detectClaudeCodeVersionWith = %q, want %q", got, "unknown")
	}
}

// ---- Uninstall tests ----

func TestRunUninstall_BinaryNotFound(t *testing.T) {
	t.Parallel()

	var buf bytes.Buffer
	tmpDir := t.TempDir()
	nonexistent := filepath.Join(tmpDir, "codeflow")
	configDir := filepath.Join(tmpDir, "config")

	err := runUninstall(strings.NewReader(""), &buf, nonexistent, configDir, true, false)
	if err != nil {
		t.Fatalf("runUninstall returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "not found") {
		t.Errorf("output missing %q: %q", "not found", got)
	}
}

func TestRunUninstall_ForceRemovesFile(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader(""), &buf, fakeBinary, configDir, true, false)
	if err != nil {
		t.Fatalf("runUninstall --force returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "removed") {
		t.Errorf("output missing %q: %q", "removed", got)
	}

	// Verify file was actually removed.
	if _, err := os.Stat(fakeBinary); !os.IsNotExist(err) {
		t.Error("binary should have been removed")
	}
}

func TestRunUninstall_ConfirmYes(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader("y\n"), &buf, fakeBinary, configDir, false, false)
	if err != nil {
		t.Fatalf("runUninstall with 'y' returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "removed") {
		t.Errorf("output missing %q: %q", "removed", got)
	}

	// Verify file was actually removed.
	if _, err := os.Stat(fakeBinary); !os.IsNotExist(err) {
		t.Error("binary should have been removed after 'y' confirmation")
	}
}

func TestRunUninstall_ConfirmYesFull(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader("yes\n"), &buf, fakeBinary, configDir, false, false)
	if err != nil {
		t.Fatalf("runUninstall with 'yes' returned error: %v", err)
	}

	if !strings.Contains(buf.String(), "removed") {
		t.Errorf("output missing %q: %q", "removed", buf.String())
	}
}

func TestRunUninstall_ConfirmNo(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader("n\n"), &buf, fakeBinary, configDir, false, false)
	if err != nil {
		t.Fatalf("runUninstall with 'n' returned error: %v", err)
	}

	got := buf.String()
	if !strings.Contains(got, "cancelled") {
		t.Errorf("output missing %q: %q", "cancelled", got)
	}

	// Verify file was NOT removed.
	if _, err := os.Stat(fakeBinary); err != nil {
		t.Error("binary should NOT have been removed after 'n'")
	}
}

func TestRunUninstall_EmptyInput(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	// Empty reader simulates EOF / no input -- scanner.Scan() returns false with nil error.
	err := runUninstall(strings.NewReader(""), &buf, fakeBinary, configDir, false, false)

	// With empty input, scanner.Scan returns false and scanner.Err is nil,
	// so fmt.Errorf wraps a nil error.
	if err == nil {
		t.Fatal("expected error for empty input, got nil")
	}
	if !strings.Contains(err.Error(), "reading confirmation") {
		t.Errorf("error = %v, want it to contain %q", err, "reading confirmation")
	}
}

func TestRunUninstall_RemovePermissionError(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	subDir := filepath.Join(tmpDir, "bin")
	if err := os.MkdirAll(subDir, 0o755); err != nil {
		t.Fatalf("failed to create subdir: %v", err)
	}
	fakeBinary := filepath.Join(subDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	// Make parent directory read-only so os.Remove fails.
	if err := os.Chmod(subDir, 0o555); err != nil {
		t.Fatalf("failed to chmod: %v", err)
	}
	t.Cleanup(func() { os.Chmod(subDir, 0o755) })

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader(""), &buf, fakeBinary, configDir, true, false)
	if err == nil {
		t.Fatal("expected error when binary cannot be removed")
	}
	if !strings.Contains(err.Error(), "removing") {
		t.Errorf("error = %v, want it to contain %q", err, "removing")
	}
}

func TestRunUninstall_ForceRemovesConfig(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "settings.json"), []byte("{}"), 0o644); err != nil {
		t.Fatalf("failed to create config file: %v", err)
	}

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader(""), &buf, fakeBinary, configDir, true, false)
	if err != nil {
		t.Fatalf("runUninstall --force returned error: %v", err)
	}

	// Verify both binary and config were removed.
	if _, err := os.Stat(fakeBinary); !os.IsNotExist(err) {
		t.Error("binary should have been removed")
	}
	if _, err := os.Stat(configDir); !os.IsNotExist(err) {
		t.Error("config directory should have been removed")
	}
}

func TestRunUninstall_KeepConfigPreservesConfig(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}
	configFile := filepath.Join(configDir, "settings.json")
	if err := os.WriteFile(configFile, []byte("{}"), 0o644); err != nil {
		t.Fatalf("failed to create config file: %v", err)
	}

	var buf bytes.Buffer
	err := runUninstall(strings.NewReader(""), &buf, fakeBinary, configDir, true, true)
	if err != nil {
		t.Fatalf("runUninstall --force --keep-config returned error: %v", err)
	}

	// Verify binary was removed.
	if _, err := os.Stat(fakeBinary); !os.IsNotExist(err) {
		t.Error("binary should have been removed")
	}
	// Verify config was preserved.
	if _, err := os.Stat(configDir); err != nil {
		t.Error("config directory should have been preserved with --keep-config")
	}
	if _, err := os.Stat(configFile); err != nil {
		t.Error("config file should have been preserved with --keep-config")
	}
}

func TestRunUninstall_PromptIncludesConfigPath(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("failed to create config dir: %v", err)
	}

	var buf bytes.Buffer
	// Decline with "n" to test the prompt message content.
	_ = runUninstall(strings.NewReader("n\n"), &buf, fakeBinary, configDir, false, false)

	got := buf.String()
	if !strings.Contains(got, "config") {
		t.Errorf("output missing config path reference: %q", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("output missing %q: %q", "[y/N]", got)
	}
}

func TestRunUninstall_KeepConfigPromptExcludesConfigPath(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	_ = runUninstall(strings.NewReader("n\n"), &buf, fakeBinary, configDir, false, true)

	got := buf.String()
	// When --keep-config is set, the prompt should only mention the binary.
	if !strings.Contains(got, "Remove codeflow binary at") {
		t.Errorf("output missing %q: %q", "Remove codeflow binary at", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("output missing %q: %q", "[y/N]", got)
	}
}

func TestDefaultBinaryPath(t *testing.T) {
	t.Parallel()

	path := defaultBinaryPath()
	if path == "" {
		t.Fatal("defaultBinaryPath should return non-empty string")
	}
	if !strings.HasSuffix(path, filepath.Join(".local", "bin", "codeflow")) {
		t.Errorf("defaultBinaryPath() = %s, want suffix .local/bin/codeflow", path)
	}
}

func TestDefaultConfigDir(t *testing.T) {
	t.Parallel()

	path := defaultConfigDir()
	if path == "" {
		t.Fatal("defaultConfigDir should return non-empty string")
	}
	if !strings.HasSuffix(path, filepath.Join(".config", "codeflow")) {
		t.Errorf("defaultConfigDir() = %s, want suffix .config/codeflow", path)
	}
}

func TestRun_ContextPropagation(t *testing.T) {
	t.Parallel()

	ctx := t.Context()
	// run() should work with a valid context (shows help by default).
	err := run(ctx)
	if err != nil {
		t.Fatalf("run with valid context returned error: %v", err)
	}
}

func TestNewUninstallCmd_Flags(t *testing.T) {
	t.Parallel()

	cmd := newUninstallCmd()

	forceFlag := cmd.Flags().Lookup("force")
	if forceFlag == nil {
		t.Fatal("uninstall command should have --force flag")
	}
	if forceFlag.Shorthand != "f" {
		t.Errorf("--force shorthand should be 'f', got %q", forceFlag.Shorthand)
	}
	if forceFlag.DefValue != "false" {
		t.Errorf("--force default should be 'false', got %q", forceFlag.DefValue)
	}

	keepConfigFlag := cmd.Flags().Lookup("keep-config")
	if keepConfigFlag == nil {
		t.Fatal("uninstall command should have --keep-config flag")
	}
	if keepConfigFlag.DefValue != "false" {
		t.Errorf("--keep-config default should be 'false', got %q", keepConfigFlag.DefValue)
	}
}

func TestUninstallCmd_HelpOutput(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"uninstall", "--help"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("uninstall --help returned error: %v", err)
	}

	got := buf.String()
	for _, want := range []string{"Remove the codeflow binary", "--force", "-f", "--keep-config"} {
		if !strings.Contains(got, want) {
			t.Errorf("uninstall help missing %q:\n%s", want, got)
		}
	}
}

func TestUninstallCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	// Test uninstall through the root command with --force on a nonexistent path.
	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"uninstall", "--force"})

	err := cmd.Execute()
	if err != nil {
		t.Fatalf("uninstall --force via root cmd returned error: %v", err)
	}

	// Binary likely doesn't exist at default path in test env.
	got := buf.String()
	if !strings.Contains(got, "not found") && !strings.Contains(got, "removed") {
		t.Errorf("output missing 'not found' or 'removed': %q", got)
	}
}

func TestNewVersionCmd(t *testing.T) {
	t.Parallel()

	cmd := newVersionCmd()
	if cmd.Use != "version" {
		t.Errorf("Use = %q, want %q", cmd.Use, "version")
	}
	if cmd.Short == "" {
		t.Error("version command should have a short description")
	}
}

func TestRunUninstall_PromptMessage(t *testing.T) {
	t.Parallel()

	tmpDir := t.TempDir()
	fakeBinary := filepath.Join(tmpDir, "codeflow")
	if err := os.WriteFile(fakeBinary, []byte("binary"), 0o755); err != nil {
		t.Fatalf("failed to create fake binary: %v", err)
	}
	configDir := filepath.Join(tmpDir, "config")

	var buf bytes.Buffer
	// Decline with "n" to test the prompt message content.
	_ = runUninstall(strings.NewReader("n\n"), &buf, fakeBinary, configDir, false, true)

	got := buf.String()
	if !strings.Contains(got, "Remove codeflow binary at") {
		t.Errorf("output missing %q: %q", "Remove codeflow binary at", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("output missing %q: %q", "[y/N]", got)
	}
}

// ---- Welcome command tests ----

func TestNewWelcomeCmd(t *testing.T) {
	t.Parallel()

	cmd := newWelcomeCmd()
	if cmd.Use != "welcome" {
		t.Errorf("Use = %q, want %q", cmd.Use, "welcome")
	}

	quietFlag := cmd.Flags().Lookup("quiet")
	if quietFlag == nil {
		t.Fatal("welcome command should have --quiet flag")
	}
	if quietFlag.Shorthand != "q" {
		t.Errorf("--quiet shorthand should be 'q', got %q", quietFlag.Shorthand)
	}
}

func TestWelcomeCmd_ViaRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	subcommands := make(map[string]bool)
	for _, sub := range cmd.Commands() {
		subcommands[sub.Use] = true
	}
	if !subcommands["welcome"] {
		t.Error("expected 'welcome' subcommand to be registered in root cmd")
	}
}

func TestDetectPhase_NoSentinels(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	root := t.TempDir()
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	got := detectPhase("ses-test-nosent")
	if got != "PF1-INIT" {
		t.Errorf("detectPhase with no sentinels = %q, want %q", got, "PF1-INIT")
	}
}

func TestDetectPhase_WithSentinels(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	sid := "ses-test-phase"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	// Create sentinels pf-1, pf-2, pf-3.
	sentDir := filepath.Join(root, ".state", "sentinels", "pathflow", sid)
	for _, n := range []string{"pathflow-pf-1", "pathflow-pf-2", "pathflow-pf-3"} {
		if err := os.WriteFile(filepath.Join(sentDir, n), []byte(""), 0o644); err != nil {
			t.Fatalf("creating sentinel: %v", err)
		}
	}

	got := detectPhase(sid)
	if got != "PF4-EXECUTE" {
		t.Errorf("detectPhase with pf-1,2,3 = %q, want %q", got, "PF4-EXECUTE")
	}
}

func TestDetectPhase_AllCompleted(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	sid := "ses-test-allphase"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	sentDir := filepath.Join(root, ".state", "sentinels", "pathflow", sid)
	for i := 1; i <= 7; i++ {
		name := filepath.Join(sentDir, "pathflow-pf-"+strings.Repeat("", 0)+string(rune('0'+i)))
		if err := os.WriteFile(name, []byte(""), 0o644); err != nil {
			t.Fatalf("creating sentinel: %v", err)
		}
	}

	got := detectPhase(sid)
	// With all 7 sentinels, nextPhase=8 which has no name, falls back to phaseNames[7].
	if got != "PF7-END" {
		t.Errorf("detectPhase with all sentinels = %q, want %q", got, "PF7-END")
	}
}

func TestDetectStage_NoSentinels(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	root := t.TempDir()
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	got := detectStage("ses-test-nostage")
	if got != "" {
		t.Errorf("detectStage with no sentinel dir = %q, want empty", got)
	}
}

func TestDetectStage_WithDevCompleted(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	sid := "ses-test-stage"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	sentDir := filepath.Join(root, ".state", "sentinels", "pathflow", sid)
	if err := os.WriteFile(filepath.Join(sentDir, "pathflow-ws-dev"), []byte(""), 0o644); err != nil {
		t.Fatalf("creating sentinel: %v", err)
	}

	got := detectStage(sid)
	// ws-dev done, next in order is ws-plan.
	if got != "WS-PLAN" {
		t.Errorf("detectStage with ws-dev = %q, want %q", got, "WS-PLAN")
	}
}

func TestDetectReworkCount_NoFile(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	root := t.TempDir()
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	got := detectReworkCount("ses-test-norework")
	if got != "0" {
		t.Errorf("detectReworkCount with no JSONL = %q, want %q", got, "0")
	}
}

func TestDetectReworkCount_WithRework(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	sid := "ses-test-rework"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	// Write JSONL with rework events.
	events := []string{
		`{"event":"stage_transition","stage":"WS-REV","status":"complete","verdict":"changes_requested","iteration":1,"session_id":"` + sid + `"}`,
		`{"event":"stage_transition","stage":"WS-REV","status":"complete","verdict":"approved","iteration":1,"session_id":"` + sid + `"}`,
		`{"event":"stage_transition","stage":"WS-REV","status":"complete","verdict":"changes_requested","iteration":2,"session_id":"` + sid + `"}`,
		`{"event":"stage_transition","stage":"WS-REV","status":"complete","verdict":"changes_requested","iteration":1,"session_id":"other-session"}`,
	}
	eventsPath := filepath.Join(root, ".state", "logs", "pathflow-events.jsonl")
	if err := os.WriteFile(eventsPath, []byte(strings.Join(events, "\n")+"\n"), 0o644); err != nil {
		t.Fatalf("writing events: %v", err)
	}

	got := detectReworkCount(sid)
	if got != "2" {
		t.Errorf("detectReworkCount = %q, want %q", got, "2")
	}
}

func TestDetectReworkCount_ReworkIterationField(t *testing.T) {
	// NOTE: no t.Parallel() -- uses os.Chdir which is global state
	sid := "ses-test-reworkfield"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	// Write JSONL with rework_iteration field (alternate naming).
	events := []string{
		`{"event":"stage_transition","stage":"WS-REV","status":"complete","verdict":"changes_requested","rework_iteration":3,"session_id":"` + sid + `"}`,
	}
	eventsPath := filepath.Join(root, ".state", "logs", "pathflow-events.jsonl")
	if err := os.WriteFile(eventsPath, []byte(strings.Join(events, "\n")+"\n"), 0o644); err != nil {
		t.Fatalf("writing events: %v", err)
	}

	got := detectReworkCount(sid)
	if got != "3" {
		t.Errorf("detectReworkCount with rework_iteration = %q, want %q", got, "3")
	}
}

func TestReadPathFlowState_NoSession(t *testing.T) {
	// NOTE: no t.Parallel() -- t.Setenv is not compatible with parallel tests
	t.Setenv("CODEFLOW_SESSION_ID", "")

	got := readPathFlowState()
	if got != nil {
		t.Error("readPathFlowState should return nil when CODEFLOW_SESSION_ID is empty")
	}
}

func TestReadPathFlowState_NoFlag(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	root := t.TempDir()
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	t.Setenv("CODEFLOW_SESSION_ID", "ses-noflag")

	got := readPathFlowState()
	if got != nil {
		t.Error("readPathFlowState should return nil when pathflow-active flag missing")
	}
}

func TestReadPathFlowState_WithFlag(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	sid := "ses-test-withflag"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	t.Setenv("CODEFLOW_SESSION_ID", sid)

	got := readPathFlowState()
	if got == nil {
		t.Fatal("readPathFlowState should return non-nil when flag exists")
	}
}

func TestReadTeamState_NoSession(t *testing.T) {
	// NOTE: no t.Parallel() -- t.Setenv is not compatible with parallel tests
	t.Setenv("CODEFLOW_SESSION_ID", "")

	got := readTeamState()
	if got != nil {
		t.Error("readTeamState should return nil when CODEFLOW_SESSION_ID is empty")
	}
}

func TestReadTeamState_WithTeamJSON(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	sid := "ses-test-team"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	t.Setenv("CODEFLOW_SESSION_ID", sid)

	// Write pathflow-team.json.
	teamJSON := `{"team_name":"my-team","lead_pid":12345,"codeflow_session_id":"` + sid + `"}`
	teamPath := filepath.Join(root, ".state", "session", sid, "pathflow", "pathflow-team.json")
	if err := os.WriteFile(teamPath, []byte(teamJSON), 0o644); err != nil {
		t.Fatalf("writing team JSON: %v", err)
	}

	got := readTeamState()
	if got == nil {
		t.Fatal("readTeamState should return non-nil when team JSON exists")
	}
}

func TestReadTeamState_NoTeamJSON(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	sid := "ses-test-noteam"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })

	t.Setenv("CODEFLOW_SESSION_ID", sid)

	// No team JSON file -- should return nil (fallback removed since it
	// depends on ~/.claude/teams which may be stale).
	got := readTeamState()
	if got != nil {
		t.Error("readTeamState should return nil when no team JSON exists")
	}
}

func TestReadTeammates_InvalidHome(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv which is not parallel-safe
	// readTeammates returns nil when home dir lookup fails or config not found.
	t.Setenv("HOME", "/nonexistent-home-for-test")

	got := readTeammates("nonexistent-team")
	if got != nil {
		t.Errorf("readTeammates with bad HOME = %v, want nil", got)
	}
}

func TestReadTeammates_InvalidJSON(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv which is not parallel-safe
	tmpDir := t.TempDir()
	t.Setenv("HOME", tmpDir)

	// Create config file with invalid JSON.
	configDir := filepath.Join(tmpDir, ".claude", "teams", "test-team")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}
	if err := os.WriteFile(filepath.Join(configDir, "config.json"),
		[]byte("not-json"), 0o644); err != nil {
		t.Fatalf("writing config: %v", err)
	}

	got := readTeammates("test-team")
	if got != nil {
		t.Errorf("readTeammates with invalid JSON = %v, want nil", got)
	}
}

func TestReadTeammates_ValidConfig(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv which is not parallel-safe
	tmpDir := t.TempDir()
	t.Setenv("HOME", tmpDir)

	configDir := filepath.Join(tmpDir, ".claude", "teams", "test-team")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	configJSON := `{"teammates":{"id1":{"name":"cf-dev"},"id2":{"name":"cf-review"},"id3":{"name":""}}}`
	if err := os.WriteFile(filepath.Join(configDir, "config.json"),
		[]byte(configJSON), 0o644); err != nil {
		t.Fatalf("writing config: %v", err)
	}

	got := readTeammates("test-team")
	if len(got) != 2 {
		t.Fatalf("readTeammates len = %d, want 2", len(got))
	}
	// Should be sorted.
	if got[0] != "cf-dev" || got[1] != "cf-review" {
		t.Errorf("readTeammates = %v, want [cf-dev cf-review]", got)
	}
}

func TestReadTeammates_EmptyTeammates(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv which is not parallel-safe
	tmpDir := t.TempDir()
	t.Setenv("HOME", tmpDir)

	configDir := filepath.Join(tmpDir, ".claude", "teams", "test-team")
	if err := os.MkdirAll(configDir, 0o755); err != nil {
		t.Fatalf("creating dir: %v", err)
	}

	configJSON := `{"teammates":{}}`
	if err := os.WriteFile(filepath.Join(configDir, "config.json"),
		[]byte(configJSON), 0o644); err != nil {
		t.Fatalf("writing config: %v", err)
	}

	got := readTeammates("test-team")
	if len(got) != 0 {
		t.Errorf("readTeammates with empty teammates = %v, want empty", got)
	}
}

func TestRunWelcome_Basic(t *testing.T) {
	// NOTE: no t.Parallel -- t.Setenv incompatible with parallel tests.
	t.Setenv("CODEFLOW_SESSION_ID", "")

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"welcome"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("welcome returned error: %v", err)
	}

	got := buf.String()
	if len(got) == 0 {
		t.Error("expected non-empty welcome output")
	}
}

func TestRunWelcome_Quiet(t *testing.T) {
	// NOTE: no t.Parallel -- t.Setenv incompatible with parallel tests.
	t.Setenv("CODEFLOW_SESSION_ID", "")

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"welcome", "--quiet"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("welcome --quiet returned error: %v", err)
	}

	got := buf.String()
	if len(got) != 0 {
		t.Errorf("welcome --quiet should produce no output, got: %q", got)
	}
}

func TestRunWelcome_WithPathFlow(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	sid := "ses-test-welcome-pf"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })
	t.Setenv("CODEFLOW_SESSION_ID", sid)

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"welcome"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("welcome with pathflow returned error: %v", err)
	}

	got := buf.String()
	if len(got) == 0 {
		t.Error("expected non-empty welcome output with PathFlow state")
	}
}

func TestRunWelcome_WithTeam(t *testing.T) {
	// NOTE: no t.Parallel() -- uses t.Setenv and os.Chdir which are not parallel-safe
	sid := "ses-test-welcome-team"
	root := setupPathFlowFixtures(t, sid)
	oldWd, _ := os.Getwd()
	if err := os.Chdir(root); err != nil {
		t.Fatalf("chdir: %v", err)
	}
	t.Cleanup(func() { os.Chdir(oldWd) })
	t.Setenv("CODEFLOW_SESSION_ID", sid)

	// Write team JSON so readTeamState returns non-nil.
	teamJSON := `{"team_name":"test-welcome-team"}`
	teamPath := filepath.Join(root, ".state", "session", sid, "pathflow", "pathflow-team.json")
	if err := os.WriteFile(teamPath, []byte(teamJSON), 0o644); err != nil {
		t.Fatalf("writing team JSON: %v", err)
	}

	cmd := newRootCmd()
	var buf bytes.Buffer
	cmd.SetOut(&buf)
	cmd.SetArgs([]string{"welcome"})

	if err := cmd.Execute(); err != nil {
		t.Fatalf("welcome with team returned error: %v", err)
	}

	got := buf.String()
	if len(got) == 0 {
		t.Error("expected non-empty welcome output with team state")
	}
}
