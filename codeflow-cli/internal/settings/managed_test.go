package settings

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"
)

// mockSudoOps replaces the package-level sudoOps for testing.
// It restores the original ops when cleanup runs.
// Since tests that inject mocks cannot run in parallel with each other
// (they modify package state), they use a shared mutex.
var sudoMu sync.Mutex

func withMockSudo(t *testing.T, read func(string) ([]byte, error), copy_ func(string, string) error, mkdir func(string) error, chmod func(string, string) error) {
	t.Helper()
	sudoMu.Lock()

	orig := sudoOps
	sudoOps.readFile = read
	sudoOps.copyFile = copy_
	sudoOps.mkdirAll = mkdir
	sudoOps.chmod = chmod

	t.Cleanup(func() {
		sudoOps = orig
		sudoMu.Unlock()
	})
}

func TestManagedSettingsDir(t *testing.T) {
	t.Parallel()

	dir, err := ManagedSettingsDir()
	if err != nil {
		t.Fatalf("ManagedSettingsDir() error: %v", err)
	}

	switch runtime.GOOS {
	case "darwin":
		if dir != "/Library/Application Support/ClaudeCode" {
			t.Errorf("ManagedSettingsDir() = %q, want /Library/Application Support/ClaudeCode", dir)
		}
	case "linux":
		if dir != "/etc/claude-code" {
			t.Errorf("ManagedSettingsDir() = %q, want /etc/claude-code", dir)
		}
	}
}

func TestSourceFilePath(t *testing.T) {
	t.Parallel()

	got := SourceFilePath("/home/user/project")
	want := filepath.Join("/home/user/project", ".codeflow", "docs", "security", "claude-enterprise", ManagedSettingsFile)
	if got != want {
		t.Errorf("SourceFilePath() = %q, want %q", got, want)
	}
}

func TestSetupManagedSettings_SourceNotFound(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") },
		func(string, string) error { return nil },
		func(string) error { return nil },
		func(string, string) error { return nil },
	)

	_, err := SetupManagedSettings("/nonexistent/source.json", false)
	if err == nil {
		t.Error("expected error for missing source file")
	}
	if !strings.Contains(err.Error(), "source file not found") {
		t.Errorf("error = %v, want source file not found", err)
	}
}

func TestSetupManagedSettings_DryRun(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") },
		func(string, string) error { return nil },
		func(string) error { return nil },
		func(string, string) error { return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{"test": true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := SetupManagedSettings(sourceFile, true)
	if err != nil {
		t.Fatalf("SetupManagedSettings(dryRun=true) error: %v", err)
	}

	if result.Installed {
		t.Error("dry run should not install")
	}
	if result.SourcePath != sourceFile {
		t.Errorf("SourcePath = %q, want %q", result.SourcePath, sourceFile)
	}
}

func TestSetupManagedSettings_FreshInstall(t *testing.T) {
	t.Parallel()
	var copiedSrc, copiedDst string
	var mkdirCalled, chmodCalled bool

	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") }, // no existing file
		func(src, dst string) error { copiedSrc = src; copiedDst = dst; return nil },
		func(string) error { mkdirCalled = true; return nil },
		func(string, string) error { chmodCalled = true; return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{"test": true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := SetupManagedSettings(sourceFile, false)
	if err != nil {
		t.Fatalf("SetupManagedSettings() error: %v", err)
	}

	if !result.Installed {
		t.Error("should be installed")
	}
	if result.BackedUp {
		t.Error("fresh install should not backup")
	}
	if result.WasExisting {
		t.Error("should not detect existing file")
	}
	if copiedSrc != sourceFile {
		t.Errorf("copied from %q, want %q", copiedSrc, sourceFile)
	}
	if copiedDst == "" {
		t.Error("copy destination should not be empty")
	}
	if !mkdirCalled {
		t.Error("mkdir should have been called")
	}
	if !chmodCalled {
		t.Error("chmod should have been called")
	}
}

func TestSetupManagedSettings_ExistingWithBackup(t *testing.T) {
	t.Parallel()
	copyCount := 0

	withMockSudo(t,
		func(string) ([]byte, error) { return []byte(`{"existing": true}`), nil }, // existing file
		func(string, string) error { copyCount++; return nil },
		func(string) error { return nil },
		func(string, string) error { return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{"test": true}`), 0o644); err != nil {
		t.Fatal(err)
	}

	result, err := SetupManagedSettings(sourceFile, false)
	if err != nil {
		t.Fatalf("SetupManagedSettings() error: %v", err)
	}

	if !result.Installed {
		t.Error("should be installed")
	}
	if !result.BackedUp {
		t.Error("should have backed up existing file")
	}
	if !result.WasExisting {
		t.Error("should detect existing file")
	}
	if result.BackupPath == "" {
		t.Error("backup path should not be empty")
	}
	// Two copies: one for backup, one for install.
	if copyCount != 2 {
		t.Errorf("expected 2 copy calls (backup + install), got %d", copyCount)
	}
}

func TestSetupManagedSettings_BackupError(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return []byte(`{"existing": true}`), nil },
		func(string, string) error { return fmt.Errorf("permission denied") },
		func(string) error { return nil },
		func(string, string) error { return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := SetupManagedSettings(sourceFile, false)
	if err == nil {
		t.Error("expected error when backup fails")
	}
	if !strings.Contains(err.Error(), "backup existing file") {
		t.Errorf("error = %v, want 'backup existing file'", err)
	}
}

func TestSetupManagedSettings_MkdirError(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") },
		func(string, string) error { return nil },
		func(string) error { return fmt.Errorf("permission denied") },
		func(string, string) error { return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := SetupManagedSettings(sourceFile, false)
	if err == nil {
		t.Error("expected error when mkdir fails")
	}
	if !strings.Contains(err.Error(), "create settings directory") {
		t.Errorf("error = %v, want 'create settings directory'", err)
	}
}

func TestSetupManagedSettings_CopyError(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") },
		func(string, string) error { return fmt.Errorf("disk full") },
		func(string) error { return nil },
		func(string, string) error { return nil },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := SetupManagedSettings(sourceFile, false)
	if err == nil {
		t.Error("expected error when copy fails")
	}
	if !strings.Contains(err.Error(), "install settings file") {
		t.Errorf("error = %v, want 'install settings file'", err)
	}
}

func TestSetupManagedSettings_ChmodError(t *testing.T) {
	t.Parallel()
	withMockSudo(t,
		func(string) ([]byte, error) { return nil, fmt.Errorf("not found") },
		func(string, string) error { return nil },
		func(string) error { return nil },
		func(string, string) error { return fmt.Errorf("permission denied") },
	)

	dir := t.TempDir()
	sourceFile := filepath.Join(dir, "managed-settings.json")
	if err := os.WriteFile(sourceFile, []byte(`{}`), 0o644); err != nil {
		t.Fatal(err)
	}

	_, err := SetupManagedSettings(sourceFile, false)
	if err == nil {
		t.Error("expected error when chmod fails")
	}
	if !strings.Contains(err.Error(), "set permissions") {
		t.Errorf("error = %v, want 'set permissions'", err)
	}
}

func TestFormatSetupOutput(t *testing.T) {
	t.Parallel()

	t.Run("dry run", func(t *testing.T) {
		t.Parallel()
		result := &SetupResult{
			Installed:   false,
			SourcePath:  "/src/managed-settings.json",
			TargetPath:  "/Library/Application Support/ClaudeCode/managed-settings.json",
			WasExisting: false,
		}

		output := FormatSetupOutput(result)
		if !strings.Contains(output, "Dry run complete") {
			t.Error("output should mention dry run")
		}
		if !strings.Contains(output, "Source:") {
			t.Error("output should show source path")
		}
	})

	t.Run("installed with backup", func(t *testing.T) {
		t.Parallel()
		result := &SetupResult{
			Installed:   true,
			BackedUp:    true,
			BackupPath:  "/Library/Application Support/ClaudeCode/managed-settings.json.bak.20260301",
			SourcePath:  "/src/managed-settings.json",
			TargetPath:  "/Library/Application Support/ClaudeCode/managed-settings.json",
			WasExisting: true,
		}

		output := FormatSetupOutput(result)
		if !strings.Contains(output, "Installation successful") {
			t.Error("output should mention installation success")
		}
		if !strings.Contains(output, "Backup:") {
			t.Error("output should mention backup")
		}
		if !strings.Contains(output, "Restart Claude Code") {
			t.Error("output should mention restart")
		}
	})

	t.Run("installed fresh", func(t *testing.T) {
		t.Parallel()
		result := &SetupResult{
			Installed:   true,
			SourcePath:  "/src/managed-settings.json",
			TargetPath:  "/target/managed-settings.json",
			WasExisting: false,
		}

		output := FormatSetupOutput(result)
		if !strings.Contains(output, "Installation successful") {
			t.Error("output should mention installation success")
		}
		if strings.Contains(output, "Backup:") {
			t.Error("output should NOT mention backup for fresh install")
		}
	})
}
