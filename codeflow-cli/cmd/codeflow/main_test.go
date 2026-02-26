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

func TestVersionVariable(t *testing.T) {
	t.Parallel()
	if version == "" {
		t.Fatal("version should have a default value")
	}
	if version != "dev" {
		t.Errorf("expected default version to be %q, got %q", "dev", version)
	}
}

func TestNewRootCmd(t *testing.T) {
	t.Parallel()

	cmd := newRootCmd()
	if cmd.Use != "codeflow" {
		t.Errorf("expected root command Use=%q, got %q", "codeflow", cmd.Use)
	}
	if cmd.Version != version {
		t.Errorf("expected root command Version=%q, got %q", version, cmd.Version)
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
		t.Errorf("expected 'unknown command' in error, got: %v", err)
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
		t.Errorf("expected 'not yet implemented' in output, got: %q", got)
	}
	if !strings.Contains(got, version) {
		t.Errorf("expected version string in output, got: %q", got)
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
		t.Errorf("expected 'not found' in output, got: %q", got)
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
		t.Errorf("expected 'removed' in output, got: %q", got)
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
		t.Errorf("expected 'removed' in output, got: %q", got)
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
		t.Errorf("expected 'removed' in output, got: %q", buf.String())
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
		t.Errorf("expected 'cancelled' in output, got: %q", got)
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
		t.Errorf("expected 'reading confirmation' in error, got: %v", err)
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
		t.Errorf("expected 'removing' in error, got: %v", err)
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
		t.Errorf("expected prompt to mention config path, got: %q", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("expected '[y/N]' in prompt, got: %q", got)
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
		t.Errorf("expected binary-only prompt, got: %q", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("expected '[y/N]' in prompt, got: %q", got)
	}
}

func TestDefaultBinaryPath(t *testing.T) {
	t.Parallel()

	path := defaultBinaryPath()
	if path == "" {
		t.Fatal("defaultBinaryPath should return non-empty string")
	}
	if !strings.HasSuffix(path, filepath.Join(".local", "bin", "codeflow")) {
		t.Errorf("expected path to end with .local/bin/codeflow, got: %s", path)
	}
}

func TestDefaultConfigDir(t *testing.T) {
	t.Parallel()

	path := defaultConfigDir()
	if path == "" {
		t.Fatal("defaultConfigDir should return non-empty string")
	}
	if !strings.HasSuffix(path, filepath.Join(".config", "codeflow")) {
		t.Errorf("expected path to end with .config/codeflow, got: %s", path)
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
		t.Errorf("expected 'not found' or 'removed' in output, got: %q", got)
	}
}

func TestNewVersionCmd(t *testing.T) {
	t.Parallel()

	cmd := newVersionCmd()
	if cmd.Use != "version" {
		t.Errorf("expected Use=%q, got %q", "version", cmd.Use)
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
		t.Errorf("expected prompt message in output, got: %q", got)
	}
	if !strings.Contains(got, "[y/N]") {
		t.Errorf("expected '[y/N]' in prompt, got: %q", got)
	}
}
